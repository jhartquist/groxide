//! Stress tests for groxide.
//!
//! Tests the `grox` binary against the top-20 most popular Rust crates
//! to verify robustness, crash-freedom, and correctness at scale.
//!
//! These tests require `cargo +nightly` and network access (auto-fetch
//! from crates.io). They are marked `#[ignore]` and should be run with:
//!
//!     cargo test --test stress -- --ignored --test-threads=1
//!
//! The `--test-threads=1` is recommended to avoid overwhelming crates.io
//! and nightly rustdoc with concurrent builds.

use std::process::Output;
use std::time::{Duration, Instant};

use assert_cmd::Command;

// ═══════════════════════════════════════════════════════════════════════
// Test infrastructure
// ═══════════════════════════════════════════════════════════════════════

/// Builds a `grox` command running from a temp directory (no project context),
/// forcing auto-fetch from crates.io.
fn grox() -> Command {
    let tmp = std::env::temp_dir();
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("grox"));
    cmd.current_dir(tmp);
    // 5 minute timeout per command — some crate builds are slow
    cmd.timeout(Duration::from_secs(300));
    cmd
}

/// Result of a single stress test probe.
#[derive(Debug)]
struct ProbeResult {
    crate_name: &'static str,
    query: String,
    mode: &'static str,
    passed: bool,
    exit_code: Option<i32>,
    crashed: bool,
    duration: Duration,
    error_detail: String,
}

impl ProbeResult {
    fn success(
        crate_name: &'static str,
        query: String,
        mode: &'static str,
        duration: Duration,
    ) -> Self {
        Self {
            crate_name,
            query,
            mode,
            passed: true,
            exit_code: Some(0),
            crashed: false,
            duration,
            error_detail: String::new(),
        }
    }

    fn failure(
        crate_name: &'static str,
        query: String,
        mode: &'static str,
        exit_code: Option<i32>,
        duration: Duration,
        detail: String,
    ) -> Self {
        // A crash is when the process was killed by signal (no exit code)
        // or when stderr contains panic/overflow indicators
        let crashed = exit_code.is_none()
            || detail.contains("panicked")
            || detail.contains("stack overflow")
            || detail.contains("SIGSEGV")
            || detail.contains("SIGABRT");

        Self {
            crate_name,
            query,
            mode,
            passed: false,
            exit_code,
            crashed,
            duration,
            error_detail: detail,
        }
    }
}

/// Runs a grox command and returns a `ProbeResult`.
fn probe(
    crate_name: &'static str,
    args: &[&str],
    mode: &'static str,
    validate: impl FnOnce(&Output) -> Result<(), String>,
) -> ProbeResult {
    let query = args.join(" ");
    let start = Instant::now();

    let output = match grox().args(args).output() {
        Ok(output) => output,
        Err(e) => {
            return ProbeResult::failure(
                crate_name,
                query,
                mode,
                None,
                start.elapsed(),
                format!("command failed to execute: {e}"),
            );
        }
    };

    let duration = start.elapsed();
    let exit_code = output.status.code();

    // Check for crashes first (signal-killed processes have no exit code)
    if exit_code.is_none() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return ProbeResult::failure(
            crate_name,
            query,
            mode,
            None,
            duration,
            format!("process killed by signal: {stderr}"),
        );
    }

    // Check stderr for panic indicators even on "success"
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("panicked") || stderr.contains("stack overflow") {
        return ProbeResult::failure(
            crate_name,
            query,
            mode,
            exit_code,
            duration,
            format!("panic detected in stderr: {stderr}"),
        );
    }

    match validate(&output) {
        Ok(()) => ProbeResult::success(crate_name, query, mode, duration),
        Err(detail) => ProbeResult::failure(crate_name, query, mode, exit_code, duration, detail),
    }
}

/// Validates that the command succeeded (exit 0) and stdout is non-empty.
fn expect_success(output: &Output) -> Result<(), String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "expected exit 0, got {:?}: {stderr}",
            output.status.code()
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() {
        return Err("stdout is empty".to_string());
    }
    // Verify stdout is valid UTF-8 (it should be, since from_utf8_lossy succeeded,
    // but let's explicitly verify no replacement chars)
    if String::from_utf8(output.stdout.clone()).is_err() {
        return Err("stdout contains invalid UTF-8".to_string());
    }
    Ok(())
}

/// Validates that the command produced valid JSON on stdout.
fn expect_json(output: &Output) -> Result<(), String> {
    expect_success(output)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("");
    serde_json::from_str::<serde_json::Value>(first_line)
        .map_err(|e| format!("invalid JSON: {e}: {first_line}"))?;
    Ok(())
}

/// Validates that recursive mode produced tabular output.
fn expect_list(output: &Output) -> Result<(), String> {
    expect_success(output)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let line_count = stdout.lines().count();
    if line_count < 2 {
        return Err(format!(
            "expected multiple list lines, got {line_count}: {stdout}"
        ));
    }
    Ok(())
}

/// Validates that --search mode produced results.
fn expect_search(output: &Output) -> Result<(), String> {
    expect_success(output)?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.contains("results for") {
        return Err(format!("missing 'results for' header: {stdout}"));
    }
    Ok(())
}

/// Validates that the output did not crash (exit code 0 or 1, not a signal).
fn expect_no_crash(output: &Output) -> Result<(), String> {
    match output.status.code() {
        Some(code) if code <= 2 => Ok(()),
        Some(code) => Err(format!("unexpected exit code: {code}")),
        None => Err("process killed by signal".to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Top-20 crate definitions
// ═══════════════════════════════════════════════════════════════════════

/// A crate to stress-test, with representative queries.
struct CrateSpec {
    name: &'static str,
    /// An item path to look up (struct, trait, fn, etc.)
    item_query: &'static str,
    /// A search term to use
    search_term: &'static str,
}

/// The top-20 most-downloaded crates on crates.io (as of 2025),
/// covering diverse crate sizes, architectures, and doc styles.
const TOP_CRATES: &[CrateSpec] = &[
    CrateSpec {
        name: "serde",
        item_query: "serde::Serialize",
        search_term: "Deserialize",
    },
    CrateSpec {
        name: "serde_json",
        item_query: "serde_json::Value",
        search_term: "from_str",
    },
    CrateSpec {
        name: "rand",
        item_query: "rand::Rng",
        search_term: "random",
    },
    CrateSpec {
        name: "log",
        item_query: "log::Log",
        search_term: "info",
    },
    CrateSpec {
        name: "syn",
        item_query: "syn::DeriveInput",
        search_term: "parse",
    },
    CrateSpec {
        name: "quote",
        item_query: "quote::ToTokens",
        search_term: "quote",
    },
    CrateSpec {
        name: "proc-macro2",
        item_query: "proc_macro2::TokenStream",
        search_term: "Span",
    },
    CrateSpec {
        name: "regex",
        item_query: "regex::Regex",
        search_term: "captures",
    },
    CrateSpec {
        name: "once_cell",
        item_query: "once_cell::sync::Lazy",
        search_term: "Lazy",
    },
    CrateSpec {
        name: "anyhow",
        item_query: "anyhow::Error",
        search_term: "context",
    },
    CrateSpec {
        name: "thiserror",
        item_query: "thiserror",
        search_term: "error",
    },
    CrateSpec {
        name: "clap",
        item_query: "clap",
        search_term: "Parser",
    },
    CrateSpec {
        name: "bytes",
        item_query: "bytes::Bytes",
        search_term: "Buf",
    },
    CrateSpec {
        name: "futures",
        item_query: "futures::Future",
        search_term: "Stream",
    },
    CrateSpec {
        name: "itertools",
        item_query: "itertools::Itertools",
        search_term: "chunk",
    },
    CrateSpec {
        name: "chrono",
        item_query: "chrono::DateTime",
        search_term: "NaiveDate",
    },
    CrateSpec {
        name: "url",
        item_query: "url::Url",
        search_term: "parse",
    },
    CrateSpec {
        name: "semver",
        item_query: "semver::Version",
        search_term: "parse",
    },
    CrateSpec {
        name: "itoa",
        item_query: "itoa::Buffer",
        search_term: "Buffer",
    },
    CrateSpec {
        name: "memchr",
        item_query: "memchr::memchr",
        search_term: "memchr",
    },
];

// ═══════════════════════════════════════════════════════════════════════
// Main stress test
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain, network, and significant time"]
fn stress_top20_crates_pass_rate() {
    let mut results: Vec<ProbeResult> = Vec::new();

    for spec in TOP_CRATES {
        eprintln!("\n{}", "=".repeat(60));
        eprintln!("Testing crate: {}", spec.name);
        eprintln!("{}", "=".repeat(60));

        // 1. Crate root query
        eprintln!("  [root] {}", spec.name);
        results.push(probe(spec.name, &[spec.name], "root", expect_success));

        // 2. Item query
        eprintln!("  [item] {}", spec.item_query);
        results.push(probe(
            spec.name,
            &[spec.item_query],
            "item",
            expect_no_crash,
        ));

        // 3. List mode
        eprintln!("  [list] {} -r", spec.name);
        results.push(probe(spec.name, &["-r", spec.name], "list", expect_list));

        // 4. Search mode
        eprintln!("  [search] --search {} {}", spec.search_term, spec.name);
        results.push(probe(
            spec.name,
            &["--search", spec.search_term, spec.name],
            "search",
            expect_search,
        ));

        // 5. JSON mode
        eprintln!("  [json] --json {}", spec.name);
        results.push(probe(
            spec.name,
            &["--json", spec.name],
            "json",
            expect_json,
        ));
    }

    // ── Report ──
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let crashed = results.iter().filter(|r| r.crashed).count();
    let failed: Vec<&ProbeResult> = results.iter().filter(|r| !r.passed).collect();
    #[allow(clippy::cast_precision_loss)]
    let pass_rate = (passed as f64 / total as f64) * 100.0;

    eprintln!("\n");
    eprintln!("{}", "=".repeat(60));
    eprintln!("STRESS TEST RESULTS");
    eprintln!("{}", "=".repeat(60));
    eprintln!("Total probes: {total}");
    eprintln!("Passed:       {passed}");
    eprintln!("Failed:       {}", failed.len());
    eprintln!("Crashed:      {crashed}");
    eprintln!("Pass rate:    {pass_rate:.1}%");
    eprintln!();

    if !failed.is_empty() {
        eprintln!("FAILURES:");
        for f in &failed {
            eprintln!(
                "  [{:>6}] {} {} — exit {:?} — {}",
                f.mode,
                f.crate_name,
                f.query,
                f.exit_code,
                if f.error_detail.len() > 200 {
                    format!("{}...", &f.error_detail[..200])
                } else {
                    f.error_detail.clone()
                }
            );
        }
        eprintln!();
    }

    // ── Assertions ──
    assert_eq!(
        crashed, 0,
        "ZERO crashes required. {crashed} probe(s) crashed."
    );
    assert!(
        pass_rate >= 96.0,
        "Pass rate {pass_rate:.1}% is below the 96% threshold. {passed}/{total} passed.",
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Typenum recursion limit fix
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain and network; typenum build is slow"]
fn stress_typenum_no_recursion_crash() {
    let result = probe("typenum", &["typenum"], "root", expect_success);

    eprintln!(
        "typenum root: passed={}, duration={:?}",
        result.passed, result.duration
    );
    assert!(
        !result.crashed,
        "typenum must not crash: {:?}",
        result.error_detail
    );
    assert!(
        result.passed,
        "typenum root should succeed: {}",
        result.error_detail
    );
}

#[test]
#[ignore = "requires nightly toolchain and network; typenum build is slow"]
fn stress_typenum_search_no_crash() {
    let result = probe(
        "typenum",
        &["--search", "Integer", "typenum"],
        "search",
        expect_no_crash,
    );

    eprintln!(
        "typenum search: passed={}, duration={:?}",
        result.passed, result.duration
    );
    assert!(
        !result.crashed,
        "typenum search must not crash: {:?}",
        result.error_detail
    );
}

#[test]
#[ignore = "requires nightly toolchain and network; typenum build is slow"]
fn stress_typenum_list_no_crash() {
    let result = probe("typenum", &["-r", "typenum"], "list", expect_no_crash);

    eprintln!(
        "typenum list: passed={}, duration={:?}",
        result.passed, result.duration
    );
    assert!(
        !result.crashed,
        "typenum list must not crash: {:?}",
        result.error_detail
    );
}

#[test]
#[ignore = "requires nightly toolchain and network; typenum build is slow"]
fn stress_typenum_json_no_crash() {
    let result = probe("typenum", &["--json", "typenum"], "json", expect_no_crash);

    eprintln!(
        "typenum json: passed={}, duration={:?}",
        result.passed, result.duration
    );
    assert!(
        !result.crashed,
        "typenum json must not crash: {:?}",
        result.error_detail
    );
}

// ═══════════════════════════════════════════════════════════════════════
// UTF-8 truncation safety
// ═══════════════════════════════════════════════════════════════════════

/// Tests that crates with Unicode in documentation don't crash during
/// truncation. We use `regex` which has extensive Unicode docs.
#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_utf8_truncation_no_crash_regex() {
    // regex has extensive Unicode property discussion in its docs
    let result = probe("regex", &["regex"], "root", expect_success);
    assert!(
        !result.crashed,
        "regex root must not crash (UTF-8): {}",
        result.error_detail
    );
    assert!(
        result.passed,
        "regex root should succeed: {}",
        result.error_detail
    );
}

#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_utf8_truncation_no_crash_chrono() {
    // chrono has international date/time content with various Unicode
    let result = probe("chrono", &["chrono"], "root", expect_success);
    assert!(
        !result.crashed,
        "chrono root must not crash (UTF-8): {}",
        result.error_detail
    );
    assert!(
        result.passed,
        "chrono root should succeed: {}",
        result.error_detail
    );
}

#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_utf8_truncation_no_crash_url() {
    // url handles international domain names with Unicode
    let result = probe("url", &["url"], "root", expect_success);
    assert!(
        !result.crashed,
        "url root must not crash (UTF-8): {}",
        result.error_detail
    );
    assert!(
        result.passed,
        "url root should succeed: {}",
        result.error_detail
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Large crate robustness (crates with many items)
// ═══════════════════════════════════════════════════════════════════════

/// syn has hundreds of types — verifies we handle large indexes.
#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_large_crate_syn() {
    // Root query
    let root = probe("syn", &["syn"], "root", expect_success);
    assert!(
        !root.crashed,
        "syn root must not crash: {}",
        root.error_detail
    );

    // List mode on a large crate
    let list = probe("syn", &["-r", "syn"], "list", |output| {
        expect_success(output)?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line_count = stdout.lines().count();
        if line_count < 20 {
            return Err(format!(
                "syn should have many items in list, got {line_count}"
            ));
        }
        Ok(())
    });
    assert!(
        !list.crashed,
        "syn list must not crash: {}",
        list.error_detail
    );

    eprintln!(
        "syn: root={}, list={}, durations={:?}/{:?}",
        root.passed, list.passed, root.duration, list.duration
    );
}

/// futures has many re-exports — verifies re-export handling at scale.
#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_large_crate_futures() {
    let root = probe("futures", &["futures"], "root", expect_success);
    assert!(
        !root.crashed,
        "futures root must not crash: {}",
        root.error_detail
    );

    let list = probe("futures", &["-r", "futures"], "list", expect_list);
    assert!(
        !list.crashed,
        "futures list must not crash: {}",
        list.error_detail
    );

    eprintln!("futures: root={}, list={}", root.passed, list.passed);
}

/// itertools has many trait extension methods — verifies method collection.
#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_large_crate_itertools() {
    let root = probe("itertools", &["itertools"], "root", expect_success);
    assert!(
        !root.crashed,
        "itertools root must not crash: {}",
        root.error_detail
    );

    let item = probe(
        "itertools",
        &["itertools::Itertools"],
        "item",
        expect_no_crash,
    );
    assert!(
        !item.crashed,
        "itertools::Itertools must not crash: {}",
        item.error_detail
    );

    eprintln!("itertools: root={}, item={}", root.passed, item.passed);
}

// ═══════════════════════════════════════════════════════════════════════
// Output quality spot checks
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_output_quality_serde_json_value() {
    let result = probe("serde_json", &["serde_json::Value"], "item", |output| {
        expect_success(output)?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        // Should mention it's an enum
        if !stdout.contains("enum") {
            return Err(format!("serde_json::Value should show as enum: {stdout}"));
        }

        // Should show variants
        let has_variants = stdout.contains("Null")
            || stdout.contains("Bool")
            || stdout.contains("Number")
            || stdout.contains("String")
            || stdout.contains("Array")
            || stdout.contains("Object");
        if !has_variants {
            return Err(format!("serde_json::Value should show variants: {stdout}"));
        }

        Ok(())
    });
    assert!(
        result.passed,
        "quality check failed: {}",
        result.error_detail
    );
}

#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_output_quality_regex() {
    let result = probe("regex", &["regex::Regex"], "item", |output| {
        expect_success(output)?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        if !stdout.contains("struct") && !stdout.contains("Regex") {
            return Err(format!("regex::Regex should show struct: {stdout}"));
        }

        // Should show key methods
        let has_methods = stdout.contains("is_match")
            || stdout.contains("find")
            || stdout.contains("captures")
            || stdout.contains("new");
        if !has_methods {
            return Err(format!("regex::Regex should show methods: {stdout}"));
        }

        Ok(())
    });
    assert!(
        result.passed,
        "quality check failed: {}",
        result.error_detail
    );
}

#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_output_quality_bytes() {
    let result = probe("bytes", &["bytes::Bytes"], "item", |output| {
        expect_success(output)?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        if !stdout.contains("struct") && !stdout.contains("Bytes") {
            return Err(format!("bytes::Bytes should show struct: {stdout}"));
        }

        // Should have methods
        if !stdout.contains("methods:") && !stdout.contains("pub fn ") {
            return Err(format!("bytes::Bytes should show methods: {stdout}"));
        }

        Ok(())
    });
    assert!(
        result.passed,
        "quality check failed: {}",
        result.error_detail
    );
}

// ═══════════════════════════════════════════════════════════════════════
// Token efficiency check
// ═══════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires nightly toolchain and network"]
fn stress_token_efficiency_default_output() {
    // Default output should be in the ~200-800 token range (roughly 800-3200 chars)
    // We'll check a few crates to make sure we're not dumping everything
    for crate_name in &["serde", "regex", "clap", "bytes", "url"] {
        let result = probe(crate_name, &[crate_name], "root", |output| {
            expect_success(output)?;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let char_count = stdout.len();

            // Should produce meaningful output (> 100 chars)
            if char_count < 100 {
                return Err(format!(
                    "{crate_name}: output too short ({char_count} chars)"
                ));
            }

            // Should not dump entire crate (< 15000 chars)
            if char_count > 15000 {
                return Err(format!(
                    "{crate_name}: output too long ({char_count} chars), truncation may be broken"
                ));
            }

            Ok(())
        });

        assert!(
            !result.crashed,
            "{crate_name} crashed during token efficiency check: {}",
            result.error_detail
        );
    }
}
