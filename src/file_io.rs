//! Safe single-document file I/O operations for Notefart.
//!
//! Provides bounded, nonblocking file loading, format detection (BOM and LF/CRLF/Mixed),
//! metadata and content fingerprinting for optimistic concurrency control, and transactional,
//! atomic file replacement with mode, ownership, and extended attributes / ACL preservation.
//!
//! # Security and Concurrency Architecture
//!
//! 1. **Nonblocking file open on load**:
//!    Files are initially opened with `O_RDONLY | O_NONBLOCK | O_CLOEXEC | O_NOCTTY` before
//!    inspection. Non-regular filesystem objects (FIFOs/named pipes, character/block devices,
//!    sockets, directories) are immediately rejected via `fstat` without blocking or unbounded reads.
//! 2. **Bounded read**:
//!    File sizes and read streams are capped at 64 MiB (`MAX_LOAD_SIZE`). Binary files containing
//!    NUL bytes and invalid UTF-8 sequences are rejected before editing.
//! 3. **Format tracking**:
//!    UTF-8 BOM is detected and tracked. Pure LF and pure CRLF files are normalized to LF in memory
//!    and serialized back to their native line ending on save. Mixed line endings or lone CRs
//!    are opened as read-only to preserve the exact buffer bytes and prevent silent data corruption.
//! 4. **Symlink and Hard Link Refusal**:
//!    While reading through symlinks and loading hard-linked files is allowed, direct in-place
//!    saving to symlinks or files with `nlink > 1` is strictly refused (even with `overwrite = true`)
//!    to prevent accidental link clobbering, target divergence, or privilege escalation.
//!    Callers are directed to use "Save As".
//! 5. **Transactional Atomic Save**:
//!    Content is written to a temporary file created in the same parent directory with restrictive
//!    permissions (`0o600`). If replacing an existing target, mode bits, ownership (uid/gid),
//!    and extended attributes / POSIX ACLs are copied to the temporary file descriptor. If any
//!    critical metadata cannot be preserved, save is aborted and the temporary file is unlinked.
//! 6. **Pre-persist Verification and Conflict Checks**:
//!    Immediately before atomic replacement, the target path is re-verified via `symlink_metadata`
//!    and `fstat` on an open file descriptor (`O_NOFOLLOW`) to ensure baseline dev/inode, mtime,
//!    size, and BLAKE3 content hash have not diverged.
//!    The temporary *name* is re-verified against the descriptor that received the buffer as well:
//!    `persist` renames by pathname, so an entry recreated in the same shared directory would
//!    otherwise be installed in place of the buffer.
//! 7. **Durability and Post-Persist Sync**:
//!    The temporary file is flushed and fsynced before `persist`. Once atomically renamed into place,
//!    the destination must name the very inode that was written, and the parent directory is
//!    fsynced. If the directory fsync fails, `FileError::Durability` is reported so the caller
//!    retains the document dirty state despite the on-disk file replacement. A destination that
//!    names a different inode reports `FileError::Substituted` for the same reason.
//!
//! # Note on Concurrent Writer Race (POSIX Limitation)
//!
//! There is an unavoidable microscopic race window inherent to POSIX filesystems between our
//! final pre-persist re-check of `path` (which verifies it is not a symlink, not a hard link,
//! and matches the expected baseline) and the execution of the `renameat` syscall by the kernel.
//! Because POSIX lacks an atomic "replace only if target inode matches D and link count == 1"
//! primitive, an external process with write permissions to the parent directory could
//! theoretically swap the target path with a symlink or hard link inside this window.
//! We defend against TOCTOU up to the boundary of the `renameat` syscall using fd-based checks
//! with `O_NOFOLLOW`, and we use `RenameFlags::NOREPLACE` for newly created paths.

use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use rustix::fs::{FileType, Mode, OFlags};
use rustix::io::Errno;
use xattr::FileExt;

/// Maximum supported file size for loading and saving (64 MiB).
pub const MAX_LOAD_SIZE: usize = 64 * 1024 * 1024;

/// Line ending format detected or requested for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Newline {
    /// Unix line feed (`\n`).
    Lf,
    /// Windows carriage return + line feed (`\r\n`).
    Crlf,
    /// Mixed line endings or lone carriage returns (`\r`). Files with this format
    /// are opened as read-only.
    Mixed,
}

/// File encoding and formatting properties.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileFormat {
    /// Whether the file contains a UTF-8 Byte Order Mark (`0xEF, 0xBB, 0xBF`).
    pub bom: bool,
    /// Line ending convention used by the file.
    pub newline: Newline,
}

impl Default for FileFormat {
    fn default() -> Self {
        Self {
            bom: false,
            newline: Newline::Lf,
        }
    }
}

/// Opaque snapshot of on-disk file state used for optimistic concurrency control.
///
/// Stores file identity (device and inode), modification timestamps, permissions,
/// ownership, size, and BLAKE3 content hash. Callers cannot edit its internal fields.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiskState {
    dev: u64,
    ino: u64,
    mtime_sec: i64,
    mtime_nsec: i64,
    size: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    content_hash: [u8; 32],
}

impl DiskState {
    pub(crate) fn identity(&self) -> (u64, u64) {
        (self.dev, self.ino)
    }
}

/// Result of a successful file load operation.
#[derive(Debug, Clone)]
pub struct LoadedFile {
    /// In-memory document text, normalized to LF line endings when format is LF or CRLF.
    /// Preserved verbatim when format is Mixed.
    pub text: String,
    /// Detected file format (BOM and newline convention).
    pub format: FileFormat,
    /// If present, explains why the file was opened as read-only (e.g. mixed line endings).
    pub read_only: Option<String>,
    /// Baseline disk state captured upon opening the file, used for conflict checking on save.
    pub baseline: DiskState,
}

/// Errors that can occur during file loading or saving.
#[derive(Debug)]
pub enum FileError {
    /// Standard I/O error encountered during filesystem interaction.
    Io(io::Error),
    /// The file or requested operation is unsupported (e.g. non-regular file, invalid UTF-8,
    /// mixed line ending save, symlink/hardlink save refusal, metadata preservation failure).
    Unsupported(String),
    /// File on disk was modified, recreated, or deleted externally since the baseline was captured.
    Conflict,
    /// The target file already exists and overwrite was not explicitly specified.
    Exists,
    /// The file was successfully written and replaced on disk, but syncing the parent directory
    /// failed, leaving durability on persistent media uncertain.
    Durability(io::Error),
    /// The temporary file or the destination stopped naming the object that received the buffer.
    /// The document must stay dirty because the on-disk content may differ from this save.
    Substituted(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            Self::Conflict => write!(f, "file conflict: target was modified externally"),
            Self::Exists => write!(f, "target file already exists"),
            Self::Durability(err) => {
                write!(
                    f,
                    "durability uncertainty: file persisted but directory sync failed: {err}"
                )
            }
            Self::Substituted(msg) => write!(f, "integrity: {msg}"),
        }
    }
}

impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) | Self::Durability(err) => Some(err),
            Self::Unsupported(_) | Self::Conflict | Self::Exists | Self::Substituted(_) => None,
        }
    }
}

impl From<io::Error> for FileError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<Errno> for FileError {
    fn from(err: Errno) -> Self {
        Self::Io(err.into())
    }
}

/// Loads a document from the filesystem.
///
/// # Security and Validation
/// - Opens the path safely using nonblocking mode (`O_NONBLOCK | O_CLOEXEC | O_NOCTTY`)
///   to prevent hanging on FIFOs/named pipes or allocating controlling terminals.
/// - Validates via `fstat` that the target is a regular file; directories, FIFOs,
///   sockets, and device nodes are rejected.
/// - Symlinks are followed during load; neither symlinks nor hard links automatically force
///   read-only mode so the user can make local edits and use "Save As".
/// - Enforces a strict 64 MiB read cap.
/// - Rejects binary files containing NUL (`0x00`) bytes and non-UTF-8 sequences.
/// - Detects UTF-8 BOM and preserves line ending convention (LF, CRLF, or Mixed).
/// - Normalizes LF/CRLF to LF in memory. Lone CR or mixed endings result in `read_only = Some(...)`
///   with the original text preserved verbatim.
pub fn load(path: &Path) -> Result<LoadedFile, FileError> {
    // Open nonblocking safely first to reject FIFOs/devices without hanging.
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY,
        Mode::empty(),
    )?;

    let stat = rustix::fs::fstat(&fd)?;
    let raw_file_type = FileType::from_raw_mode(stat.st_mode as _);
    if !raw_file_type.is_file() {
        let desc = if raw_file_type.is_dir() {
            "directory"
        } else if raw_file_type.is_fifo() {
            "FIFO/named pipe"
        } else if raw_file_type.is_socket() {
            "socket"
        } else if raw_file_type.is_char_device() {
            "character device"
        } else if raw_file_type.is_block_device() {
            "block device"
        } else {
            "non-regular file"
        };
        return Err(FileError::Unsupported(format!(
            "cannot open {}: {desc} is not supported",
            path.display()
        )));
    }

    if (stat.st_size as u64) > MAX_LOAD_SIZE as u64 {
        return Err(FileError::Unsupported(format!(
            "file size ({} bytes) exceeds maximum supported limit of 64 MiB",
            stat.st_size
        )));
    }

    // Clear O_NONBLOCK now that we have verified the fd refers to a regular file
    if let Ok(flags) = rustix::fs::fcntl_getfl(&fd) {
        let _ = rustix::fs::fcntl_setfl(&fd, flags - OFlags::NONBLOCK);
    }

    // Read bounded content
    let mut file = File::from(fd);
    let initial_cap = usize::try_from(stat.st_size)
        .unwrap_or(0)
        .min(4 * 1024 * 1024);
    let mut bytes = Vec::with_capacity(initial_cap);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(FileError::Io(e)),
        };
        if bytes.len() + n > MAX_LOAD_SIZE {
            return Err(FileError::Unsupported(
                "file size exceeds maximum supported limit of 64 MiB".to_string(),
            ));
        }
        bytes.extend_from_slice(&chunk[..n]);
    }

    // Reject NUL bytes (binary files)
    if bytes.contains(&0) {
        return Err(FileError::Unsupported(
            "file contains NUL bytes (binary files are not supported)".to_string(),
        ));
    }

    // Check UTF-8 BOM
    let (bom, text_bytes) = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        (true, &bytes[3..])
    } else {
        (false, &bytes[..])
    };

    // Validate UTF-8
    let raw_text = std::str::from_utf8(text_bytes)
        .map_err(|e| FileError::Unsupported(format!("file is not valid UTF-8: {e}")))?;

    // Capture baseline DiskState fingerprint
    let content_hash = *blake3::hash(&bytes).as_bytes();
    let baseline = DiskState {
        dev: stat.st_dev as u64,
        ino: stat.st_ino as u64,
        mtime_sec: stat.st_mtime as i64,
        mtime_nsec: stat.st_mtime_nsec as i64,
        size: bytes.len() as u64,
        mode: stat.st_mode as u32,
        uid: stat.st_uid as u32,
        gid: stat.st_gid as u32,
        content_hash,
    };

    // Parse line endings and normalize
    let (format, text, read_only) = parse_line_endings_and_normalize(raw_text, bom);

    Ok(LoadedFile {
        text,
        format,
        read_only,
        baseline,
    })
}

/// Analyzes line endings in `raw_text`, normalizes LF/CRLF to LF, or marks mixed endings read-only.
fn parse_line_endings_and_normalize(
    raw_text: &str,
    bom: bool,
) -> (FileFormat, String, Option<String>) {
    let bytes = raw_text.as_bytes();
    let mut crlf_count = 0usize;
    let mut lone_cr_count = 0usize;
    let mut lf_count = 0usize;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\r' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
                crlf_count += 1;
                i += 2;
                continue;
            } else {
                lone_cr_count += 1;
            }
        } else if bytes[i] == b'\n' {
            lf_count += 1;
        }
        i += 1;
    }

    if lone_cr_count > 0 {
        // Lone CR line endings -> read-only, preserve original text verbatim
        (
            FileFormat {
                bom,
                newline: Newline::Mixed,
            },
            raw_text.to_string(),
            Some("file contains lone CR line endings (opened as read-only)".to_string()),
        )
    } else if crlf_count > 0 && lf_count > 0 {
        // Mixed CRLF and LF line endings -> read-only, preserve original text verbatim
        (
            FileFormat {
                bom,
                newline: Newline::Mixed,
            },
            raw_text.to_string(),
            Some("file contains mixed CRLF and LF line endings (opened as read-only)".to_string()),
        )
    } else if crlf_count > 0 {
        // Pure CRLF -> normalize to LF
        let normalized = raw_text.replace("\r\n", "\n");
        (
            FileFormat {
                bom,
                newline: Newline::Crlf,
            },
            normalized,
            None,
        )
    } else {
        // Pure LF or empty/no newlines -> default LF
        (
            FileFormat {
                bom,
                newline: Newline::Lf,
            },
            raw_text.to_string(),
            None,
        )
    }
}

/// Encodes in-memory document text according to the target file format.
///
/// Preserves BOM, trailing newline (or absence thereof), tabs, and non-NUL control bytes.
fn encode_text(text: &str, format: FileFormat) -> Result<Vec<u8>, FileError> {
    if format.newline == Newline::Mixed {
        return Err(FileError::Unsupported(
            "saving files with mixed or lone CR line endings is not supported".to_string(),
        ));
    }
    if text.contains('\0') {
        return Err(FileError::Unsupported(
            "cannot save text containing NUL bytes".to_string(),
        ));
    }

    let extra_capacity = if format.bom { 3 } else { 0 }
        + if format.newline == Newline::Crlf {
            text.len() / 20
        } else {
            0
        };
    let mut out = Vec::with_capacity(text.len() + extra_capacity);

    if format.bom {
        out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    }

    match format.newline {
        Newline::Lf => {
            out.extend_from_slice(text.as_bytes());
        }
        Newline::Crlf => {
            let mut prev_cr = false;
            for &b in text.as_bytes() {
                if b == b'\n' {
                    if !prev_cr {
                        out.push(b'\r');
                    }
                    out.push(b'\n');
                    prev_cr = false;
                } else {
                    if b == b'\r' {
                        prev_cr = true;
                    } else {
                        prev_cr = false;
                    }
                    out.push(b);
                }
            }
        }
        Newline::Mixed => unreachable!(),
    }

    if out.len() > MAX_LOAD_SIZE {
        return Err(FileError::Unsupported(
            "document size exceeds maximum supported limit of 64 MiB".to_string(),
        ));
    }

    Ok(out)
}

/// Atomically saves document text to the specified filesystem path.
///
/// # Concurrency and Conflict Semantics
/// - `expected: Some(baseline)`: Validates that the target file still matches the baseline
///   identity (dev/inode), permissions, ownership, timestamp, size, and BLAKE3 content hash.
///   If any attribute or content byte has changed, or if the file was deleted, `FileError::Conflict`
///   is returned unless `overwrite == true`.
/// - `expected: None`: If the target path already exists, `FileError::Exists` is returned
///   unless `overwrite == true`. New paths are persisted with no-clobber semantics so that
///   a racing file creation returns `FileError::Exists`.
///
/// # Security and Integrity
/// - Symlinks as the final path component and hard links (`nlink > 1`) are **strictly refused**
///   even when `overwrite == true`. Callers must use "Save As" to choose a regular target.
/// - Writes to a temporary file in the same directory (`.notefart_tmp_*`) with restrictive
///   permissions (`0o600`), flushes, and fsyncs before replacement.
/// - When replacing an existing file, preserves permission bits, ownership (uid/gid), and
///   extended attributes / POSIX ACLs. If any critical metadata cannot be preserved, the save
///   operation is aborted with an explanatory error, leaving the original target file intact.
/// - Fsyncs the parent directory after atomic replacement. If directory sync fails,
///   `FileError::Durability` is returned to keep the document marked dirty in the caller.
pub fn save(
    path: &Path,
    text: &str,
    format: FileFormat,
    expected: Option<&DiskState>,
    overwrite: bool,
) -> Result<DiskState, FileError> {
    if path.as_os_str().is_empty() {
        return Err(FileError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            "file path cannot be empty",
        )));
    }

    let encoded_bytes = encode_text(text, format)?;
    let content_hash = *blake3::hash(&encoded_bytes).as_bytes();

    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    // Verify parent directory exists and is a directory
    let parent_meta = std::fs::metadata(parent)?;
    if !parent_meta.is_dir() {
        return Err(FileError::Io(io::Error::new(
            io::ErrorKind::NotADirectory,
            "parent is not a directory",
        )));
    }

    // Inspect target path status
    let target_exists = match std::fs::symlink_metadata(path) {
        Ok(sym_meta) => {
            // Refuse symlink target even with overwrite: true
            if sym_meta.file_type().is_symlink() {
                return Err(FileError::Unsupported(
                    "cannot save to a symlink; use Save As to choose a regular file target"
                        .to_string(),
                ));
            }
            if !sym_meta.is_file() {
                return Err(FileError::Unsupported(
                    "cannot save to a non-regular file".to_string(),
                ));
            }
            // Refuse hard-linked file even with overwrite: true
            if sym_meta.nlink() > 1 {
                return Err(FileError::Unsupported(format!(
                    "cannot save to a hard-linked file (link count {}); use Save As to choose a new target",
                    sym_meta.nlink(),
                )));
            }

            if expected.is_none() && !overwrite {
                return Err(FileError::Exists);
            }

            if let Some(expected_state) = expected {
                if !overwrite {
                    check_conflict(path, expected_state)?;
                }
            }

            true
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if expected.is_some() && !overwrite {
                return Err(FileError::Conflict);
            }
            false
        }
        Err(err) => return Err(FileError::Io(err)),
    };

    // Create temporary file in same directory with restrictive permissions (0o600)
    let mut temp_builder = tempfile::Builder::new();
    let mut temp = temp_builder
        .prefix(".notefart_tmp_")
        .permissions(std::fs::Permissions::from_mode(0o600))
        .tempfile_in(parent)?;

    // Write encoded bytes to temporary file and fsync data
    temp.as_file_mut().write_all(&encoded_bytes)?;
    temp.as_file_mut().flush()?;

    // If target exists, preserve mode, ownership, and extended attributes / ACL
    let mut copied_metadata = if target_exists {
        preserve_metadata(path, &temp, overwrite)?
    } else {
        None
    };

    // Pre-persist re-check: defend against symlink / hardlink TOCTOU substitution and concurrent changes
    match std::fs::symlink_metadata(path) {
        Ok(sym_meta) => {
            if sym_meta.file_type().is_symlink() {
                return Err(FileError::Unsupported(
                    "cannot save to a symlink; target was replaced with a symlink before persist"
                        .to_string(),
                ));
            }
            if !sym_meta.is_file() {
                return Err(FileError::Unsupported(
                    "cannot save to a non-regular file; target changed before persist".to_string(),
                ));
            }
            if sym_meta.nlink() > 1 {
                return Err(FileError::Unsupported(format!(
                    "cannot save to a hard-linked file (link count {}); target was linked before persist",
                    sym_meta.nlink()
                )));
            }
            if !target_exists {
                if !overwrite {
                    return Err(if expected.is_some() {
                        FileError::Conflict
                    } else {
                        FileError::Exists
                    });
                }
                copied_metadata = preserve_metadata(path, &temp, true)?;
            } else if let Some(expected_state) = expected {
                if !overwrite {
                    check_conflict(path, expected_state)?;
                }
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            if expected.is_some() && !overwrite {
                return Err(FileError::Conflict);
            }
        }
        Err(err) => return Err(FileError::Io(err)),
    }

    // Metadata/ACL/mode updates must reach disk along with the payload.
    temp.as_file().sync_all()?;
    if let Some(stamp) = &copied_metadata {
        validate_security_metadata(path, stamp)?;
    }

    // Persist renames the temporary *name*; the descriptor that received the buffer proves
    // nothing about the entry sitting under that name. A shared parent directory lets another
    // process unlink and recreate it, so refuse to rename before confirming the binding.
    if !names_same_object(temp.path(), &rustix::fs::fstat(temp.as_file())?)? {
        return Err(FileError::Substituted(
            "temporary file was replaced before the atomic replace; buffer not saved".to_string(),
        ));
    }

    // Atomic persist
    let persisted_file = if !target_exists && !overwrite {
        // Persist with no-clobber semantics: a racing creation returns FileError::Exists
        match temp.persist_noclobber(path) {
            Ok(f) => f,
            Err(e) => {
                if e.error.kind() == io::ErrorKind::AlreadyExists {
                    return Err(FileError::Exists);
                }
                return Err(FileError::Io(e.error));
            }
        }
    } else {
        match temp.persist(path) {
            Ok(f) => f,
            Err(e) => return Err(FileError::Io(e.error)),
        }
    };

    // The rename itself is unconditional, so the entry installed may still have been swapped in
    // after the binding check. The destination must name the very inode this save wrote, or the
    // caller would record a checkpoint for content that is not on disk.
    let final_stat = rustix::fs::fstat(&persisted_file)?;
    if !names_same_object(path, &final_stat)? {
        return Err(FileError::Substituted(
            "destination does not name the saved file; verify the file on disk before retrying"
                .to_string(),
        ));
    }

    // Post-persist durability: fsync parent directory via fd
    let sync_dir_res = (|| -> io::Result<()> {
        let dir_fd = rustix::fs::open(
            parent,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        rustix::fs::fsync(&dir_fd)?;
        Ok(())
    })();

    if let Err(err) = sync_dir_res {
        return Err(FileError::Durability(err));
    }

    Ok(DiskState {
        dev: final_stat.st_dev as u64,
        ino: final_stat.st_ino as u64,
        mtime_sec: final_stat.st_mtime as i64,
        mtime_nsec: final_stat.st_mtime_nsec as i64,
        size: encoded_bytes.len() as u64,
        mode: final_stat.st_mode as u32,
        uid: final_stat.st_uid as u32,
        gid: final_stat.st_gid as u32,
        content_hash,
    })
}

/// Checks whether an on-disk target file matches the expected baseline snapshot.
fn check_conflict(path: &Path, expected: &DiskState) -> Result<(), FileError> {
    let fd = match rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NOCTTY,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Err(FileError::Conflict),
        Err(Errno::LOOP) => {
            return Err(FileError::Unsupported(
                "cannot save to a symlink; use Save As to choose a regular file target".to_string(),
            ));
        }
        Err(e) => return Err(FileError::Io(e.into())),
    };

    let stat = rustix::fs::fstat(&fd)?;
    if !FileType::from_raw_mode(stat.st_mode as _).is_file() {
        return Err(FileError::Unsupported(
            "cannot save to a non-regular file".to_string(),
        ));
    }
    if stat.st_nlink > 1 {
        return Err(FileError::Unsupported(format!(
            "cannot save to a hard-linked file (link count {}); use Save As to choose a new target",
            stat.st_nlink
        )));
    }
    if (stat.st_dev as u64) != expected.dev || (stat.st_ino as u64) != expected.ino {
        return Err(FileError::Conflict);
    }
    if (stat.st_mode as u32) != expected.mode
        || (stat.st_uid as u32) != expected.uid
        || (stat.st_gid as u32) != expected.gid
    {
        return Err(FileError::Conflict);
    }

    // Check modification time
    let mtime_sec = stat.st_mtime as i64;
    let mtime_nsec = stat.st_mtime_nsec as i64;
    if mtime_sec != expected.mtime_sec || mtime_nsec != expected.mtime_nsec {
        return Err(FileError::Conflict);
    }

    // Check size
    if (stat.st_size as u64) != expected.size {
        return Err(FileError::Conflict);
    }

    if let Ok(flags) = rustix::fs::fcntl_getfl(&fd) {
        let _ = rustix::fs::fcntl_setfl(&fd, flags - OFlags::NONBLOCK);
    }

    // Check content hash streaming through BLAKE3 (bounded by expected.size)
    let mut file = File::from(fd);
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total_read: u64 = 0;
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(FileError::Io(e)),
        };
        total_read = total_read.saturating_add(n as u64);
        if total_read > expected.size {
            return Err(FileError::Conflict);
        }
        hasher.update(&buf[..n]);
    }
    let hash = *hasher.finalize().as_bytes();
    if hash != expected.content_hash {
        return Err(FileError::Conflict);
    }

    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
struct SecurityStamp {
    dev: u64,
    ino: u64,
    mode: u32,
    uid: u32,
    gid: u32,
    ctime: i64,
    ctime_nsec: i64,
}

impl From<&std::fs::Metadata> for SecurityStamp {
    fn from(metadata: &std::fs::Metadata) -> Self {
        Self {
            dev: metadata.dev(),
            ino: metadata.ino(),
            mode: metadata.mode(),
            uid: metadata.uid(),
            gid: metadata.gid(),
            ctime: metadata.ctime(),
            ctime_nsec: metadata.ctime_nsec(),
        }
    }
}

fn validate_security_metadata(path: &Path, copied: &SecurityStamp) -> Result<(), FileError> {
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NOCTTY,
        Mode::empty(),
    )
    .map_err(|error| match error {
        Errno::NOENT | Errno::LOOP => FileError::Conflict,
        _ => FileError::Io(error.into()),
    })?;
    let metadata = File::from(fd).metadata()?;
    if !metadata.is_file() || metadata.nlink() != 1 || SecurityStamp::from(&metadata) != *copied {
        return Err(FileError::Conflict);
    }
    Ok(())
}

/// Returns whether the directory entry at `path` names exactly the object `expected` describes:
/// a regular file with one link, opened without following a symlink.
///
/// Atomic replacement renames a *name*, so the open descriptor alone cannot prove what will be
/// installed or what the destination ended up naming.
fn names_same_object(path: &Path, expected: &rustix::fs::Stat) -> Result<bool, FileError> {
    let fd = match rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NOCTTY,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT | Errno::LOOP) => return Ok(false),
        Err(error) => return Err(FileError::Io(error.into())),
    };
    let metadata = File::from(fd).metadata()?;
    Ok(metadata.is_file()
        && metadata.nlink() == 1
        && metadata.dev() == expected.st_dev as u64
        && metadata.ino() == expected.st_ino as u64)
}

/// Preserves permissions (mode), ownership (uid/gid), and extended attributes / POSIX ACLs
/// from the existing target file to the temporary file descriptor.
fn preserve_metadata(
    path: &Path,
    temp: &tempfile::NamedTempFile,
    overwrite: bool,
) -> Result<Option<SecurityStamp>, FileError> {
    let target_fd = match rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NOCTTY,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => {
            if overwrite {
                return Ok(None);
            }
            return Err(FileError::Conflict);
        }
        Err(Errno::LOOP) => {
            return Err(FileError::Unsupported(
                "cannot save to a symlink; target was replaced with a symlink".to_string(),
            ));
        }
        Err(e) => return Err(FileError::Io(e.into())),
    };

    let target_file = File::from(target_fd);
    let stat = rustix::fs::fstat(&target_file)?;
    let copied = SecurityStamp {
        dev: stat.st_dev as u64,
        ino: stat.st_ino as u64,
        mode: stat.st_mode as u32,
        uid: stat.st_uid,
        gid: stat.st_gid,
        ctime: stat.st_ctime,
        ctime_nsec: stat.st_ctime_nsec as i64,
    };
    if !FileType::from_raw_mode(stat.st_mode as _).is_file() {
        return Err(FileError::Unsupported(
            "cannot save to a non-regular file".to_string(),
        ));
    }
    if stat.st_nlink > 1 {
        return Err(FileError::Unsupported(format!(
            "cannot save to a hard-linked file (link count {}); use Save As to choose a new target",
            stat.st_nlink
        )));
    }

    let target_mode = (stat.st_mode as u32) & 0o7777;

    // 1. Preserve ownership (uid / gid) against temp's actual fstat (handles setgid directories).
    //    Must run before fchmod so SUID/SGID bits are preserved cleanly.
    let temp_stat = rustix::fs::fstat(temp.as_file())?;
    let want_uid =
        (stat.st_uid != temp_stat.st_uid).then(|| rustix::fs::Uid::from_raw(stat.st_uid));
    let want_gid =
        (stat.st_gid != temp_stat.st_gid).then(|| rustix::fs::Gid::from_raw(stat.st_gid));
    if want_uid.is_some() || want_gid.is_some() {
        if let Err(e) = rustix::fs::fchown(temp.as_file(), want_uid, want_gid) {
            return Err(FileError::Unsupported(format!(
                "cannot preserve original file ownership (uid {} gid {}): {e}; use Save As instead",
                stat.st_uid, stat.st_gid
            )));
        }
    }

    // 2. Preserve extended attributes and POSIX ACLs while temp still has 0o600 write permissions.
    //    Non-ACL xattrs (e.g. user.*) are applied before system.posix_acl_access, and fchmod runs last,
    //    because setting a restrictive ACL or mode (like 0o444) can revoke write permission for user.* xattrs.
    let posix_acl_name = std::ffi::OsStr::new("system.posix_acl_access");
    let mut target_attrs: Vec<(std::ffi::OsString, Vec<u8>)> = Vec::new();

    match target_file.list_xattr() {
        Ok(xattrs) => {
            for name in xattrs {
                match target_file.get_xattr(&name) {
                    Ok(Some(val)) => target_attrs.push((name, val)),
                    Ok(None) => {}
                    Err(e) => {
                        return Err(FileError::Unsupported(format!(
                            "failed to read extended attribute {:?}: {e}; use Save As instead",
                            name
                        )));
                    }
                }
            }
        }
        Err(e) => {
            let is_notsup = matches!(Errno::from_io_error(&e), Some(Errno::OPNOTSUPP));
            if !is_notsup {
                return Err(FileError::Unsupported(format!(
                    "failed to inspect extended attributes: {e}; use Save As instead"
                )));
            }
        }
    }

    // Every inherited attribute must be removable, not only the POSIX ACL.
    let temp_names: Vec<_> = match temp.as_file().list_xattr() {
        Ok(names) => names.collect(),
        Err(error) if Errno::from_io_error(&error) == Some(Errno::OPNOTSUPP) => Vec::new(),
        Err(error) => {
            return Err(FileError::Unsupported(format!(
                "cannot inspect temporary file attributes: {error}"
            )));
        }
    };
    for name in temp_names {
        if !target_attrs
            .iter()
            .any(|(target_name, _)| target_name == &name)
        {
            temp.as_file().remove_xattr(&name).map_err(|error| {
                FileError::Unsupported(format!(
                    "cannot clear inherited attribute {name:?}: {error}"
                ))
            })?;
        }
    }

    // Copy non-ACL xattrs first while temp is 0o600
    for (name, val) in &target_attrs {
        if name == posix_acl_name {
            continue;
        }
        if temp.as_file().get_xattr(name).ok().flatten().as_ref() == Some(val) {
            continue;
        }
        if let Err(e) = temp.as_file().set_xattr(name, val) {
            return Err(FileError::Unsupported(format!(
                "cannot preserve extended attribute {:?}: {e}; use Save As instead",
                name
            )));
        }
    }

    // Copy POSIX access ACL if present
    for (name, val) in &target_attrs {
        if name != posix_acl_name {
            continue;
        }
        if temp.as_file().get_xattr(name).ok().flatten().as_ref() == Some(val) {
            continue;
        }
        if let Err(e) = temp.as_file().set_xattr(name, val) {
            return Err(FileError::Unsupported(format!(
                "cannot preserve POSIX ACL: {e}; use Save As instead"
            )));
        }
    }

    // 3. Preserve mode / permissions (runs last to preserve special bits and target mode)
    rustix::fs::fchmod(temp.as_file(), Mode::from_bits_truncate(target_mode)).map_err(|e| {
        FileError::Unsupported(format!(
            "failed to preserve file permissions (mode {target_mode:o}): {e}; use Save As instead"
        ))
    })?;

    if SecurityStamp::from(&target_file.metadata()?) != copied {
        return Err(FileError::Conflict);
    }
    Ok(Some(copied))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_load_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does_not_exist.txt");
        let res = load(&path);
        match res {
            Err(FileError::Io(err)) => assert_eq!(err.kind(), io::ErrorKind::NotFound),
            other => panic!("expected NotFound io error, got {other:?}"),
        }
    }

    #[test]
    fn test_load_directory() {
        let dir = tempfile::tempdir().unwrap();
        let res = load(dir.path());
        match res {
            Err(FileError::Unsupported(msg)) => {
                assert!(msg.contains("directory"), "got {msg}");
            }
            other => panic!("expected Unsupported error, got {other:?}"),
        }
    }

    #[test]
    fn test_load_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        fs::write(&path, b"").unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, "");
        assert_eq!(loaded.format.bom, false);
        assert_eq!(loaded.format.newline, Newline::Lf);
        assert_eq!(loaded.read_only, None);
    }

    #[test]
    fn test_load_no_newline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no_nl.txt");
        fs::write(&path, b"hello world").unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, "hello world");
        assert_eq!(loaded.format.newline, Newline::Lf);
        assert_eq!(loaded.read_only, None);
    }

    #[test]
    fn test_load_save_roundtrip_lf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_lf.txt");
        let original = "line 1\nline 2\nline 3\n";
        fs::write(&path, original.as_bytes()).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, original);
        assert_eq!(loaded.format.newline, Newline::Lf);
        assert_eq!(loaded.format.bom, false);

        let new_text = "line 1\nmodified line 2\nline 3\n";
        save(
            &path,
            new_text,
            loaded.format,
            Some(&loaded.baseline),
            false,
        )
        .unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), new_text);
    }

    #[test]
    fn test_load_save_roundtrip_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_crlf.txt");
        let disk_bytes = b"alpha\r\nbeta\r\n";
        fs::write(&path, disk_bytes).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, "alpha\nbeta\n");
        assert_eq!(loaded.format.newline, Newline::Crlf);
        assert_eq!(loaded.format.bom, false);
        assert_eq!(loaded.read_only, None);

        let edited = "alpha\nbeta\ngamma\n";
        save(&path, edited, loaded.format, Some(&loaded.baseline), false).unwrap();

        let saved_bytes = fs::read(&path).unwrap();
        assert_eq!(saved_bytes, b"alpha\r\nbeta\r\ngamma\r\n");
    }

    #[test]
    fn test_load_save_roundtrip_bom() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_bom.txt");
        let mut disk_bytes = vec![0xEF, 0xBB, 0xBF];
        disk_bytes.extend_from_slice(b"content with bom\n");
        fs::write(&path, &disk_bytes).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, "content with bom\n");
        assert_eq!(loaded.format.bom, true);
        assert_eq!(loaded.format.newline, Newline::Lf);

        save(
            &path,
            "modified with bom\n",
            loaded.format,
            Some(&loaded.baseline),
            false,
        )
        .unwrap();

        let saved_bytes = fs::read(&path).unwrap();
        assert!(saved_bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
        assert_eq!(&saved_bytes[3..], b"modified with bom\n");
    }

    #[test]
    fn test_load_mixed_endings_readonly_and_save_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.txt");
        let content = "line 1\r\nline 2\nline 3\r\n";
        fs::write(&path, content.as_bytes()).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, content);
        assert_eq!(loaded.format.newline, Newline::Mixed);
        assert!(loaded.read_only.is_some());

        let res = save(
            &path,
            &loaded.text,
            loaded.format,
            Some(&loaded.baseline),
            false,
        );
        match res {
            Err(FileError::Unsupported(msg)) => {
                assert!(msg.contains("mixed"), "got {msg}");
            }
            other => panic!("expected Unsupported error on mixed save, got {other:?}"),
        }
    }

    #[test]
    fn test_load_lone_cr_readonly_and_save_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lone_cr.txt");
        let content = "line 1\rline 2\rline 3";
        fs::write(&path, content.as_bytes()).unwrap();

        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, content);
        assert_eq!(loaded.format.newline, Newline::Mixed);
        assert!(loaded.read_only.is_some());

        let res = save(
            &path,
            &loaded.text,
            loaded.format,
            Some(&loaded.baseline),
            false,
        );
        match res {
            Err(FileError::Unsupported(msg)) => {
                assert!(msg.contains("mixed") || msg.contains("CR"), "got {msg}");
            }
            other => panic!("expected Unsupported error, got {other:?}"),
        }
    }

    #[test]
    fn test_load_nul_byte_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("binary.bin");
        fs::write(&path, b"hello\x00world").unwrap();

        let res = load(&path);
        match res {
            Err(FileError::Unsupported(msg)) => {
                assert!(msg.contains("NUL"), "got {msg}");
            }
            other => panic!("expected Unsupported for NUL, got {other:?}"),
        }
    }

    #[test]
    fn test_load_invalid_utf8_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("invalid_utf8.txt");
        fs::write(&path, b"hello\xff\xfeworld").unwrap();

        let res = load(&path);
        match res {
            Err(FileError::Unsupported(msg)) => {
                assert!(msg.contains("UTF-8"), "got {msg}");
            }
            other => panic!("expected Unsupported for invalid UTF-8, got {other:?}"),
        }
    }

    #[test]
    fn test_load_fifo_rejected_safely() {
        let dir = tempfile::tempdir().unwrap();
        let fifo_path = dir.path().join("test_pipe.fifo");
        rustix::fs::mkfifoat(rustix::fs::CWD, &fifo_path, Mode::from_bits_truncate(0o600)).unwrap();

        let res = load(&fifo_path);
        match res {
            Err(FileError::Unsupported(msg)) => {
                assert!(msg.contains("FIFO") || msg.contains("pipe"), "got {msg}");
            }
            other => panic!("expected Unsupported for FIFO, got {other:?}"),
        }
    }

    #[test]
    fn test_load_symlink_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        fs::write(&target, b"symlink target content\n").unwrap();

        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let loaded = load(&link).unwrap();
        assert_eq!(loaded.text, "symlink target content\n");
        assert_eq!(loaded.read_only, None);
    }

    #[test]
    fn test_save_to_symlink_refused() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        fs::write(&target, b"symlink target content\n").unwrap();

        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let loaded = load(&link).unwrap();

        // Direct save without overwrite -> rejected
        let res1 = save(
            &link,
            "new content\n",
            loaded.format,
            Some(&loaded.baseline),
            false,
        );
        assert!(matches!(res1, Err(FileError::Unsupported(_))));

        // Direct save WITH overwrite -> still rejected!
        let res2 = save(
            &link,
            "new content\n",
            loaded.format,
            Some(&loaded.baseline),
            true,
        );
        assert!(matches!(res2, Err(FileError::Unsupported(_))));

        // Real target and link must remain completely unmodified
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "symlink target content\n"
        );
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn test_save_to_hardlink_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file_a = dir.path().join("file_a.txt");
        fs::write(&file_a, b"hardlink original\n").unwrap();

        let file_b = dir.path().join("file_b.txt");
        fs::hard_link(&file_a, &file_b).unwrap();
        assert_eq!(fs::symlink_metadata(&file_a).unwrap().nlink(), 2);

        let loaded = load(&file_a).unwrap();

        // Direct save without overwrite -> rejected
        let res1 = save(
            &file_a,
            "changed\n",
            loaded.format,
            Some(&loaded.baseline),
            false,
        );
        assert!(matches!(res1, Err(FileError::Unsupported(_))));

        // Direct save WITH overwrite -> still rejected!
        let res2 = save(
            &file_a,
            "changed\n",
            loaded.format,
            Some(&loaded.baseline),
            true,
        );
        assert!(matches!(res2, Err(FileError::Unsupported(_))));

        // Both hardlinks must remain unmodified with nlink == 2
        assert_eq!(fs::read_to_string(&file_a).unwrap(), "hardlink original\n");
        assert_eq!(fs::read_to_string(&file_b).unwrap(), "hardlink original\n");
        assert_eq!(fs::symlink_metadata(&file_a).unwrap().nlink(), 2);
    }

    #[test]
    fn test_save_new_file_no_clobber_and_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new_file.txt");

        // Save new file (expected: None, overwrite: false) -> succeeds
        let format = FileFormat::default();
        save(&path, "first version\n", format, None, false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first version\n");

        // Try saving again as new file (expected: None, overwrite: false) -> Exists!
        let res_exists = save(&path, "second version\n", format, None, false);
        assert!(matches!(res_exists, Err(FileError::Exists)));
        assert_eq!(fs::read_to_string(&path).unwrap(), "first version\n");

        // Save with overwrite: true (expected: None, overwrite: true) -> succeeds
        save(&path, "third version\n", format, None, true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "third version\n");
    }

    #[test]
    fn test_save_conflict_content_changed_same_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conflict_same_time.txt");
        let initial_text = "initial text content\n";
        fs::write(&path, initial_text.as_bytes()).unwrap();

        let loaded = load(&path).unwrap();

        // External edit of identical byte length
        let external_text = "changed text content\n";
        fs::write(&path, external_text.as_bytes()).unwrap();

        // Restore mtime to match baseline exactly using rustix::fs::futimens
        let fd = rustix::fs::open(&path, OFlags::WRONLY | OFlags::CLOEXEC, Mode::empty()).unwrap();
        let ts = rustix::fs::Timestamps {
            last_access: rustix::fs::Timespec {
                tv_sec: loaded.baseline.mtime_sec as _,
                tv_nsec: loaded.baseline.mtime_nsec as _,
            },
            last_modification: rustix::fs::Timespec {
                tv_sec: loaded.baseline.mtime_sec as _,
                tv_nsec: loaded.baseline.mtime_nsec as _,
            },
        };
        rustix::fs::futimens(&fd, &ts).unwrap();
        drop(fd);

        // Attempt save with old baseline and overwrite: false -> must detect Conflict!
        let res_conflict = save(
            &path,
            "local edit\n",
            loaded.format,
            Some(&loaded.baseline),
            false,
        );
        assert!(matches!(res_conflict, Err(FileError::Conflict)));
        assert_eq!(fs::read_to_string(&path).unwrap(), external_text);

        // Overwrite: true successfully overrides conflict
        save(
            &path,
            "local edit\n",
            loaded.format,
            Some(&loaded.baseline),
            true,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "local edit\n");
    }

    #[test]
    fn test_save_conflict_deleted_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("will_delete.txt");
        fs::write(&path, b"something\n").unwrap();

        let loaded = load(&path).unwrap();
        fs::remove_file(&path).unwrap();

        // Attempt save expecting baseline -> Conflict
        let res = save(
            &path,
            "new text\n",
            loaded.format,
            Some(&loaded.baseline),
            false,
        );
        assert!(matches!(res, Err(FileError::Conflict)));

        // Overwrite: true succeeds and recreates file
        save(
            &path,
            "new text\n",
            loaded.format,
            Some(&loaded.baseline),
            true,
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new text\n");
    }

    #[test]
    fn test_save_failure_preserves_original_and_cleans_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stable.txt");
        fs::write(&path, b"stable original\n").unwrap();

        let loaded = load(&path).unwrap();

        // Create conflict
        fs::write(&path, b"modified by someone else\n").unwrap();

        // Attempt save -> Conflict
        let res = save(
            &path,
            "attempt\n",
            loaded.format,
            Some(&loaded.baseline),
            false,
        );
        assert!(matches!(res, Err(FileError::Conflict)));

        // Original file must be preserved
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "modified by someone else\n"
        );

        // Verify no leftover .notefart_tmp_* files exist in directory
        for entry in fs::read_dir(dir.path()).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(
                !name.starts_with(".notefart_tmp_"),
                "leftover tempfile found: {name}"
            );
        }
    }

    #[test]
    fn test_save_preserves_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("perm_test.txt");
        fs::write(&path, b"perm test\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        let loaded = load(&path).unwrap();
        save(
            &path,
            "updated content\n",
            loaded.format,
            Some(&loaded.baseline),
            false,
        )
        .unwrap();

        let meta = fs::metadata(&path).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn test_save_preserves_trailing_newline_and_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let path_no_nl = dir.path().join("no_nl.txt");
        let format = FileFormat::default();

        // Text without final newline
        save(&path_no_nl, "hello\tworld", format, None, false).unwrap();
        assert_eq!(fs::read(&path_no_nl).unwrap(), b"hello\tworld");

        // Text with final newline
        let path_nl = dir.path().join("with_nl.txt");
        save(&path_nl, "hello\tworld\n", format, None, false).unwrap();
        assert_eq!(fs::read(&path_nl).unwrap(), b"hello\tworld\n");
    }

    #[test]
    fn test_save_preserves_non_nul_control_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ctrl.txt");
        let text = "line1\x1b[31mred\x1b[0m\x07\n";
        save(&path, text, FileFormat::default(), None, false).unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.text, text);
    }

    #[test]
    fn test_load_rejects_oversize_over_64mib() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.txt");
        let f = fs::File::create(&path).unwrap();
        f.set_len((MAX_LOAD_SIZE + 1) as u64).unwrap();
        drop(f);

        let res = load(&path);
        match res {
            Err(FileError::Unsupported(msg)) => {
                assert!(msg.contains("64 MiB"), "got {msg}");
            }
            other => panic!("expected Unsupported for >64 MiB, got {other:?}"),
        }
    }

    #[test]
    fn test_save_preserves_xattrs_and_readonly_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("xattr_test.txt");
        fs::write(&path, b"original\n").unwrap();

        let f = fs::OpenOptions::new().write(true).open(&path).unwrap();
        if let Err(e) = f.set_xattr("user.notefart_test", b"meta_value") {
            if matches!(Errno::from_io_error(&e), Some(Errno::OPNOTSUPP)) {
                return;
            }
            panic!("unexpected set_xattr error: {e}");
        }
        drop(f);

        // Set read-only mode (0o444) to verify user.* xattrs are still preserved before fchmod
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();

        let loaded = load(&path).unwrap();
        save(
            &path,
            "updated\n",
            loaded.format,
            Some(&loaded.baseline),
            false,
        )
        .unwrap();

        let reopened = fs::File::open(&path).unwrap();
        assert_eq!(
            reopened.get_xattr("user.notefart_test").unwrap().as_deref(),
            Some(&b"meta_value"[..])
        );
        assert_eq!(
            reopened.metadata().unwrap().permissions().mode() & 0o777,
            0o444
        );
    }

    #[test]
    fn test_save_durability_error_on_dir_sync_failure() {
        // Root bypasses directory mode 0o300 read restriction
        if rustix::process::geteuid().as_raw() == 0 {
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let subdir = dir.path().join("write_only_dir");
        fs::create_dir(&subdir).unwrap();
        fs::set_permissions(&subdir, fs::Permissions::from_mode(0o300)).unwrap();

        let target = subdir.join("persisted.txt");
        let res = save(
            &target,
            "important data\n",
            FileFormat::default(),
            None,
            false,
        );

        // Restore directory permissions immediately so tempdir cleanup succeeds
        let _ = fs::set_permissions(&subdir, fs::Permissions::from_mode(0o700));

        match res {
            Err(FileError::Durability(_)) => {
                assert_eq!(fs::read_to_string(&target).unwrap(), "important data\n");
            }
            other => panic!("expected FileError::Durability, got {other:?}"),
        }
    }

    #[test]
    fn copied_security_metadata_cannot_overwrite_later_permission_change() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("private.txt");
        fs::write(&path, "original").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        let temp = tempfile::NamedTempFile::new_in(directory.path()).unwrap();
        let stamp = preserve_metadata(&path, &temp, true).unwrap().unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        assert!(matches!(
            validate_security_metadata(&path, &stamp),
            Err(FileError::Conflict)
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    }

    #[test]
    fn substituted_temp_name_no_longer_names_the_written_file() {
        let directory = tempfile::tempdir().unwrap();
        let temp = tempfile::Builder::new()
            .prefix(".notefart_tmp_")
            .permissions(fs::Permissions::from_mode(0o600))
            .tempfile_in(directory.path())
            .unwrap();
        let written = rustix::fs::fstat(temp.as_file()).unwrap();
        assert!(names_same_object(temp.path(), &written).unwrap());

        // A process with write access to the directory unlinks the temporary name and puts its
        // own file there; the descriptor holding the buffer is untouched.
        let name = temp.path().to_path_buf();
        fs::remove_file(&name).unwrap();
        fs::write(&name, b"SUBSTITUTED\n").unwrap();
        assert!(!names_same_object(&name, &written).unwrap());

        // The same swap performed with a symlink never opens through it.
        fs::remove_file(&name).unwrap();
        let decoy = directory.path().join("decoy.txt");
        fs::write(&decoy, b"DECOY\n").unwrap();
        std::os::unix::fs::symlink(&decoy, &name).unwrap();
        assert!(!names_same_object(&name, &written).unwrap());
    }

    #[test]
    fn save_still_binds_temp_and_destination_on_success() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bound.txt");
        let saved = save(&path, "payload\n", FileFormat::default(), None, false).unwrap();
        let on_disk = fs::metadata(&path).unwrap();
        assert_eq!((saved.dev, saved.ino), (on_disk.dev(), on_disk.ino()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "payload\n");
    }
}
