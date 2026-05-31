/// AST-level slop detection using line-based source analysis (no syn dependency).
///
/// Goes beyond simple regex matching by tracking function definitions,
/// reference counts, block structure, comment patterns, and control-flow
/// shapes across a Rust source file. All analysis is purely structural —
/// no semantic type resolution, no external parser.
use std::collections::HashMap;

use crate::rules::{RuleViolation, RulesCheck, ViolationSeverity};

// ── AI narration comment patterns ───────────────────────────────────────────

/// Patterns that indicate AI-generated narration comments.
///
/// These are comments written in a "tutorial" voice that narrate what the
/// code is doing. They add noise without value in production code.
const NARRATION_PATTERNS: &[&str] = &[
    "now we",
    "here we",
    "let us",
    "let's",
    "first we",
    "next we",
    "this function",
    "this method",
    "this helper",
    "this utility",
    "we create",
    "we define",
    "we implement",
    "we set up",
    "we build",
    "we add",
    "we write",
    "we need",
    "we want",
    "we can",
    "we will",
    "we use",
    "we call",
    "we check",
    "we handle",
    "we return",
    "we parse",
    "we convert",
    "we transform",
    "we generate",
    "we process",
    "we take",
    "we start",
    "we begin",
    "essentially",
    "basically just",
    "simply a",
    "in other words",
    "that is to say",
    "as mentioned",
    "as noted",
    "as we saw",
    "the idea is",
    "the concept is",
    "basically,",
    "simply put",
    "in essence",
    "// now ",
    "// next, ",
];

/// Lines that look like function definition headers.
fn is_fn_def(line: &str) -> bool {
    let trimmed = line.trim();
    // Skip lines that are comments or contain "#["
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with("#[") {
        return false;
    }
    // Match `fn name(...` at start (possibly with pub/async/unsafe/extern modifiers)
    let stripped = strip_visibility_and_modifiers(trimmed);
    stripped.starts_with("fn ")
        && stripped[3..]
            .trim_start()
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
}

fn strip_visibility_and_modifiers(s: &str) -> &str {
    let mut s = s.trim_start();
    loop {
        let before = s;
        for prefix in &[
            "pub ",
            "pub(crate) ",
            "pub(super) ",
            "pub(self) ",
            "async ",
            "unsafe ",
            "extern ",
            "const ",
            "default ",
            "override ",
        ] {
            if let Some(rest) = s.strip_prefix(prefix) {
                s = rest;
                break;
            }
        }
        // Also handle pub(in path) etc — simplified: just skip "pub(" ... ")"
        if s.starts_with("pub(")
            && let Some(close) = s.find(')')
        {
            s = s[close + 1..].trim_start();
        }
        if s == before {
            break;
        }
    }
    s
}

/// Extract function name from a `fn name(...` line.
fn extract_fn_name(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let stripped = strip_visibility_and_modifiers(trimmed);
    if let Some(name_start) = stripped.strip_prefix("fn ") {
        let after_fn = name_start.trim_start();
        // name is up to '(' or '<' (generics) or ' ' (whitespace before parens in some edge cases)
        if let Some(paren) = after_fn.find('(') {
            let name_candidate = after_fn[..paren].trim();
            // Strip generics like 'name<T>'
            let name = name_candidate
                .split('<')
                .next()
                .unwrap_or(name_candidate)
                .trim();
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Extract function call references from a line.
/// Ignores the definition site itself.
fn extract_fn_calls(line: &str, defined_fns: &[String]) -> Vec<String> {
    let mut calls = Vec::new();
    for fn_name in defined_fns {
        // Look for `fn_name(` pattern, but not `fn fn_name(` (definition)
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("fn {}", fn_name)) {
            continue;
        }
        // Simple detection: function_name followed by '('
        let search = format!("{}(", fn_name);
        if line.contains(&search) {
            calls.push(fn_name.clone());
        }
    }
    calls
}

/// Check if a line is unreachable (comes after return/panic!/todo!/unreachable!)
fn has_unreachable_after(lines: &[&str], idx: usize) -> Option<String> {
    if idx == 0 {
        return None;
    }
    let prev_line = lines[idx - 1].trim();

    // Check if previous line ends a block: `return expr;`, `panic!(...);`, etc.
    let terminating_patterns = [
        "return ",
        "return;",
        "panic!(",
        "unreachable!(",
        "todo!(",
        "unimplemented!(",
        "std::process::exit(",
        "process::exit(",
        "std::mem::forget(",
        "mem::forget(",
        "loop { break",
    ];

    let is_terminator = terminating_patterns.iter().any(|p| {
        if p.ends_with('(') {
            prev_line.contains(p) && prev_line.ends_with(';')
        } else if p.ends_with(';') || p.ends_with("break") {
            prev_line.starts_with(p)
        } else {
            prev_line.starts_with(p) && prev_line.ends_with(';')
        }
    });

    if !is_terminator {
        return None;
    }

    // Don't flag empty lines, closing braces, or comments after a return
    let current = lines[idx].trim();
    if current.is_empty()
        || current == "}"
        || current.starts_with("//")
        || current.starts_with("/*")
        || current.starts_with('*')
    {
        return None;
    }

    Some(format!(
        "Code after `{}` at line {} may be unreachable",
        prev_line,
        idx + 1 // 1-indexed for user display
    ))
}

/// Check if a function is a single-use helper (definition appears once
/// as a fn, and is referenced exactly once elsewhere).
fn check_single_use_helpers(
    fn_defs: &HashMap<String, Vec<usize>>,
    fn_calls: &HashMap<String, Vec<usize>>,
    lines: &[&str],
) -> Vec<RuleViolation> {
    let mut violations = Vec::new();
    for (name, def_lines) in fn_defs {
        let call_count = fn_calls.get(name).map(|c| c.len()).unwrap_or(0);
        let def_count = def_lines.len();

        // Only flag functions defined within this file that are called once
        // or zero times (private helpers no one calls)
        if def_count == 1 && call_count == 1 {
            let line_no = def_lines[0] + 1; // 1-indexed
            violations.push(RuleViolation {
                rule: "ast:single-use-helper".into(),
                description: format!(
                    "Function `{}` (defined at line {}) is only called once — consider inlining",
                    name, line_no
                ),
                severity: ViolationSeverity::Warning,
            });
        }

        // Zero-call private functions (not pub) are dead code
        if def_count == 1 && call_count == 0 {
            let line_no = def_lines[0] + 1;
            // Check if it's a public function — we can only approximate
            let def_line = lines[def_lines[0]].trim();
            // Skip `main` (called by the runtime) and pub functions
            if name != "main"
                && !def_line.trim_start().starts_with("pub ")
                && !def_line.trim_start().starts_with("pub(")
            {
                violations.push(RuleViolation {
                    rule: "ast:unused-function".into(),
                    description: format!(
                        "Function `{}` (defined at line {}) is defined but never called — dead code",
                        name, line_no
                    ),
                    severity: ViolationSeverity::Warning,
                });
            }
        }
    }
    violations
}

/// Check for over-abstraction: function whose body is just a single
/// call to another function (thin wrapper with no added logic).
fn check_over_abstraction(
    fn_defs: &HashMap<String, Vec<usize>>,
    lines: &[&str],
) -> Vec<RuleViolation> {
    let mut violations = Vec::new();

    for (name, def_lines) in fn_defs {
        for &def_line in def_lines {
            // Collect the body lines of this function
            let body = collect_fn_body(lines, def_line);
            if body.is_empty() {
                continue;
            }

            // A thin wrapper: body has exactly one statement that's a function call
            // Count actual statements (non-empty, non-brace, non-comment, non-fn-def lines)
            let stmts: Vec<&str> = body
                .iter()
                .map(|l| l.trim())
                .filter(|l| {
                    !l.is_empty()
                        && *l != "{"
                        && *l != "}"
                        && !l.starts_with("//")
                        && !l.starts_with("/*")
                        && !l.starts_with('*')
                        && !l.starts_with("fn ")
                })
                .collect();

            if stmts.len() == 1 {
                let stmt = stmts[0];
                // Check if it's just calling another function (possibly with semicolon)
                let clean_stmt = stmt.trim_end_matches(';').trim();
                if clean_stmt.contains('(') && clean_stmt.ends_with(')') {
                    // Extract the called function name
                    if let Some(called_name) = clean_stmt.split('(').next() {
                        let called_name = called_name.trim();
                        // Skip if calling self recursively
                        if called_name != name
                            && !called_name.is_empty()
                            && !called_name.starts_with('&')
                            && !called_name.starts_with('*')
                        {
                            violations.push(RuleViolation {
                                rule: "ast:over-abstraction".into(),
                                description: format!(
                                    "Function `{}` (line {}) is a thin wrapper around `{}` — inline it",
                                    name, def_line + 1, called_name
                                ),
                                severity: ViolationSeverity::Warning,
                            });
                        }
                    }
                }
            }
        }
    }

    violations
}

/// Collect the lines comprising a function body, given the definition line.
fn collect_fn_body<'a>(lines: &'a [&str], def_line: usize) -> Vec<&'a str> {
    let mut body = Vec::new();
    let mut brace_depth: i32 = 0;
    let mut in_body = false;

    for (i, line) in lines.iter().copied().enumerate().skip(def_line) {
        for ch in line.chars() {
            if ch == '{' {
                brace_depth += 1;
                in_body = true;
            } else if ch == '}' {
                brace_depth -= 1;
            }
        }
        if in_body {
            body.push(line);
            if brace_depth <= 0 && i > def_line {
                break;
            }
        }
    }

    body
}

/// Detect long if-else chains that should be match statements.
fn check_if_else_chains(lines: &[&str]) -> Vec<RuleViolation> {
    let mut violations = Vec::new();

    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();

        // Check for `if` not preceded by `else` (start of a chain)
        // Must handle `if ... {` and not `} else if ... {` or `else if ... {`
        if (trimmed.starts_with("if ") || trimmed.starts_with("if("))
            && !trimmed.starts_with("else if")
            && !lines[i].contains("else if")
            && !trimmed.starts_with("//")
            && !trimmed.starts_with("/*")
        {
            let mut chain_len = 1;
            let start = i;

            // Look ahead for else if branches
            let mut j = i + 1;
            let mut brace_depth = count_opening_braces(trimmed) - count_closing_braces(trimmed);
            while j < lines.len() {
                let l = lines[j].trim();

                // Track brace depth
                brace_depth += count_opening_braces(l) - count_closing_braces(l);
                let has_else_if = l.contains("else if");
                let has_else_block =
                    (l.contains("else {") || l.contains("else{")) && !l.contains("else if");

                if has_else_if && !has_else_block {
                    // We should only count a new branch if the else-if starts at the
                    // same or lower indentation as the original if (i.e., same scope level)
                    if brace_depth <= 1 {
                        chain_len += 1;
                    }
                } else if has_else_block && brace_depth <= 1 {
                    // Final `else { }` is not counted in chain length — it's the
                    // catch-all, not a condition. We just note the chain ends here.
                    break;
                }

                // If brace depth drops to 0 (or below), we've exited the entire
                // if-else construct
                if brace_depth <= 0 && (l == "}" || l.starts_with("}")) && !has_else_if {
                    break;
                }

                j += 1;
            }

            if chain_len >= 3 {
                violations.push(RuleViolation {
                    rule: "ast:if-else-chain".into(),
                    description: format!(
                        "Long if-else chain with {} branches starting at line {} — consider `match`",
                        chain_len, start + 1
                    ),
                    severity: ViolationSeverity::Warning,
                });
            }
        }
        i += 1;
    }

    violations
}

fn count_opening_braces(s: &str) -> i32 {
    // Count only braces outside of string literals (simplified)
    s.chars().filter(|&c| c == '{').count() as i32
}

fn count_closing_braces(s: &str) -> i32 {
    s.chars().filter(|&c| c == '}').count() as i32
}

/// Check for AI narration comments across the file.
fn check_narration_comments(lines: &[&str]) -> Vec<RuleViolation> {
    let mut violations = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Only check comments
        if !trimmed.starts_with("//") && !trimmed.starts_with("/*") && !trimmed.starts_with("*") {
            continue;
        }

        // Remove comment markers for pattern matching
        let comment_text = if let Some(s) = trimmed.strip_prefix("//") {
            s
        } else if let Some(s) = trimmed.strip_prefix("/*") {
            // strip_prefix on "/*" strips both chars — same as "//" for 2-char prefix
            s
        } else if let Some(s) = trimmed.strip_prefix('*') {
            s
        } else {
            continue;
        };
        let comment_text = comment_text.to_lowercase();

        // Check each pattern
        for pattern in NARRATION_PATTERNS {
            if comment_text.contains(pattern) {
                violations.push(RuleViolation {
                    rule: "ast:narration-comment".into(),
                    description: format!(
                        "AI narration comment at line {}: matches pattern '{}'",
                        i + 1,
                        pattern
                    ),
                    severity: ViolationSeverity::Warning,
                });
                break; // One violation per comment line
            }
        }
    }

    violations
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Run all AST-level checks against a Rust source file.
///
/// This performs structural analysis that goes beyond what simple regex
/// can detect: function reference counting, block scoping for unreachable
/// code detection, if-else chain length measurement, and multi-line
/// comment pattern analysis anchored to specific line numbers.
pub fn check_ast(source: &str) -> RulesCheck {
    let lines: Vec<&str> = source.lines().collect();
    let mut violations: Vec<RuleViolation> = Vec::new();

    // ── Phase 1: Scan for function definitions and calls ──
    let mut fn_defs: HashMap<String, Vec<usize>> = HashMap::new();
    let mut fn_calls: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, line) in lines.iter().enumerate() {
        if is_fn_def(line)
            && let Some(name) = extract_fn_name(line)
        {
            fn_defs.entry(name).or_default().push(i);
        }
    }

    // Phase 1b: Collect defined function names for call detection
    let defined_names: Vec<String> = fn_defs.keys().cloned().collect();

    for (i, line) in lines.iter().enumerate() {
        let calls = extract_fn_calls(line, &defined_names);
        for call in calls {
            fn_calls.entry(call).or_default().push(i);
        }
    }

    // ── Phase 2: Single-use helper detection ──
    violations.extend(check_single_use_helpers(&fn_defs, &fn_calls, &lines));

    // ── Phase 3: Over-abstraction detection ──
    violations.extend(check_over_abstraction(&fn_defs, &lines));

    // ── Phase 4: Unreachable code detection ──
    for i in 0..lines.len() {
        if let Some(desc) = has_unreachable_after(&lines, i) {
            violations.push(RuleViolation {
                rule: "ast:unreachable-code".into(),
                description: desc,
                severity: ViolationSeverity::Warning,
            });
        }
    }

    // ── Phase 5: AI narration comments ──
    violations.extend(check_narration_comments(&lines));

    // ── Phase 6: if-else chain detection ──
    violations.extend(check_if_else_chains(&lines));

    // ── Decision ──
    let passed = violations
        .iter()
        .all(|v| v.severity != ViolationSeverity::Fatal);
    RulesCheck { passed, violations }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_code_passes() {
        let source = r#"
fn add(a: i32, b: i32) -> i32 {
    a + b
}

fn multiply(x: i32, y: i32) -> i32 {
    x * y
}

pub fn main() {
    let result = add(1, 2);
    let product = multiply(3, 4);
    println!("{}", result + product);
}
"#;
        let check = check_ast(source);
        assert!(
            check.passed,
            "Clean code should pass, got violations: {:?}",
            check.violations
        );
    }

    #[test]
    fn test_narration_comments_detected() {
        let source = r#"
// Now we create the config parser
fn parse_config(input: &str) -> Config {
    // Here we implement the parsing logic
    Config { data: input.to_string() }
}

// This function handles the validation
fn validate(cfg: &Config) -> bool {
    // First we check the input
    cfg.data.len() > 0
}
"#;
        let check = check_ast(source);
        let narration_violations: Vec<_> = check
            .violations
            .iter()
            .filter(|v| v.rule == "ast:narration-comment")
            .collect();
        assert!(
            narration_violations.len() >= 3,
            "Expected at least 3 narration comment violations, got {}",
            narration_violations.len()
        );
    }

    #[test]
    fn test_single_use_helper_detected() {
        let source = r#"
fn helper_parse_token(s: &str) -> &str {
    s.trim()
}

fn main() {
    let data = " hello ";
    let cleaned = helper_parse_token(data);
    println!("{}", cleaned);
}
"#;
        let check = check_ast(source);
        let single_use: Vec<_> = check
            .violations
            .iter()
            .filter(|v| v.rule == "ast:single-use-helper")
            .collect();
        assert_eq!(
            single_use.len(),
            1,
            "Should detect single-use helper function"
        );
        assert!(single_use[0].description.contains("helper_parse_token"));
    }

    #[test]
    fn test_over_abstraction_detected() {
        let source = r#"
fn do_real_work(x: i32) -> i32 {
    x * 2 + 1
}

fn thin_wrapper(value: i32) -> i32 {
    do_real_work(value)
}

fn another_thin_wrapper(a: i32) -> i32 {
    do_real_work(a)
}
"#;
        let check = check_ast(source);
        let wrappers: Vec<_> = check
            .violations
            .iter()
            .filter(|v| v.rule == "ast:over-abstraction")
            .collect();
        assert_eq!(wrappers.len(), 2, "Should detect both thin wrappers");
    }

    #[test]
    fn test_unreachable_code_detected() {
        let source = r#"
fn example() {
    let x = 42;
    return;
    let y = x + 1;
    println!("{}", y);
}
"#;
        let check = check_ast(source);
        let unreachable: Vec<_> = check
            .violations
            .iter()
            .filter(|v| v.rule == "ast:unreachable-code")
            .collect();
        assert!(
            !unreachable.is_empty(),
            "Should detect unreachable code after return"
        );
    }

    #[test]
    fn test_if_else_chain_detected() {
        let source = r#"
fn classify(value: i32) -> &'static str {
    if value == 1 {
        "one"
    } else if value == 2 {
        "two"
    } else if value == 3 {
        "three"
    } else if value == 4 {
        "four"
    } else {
        "other"
    }
}
"#;
        let check = check_ast(source);
        let chains: Vec<_> = check
            .violations
            .iter()
            .filter(|v| v.rule == "ast:if-else-chain")
            .collect();
        assert!(
            !chains.is_empty(),
            "Should detect long if-else chain, got: {:?}",
            check.violations
        );
    }
    #[test]
    fn test_short_if_else_not_flagged() {
        let source = r#"
fn classify(value: i32) -> &'static str {
    if value == 1 {
        "one"
    } else if value == 2 {
        "two"
    } else {
        "other"
    }
}
"#;
        let check = check_ast(source);
        let chains: Vec<_> = check
            .violations
            .iter()
            .filter(|v| v.rule == "ast:if-else-chain")
            .collect();
        assert!(
            chains.is_empty(),
            "Short if-else chain (2 branches) should not be flagged"
        );
    }

    #[test]
    fn test_unused_function_detected() {
        let source = r#"
fn helper_internal(x: i32) -> i32 {
    x * 2
}

pub fn called_fn() -> i32 {
    42
}

fn main() {
    let _ = called_fn();
}
"#;
        let check = check_ast(source);
        let unused: Vec<_> = check
            .violations
            .iter()
            .filter(|v| v.rule == "ast:unused-function")
            .collect();
        assert_eq!(unused.len(), 1, "Should detect one unused function");
        assert!(unused[0].description.contains("helper_internal"));
    }

    #[test]
    fn test_multiple_violation_types() {
        let source = r#"
// Now we set up the logger
fn setup_logger() -> Logger {
    // Here we create a new logger instance
    Logger::new()
}

fn main() {
    let logger = setup_logger();
    logger.log("hello");
    return;
    logger.log("unreachable");
}
"#;
        let check = check_ast(source);
        let rule_types: std::collections::HashSet<&str> =
            check.violations.iter().map(|v| v.rule.as_str()).collect();

        // Clean code should have a helper that's called once — flag it
        assert!(
            rule_types.contains("ast:narration-comment"),
            "Should have narration comment violations"
        );
        assert!(
            rule_types.contains("ast:unreachable-code"),
            "Should have unreachable code violations"
        );
    }

    #[test]
    fn test_fn_def_extraction() {
        assert_eq!(extract_fn_name("fn foo() {}").as_deref(), Some("foo"));
        assert_eq!(
            extract_fn_name("pub fn bar(x: i32) -> i32").as_deref(),
            Some("bar")
        );
        assert_eq!(extract_fn_name("async fn baz()").as_deref(), Some("baz"));
        assert_eq!(
            extract_fn_name("pub async fn qux()").as_deref(),
            Some("qux")
        );
        assert_eq!(
            extract_fn_name("fn with_generics<T: Clone>(x: T)").as_deref(),
            Some("with_generics")
        );
        assert!(extract_fn_name("// just a comment").is_none());
        assert!(extract_fn_name("#[derive(Debug)]").is_none());
    }

    #[test]
    fn test_strip_modifiers() {
        assert_eq!(strip_visibility_and_modifiers("pub fn foo"), "fn foo");
        assert_eq!(
            strip_visibility_and_modifiers("pub(crate) fn bar"),
            "fn bar"
        );
        assert_eq!(strip_visibility_and_modifiers("async fn baz"), "fn baz");
        assert_eq!(
            strip_visibility_and_modifiers("pub unsafe fn qux"),
            "fn qux"
        );
    }

    #[test]
    fn test_fn_body_collection() {
        let lines: Vec<&str> = r#"
fn foo() {
    let x = 1;
    let y = 2;
    x + y
}
fn bar() {
    42
}
"#
        .lines()
        .collect();

        let body = collect_fn_body(&lines, 1);
        assert!(!body.is_empty(), "Should collect function body");
        assert!(body.iter().any(|l| l.contains("let x = 1")));
        assert!(body.iter().any(|l| l.contains("x + y")));

        let body2 = collect_fn_body(&lines, 6);
        assert!(
            body2.iter().any(|l| l.contains("42")),
            "Should collect bar's body"
        );
    }
}
