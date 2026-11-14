//! Persistent user preferences (`theme`, `color`, `icons`, `clipboard`).
//!
//! Standard location on Linux (per the XDG Base Directory Specification):
//! `$XDG_CONFIG_HOME/notefart/config.toml` when `XDG_CONFIG_HOME` is an
//! absolute path, otherwise `$HOME/.config/notefart/config.toml` when `HOME`
//! is an absolute path. Relative `XDG_CONFIG_HOME` values are ignored so
//! startup never picks up cwd-relative configuration; there is no project- or
//! workspace-local configuration file.
//!
//! Precedence is `built-ins < user TOML < CLI flags`, independent of CLI
//! flag order: only flags explicitly provided on the command line override
//! values loaded from disk. Normal editor startup never writes the file. Only
//! `--save-config` merges existing preferences with CLI overrides, writes a
//! canonical four-key TOML file atomically with `0700` directories and `0600`
//! permissions, and exits without initializing the terminal.
//!
//! Input loading is nonblocking and bounded to `64 KiB` from a regular file
//! (`O_NONBLOCK` + `fstat` regular check, valid UTF-8, strict root-table string
//! validation). Unknown keys or sections, wrong types, invalid values,
//! non-regular files, and oversized files fail with escaped diagnostics before
//! raw mode, offering `--no-config` for recovery without silent fallback.
//! Like document loading, reads follow symbolic links and require the resolved
//! target to be a regular file. Saving refuses symlinked, hard-linked, or
//! non-regular targets.

use std::{
    env,
    ffi::OsString,
    fmt, fs,
    io::{self, Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use rustix::{
    fs::{FileType, Mode, OFlags},
    io::Errno,
};

use crate::{clipboard::ClipboardMode, theme::Options};

/// Maximum accepted configuration file size (`64 KiB`).
pub const MAX_CONFIG_BYTES: u64 = 64 * 1024;

const APP_DIR: &str = "notefart";
const FILE_NAME: &str = "config.toml";

/// Origin of the user preferences file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    /// Standard XDG/HOME user configuration path.
    Default,
    /// Explicit `--config PATH`.
    Explicit(PathBuf),
    /// `--no-config`: bypass user configuration completely.
    Disabled,
}

/// Snapshot of environment variables used to locate the standard user config.
///
/// Passed explicitly so path resolution stays pure and unit tests never mutate
/// process-global environment variables.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Env {
    pub xdg_config_home: Option<OsString>,
    pub home: Option<OsString>,
}

impl Env {
    /// Captures `XDG_CONFIG_HOME` and `HOME` from the current process.
    pub fn from_process() -> Self {
        Self {
            xdg_config_home: env::var_os("XDG_CONFIG_HOME"),
            home: env::var_os("HOME"),
        }
    }
}

/// Resolves the standard user configuration path, or `None` if neither
/// `XDG_CONFIG_HOME` nor `HOME` yields an absolute base directory.
pub fn default_path(env: &Env) -> Option<PathBuf> {
    if let Some(dir) = env
        .xdg_config_home
        .as_deref()
        .map(Path::new)
        .filter(|path| path.is_absolute())
    {
        return Some(dir.join(APP_DIR).join(FILE_NAME));
    }

    let home = env
        .home
        .as_deref()
        .map(Path::new)
        .filter(|path| path.is_absolute())?;
    Some(home.join(".config").join(APP_DIR).join(FILE_NAME))
}

/// CLI preference values paired with per-field presence flags.
///
/// Only fields marked as set override the user configuration file.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct CliOverrides {
    pub options: Options,
    pub theme: bool,
    pub color: bool,
    pub icons: bool,
    pub clipboard: bool,
}

impl CliOverrides {
    /// Applies explicitly passed CLI flags onto `base`.
    pub fn apply(&self, base: &mut Options) {
        if self.theme {
            base.theme = self.options.theme;
        }
        if self.color {
            base.colors = self.options.colors;
        }
        if self.icons {
            base.icons = self.options.icons;
        }
        if self.clipboard {
            base.clipboard = self.options.clipboard;
        }
    }
}

/// Configuration loading, validation, and saving errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigError {
    /// Neither `XDG_CONFIG_HOME` nor `HOME` yielded an absolute path for `--save-config`.
    NoLocation,
    /// `--save-config` was requested with `--no-config`.
    DisabledTarget,
    /// An explicit `--config PATH` was missing or empty.
    NotFound { path: PathBuf },
    /// The target path is not a regular file (directory, FIFO, socket, device).
    NotRegular { path: PathBuf, kind: &'static str },
    /// The file exceeds `MAX_CONFIG_BYTES`.
    TooLarge {
        path: PathBuf,
        limit: u64,
        size: u64,
    },
    /// Opening or reading the file failed.
    Read { path: PathBuf, detail: String },
    /// File bytes are not valid UTF-8.
    InvalidUtf8 { path: PathBuf, offset: usize },
    /// TOML syntax error.
    Parse { path: PathBuf, detail: String },
    /// Unrecognized key or `[section]` table in the TOML root.
    UnknownKey { path: PathBuf, key: String },
    /// Known preference key had a non-string TOML value.
    WrongType {
        path: PathBuf,
        key: String,
        found: &'static str,
    },
    /// Preference string value was rejected by the option parser.
    InvalidValue {
        path: PathBuf,
        key: String,
        detail: &'static str,
    },
    /// Saving refused a symlink, hard link, or non-regular target.
    UnsafeTarget { path: PathBuf, detail: String },
    /// Directory creation, temporary file write, sync, or atomic replace failed.
    Write { path: PathBuf, detail: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoLocation => write!(
                f,
                "cannot locate user configuration: set an absolute XDG_CONFIG_HOME or HOME"
            ),
            Self::DisabledTarget => write!(f, "--save-config cannot be combined with --no-config"),
            Self::NotFound { path } => write!(
                f,
                "configuration file not found: {} (use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::NotRegular { path, kind } => write!(
                f,
                "configuration {} is {kind}, not a regular file (use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::TooLarge { path, limit, size } => write!(
                f,
                "configuration {} is too large ({size} bytes, maximum {limit} bytes) (use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::Read { path, detail } => write!(
                f,
                "cannot read configuration {}: {detail} (use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::InvalidUtf8 { path, offset } => write!(
                f,
                "configuration {} is not valid UTF-8 (invalid byte at offset {offset}) (use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::Parse { path, detail } => write!(
                f,
                "invalid configuration {}: {detail} (use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::UnknownKey { path, key } => write!(
                f,
                "invalid configuration {}: unknown key or section `{key}` (expected theme, color, icons, or clipboard; use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::WrongType { path, key, found } => write!(
                f,
                "invalid configuration {}: `{key}` must be a string, found {found} (use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::InvalidValue { path, key, detail } => write!(
                f,
                "invalid configuration {}: `{key}`: {detail} (use --no-config to start without saved preferences)",
                path.display()
            ),
            Self::UnsafeTarget { path, detail } => write!(
                f,
                "refusing to write configuration {}: {detail}",
                path.display()
            ),
            Self::Write { path, detail } => {
                write!(f, "cannot write configuration {}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Resolves runtime options using `built-ins < user TOML < CLI flags`.
pub fn resolve(source: &Source, env: &Env, cli: &CliOverrides) -> Result<Options, ConfigError> {
    let mut options = Options::default();
    match source {
        Source::Disabled => {}
        Source::Explicit(path) => {
            if path.as_os_str().is_empty() {
                return Err(ConfigError::NotFound { path: path.clone() });
            }
            options = load(path)?;
        }
        Source::Default => {
            if let Some(path) = default_path(env) {
                match load(&path) {
                    Ok(saved) => options = saved,
                    Err(ConfigError::NotFound { .. }) => {}
                    Err(err) => return Err(err),
                }
            }
        }
    }
    cli.apply(&mut options);
    Ok(options)
}

/// Merges existing preferences at `source` with `cli`, writes the canonical
/// TOML file atomically, and returns the destination path.
pub fn save_merged(source: &Source, env: &Env, cli: &CliOverrides) -> Result<PathBuf, ConfigError> {
    let target = target_path(source, env)?;
    let mut options = match load(&target) {
        Ok(saved) => saved,
        Err(ConfigError::NotFound { .. }) => Options::default(),
        Err(err) => return Err(err),
    };
    cli.apply(&mut options);
    save(&target, &options)?;
    Ok(target)
}

fn target_path(source: &Source, env: &Env) -> Result<PathBuf, ConfigError> {
    match source {
        Source::Explicit(path) => {
            if path.as_os_str().is_empty() {
                return Err(ConfigError::Write {
                    path: path.clone(),
                    detail: "configuration path cannot be empty".to_string(),
                });
            }
            Ok(path.clone())
        }
        Source::Default => default_path(env).ok_or(ConfigError::NoLocation),
        Source::Disabled => Err(ConfigError::DisabledTarget),
    }
}

/// Loads and validates preferences from `path`.
pub fn load(path: &Path) -> Result<Options, ConfigError> {
    let bytes = read_bytes(path)?;
    let text = std::str::from_utf8(&bytes).map_err(|err| ConfigError::InvalidUtf8 {
        path: path.to_path_buf(),
        offset: err.valid_up_to(),
    })?;
    parse(text, path)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, ConfigError> {
    // Open with O_NONBLOCK first so FIFOs/named pipes fail immediately in fstat
    // instead of blocking without a writer.
    let fd = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::NONBLOCK | OFlags::CLOEXEC | OFlags::NOCTTY,
        Mode::empty(),
    )
    .map_err(|err| open_error(path, err))?;

    let stat = rustix::fs::fstat(&fd).map_err(|err| ConfigError::Read {
        path: path.to_path_buf(),
        detail: io::Error::from(err).to_string(),
    })?;

    let file_type = FileType::from_raw_mode(stat.st_mode as _);
    if !file_type.is_file() {
        return Err(ConfigError::NotRegular {
            path: path.to_path_buf(),
            kind: file_kind(file_type),
        });
    }

    let size = u64::try_from(stat.st_size).unwrap_or(0);
    if size > MAX_CONFIG_BYTES {
        return Err(ConfigError::TooLarge {
            path: path.to_path_buf(),
            limit: MAX_CONFIG_BYTES,
            size,
        });
    }

    if let Ok(flags) = rustix::fs::fcntl_getfl(&fd) {
        let _ = rustix::fs::fcntl_setfl(&fd, flags - OFlags::NONBLOCK);
    }

    let mut file = fs::File::from(fd);
    let mut bytes = Vec::with_capacity(size.min(8192) as usize);
    let mut chunk = [0u8; 8192];
    loop {
        let n = match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => n,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    detail: err.to_string(),
                });
            }
        };
        let next_len = (bytes.len() + n) as u64;
        if next_len > MAX_CONFIG_BYTES {
            return Err(ConfigError::TooLarge {
                path: path.to_path_buf(),
                limit: MAX_CONFIG_BYTES,
                size: next_len,
            });
        }
        bytes.extend_from_slice(&chunk[..n]);
    }

    Ok(bytes)
}

fn open_error(path: &Path, errno: Errno) -> ConfigError {
    if errno == Errno::NOENT {
        ConfigError::NotFound {
            path: path.to_path_buf(),
        }
    } else {
        ConfigError::Read {
            path: path.to_path_buf(),
            detail: io::Error::from(errno).to_string(),
        }
    }
}

fn file_kind(file_type: FileType) -> &'static str {
    if file_type.is_dir() {
        "a directory"
    } else if file_type.is_fifo() {
        "a FIFO/named pipe"
    } else if file_type.is_socket() {
        "a socket"
    } else if file_type.is_char_device() {
        "a character device"
    } else if file_type.is_block_device() {
        "a block device"
    } else {
        "a non-regular filesystem object"
    }
}

fn parse(input: &str, path: &Path) -> Result<Options, ConfigError> {
    let table = toml::de::DeTable::parse(input)
        .map_err(|err| ConfigError::Parse {
            path: path.to_path_buf(),
            detail: err.to_string(),
        })?
        .into_inner();

    let mut options = Options::default();
    for (key, value) in table.iter() {
        let name: &str = key.get_ref();
        let raw = value.get_ref();
        match name {
            "theme" => {
                let s = expect_str(path, name, raw)?;
                options.theme = s.parse().map_err(|detail| ConfigError::InvalidValue {
                    path: path.to_path_buf(),
                    key: name.to_string(),
                    detail,
                })?;
            }
            "color" => {
                let s = expect_str(path, name, raw)?;
                options.colors = s.parse().map_err(|detail| ConfigError::InvalidValue {
                    path: path.to_path_buf(),
                    key: name.to_string(),
                    detail,
                })?;
            }
            "icons" => {
                let s = expect_str(path, name, raw)?;
                options.icons = s.parse().map_err(|detail| ConfigError::InvalidValue {
                    path: path.to_path_buf(),
                    key: name.to_string(),
                    detail,
                })?;
            }
            "clipboard" => {
                let s = expect_str(path, name, raw)?;
                options.clipboard =
                    s.parse::<ClipboardMode>()
                        .map_err(|detail| ConfigError::InvalidValue {
                            path: path.to_path_buf(),
                            key: name.to_string(),
                            detail,
                        })?;
            }
            _ => {
                return Err(ConfigError::UnknownKey {
                    path: path.to_path_buf(),
                    key: name.to_string(),
                });
            }
        }
    }
    Ok(options)
}

fn expect_str<'v, 'i>(
    path: &Path,
    key: &str,
    value: &'v toml::de::DeValue<'i>,
) -> Result<&'v str, ConfigError> {
    value.as_str().ok_or_else(|| ConfigError::WrongType {
        path: path.to_path_buf(),
        key: key.to_string(),
        found: value.type_str(),
    })
}

fn clipboard_name(mode: ClipboardMode) -> &'static str {
    match mode {
        ClipboardMode::Auto => "auto",
        ClipboardMode::Internal => "internal",
        ClipboardMode::System => "system",
    }
}

fn canonical_toml(options: &Options) -> String {
    format!(
        "theme = \"{}\"\ncolor = \"{}\"\nicons = \"{}\"\nclipboard = \"{}\"\n",
        options.theme.as_str(),
        options.colors.as_str(),
        options.icons.as_str(),
        clipboard_name(options.clipboard),
    )
}

/// Atomically writes canonical preferences to `path` with `0700` created
/// directories and `0600` file permissions, refusing unsafe targets.
pub fn save(path: &Path, options: &Options) -> Result<(), ConfigError> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    ensure_parent_dir(parent)?;
    reject_unsafe_target(path)?;

    let mut temp = tempfile::Builder::new()
        .prefix(".notefart-config-")
        .tempfile_in(parent)
        .map_err(|err| ConfigError::Write {
            path: path.to_path_buf(),
            detail: err.to_string(),
        })?;

    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|err| ConfigError::Write {
            path: path.to_path_buf(),
            detail: err.to_string(),
        })?;

    let body = canonical_toml(options);
    let file = temp.as_file_mut();
    if let Err(err) = file.write_all(body.as_bytes()) {
        return Err(ConfigError::Write {
            path: path.to_path_buf(),
            detail: err.to_string(),
        });
    }
    if let Err(err) = file.flush() {
        return Err(ConfigError::Write {
            path: path.to_path_buf(),
            detail: err.to_string(),
        });
    }
    if let Err(err) = file.sync_all() {
        return Err(ConfigError::Write {
            path: path.to_path_buf(),
            detail: err.to_string(),
        });
    }

    // Defend against pre-rename target substitution into a symlink or link.
    reject_unsafe_target(path)?;

    temp.persist(path).map_err(|err| ConfigError::Write {
        path: path.to_path_buf(),
        detail: err.error.to_string(),
    })?;

    sync_dir(parent)?;
    Ok(())
}

fn ensure_parent_dir(dir: &Path) -> Result<(), ConfigError> {
    match fs::symlink_metadata(dir) {
        Ok(_) => {
            let meta = fs::metadata(dir).map_err(|err| ConfigError::Write {
                path: dir.to_path_buf(),
                detail: err.to_string(),
            })?;
            if !meta.is_dir() {
                return Err(ConfigError::UnsafeTarget {
                    path: dir.to_path_buf(),
                    detail: "parent path is not a directory".to_string(),
                });
            }
            return Ok(());
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(ConfigError::Write {
                path: dir.to_path_buf(),
                detail: err.to_string(),
            });
        }
    }

    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(0o700);
    builder.create(dir).map_err(|err| ConfigError::Write {
        path: dir.to_path_buf(),
        detail: err.to_string(),
    })?;

    let meta = fs::metadata(dir).map_err(|err| ConfigError::Write {
        path: dir.to_path_buf(),
        detail: err.to_string(),
    })?;
    if !meta.is_dir() {
        return Err(ConfigError::UnsafeTarget {
            path: dir.to_path_buf(),
            detail: "parent path is not a directory".to_string(),
        });
    }
    Ok(())
}

fn reject_unsafe_target(path: &Path) -> Result<(), ConfigError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            let file_type = meta.file_type();
            if file_type.is_symlink() {
                return Err(ConfigError::UnsafeTarget {
                    path: path.to_path_buf(),
                    detail: "target is a symbolic link".to_string(),
                });
            }
            if !file_type.is_file() {
                return Err(ConfigError::UnsafeTarget {
                    path: path.to_path_buf(),
                    detail: "target is not a regular file".to_string(),
                });
            }
            if meta.nlink() > 1 {
                return Err(ConfigError::UnsafeTarget {
                    path: path.to_path_buf(),
                    detail: format!("target has {} hard links", meta.nlink()),
                });
            }
            Ok(())
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(ConfigError::Write {
            path: path.to_path_buf(),
            detail: err.to_string(),
        }),
    }
}

fn sync_dir(dir: &Path) -> Result<(), ConfigError> {
    let fd = rustix::fs::open(
        dir,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|err| ConfigError::Write {
        path: dir.to_path_buf(),
        detail: io::Error::from(err).to_string(),
    })?;

    rustix::fs::fsync(&fd).map_err(|err| ConfigError::Write {
        path: dir.to_path_buf(),
        detail: io::Error::from(err).to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ColorMode, IconMode, ThemeKind};
    use std::os::unix::fs::symlink;

    #[test]
    fn default_path_prefers_absolute_xdg_and_ignores_relative_xdg() {
        let xdg_env = Env {
            xdg_config_home: Some(OsString::from("/xdg/base")),
            home: Some(OsString::from("/home/user")),
        };
        assert_eq!(
            default_path(&xdg_env),
            Some(PathBuf::from("/xdg/base/notefart/config.toml"))
        );

        let relative_xdg = Env {
            xdg_config_home: Some(OsString::from("relative/xdg")),
            home: Some(OsString::from("/home/user")),
        };
        assert_eq!(
            default_path(&relative_xdg),
            Some(PathBuf::from("/home/user/.config/notefart/config.toml"))
        );

        let empty_xdg = Env {
            xdg_config_home: Some(OsString::new()),
            home: Some(OsString::from("/home/user")),
        };
        assert_eq!(
            default_path(&empty_xdg),
            Some(PathBuf::from("/home/user/.config/notefart/config.toml"))
        );

        let relative_home = Env {
            xdg_config_home: Some(OsString::from("rel")),
            home: Some(OsString::from("relhome")),
        };
        assert_eq!(default_path(&relative_home), None);
    }

    #[test]
    fn parse_rejects_unknown_keys_sections_wrong_types_and_bad_values() {
        let p = Path::new("/tmp/config.toml");

        let err = parse("extra = \"x\"\n", p).unwrap_err();
        assert!(matches!(&err, ConfigError::UnknownKey { key, .. } if key.as_str() == "extra"));

        let section = parse("[extra]\ntheme = \"dark\"\n", p).unwrap_err();
        assert!(matches!(&section, ConfigError::UnknownKey { key, .. } if key.as_str() == "extra"));

        let nested_theme = parse("[theme]\nname = \"dark\"\n", p).unwrap_err();
        assert!(matches!(
            &nested_theme,
            ConfigError::WrongType { key, .. } if key.as_str() == "theme"
        ));

        let int_icons = parse("icons = 42\n", p).unwrap_err();
        assert!(matches!(
            &int_icons,
            ConfigError::WrongType { key, .. } if key.as_str() == "icons"
        ));

        let bad_val = parse("clipboard = \"osc52\"\n", p).unwrap_err();
        assert!(matches!(
            &bad_val,
            ConfigError::InvalidValue { key, .. } if key.as_str() == "clipboard"
        ));
    }

    #[test]
    fn load_enforces_size_limit_utf8_and_regular_file_without_blocking() {
        let dir = tempfile::tempdir().unwrap();

        let exact_path = dir.path().join("exact.toml");
        let mut bytes = b"theme = \"light\"\n".to_vec();
        bytes.resize(MAX_CONFIG_BYTES as usize, b' ');
        fs::write(&exact_path, bytes).unwrap();
        assert_eq!(load(&exact_path).unwrap().theme, ThemeKind::Light);

        let over_path = dir.path().join("over.toml");
        fs::write(&over_path, vec![b' '; MAX_CONFIG_BYTES as usize + 1]).unwrap();
        assert!(matches!(
            load(&over_path),
            Err(ConfigError::TooLarge { .. })
        ));

        let bad_utf8 = dir.path().join("bad_utf8.toml");
        fs::write(&bad_utf8, [0xff, 0xfe, b'\n']).unwrap();
        assert!(matches!(
            load(&bad_utf8),
            Err(ConfigError::InvalidUtf8 { offset: 0, .. })
        ));

        assert!(matches!(
            load(dir.path()),
            Err(ConfigError::NotRegular { .. })
        ));

        let fifo_path = dir.path().join("pipe.toml");
        rustix::fs::mkfifoat(rustix::fs::CWD, &fifo_path, Mode::from_bits_truncate(0o600)).unwrap();
        assert!(matches!(
            load(&fifo_path),
            Err(ConfigError::NotRegular { .. })
        ));
    }

    #[test]
    fn resolve_precedence_built_ins_lt_file_lt_cli_and_no_silent_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let xdg = dir.path().join("xdg");
        let cfg_dir = xdg.join("notefart");
        fs::create_dir_all(&cfg_dir).unwrap();
        let cfg_path = cfg_dir.join("config.toml");
        fs::write(
            &cfg_path,
            "theme = \"terminal\"\ncolor = \"16\"\nicons = \"ascii\"\nclipboard = \"internal\"\n",
        )
        .unwrap();

        let env = Env {
            xdg_config_home: Some(xdg.into_os_string()),
            home: Some(dir.path().as_os_str().to_os_string()),
        };

        let cli = CliOverrides {
            options: Options {
                theme: ThemeKind::Light,
                clipboard: ClipboardMode::System,
                ..Options::default()
            },
            theme: true,
            color: false,
            icons: false,
            clipboard: true,
        };

        let resolved = resolve(&Source::Default, &env, &cli).unwrap();
        assert_eq!(resolved.theme, ThemeKind::Light);
        assert_eq!(resolved.colors, ColorMode::Ansi16);
        assert_eq!(resolved.icons, IconMode::Ascii);
        assert_eq!(resolved.clipboard, ClipboardMode::System);

        // Missing default file is normal; missing explicit --config is an error.
        let empty_env = Env {
            xdg_config_home: Some(dir.path().join("absent").into_os_string()),
            home: None,
        };
        let from_missing_default = resolve(&Source::Default, &empty_env, &cli).unwrap();
        assert_eq!(from_missing_default.theme, ThemeKind::Light);
        assert_eq!(from_missing_default.icons, IconMode::Unicode);

        let missing_explicit = dir.path().join("explicit-missing.toml");
        assert!(matches!(
            resolve(
                &Source::Explicit(missing_explicit),
                &env,
                &CliOverrides::default()
            ),
            Err(ConfigError::NotFound { .. })
        ));

        // Corrupt file fails instead of silently falling back; --no-config bypasses it.
        fs::write(&cfg_path, "icons = 123\n").unwrap();
        assert!(resolve(&Source::Default, &env, &CliOverrides::default()).is_err());
        let bypassed = resolve(&Source::Disabled, &env, &cli).unwrap();
        assert_eq!(bypassed.theme, ThemeKind::Light);
        assert_eq!(bypassed.icons, IconMode::Unicode);
    }

    #[test]
    fn save_merged_persists_overrides_preserves_other_keys_and_creates_private_file() {
        let dir = tempfile::tempdir().unwrap();
        let xdg = dir.path().join("xdg");
        let env = Env {
            xdg_config_home: Some(xdg.clone().into_os_string()),
            home: None,
        };

        let first = CliOverrides {
            options: Options {
                icons: IconMode::Nerd,
                ..Options::default()
            },
            icons: true,
            ..CliOverrides::default()
        };
        let written = save_merged(&Source::Default, &env, &first).unwrap();
        assert_eq!(written, xdg.join("notefart/config.toml"));
        let mode = fs::metadata(&written).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        // Normal launch reads saved icon profile without modifying file.
        let launched = resolve(&Source::Default, &env, &CliOverrides::default()).unwrap();
        assert_eq!(launched.icons, IconMode::Nerd);

        // Second save merges new CLI override while preserving earlier icons preference.
        let second = CliOverrides {
            options: Options {
                theme: ThemeKind::Light,
                clipboard: ClipboardMode::System,
                ..Options::default()
            },
            theme: true,
            clipboard: true,
            ..CliOverrides::default()
        };
        save_merged(&Source::Default, &env, &second).unwrap();

        let relaunched = resolve(&Source::Default, &env, &CliOverrides::default()).unwrap();
        assert_eq!(relaunched.theme, ThemeKind::Light);
        assert_eq!(relaunched.icons, IconMode::Nerd);
        assert_eq!(relaunched.clipboard, ClipboardMode::System);

        // Explicit CLI override at runtime wins without mutating the saved file.
        let runtime_override = CliOverrides {
            options: Options {
                icons: IconMode::Ascii,
                ..Options::default()
            },
            icons: true,
            ..CliOverrides::default()
        };
        let overridden = resolve(&Source::Default, &env, &runtime_override).unwrap();
        assert_eq!(overridden.icons, IconMode::Ascii);
        assert_eq!(load(&written).unwrap().icons, IconMode::Nerd);
    }

    #[test]
    fn save_refuses_symlinks_hardlinks_and_directories_without_clobbering() {
        let dir = tempfile::tempdir().unwrap();

        let real = dir.path().join("real.toml");
        fs::write(&real, b"sentinel\n").unwrap();
        let link = dir.path().join("link.toml");
        symlink(&real, &link).unwrap();

        let err = save(&link, &Options::default()).unwrap_err();
        assert!(matches!(err, ConfigError::UnsafeTarget { .. }));
        assert_eq!(fs::read(&real).unwrap(), b"sentinel\n");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );

        let hard_a = dir.path().join("hard_a.toml");
        let hard_b = dir.path().join("hard_b.toml");
        fs::write(&hard_a, b"theme = \"dark\"\n").unwrap();
        fs::hard_link(&hard_a, &hard_b).unwrap();
        assert!(matches!(
            save(&hard_a, &Options::default()),
            Err(ConfigError::UnsafeTarget { .. })
        ));
        assert_eq!(fs::read(&hard_b).unwrap(), b"theme = \"dark\"\n");

        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        assert!(matches!(
            save(&subdir, &Options::default()),
            Err(ConfigError::UnsafeTarget { .. })
        ));
    }

    #[test]
    fn save_merged_refuses_malformed_existing_config_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        fs::write(&cfg, b"theme = 999\n").unwrap();

        let cli = CliOverrides {
            options: Options {
                icons: IconMode::Nerd,
                ..Options::default()
            },
            icons: true,
            ..CliOverrides::default()
        };

        assert!(save_merged(&Source::Explicit(cfg.clone()), &Env::default(), &cli).is_err());
        assert_eq!(fs::read(&cfg).unwrap(), b"theme = 999\n");
    }
}
