use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::FeatureFlags;
use crate::error::{GroxError, Result};
use crate::resolve::CrateSource;

/// Patterns in stderr that indicate a platform-specific build failure.
///
/// When building with `--all-features` and the command fails, check stderr
/// for these patterns before retrying with default features.
const PLATFORM_FAILURE_PATTERNS: &[&str] = &[
    "failed to run custom build command",
    "could not find",
    "ld: library not found",
    "ld: framework not found",
    "ld: cannot find",
    "Unable to find",
    "not found in PATH",
    "LINK : fatal error",
    "error occurred: Command",
    "is not recognized as an internal or external command",
    "cannot specify features for packages outside of workspace",
];

/// Paired patterns: both strings must appear in stderr for the pattern to match.
const PLATFORM_FAILURE_PAIRED_PATTERNS: &[(&str, &str)] =
    &[("linker", "error"), ("could not find", "native")];

/// Generates rustdoc JSON for the given crate source.
///
/// Returns the path to the generated JSON file on success.
///
/// # Errors
///
/// Returns `GroxError::NightlyNotAvailable` if the nightly toolchain is missing.
/// Returns `GroxError::RustdocFailed` if `cargo rustdoc` fails.
pub(crate) fn generate_rustdoc_json(
    source: &CrateSource,
    features: &FeatureFlags,
    private: bool,
) -> Result<PathBuf> {
    check_nightly_available()?;

    match source {
        CrateSource::CurrentCrate {
            manifest_path,
            name,
            version,
            ..
        } => {
            let workspace_root = manifest_path
                .parent()
                .expect("invariant: manifest_path has a parent");
            let target_dir = workspace_root.join("target");
            eprint_status(name, version);
            generate_for_current_crate(workspace_root, name, &target_dir, features, private)?;
            let json_path = json_output_path(&target_dir, name);
            eprint_done();
            Ok(json_path)
        }
        CrateSource::Dependency {
            manifest_path,
            name,
            version,
            ..
        } => {
            let target_dir = find_workspace_target_dir(manifest_path)?;
            eprint_status(name, version);
            generate_for_dependency(&target_dir, name, features, private)?;
            let json_path = json_output_path(&target_dir, name);
            eprint_done();
            Ok(json_path)
        }
        CrateSource::Stdlib { name } => {
            // Stdlib generation is handled by a separate module (stdlib.rs)
            // For now, generate using --manifest-path pointing to stdlib source
            let json_path = generate_for_stdlib(name, features, private)?;
            Ok(json_path)
        }
        CrateSource::External { name, .. } => {
            // External crate generation is handled by external.rs after extraction.
            // This path assumes we're in the extracted source directory.
            // The caller should set up the working directory appropriately.
            Err(GroxError::RustdocFailed {
                stderr: format!(
                    "external crate '{name}' must be fetched and extracted before doc generation"
                ),
            })
        }
    }
}

/// Checks that the nightly toolchain is available.
fn check_nightly_available() -> Result<()> {
    let output = Command::new("rustup")
        .args(["run", "nightly", "rustc", "--version"])
        .output()
        .map_err(|_| GroxError::NightlyNotAvailable)?;

    if output.status.success() {
        Ok(())
    } else {
        Err(GroxError::NightlyNotAvailable)
    }
}

/// Generates rustdoc JSON for the current workspace crate.
///
/// Uses `--all-features` with platform failure fallback unless the user
/// specified explicit feature flags.
fn generate_for_current_crate(
    workspace_root: &Path,
    crate_name: &str,
    target_dir: &Path,
    features: &FeatureFlags,
    private: bool,
) -> Result<()> {
    if !features.is_default() {
        // User specified explicit flags — use them directly, no fallback
        let cmd = build_rustdoc_command(
            Some(workspace_root),
            Some(crate_name),
            None,
            Some(target_dir),
            features,
            private,
            false, // no --lib for workspace crate (uses -p)
        );
        return run_rustdoc_command(cmd);
    }

    // Default: try with --all-features, fallback on platform failure
    let all_features = FeatureFlags {
        all_features: true,
        no_default_features: false,
        features: Vec::new(),
    };
    let cmd = build_rustdoc_command(
        Some(workspace_root),
        Some(crate_name),
        None,
        Some(target_dir),
        &all_features,
        private,
        false,
    );

    match run_rustdoc_command_with_output(cmd) {
        Ok(()) => Ok(()),
        Err(stderr) => {
            if is_platform_failure(&stderr) {
                eprintln!(
                    "[grox] Build with --all-features failed, retrying with default features..."
                );
                let default_features = FeatureFlags {
                    all_features: false,
                    no_default_features: false,
                    features: Vec::new(),
                };
                let retry_cmd = build_rustdoc_command(
                    Some(workspace_root),
                    Some(crate_name),
                    None,
                    Some(target_dir),
                    &default_features,
                    private,
                    false,
                );
                run_rustdoc_command(retry_cmd)
            } else {
                Err(GroxError::RustdocFailed { stderr })
            }
        }
    }
}

/// Generates rustdoc JSON for a dependency crate.
///
/// Dependencies use no feature flags — let cargo resolver unify features from Cargo.toml.
fn generate_for_dependency(
    target_dir: &Path,
    crate_name: &str,
    features: &FeatureFlags,
    private: bool,
) -> Result<()> {
    let effective_features = if features.is_default() {
        // No features for deps — let cargo resolver handle it
        &FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        }
    } else {
        features
    };

    // For dependencies, we use -p <name> from the workspace root
    // The workspace root is the parent of the target_dir
    let workspace_root = target_dir
        .parent()
        .expect("invariant: target_dir has a parent");

    let cmd = build_rustdoc_command(
        Some(workspace_root),
        Some(crate_name),
        None,
        Some(target_dir),
        effective_features,
        private,
        false,
    );
    run_rustdoc_command(cmd)
}

/// Generates rustdoc JSON for a stdlib crate.
///
/// Delegates to the `stdlib` module for sysroot detection, toolchain hashing,
/// and per-toolchain cache isolation.
fn generate_for_stdlib(
    crate_name: &str,
    features: &FeatureFlags,
    private: bool,
) -> Result<PathBuf> {
    crate::stdlib::generate_stdlib_json(crate_name, features, private)
}

/// Generates rustdoc JSON for an external crate in its extracted source directory.
///
/// Called by the external crate fetching module after extraction.
pub(crate) fn generate_rustdoc_json_external(
    source_dir: &Path,
    crate_name: &str,
    features: &FeatureFlags,
    private: bool,
) -> Result<PathBuf> {
    check_nightly_available()?;

    let target_dir = source_dir.join("target");

    let cmd = build_rustdoc_command(
        Some(source_dir),
        None,
        None,
        Some(&target_dir),
        features,
        private,
        true, // --lib for non-workspace crates
    );
    run_rustdoc_command(cmd)?;

    Ok(json_output_path(&target_dir, crate_name))
}

/// Builds the `cargo +nightly rustdoc` command with appropriate flags.
///
/// # Arguments
///
/// * `working_dir` - Working directory for the command (None uses current dir).
/// * `package` - Package name for `-p <name>` (None omits it).
/// * `manifest_path` - Path for `--manifest-path` (None omits it).
/// * `target_dir` - Path for `--target-dir` (None omits it).
/// * `features` - Feature flags to forward.
/// * `private` - Whether to include `--document-private-items`.
/// * `use_lib_flag` - Whether to include `--lib`.
fn build_rustdoc_command(
    working_dir: Option<&Path>,
    package: Option<&str>,
    manifest_path: Option<&Path>,
    target_dir: Option<&Path>,
    features: &FeatureFlags,
    private: bool,
    use_lib_flag: bool,
) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("+nightly").arg("rustdoc");

    if use_lib_flag {
        cmd.arg("--lib");
    }

    if let Some(pkg) = package {
        cmd.arg("-p").arg(pkg);
    }

    if let Some(path) = manifest_path {
        cmd.arg("--manifest-path").arg(path);
    }

    if let Some(dir) = target_dir {
        cmd.arg("--target-dir").arg(dir);
    }

    // Feature flags
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

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
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

/// Runs a rustdoc command, returning `Ok(())` on success or the stderr string on failure.
///
/// This variant is used for the fallback retry logic where we need to inspect stderr
/// before deciding how to handle the failure.
fn run_rustdoc_command_with_output(mut cmd: Command) -> std::result::Result<(), String> {
    let output = cmd
        .output()
        .map_err(|e| format!("failed to execute cargo rustdoc: {e}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Checks whether stderr from a failed build matches platform-specific failure patterns.
fn is_platform_failure(stderr: &str) -> bool {
    let stderr_lower = stderr.to_lowercase();

    for pattern in PLATFORM_FAILURE_PATTERNS {
        if stderr_lower.contains(&pattern.to_lowercase()) {
            return true;
        }
    }

    for (a, b) in PLATFORM_FAILURE_PAIRED_PATTERNS {
        if stderr_lower.contains(&a.to_lowercase()) && stderr_lower.contains(&b.to_lowercase()) {
            return true;
        }
    }

    false
}

/// Computes the expected JSON output path for a crate.
///
/// Hyphens in the crate name are converted to underscores to match rustdoc behavior.
fn json_output_path(target_dir: &Path, crate_name: &str) -> PathBuf {
    let normalized = crate_name.replace('-', "_");
    target_dir.join("doc").join(format!("{normalized}.json"))
}

/// Finds the workspace target directory for a dependency.
///
/// Walks up from the dependency manifest path to find the workspace root's target dir.
fn find_workspace_target_dir(dep_manifest_path: &Path) -> Result<PathBuf> {
    // Run cargo metadata on the workspace root to get the target directory
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(dep_manifest_path)
        .no_deps()
        .exec()
        .map_err(|e| GroxError::CargoMetadataFailed {
            details: e.to_string(),
        })?;
    Ok(metadata.target_directory.into_std_path_buf())
}

/// Prints a status message to stderr.
fn eprint_status(name: &str, version: &str) {
    eprint!("[grox] Building index for {name} {version}...");
}

/// Prints the "done" status to stderr.
fn eprint_done() {
    eprintln!(" done");
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Nightly detection ----

    #[test]
    fn check_nightly_available_succeeds_when_nightly_installed() {
        // This test passes only if nightly is actually installed
        let result = check_nightly_available();
        if result.is_err() {
            eprintln!("SKIP: nightly toolchain not installed");
            return;
        }
        assert!(result.is_ok());
    }

    // ---- JSON path construction ----

    #[test]
    fn json_output_path_converts_hyphens_to_underscores() {
        let target = Path::new("/project/target");
        let path = json_output_path(target, "rmp-serde");
        assert_eq!(path, PathBuf::from("/project/target/doc/rmp_serde.json"));
    }

    #[test]
    fn json_output_path_preserves_underscores() {
        let target = Path::new("/project/target");
        let path = json_output_path(target, "serde_json");
        assert_eq!(path, PathBuf::from("/project/target/doc/serde_json.json"));
    }

    #[test]
    fn json_output_path_simple_name() {
        let target = Path::new("/project/target");
        let path = json_output_path(target, "serde");
        assert_eq!(path, PathBuf::from("/project/target/doc/serde.json"));
    }

    // ---- Feature flag command construction ----

    #[test]
    fn build_command_includes_nightly_and_json_format() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "+nightly"));
        assert!(has_arg(&args, "rustdoc"));
        assert!(has_arg(&args, "--output-format"));
        assert!(has_arg(&args, "json"));
        assert!(has_arg(&args, "-Z"));
        assert!(has_arg(&args, "unstable-options"));
    }

    #[test]
    fn build_command_includes_lib_flag_when_requested() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, true);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--lib"));
    }

    #[test]
    fn build_command_omits_lib_flag_when_not_requested() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false);
        let args = format_command_args(&cmd);
        assert!(!has_arg(&args, "--lib"));
    }

    #[test]
    fn build_command_includes_package_when_provided() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, Some("serde"), None, None, &features, false, false);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "-p"));
        assert!(has_arg(&args, "serde"));
    }

    #[test]
    fn build_command_includes_manifest_path_when_provided() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(
            None,
            None,
            Some(Path::new("/tmp/Cargo.toml")),
            None,
            &features,
            false,
            true,
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--manifest-path"));
        assert!(has_arg(&args, "/tmp/Cargo.toml"));
    }

    #[test]
    fn build_command_includes_target_dir_when_provided() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(
            None,
            None,
            None,
            Some(Path::new("/tmp/target")),
            &features,
            false,
            false,
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--target-dir"));
        assert!(has_arg(&args, "/tmp/target"));
    }

    #[test]
    fn build_command_includes_all_features_flag() {
        let features = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--all-features"));
    }

    #[test]
    fn build_command_includes_no_default_features_flag() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: true,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--no-default-features"));
    }

    #[test]
    fn build_command_includes_specific_features() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: vec!["fs".to_string(), "net".to_string()],
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--features"));
        assert!(has_arg(&args, "fs,net"));
    }

    #[test]
    fn build_command_includes_document_private_items_when_private() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, true, false);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--"));
        assert!(has_arg(&args, "--document-private-items"));
    }

    #[test]
    fn build_command_omits_private_items_when_not_private() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false);
        let args = format_command_args(&cmd);
        assert!(!has_arg(&args, "--document-private-items"));
    }

    #[test]
    fn build_command_current_crate_uses_package_no_lib() {
        let features = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        // CurrentCrate: uses -p, no --lib
        let cmd = build_rustdoc_command(
            Some(Path::new("/workspace")),
            Some("my_crate"),
            None,
            Some(Path::new("/workspace/target")),
            &features,
            false,
            false,
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "-p"));
        assert!(has_arg(&args, "my_crate"));
        assert!(!has_arg(&args, "--lib"));
    }

    #[test]
    fn build_command_external_crate_uses_lib_no_package() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        // External: uses --lib, no -p
        let cmd = build_rustdoc_command(
            Some(Path::new("/cache/serde-1.0.210")),
            None,
            None,
            Some(Path::new("/cache/serde-1.0.210/target")),
            &features,
            false,
            true,
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--lib"));
        assert!(!has_arg(&args, "-p"));
    }

    #[test]
    fn build_command_stdlib_uses_manifest_path_and_target_dir() {
        let features = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(
            None,
            None,
            Some(Path::new(
                "/sysroot/lib/rustlib/src/rust/library/std/Cargo.toml",
            )),
            Some(Path::new("/cache/stdlib/target-std-abc123")),
            &features,
            false,
            true,
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--manifest-path"));
        assert!(has_arg(&args, "--target-dir"));
        assert!(has_arg(&args, "--lib"));
        assert!(!has_arg(&args, "-p"));
    }

    // ---- Platform failure detection ----

    #[test]
    fn is_platform_failure_detects_custom_build_command() {
        assert!(is_platform_failure(
            "error: failed to run custom build command for `openssl-sys v0.9`"
        ));
    }

    #[test]
    fn is_platform_failure_detects_linker_error() {
        assert!(is_platform_failure("error: linker `cc` returned error"));
    }

    #[test]
    fn is_platform_failure_detects_library_not_found() {
        assert!(is_platform_failure("ld: library not found for -lssl"));
    }

    #[test]
    fn is_platform_failure_detects_framework_not_found() {
        assert!(is_platform_failure("ld: framework not found Security"));
    }

    #[test]
    fn is_platform_failure_detects_cannot_find() {
        assert!(is_platform_failure("ld: cannot find -lz"));
    }

    #[test]
    fn is_platform_failure_detects_unable_to_find() {
        assert!(is_platform_failure("Unable to find libclang"));
    }

    #[test]
    fn is_platform_failure_detects_not_found_in_path() {
        assert!(is_platform_failure("cmake not found in PATH"));
    }

    #[test]
    fn is_platform_failure_detects_windows_link_error() {
        assert!(is_platform_failure("LINK : fatal error LNK1181"));
    }

    #[test]
    fn is_platform_failure_detects_features_outside_workspace() {
        assert!(is_platform_failure(
            "cannot specify features for packages outside of workspace"
        ));
    }

    #[test]
    fn is_platform_failure_returns_false_for_regular_error() {
        assert!(!is_platform_failure("error[E0412]: cannot find type `Foo`"));
    }

    #[test]
    fn is_platform_failure_returns_false_for_empty_stderr() {
        assert!(!is_platform_failure(""));
    }

    #[test]
    fn is_platform_failure_case_insensitive() {
        assert!(is_platform_failure("FAILED TO RUN CUSTOM BUILD COMMAND"));
    }

    /// Extracts command arguments as a list of strings from the Command's debug output.
    fn format_command_args(cmd: &Command) -> Vec<String> {
        let debug = format!("{cmd:?}");
        // The Debug format for Command looks like:
        // "cargo" "+nightly" "rustdoc" "--lib" ...
        // Extract each quoted string
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
