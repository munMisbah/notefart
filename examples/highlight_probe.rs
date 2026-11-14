//! Reproducible Syntect evaluation, not the production highlight engine.
use std::{
    collections::BTreeMap, error::Error, hint::black_box, ops::Range, str::FromStr, time::Instant,
};

use syntect::{
    easy::ScopeRangeIterator,
    highlighting::ScopeSelectors,
    parsing::{MatchPower, ParseState, ScopeStack, SyntaxReference, SyntaxSet},
    util::LinesWithEndings,
};

const SAMPLE: &str = "/* multiline comment\nlet hidden = \"not a string\";\n*/\nstruct Thing;\nfn main() {\n    let café = \"東京🧪\";/*🔥*/let number=42;\n}";
const ITERATIONS: usize = 20;

type Rules = Vec<(ScopeSelectors, &'static str)>;

fn semantic_rules() -> Result<Rules, Box<dyn Error>> {
    [
        ("comment", "muted"),
        ("keyword - keyword.operator, storage.type - storage.type.string, storage.modifier", "syntax.keyword"),
        ("string, storage.type.string", "syntax.string"),
        ("constant.numeric, constant.language", "syntax.number"),
        ("entity.name.function, support.function, support.macro, variable.function", "syntax.function"),
        ("entity.name.struct, entity.name.enum, entity.name.trait, entity.name.type, support.type", "syntax.type"),
    ]
    .into_iter()
    .map(|(selector, token)| Ok((ScopeSelectors::from_str(selector)?, token)))
    .collect()
}

fn classify(stack: &ScopeStack, rules: &Rules) -> &'static str {
    let mut best: Option<(MatchPower, &'static str)> = None;
    for (selector, token) in rules {
        if let Some(power) = selector.does_match(stack.as_slice())
            && best.is_none_or(|(previous, _)| power > previous)
        {
            best = Some((power, token));
        }
    }
    best.map_or("fg", |(_, token)| token)
}

#[derive(Debug, PartialEq)]
struct Span {
    bytes: Range<usize>,
    token: &'static str,
}

struct Report {
    spans: Vec<Span>,
    checkpoints: Vec<(ParseState, ScopeStack)>,
}

fn parse(
    syntaxes: &SyntaxSet,
    syntax: &SyntaxReference,
    rules: &Rules,
    source: &str,
) -> Result<Report, Box<dyn Error>> {
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut spans = Vec::new();
    let mut checkpoints = Vec::new();
    let mut line_offset = 0;
    let mut reconstructed = String::with_capacity(source.len());
    for line in LinesWithEndings::from(source) {
        let operations = state.parse_line(line, syntaxes)?;
        let mut covered = 0;
        for (range, operation) in ScopeRangeIterator::new(&operations, line) {
            stack.apply(operation)?;
            assert_eq!(range.start, covered, "highlight ranges must be contiguous");
            assert!(line.is_char_boundary(range.start) && line.is_char_boundary(range.end));
            covered = range.end;
            reconstructed.push_str(&line[range.clone()]);
            if !range.is_empty() {
                spans.push(Span {
                    bytes: line_offset + range.start..line_offset + range.end,
                    token: classify(&stack, rules),
                });
            }
        }
        assert_eq!(
            covered,
            line.len(),
            "highlighting must cover the whole line"
        );
        checkpoints.push((state.clone(), stack.clone()));
        line_offset += line.len();
    }
    assert_eq!(
        reconstructed, source,
        "highlighting must preserve every UTF-8 byte"
    );
    Ok(Report { spans, checkpoints })
}

fn token_at(report: &Report, source: &str, needle: &str) -> &'static str {
    let byte = source
        .find(needle)
        .expect("probe fixture contains the requested fragment");
    report
        .spans
        .iter()
        .find(|span| span.bytes.contains(&byte))
        .expect("complete highlight coverage")
        .token
}

// Timed loop deliberately excludes validation allocations and checkpoint cloning.
fn parse_and_map(
    syntaxes: &SyntaxSet,
    syntax: &SyntaxReference,
    rules: &Rules,
    source: &str,
) -> Result<usize, Box<dyn Error>> {
    let mut state = ParseState::new(syntax);
    let mut stack = ScopeStack::new();
    let mut checksum = 0usize;
    for line in LinesWithEndings::from(source) {
        let operations = state.parse_line(line, syntaxes)?;
        for (range, operation) in ScopeRangeIterator::new(&operations, line) {
            stack.apply(operation)?;
            checksum = checksum.wrapping_add(range.len() * classify(&stack, rules).len());
        }
    }
    Ok(checksum)
}

fn main() -> Result<(), Box<dyn Error>> {
    println!("Notefart highlighting evaluation: Syntect / regex-onig / semantic tokens");
    if cfg!(debug_assertions) {
        println!("Timing is unoptimized; use --release for performance measurements.");
    }
    let start = Instant::now();
    let syntaxes = SyntaxSet::load_defaults_newlines();
    let load = start.elapsed();
    let syntax = syntaxes
        .find_syntax_by_extension("rs")
        .ok_or("bundled Rust grammar unavailable")?;
    let rules = semantic_rules()?;
    let workload = format!("{SAMPLE}\n").repeat(40);
    let cold_start = Instant::now();
    black_box(parse_and_map(&syntaxes, syntax, &rules, &workload)?);
    let cold = cold_start.elapsed();

    let original = parse(&syntaxes, syntax, &rules, SAMPLE)?;
    for (fragment, token) in [
        ("hidden", "muted"),
        ("fn main", "syntax.keyword"),
        ("main()", "syntax.function"),
        ("Thing", "syntax.type"),
        ("café", "fg"),
        ("東京🧪", "syntax.string"),
        ("🔥", "muted"),
        ("42", "syntax.number"),
    ] {
        assert_eq!(
            token_at(&original, SAMPLE, fragment),
            token,
            "semantic category for {fragment:?}"
        );
    }

    // Breaking a multiline terminator must invalidate downstream highlight state.
    let edited_source = SAMPLE.replacen("*/", "* /", 1);
    let edited = parse(&syntaxes, syntax, &rules, &edited_source)?;
    let closing_line = LinesWithEndings::from(SAMPLE)
        .position(|line| line.contains("*/"))
        .unwrap();
    assert_eq!(
        &original.checkpoints[..closing_line],
        &edited.checkpoints[..closing_line]
    );
    assert_ne!(
        original.checkpoints[closing_line],
        edited.checkpoints[closing_line]
    );
    assert_eq!(token_at(&edited, &edited_source, "fn main"), "muted");
    assert_eq!(token_at(&edited, &edited_source, "東京🧪"), "muted");
    let restored_source = edited_source.replacen("* /", "*/", 1);
    let restored = parse(&syntaxes, syntax, &rules, &restored_source)?;
    assert_eq!(restored.spans, original.spans);
    assert_eq!(restored.checkpoints, original.checkpoints);

    let start = Instant::now();
    for _ in 0..ITERATIONS {
        black_box(parse_and_map(&syntaxes, syntax, &rules, &workload)?);
    }
    let elapsed = start.elapsed();
    let mut categories = BTreeMap::new();
    for span in &original.spans {
        *categories.entry(span.token).or_insert(0usize) += span.bytes.len();
    }
    println!(
        "syntax load: {:.3} ms; cold parse: {:.3} ms",
        load.as_secs_f64() * 1000.0,
        cold.as_secs_f64() * 1000.0
    );
    println!(
        "warm parse+map: {} bytes, {} lines x {} iterations, {:.3} ms/pass",
        workload.len(),
        LinesWithEndings::from(&workload).count(),
        ITERATIONS,
        elapsed.as_secs_f64() * 1000.0 / ITERATIONS as f64
    );
    for (token, bytes) in categories {
        println!("  {token:<16} {bytes:>4} bytes");
    }
    println!(
        "PASS: UTF-8 coverage, multiline state, edited terminator, restored categories, EOF without newline."
    );
    println!(
        "Tree-sitter was compared architecturally, not benchmarked. This is not the production cache."
    );
    Ok(())
}
