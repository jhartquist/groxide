use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::FeatureFlags;
use crate::error::{GroxError, Result};
use crate::types::is_stdlib_crate;

/// Detects the nightly sysroot path.
///
/// Runs `rustc +nightly --print sysroot` and validates the returned path exists.
///
/// # Errors
///
/// Returns `GroxError::NightlyNotAvailable` if nightly is not installed or the
/// sysroot path does not exist on disk.
pub(crate) fn get_sysroot() -> Result<PathBuf> {
    let output = Command::new("rustc")
        .args(["+nightly", "--print", "sysroot"])
        .output()
        .map_err(|_| GroxError::NightlyNotAvailable)?;

    if !output.status.success() {
        return Err(GroxError::NightlyNotAvailable);
    }

    let sysroot = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let path = PathBuf::from(&sysroot);
    if path.exists() {
        Ok(path)
    } else {
        Err(GroxError::NightlyNotAvailable)
    }
}

/// Returns the path to the stdlib source library directory.
///
/// The stdlib source lives at `<sysroot>/lib/rustlib/src/rust/library/`.
///
/// # Errors
///
/// Returns `GroxError::StdLibSourceMissing` if the rust-src component is not installed.
pub(crate) fn stdlib_library_path(sysroot: &Path) -> Result<PathBuf> {
    let library_path = sysroot.join("lib/rustlib/src/rust/library");
    if library_path.exists() {
        Ok(library_path)
    } else {
        Err(GroxError::StdLibSourceMissing)
    }
}

/// Extracts a toolchain hash from `rustc +nightly --version --verbose`.
///
/// Parses the verbose output for the `commit-hash:` line. If the commit-hash line
/// is missing, falls back to a DJB2 hash of the first line.
///
/// # Errors
///
/// Returns `GroxError::NightlyNotAvailable` if nightly is not installed.
pub(crate) fn get_toolchain_hash() -> Result<String> {
    let output = Command::new("rustc")
        .args(["+nightly", "--version", "--verbose"])
        .output()
        .map_err(|_| GroxError::NightlyNotAvailable)?;

    if !output.status.success() {
        return Err(GroxError::NightlyNotAvailable);
    }

    let verbose = String::from_utf8_lossy(&output.stdout);
    Ok(parse_toolchain_hash(&verbose))
}

/// Parses a toolchain hash from verbose rustc output.
///
/// Looks for a `commit-hash:` line. Falls back to DJB2 hash of the first line.
fn parse_toolchain_hash(verbose_output: &str) -> String {
    for line in verbose_output.lines() {
        if let Some(hash) = line.strip_prefix("commit-hash: ") {
            let hash = hash.trim();
            if !hash.is_empty() {
                return hash.to_string();
            }
        }
    }

    // Fallback: DJB2 of the first line
    let first_line = verbose_output.lines().next().unwrap_or("unknown");
    format!("{:016x}", djb2_hash(first_line))
}

/// DJB2 hash function.
fn djb2_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(byte));
    }
    hash
}

/// Returns the global cache directory for stdlib crates.
fn stdlib_cache_dir() -> Result<PathBuf> {
    dirs::cache_dir()
        .map(|d| d.join("groxide").join("stdlib"))
        .ok_or_else(|| GroxError::Io(std::io::Error::other("could not determine cache directory")))
}

/// Computes the target directory for stdlib rustdoc generation.
///
/// Path: `~/.cache/groxide/stdlib/target-<crate_name>-<toolchain_hash>`
fn stdlib_target_dir(crate_name: &str, toolchain_hash: &str) -> Result<PathBuf> {
    let cache_dir = stdlib_cache_dir()?;
    Ok(cache_dir.join(format!("target-{crate_name}-{toolchain_hash}")))
}

/// Generates rustdoc JSON for a stdlib crate and returns the path to the JSON file.
///
/// Locates the stdlib source via sysroot, builds with `--manifest-path` and
/// `--target-dir` pointing to the global cache.
///
/// Unlike dependency crates, stdlib crates are always built with default features
/// (unless the user explicitly specifies feature flags). Stdlib features like
/// `panic_immediate_abort`, `optimize_for_size`, and `compiler-builtins-c` are
/// internal build-system knobs that can break across nightly versions — they don't
/// gate public API items the way user-crate features do.
///
/// # Errors
///
/// Returns `GroxError::NightlyNotAvailable` if nightly is missing.
/// Returns `GroxError::StdLibSourceMissing` if rust-src is not installed.
/// Returns `GroxError::RustdocFailed` if doc generation fails.
pub(crate) fn generate_stdlib_json(
    crate_name: &str,
    features: &FeatureFlags,
    private: bool,
) -> Result<String> {
    // 1. Validate crate name
    if !is_stdlib_crate(crate_name) {
        return Err(GroxError::RustdocFailed {
            stderr: format!("'{crate_name}' is not a recognized stdlib crate"),
        });
    }

    // 2. Locate source
    let sysroot = get_sysroot()?;
    let library_path = stdlib_library_path(&sysroot)?;
    let manifest_path = library_path.join(crate_name).join("Cargo.toml");
    if !manifest_path.exists() {
        return Err(GroxError::StdLibSourceMissing);
    }

    // 3. Determine target dir
    let toolchain_hash = get_toolchain_hash()?;
    let target_dir = stdlib_target_dir(crate_name, &toolchain_hash)?;
    std::fs::create_dir_all(
        target_dir
            .parent()
            .expect("invariant: target_dir has parent"),
    )
    .map_err(GroxError::Io)?;

    // 4. Build rustdoc JSON
    //
    // For stdlib crates we intentionally skip `--all-features`. Stdlib features
    // are internal build-system knobs (e.g. `panic_immediate_abort`,
    // `compiler-builtins-c`) that break across nightly versions and don't gate
    // public API items. We use default features unless the user explicitly
    // specified feature flags.
    let effective_features = if features.is_default() {
        // Strip --all-features; use defaults for stdlib crates
        FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        }
    } else {
        // User specified explicit flags — honour them, but never inject --all-features
        FeatureFlags {
            all_features: false,
            no_default_features: features.no_default_features,
            features: features.features.clone(),
        }
    };

    // 5. Run rustdoc and read JSON under an exclusive lock on the per-toolchain
    // target dir. Concurrent grox invocations querying the same stdlib crate
    // would otherwise race on `target/doc/<crate>.json` between cargo's
    // unlink and rewrite — same race as the workspace path.
    let normalized = crate_name.replace('-', "_");
    let json_path = target_dir.join("doc").join(format!("{normalized}.json"));
    crate::docgen::run_cargo_and_read_json(&target_dir, &json_path, || {
        let cmd = build_stdlib_rustdoc_command(
            &manifest_path,
            &target_dir,
            &effective_features,
            private,
        );
        run_rustdoc_command(cmd)
    })
}

/// Builds the `cargo +nightly rustdoc` command for stdlib crates.
///
/// Uses `--manifest-path` and `--target-dir` instead of `-p`.
fn build_stdlib_rustdoc_command(
    manifest_path: &Path,
    target_dir: &Path,
    features: &FeatureFlags,
    private: bool,
) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("+nightly").arg("rustdoc");
    cmd.arg("--lib");
    cmd.arg("--manifest-path").arg(manifest_path);
    cmd.arg("--target-dir").arg(target_dir);

    if features.all_features {
        cmd.arg("--all-features");
    }
    if features.no_default_features {
        cmd.arg("--no-default-features");
    }
    if !features.features.is_empty() {
        cmd.arg("--features").arg(features.features.join(","));
    }

    cmd.arg("--output-format").arg("json");
    cmd.arg("-Z").arg("unstable-options");

    if private {
        cmd.arg("--").arg("--document-private-items");
    }

    cmd
}

/// Runs a rustdoc command and returns `Ok(())` on success or `Err(GroxError)`.
fn run_rustdoc_command(mut cmd: Command) -> Result<()> {
    let output = cmd.output().map_err(|e| GroxError::RustdocFailed {
        stderr: format!("failed to execute cargo rustdoc: {e}"),
    })?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(GroxError::RustdocFailed { stderr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Sysroot detection ----

    #[test]
    fn get_sysroot_returns_existing_path_when_nightly_installed() {
        let result = get_sysroot();
        if result.is_err() {
            eprintln!("SKIP: nightly toolchain not installed");
            return;
        }
        let sysroot = result.expect("already checked");
        assert!(sysroot.exists(), "sysroot path should exist: {sysroot:?}");
        assert!(
            sysroot.is_dir(),
            "sysroot should be a directory: {sysroot:?}"
        );
    }

    #[test]
    fn get_sysroot_path_contains_nightly() {
        let result = get_sysroot();
        if result.is_err() {
            eprintln!("SKIP: nightly toolchain not installed");
            return;
        }
        let sysroot = result.expect("already checked");
        let sysroot_str = sysroot.to_str().expect("valid utf8");
        assert!(
            sysroot_str.contains("nightly"),
            "sysroot should contain 'nightly': {sysroot_str}"
        );
    }

    // ---- stdlib_library_path ----

    #[test]
    fn stdlib_library_path_returns_path_when_rust_src_installed() {
        let Ok(sysroot) = get_sysroot() else {
            eprintln!("SKIP: nightly toolchain not installed");
            return;
        };
        let result = stdlib_library_path(&sysroot);
        match result {
            Ok(path) => {
                assert!(path.exists(), "library path should exist: {path:?}");
                // Check that it contains std, core, alloc subdirs
                assert!(path.join("std").exists(), "should contain std directory");
                assert!(path.join("core").exists(), "should contain core directory");
                assert!(
                    path.join("alloc").exists(),
                    "should contain alloc directory"
                );
            }
            Err(_) => {
                eprintln!("SKIP: rust-src component not installed");
            }
        }
    }

    #[test]
    fn stdlib_library_path_returns_error_for_nonexistent_sysroot() {
        let result = stdlib_library_path(Path::new("/nonexistent/sysroot"));
        assert!(result.is_err());
        assert!(
            matches!(result.unwrap_err(), GroxError::StdLibSourceMissing),
            "should return StdLibSourceMissing"
        );
    }

    // ---- Toolchain hash extraction ----

    #[test]
    fn get_toolchain_hash_returns_nonempty_string() {
        let result = get_toolchain_hash();
        if result.is_err() {
            eprintln!("SKIP: nightly toolchain not installed");
            return;
        }
        let hash = result.expect("already checked");
        assert!(!hash.is_empty(), "toolchain hash should not be empty");
    }

    #[test]
    fn get_toolchain_hash_returns_hex_commit_hash() {
        let result = get_toolchain_hash();
        if result.is_err() {
            eprintln!("SKIP: nightly toolchain not installed");
            return;
        }
        let hash = result.expect("already checked");
        // The commit hash should be a 40-character hex string
        // (or a 16-char hex if using DJB2 fallback)
        assert!(
            hash.len() == 40 || hash.len() == 16,
            "hash should be 40 (commit) or 16 (djb2) chars: got {} chars = '{hash}'",
            hash.len()
        );
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "hash should be hex: '{hash}'"
        );
    }

    #[test]
    fn parse_toolchain_hash_extracts_commit_hash() {
        let verbose = "rustc 1.83.0-nightly (90b35a623 2024-11-26)\n\
                        binary: rustc\n\
                        commit-hash: 90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf\n\
                        commit-date: 2024-11-26\n\
                        host: aarch64-apple-darwin\n\
                        release: 1.83.0-nightly\n";
        let hash = parse_toolchain_hash(verbose);
        assert_eq!(hash, "90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf");
    }

    #[test]
    fn parse_toolchain_hash_falls_back_to_djb2_when_no_commit_hash() {
        let verbose = "rustc 1.83.0-nightly\nbinary: rustc\nhost: aarch64-apple-darwin\n";
        let hash = parse_toolchain_hash(verbose);
        assert_eq!(hash.len(), 16, "DJB2 hash should be 16 hex chars");
        assert!(
            hash.chars().all(|c| c.is_ascii_hexdigit()),
            "should be hex: '{hash}'"
        );
    }

    #[test]
    fn parse_toolchain_hash_falls_back_for_empty_commit_hash() {
        let verbose = "rustc 1.83.0-nightly\ncommit-hash: \nhost: aarch64-apple-darwin\n";
        let hash = parse_toolchain_hash(verbose);
        assert_eq!(
            hash.len(),
            16,
            "should fall back to DJB2 for empty commit-hash"
        );
    }

    #[test]
    fn parse_toolchain_hash_deterministic() {
        let verbose = "rustc 1.83.0-nightly (90b35a623 2024-11-26)\n\
                        commit-hash: 90b35a6239c3d8bdabc530a6a0816f7ff89a0aaf\n";
        let hash1 = parse_toolchain_hash(verbose);
        let hash2 = parse_toolchain_hash(verbose);
        assert_eq!(hash1, hash2, "should be deterministic");
    }

    // ---- DJB2 hash ----

    #[test]
    fn djb2_hash_produces_consistent_results() {
        assert_eq!(djb2_hash("test"), djb2_hash("test"));
    }

    #[test]
    fn djb2_hash_differs_for_different_inputs() {
        assert_ne!(djb2_hash("test"), djb2_hash("other"));
    }

    #[test]
    fn djb2_hash_works_on_empty_string() {
        // Should return the initial value (5381)
        assert_eq!(djb2_hash(""), 5381);
    }

    // ---- build_stdlib_rustdoc_command ----

    #[test]
    fn build_stdlib_command_includes_manifest_path_and_target_dir() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_stdlib_rustdoc_command(
            Path::new("/sysroot/lib/rustlib/src/rust/library/std/Cargo.toml"),
            Path::new("/cache/stdlib/target-std-abc123"),
            &features,
            false,
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "+nightly"));
        assert!(has_arg(&args, "rustdoc"));
        assert!(has_arg(&args, "--lib"));
        assert!(has_arg(&args, "--manifest-path"));
        assert!(has_arg(
            &args,
            "/sysroot/lib/rustlib/src/rust/library/std/Cargo.toml"
        ));
        assert!(has_arg(&args, "--target-dir"));
        assert!(has_arg(&args, "/cache/stdlib/target-std-abc123"));
        assert!(has_arg(&args, "--output-format"));
        assert!(has_arg(&args, "json"));
        assert!(!has_arg(&args, "-p"));
    }

    #[test]
    fn build_stdlib_command_includes_all_features() {
        let features = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_stdlib_rustdoc_command(
            Path::new("/std/Cargo.toml"),
            Path::new("/cache/target"),
            &features,
            false,
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--all-features"));
    }

    #[test]
    fn build_stdlib_command_includes_private_items() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_stdlib_rustdoc_command(
            Path::new("/std/Cargo.toml"),
            Path::new("/cache/target"),
            &features,
            true,
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--"));
        assert!(has_arg(&args, "--document-private-items"));
    }

    #[test]
    fn build_stdlib_command_omits_private_when_not_requested() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_stdlib_rustdoc_command(
            Path::new("/std/Cargo.toml"),
            Path::new("/cache/target"),
            &features,
            false,
        );
        let args = format_command_args(&cmd);
        assert!(!has_arg(&args, "--document-private-items"));
    }

    // ---- generate_stdlib_json rejects non-stdlib ----

    #[test]
    fn generate_stdlib_json_rejects_non_stdlib_crate() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let result = generate_stdlib_json("serde", &features, false);
        assert!(result.is_err());
        match result.unwrap_err() {
            GroxError::RustdocFailed { stderr } => {
                assert!(
                    stderr.contains("not a recognized stdlib crate"),
                    "error should mention unrecognized: {stderr}"
                );
            }
            other => panic!("expected RustdocFailed, got: {other:?}"),
        }
    }

    // ---- Integration test (requires nightly + rust-src) ----

    #[test]
    #[ignore = "requires nightly toolchain and rust-src component; slow"]
    fn generate_stdlib_json_produces_json_for_core() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let result = generate_stdlib_json("core", &features, false);
        match result {
            Ok(json) => {
                assert!(!json.is_empty(), "JSON content should be non-empty");
                // rustdoc JSON starts with `{` and contains the crate name.
                assert!(json.trim_start().starts_with('{'));
                assert!(
                    json.contains("\"crate_name\""),
                    "JSON should contain crate_name field"
                );
            }
            Err(GroxError::NightlyNotAvailable) => {
                eprintln!("SKIP: nightly not available");
            }
            Err(GroxError::StdLibSourceMissing) => {
                eprintln!("SKIP: rust-src not installed");
            }
            Err(e) => panic!("unexpected error: {e}"),
        }
    }

    // ---- Test helpers ----

    /// Extracts command arguments from the Command's debug output.
    fn format_command_args(cmd: &Command) -> Vec<String> {
        let debug = format!("{cmd:?}");
        let mut args = Vec::new();
        let mut in_quote = false;
        let mut current = String::new();
        for ch in debug.chars() {
            if ch == '"' {
                if in_quote {
                    args.push(current.clone());
                    current.clear();
                }
                in_quote = !in_quote;
            } else if in_quote {
                current.push(ch);
            }
        }
        args
    }

    fn has_arg(args: &[String], expected: &str) -> bool {
        args.iter().any(|a| a == expected)
    }
}
