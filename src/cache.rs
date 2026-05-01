use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{GroxError, Result};
use crate::resolve::CrateSource;
use crate::types::DocIndex;

/// Current cache format version. Bump when serialization format changes.
const FORMAT_VERSION: u32 = 4;

/// Serialized cache file: header + index, both MessagePack-encoded.
#[derive(Serialize, Deserialize)]
struct CachedData {
    header: CacheHeader,
    index: DocIndex,
}

/// Metadata at the start of each `.groxide` cache file.
#[derive(Serialize, Deserialize)]
struct CacheHeader {
    /// groxide version, e.g., "0.1.0".
    grox_version: String,
    /// Serialization format version.
    format_version: u32,
    /// UNIX epoch seconds when cache was created.
    created_at: u64,
    /// Source-specific invalidation data.
    metadata: CacheMetadata,
}

/// Source-specific invalidation metadata stored in the cache header.
#[derive(Serialize, Deserialize)]
enum CacheMetadata {
    /// Source-tree max mtime (UNIX seconds) at the time the cache was saved.
    /// On load, we recompute the source-tree mtime and compare; any edit to
    /// `Cargo.toml`, `src/**/*.rs`, or `build.rs` invalidates.
    CurrentCrate { source_mtime: u64 },
    Dependency { version: String },
    StdLib { toolchain_version: String },
    External { version: String },
}

/// Builds the cache-key suffix that distinguishes rustdoc outputs produced
/// with different feature/private settings. The same crate at the same
/// version compiled with `--all-features` or `--private` produces a
/// different `DocIndex`, so they must hit different cache files.
pub(crate) fn cache_suffix(features: &crate::cli::FeatureFlags, private: bool) -> String {
    let mut suffix = features.cache_suffix();
    if private {
        suffix.push_str("-priv");
    }
    suffix
}

/// Computes the cache file path for a given crate source.
///
/// * Current crate: `<workspace_target>/groxide/<crate>{suffix}.groxide` —
///   colocated with cargo's own incremental artifacts so a normal
///   `cargo clean` wipes both. Requires `ctx` to know the workspace target.
/// * Dependencies / stdlib / external: under `~/.cache/groxide/` because
///   they're shared across projects (or have no project at all).
pub(crate) fn cache_path(
    source: &CrateSource,
    feature_suffix: &str,
    ctx: Option<&crate::resolve::ProjectContext>,
) -> Option<PathBuf> {
    match source {
        CrateSource::CurrentCrate { name, .. } => {
            let target_dir = ctx?.workspace_target_directory();
            let filename = format!("{name}{feature_suffix}.groxide");
            Some(target_dir.join("groxide").join(filename))
        }
        CrateSource::Dependency { name, version, .. } => {
            let cache_dir = dirs::cache_dir()?;
            let filename = format!("{name}-{version}{feature_suffix}.groxide");
            Some(cache_dir.join("groxide").join("deps").join(filename))
        }
        CrateSource::Stdlib { name } => {
            let cache_dir = dirs::cache_dir()?;
            let toolchain = crate::stdlib::get_toolchain_hash().ok()?;
            let filename = format!("{name}-{toolchain}{feature_suffix}.groxide");
            Some(cache_dir.join("groxide").join("stdlib").join(filename))
        }
        CrateSource::External { name, version } => {
            let cache_dir = dirs::cache_dir()?;
            let ver = version.as_deref().unwrap_or("latest");
            let filename = format!("{name}-{ver}{feature_suffix}.groxide");
            Some(cache_dir.join("groxide").join("external").join(filename))
        }
    }
}

/// Returns the max mtime (UNIX seconds) of the inputs that affect a crate's
/// rustdoc JSON. Walks:
///
/// * `<package_dir>/Cargo.toml` (required)
/// * `<package_dir>/build.rs` (optional)
/// * `<package_dir>/src/**/*.rs`
/// * `<workspace_root>/Cargo.toml` and `<workspace_root>/Cargo.lock`
///   when `workspace_root` is `Some` and differs from `package_dir`. The
///   workspace `Cargo.toml` matters because `[workspace.dependencies]`
///   edits flow into member crates via inheritance; `Cargo.lock` matters
///   because a transitive dep bump can alter the surface a `pub use`
///   statement re-exports.
///
/// Known limitations (intentional): doesn't track build-script-generated
/// source under `OUT_DIR`, `#[path = ...]` files outside `src/`, or
/// rustflags / config.toml changes. Run `grox --clear-cache` to recover
/// from those cases on the rare occasions they bite.
pub(crate) fn current_crate_source_mtime(
    package_dir: &Path,
    workspace_root: Option<&Path>,
) -> Option<u64> {
    fn mtime_secs(path: &Path) -> Option<u64> {
        fs::metadata(path)
            .ok()?
            .modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_secs())
    }

    let mut max = mtime_secs(&package_dir.join("Cargo.toml"))?;

    if let Some(t) = mtime_secs(&package_dir.join("build.rs")) {
        max = max.max(t);
    }

    let src_dir = package_dir.join("src");
    if src_dir.is_dir() {
        let mut stack = vec![src_dir];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if file_type.is_dir() {
                    stack.push(path);
                } else if file_type.is_file()
                    && path.extension().is_some_and(|e| e == "rs")
                {
                    if let Some(t) = mtime_secs(&path) {
                        max = max.max(t);
                    }
                }
            }
        }
    }

    if let Some(ws_root) = workspace_root {
        if ws_root != package_dir {
            // Workspace Cargo.toml — different file from the package one
            if let Some(t) = mtime_secs(&ws_root.join("Cargo.toml")) {
                max = max.max(t);
            }
        }
        // Cargo.lock always at the workspace root (or single-crate package
        // root, which equals package_dir — same file path either way).
        if let Some(t) = mtime_secs(&ws_root.join("Cargo.lock")) {
            max = max.max(t);
        }
    }

    Some(max)
}

/// Loads a cached [`DocIndex`] from disk if the cache is valid.
///
/// Returns `None` if the cache file doesn't exist, is corrupted, or is stale.
pub(crate) fn load_cached(
    path: &Path,
    source: &CrateSource,
    ctx: Option<&crate::resolve::ProjectContext>,
) -> Option<DocIndex> {
    if !path.exists() {
        return None;
    }

    // Debug-only: invalidate if binary is newer than cache
    #[cfg(debug_assertions)]
    if is_binary_newer_than_cache(path) {
        return None;
    }

    let bytes = fs::read(path).ok()?;
    let data: CachedData = rmp_serde::from_slice(&bytes).ok()?;

    let current_mtime = compute_source_mtime(source, ctx);

    if !is_cache_valid(&data.header, source, current_mtime) {
        return None;
    }

    Some(data.index)
}

/// Saves a [`DocIndex`] to disk with atomic write (temp file + rename).
///
/// Cache save errors are non-fatal — logs a warning to stderr and continues.
pub(crate) fn save_to_cache(
    path: &Path,
    index: &DocIndex,
    source: &CrateSource,
    ctx: Option<&crate::resolve::ProjectContext>,
) {
    if let Err(e) = save_to_cache_inner(path, index, source, ctx) {
        eprintln!("[grox] warning: failed to save cache: {e}");
    }
}

/// Computes the source-tree mtime used for `CurrentCrate` cache invalidation.
/// Returns `None` for variants that don't use mtime keying (their cache key
/// is version- or toolchain-based instead).
fn compute_source_mtime(
    source: &CrateSource,
    ctx: Option<&crate::resolve::ProjectContext>,
) -> Option<u64> {
    match source {
        CrateSource::CurrentCrate { manifest_path, .. } => {
            let package_dir = manifest_path
                .parent()
                .expect("invariant: manifest_path has a parent");
            let workspace_root = ctx.map(crate::resolve::ProjectContext::workspace_root);
            current_crate_source_mtime(package_dir, workspace_root.as_deref())
        }
        _ => None,
    }
}

/// Inner implementation of cache saving that can return errors.
fn save_to_cache_inner(
    path: &Path,
    index: &DocIndex,
    source: &CrateSource,
    ctx: Option<&crate::resolve::ProjectContext>,
) -> Result<()> {
    let mtime = compute_source_mtime(source, ctx);
    let header = create_header(source, mtime);
    let data = CachedData {
        header,
        index: index.clone(),
    };

    let bytes = rmp_serde::to_vec(&data).map_err(|e| GroxError::CacheSerializationFailed {
        message: format!("serialize: {e}"),
    })?;

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Atomic write: temp file + rename
    let pid = std::process::id();
    let temp_path = path.with_extension(format!("tmp.{pid}"));

    if let Err(e) = fs::write(&temp_path, &bytes) {
        return Err(GroxError::CacheSerializationFailed {
            message: format!("write temp file: {e}"),
        });
    }

    if let Err(e) = fs::rename(&temp_path, path) {
        // Clean up temp file on rename failure
        let _ = fs::remove_file(&temp_path);
        return Err(GroxError::CacheSerializationFailed {
            message: format!("rename: {e}"),
        });
    }

    Ok(())
}

/// Creates a cache header for the given crate source.
///
/// `current_mtime` must be `Some` for `CrateSource::CurrentCrate`; it's the
/// max source-tree mtime that becomes the invalidation key.
fn create_header(source: &CrateSource, current_mtime: Option<u64>) -> CacheHeader {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let metadata = match source {
        CrateSource::CurrentCrate { .. } => CacheMetadata::CurrentCrate {
            source_mtime: current_mtime.unwrap_or(0),
        },
        CrateSource::Dependency { version: ver, .. } => CacheMetadata::Dependency {
            version: ver.clone(),
        },
        CrateSource::Stdlib { .. } => {
            let toolchain =
                crate::stdlib::get_toolchain_hash().unwrap_or_else(|_| "unknown".to_string());
            CacheMetadata::StdLib {
                toolchain_version: toolchain,
            }
        }
        CrateSource::External { version, .. } => CacheMetadata::External {
            version: version.as_deref().unwrap_or("unknown").to_string(),
        },
    };

    CacheHeader {
        grox_version: env!("CARGO_PKG_VERSION").to_string(),
        format_version: FORMAT_VERSION,
        created_at,
        metadata,
    }
}

/// Validates a cache header against the current source state.
///
/// `current_mtime` must be `Some` for `CrateSource::CurrentCrate` (the
/// caller has just computed it); other variants ignore it.
fn is_cache_valid(
    header: &CacheHeader,
    source: &CrateSource,
    current_mtime: Option<u64>,
) -> bool {
    // Version mismatch: always invalidate
    if header.grox_version != env!("CARGO_PKG_VERSION") {
        return false;
    }
    if header.format_version != FORMAT_VERSION {
        return false;
    }

    match (&header.metadata, source) {
        (
            CacheMetadata::CurrentCrate {
                source_mtime: cached_mtime,
            },
            CrateSource::CurrentCrate { .. },
        ) => current_mtime.is_some_and(|now| now == *cached_mtime),
        (
            CacheMetadata::Dependency {
                version: cached_version,
            },
            CrateSource::Dependency { version, .. },
        ) => cached_version == version,
        (
            CacheMetadata::StdLib {
                toolchain_version: cached_toolchain,
            },
            CrateSource::Stdlib { .. },
        ) => {
            let current =
                crate::stdlib::get_toolchain_hash().unwrap_or_else(|_| "unknown".to_string());
            cached_toolchain == &current
        }
        (
            CacheMetadata::External {
                version: cached_version,
            },
            CrateSource::External { version, .. },
        ) => {
            let current = version.as_deref().unwrap_or("unknown");
            cached_version == current
        }
        // Source type mismatch — cache is invalid
        _ => false,
    }
}

/// Debug-only: checks if the groxide binary is newer than the cache file.
#[cfg(debug_assertions)]
fn is_binary_newer_than_cache(cache_path: &Path) -> bool {
    let Ok(binary_path) = std::env::current_exe() else {
        return false;
    };

    let Ok(binary_mtime) = fs::metadata(&binary_path).and_then(|m| m.modified()) else {
        return false;
    };

    let Ok(cache_mtime) = fs::metadata(cache_path).and_then(|m| m.modified()) else {
        return true; // can't read cache mtime, treat as stale
    };

    binary_mtime > cache_mtime
}

/// Removes the global cache directory.
///
/// Returns the path that was cleared, or `None` if the cache dir could not be determined.
pub(crate) fn clear_global_cache() -> Option<PathBuf> {
    let cache_dir = dirs::cache_dir()?.join("groxide");
    if cache_dir.exists() {
        let _ = fs::remove_dir_all(&cache_dir);
    }
    Some(cache_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::CrateSpec;
    use crate::types::{DocIndex, IndexItem, ItemKind, SourceSpan};
    use tempfile::TempDir;

    /// Creates a minimal test `DocIndex` with one item.
    fn make_test_index() -> DocIndex {
        let mut index = DocIndex::new("testcrate".to_string(), "1.0.0".to_string());
        index.add_item(IndexItem {
            path: "testcrate::Foo".to_string(),
            name: "Foo".to_string(),
            kind: ItemKind::Struct,
            signature: "pub struct Foo".to_string(),
            docs: "A test struct.".to_string(),
            summary: "A test struct.".to_string(),
            span: SourceSpan {
                file: "src/lib.rs".to_string(),
                line_start: 1,
                line_end: 5,
            },
            children: Vec::new(),
            is_public: true,
            has_body: false,
            feature_gate: None,
            reexport_source: None,
        });
        index
    }

    /// Creates a `CrateSource::Dependency` pointing at a temp directory.
    fn make_dep_source(tmp: &Path) -> CrateSource {
        CrateSource::Dependency {
            manifest_path: tmp.join("Cargo.toml"),
            name: "testcrate".to_string(),
            version: "1.0.0".to_string(),
        }
    }

    // ---- Round-trip: save and load ----

    #[test]
    fn round_trip_dependency_save_load() {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.path();
        let source = make_dep_source(tmp_path);

        let index = make_test_index();
        // Use a temp dir path directly — cache_path() returns global dir which we shouldn't pollute in tests
        let cache_file = tmp_path.join("deps/testcrate-1.0.0.groxide");

        save_to_cache(&cache_file, &index, &source, None);
        let loaded = load_cached(&cache_file, &source, None);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().crate_name, "testcrate");
    }

    // ---- Cache path includes version ----

    #[test]
    fn cache_path_for_current_crate_requires_project_context() {
        // CurrentCrate caches live in the workspace target dir, which
        // we can only learn about from a ProjectContext. Without ctx,
        // cache_path returns None — call sites that have a project
        // always pass one.
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::CurrentCrate {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "mycrate".to_string(),
            version: "2.3.4".to_string(),
        };
        assert!(cache_path(&source, "", None).is_none());
    }

    #[test]
    fn cache_path_for_current_crate_with_ctx_lives_in_workspace_target() {
        // Run against the live groxide project. Cache should be
        // <workspace_target>/groxide/<crate>{suffix}.groxide.
        let ctx =
            crate::resolve::ProjectContext::discover(None).expect("groxide project context");
        let source = ctx.resolve_crate(&CrateSpec::CurrentCrate);
        let path =
            cache_path(&source, "", Some(&ctx)).expect("Some(path) when ctx is provided");
        assert!(
            path.parent().is_some_and(|p| p.ends_with("groxide")),
            "path should be in <target>/groxide/, got {path:?}"
        );
        assert_eq!(
            path.file_name().and_then(|n| n.to_str()),
            Some("groxide.groxide"),
        );
    }

    #[test]
    fn cache_path_includes_version_for_dependency() {
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::Dependency {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "serde".to_string(),
            version: "1.0.210".to_string(),
        };
        let path = cache_path(&source, "", None).unwrap();
        assert!(
            path.to_str().unwrap().contains("serde-1.0.210.groxide"),
            "path should include version: {path:?}"
        );
    }

    #[test]
    fn cache_path_includes_version_for_external() {
        let source = CrateSource::External {
            name: "tokio".to_string(),
            version: Some("1.40.0".to_string()),
        };
        let path = cache_path(&source, "", None).unwrap();
        assert!(
            path.to_str().unwrap().contains("tokio-1.40.0.groxide"),
            "path should include version: {path:?}"
        );
    }

    // ---- Feature suffix changes path ----

    #[test]
    fn cache_path_changes_with_feature_suffix() {
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::Dependency {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "mycrate".to_string(),
            version: "1.0.0".to_string(),
        };

        let path_default = cache_path(&source, "", None).unwrap();
        let path_feat = cache_path(&source, "-feat_0a1b2c3d4e5f6a7b", None).unwrap();

        assert_ne!(path_default, path_feat);
        assert!(
            path_feat
                .to_str()
                .unwrap()
                .contains("-feat_0a1b2c3d4e5f6a7b"),
            "path should include feature suffix: {path_feat:?}"
        );
    }

    #[test]
    fn cache_path_no_suffix_for_default_features() {
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::Dependency {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "mycrate".to_string(),
            version: "1.0.0".to_string(),
        };
        let path = cache_path(&source, "", None).unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename, "mycrate-1.0.0.groxide");
    }

    // ---- Atomic write: temp file cleaned up ----

    #[test]
    fn atomic_write_no_temp_file_left_behind() {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.path();
        let source = make_dep_source(tmp_path);
        let index = make_test_index();

        let cache_dir = tmp_path.join("deps");
        let cache_file = cache_dir.join("testcrate-1.0.0.groxide");

        save_to_cache(&cache_file, &index, &source, None);

        // Check that no .tmp.* files remain in the cache directory
        let entries: Vec<_> = fs::read_dir(&cache_dir)
            .unwrap()
            .flatten()
            .filter(|e| e.path().to_str().unwrap_or("").contains(".tmp."))
            .collect();
        assert!(
            entries.is_empty(),
            "no temp files should remain after successful save"
        );
    }

    // ---- Dependency version mismatch invalidation ----

    #[test]
    fn stale_cache_detected_when_dep_version_changes() {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.path();
        let source_v1 = CrateSource::Dependency {
            manifest_path: tmp_path.join("Cargo.toml"),
            name: "serde".to_string(),
            version: "1.0.0".to_string(),
        };
        let source_v2 = CrateSource::Dependency {
            manifest_path: tmp_path.join("Cargo.toml"),
            name: "serde".to_string(),
            version: "1.0.1".to_string(),
        };

        let index = make_test_index();
        let cache_file = tmp_path.join("deps/serde-1.0.0.groxide");

        save_to_cache(&cache_file, &index, &source_v1, None);
        assert!(
            load_cached(&cache_file, &source_v1, None).is_some(),
            "cache should be valid for v1"
        );
        assert!(
            load_cached(&cache_file, &source_v2, None).is_none(),
            "cache should be invalid for v2"
        );
    }

    // ---- load_cached returns None for missing file ----

    #[test]
    fn load_cached_returns_none_for_missing_file() {
        let tmp = TempDir::new().unwrap();
        let source = make_dep_source(tmp.path());
        let result = load_cached(&tmp.path().join("nonexistent.groxide"), &source, None);
        assert!(result.is_none());
    }

    // ---- load_cached returns None for corrupted file ----

    #[test]
    fn load_cached_returns_none_for_corrupted_file() {
        let tmp = TempDir::new().unwrap();
        let cache_file = tmp.path().join("corrupt.groxide");
        fs::write(&cache_file, b"not valid msgpack data").unwrap();
        let source = make_dep_source(tmp.path());
        let result = load_cached(&cache_file, &source, None);
        assert!(result.is_none());
    }

    // ---- cache_path for stdlib ----

    #[test]
    fn cache_path_stdlib_uses_global_cache() {
        let source = CrateSource::Stdlib {
            name: "std".to_string(),
        };
        // This test requires nightly for toolchain hash detection
        let Some(path) = cache_path(&source, "", None) else {
            eprintln!("SKIP: nightly not available for toolchain hash");
            return;
        };
        let path_str = path.to_str().unwrap();
        assert!(
            path_str.contains("groxide/stdlib/"),
            "stdlib should use global cache: {path_str}"
        );
        // Path now includes toolchain hash: std-<hash>.groxide
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert!(
            filename.starts_with("std-"),
            "should start with crate name and dash: {filename}"
        );
        assert!(
            filename.ends_with(".groxide"),
            "should end with .groxide: {filename}"
        );
    }

    // ---- cache_path for external ----

    #[test]
    fn cache_path_external_uses_global_cache() {
        let source = CrateSource::External {
            name: "tokio".to_string(),
            version: Some("1.40.0".to_string()),
        };
        let path = cache_path(&source, "", None).unwrap();
        let path_str = path.to_str().unwrap();
        assert!(
            path_str.contains("groxide/external/"),
            "external should use global cache: {path_str}"
        );
    }

    // ---- dependency uses global cache ----

    #[test]
    fn cache_path_dependency_uses_global_cache() {
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::Dependency {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "serde".to_string(),
            version: "1.0.0".to_string(),
        };
        let path = cache_path(&source, "", None).unwrap();
        let path_str = path.to_str().unwrap();
        assert!(
            path_str.contains("groxide/deps/"),
            "dependency should use global cache: {path_str}"
        );
    }

    // ---- clear_global_cache ----

    #[test]
    fn clear_global_cache_returns_some_path() {
        let result = clear_global_cache();
        assert!(
            result.is_some(),
            "should return a path on systems with a cache dir"
        );
        let path = result.unwrap();
        assert!(
            path.ends_with("groxide"),
            "path should end with 'groxide': {path:?}"
        );
    }
}
