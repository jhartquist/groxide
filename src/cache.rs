use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{GroxError, Result};
use crate::resolve::CrateSource;
use crate::types::DocIndex;

/// Current cache format version. Bump when serialization format changes.
const FORMAT_VERSION: u32 = 1;

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
    CurrentCrate { newest_source_mtime: u64 },
    Dependency { package_version: String },
    StdLib { toolchain_version: String },
    External { crate_version: String },
}

/// Computes the cache file path for a given crate source.
///
/// Project caches go under `target/groxide/`, global caches under `~/.cache/groxide/`.
pub(crate) fn cache_path(source: &CrateSource, feature_suffix: &str) -> Option<PathBuf> {
    match source {
        CrateSource::CurrentCrate {
            manifest_path,
            name,
            version,
            ..
        } => {
            let workspace_root = manifest_path.parent()?;
            let filename = format!("{name}-{version}{feature_suffix}.groxide");
            Some(workspace_root.join("target").join("groxide").join(filename))
        }
        CrateSource::Dependency {
            manifest_path,
            name,
            version,
            ..
        } => {
            // Use the workspace target dir — walk up to workspace root via the manifest's parent
            let workspace_root = manifest_path.parent()?;
            let filename = format!("{name}-{version}{feature_suffix}.groxide");
            Some(workspace_root.join("target").join("groxide").join(filename))
        }
        CrateSource::Stdlib { name } => {
            let cache_dir = dirs::cache_dir()?;
            let filename = format!("{name}{feature_suffix}.groxide");
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

/// Loads a cached [`DocIndex`] from disk if the cache is valid.
///
/// Returns `None` if the cache file doesn't exist, is corrupted, or is stale.
pub(crate) fn load_cached(path: &Path, source: &CrateSource) -> Option<DocIndex> {
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

    if !is_cache_valid(&data.header, source) {
        return None;
    }

    Some(data.index)
}

/// Saves a [`DocIndex`] to disk with atomic write (temp file + rename).
///
/// Cache save errors are non-fatal — logs a warning to stderr and continues.
pub(crate) fn save_to_cache(path: &Path, index: &DocIndex, source: &CrateSource) {
    if let Err(e) = save_to_cache_inner(path, index, source) {
        eprintln!("[grox] warning: failed to save cache: {e}");
    }
}

/// Inner implementation of cache saving that can return errors.
fn save_to_cache_inner(path: &Path, index: &DocIndex, source: &CrateSource) -> Result<()> {
    let header = create_header(source);
    let data = CachedData {
        header,
        index: serialize_index_ref(index)?,
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

/// Serializes a `&DocIndex` into a `DocIndex` for embedding in `CachedData`.
///
/// This exists because `CachedData` owns its `DocIndex`. We re-serialize the reference
/// by serializing and deserializing through `rmp-serde`. This avoids requiring `Clone`
/// on `DocIndex`.
fn serialize_index_ref(index: &DocIndex) -> Result<DocIndex> {
    let bytes = rmp_serde::to_vec(index).map_err(|e| GroxError::CacheSerializationFailed {
        message: format!("serialize index: {e}"),
    })?;
    rmp_serde::from_slice(&bytes).map_err(|e| GroxError::CacheSerializationFailed {
        message: format!("deserialize index: {e}"),
    })
}

/// Creates a cache header for the given crate source.
fn create_header(source: &CrateSource) -> CacheHeader {
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());

    let metadata = match source {
        CrateSource::CurrentCrate { manifest_path, .. } => {
            let manifest_dir = manifest_path.parent().unwrap_or(Path::new("."));
            let mtime = get_newest_source_mtime(manifest_dir);
            CacheMetadata::CurrentCrate {
                newest_source_mtime: mtime,
            }
        }
        CrateSource::Dependency { version, .. } => CacheMetadata::Dependency {
            package_version: version.clone(),
        },
        CrateSource::Stdlib { name } => CacheMetadata::StdLib {
            toolchain_version: name.clone(), // placeholder — real impl would use toolchain hash
        },
        CrateSource::External { version, .. } => CacheMetadata::External {
            crate_version: version.as_deref().unwrap_or("unknown").to_string(),
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
fn is_cache_valid(header: &CacheHeader, source: &CrateSource) -> bool {
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
                newest_source_mtime: cached_mtime,
            },
            CrateSource::CurrentCrate { manifest_path, .. },
        ) => {
            let manifest_dir = manifest_path.parent().unwrap_or(Path::new("."));
            let current_mtime = get_newest_source_mtime(manifest_dir);
            current_mtime <= *cached_mtime
        }
        (
            CacheMetadata::Dependency {
                package_version: cached_version,
            },
            CrateSource::Dependency { version, .. },
        ) => cached_version == version,
        (
            CacheMetadata::StdLib {
                toolchain_version: cached_toolchain,
            },
            CrateSource::Stdlib { name },
        ) => cached_toolchain == name, // placeholder comparison
        (
            CacheMetadata::External {
                crate_version: cached_version,
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

/// Scans `src/` for the newest `.rs` file modification time (UNIX epoch seconds).
pub(crate) fn get_newest_source_mtime(manifest_dir: &Path) -> u64 {
    let src_dir = manifest_dir.join("src");
    if !src_dir.exists() {
        return 0;
    }

    walk_for_newest_mtime(&src_dir)
}

/// Recursively walks a directory for the newest `.rs` file mtime.
fn walk_for_newest_mtime(dir: &Path) -> u64 {
    let mut newest: u64 = 0;

    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            newest = newest.max(walk_for_newest_mtime(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            if let Ok(meta) = fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    let mtime = modified
                        .duration_since(UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs());
                    newest = newest.max(mtime);
                }
            }
        }
    }

    newest
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DocIndex, IndexItem, ItemKind, SourceSpan};
    use std::fs::File;
    use std::io::Write;
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
        });
        index
    }

    /// Creates a `CrateSource::CurrentCrate` pointing at a temp directory.
    fn make_current_source(tmp: &Path) -> CrateSource {
        CrateSource::CurrentCrate {
            manifest_path: tmp.join("Cargo.toml"),
            name: "testcrate".to_string(),
            version: "1.0.0".to_string(),
        }
    }

    /// Creates a `CrateSource::Dependency` pointing at a temp directory.
    fn make_dep_source(tmp: &Path) -> CrateSource {
        CrateSource::Dependency {
            manifest_path: tmp.join("Cargo.toml"),
            name: "testcrate".to_string(),
            version: "1.0.0".to_string(),
        }
    }

    /// Sets up a temp directory with a `src/lib.rs` file.
    fn setup_src_dir(tmp: &Path) {
        let src_dir = tmp.join("src");
        fs::create_dir_all(&src_dir).unwrap();
        let mut f = File::create(src_dir.join("lib.rs")).unwrap();
        writeln!(f, "pub struct Foo;").unwrap();
    }

    // ---- Round-trip: save and load ----

    #[test]
    fn round_trip_save_load_preserves_index() {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.path();
        setup_src_dir(tmp_path);
        let source = make_current_source(tmp_path);

        let index = make_test_index();
        let cache_file = tmp_path.join("target/groxide/testcrate-1.0.0.groxide");

        save_to_cache(&cache_file, &index, &source);
        assert!(cache_file.exists(), "cache file should exist after save");

        let loaded = load_cached(&cache_file, &source);
        assert!(loaded.is_some(), "should load valid cache");
        let loaded = loaded.unwrap();

        assert_eq!(loaded.crate_name, index.crate_name);
        assert_eq!(loaded.crate_version, index.crate_version);
        assert_eq!(loaded.items.len(), index.items.len());
        assert_eq!(loaded.items[0], index.items[0]);
        assert_eq!(loaded.path_map, index.path_map);
        assert_eq!(loaded.name_map, index.name_map);
        assert_eq!(loaded.suffix_map, index.suffix_map);
        assert_eq!(loaded.trait_impls, index.trait_impls);
    }

    #[test]
    fn round_trip_dependency_save_load() {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.path();
        let source = make_dep_source(tmp_path);

        let index = make_test_index();
        let cache_file = tmp_path.join("target/groxide/testcrate-1.0.0.groxide");

        save_to_cache(&cache_file, &index, &source);
        let loaded = load_cached(&cache_file, &source);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().crate_name, "testcrate");
    }

    // ---- Cache path includes version ----

    #[test]
    fn cache_path_includes_version_for_current_crate() {
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::CurrentCrate {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "mycrate".to_string(),
            version: "2.3.4".to_string(),
        };
        let path = cache_path(&source, "").unwrap();
        assert!(
            path.to_str().unwrap().contains("mycrate-2.3.4.groxide"),
            "path should include version: {path:?}"
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
        let path = cache_path(&source, "").unwrap();
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
        let path = cache_path(&source, "").unwrap();
        assert!(
            path.to_str().unwrap().contains("tokio-1.40.0.groxide"),
            "path should include version: {path:?}"
        );
    }

    // ---- Feature suffix changes path ----

    #[test]
    fn cache_path_changes_with_feature_suffix() {
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::CurrentCrate {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "mycrate".to_string(),
            version: "1.0.0".to_string(),
        };

        let path_default = cache_path(&source, "").unwrap();
        let path_feat = cache_path(&source, "-feat_0a1b2c3d4e5f6a7b").unwrap();

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
        let source = CrateSource::CurrentCrate {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "mycrate".to_string(),
            version: "1.0.0".to_string(),
        };
        let path = cache_path(&source, "").unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(filename, "mycrate-1.0.0.groxide");
    }

    // ---- Atomic write: temp file cleaned up ----

    #[test]
    fn atomic_write_no_temp_file_left_behind() {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.path();
        setup_src_dir(tmp_path);
        let source = make_current_source(tmp_path);
        let index = make_test_index();

        let cache_dir = tmp_path.join("target/groxide");
        let cache_file = cache_dir.join("testcrate-1.0.0.groxide");

        save_to_cache(&cache_file, &index, &source);

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

    // ---- Stale cache detected by mtime ----

    #[test]
    fn stale_cache_detected_when_source_newer() {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.path();
        setup_src_dir(tmp_path);
        let source = make_current_source(tmp_path);
        let index = make_test_index();
        let cache_file = tmp_path.join("target/groxide/testcrate-1.0.0.groxide");

        // Save cache
        save_to_cache(&cache_file, &index, &source);
        assert!(
            load_cached(&cache_file, &source).is_some(),
            "fresh cache should load"
        );

        // Modify source file to make cache stale — write with a newer timestamp
        // Sleep briefly to ensure mtime changes
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let src_file = tmp_path.join("src/lib.rs");
        let mut f = File::create(&src_file).unwrap();
        writeln!(f, "pub struct Bar;").unwrap();

        let loaded = load_cached(&cache_file, &source);
        assert!(loaded.is_none(), "stale cache should not load");
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
        let cache_file = tmp_path.join("target/groxide/serde-1.0.0.groxide");

        save_to_cache(&cache_file, &index, &source_v1);
        assert!(
            load_cached(&cache_file, &source_v1).is_some(),
            "cache should be valid for v1"
        );
        assert!(
            load_cached(&cache_file, &source_v2).is_none(),
            "cache should be invalid for v2"
        );
    }

    // ---- load_cached returns None for missing file ----

    #[test]
    fn load_cached_returns_none_for_missing_file() {
        let tmp = TempDir::new().unwrap();
        let source = make_dep_source(tmp.path());
        let result = load_cached(&tmp.path().join("nonexistent.groxide"), &source);
        assert!(result.is_none());
    }

    // ---- load_cached returns None for corrupted file ----

    #[test]
    fn load_cached_returns_none_for_corrupted_file() {
        let tmp = TempDir::new().unwrap();
        let cache_file = tmp.path().join("corrupt.groxide");
        fs::write(&cache_file, b"not valid msgpack data").unwrap();
        let source = make_dep_source(tmp.path());
        let result = load_cached(&cache_file, &source);
        assert!(result.is_none());
    }

    // ---- cache_path for stdlib ----

    #[test]
    fn cache_path_stdlib_uses_global_cache() {
        let source = CrateSource::Stdlib {
            name: "std".to_string(),
        };
        let path = cache_path(&source, "").unwrap();
        let path_str = path.to_str().unwrap();
        assert!(
            path_str.contains("groxide/stdlib/"),
            "stdlib should use global cache: {path_str}"
        );
        assert!(
            path_str.contains("std.groxide"),
            "should contain crate name: {path_str}"
        );
    }

    // ---- cache_path for external ----

    #[test]
    fn cache_path_external_uses_global_cache() {
        let source = CrateSource::External {
            name: "tokio".to_string(),
            version: Some("1.40.0".to_string()),
        };
        let path = cache_path(&source, "").unwrap();
        let path_str = path.to_str().unwrap();
        assert!(
            path_str.contains("groxide/external/"),
            "external should use global cache: {path_str}"
        );
    }

    // ---- get_newest_source_mtime ----

    #[test]
    fn get_newest_source_mtime_returns_zero_when_no_src_dir() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(get_newest_source_mtime(tmp.path()), 0);
    }

    #[test]
    fn get_newest_source_mtime_finds_rs_files() {
        let tmp = TempDir::new().unwrap();
        setup_src_dir(tmp.path());
        let mtime = get_newest_source_mtime(tmp.path());
        assert!(mtime > 0, "should find mtime of src/lib.rs");
    }

    #[test]
    fn get_newest_source_mtime_finds_nested_rs_files() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("src/submod");
        fs::create_dir_all(&nested).unwrap();
        let mut f = File::create(nested.join("mod.rs")).unwrap();
        writeln!(f, "// nested").unwrap();

        let mtime = get_newest_source_mtime(tmp.path());
        assert!(mtime > 0, "should find nested .rs file");
    }

    // ---- source type mismatch ----

    #[test]
    fn load_cached_returns_none_for_source_type_mismatch() {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.path();
        setup_src_dir(tmp_path);

        let current_source = make_current_source(tmp_path);
        let dep_source = make_dep_source(tmp_path);

        let index = make_test_index();
        let cache_file = tmp_path.join("target/groxide/testcrate-1.0.0.groxide");

        // Save as CurrentCrate
        save_to_cache(&cache_file, &index, &current_source);

        // Try to load as Dependency — should fail due to metadata type mismatch
        let loaded = load_cached(&cache_file, &dep_source);
        assert!(loaded.is_none(), "source type mismatch should invalidate");
    }

    // ---- project vs global cache paths ----

    #[test]
    fn cache_path_current_crate_under_target() {
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::CurrentCrate {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "mylib".to_string(),
            version: "0.5.0".to_string(),
        };
        let path = cache_path(&source, "").unwrap();
        assert!(
            path.starts_with(tmp.path().join("target/groxide")),
            "should be under target/groxide: {path:?}"
        );
    }

    #[test]
    fn cache_path_dependency_under_target() {
        let tmp = TempDir::new().unwrap();
        let source = CrateSource::Dependency {
            manifest_path: tmp.path().join("Cargo.toml"),
            name: "serde".to_string(),
            version: "1.0.0".to_string(),
        };
        let path = cache_path(&source, "").unwrap();
        assert!(
            path.starts_with(tmp.path().join("target/groxide")),
            "should be under target/groxide: {path:?}"
        );
    }
}
