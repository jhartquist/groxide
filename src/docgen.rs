use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::cli::FeatureFlags;
use crate::error::{GroxError, Result};
use crate::resolve::CrateSource;

/// Metadata from `[package.metadata.docs.rs]` in a crate's Cargo.toml.
///
/// This is a well-established convention used by docs.rs to configure how
/// documentation is built. Crates like tokio use this to specify features,
/// rustdoc args, and cfg flags needed to build complete documentation.
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct DocsRsMetadata {
    all_features: bool,
    no_default_features: bool,
    features: Vec<String>,
    rustdoc_args: Vec<String>,
    rustc_args: Vec<String>,
}

/// Reads `[package.metadata.docs.rs]` from a crate's Cargo.toml.
///
/// Returns `None` if the metadata is absent or cannot be parsed. This provides
/// graceful degradation — callers fall back to their default build strategy.
fn read_docsrs_metadata(crate_dir: &Path) -> Option<DocsRsMetadata> {
    let manifest_path = crate_dir.join("Cargo.toml");
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(&manifest_path)
        .no_deps()
        .exec()
        .ok()?;

    let package = metadata.packages.first()?;
    let docs_rs = package.metadata.get("docs")?.get("rs")?;
    serde_json::from_value(docs_rs.clone()).ok()
}

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
        } => {
            let package_dir = manifest_path
                .parent()
                .expect("invariant: manifest_path has a parent");
            let target_dir = find_workspace_target_dir(manifest_path)?;
            generate_for_current_crate(
                package_dir,
                name,
                version,
                &target_dir,
                features,
                private,
            )?;
            let json_path = json_output_path(&target_dir, name);
            Ok(json_path)
        }
        CrateSource::Dependency {
            manifest_path,
            name,
            ..
        } => {
            let target_dir = find_workspace_target_dir(manifest_path)?;
            generate_for_dependency(&target_dir, name, features, private)?;
            let json_path = json_output_path(&target_dir, name);
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

/// Returns the path to the docs.rs failure cache file.
fn docsrs_failure_cache_path() -> Option<PathBuf> {
    Some(
        dirs::cache_dir()?
            .join("groxide")
            .join("docsrs-failures.json"),
    )
}

/// Loads the set of known docs.rs metadata build failures from disk.
fn load_docsrs_failures(path: &Path) -> HashSet<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Records that a docs.rs metadata build failed for a crate at a specific version.
fn record_docsrs_failure(name: &str, version: &str) {
    let Some(path) = docsrs_failure_cache_path() else {
        return;
    };
    let mut failures = load_docsrs_failures(&path);
    failures.insert(format!("{name}@{version}"));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, serde_json::to_string(&failures).unwrap_or_default());
}

/// Checks whether docs.rs metadata is known to fail for this crate at a specific version.
fn is_docsrs_known_failure(name: &str, version: &str) -> bool {
    let Some(path) = docsrs_failure_cache_path() else {
        return false;
    };
    let failures = load_docsrs_failures(&path);
    failures.contains(&format!("{name}@{version}"))
}

/// Runs the feature cascade strategy for generating rustdoc JSON.
///
/// The cascade tries strategies in order until one succeeds:
/// 1. If the user specified explicit feature flags, use them directly (no fallback).
/// 2. Try `[package.metadata.docs.rs]` settings if present.
/// 3. Try `--all-features`.
/// 4. Final fallback: default features.
///
/// Steps 2-4 each fall back on **any** build failure — not just platform failures.
/// `is_platform_failure()` is only used to choose the log message wording.
fn generate_with_feature_cascade(
    crate_dir: &Path,
    crate_name: &str,
    crate_version: Option<&str>,
    features: &FeatureFlags,
    build_cmd: impl Fn(&FeatureFlags, &[String], &[String]) -> Command,
) -> Result<()> {
    // User specified explicit flags — use them directly, no fallback
    if !features.is_default() {
        let cmd = build_cmd(features, &[], &[]);
        return run_rustdoc_command(cmd);
    }

    // Try docs.rs metadata first (skip if known to fail for this crate@version)
    let skip_docsrs = crate_version.is_some_and(|v| is_docsrs_known_failure(crate_name, v));

    if skip_docsrs {
        eprintln!("[grox] Skipping docs.rs metadata for {crate_name} (known failure)");
    } else if let Some(meta) = read_docsrs_metadata(crate_dir) {
        eprintln!("[grox] Using docs.rs metadata for {crate_name}");
        let meta_features = FeatureFlags {
            all_features: meta.all_features,
            no_default_features: meta.no_default_features,
            features: meta.features.clone(),
        };
        let cmd = build_cmd(&meta_features, &meta.rustdoc_args, &meta.rustc_args);

        match run_rustdoc_command_with_output(cmd) {
            Ok(()) => return Ok(()),
            Err(stderr) => {
                if let Some(v) = crate_version {
                    record_docsrs_failure(crate_name, v);
                }
                if is_platform_failure(&stderr) {
                    eprintln!(
                        "[grox] Build with docs.rs metadata failed (platform issue), \
                         retrying with default features..."
                    );
                } else {
                    eprintln!(
                        "[grox] Build with docs.rs metadata failed, \
                         retrying with default features..."
                    );
                }
            }
        }
    } else {
        // No docs.rs metadata: try --all-features
        let all_features = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_cmd(&all_features, &[], &[]);

        match run_rustdoc_command_with_output(cmd) {
            Ok(()) => return Ok(()),
            Err(stderr) => {
                if is_platform_failure(&stderr) {
                    eprintln!(
                        "[grox] Build with --all-features failed (platform issue), \
                         retrying with default features..."
                    );
                } else {
                    eprintln!(
                        "[grox] Build with --all-features failed, \
                         retrying with default features..."
                    );
                }
            }
        }
    }

    // Final fallback: default features
    let default_features = FeatureFlags {
        all_features: false,
        no_default_features: false,
        features: Vec::new(),
    };
    let cmd = build_cmd(&default_features, &[], &[]);
    run_rustdoc_command(cmd)
}

/// Generates rustdoc JSON for the current workspace crate.
///
/// Uses the shared feature cascade unless the user specified explicit feature
/// flags.
fn generate_for_current_crate(
    package_dir: &Path,
    crate_name: &str,
    crate_version: &str,
    target_dir: &Path,
    features: &FeatureFlags,
    private: bool,
) -> Result<()> {
    generate_with_feature_cascade(
        package_dir,
        crate_name,
        Some(crate_version),
        features,
        |f, rustdoc_args, rustc_args| {
            build_rustdoc_command(
                Some(package_dir),
                Some(crate_name),
                None,
                Some(target_dir),
                f,
                private,
                true, // --lib to disambiguate when crate has multiple targets
                rustdoc_args,
                rustc_args,
            )
        },
    )
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
        &[],
        &[],
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
/// Uses the shared feature cascade unless the user specified explicit feature
/// flags. Falls back on any build failure since external crates may have features
/// that require platform-specific deps or unstable cfg flags.
///
/// Called by the external crate fetching module after extraction.
pub(crate) fn generate_rustdoc_json_external(
    source_dir: &Path,
    crate_name: &str,
    crate_version: &str,
    features: &FeatureFlags,
    private: bool,
) -> Result<PathBuf> {
    check_nightly_available()?;

    let target_dir = source_dir.join("target");

    generate_with_feature_cascade(
        source_dir,
        crate_name,
        Some(crate_version),
        features,
        |f, rustdoc_args, rustc_args| {
            build_rustdoc_command(
                Some(source_dir),
                None,
                None,
                Some(&target_dir),
                f,
                private,
                true, // --lib for non-workspace crates
                rustdoc_args,
                rustc_args,
            )
        },
    )?;

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
/// * `extra_rustdoc_args` - Additional args passed after `--` (e.g. from docs.rs metadata).
/// * `rustc_env_args` - Args set as `RUSTFLAGS` env var (e.g. `--cfg` flags).
#[allow(clippy::too_many_arguments)]
fn build_rustdoc_command(
    working_dir: Option<&Path>,
    package: Option<&str>,
    manifest_path: Option<&Path>,
    target_dir: Option<&Path>,
    features: &FeatureFlags,
    private: bool,
    use_lib_flag: bool,
    extra_rustdoc_args: &[String],
    rustc_env_args: &[String],
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

    // Rustdoc-specific args go after `--`
    cmd.arg("--");
    cmd.arg("--document-hidden-items");
    if private {
        cmd.arg("--document-private-items");
    }
    for arg in extra_rustdoc_args {
        cmd.arg(arg);
    }

    // rustc args passed via RUSTFLAGS env var
    if !rustc_env_args.is_empty() {
        cmd.env("RUSTFLAGS", rustc_env_args.join(" "));
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
        if let Some(name) = parse_no_library_target(&stderr) {
            return Err(GroxError::NoLibraryTarget { name });
        }
        Err(GroxError::RustdocFailed { stderr })
    }
}

/// Detects cargo's "no library targets found in package `<name>`" error and
/// extracts the package name. Returns `None` when stderr matches a different
/// failure.
fn parse_no_library_target(stderr: &str) -> Option<String> {
    let needle = "no library targets found in package `";
    let start = stderr.find(needle)? + needle.len();
    let rest = &stderr[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
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
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false, &[], &[]);
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
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, true, &[], &[]);
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
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false, &[], &[]);
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
        let cmd = build_rustdoc_command(
            None,
            Some("serde"),
            None,
            None,
            &features,
            false,
            false,
            &[],
            &[],
        );
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
            &[],
            &[],
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
            &[],
            &[],
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
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false, &[], &[]);
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
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false, &[], &[]);
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
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false, &[], &[]);
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
        let cmd = build_rustdoc_command(None, None, None, None, &features, true, false, &[], &[]);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--"));
        assert!(has_arg(&args, "--document-hidden-items"));
        assert!(has_arg(&args, "--document-private-items"));
    }

    #[test]
    fn build_command_omits_private_items_when_not_private() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false, &[], &[]);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--"));
        assert!(has_arg(&args, "--document-hidden-items"));
        assert!(!has_arg(&args, "--document-private-items"));
    }

    #[test]
    fn build_command_always_includes_document_hidden_items() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        // Without private flag
        let cmd = build_rustdoc_command(None, None, None, None, &features, false, false, &[], &[]);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--document-hidden-items"));

        // With private flag
        let cmd = build_rustdoc_command(None, None, None, None, &features, true, false, &[], &[]);
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--document-hidden-items"));
    }

    #[test]
    fn build_command_current_crate_uses_package_and_lib() {
        let features = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        // CurrentCrate: uses -p and --lib to disambiguate multiple targets
        let cmd = build_rustdoc_command(
            Some(Path::new("/workspace")),
            Some("my_crate"),
            None,
            Some(Path::new("/workspace/target")),
            &features,
            false,
            true,
            &[],
            &[],
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "-p"));
        assert!(has_arg(&args, "my_crate"));
        assert!(has_arg(&args, "--lib"));
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
            &[],
            &[],
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
            &[],
            &[],
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

    // ---- DocsRsMetadata deserialization ----

    #[test]
    fn docsrs_metadata_deserializes_all_fields() {
        let json = serde_json::json!({
            "all-features": true,
            "no-default-features": false,
            "features": ["sync", "fs"],
            "rustdoc-args": ["--cfg", "docsrs"],
            "rustc-args": ["--cfg", "tokio_unstable"]
        });
        let meta: DocsRsMetadata = serde_json::from_value(json).unwrap();
        assert!(meta.all_features);
        assert!(!meta.no_default_features);
        assert_eq!(meta.features, vec!["sync", "fs"]);
        assert_eq!(meta.rustdoc_args, vec!["--cfg", "docsrs"]);
        assert_eq!(meta.rustc_args, vec!["--cfg", "tokio_unstable"]);
    }

    #[test]
    fn docsrs_metadata_defaults_missing_fields() {
        let json = serde_json::json!({});
        let meta: DocsRsMetadata = serde_json::from_value(json).unwrap();
        assert!(!meta.all_features);
        assert!(!meta.no_default_features);
        assert!(meta.features.is_empty());
        assert!(meta.rustdoc_args.is_empty());
        assert!(meta.rustc_args.is_empty());
    }

    #[test]
    fn docsrs_metadata_ignores_unknown_fields() {
        let json = serde_json::json!({
            "all-features": true,
            "default-target": "x86_64-unknown-linux-gnu",
            "targets": ["x86_64-unknown-linux-gnu"]
        });
        let meta: DocsRsMetadata = serde_json::from_value(json).unwrap();
        assert!(meta.all_features);
    }

    #[test]
    fn docsrs_metadata_partial_fields() {
        let json = serde_json::json!({
            "features": ["full"],
            "rustdoc-args": ["--cfg", "docsrs"]
        });
        let meta: DocsRsMetadata = serde_json::from_value(json).unwrap();
        assert!(!meta.all_features);
        assert_eq!(meta.features, vec!["full"]);
        assert_eq!(meta.rustdoc_args, vec!["--cfg", "docsrs"]);
        assert!(meta.rustc_args.is_empty());
    }

    #[test]
    fn docsrs_metadata_tokio_style() {
        // Tokio's actual docs.rs metadata configuration
        let json = serde_json::json!({
            "all-features": true,
            "rustdoc-args": ["--cfg", "docsrs"],
            "rustc-args": ["--cfg", "tokio_unstable"]
        });
        let meta: DocsRsMetadata = serde_json::from_value(json).unwrap();
        assert!(meta.all_features);
        assert_eq!(meta.rustdoc_args, vec!["--cfg", "docsrs"]);
        assert_eq!(meta.rustc_args, vec!["--cfg", "tokio_unstable"]);
    }

    // ---- Extra rustdoc args and RUSTFLAGS ----

    #[test]
    fn build_command_includes_extra_rustdoc_args_after_separator() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let extra_args = vec!["--cfg".to_string(), "docsrs".to_string()];
        let cmd = build_rustdoc_command(
            None,
            None,
            None,
            None,
            &features,
            false,
            false,
            &extra_args,
            &[],
        );
        let args = format_command_args(&cmd);
        assert!(has_arg(&args, "--"));
        assert!(has_arg(&args, "--cfg"));
        assert!(has_arg(&args, "docsrs"));
    }

    #[test]
    fn build_command_combines_private_and_extra_rustdoc_args() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let extra_args = vec!["--cfg".to_string(), "docsrs".to_string()];
        let cmd = build_rustdoc_command(
            None,
            None,
            None,
            None,
            &features,
            true,
            false,
            &extra_args,
            &[],
        );
        let args = format_command_args(&cmd);
        // Should have exactly one -- separator with all args after it
        let separator_count = args.iter().filter(|a| *a == "--").count();
        assert_eq!(separator_count, 1);
        assert!(has_arg(&args, "--document-hidden-items"));
        assert!(has_arg(&args, "--document-private-items"));
        assert!(has_arg(&args, "--cfg"));
        assert!(has_arg(&args, "docsrs"));
    }

    #[test]
    fn build_command_sets_rustflags_env_var() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let rustc_args = vec!["--cfg".to_string(), "tokio_unstable".to_string()];
        let cmd = build_rustdoc_command(
            None,
            None,
            None,
            None,
            &features,
            false,
            false,
            &[],
            &rustc_args,
        );
        let envs: Vec<_> = cmd.get_envs().collect();
        let rustflags = envs.iter().find(|(k, _)| k == &"RUSTFLAGS");
        assert!(rustflags.is_some());
        let (_, val) = rustflags.unwrap();
        assert_eq!(val.unwrap().to_str().unwrap(), "--cfg tokio_unstable");
    }

    // ---- docs.rs failure cache ----

    #[test]
    fn record_and_check_docsrs_failure_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docsrs-failures.json");

        // Initially empty
        let failures = load_docsrs_failures(&path);
        assert!(failures.is_empty());

        // Record a failure by writing directly
        let mut failures = HashSet::new();
        failures.insert("wgpu@0.20.0".to_string());
        std::fs::write(&path, serde_json::to_string(&failures).unwrap()).unwrap();

        // Should be found
        let loaded = load_docsrs_failures(&path);
        assert!(loaded.contains("wgpu@0.20.0"));
        assert!(!loaded.contains("serde@1.0.0"));
    }

    #[test]
    fn is_docsrs_known_failure_returns_false_for_unknown_crate() {
        // With no cache file, should return false
        assert!(!is_docsrs_known_failure("nonexistent-crate", "0.0.0"));
    }

    #[test]
    fn load_docsrs_failures_returns_empty_when_file_missing() {
        let path = Path::new("/tmp/groxide-test-nonexistent/docsrs-failures.json");
        let failures = load_docsrs_failures(path);
        assert!(failures.is_empty());
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
