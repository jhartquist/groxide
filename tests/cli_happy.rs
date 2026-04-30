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
        stdout.contains("src/lib.rs:"),
        "should have source file location in header: {stdout}"
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
        .arg("groxide_test_api::GenericStruct")
        .arg("--impls")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("trait implementations:"),
        "should show trait implementations: {stdout}"
    );

    insta::assert_snapshot!("impls_mode", stdout);
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
        stdout.contains("src/lib.rs:"),
        "should have source location in header: {stdout}"
    );
    assert!(
        stdout.contains("pub struct GenericStruct"),
        "should contain struct source: {stdout}"
    );

    insta::assert_snapshot!("source_struct", stdout);
}

#[test]
fn source_mode_shows_source_instead_of_docs() {
    let output = grox()
        .arg("--source")
        .arg("groxide_test_api::add")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should NOT include rendered docs (source replaces docs)
    assert!(
        !stdout.contains("Adds two numbers"),
        "should not contain doc text in source mode: {stdout}"
    );
    // Should include source code
    assert!(
        stdout.contains("a + b"),
        "should contain source code: {stdout}"
    );
    // Should include source header
    assert!(
        stdout.contains("src/lib.rs:"),
        "should have source location in header: {stdout}"
    );
}

#[test]
fn impls_on_trait() {
    let output = grox()
        .arg("groxide_test_api::traits::Stringify")
        .arg("--impls")
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
        stdout.contains("variants:"),
        "should show variants section: {stdout}"
    );
    assert!(
        stdout.contains("Circle"),
        "should list Circle variant: {stdout}"
    );

    insta::assert_snapshot!("enum_with_data_variants", stdout);
}

// ── Re-exports module shows real signatures ──────────────────────────

#[test]
fn reexports_module_shows_real_signatures() {
    let output = grox()
        .arg("groxide_test_api::reexports")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // In-crate re-exports should show real signatures, not "pub use ... as ..." stubs
    assert!(
        !stdout.contains("pub use inner::"),
        "should not show 'pub use' stubs for in-crate re-exports: {stdout}"
    );
    assert!(
        stdout.contains("pub fn inner_fn() -> i32"),
        "should show real function signature: {stdout}"
    );
    assert!(
        stdout.contains("pub fn glob_fn() -> bool"),
        "should show real glob-reexported function signature: {stdout}"
    );

    insta::assert_snapshot!("reexports_module", stdout);
}

#[test]
fn reexported_struct_shows_fields_and_impls() {
    let output = grox()
        .arg("groxide_test_api::reexports::Helper")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("pub struct Helper"),
        "should show real struct signature: {stdout}"
    );
    assert!(stdout.contains("id"), "should show struct fields: {stdout}");

    insta::assert_snapshot!("reexported_struct", stdout);
}

// ── Re-export resolution on not-found path ──────────────────────────

#[test]
fn reexport_resolves_transparently_with_wrong_module_path() {
    // Query Helper via a non-existent module path — the re-export fallback
    // should find it under reexports::Helper since it's re-exported from inner.
    let output = grox()
        .arg("groxide_test_api::wrong_module::Helper")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "should resolve via re-export fallback, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Helper"), "should show Helper: {stdout}");
}

// ── --recursive mode ──────────────────────────────────────────────────

#[test]
fn recursive_lists_all_items() {
    let output = grox()
        .arg("--recursive")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain items from nested modules
    assert!(
        stdout.contains("groxide_test_api::containers::Stack"),
        "should list nested struct: {stdout}"
    );
    assert!(
        stdout.contains("groxide_test_api::traits::Stringify"),
        "should list nested trait: {stdout}"
    );

    insta::assert_snapshot!("recursive_crate_root", stdout);
}

#[test]
fn recursive_with_json() {
    let output = grox()
        .arg("--recursive")
        .arg("--json")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Each line should be valid JSON
    for line in stdout.lines() {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("each line should be valid JSON");
        assert!(
            parsed.get("path").is_some(),
            "should have path field: {line}"
        );
    }

    insta::assert_snapshot!("recursive_json", stdout);
}

#[test]
fn recursive_with_kind_filter() {
    let output = grox()
        .arg("--recursive")
        .arg("--kind")
        .arg("fn")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should only contain functions
    for line in stdout.lines() {
        if line.ends_with(':') || line.is_empty() {
            continue; // skip section headers and blank lines
        }
        assert!(line.contains("fn"), "all items should be functions: {line}");
    }

    insta::assert_snapshot!("recursive_kind_filter", stdout);
}

// ── --impls with trait filter ────────────────────────────────────────

#[test]
fn impls_filter_by_trait_name() {
    let output = grox()
        .arg("--impls-of")
        .arg("Default")
        .arg("groxide_test_api::containers::Stack")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Default"),
        "should show Default impl: {stdout}"
    );
    // Should NOT show synthetic impls like Send/Sync when filtering
    assert!(
        !stdout.contains("Send"),
        "should not show unrelated impls: {stdout}"
    );

    insta::assert_snapshot!("impls_filter_default", stdout);
}

#[test]
fn impls_filter_no_match() {
    let output = grox()
        .arg("--impls-of")
        .arg("NonexistentTrait")
        .arg("groxide_test_api::containers::Stack")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No implementation of 'NonexistentTrait' found"),
        "should show no-match message: {stdout}"
    );

    insta::assert_snapshot!("impls_filter_no_match", stdout);
}

#[test]
fn impls_bare_flag_still_works() {
    let output = grox()
        .arg("groxide_test_api::containers::Stack")
        .arg("--impls")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("trait implementations:"),
        "should show all trait implementations: {stdout}"
    );

    insta::assert_snapshot!("impls_bare_flag", stdout);
}

// ── --recursive composes with detail flags ───────────────────────────

#[test]
fn recursive_and_source_composable() {
    let output = grox()
        .args(["-r", "-s", "groxide_test_api"])
        .output()
        .expect("command runs");
    // Should not fail with conflict error (exit 2)
    assert_ne!(output.status.code(), Some(2));
}

#[test]
fn recursive_and_brief_composable() {
    let output = grox()
        .args(["-r", "-b", "groxide_test_api"])
        .output()
        .expect("command runs");
    assert_ne!(output.status.code(), Some(2));
}

#[test]
fn recursive_and_docs_composable() {
    let output = grox()
        .args(["-r", "-d", "groxide_test_api"])
        .output()
        .expect("command runs");
    assert_ne!(output.status.code(), Some(2));
}

// ── --clear-cache ────────────────────────────────────────────────────

// ── Brief mode ──────────────────────────────────────────────────────

#[test]
fn brief_mode_crate_root() {
    let output = grox()
        .arg("-b")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("brief_crate_root", stdout);

    // Verify no signatures appear
    assert!(!stdout.contains("pub fn"), "should not contain signatures");
    assert!(
        !stdout.contains("pub struct"),
        "should not contain signatures"
    );
}

#[test]
fn brief_mode_module() {
    let output = grox()
        .arg("-b")
        .arg("groxide_test_api::containers")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("brief_module", stdout);

    assert!(!stdout.contains("pub fn"), "should not contain signatures");
    assert!(
        !stdout.contains("pub struct"),
        "should not contain signatures"
    );
}

#[test]
fn brief_recursive_crate_root() {
    let output = grox()
        .arg("-r")
        .arg("-b")
        .arg("groxide_test_api")
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("brief_recursive_crate_root", stdout);

    assert!(!stdout.contains("pub fn"), "should not contain signatures");
    assert!(
        !stdout.contains("pub struct"),
        "should not contain signatures"
    );
}

// ── Docs mode ───────────────────────────────────────────────────────

#[test]
fn docs_mode_function() {
    let output = grox()
        .args(["-d", "groxide_test_api::add"])
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("docs_mode_function", stdout);

    // Should show full docs (same as default for single items)
    assert!(stdout.contains("add"), "should contain function name");
    assert!(stdout.contains("fn"), "should contain kind");
}

#[test]
fn docs_mode_recursive() {
    let output = grox()
        .args(["-r", "-d", "groxide_test_api::containers"])
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("docs_mode_recursive", stdout);

    // Should show full docs, not just one-line summaries
    assert!(
        stdout.contains("pub struct") || stdout.contains("pub fn"),
        "should contain signatures: {stdout}"
    );
}

// ── Recursive + source mode ──────────────────────────────────────────

#[test]
fn recursive_source_module() {
    let output = grox()
        .args(["-r", "-s", "groxide_test_api::containers"])
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    insta::assert_snapshot!("recursive_source_module", stdout);
}

#[test]
fn recursive_source_crate_root() {
    let output = grox()
        .args(["-r", "-s", "groxide_test_api"])
        .output()
        .expect("command runs");

    assert!(output.status.success(), "exit code should be 0");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should contain source from multiple modules
    assert!(
        stdout.contains("src/"),
        "should contain source locations in headers: {stdout}"
    );
    // Should contain items from different modules
    assert!(
        stdout.contains("containers"),
        "should contain containers module items"
    );
}

// ── --clear-cache ────────────────────────────────────────────────────

#[test]
fn clear_cache_exits_successfully() {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("grox"))
        .arg("--clear-cache")
        .output()
        .expect("command runs");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[grox]"),
        "should print status message: {stderr}"
    );
}

// ── Feature hint on zero search results ──────────────────────────────

#[test]
fn search_zero_results_hints_features() {
    // Use --no-default-features to bypass the feature cascade (which tries
    // --all-features by default). This builds without `unstable`, so
    // `unstable_api` won't be in the index, triggering the hint.
    let output = grox()
        .args([
            "--no-default-features",
            "-S",
            "unstable_api",
            "groxide_test_api",
        ])
        .output()
        .expect("command runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains("0 results"),
        "should show 0 results on stdout: {stdout}"
    );
    assert!(
        stderr.contains("hint:") && stderr.contains("--all-features"),
        "should hint about --all-features on stderr: {stderr}"
    );
}
