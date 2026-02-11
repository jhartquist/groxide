//! End-to-end CLI integration tests for groxide — happy paths.
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

// ── Default output for a struct ──────────────────────────────────────

#[test]
fn default_output_struct() {
    let output = grox()
        .arg("groxide_test_api::GenericStruct")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("struct_default", stdout);
}

// ── Default output for a module ──────────────────────────────────────

#[test]
fn default_output_module() {
    let output = grox()
        .arg("groxide_test_api::containers")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("module_default", stdout);
}

// ── Default output for a function ────────────────────────────────────

#[test]
fn default_output_function() {
    let output = grox()
        .arg("groxide_test_api::add")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("function_default", stdout);
}

// ── Default output for a trait ───────────────────────────────────────

#[test]
fn default_output_trait() {
    let output = grox()
        .arg("groxide_test_api::traits::Stringify")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("trait_default", stdout);
}

// ── Default output for an enum ───────────────────────────────────────

#[test]
fn default_output_enum() {
    let output = grox()
        .arg("groxide_test_api::Direction")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("enum_default", stdout);
}

// ── Default output for crate root ────────────────────────────────────

#[test]
fn default_output_crate_root() {
    let output = grox()
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("crate_root_default", stdout);
}

// ── --list mode ──────────────────────────────────────────────────────

#[test]
fn list_mode() {
    let output = grox()
        .arg("--list")
        .arg("groxide_test_api::containers")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("list_mode", stdout);
}

// ── --json mode ──────────────────────────────────────────────────────

#[test]
fn json_mode() {
    let output = grox()
        .arg("--json")
        .arg("groxide_test_api::GenericStruct")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Verify it's valid JSON
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout).expect("output should be valid JSON");
    assert_eq!(parsed["kind"], "struct");
    assert!(
        parsed["path"]
            .as_str()
            .unwrap_or("")
            .contains("GenericStruct"),
        "JSON path should contain GenericStruct"
    );
    assert!(parsed["methods"].is_array(), "should have methods array");
    assert!(
        parsed["trait_impls"].is_array(),
        "should have trait_impls array"
    );

    insta::assert_snapshot!("json_mode", stdout);
}

// ── --json --list combined ───────────────────────────────────────────

#[test]
fn json_list_combined() {
    let output = grox()
        .arg("--json")
        .arg("--list")
        .arg("groxide_test_api::containers")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("json_list_combined", stdout);
}

// ── --search "query" mode ────────────────────────────────────────────

#[test]
fn search_mode() {
    let output = grox()
        .arg("--search")
        .arg("add")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("results for \"add\""),
        "should have results header: {stdout}"
    );
    assert!(
        stdout.contains("add"),
        "should find the add function: {stdout}"
    );

    insta::assert_snapshot!("search_mode", stdout);
}

// ── --source mode ────────────────────────────────────────────────────

#[test]
fn source_mode() {
    let output = grox()
        .arg("--source")
        .arg("groxide_test_api::add")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("// src/lib.rs:"),
        "should have source file header: {stdout}"
    );
    assert!(
        stdout.contains("pub fn add"),
        "should contain the function source: {stdout}"
    );

    insta::assert_snapshot!("source_mode", stdout);
}

// ── --impls mode ─────────────────────────────────────────────────────

#[test]
fn impls_mode() {
    let output = grox()
        .arg("--impls")
        .arg("groxide_test_api::GenericStruct")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Trait Implementations:"),
        "should show trait implementations: {stdout}"
    );

    insta::assert_snapshot!("impls_mode", stdout);
}

// ── --all expands truncation ─────────────────────────────────────────

#[test]
fn all_expands_truncation() {
    // Without --all: trait impls may be truncated
    let without_all = grox()
        .arg("groxide_test_api::GenericStruct")
        .output()
        .expect("command runs");
    assert!(without_all.status.success());
    let without_all_stdout = String::from_utf8_lossy(&without_all.stdout);

    // With --all: no truncation
    let with_all = grox()
        .arg("--all")
        .arg("groxide_test_api::GenericStruct")
        .output()
        .expect("command runs");
    assert!(with_all.status.success());
    let with_all_stdout = String::from_utf8_lossy(&with_all.stdout);

    // --all version should show all trait impls without truncation notice
    assert!(
        !with_all_stdout.contains("use --impls to expand"),
        "--all should expand trait impls"
    );
    assert!(
        !with_all_stdout.contains("use --all to expand"),
        "--all should expand methods"
    );
    // If the default was truncated, --all should show all items
    if without_all_stdout.contains("use --impls to expand") {
        // --all removes the truncation notice but shows all impls
        assert!(
            with_all_stdout.contains("Trait Implementations:\n"),
            "--all should have untruncated Trait Implementations header"
        );
    }

    insta::assert_snapshot!("all_expanded", with_all_stdout);
}

// ── --kind fn filters to functions ───────────────────────────────────

#[test]
fn kind_filter_fn() {
    let output = grox()
        .arg("--kind")
        .arg("fn")
        .arg("groxide_test_api::add")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fn groxide_test_api::add"),
        "should match the function: {stdout}"
    );

    insta::assert_snapshot!("kind_filter_fn", stdout);
}

// ── --private includes private items ─────────────────────────────────

#[test]
fn private_flag_accepted() {
    let output = grox()
        .arg("--private")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // The --private flag should be accepted and produce valid output
    assert!(
        stdout.contains("groxide_test_api"),
        "should produce crate root output: {stdout}"
    );

    insta::assert_snapshot!("private_flag", stdout);
}

// ── Additional happy path tests ──────────────────────────────────────

#[test]
fn constant_output() {
    let output = grox()
        .arg("groxide_test_api::MAX_BUFFER_SIZE")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("const"), "should show constant: {stdout}");

    insta::assert_snapshot!("constant_default", stdout);
}

#[test]
fn type_alias_output() {
    let output = grox()
        .arg("groxide_test_api::Result")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("type_alias_default", stdout);
}

#[test]
fn list_mode_on_struct_shows_methods() {
    let output = grox()
        .arg("--list")
        .arg("groxide_test_api::containers::Stack")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("fn"),
        "list should show fn methods: {stdout}"
    );

    insta::assert_snapshot!("list_struct_methods", stdout);
}

#[test]
fn json_mode_module() {
    let output = grox()
        .arg("--json")
        .arg("groxide_test_api::containers")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Module JSON produces JSON Lines (first line = module, rest = children)
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(!lines.is_empty(), "should have at least one line");
    // First line should be valid JSON
    let first: serde_json::Value =
        serde_json::from_str(lines[0]).expect("first line should be valid JSON");
    assert_eq!(first["kind"], "mod");

    insta::assert_snapshot!("json_mode_module", stdout);
}

#[test]
fn search_with_json_mode() {
    let output = grox()
        .arg("--json")
        .arg("--search")
        .arg("stack")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each line should be valid JSON with a score field
    for line in stdout.lines() {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("each line should be valid JSON");
        assert!(
            parsed.get("score").is_some(),
            "should have score field: {line}"
        );
    }

    insta::assert_snapshot!("search_json", stdout);
}

#[test]
fn source_mode_struct() {
    let output = grox()
        .arg("--source")
        .arg("groxide_test_api::GenericStruct")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("// src/lib.rs:"),
        "should have source header: {stdout}"
    );
    assert!(
        stdout.contains("pub struct GenericStruct"),
        "should contain struct source: {stdout}"
    );

    insta::assert_snapshot!("source_struct", stdout);
}

#[test]
fn impls_on_trait() {
    let output = grox()
        .arg("--impls")
        .arg("groxide_test_api::traits::Stringify")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should show implementors section (may be empty)
    assert!(
        stdout.contains("trait groxide_test_api::traits::Stringify"),
        "should show trait header: {stdout}"
    );

    insta::assert_snapshot!("impls_trait", stdout);
}

#[test]
fn deep_nested_path() {
    let output = grox()
        .arg("groxide_test_api::deeply")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("deeply"),
        "should show deeply module: {stdout}"
    );

    insta::assert_snapshot!("deep_nested", stdout);
}

#[test]
fn method_lookup() {
    let output = grox()
        .arg("groxide_test_api::GenericStruct::new")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("new"),
        "should show the new method: {stdout}"
    );

    insta::assert_snapshot!("method_lookup", stdout);
}

#[test]
fn list_mode_crate_root() {
    let output = grox()
        .arg("--list")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("struct"),
        "list should show struct kind: {stdout}"
    );
    assert!(
        stdout.contains("mod"),
        "list should show mod kind: {stdout}"
    );

    insta::assert_snapshot!("list_crate_root", stdout);
}

#[test]
fn kind_filter_struct() {
    let output = grox()
        .arg("--kind")
        .arg("struct")
        .arg("groxide_test_api::GenericStruct")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("struct groxide_test_api::GenericStruct"),
        "should match the struct: {stdout}"
    );
}

#[test]
fn enum_with_variants() {
    let output = grox()
        .arg("groxide_test_api::Shape")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Variants:"),
        "should show variants section: {stdout}"
    );
    assert!(
        stdout.contains("Circle"),
        "should list Circle variant: {stdout}"
    );

    insta::assert_snapshot!("enum_with_data_variants", stdout);
}
