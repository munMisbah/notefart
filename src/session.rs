use std::{
    fs::{self, DirBuilder, File, Metadata, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use ropey::Rope;
use serde::{Deserialize, Serialize};

use crate::{
    file_io::{DiskState, FileFormat},
    layout::Axis,
};

pub(crate) const VERSION: u32 = 1;
pub(crate) const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_SESSION_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) const MAX_DOCUMENTS: usize = 1024;
pub(crate) const MAX_PANES: usize = 256;
pub(crate) const MAX_VIEWS: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionMetadata {
    pub version: u32,
    pub explorer_root: Vec<u8>,
    pub explorer_visible: bool,
    pub explorer_focused: bool,
    pub sidebar_width: u16,
    pub active_pane_id: u64,
    pub documents: Vec<DocumentState>,
    pub panes: Vec<PaneState>,
    pub tree: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DocumentState {
    pub id: u64,
    pub path: Option<Vec<u8>>,
    pub baseline: Option<DiskState>,
    pub format: FileFormat,
    pub read_only: Option<String>,
    pub dirty: bool,
    pub text_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PaneState {
    pub id: u64,
    pub active_view_id: u64,
    pub tabs: Vec<ViewState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ViewState {
    pub id: u64,
    pub document_id: u64,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub preferred_column: Option<usize>,
    pub top_line: usize,
    pub left_cell: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum TreeNode {
    Leaf {
        pane_id: u64,
    },
    Split {
        axis: Axis,
        ratio: u32,
        first: usize,
        second: usize,
    },
}

pub(crate) struct SessionImage<'a> {
    pub metadata: SessionMetadata,
    pub texts: Vec<&'a Rope>,
}

pub(crate) struct LoadedSession {
    pub metadata: SessionMetadata,
    pub texts: Vec<Rope>,
}

const MAGIC: &[u8; 8] = b"NAFSES01";
const FRAME_BYTES: u64 = 8 + 8 + 32;

pub(crate) struct SessionStore {
    root: PathBuf,
    path: PathBuf,
    lock_path: PathBuf,
    directory: File,
    lock: File,
}

impl SessionStore {
    pub(crate) fn open_default() -> io::Result<Self> {
        let base = match std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
            Some(value) => PathBuf::from(value),
            None => {
                let home = std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| invalid("session requires absolute XDG_STATE_HOME or HOME"))?;
                let home = PathBuf::from(home);
                if !home.is_absolute() {
                    return Err(invalid("HOME for session state must be absolute"));
                }
                home.join(".local/state")
            }
        };
        if !base.is_absolute() {
            return Err(invalid("XDG_STATE_HOME for session state must be absolute"));
        }
        Self::open(base.join("notefart"))
    }

    pub(crate) fn open(root: PathBuf) -> io::Result<Self> {
        if !root.is_absolute() {
            return Err(invalid("session directory must be absolute"));
        }
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)?;
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::NOFOLLOW).bits() as i32,
            )
            .open(&root)?;
        check_private(&directory.metadata()?, true)?;
        let lock_path = root.join("session.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .mode(0o600)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(&lock_path)?;
        check_private(&lock.metadata()?, false)?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).map_err(
            |error| {
                let error = io::Error::from(error);
                if error.kind() == io::ErrorKind::WouldBlock {
                    io::Error::new(io::ErrorKind::WouldBlock, "session is already open")
                } else {
                    error
                }
            },
        )?;
        let store = Self {
            path: root.join("session.naf"),
            root,
            lock_path,
            directory,
            lock,
        };
        store.verify_directory()?;
        Ok(store)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    fn verify_directory(&self) -> io::Result<()> {
        let entry = fs::symlink_metadata(&self.root)?;
        check_private(&entry, true)?;
        let opened = self.directory.metadata()?;
        if (entry.dev(), entry.ino()) != (opened.dev(), opened.ino()) {
            return Err(invalid("session directory was replaced"));
        }
        let entry = fs::symlink_metadata(&self.lock_path)?;
        check_private(&entry, false)?;
        let opened = self.lock.metadata()?;
        if (entry.dev(), entry.ino()) != (opened.dev(), opened.ino()) {
            return Err(invalid("session lock was replaced"));
        }
        Ok(())
    }

    pub(crate) fn load(&self) -> io::Result<Option<LoadedSession>> {
        self.verify_directory()?;
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(&self.path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let stat = file.metadata()?;
        check_private(&stat, false)?;
        if stat.len() > MAX_SESSION_BYTES || stat.len() < FRAME_BYTES {
            return Err(invalid("session file size is outside the supported bounds"));
        }
        let mut reader = BufReader::new(file);
        let mut hasher = blake3::Hasher::new();
        let loaded = {
            let mut hashed = HashReader {
                inner: &mut reader,
                hasher: &mut hasher,
            };
            let mut magic = [0; 8];
            hashed.read_exact(&mut magic)?;
            if &magic != MAGIC {
                return Err(invalid("unsupported session format/version"));
            }
            let mut length = [0; 8];
            hashed.read_exact(&mut length)?;
            let length = u64::from_le_bytes(length);
            if length > MAX_METADATA_BYTES as u64 {
                return Err(invalid("session metadata exceeds 4 MiB"));
            }
            let mut bytes = vec![0; length as usize];
            hashed.read_exact(&mut bytes)?;
            let metadata: SessionMetadata = serde_json::from_slice(&bytes)
                .map_err(|error| invalid(&format!("invalid session metadata: {error}")))?;
            let payload = payload_size(&metadata)?;
            if FRAME_BYTES + length + payload != stat.len() {
                return Err(invalid("session payload length does not match the file"));
            }
            let mut texts = Vec::with_capacity(metadata.documents.len());
            for document in &metadata.documents {
                let mut limited = (&mut hashed).take(document.text_bytes);
                let text = Rope::from_reader(&mut limited)?;
                if limited.limit() != 0 {
                    return Err(invalid("session text was truncated"));
                }
                texts.push(text);
            }
            LoadedSession { metadata, texts }
        };
        let mut checksum = [0; 32];
        reader.read_exact(&mut checksum)?;
        if hasher.finalize().as_bytes() != &checksum {
            return Err(invalid("session checksum does not match"));
        }
        let mut extra = [0; 1];
        if reader.read(&mut extra)? != 0 {
            return Err(invalid("unexpected bytes after the session checksum"));
        }
        self.verify_directory()?;
        Ok(Some(loaded))
    }

    pub(crate) fn save(&self, image: &SessionImage<'_>) -> io::Result<()> {
        self.verify_directory()?;
        let payload = payload_size(&image.metadata)?;
        if image.texts.len() != image.metadata.documents.len()
            || image
                .texts
                .iter()
                .zip(&image.metadata.documents)
                .any(|(text, state)| text.len_bytes() as u64 != state.text_bytes)
        {
            return Err(invalid("session document text lengths are inconsistent"));
        }
        let mut metadata = MetadataBuffer(Vec::new());
        serde_json::to_writer(&mut metadata, &image.metadata)
            .map_err(|error| invalid(&format!("cannot encode session metadata: {error}")))?;
        if FRAME_BYTES + metadata.0.len() as u64 + payload > MAX_SESSION_BYTES {
            return Err(invalid("session snapshot exceeds 256 MiB"));
        }
        self.check_target()?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".notefart_session_")
            .tempfile_in(&self.root)?;
        let mut hasher = blake3::Hasher::new();
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            {
                let mut hashed = HashWriter {
                    inner: &mut writer,
                    hasher: &mut hasher,
                };
                hashed.write_all(MAGIC)?;
                hashed.write_all(&(metadata.0.len() as u64).to_le_bytes())?;
                hashed.write_all(&metadata.0)?;
                for text in &image.texts {
                    for chunk in text.chunks() {
                        hashed.write_all(chunk.as_bytes())?;
                    }
                }
            }
            writer.write_all(hasher.finalize().as_bytes())?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        self.verify_directory()?;
        self.check_target()?;
        let entry = fs::symlink_metadata(temporary.path())?;
        check_private(&entry, false)?;
        let written = temporary.as_file().metadata()?;
        if (entry.dev(), entry.ino()) != (written.dev(), written.ino()) {
            return Err(invalid("session temporary file was replaced"));
        }
        let persisted = temporary.persist(&self.path).map_err(|error| error.error)?;
        let entry = fs::symlink_metadata(&self.path)?;
        check_private(&entry, false)?;
        let written = persisted.metadata()?;
        if (entry.dev(), entry.ino()) != (written.dev(), written.ino()) {
            return Err(invalid(
                "session destination no longer names the written snapshot",
            ));
        }
        self.verify_directory()?;
        self.directory.sync_all().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "session replaced but directory sync failed; durability uncertain: {error}"
                ),
            )
        })
    }

    fn check_target(&self) -> io::Result<()> {
        match fs::symlink_metadata(&self.path) {
            Ok(metadata) => check_private(&metadata, false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn check_private(metadata: &Metadata, directory: bool) -> io::Result<()> {
    let regular = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file() && metadata.nlink() == 1
    };
    if !regular
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "session state must be a private, owned directory or single-link regular file",
        ));
    }
    Ok(())
}

fn payload_size(metadata: &SessionMetadata) -> io::Result<u64> {
    if metadata.version != VERSION {
        return Err(invalid("unsupported session metadata version"));
    }
    if metadata.documents.is_empty()
        || metadata.documents.len() > MAX_DOCUMENTS
        || metadata.panes.is_empty()
        || metadata.panes.len() > MAX_PANES
        || metadata.tree.len() > MAX_PANES * 2 - 1
    {
        return Err(invalid(
            "session document/pane counts are outside the supported bounds",
        ));
    }
    let mut views = 0usize;
    for pane in &metadata.panes {
        views = views
            .checked_add(pane.tabs.len())
            .ok_or_else(|| invalid("session view count overflow"))?;
        if pane.tabs.is_empty() || views > MAX_VIEWS {
            return Err(invalid(
                "session view count is outside the supported bounds",
            ));
        }
    }
    let mut total = 0u64;
    for document in &metadata.documents {
        if document.text_bytes > MAX_TEXT_BYTES as u64 {
            return Err(invalid("session document exceeds 64 MiB"));
        }
        total = total
            .checked_add(document.text_bytes)
            .ok_or_else(|| invalid("session text length overflow"))?;
        if total > MAX_SESSION_BYTES {
            return Err(invalid("session snapshot exceeds 256 MiB"));
        }
    }
    Ok(total)
}

struct MetadataBuffer(Vec<u8>);

impl Write for MetadataBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_METADATA_BYTES - self.0.len() {
            return Err(invalid("session metadata exceeds 4 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct HashReader<'a, R> {
    inner: &'a mut R,
    hasher: &'a mut blake3::Hasher,
}

impl<R: Read> Read for HashReader<'_, R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = self.inner.read(bytes)?;
        self.hasher.update(&bytes[..count]);
        Ok(count)
    }
}

struct HashWriter<'a, W> {
    inner: &'a mut W,
    hasher: &'a mut blake3::Hasher,
}

impl<W: Write> Write for HashWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.inner.write(bytes)?;
        self.hasher.update(&bytes[..count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
