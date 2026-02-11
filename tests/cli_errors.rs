//! End-to-end CLI integration tests for groxide — edge cases and error scenarios.
//!
//! All tests run the `grox` binary via `assert_cmd` against the fixture crate
//! at `test-fixtures/groxide_test_api/` using `--manifest-path`.
//! Snapshot tests use `insta` for output format stability.

use assert_cmd::Command;

/// Returns the absolute path to the fixture crate's Cargo.toml.
fn fixture_manifest() -> String {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    format!("{manifest_dir}/test-fixtures/groxide_test_api/Cargo.toml")
}

/// Builds a `grox` command pre-configured with the fixture crate manifest.
fn grox() -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("grox"));
    cmd.arg("--manifest-path").arg(fixture_manifest());
    cmd
}

/// Builds a bare `grox` command without any manifest path.
fn grox_bare() -> Command {
    Command::new(assert_cmd::cargo::cargo_bin!("grox"))
}

// ── Typo query produces "not found" with exit code 1 ─────────────────

#[test]
fn typo_query_exits_1_with_not_found_message() {
    let output = grox()
        .arg("groxide_test_api::GenericStruxt")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "typo should exit with code 1"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no item matching"),
        "stderr should contain 'no item matching': {stderr}"
    );
    assert!(
        stderr.contains("GenericStruxt"),
        "stderr should mention the typo: {stderr}"
    );
}

// ── Typo in method name produces not found ───────────────────────────

#[test]
fn typo_method_exits_1() {
    let output = grox()
        .arg("groxide_test_api::GenericStruct::neww")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "method typo should exit with code 1"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no item matching"),
        "stderr should contain 'no item matching': {stderr}"
    );
}

// ── Unknown item name produces exit code 1 ───────────────────────────

#[test]
fn unknown_item_exits_1() {
    let output = grox()
        .arg("groxide_test_api::CompletelyFakeItem")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(1),
        "unknown item should exit with code 1"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no item matching"),
        "stderr should contain error message: {stderr}"
    );
    assert!(
        stderr.contains("groxide_test_api"),
        "stderr should mention the crate: {stderr}"
    );
    // stdout should be empty for errors
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on error: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

// ── Invalid flag combinations (exit code 2) ──────────────────────────

#[test]
fn conflicting_source_and_list_exits_2() {
    let output = grox_bare()
        .arg("--source")
        .arg("--list")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "--source --list should exit with code 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot be used with"),
        "should report conflict: {stderr}"
    );
}

#[test]
fn conflicting_source_and_impls_exits_2() {
    let output = grox_bare()
        .arg("--source")
        .arg("--impls")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "--source --impls should exit with code 2"
    );
}

#[test]
fn conflicting_search_and_source_exits_2() {
    let output = grox_bare()
        .arg("--search")
        .arg("foo")
        .arg("--source")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "--search --source should exit with code 2"
    );
}

#[test]
fn conflicting_search_and_list_exits_2() {
    let output = grox_bare()
        .arg("--search")
        .arg("foo")
        .arg("--list")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "--search --list should exit with code 2"
    );
}

#[test]
fn conflicting_readme_and_source_exits_2() {
    let output = grox_bare()
        .arg("--readme")
        .arg("--source")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "--readme --source should exit with code 2"
    );
}

#[test]
fn invalid_kind_value_exits_2() {
    let output = grox_bare()
        .arg("--kind")
        .arg("widget")
        .arg("something")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "invalid --kind value should exit with code 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("widget") || stderr.contains("invalid"),
        "should report the invalid value: {stderr}"
    );
}

// ── Empty search query (exit code 2) ─────────────────────────────────

#[test]
fn empty_search_query_exits_2() {
    let output = grox()
        .arg("--search")
        .arg("")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "empty search should exit with code 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("search query cannot be empty"),
        "should report empty query error: {stderr}"
    );
}

#[test]
fn whitespace_only_search_query_exits_2() {
    let output = grox()
        .arg("--search")
        .arg("   ")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "whitespace-only search should exit with code 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("search query cannot be empty"),
        "should report empty query error: {stderr}"
    );
}

// ── --version output ─────────────────────────────────────────────────

#[test]
fn version_flag_prints_version_and_exits_0() {
    let output = grox_bare().arg("--version").output().expect("command runs");

    assert!(output.status.success(), "--version should exit 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("grox "),
        "--version should print 'grox <version>': {stdout}"
    );
    // Check it contains a semver-like pattern
    assert!(
        stdout.contains('.'),
        "version should contain a dot (semver): {stdout}"
    );
}

// ── --help output (snapshot) ─────────────────────────────────────────

#[test]
fn help_flag_prints_usage_and_exits_0() {
    let output = grox_bare().arg("--help").output().expect("command runs");

    assert!(output.status.success(), "--help should exit 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Query Rust crate documentation from the terminal"),
        "help should contain description: {stdout}"
    );
    assert!(
        stdout.contains("EXAMPLES:"),
        "help should contain examples: {stdout}"
    );

    insta::assert_snapshot!("help_output", stdout);
}

#[test]
fn short_help_flag_prints_summary() {
    let output = grox_bare().arg("-h").output().expect("command runs");

    assert!(output.status.success(), "-h should exit 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Query Rust crate documentation from the terminal"),
        "-h should contain description: {stdout}"
    );
}

// ── Deep nested path query ───────────────────────────────────────────

#[test]
fn deep_nested_path_query_resolves_4_level_path() {
    let output = grox()
        .arg("groxide_test_api::deeply::nested::deep_fn")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "4-level path should resolve successfully"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("deep_fn"), "should show deep_fn: {stdout}");
    assert!(
        stdout.contains("deeply::nested"),
        "should show the full nested path: {stdout}"
    );

    insta::assert_snapshot!("deep_nested_fn", stdout);
}

#[test]
fn deep_nested_struct_resolves() {
    let output = grox()
        .arg("groxide_test_api::deeply::nested::InnerItem")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "deeply nested struct should resolve"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("InnerItem"),
        "should show InnerItem: {stdout}"
    );

    insta::assert_snapshot!("deep_nested_struct", stdout);
}

#[test]
fn deep_nested_constant_resolves() {
    let output = grox()
        .arg("groxide_test_api::deeply::nested::DEPTH")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "deeply nested constant should resolve"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("DEPTH"),
        "should show DEPTH constant: {stdout}"
    );
}

// ── Unicode in doc comments ──────────────────────────────────────────

#[test]
fn unicode_in_docs_renders_correctly() {
    let output = grox()
        .arg("groxide_test_api::unicode_docs")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "unicode query should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Japanese characters
    assert!(
        stdout.contains("こんにちは"),
        "should render Japanese: {stdout}"
    );
    // French accented characters
    assert!(
        stdout.contains("café"),
        "should render French accents: {stdout}"
    );
    assert!(
        stdout.contains("naïve"),
        "should render diaeresis: {stdout}"
    );
    // Emoji
    assert!(stdout.contains('🦀'), "should render crab emoji: {stdout}");
    // Mathematical symbols
    assert!(stdout.contains('∀'), "should render math symbols: {stdout}");
    assert!(stdout.contains('ℝ'), "should render set symbols: {stdout}");

    insta::assert_snapshot!("unicode_docs", stdout);
}

// ── crate@version syntax ─────────────────────────────────────────────

#[test]
fn crate_at_missing_version_exits_2() {
    let output = grox().arg("mycrate@").output().expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "crate@ with missing version should exit 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing version after @"),
        "should report missing version: {stderr}"
    );
}

#[test]
fn crate_at_version_parses_correctly() {
    // We can't actually test fetching a real versioned crate without network.
    // Instead, verify the parsing works by checking a versioned query of the
    // fixture crate name — it will try to fetch externally and fail (exit 1),
    // but the parse itself should succeed (no "missing version" error).
    let output = grox()
        .arg("groxide_test_api@99.99.99")
        .output()
        .expect("command runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should NOT get parse errors — the @ syntax was accepted
    assert!(
        !stderr.contains("missing version after @"),
        "should not report parse error: {stderr}"
    );
    assert!(
        !stderr.contains("@ prefix is no longer supported"),
        "should not report prefix error: {stderr}"
    );
}

// ── @crate syntax (helpful error) ────────────────────────────────────

#[test]
fn at_prefix_syntax_exits_2_with_helpful_message() {
    let output = grox().arg("@serde").output().expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "@crate should exit with code 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("the @ prefix is no longer supported"),
        "should explain the @ prefix: {stderr}"
    );
    assert!(
        stderr.contains("grox serde"),
        "should suggest the correct usage: {stderr}"
    );
}

#[test]
fn at_prefix_with_path_exits_2() {
    let output = grox()
        .arg("@tokio::sync::Mutex")
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "@crate::path should exit with code 2"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("the @ prefix is no longer supported"),
        "should explain the @ prefix: {stderr}"
    );
    assert!(
        stderr.contains("grox tokio::sync::Mutex"),
        "should suggest the full path: {stderr}"
    );
}

// ── Error output routing: errors to stderr, nothing to stdout ────────

#[test]
fn error_output_goes_to_stderr_not_stdout() {
    let output = grox()
        .arg("groxide_test_api::NonexistentThing")
        .output()
        .expect("command runs");

    assert!(!output.status.success(), "should fail");
    assert!(
        output.stdout.is_empty(),
        "stdout should be empty on error: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no item matching"),
        "error should be on stderr: {stderr}"
    );
}

// ── Search with zero results exits 0 ─────────────────────────────────

#[test]
fn search_zero_results_exits_0() {
    let output = grox()
        .arg("--search")
        .arg("zzz_impossiblematch_qqq")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "search with no results should exit 0"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0 results"),
        "should show 0 results: {stdout}"
    );
}

// ── Missing path without manifest exits 2 (ManifestNotFound) ─────────

#[test]
fn no_path_outside_project_exits_2() {
    let output = grox_bare()
        .current_dir(std::env::temp_dir())
        .output()
        .expect("command runs");

    assert_eq!(
        output.status.code(),
        Some(2),
        "no path outside project should exit 2 (ManifestNotFound)"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not in a Rust project"),
        "should report not in a Rust project: {stderr}"
    );
}
