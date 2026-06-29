use exohook::filter::{build_globset, filter_files};

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn filter_files_basic_match() {
    let files = list(&["src/main.rs", "src/lib.rs", "README.md"]);
    let filters = list(&["src/*.rs"]);
    let filtered = filter_files(&files, &filters).expect("filter should succeed");
    assert_eq!(filtered, list(&["src/main.rs", "src/lib.rs"]));
}

#[test]
fn filter_files_empty_filters_returns_all() {
    let files = list(&["a.rs", "b.txt"]);
    let filtered = filter_files(&files, &[]).expect("filter should succeed");
    assert_eq!(filtered, files);
}

#[test]
fn filter_files_empty_files_returns_empty() {
    let files: Vec<String> = Vec::new();
    let filters = list(&["*.rs"]);
    let filtered = filter_files(&files, &filters).expect("filter should succeed");
    assert!(filtered.is_empty());
}

#[test]
fn filter_files_multiple_patterns_or_semantics() {
    let files = list(&["src/main.rs", "README.md", "notes.txt"]);
    let filters = list(&["*.md", "src/*.rs"]);
    let filtered = filter_files(&files, &filters).expect("filter should succeed");
    assert_eq!(filtered, list(&["src/main.rs", "README.md"]));
}

#[test]
fn filter_files_negation_pattern_is_literal() {
    let files = list(&["main.rs", "!main.rs"]);
    let filters = list(&["!*.rs"]);
    let filtered = filter_files(&files, &filters).expect("filter should succeed");
    assert_eq!(filtered, list(&["!main.rs"]));
}

#[test]
fn filter_files_brace_expansion_matches_multiple_extensions() {
    let files = list(&["app.js", "app.ts", "app.rs"]);
    let filters = list(&["*.{js,ts}"]);
    let filtered = filter_files(&files, &filters).expect("filter should succeed");
    assert_eq!(filtered, list(&["app.js", "app.ts"]));
}

#[test]
fn filter_files_path_separators_match_nested_paths() {
    let files = list(&["src/lib.rs", "src/nested/mod.rs", "tests/mod.rs"]);
    let filters = list(&["src/**/*.rs"]);
    let filtered = filter_files(&files, &filters).expect("filter should succeed");
    assert_eq!(filtered, list(&["src/lib.rs", "src/nested/mod.rs"]));
}

#[test]
fn filter_files_case_sensitive_by_default() {
    let files = list(&["README.md", "README.MD"]);
    let filters = list(&["*.md"]);
    let filtered = filter_files(&files, &filters).expect("filter should succeed");
    assert_eq!(filtered, list(&["README.md"]));
}

#[test]
fn build_globset_rejects_invalid_glob() {
    let filters = list(&["["]);
    let err = build_globset(&filters).expect_err("invalid glob should error");
    assert!(err.to_string().contains("invalid filter glob '['"));
}

#[test]
fn filter_files_reports_invalid_glob_in_context() {
    let files = list(&["src/lib.rs"]);
    let filters = list(&["**/[abc"]);
    let err = filter_files(&files, &filters).expect_err("invalid glob should error");
    assert!(err.to_string().contains("invalid filter glob '**/[abc'"));
}
