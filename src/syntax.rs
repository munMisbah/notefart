//! Process-wide syntax set for the highlight engine.
//!
//! `build.rs` combines syntect's bundled default syntaxes (newline mode) with
//! the real TypeScript and TSX grammars vendored under `assets/syntaxes/`,
//! links every context, and writes one uncompressed pack dump into `OUT_DIR`.
//! This module embeds that dump and deserializes it exactly once per process,
//! so every highlighter shares the same grammar data, regex caches, and
//! first-line cache. Nothing here reads YAML, project config, plugins, or the
//! network at runtime.

use std::sync::LazyLock;

use syntect::parsing::SyntaxSet;

static SYNTAX_SET: LazyLock<SyntaxSet> = LazyLock::new(|| {
    let dump = include_bytes!(concat!(env!("OUT_DIR"), "/syntaxes.packdump"));
    syntect::dumps::from_uncompressed_data(dump).expect("embedded syntax dump must load")
});

/// Returns the shared syntax set: syntect defaults plus real TypeScript/TSX.
///
/// [`SyntaxSet`] is `Send + Sync`; the returned `&'static` reference is
/// immutable and cheap to share across highlighter instances.
pub(crate) fn load() -> &'static SyntaxSet {
    &SYNTAX_SET
}

// Sharing `&'static SyntaxSet` across highlighter instances relies on the set
// being safe to reference from any thread; fail the build if that changes.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SyntaxSet>();
};

#[cfg(test)]
mod tests {
    use super::load;

    use syntect::parsing::{ParseState, ScopeStack, ScopeStackOp, SyntaxReference};

    /// The scope ops returned for one probe line, plus the stack all earlier
    /// lines already applied, so assertions can inspect any character.
    struct LineScopes {
        base: ScopeStack,
        ops: Vec<(usize, ScopeStackOp)>,
    }

    fn parse_lines(syntax: &SyntaxReference, lines: &[&str]) -> Vec<LineScopes> {
        let syntaxes = load();
        let mut state = ParseState::new(syntax);
        let mut running = ScopeStack::new();
        lines
            .iter()
            .map(|line| {
                // Newline mode: the engine always parses lines that carry `\n`.
                let with_newline = format!("{line}\n");
                let ops = state
                    .parse_line(&with_newline, syntaxes)
                    .expect("probe line must parse");
                let base = running.clone();
                for (_, op) in &ops {
                    running.apply(op).expect("scope ops must apply in order");
                }
                LineScopes { base, ops }
            })
            .collect()
    }

    fn scope_prefixes_at(line: &LineScopes, index: usize) -> Vec<String> {
        let mut stack = line.base.clone();
        for &(position, ref op) in &line.ops {
            if position > index {
                break;
            }
            stack.apply(op).expect("scope ops must apply in order");
        }
        stack
            .as_slice()
            .iter()
            .map(|scope| scope.to_string())
            .collect()
    }

    fn assert_scope(line: &LineScopes, index: usize, prefix: &str) {
        let prefixes = scope_prefixes_at(line, index);
        assert!(
            prefixes.iter().any(|scope| scope.starts_with(prefix)),
            "char {index} must carry a `{prefix}` scope, got {prefixes:?}"
        );
    }

    fn deny_scope(line: &LineScopes, index: usize, prefix: &str) {
        let prefixes = scope_prefixes_at(line, index);
        assert!(
            !prefixes.iter().any(|scope| scope.starts_with(prefix)),
            "char {index} must not carry a `{prefix}` scope, got {prefixes:?}"
        );
    }

    /// Byte index in the middle of an ASCII fixture token.
    fn mid(line: &str, needle: &str) -> usize {
        let start = line
            .find(needle)
            .unwrap_or_else(|| panic!("probe text {needle:?} missing from {line:?}"));
        start + needle.len() / 2
    }

    fn ts_syntax() -> &'static SyntaxReference {
        load()
            .find_syntax_by_extension("ts")
            .expect("TypeScript syntax must be bundled")
    }

    fn tsx_syntax() -> &'static SyntaxReference {
        load()
            .find_syntax_by_extension("tsx")
            .expect("TypeScriptReact syntax must be bundled")
    }

    #[test]
    fn typescript_scopes_generics_types_and_templates() {
        let ts = ts_syntax();

        let signature =
            "export function formatNote<T extends Note>(note: T, theme: Theme): string {";
        let line = &parse_lines(ts, &[signature])[0];
        assert_scope(line, mid(signature, "export"), "keyword.control.export");
        assert_scope(line, mid(signature, "function"), "storage.type.function");
        assert_scope(line, mid(signature, "formatNote"), "entity.name.function");
        // Capture scopes pop right after their match, so assert on exact
        // positions: the opening `<` itself, and the middle of `extends`.
        assert_scope(
            line,
            signature.find('<').expect("generic parameter list present"),
            "punctuation.definition.typeparameters.begin",
        );
        assert_scope(line, mid(signature, "<T extends"), "meta.type.parameters");
        assert_scope(line, mid(signature, "<T extends"), "storage.modifier");
        // The `:` of the return annotation is a one-character capture.
        assert_scope(
            line,
            mid(signature, "): "),
            "keyword.operator.type.annotation",
        );
        assert_scope(line, mid(signature, "string {"), "support.type");

        let template = "  return `${prefix} ${note.title} (${theme})`;";
        let line = &parse_lines(ts, &[template])[0];
        deny_scope(line, mid(template, "return"), "string.template");
        assert_scope(line, mid(template, "return `${prefix}"), "string.template");
        assert_scope(
            line,
            mid(template, "return `${prefix}"),
            "punctuation.definition.template-expression.begin",
        );
        assert_scope(line, mid(template, "`${prefix}"), "meta.embedded.line");
        assert_scope(line, mid(template, "note.title"), "meta.embedded.line");
    }

    #[test]
    fn typescript_multiline_comment_state_spans_lines() {
        let ts = ts_syntax();
        let texts = ["/* doc", " * body", " */", "const after = 1;"];
        let lines = parse_lines(ts, &texts);

        assert_scope(&lines[0], mid(texts[0], "doc"), "comment.block");
        assert_scope(&lines[1], mid(texts[1], "body"), "comment.block");
        deny_scope(&lines[3], mid(texts[3], "after"), "comment.block");
    }

    #[test]
    fn tsx_scopes_jsx_tags_embedded_expressions_and_comments() {
        let tsx = tsx_syntax();
        let texts = [
            "export function NoteCard() {",
            "  return (",
            "    <article className=\"note\">",
            "      {/* JSX comment */}",
            "      <p>Clicks: {count}</p>",
            "      <button onClick={() => setCount(count + 1)}>",
            "        Add one",
            "      </button>",
            "    </article>",
            "  );",
            "}",
        ];
        let lines = parse_lines(tsx, &texts);

        assert_scope(&lines[0], mid(texts[0], "export"), "keyword.control.export");

        let tag = &lines[2];
        assert_scope(tag, mid(texts[2], "article"), "meta.tag");
        assert_scope(tag, mid(texts[2], "article"), "entity.name.tag");
        assert_scope(tag, mid(texts[2], "\"note\""), "string.quoted.double");

        let comment = &lines[3];
        assert_scope(comment, mid(texts[3], "JSX comment"), "comment.block");
        deny_scope(comment, mid(texts[3], "{/*"), "string.quoted");

        let embedded = &lines[4];
        assert_scope(embedded, mid(texts[4], "<p>"), "meta.tag");
        assert_scope(embedded, mid(texts[4], "<p>"), "entity.name.tag");
        assert_scope(
            embedded,
            mid(texts[4], "{count}"),
            "meta.embedded.expression",
        );

        let nested = &lines[5];
        assert_scope(nested, mid(texts[5], "onClick"), "meta.tag");
        assert_scope(
            nested,
            mid(texts[5], "setCount"),
            "meta.embedded.expression",
        );

        // Tag context must survive across the child lines and close cleanly.
        let closing = &lines[7];
        assert_scope(closing, mid(texts[7], "</button>"), "meta.tag");
        assert_scope(closing, mid(texts[7], "</button>"), "entity.name.tag");
        let outer = &lines[8];
        assert_scope(outer, mid(texts[8], "</article>"), "meta.tag");
        assert_scope(outer, mid(texts[8], "</article>"), "entity.name.tag");
    }

    #[test]
    fn markdown_preserves_bundled_scopes_across_tables_and_block_boundaries() {
        use syntect::parsing::SyntaxSet;

        let original = SyntaxSet::load_defaults_newlines();
        let optimized = load();
        let mut before = ParseState::new(original.find_syntax_by_extension("md").unwrap());
        let mut after = ParseState::new(optimized.find_syntax_by_extension("md").unwrap());
        let corpus = concat!(
            "# Heading\n\nParagraph **bold** *italic* [link](https://example.invalid) `code`.\n\n",
            "> Quote\n> next line\n\n    indented code\n\n---\n***\n___\n\n",
            "- item\n  continuation\n\n1. ordered item\n\n",
            "| First | Second |\n| --- | --- |\n| a | b |\n\n",
            "Alpha | Beta\n--- | ---\nleft | right\n\n",
            "`a|b` | **bold**\n--- | ---\nvalue | cell\n\n",
            "escaped \\| ordinary text\n\n| escaped \\| cell | 日本語 |\n| --- | --- |\n\n",
            "*first* | [second](url)\n--- | ---\n\n",
            "one pipe |\n\n| one leading pipe\n\n",
            "```rust\nlet value = 42; // comment\n```\n\n",
            "~~~\n| not a table |\n~~~\n\n",
            "<!-- multiline\n| comment |\n-->\n\n",
            "<div>\nHTML | content\n</div>\n\n",
            "\t\n\u{00a0}\n\u{2003}\n\r\n",
            "CRLF | table\r\n--- | ---\r\nleft | right\r\n\r\n",
            "Setext heading\n===\n\nFinal paragraph without newline"
        );
        for (index, line) in corpus.split_inclusive('\n').enumerate() {
            assert_eq!(
                before.parse_line(line, &original).unwrap(),
                after.parse_line(line, optimized).unwrap(),
                "Markdown scopes differ on line {}: {line:?}",
                index + 1
            );
        }
    }
}
