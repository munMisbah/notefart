//! Semantic color themes, capability degradation, and icon profiles.
//!
//! Note on ANSI 16 and `Terminal` themes: terminal emulators and user color
//! schemes control the actual RGB values of ANSI palette slots `0..=15` and the
//! default foreground/background. Contrast ratios therefore cannot be guaranteed
//! in `ColorMode::Ansi16` or `ThemeKind::Terminal`; non-color reverse/bold cues
//! are always applied for selection and focus in those modes (and in
//! `ColorMode::None`) so active state stays distinguishable.
//!
//! Nerd Fonts Codicons mappings are sourced directly from upstream Nerd Fonts
//! v3.5.1 metadata (Codicons v0.0.45):
//! - <https://raw.githubusercontent.com/ryanoasis/nerd-fonts/master/glyphnames.json>
//! - <https://raw.githubusercontent.com/ryanoasis/nerd-fonts/master/bin/scripts/lib/i_cod.sh>

use std::{env, ffi::OsStr, path::Path, str::FromStr};

use ratatui::style::{Color, Modifier, Style};

use crate::clipboard::ClipboardMode;
use crate::highlight::Token;

/// Visual theme family selected explicitly by user options.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ThemeKind {
    Dark,
    Light,
    Terminal,
}

impl FromStr for ThemeKind {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("dark") {
            Ok(Self::Dark)
        } else if s.eq_ignore_ascii_case("light") {
            Ok(Self::Light)
        } else if s.eq_ignore_ascii_case("terminal") {
            Ok(Self::Terminal)
        } else {
            Err("invalid theme (expected dark, light, or terminal)")
        }
    }
}

impl ThemeKind {
    /// Canonical preference name accepted by [`FromStr`].
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::Terminal => "terminal",
        }
    }
}

/// Terminal color capability mode.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ColorMode {
    Auto,
    Truecolor,
    Ansi256,
    Ansi16,
    None,
}

impl FromStr for ColorMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("auto") {
            Ok(Self::Auto)
        } else if s.eq_ignore_ascii_case("truecolor")
            || s.eq_ignore_ascii_case("24bit")
            || s.eq_ignore_ascii_case("rgb")
        {
            Ok(Self::Truecolor)
        } else if s.eq_ignore_ascii_case("256") || s.eq_ignore_ascii_case("ansi256") {
            Ok(Self::Ansi256)
        } else if s.eq_ignore_ascii_case("16") || s.eq_ignore_ascii_case("ansi16") {
            Ok(Self::Ansi16)
        } else if s.eq_ignore_ascii_case("none") {
            Ok(Self::None)
        } else {
            Err("invalid color mode (expected auto, truecolor, 256, 16, or none)")
        }
    }
}

impl ColorMode {
    /// Canonical preference name accepted by [`FromStr`].
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Truecolor => "truecolor",
            Self::Ansi256 => "256",
            Self::Ansi16 => "16",
            Self::None => "none",
        }
    }
}

/// Icon rendering profile. Nerd Fonts glyphs are strictly opt-in.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum IconMode {
    Unicode,
    Nerd,
    Ascii,
}

impl FromStr for IconMode {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.eq_ignore_ascii_case("unicode") {
            Ok(Self::Unicode)
        } else if s.eq_ignore_ascii_case("nerd") {
            Ok(Self::Nerd)
        } else if s.eq_ignore_ascii_case("ascii") {
            Ok(Self::Ascii)
        } else {
            Err("invalid icon profile (expected unicode, nerd, or ascii)")
        }
    }
}

impl IconMode {
    /// Canonical preference name accepted by [`FromStr`].
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unicode => "unicode",
            Self::Nerd => "nerd",
            Self::Ascii => "ascii",
        }
    }
}

/// Theme, color, icon, and clipboard options resolved from defaults, user configuration, and CLI flags.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Options {
    pub theme: ThemeKind,
    pub colors: ColorMode,
    pub icons: IconMode,
    pub clipboard: ClipboardMode,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            theme: ThemeKind::Dark,
            colors: ColorMode::Auto,
            icons: IconMode::Unicode,
            clipboard: ClipboardMode::Auto,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct SyntaxPalette {
    plain: Color,
    comment: Color,
    keyword: Color,
    string: Color,
    number: Color,
    function: Color,
    ty: Color,
}

/// Precomputed UI and syntax color palette ready for per-frame rendering.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Theme {
    pub canvas: Color,
    pub fg: Color,
    pub muted: Color,
    pub border: Color,
    pub accent: Color,
    pub selection: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    syntax: SyntaxPalette,
    uses_reverse_cues: bool,
}

impl Theme {
    /// Builds a precomputed theme palette for `kind` and `mode`.
    ///
    /// When `mode` is [`ColorMode::Auto`], environment variables (`NO_COLOR`,
    /// `TERM`, `COLORTERM`) are inspected once. Explicit non-`Auto` modes
    /// always override the environment.
    pub fn new(kind: ThemeKind, mode: ColorMode) -> Self {
        let resolved = if mode == ColorMode::Auto {
            let no_color = env::var_os("NO_COLOR");
            let term = env::var_os("TERM");
            let colorterm = env::var_os("COLORTERM");
            resolve_color_mode(
                mode,
                no_color.as_deref(),
                term.as_deref().and_then(OsStr::to_str),
                colorterm.as_deref().and_then(OsStr::to_str),
            )
        } else {
            mode
        };
        Self::from_resolved(kind, resolved)
    }

    const fn from_resolved(kind: ThemeKind, resolved: ColorMode) -> Self {
        match (kind, resolved) {
            (_, ColorMode::None) => PALETTE_NONE,
            (ThemeKind::Terminal, _) => PALETTE_TERMINAL,
            (ThemeKind::Dark, ColorMode::Truecolor) => PALETTE_DARK_TRUECOLOR,
            (ThemeKind::Light, ColorMode::Truecolor) => PALETTE_LIGHT_TRUECOLOR,
            (ThemeKind::Dark, ColorMode::Ansi256) => PALETTE_DARK_256,
            (ThemeKind::Light, ColorMode::Ansi256) => PALETTE_LIGHT_256,
            (ThemeKind::Dark, ColorMode::Ansi16 | ColorMode::Auto) => PALETTE_DARK_16,
            (ThemeKind::Light, ColorMode::Ansi16 | ColorMode::Auto) => PALETTE_LIGHT_16,
        }
    }

    /// Returns the precomputed foreground color for a semantic syntax token.
    pub const fn syntax(&self, token: Token) -> Color {
        match token {
            Token::Plain => self.syntax.plain,
            Token::Comment => self.syntax.comment,
            Token::Keyword => self.syntax.keyword,
            Token::String => self.syntax.string,
            Token::Number => self.syntax.number,
            Token::Function => self.syntax.function,
            Token::Type => self.syntax.ty,
            Token::Heading => self.syntax.function,
            Token::Emphasis => self.syntax.keyword,
            Token::Code => self.syntax.string,
            Token::Link | Token::Quote => self.syntax.ty,
            Token::Marker => self.syntax.number,
        }
    }

    /// Style for selected text and selected explorer rows.
    ///
    /// Uses `Modifier::REVERSED` in monochrome, ANSI 16, and `Terminal` modes
    /// where explicit background contrast cannot be relied on.
    pub fn selection_style(&self) -> Style {
        let style = Style::default().fg(self.fg).bg(self.selection);
        if self.uses_reverse_cues {
            style.add_modifier(Modifier::REVERSED)
        } else {
            style
        }
    }

    /// Whether explicit background colors cannot be trusted (monochrome,
    /// ANSI 16, and `Terminal` palettes). Renderers use this to fall back to
    /// reverse/bold cues instead of distinct background colors.
    pub(crate) const fn uses_reverse_cues(&self) -> bool {
        self.uses_reverse_cues
    }

    /// Style for focused interactive controls (such as active dialog buttons).
    ///
    /// Always applies `Modifier::BOLD`, and adds `Modifier::REVERSED` in
    /// monochrome, ANSI 16, and `Terminal` modes so focus remains legible
    /// without relying on colors alone.
    pub fn focus_style(&self) -> Style {
        if self.uses_reverse_cues {
            Style::default()
                .fg(self.fg)
                .bg(self.canvas)
                .add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.canvas)
                .bg(self.accent)
                .add_modifier(Modifier::BOLD)
        }
    }
}

fn resolve_color_mode(
    mode: ColorMode,
    no_color: Option<&OsStr>,
    term: Option<&str>,
    colorterm: Option<&str>,
) -> ColorMode {
    if mode != ColorMode::Auto {
        return mode;
    }
    if no_color.is_some_and(|value| !value.is_empty()) {
        return ColorMode::None;
    }
    if term.is_some_and(|value| value.trim().eq_ignore_ascii_case("dumb")) {
        return ColorMode::None;
    }
    if colorterm.is_some_and(|value| {
        let trimmed = value.trim();
        trimmed.eq_ignore_ascii_case("truecolor") || trimmed.eq_ignore_ascii_case("24bit")
    }) {
        return ColorMode::Truecolor;
    }
    if term.is_some_and(|value| {
        value
            .as_bytes()
            .windows(8)
            .any(|window| window.eq_ignore_ascii_case(b"256color"))
    }) {
        return ColorMode::Ansi256;
    }
    ColorMode::Ansi16
}

// Notefart Dark (exact docs/DESIGN.md truecolor hex values)
const PALETTE_DARK_TRUECOLOR: Theme = Theme {
    canvas: Color::from_u32(0x17_1B_24),
    fg: Color::from_u32(0xDC_E3_EE),
    muted: Color::from_u32(0xA4_AF_C0),
    border: Color::from_u32(0x74_82_9A),
    accent: Color::from_u32(0x8B_C5_FF),
    selection: Color::from_u32(0x34_48_66),
    success: Color::from_u32(0x91_D5_A5),
    warning: Color::from_u32(0xF2_CC_8F),
    error: Color::from_u32(0xFF_9E_AE),
    syntax: SyntaxPalette {
        plain: Color::from_u32(0xDC_E3_EE),
        comment: Color::from_u32(0xA4_AF_C0),
        keyword: Color::from_u32(0xC9_A7_F5),
        string: Color::from_u32(0x91_D5_A5),
        number: Color::from_u32(0xF2_CC_8F),
        function: Color::from_u32(0x8B_C5_FF),
        ty: Color::from_u32(0x8B_D5_CA),
    },
    uses_reverse_cues: false,
};

// Notefart Light (exact docs/DESIGN.md truecolor hex values)
const PALETTE_LIGHT_TRUECOLOR: Theme = Theme {
    canvas: Color::from_u32(0xF5_F7_FB),
    fg: Color::from_u32(0x20_2A_3A),
    muted: Color::from_u32(0x53_61_76),
    border: Color::from_u32(0x69_77_8B),
    accent: Color::from_u32(0x19_5C_A8),
    selection: Color::from_u32(0xC7_DC_F6),
    success: Color::from_u32(0x22_62_3C),
    warning: Color::from_u32(0x80_53_00),
    error: Color::from_u32(0xA5_28_40),
    syntax: SyntaxPalette {
        plain: Color::from_u32(0x20_2A_3A),
        comment: Color::from_u32(0x53_61_76),
        keyword: Color::from_u32(0x6D_3B_A3),
        string: Color::from_u32(0x22_62_3C),
        number: Color::from_u32(0x8B_4B_12),
        function: Color::from_u32(0x19_5C_A8),
        ty: Color::from_u32(0x0A_65_6B),
    },
    uses_reverse_cues: false,
};

// Precomputed xterm-256 palette for Notefart Dark.
// Adjusted from raw Euclidean RGB nearest where needed for hue and contrast:
// - selection uses 24 (#005F87) so selection stays visibly blue and >=4.5:1 with fg 254.
// - border uses 103 (#8787AF) to keep cool slate separation >=3.0:1 across all surfaces.
const PALETTE_DARK_256: Theme = Theme {
    canvas: Color::Indexed(234),
    fg: Color::Indexed(254),
    muted: Color::Indexed(249),
    border: Color::Indexed(103),
    accent: Color::Indexed(117),
    selection: Color::Indexed(24),
    success: Color::Indexed(115),
    warning: Color::Indexed(222),
    error: Color::Indexed(217),
    syntax: SyntaxPalette {
        plain: Color::Indexed(254),
        comment: Color::Indexed(249),
        keyword: Color::Indexed(183),
        string: Color::Indexed(115),
        number: Color::Indexed(222),
        function: Color::Indexed(117),
        ty: Color::Indexed(116),
    },
    uses_reverse_cues: false,
};

// Precomputed xterm-256 palette for Notefart Light.
// Adjusted from raw Euclidean RGB nearest to guarantee hue & >=4.5:1 contrast:
// - success/string use 22 (#005F00) instead of gray 238 or colliding with type 23.
// - keyword uses 91 (#8700AF) instead of 61 (which fell below 4.5:1 on raised 254).
// - selection uses 153 (#AFD7FF) for sky-blue separation from surface/raised.
const PALETTE_LIGHT_256: Theme = Theme {
    canvas: Color::Indexed(231),
    fg: Color::Indexed(236),
    muted: Color::Indexed(60),
    border: Color::Indexed(66),
    accent: Color::Indexed(25),
    selection: Color::Indexed(153),
    success: Color::Indexed(22),
    warning: Color::Indexed(94),
    error: Color::Indexed(125),
    syntax: SyntaxPalette {
        plain: Color::Indexed(236),
        comment: Color::Indexed(60),
        keyword: Color::Indexed(91),
        string: Color::Indexed(22),
        number: Color::Indexed(94),
        function: Color::Indexed(25),
        ty: Color::Indexed(23),
    },
    uses_reverse_cues: false,
};

const PALETTE_DARK_16: Theme = Theme {
    canvas: Color::Reset,
    fg: Color::Reset,
    muted: Color::DarkGray,
    border: Color::DarkGray,
    accent: Color::LightBlue,
    selection: Color::Reset,
    success: Color::LightGreen,
    warning: Color::LightYellow,
    error: Color::LightRed,
    syntax: SyntaxPalette {
        plain: Color::Reset,
        comment: Color::DarkGray,
        keyword: Color::LightMagenta,
        string: Color::LightGreen,
        number: Color::LightYellow,
        function: Color::LightBlue,
        ty: Color::LightCyan,
    },
    uses_reverse_cues: true,
};

const PALETTE_LIGHT_16: Theme = Theme {
    canvas: Color::Reset,
    fg: Color::Reset,
    muted: Color::DarkGray,
    border: Color::DarkGray,
    accent: Color::Blue,
    selection: Color::Reset,
    success: Color::Green,
    warning: Color::Yellow,
    error: Color::Red,
    syntax: SyntaxPalette {
        plain: Color::Reset,
        comment: Color::DarkGray,
        keyword: Color::Magenta,
        string: Color::Green,
        number: Color::Yellow,
        function: Color::Blue,
        ty: Color::Cyan,
    },
    uses_reverse_cues: true,
};

const PALETTE_TERMINAL: Theme = Theme {
    canvas: Color::Reset,
    fg: Color::Reset,
    muted: Color::DarkGray,
    border: Color::DarkGray,
    accent: Color::Cyan,
    selection: Color::Reset,
    success: Color::Green,
    warning: Color::Yellow,
    error: Color::Red,
    syntax: SyntaxPalette {
        plain: Color::Reset,
        comment: Color::DarkGray,
        keyword: Color::Magenta,
        string: Color::Green,
        number: Color::Yellow,
        function: Color::Cyan,
        ty: Color::Blue,
    },
    uses_reverse_cues: true,
};

const PALETTE_NONE: Theme = Theme {
    canvas: Color::Reset,
    fg: Color::Reset,
    muted: Color::Reset,
    border: Color::Reset,
    accent: Color::Reset,
    selection: Color::Reset,
    success: Color::Reset,
    warning: Color::Reset,
    error: Color::Reset,
    syntax: SyntaxPalette {
        plain: Color::Reset,
        comment: Color::Reset,
        keyword: Color::Reset,
        string: Color::Reset,
        number: Color::Reset,
        function: Color::Reset,
        ty: Color::Reset,
    },
    uses_reverse_cues: true,
};

/// Explorer/entry classification used to pick an icon or marker.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum IconKind {
    File,
    Directory { expanded: bool },
    Symlink,
    Other,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum FileCategory {
    Text,
    Code,
    Config,
    Media,
    Binary,
    Generic,
}

/// Frame, dialog, or dock role for integrated chrome icons.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum ChromeIcon {
    Workspace,
    Explorer,
    Editor,
    Status,
    Search,
    Help,
}

// Verified Nerd Fonts v3.5.1 glyphs sourced from upstream glyphnames.json:
// filled Material Design icons (md-*) for chrome/entry roles, plus Codicons
// v0.0.45 for disclosure chevrons and the unknown-entry question fallback:
const NERD_CHEVRON_RIGHT: &str = "\u{EAB6}"; // cod-chevron_right
const NERD_CHEVRON_DOWN: &str = "\u{EAB4}"; // cod-chevron_down
const NERD_QUESTION: &str = "\u{EB32}"; // cod-question
const NERD_FOLDER: &str = "\u{F024B}"; // md-folder
const NERD_FOLDER_OPENED: &str = "\u{F0770}"; // md-folder_open
const NERD_FOLDER_HOME: &str = "\u{F10B5}"; // md-folder_home
const NERD_FILE: &str = "\u{F0214}"; // md-file
const NERD_FILE_DOCUMENT: &str = "\u{F0219}"; // md-file_document
const NERD_FILE_CODE: &str = "\u{F022E}"; // md-file_code
const NERD_FILE_COG: &str = "\u{F107B}"; // md-file_cog
const NERD_IMAGE: &str = "\u{F02E9}"; // md-image
const NERD_LINK: &str = "\u{F0337}"; // md-link
const NERD_CLOSE: &str = "\u{F0156}"; // md-close
const NERD_PENCIL: &str = "\u{F03EB}"; // md-pencil
const NERD_INFORMATION: &str = "\u{F02FC}"; // md-information
const NERD_MAGNIFY: &str = "\u{F0349}"; // md-magnify
const NERD_HELP_CIRCLE: &str = "\u{F02D7}"; // md-help_circle

/// Fixed horizontal slot width (in terminal columns) reserved for entry icons,
/// excluding the trailing 1-column separator drawn by the UI.
pub const fn icon_slot(mode: IconMode) -> u16 {
    match mode {
        IconMode::Unicode | IconMode::Nerd => 2,
        IconMode::Ascii => 3,
    }
}

/// Folder disclosure chevron for `mode`.
pub const fn chevron(expanded: bool, mode: IconMode) -> &'static str {
    match mode {
        IconMode::Nerd => {
            if expanded {
                NERD_CHEVRON_DOWN
            } else {
                NERD_CHEVRON_RIGHT
            }
        }
        IconMode::Unicode => {
            if expanded {
                "▾"
            } else {
                "▸"
            }
        }
        IconMode::Ascii => {
            if expanded {
                "v"
            } else {
                ">"
            }
        }
    }
}

/// Tab close button icon for `mode`.
pub const fn close_icon(mode: IconMode) -> &'static str {
    match mode {
        IconMode::Nerd => NERD_CLOSE,
        IconMode::Unicode => "×",
        IconMode::Ascii => "x",
    }
}

/// Integrated icon for frame titles, header, dock, and search/help chrome.
pub fn chrome_icon(role: ChromeIcon, mode: IconMode) -> &'static str {
    match (role, mode) {
        (ChromeIcon::Workspace, IconMode::Nerd) => NERD_FOLDER_HOME,
        (ChromeIcon::Workspace, IconMode::Unicode) => "◈",
        (ChromeIcon::Workspace, IconMode::Ascii) => "[W]",

        (ChromeIcon::Explorer, IconMode::Nerd) => NERD_FOLDER,
        (ChromeIcon::Explorer, IconMode::Unicode) => "▤",
        (ChromeIcon::Explorer, IconMode::Ascii) => "[E]",

        (ChromeIcon::Editor, IconMode::Nerd) => NERD_PENCIL,
        (ChromeIcon::Editor, IconMode::Unicode) => "✎",
        (ChromeIcon::Editor, IconMode::Ascii) => "[P]",

        (ChromeIcon::Status, IconMode::Nerd) => NERD_INFORMATION,
        (ChromeIcon::Status, IconMode::Unicode) => "◎",
        (ChromeIcon::Status, IconMode::Ascii) => "[S]",

        (ChromeIcon::Search, IconMode::Nerd) => NERD_MAGNIFY,
        (ChromeIcon::Search, IconMode::Unicode) => "⌕",
        (ChromeIcon::Search, IconMode::Ascii) => "[/]",

        (ChromeIcon::Help, IconMode::Nerd) => NERD_HELP_CIRCLE,
        (ChromeIcon::Help, IconMode::Unicode) => "?",
        (ChromeIcon::Help, IconMode::Ascii) => "[?]",
    }
}

/// Icon or text marker for an explorer entry.
pub fn icon(path: &Path, kind: IconKind, mode: IconMode) -> &'static str {
    match (kind, mode) {
        (IconKind::Directory { expanded: false }, IconMode::Nerd) => NERD_FOLDER,
        (IconKind::Directory { expanded: true }, IconMode::Nerd) => NERD_FOLDER_OPENED,
        (IconKind::Directory { expanded: false }, IconMode::Unicode) => "■",
        (IconKind::Directory { expanded: true }, IconMode::Unicode) => "□",
        (IconKind::Directory { .. }, IconMode::Ascii) => "[D]",

        (IconKind::Symlink, IconMode::Nerd) => NERD_LINK,
        (IconKind::Symlink, IconMode::Unicode) => "↪",
        (IconKind::Symlink, IconMode::Ascii) => "@",

        (IconKind::Other, IconMode::Nerd) => NERD_QUESTION,
        (IconKind::Other, IconMode::Unicode) => "?",
        (IconKind::Other, IconMode::Ascii) => "[?]",

        (IconKind::File, _) => match (classify_path(path), mode) {
            (FileCategory::Text, IconMode::Nerd) => NERD_FILE_DOCUMENT,
            (FileCategory::Generic, IconMode::Nerd) => NERD_FILE,
            (FileCategory::Code, IconMode::Nerd) => NERD_FILE_CODE,
            (FileCategory::Config, IconMode::Nerd) => NERD_FILE_COG,
            (FileCategory::Media, IconMode::Nerd) => NERD_IMAGE,
            (FileCategory::Binary, IconMode::Nerd) => NERD_FILE,

            (FileCategory::Text, IconMode::Unicode) => "≡",
            (FileCategory::Generic, IconMode::Unicode) => "▫",
            (FileCategory::Code, IconMode::Unicode) => "λ",
            (FileCategory::Config, IconMode::Unicode) => "⌘",
            (FileCategory::Media, IconMode::Unicode) => "▣",
            (FileCategory::Binary, IconMode::Unicode) => "※",

            (FileCategory::Text | FileCategory::Generic, IconMode::Ascii) => "[F]",
            (FileCategory::Code, IconMode::Ascii) => "[C]",
            (FileCategory::Config, IconMode::Ascii) => "[=]",
            (FileCategory::Media | FileCategory::Binary, IconMode::Ascii) => "[B]",
        },
    }
}

/// Semantic color for an explorer or tab entry icon based on its path and classification.
pub fn icon_color(path: &Path, kind: IconKind, theme: &Theme) -> Color {
    match kind {
        IconKind::Directory { .. } => theme.accent,
        IconKind::Symlink => theme.syntax(Token::Keyword),
        IconKind::Other => theme.muted,
        IconKind::File => match classify_path(path) {
            FileCategory::Text => theme.syntax(Token::Plain),
            FileCategory::Code => theme.syntax(Token::Function),
            FileCategory::Config => theme.warning,
            FileCategory::Media => theme.syntax(Token::Type),
            FileCategory::Binary => theme.error,
            FileCategory::Generic => theme.fg,
        },
    }
}

fn classify_path(path: &Path) -> FileCategory {
    if let Some(name) = path.file_name().and_then(OsStr::to_str)
        && let Some(cat) = classify_filename(name)
    {
        return cat;
    }
    match path.extension().and_then(OsStr::to_str) {
        Some(ext) => classify_extension(ext),
        None => FileCategory::Generic,
    }
}

fn classify_filename(name: &str) -> Option<FileCategory> {
    if name.eq_ignore_ascii_case("makefile")
        || name.eq_ignore_ascii_case("gnumakefile")
        || name.eq_ignore_ascii_case("justfile")
        || name.eq_ignore_ascii_case("dockerfile")
        || name.eq_ignore_ascii_case("containerfile")
        || name.eq_ignore_ascii_case("vagrantfile")
        || name.eq_ignore_ascii_case("procfile")
        || name.eq_ignore_ascii_case("rakefile")
        || name.eq_ignore_ascii_case("gemfile")
        || name.eq_ignore_ascii_case("brewfile")
        || name.eq_ignore_ascii_case("cmakelists.txt")
        || name.eq_ignore_ascii_case("cargo.lock")
        || name.eq_ignore_ascii_case("package-lock.json")
        || name.eq_ignore_ascii_case("yarn.lock")
        || name.eq_ignore_ascii_case("pnpm-lock.yaml")
        || name.eq_ignore_ascii_case("poetry.lock")
        || name.eq_ignore_ascii_case("flake.lock")
        || name.eq_ignore_ascii_case(".gitignore")
        || name.eq_ignore_ascii_case(".gitattributes")
        || name.eq_ignore_ascii_case(".gitmodules")
        || name.eq_ignore_ascii_case(".dockerignore")
        || name.eq_ignore_ascii_case(".ignore")
        || name.eq_ignore_ascii_case(".editorconfig")
        || name.eq_ignore_ascii_case(".prettierrc")
        || name.eq_ignore_ascii_case(".eslintrc")
        || name.eq_ignore_ascii_case(".npmrc")
        || name.eq_ignore_ascii_case(".bashrc")
        || name.eq_ignore_ascii_case(".zshrc")
        || name.eq_ignore_ascii_case(".profile")
        || name.eq_ignore_ascii_case(".env")
        || (name.len() > 5 && name.as_bytes()[..5].eq_ignore_ascii_case(b".env."))
    {
        return Some(FileCategory::Config);
    }
    if name.eq_ignore_ascii_case("readme")
        || name.eq_ignore_ascii_case("license")
        || name.eq_ignore_ascii_case("licence")
        || name.eq_ignore_ascii_case("copying")
        || name.eq_ignore_ascii_case("changelog")
        || name.eq_ignore_ascii_case("authors")
        || name.eq_ignore_ascii_case("contributors")
        || name.eq_ignore_ascii_case("notice")
        || name.eq_ignore_ascii_case("todo")
    {
        return Some(FileCategory::Text);
    }
    None
}

fn classify_extension(ext: &str) -> FileCategory {
    let bytes = ext.as_bytes();
    if bytes.len() > 16 {
        return FileCategory::Generic;
    }
    let mut lower = [0u8; 16];
    for (dst, &src) in lower[..bytes.len()].iter_mut().zip(bytes) {
        *dst = src.to_ascii_lowercase();
    }
    match &lower[..bytes.len()] {
        b"rs" | b"c" | b"h" | b"cc" | b"cpp" | b"cxx" | b"hpp" | b"hh" | b"hxx" | b"py"
        | b"pyi" | b"pyx" | b"js" | b"mjs" | b"cjs" | b"jsx" | b"ts" | b"mts" | b"cts" | b"tsx"
        | b"go" | b"java" | b"kt" | b"kts" | b"scala" | b"sc" | b"swift" | b"rb" | b"php"
        | b"pl" | b"pm" | b"lua" | b"zig" | b"nim" | b"cr" | b"ex" | b"exs" | b"erl" | b"hrl"
        | b"hs" | b"lhs" | b"ml" | b"mli" | b"fs" | b"fsi" | b"fsx" | b"cs" | b"vb" | b"r"
        | b"jl" | b"dart" | b"groovy" | b"clj" | b"cljs" | b"cljc" | b"edn" | b"lisp" | b"el"
        | b"scm" | b"rkt" | b"sh" | b"bash" | b"zsh" | b"fish" | b"ps1" | b"psm1" | b"bat"
        | b"cmd" | b"awk" | b"sed" | b"sql" | b"html" | b"htm" | b"css" | b"scss" | b"sass"
        | b"less" | b"vue" | b"svelte" | b"astro" | b"wat" | b"glsl" | b"wgsl" | b"hlsl"
        | b"proto" | b"graphql" | b"gql" | b"nix" | b"tf" | b"hcl" | b"asm" | b"s" | b"v"
        | b"sv" | b"vhd" | b"vhdl" => FileCategory::Code,

        b"toml" | b"yaml" | b"yml" | b"json" | b"jsonc" | b"json5" | b"ini" | b"cfg" | b"conf"
        | b"config" | b"env" | b"properties" | b"plist" | b"xml" | b"lock" | b"rc" | b"cmake"
        | b"mk" | b"ninja" | b"bazel" | b"bzl" | b"gradle" | b"editorconfig" | b"service"
        | b"desktop" => FileCategory::Config,

        b"txt" | b"md" | b"markdown" | b"mdx" | b"rst" | b"adoc" | b"asciidoc" | b"org"
        | b"tex" | b"bib" | b"rtf" | b"log" | b"csv" | b"tsv" | b"diff" | b"patch" | b"nfo"
        | b"me" | b"1" | b"man" => FileCategory::Text,

        b"png" | b"jpg" | b"jpeg" | b"gif" | b"webp" | b"bmp" | b"ico" | b"icns" | b"tif"
        | b"tiff" | b"avif" | b"heic" | b"svg" | b"svgz" | b"mp3" | b"wav" | b"flac" | b"ogg"
        | b"opus" | b"aac" | b"m4a" | b"mp4" | b"mkv" | b"webm" | b"avi" | b"mov" | b"wmv" => {
            FileCategory::Media
        }

        b"bin" | b"dat" | b"exe" | b"dll" | b"so" | b"dylib" | b"a" | b"lib" | b"o" | b"obj"
        | b"rlib" | b"rmeta" | b"class" | b"jar" | b"war" | b"pyc" | b"pyo" | b"wasm" | b"elf"
        | b"apk" | b"deb" | b"rpm" | b"appimage" | b"msi" | b"dmg" | b"iso" | b"img" | b"zip"
        | b"tar" | b"gz" | b"tgz" | b"bz2" | b"xz" | b"zst" | b"7z" | b"rar" | b"pdf" | b"doc"
        | b"docx" | b"xls" | b"xlsx" | b"ppt" | b"pptx" | b"odt" | b"ods" | b"odp" | b"ttf"
        | b"otf" | b"woff" | b"woff2" | b"eot" | b"db" | b"sqlite" | b"sqlite3" | b"pdb"
        | b"core" | b"dump" => FileCategory::Binary,

        _ => FileCategory::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthStr;

    fn xterm256_to_rgb(idx: u8) -> (u8, u8, u8) {
        const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];
        assert!(idx >= 16, "xterm-256 palette entries must avoid ANSI 0..15");
        if idx < 232 {
            let n = idx - 16;
            (
                CUBE[(n / 36) as usize],
                CUBE[((n / 6) % 6) as usize],
                CUBE[(n % 6) as usize],
            )
        } else {
            let v = 8 + (idx - 232) * 10;
            (v, v, v)
        }
    }

    fn color_rgb(color: Color) -> (u8, u8, u8) {
        match color {
            Color::Rgb(r, g, b) => (r, g, b),
            Color::Indexed(idx) => xterm256_to_rgb(idx),
            other => panic!("expected RGB or xterm-256 color, got {other:?}"),
        }
    }

    fn channel_lum(c: u8) -> f64 {
        let s = f64::from(c) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }

    fn contrast_ratio(a: Color, b: Color) -> f64 {
        let (ar, ag, ab) = color_rgb(a);
        let (br, bg, bb) = color_rgb(b);
        let la = 0.2126 * channel_lum(ar) + 0.7152 * channel_lum(ag) + 0.0722 * channel_lum(ab);
        let lb = 0.2126 * channel_lum(br) + 0.7152 * channel_lum(bg) + 0.0722 * channel_lum(bb);
        let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    fn assert_theme_contrasts(theme: &Theme, label: &str) {
        let surfaces = [("canvas", theme.canvas)];
        let text_roles = [
            ("fg", theme.fg),
            ("muted", theme.muted),
            ("accent", theme.accent),
            ("success", theme.success),
            ("warning", theme.warning),
            ("error", theme.error),
            ("syntax.plain", theme.syntax(Token::Plain)),
            ("syntax.comment", theme.syntax(Token::Comment)),
            ("syntax.keyword", theme.syntax(Token::Keyword)),
            ("syntax.string", theme.syntax(Token::String)),
            ("syntax.number", theme.syntax(Token::Number)),
            ("syntax.function", theme.syntax(Token::Function)),
            ("syntax.type", theme.syntax(Token::Type)),
            ("syntax.heading", theme.syntax(Token::Heading)),
            ("syntax.emphasis", theme.syntax(Token::Emphasis)),
            ("syntax.code", theme.syntax(Token::Code)),
            ("syntax.link", theme.syntax(Token::Link)),
            ("syntax.quote", theme.syntax(Token::Quote)),
            ("syntax.marker", theme.syntax(Token::Marker)),
        ];

        for (bg_name, bg) in surfaces {
            for (fg_name, fg) in text_roles {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= 4.5,
                    "{label}: {fg_name} on {bg_name} contrast {ratio:.2} < 4.5"
                );
            }
            for (border_name, border) in [("border", theme.border), ("accent", theme.accent)] {
                let ratio = contrast_ratio(border, bg);
                assert!(
                    ratio >= 3.0,
                    "{label}: {border_name} on {bg_name} contrast {ratio:.2} < 3.0"
                );
            }
        }

        let sel_ratio = contrast_ratio(theme.fg, theme.selection);
        assert!(
            sel_ratio >= 4.5,
            "{label}: fg on selection contrast {sel_ratio:.2} < 4.5"
        );

        let focus_ratio = contrast_ratio(theme.canvas, theme.accent);
        assert!(
            focus_ratio >= 4.5,
            "{label}: canvas on accent contrast {focus_ratio:.2} < 4.5"
        );

        let match_ratio = contrast_ratio(theme.canvas, theme.warning);
        assert!(
            match_ratio >= 4.5,
            "{label}: canvas on warning contrast {match_ratio:.2} < 4.5"
        );

        assert_ne!(
            theme.selection, theme.canvas,
            "{label}: selection == canvas"
        );
    }

    #[test]
    fn truecolor_contrast_targets_pass() {
        assert_theme_contrasts(
            &Theme::from_resolved(ThemeKind::Dark, ColorMode::Truecolor),
            "Dark/Truecolor",
        );
        assert_theme_contrasts(
            &Theme::from_resolved(ThemeKind::Light, ColorMode::Truecolor),
            "Light/Truecolor",
        );
    }

    #[test]
    fn ansi256_contrast_targets_and_semantic_distinction_pass() {
        let dark = Theme::from_resolved(ThemeKind::Dark, ColorMode::Ansi256);
        let light = Theme::from_resolved(ThemeKind::Light, ColorMode::Ansi256);
        assert_theme_contrasts(&dark, "Dark/256");
        assert_theme_contrasts(&light, "Light/256");

        for (name, theme) in [("Dark/256", dark), ("Light/256", light)] {
            assert_ne!(
                theme.syntax(Token::String),
                theme.syntax(Token::Type),
                "{name}: string and type collapsed"
            );
            assert_ne!(
                theme.syntax(Token::Keyword),
                theme.syntax(Token::Comment),
                "{name}: keyword and comment collapsed"
            );
            assert_ne!(theme.success, theme.error, "{name}: success == error");
        }
    }

    #[test]
    fn monochrome_and_terminal_preserve_non_color_cues() {
        let all_tokens = [
            Token::Plain,
            Token::Comment,
            Token::Keyword,
            Token::String,
            Token::Number,
            Token::Function,
            Token::Type,
            Token::Heading,
            Token::Emphasis,
            Token::Code,
            Token::Link,
            Token::Quote,
            Token::Marker,
        ];

        for kind in [ThemeKind::Dark, ThemeKind::Light, ThemeKind::Terminal] {
            let theme = Theme::from_resolved(kind, ColorMode::None);
            for field in [
                theme.canvas,
                theme.fg,
                theme.muted,
                theme.border,
                theme.accent,
                theme.selection,
                theme.success,
                theme.warning,
                theme.error,
            ] {
                assert_eq!(field, Color::Reset, "{kind:?}/None must emit Color::Reset");
            }
            for token in all_tokens {
                assert_eq!(theme.syntax(token), Color::Reset);
            }
            assert!(
                theme
                    .selection_style()
                    .add_modifier
                    .contains(Modifier::REVERSED)
            );
            assert!(
                theme
                    .focus_style()
                    .add_modifier
                    .contains(Modifier::REVERSED | Modifier::BOLD)
            );
        }

        for mode in [ColorMode::Truecolor, ColorMode::Ansi256, ColorMode::Ansi16] {
            let terminal = Theme::from_resolved(ThemeKind::Terminal, mode);
            assert_eq!(terminal.canvas, Color::Reset);
            assert_eq!(terminal.fg, Color::Reset);
            assert!(
                terminal
                    .selection_style()
                    .add_modifier
                    .contains(Modifier::REVERSED)
            );
            assert!(
                terminal
                    .focus_style()
                    .add_modifier
                    .contains(Modifier::REVERSED | Modifier::BOLD)
            );
        }
    }

    #[test]
    fn auto_color_mode_resolution_and_override_precedence() {
        let yes = Some(OsStr::new("1"));
        let empty = Some(OsStr::new(""));

        assert_eq!(
            resolve_color_mode(ColorMode::Truecolor, yes, Some("dumb"), None),
            ColorMode::Truecolor
        );
        assert_eq!(
            resolve_color_mode(ColorMode::Ansi16, None, None, Some("truecolor")),
            ColorMode::Ansi16
        );
        assert_eq!(
            resolve_color_mode(
                ColorMode::Auto,
                yes,
                Some("xterm-256color"),
                Some("truecolor")
            ),
            ColorMode::None
        );
        assert_eq!(
            resolve_color_mode(
                ColorMode::Auto,
                empty,
                Some("xterm-256color"),
                Some("24bit")
            ),
            ColorMode::Truecolor
        );
        assert_eq!(
            resolve_color_mode(ColorMode::Auto, None, Some("dumb"), Some("truecolor")),
            ColorMode::None
        );
        assert_eq!(
            resolve_color_mode(ColorMode::Auto, None, Some("xterm"), Some("truecolor")),
            ColorMode::Truecolor
        );
        assert_eq!(
            resolve_color_mode(ColorMode::Auto, None, Some("tmux-256color"), None),
            ColorMode::Ansi256
        );
        assert_eq!(
            resolve_color_mode(ColorMode::Auto, None, Some("xterm"), None),
            ColorMode::Ansi16
        );
        assert_eq!(
            resolve_color_mode(ColorMode::Auto, None, None, None),
            ColorMode::Ansi16
        );
    }

    #[test]
    fn icon_width_bounds_and_kind_precedence() {
        let samples = [
            Path::new("main.rs"),
            Path::new("Cargo.toml"),
            Path::new("README.md"),
            Path::new("README"),
            Path::new(".gitignore"),
            Path::new("photo.png"),
            Path::new("archive.tar.gz"),
            Path::new("unknown_file"),
        ];
        let kinds = [
            IconKind::File,
            IconKind::Directory { expanded: false },
            IconKind::Directory { expanded: true },
            IconKind::Symlink,
            IconKind::Other,
        ];

        for mode in [IconMode::Unicode, IconMode::Nerd, IconMode::Ascii] {
            let slot = usize::from(icon_slot(mode));
            for kind in kinds {
                for path in samples {
                    let glyph = icon(path, kind, mode);
                    assert!(!glyph.is_empty());
                    assert!(!glyph.chars().any(char::is_control));
                    let width = UnicodeWidthStr::width(glyph);
                    assert!(
                        (1..=slot).contains(&width),
                        "icon {glyph:?} width {width} outside 1..={slot} for {mode:?}"
                    );
                    if mode == IconMode::Nerd {
                        let cjk_width = UnicodeWidthStr::width_cjk(glyph);
                        assert!(cjk_width <= slot);
                    }
                }
            }

            let closed = chevron(false, mode);
            let open = chevron(true, mode);
            assert_ne!(closed, open);
            assert_eq!(UnicodeWidthStr::width(closed), 1);
            assert_eq!(UnicodeWidthStr::width(open), 1);

            let close = close_icon(mode);
            assert_eq!(UnicodeWidthStr::width(close), 1);
        }

        let chrome_roles = [
            ChromeIcon::Workspace,
            ChromeIcon::Explorer,
            ChromeIcon::Editor,
            ChromeIcon::Status,
            ChromeIcon::Search,
            ChromeIcon::Help,
        ];

        for mode in [IconMode::Unicode, IconMode::Nerd, IconMode::Ascii] {
            let slot = usize::from(icon_slot(mode));
            for role in chrome_roles {
                let glyph = chrome_icon(role, mode);
                assert!(!glyph.is_empty());
                assert!(!glyph.chars().any(char::is_control));
                let width = UnicodeWidthStr::width(glyph);
                assert!(
                    (1..=slot).contains(&width),
                    "chrome icon {glyph:?} width {width} outside 1..={slot} for {mode:?}"
                );
            }
        }

        let dark_theme = Theme::from_resolved(ThemeKind::Dark, ColorMode::Truecolor);
        let none_theme = Theme::from_resolved(ThemeKind::Dark, ColorMode::None);

        // Color mode none emits reset for all entries.
        for path in samples {
            for kind in kinds {
                assert_eq!(
                    icon_color(path, kind, &none_theme),
                    Color::Reset,
                    "ColorMode::None must emit Color::Reset"
                );
            }
        }

        // Semantic distinction in color modes.
        let dir_col = icon_color(
            Path::new("src"),
            IconKind::Directory { expanded: false },
            &dark_theme,
        );
        let config_col = icon_color(Path::new("Cargo.toml"), IconKind::File, &dark_theme);
        let text_col = icon_color(Path::new("README.md"), IconKind::File, &dark_theme);
        let link_col = icon_color(Path::new("link"), IconKind::Symlink, &dark_theme);
        let other_col = icon_color(Path::new("pipe"), IconKind::Other, &dark_theme);

        assert_ne!(dir_col, config_col);
        assert_ne!(dir_col, other_col);
        assert_ne!(config_col, text_col);
        assert_ne!(link_col, other_col);
        // Directory classification must win over extension-like directory names.
        for mode in [IconMode::Unicode, IconMode::Nerd, IconMode::Ascii] {
            assert_eq!(
                icon(
                    Path::new("src.rs"),
                    IconKind::Directory { expanded: false },
                    mode
                ),
                icon(
                    Path::new("src"),
                    IconKind::Directory { expanded: false },
                    mode
                )
            );
        }
        assert_eq!(
            icon_color(
                Path::new("src.rs"),
                IconKind::Directory { expanded: false },
                &dark_theme,
            ),
            dark_theme.accent
        );
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_fallback_safely_within_slot_bounds() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};

        let non_utf8 = PathBuf::from(OsString::from_vec(vec![b'f', 0xFF, b'o']));
        for mode in [IconMode::Unicode, IconMode::Nerd, IconMode::Ascii] {
            let glyph = icon(non_utf8.as_path(), IconKind::File, mode);
            assert!((1..=usize::from(icon_slot(mode))).contains(&UnicodeWidthStr::width(glyph)));
        }
    }

    #[test]
    fn from_str_cli_parsing() {
        assert_eq!("dark".parse::<ThemeKind>(), Ok(ThemeKind::Dark));
        assert_eq!("LIGHT".parse::<ThemeKind>(), Ok(ThemeKind::Light));
        assert_eq!("Terminal".parse::<ThemeKind>(), Ok(ThemeKind::Terminal));
        assert!("".parse::<ThemeKind>().is_err());

        assert_eq!("auto".parse::<ColorMode>(), Ok(ColorMode::Auto));
        assert_eq!("truecolor".parse::<ColorMode>(), Ok(ColorMode::Truecolor));
        assert_eq!("256".parse::<ColorMode>(), Ok(ColorMode::Ansi256));
        assert_eq!("16".parse::<ColorMode>(), Ok(ColorMode::Ansi16));
        assert_eq!("none".parse::<ColorMode>(), Ok(ColorMode::None));
        assert!("invalid".parse::<ColorMode>().is_err());

        assert_eq!("unicode".parse::<IconMode>(), Ok(IconMode::Unicode));
        assert_eq!("nerd".parse::<IconMode>(), Ok(IconMode::Nerd));
        assert_eq!("ascii".parse::<IconMode>(), Ok(IconMode::Ascii));
        assert!("emoji".parse::<IconMode>().is_err());
    }
}
