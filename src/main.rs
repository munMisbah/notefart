mod app;
mod chrome;
mod clipboard;
mod config;
mod document;
mod explorer;
mod field;
mod field_ui;
mod file_io;
mod help;
mod highlight;
mod input;
mod layout;
mod probe;
mod search;
mod shortcuts;
mod syntax;
mod terminal;
mod theme;
mod ui;
mod workspace;

use std::{env, ffi::OsString, path::PathBuf, process::ExitCode};

use clipboard::ClipboardMode;

const HELP: &str = concat!(
    "Notefart — terminal text editor\n\n",
    "Usage: naf [OPTIONS] [FILE|FOLDER]\n",
    "       naf [OPTIONS] --save-config\n",
    "       naf --probe\n",
    "       naf [OPTIONS] -- PATH\n\n",
    "  --theme dark|light|terminal          Editor palette\n",
    "  --color auto|truecolor|256|16|none    Terminal color capability override\n",
    "  --icons unicode|nerd|ascii           Icon profile (Nerd Font is opt-in)\n",
    "  --clipboard auto|internal|system     Clipboard backend for copy and paste\n",
    "  --config PATH                        Preferences file (default: user config)\n",
    "  --no-config                          Ignore saved preferences\n",
    "  --save-config                        Save merged preferences and exit\n",
    "  --probe                             Input diagnostics, no editor options\n",
    "  --help                              Show this help\n",
    "  --version                           Show the version\n\n",
    "Preferences: $XDG_CONFIG_HOME/notefart/config.toml (or $HOME/.config/notefart/config.toml).\n",
    "Precedence: built-ins < user file < CLI flags. --save-config cannot be combined with\n",
    "--no-config or an initial path; --probe rejects editor and preference options.\n\n",
    "UTF-8 with LF/CRLF and optional BOM; Linux terminal required.\n",
);

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Help,
    Version,
    Probe,
    Editor {
        path: Option<PathBuf>,
        overrides: config::CliOverrides,
        source: config::Source,
        save_config: bool,
    },
}

fn apply_option(
    overrides: &mut config::CliOverrides,
    name: &str,
    value: &str,
) -> Result<(), &'static str> {
    match name {
        "--theme" => {
            overrides.options.theme = value.parse()?;
            overrides.theme = true;
        }
        "--color" => {
            overrides.options.colors = value.parse()?;
            overrides.color = true;
        }
        "--icons" => {
            overrides.options.icons = value.parse()?;
            overrides.icons = true;
        }
        "--clipboard" => {
            overrides.options.clipboard = value.parse::<ClipboardMode>()?;
            overrides.clipboard = true;
        }
        _ => return Err("unknown option; use --help"),
    }
    Ok(())
}

fn parse_args(mut args: impl Iterator<Item = OsString>) -> Result<Command, &'static str> {
    let mut overrides = config::CliOverrides::default();
    let mut explicit_config: Option<PathBuf> = None;
    let mut no_config = false;
    let mut save_config = false;
    let mut path = None;
    let mut probe = false;
    let mut editor_option = false;

    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            return Ok(Command::Help);
        }
        if arg == "--version" || arg == "-V" {
            return Ok(Command::Version);
        }
        if arg == "--probe" {
            probe = true;
            continue;
        }
        if arg == "--no-config" {
            no_config = true;
            editor_option = true;
            continue;
        }
        if arg == "--save-config" {
            save_config = true;
            editor_option = true;
            continue;
        }
        if arg == "--" {
            if path.is_some() {
                return Err("only one initial file or folder is supported");
            }
            path = Some(PathBuf::from(args.next().ok_or("-- requires a path")?));
            if args.next().is_some() {
                return Err("only one initial file or folder is supported");
            }
            break;
        }
        if arg == "--config" {
            let value = args.next().ok_or("--config requires a path")?;
            if value.is_empty() {
                return Err("--config requires a path");
            }
            if explicit_config.is_some() {
                return Err("only one --config path is supported");
            }
            explicit_config = Some(PathBuf::from(value));
            editor_option = true;
            continue;
        }
        if let Some((name, value)) = arg.to_str().and_then(|arg| arg.split_once('='))
            && name.starts_with("--")
        {
            if name == "--config" {
                if value.is_empty() {
                    return Err("--config requires a path");
                }
                if explicit_config.is_some() {
                    return Err("only one --config path is supported");
                }
                explicit_config = Some(PathBuf::from(value));
                editor_option = true;
                continue;
            }
            apply_option(&mut overrides, name, value)?;
            editor_option = true;
            continue;
        }
        if arg == "--theme" || arg == "--color" || arg == "--icons" || arg == "--clipboard" {
            let value = args.next().ok_or("option requires a value; use --help")?;
            apply_option(
                &mut overrides,
                arg.to_str().expect("ASCII option"),
                value.to_str().ok_or("option value must be UTF-8")?,
            )?;
            editor_option = true;
            continue;
        }
        if arg.as_encoded_bytes().starts_with(b"-") {
            return Err("unknown option; use -- to open a path starting with '-'");
        }
        if path.replace(PathBuf::from(arg)).is_some() {
            return Err("only one initial file or folder is supported");
        }
    }

    if probe {
        if editor_option || path.is_some() {
            return Err("--probe cannot be combined with editor options or a path");
        }
        return Ok(Command::Probe);
    }
    if no_config && explicit_config.is_some() {
        return Err("cannot combine --config with --no-config");
    }
    if save_config && no_config {
        return Err("cannot combine --save-config with --no-config");
    }
    if save_config && path.is_some() {
        return Err("--save-config cannot be combined with an initial file or folder");
    }

    let source = if no_config {
        config::Source::Disabled
    } else if let Some(config_path) = explicit_config {
        config::Source::Explicit(config_path)
    } else {
        config::Source::Default
    };

    Ok(Command::Editor {
        path,
        overrides,
        source,
        save_config,
    })
}

fn report_io(result: std::io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("naf: {}", error.to_string().escape_default());
            ExitCode::FAILURE
        }
    }
}

fn main() -> ExitCode {
    let command = match parse_args(env::args_os().skip(1)) {
        Ok(command) => command,
        Err(error) => {
            eprintln!("naf: {error}");
            return ExitCode::FAILURE;
        }
    };

    match command {
        Command::Help => {
            print!("{HELP}");
            for section in shortcuts::SECTIONS {
                println!("\n{}:", section.title);
                for (keys, description) in section.bindings {
                    println!("  {keys:<26} {description}");
                }
            }
            ExitCode::SUCCESS
        }
        Command::Version => {
            println!("naf {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Command::Probe => report_io(probe::run()),
        Command::Editor {
            path,
            overrides,
            source,
            save_config,
        } => {
            let env = config::Env::from_process();
            if save_config {
                match config::save_merged(&source, &env, &overrides) {
                    Ok(saved) => {
                        println!(
                            "saved preferences to {}",
                            saved.display().to_string().escape_default()
                        );
                        ExitCode::SUCCESS
                    }
                    Err(error) => {
                        eprintln!("naf: {}", error.to_string().escape_default());
                        ExitCode::FAILURE
                    }
                }
            } else {
                match config::resolve(&source, &env, &overrides) {
                    Ok(options) => report_io(app::run(path, options)),
                    Err(error) => {
                        eprintln!("naf: {}", error.to_string().escape_default());
                        ExitCode::FAILURE
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ColorMode, IconMode, ThemeKind};
    use std::path::Path;

    fn parse(args: &[&str]) -> Result<Command, &'static str> {
        parse_args(args.iter().copied().map(OsString::from))
    }

    #[test]
    fn cli_flags_and_equals_forms_are_order_independent() {
        let a = parse(&[
            "--icons=nerd",
            "notes.txt",
            "--clipboard",
            "system",
            "--theme",
            "light",
            "--color=16",
        ])
        .unwrap();
        let b = parse(&[
            "--color",
            "16",
            "--theme=light",
            "--clipboard=system",
            "--icons",
            "nerd",
            "notes.txt",
        ])
        .unwrap();
        assert_eq!(a, b);

        let Command::Editor {
            path,
            overrides,
            source,
            save_config,
        } = a
        else {
            panic!("expected editor command");
        };
        assert_eq!(path, Some(PathBuf::from("notes.txt")));
        assert_eq!(source, config::Source::Default);
        assert!(!save_config);
        assert_eq!(overrides.options.theme, ThemeKind::Light);
        assert_eq!(overrides.options.colors, ColorMode::Ansi16);
        assert_eq!(overrides.options.icons, IconMode::Nerd);
        assert_eq!(overrides.options.clipboard, ClipboardMode::System);
        assert!(overrides.theme && overrides.color && overrides.icons && overrides.clipboard);
    }

    #[test]
    fn preserves_double_dash_path_and_config_flags() {
        let cmd = parse(&["--config=/tmp/custom.toml", "--save-config"]).unwrap();
        assert!(matches!(
            &cmd,
            Command::Editor {
                path: None,
                source: config::Source::Explicit(p),
                save_config: true,
                ..
            } if p.as_path() == Path::new("/tmp/custom.toml")
        ));

        let dashed = parse(&["--no-config", "--", "-leading.txt"]).unwrap();
        assert!(matches!(
            &dashed,
            Command::Editor {
                path: Some(p),
                source: config::Source::Disabled,
                save_config: false,
                ..
            } if p.as_path() == Path::new("-leading.txt")
        ));
    }

    #[test]
    fn rejects_conflicting_and_malformed_cli_combinations() {
        assert!(parse(&["--config", "a.toml", "--no-config"]).is_err());
        assert!(parse(&["--no-config", "--config=a.toml"]).is_err());
        assert!(parse(&["--save-config", "--no-config"]).is_err());
        assert!(parse(&["--save-config", "file.txt"]).is_err());
        assert!(parse(&["--save-config", "--", "file.txt"]).is_err());
        assert!(parse(&["--config", "a.toml", "--config", "b.toml"]).is_err());
        assert!(parse(&["--config="]).is_err());
        assert!(parse(&["--config"]).is_err());
        assert!(parse(&["--probe", "--save-config"]).is_err());
        assert!(parse(&["--probe", "--config", "a.toml"]).is_err());
        assert!(parse(&["--probe", "--no-config"]).is_err());
        assert!(parse(&["--probe", "--clipboard", "internal"]).is_err());
        assert!(parse(&["--clipboard", "bogus"]).is_err());
        assert_eq!(parse(&["--probe"]).unwrap(), Command::Probe);
        assert_eq!(
            parse(&["--help", "--config", "a.toml"]).unwrap(),
            Command::Help
        );
    }
}
