//! Builds the syntax dump that `src/syntax.rs` embeds at runtime.
//!
//! Combines syntect's bundled default syntax set (newline mode, matching the
//! newline-terminated lines the highlight engine parses) with the real
//! TypeScript and TSX grammars vendored under `assets/syntaxes/`, links every
//! context, and writes one uncompressed pack dump into `OUT_DIR`.
//!
//! The build rejects invalid syntax assets and unresolved TypeScript context
//! references. The application runtime only ever does `include_bytes!` plus a
//! binary load of the dump here: no YAML parsing, no project/plugin config,
//! no network.

use std::{
    env,
    path::{Path, PathBuf},
};

use syntect::parsing::{SyntaxSet, SyntaxSetBuilder};

#[path = "build_support/markdown.rs"]
mod markdown;

const SYNTAX_ASSETS: &str = "assets/syntaxes";
const DUMP_FILE: &str = "syntaxes.packdump";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build_support/markdown.rs");
    println!("cargo:rerun-if-changed={SYNTAX_ASSETS}");

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));

    let syntax_set = build_syntax_set(&manifest_dir.join(SYNTAX_ASSETS));
    reject_unresolved_contexts(&syntax_set);

    let dump_path = out_dir.join(DUMP_FILE);
    syntect::dumps::dump_to_uncompressed_file(&syntax_set, &dump_path)
        .expect("syntax set must serialize into the build dump");
}

fn build_syntax_set(assets: &Path) -> SyntaxSet {
    // Newline mode must match `load_defaults_newlines`: the highlight engine
    // parses whole lines including their trailing `\n`.
    let defaults = SyntaxSet::load_defaults_newlines().into_builder();
    let mut builder = SyntaxSetBuilder::new();
    for mut syntax in defaults.syntaxes().iter().cloned() {
        if syntax.name == "Markdown" {
            markdown::guard_tables(&mut syntax);
        }
        builder.add(syntax);
    }
    builder
        .add_from_folder(assets, true)
        .expect("vendored TypeScript syntax assets must parse as sublime-syntax");
    builder.build()
}

/// Every context reference added by the vendored grammars must link during
/// `build()`; a symbolic (non-direct) reference would silently stop scoping.
fn reject_unresolved_contexts(syntax_set: &SyntaxSet) {
    if let Some(unresolved) = syntax_set
        .find_unlinked_contexts()
        .into_iter()
        .find(|entry| entry.contains("Syntax 'TypeScript"))
    {
        panic!("bundled TypeScript grammar contains an unresolved context: {unresolved}");
    }
}
