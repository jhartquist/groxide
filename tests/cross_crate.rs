//! Cross-crate validation tests for groxide.
//!
//! These tests run the `grox` binary against real crates (dependencies and
//! external crates) to verify output quality. They require `cargo +nightly`
//! and may require network access, so they are marked `#[ignore]`.
//!
//! Run with: `cargo test --test cross_crate -- --ignored`

use assert_cmd::Command;

/// Builds a `grox` command targeting the groxide project itself.
fn grox() -> Command {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest = format!("{manifest_dir}/Cargo.toml");
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("grox"));
    cmd.arg("--manifest-path").arg(manifest);
    cmd
}

// ═══════════════════════════════════════════════════════════════════════
// Serde (direct dependency)
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain"]
fn serde_crate_root_produces_useful_output() {
    let output = grox().arg("serde").output().expect("command runs");

    assert!(output.status.success(), "serde root should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("mod serde"),
        "should show crate root as module: {stdout}"
    );
    assert!(
        stdout.contains("Serde"),
        "should contain crate description: {stdout}"
    );
    assert!(
        stdout.contains("serializ"),
        "should mention serialization: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn serde_serialize_trait_resolves() {
    let output = grox()
        .arg("serde::Serialize")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "serde::Serialize should resolve: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Serialize"),
        "should contain Serialize: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn serde_deserialize_trait_resolves() {
    let output = grox()
        .arg("serde::Deserialize")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "serde::Deserialize should resolve: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Deserialize"),
        "should contain Deserialize: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn serde_search_returns_results() {
    let output = grox()
        .arg("--search")
        .arg("Serialize")
        .arg("serde")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "serde search should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("results for"),
        "should have results header: {stdout}"
    );
    assert!(
        stdout.contains("Serialize"),
        "should find Serialize: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn serde_list_mode_shows_items() {
    let output = grox()
        .arg("--list")
        .arg("serde")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "serde --list should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // serde should have some visible items in list mode
    // (may be re-exports, modules, etc.)
    assert!(!stdout.is_empty(), "list output should not be empty");
}

// ═══════════════════════════════════════════════════════════════════════
// Clap (direct dependency)
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain"]
fn clap_crate_root_produces_useful_output() {
    let output = grox().arg("clap").output().expect("command runs");

    assert!(output.status.success(), "clap root should succeed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("mod clap"),
        "should show crate root: {stdout}"
    );
    assert!(
        stdout.contains("Command Line Argument Parser"),
        "should contain clap description: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn clap_search_finds_items() {
    let output = grox()
        .arg("--search")
        .arg("Parser")
        .arg("clap")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "clap search should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("results for"),
        "should have results header: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn clap_json_mode_produces_valid_json() {
    let output = grox()
        .arg("--json")
        .arg("clap")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "clap --json should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    // First line should be valid JSON
    let first_line = stdout.lines().next().unwrap_or("");
    let parsed: serde_json::Value =
        serde_json::from_str(first_line).expect("output should be valid JSON");
    assert_eq!(parsed["kind"], "mod", "clap root should be a module");
    assert!(
        parsed["doc"]
            .as_str()
            .unwrap_or("")
            .contains("Command Line"),
        "JSON doc should contain description"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Semver (direct dependency — good test because it has rich types)
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain"]
fn semver_version_struct_shows_fields_and_methods() {
    let output = grox()
        .arg("semver::Version")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "semver::Version should resolve: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("struct semver::Version"),
        "should show struct header: {stdout}"
    );
    assert!(
        stdout.contains("major"),
        "should show major field: {stdout}"
    );
    assert!(
        stdout.contains("minor"),
        "should show minor field: {stdout}"
    );
    assert!(
        stdout.contains("patch"),
        "should show patch field: {stdout}"
    );
    assert!(
        stdout.contains("Methods:"),
        "should have methods section: {stdout}"
    );
    assert!(stdout.contains("new"), "should show new method: {stdout}");
    assert!(
        stdout.contains("parse"),
        "should show parse method: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn semver_version_method_lookup() {
    let output = grox()
        .arg("semver::Version::new")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "semver::Version::new should resolve: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("new"), "should show new method: {stdout}");
    assert!(
        stdout.contains("major") || stdout.contains("Version"),
        "should show method details: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn semver_version_impls_shows_trait_implementations() {
    let output = grox()
        .arg("semver::Version")
        .arg("--impls")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "semver::Version --impls should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Display") || stdout.contains("Clone") || stdout.contains("Debug"),
        "should show well-known trait impls: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn semver_list_mode_shows_types() {
    let output = grox()
        .arg("--list")
        .arg("semver")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "semver --list should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Version"),
        "should list Version struct: {stdout}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// cargo_metadata (direct dependency — rich struct types)
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain"]
fn cargo_metadata_crate_root_produces_output() {
    let output = grox().arg("cargo_metadata").output().expect("command runs");

    assert!(
        output.status.success(),
        "cargo_metadata root should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("cargo_metadata"),
        "should contain crate name: {stdout}"
    );
    assert!(
        stdout.contains("cargo metadata"),
        "should mention cargo metadata: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn cargo_metadata_struct_shows_methods() {
    let output = grox()
        .arg("cargo_metadata::MetadataCommand")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "MetadataCommand should resolve: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("struct cargo_metadata::MetadataCommand"),
        "should show struct header: {stdout}"
    );
    assert!(
        stdout.contains("Methods:"),
        "should have methods section: {stdout}"
    );
    assert!(stdout.contains("exec"), "should show exec method: {stdout}");
    assert!(
        stdout.contains("manifest_path"),
        "should show manifest_path method: {stdout}"
    );
    assert!(
        stdout.contains("Trait Implementations:"),
        "should show trait impls: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn cargo_metadata_list_shows_many_items() {
    let output = grox()
        .arg("--list")
        .arg("cargo_metadata")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "cargo_metadata --list should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line_count = stdout.lines().count();
    assert!(
        line_count >= 10,
        "cargo_metadata should have many items, got {line_count} lines"
    );
    assert!(
        stdout.contains("struct"),
        "should contain struct items: {stdout}"
    );
    assert!(
        stdout.contains("Metadata"),
        "should list Metadata struct: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn cargo_metadata_search_finds_metadata() {
    let output = grox()
        .arg("--search")
        .arg("Metadata")
        .arg("cargo_metadata")
        .output()
        .expect("command runs");

    assert!(
        output.status.success(),
        "search should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Metadata"),
        "should find Metadata: {stdout}"
    );
    // Should find multiple results (Metadata struct, MetadataCommand, etc.)
    let first_line = stdout.lines().next().unwrap_or("");
    assert!(
        first_line.contains("results for"),
        "should have results count header: {first_line}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// thiserror (direct dependency — proc-macro crate)
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain"]
fn thiserror_crate_root_produces_output() {
    let output = grox().arg("thiserror").output().expect("command runs");

    assert!(
        output.status.success(),
        "thiserror root should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("thiserror"),
        "should contain crate name: {stdout}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Auto-fetch: small external crate (itoa — tiny, fast to build)
// ═══════════════════════════════════════════════════════════════════════
// Auto-fetch tests run from a temporary directory with no Cargo.toml,
// ensuring the tool falls through to crates.io fetching.

/// Builds a bare `grox` command running from a temp directory (no project context).
fn grox_auto_fetch() -> Command {
    let tmp = std::env::temp_dir();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("grox"));
    cmd.current_dir(tmp);
    cmd
}

#[test]
#[ignore = "requires nightly toolchain and network"]
fn auto_fetch_itoa_produces_useful_output() {
    let output = grox_auto_fetch()
        .arg("itoa")
        .output()
        .expect("command runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("fetching") || stderr.contains("cached") || stderr.contains("crates.io"),
        "should show fetch/cache message: {stderr}"
    );

    assert!(
        output.status.success(),
        "auto-fetch itoa should succeed: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("itoa"),
        "should contain crate name: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain and network"]
fn auto_fetch_itoa_search_works() {
    let output = grox_auto_fetch()
        .arg("--search")
        .arg("Buffer")
        .arg("itoa")
        .output()
        .expect("command runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "itoa search should succeed: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("results for"),
        "should have results header: {stdout}"
    );
    assert!(stdout.contains("Buffer"), "should find Buffer: {stdout}");
}

#[test]
#[ignore = "requires nightly toolchain and network"]
fn auto_fetch_itoa_list_mode_shows_items() {
    let output = grox_auto_fetch()
        .arg("--list")
        .arg("itoa")
        .output()
        .expect("command runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "itoa --list should succeed: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Buffer"),
        "should list Buffer struct: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain and network"]
fn auto_fetch_itoa_json_mode_produces_valid_json() {
    let output = grox_auto_fetch()
        .arg("--json")
        .arg("itoa")
        .output()
        .expect("command runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "itoa --json should succeed: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("");
    let parsed: serde_json::Value =
        serde_json::from_str(first_line).expect("output should be valid JSON");
    assert!(
        parsed["path"].as_str().unwrap_or("").contains("itoa"),
        "JSON path should contain itoa"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Typenum (recursion limit fix verification)
// ═══════════════════════════════════════════════════════════════════════
// typenum has deeply nested types that require the recursion limit fix
// in parse_rustdoc_json().

#[test]
#[ignore = "requires nightly toolchain and network; typenum build is slow"]
fn typenum_does_not_crash_with_recursion_limit() {
    let output = grox_auto_fetch()
        .arg("typenum")
        .output()
        .expect("command runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // The key requirement is that it doesn't crash with a stack overflow
    // or JSON parse error due to recursion limits.
    assert!(
        output.status.success(),
        "typenum should not crash (exit code: {:?}): stderr={stderr}",
        output.status.code()
    );

    assert!(
        stdout.contains("typenum"),
        "should contain crate name: {stdout}"
    );
}

#[test]
#[ignore = "requires nightly toolchain and network; typenum build is slow"]
fn typenum_search_works() {
    let output = grox_auto_fetch()
        .arg("--search")
        .arg("Integer")
        .arg("typenum")
        .output()
        .expect("command runs");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "typenum search should succeed: {stderr}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("results for"),
        "should have results header: {stdout}"
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Output quality: verify token-efficient, progressive disclosure
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain"]
fn output_is_token_efficient_for_struct() {
    let output = grox()
        .arg("semver::Version")
        .output()
        .expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output should be reasonable size (not dumping the entire crate)
    let char_count = stdout.len();
    assert!(
        char_count < 5000,
        "struct output should be concise, got {char_count} chars"
    );
    assert!(
        char_count > 200,
        "struct output should have meaningful content, got {char_count} chars"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn output_is_token_efficient_for_crate_root() {
    let output = grox().arg("serde").output().expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let char_count = stdout.len();
    // Crate root gets truncated docs; should be reasonable
    assert!(
        char_count < 10000,
        "crate root output should be concise, got {char_count} chars"
    );
    assert!(
        char_count > 100,
        "crate root output should have content, got {char_count} chars"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn progressive_disclosure_struct_shows_methods_summary() {
    let output = grox()
        .arg("cargo_metadata::MetadataCommand")
        .output()
        .expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Should show signature
    assert!(
        stdout.contains("pub struct MetadataCommand"),
        "should show signature: {stdout}"
    );

    // Should show methods as a table/list with summaries
    assert!(
        stdout.contains("Methods:"),
        "should have Methods section: {stdout}"
    );

    // Methods should show one-line summaries, not full docs
    // Count occurrences of method-like patterns
    let method_lines: Vec<&str> = stdout.lines().filter(|l| l.contains("pub fn ")).collect();
    assert!(
        method_lines.len() >= 3,
        "should list multiple methods: {method_lines:?}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn progressive_disclosure_crate_root_shows_children() {
    let output = grox().arg("cargo_metadata").output().expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Crate root should show child items (modules, structs, etc.)
    let has_children = stdout.contains("Modules:")
        || stdout.contains("Structs:")
        || stdout.contains("mod ")
        || stdout.contains("struct ");

    assert!(has_children, "crate root should list children: {stdout}");
}

// ═══════════════════════════════════════════════════════════════════════
// Cross-cutting: multiple output modes on real crates
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain"]
fn json_mode_on_real_struct_is_valid() {
    let output = grox()
        .arg("--json")
        .arg("semver::Version")
        .output()
        .expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("should be valid JSON");

    assert_eq!(parsed["kind"], "struct");
    assert!(
        parsed["path"].as_str().unwrap_or("").contains("Version"),
        "path should contain Version"
    );
    assert!(parsed["methods"].is_array(), "should have methods array");
    assert!(
        parsed["trait_impls"].is_array(),
        "should have trait_impls array"
    );
    // Methods should include `new` and `parse`
    let methods = parsed["methods"].as_array().expect("methods is array");
    let method_names: Vec<&str> = methods.iter().filter_map(|m| m["name"].as_str()).collect();
    assert!(
        method_names.contains(&"new"),
        "should have 'new' method: {method_names:?}"
    );
}

#[test]
#[ignore = "requires nightly toolchain"]
fn search_json_on_real_crate_produces_valid_json_lines() {
    let output = grox()
        .arg("--json")
        .arg("--search")
        .arg("Version")
        .arg("semver")
        .output()
        .expect("command runs");
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("each line should be valid JSON");
        assert!(
            parsed.get("score").is_some(),
            "should have score field: {line}"
        );
        assert!(
            parsed.get("path").is_some(),
            "should have path field: {line}"
        );
    }
}
