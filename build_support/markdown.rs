use syntect::parsing::{Regex, SyntaxDefinition, syntax_definition::Pattern};

// The bundled rule requires a literal pipe in either branch. Reject pipe-free
// input before its expensive balanced-cell lookahead; retain all captures,
// matching rules and context transitions. Review this invariant on upgrades.
const TABLE_FIRST_ROW: &str = r"(?x:
    (?:{{balanced_table_cell}}?\|){2}           # at least 2 non-escaped pipe chars on the line
|   (?!\s+\|){{balanced_table_cell}}\|(?!\s+$)  # something other than whitespace followed by a pipe char, followed by something other than whitespace and the end of the line
)";

pub(crate) fn guard_tables(markdown: &mut SyntaxDefinition) {
    assert_eq!(
        markdown
            .variables
            .get("table_first_row")
            .map(String::as_str),
        Some(TABLE_FIRST_ROW),
        "review the Markdown table guard after a bundled grammar change"
    );
    let table_context = markdown
        .contexts
        .get("table")
        .expect("Markdown table context");
    let [Pattern::Match(rule)] = table_context.patterns.as_slice() else {
        panic!("review the Markdown table context after a grammar change");
    };
    // Reuse syntect's already-expanded newline-mode expression, rather than
    // duplicating variable expansion or newline/capture rewriting here.
    let table = rule
        .regex
        .regex_str()
        .strip_prefix("^(?=")
        .and_then(|regex| regex.strip_suffix(')'))
        .expect("Markdown table entry must be an anchored lookahead")
        .to_owned();
    let guarded = format!("(?=[^|]*[|]){table}");
    let mut changed = 0;
    for context in markdown.contexts.values_mut() {
        for pattern in &mut context.patterns {
            if let Pattern::Match(rule) = pattern
                && rule.regex.regex_str().contains(&table)
            {
                rule.regex = Regex::new(rule.regex.regex_str().replace(&table, &guarded));
                changed += 1;
            }
        }
    }
    assert_eq!(
        changed, 2,
        "review Markdown table guard coverage after a grammar change"
    );
}
