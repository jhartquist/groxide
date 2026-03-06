use std::io::Read;
use std::path::{Path, PathBuf};

use crate::cli::FeatureFlags;
use crate::docgen::generate_rustdoc_json_external;
use crate::error::{GroxError, Result};

/// Maximum download size: 500 MB.
const MAX_DOWNLOAD_BYTES: u64 = 500 * 1024 * 1024;

/// Connect timeout in seconds.
const CONNECT_TIMEOUT_SECS: u64 = 10;

/// Read timeout in seconds.
const READ_TIMEOUT_SECS: u64 = 30;

/// crates.io API response for crate info.
#[derive(Debug, serde::Deserialize)]
struct CratesIoResponse {
    #[serde(rename = "crate")]
    crate_info: CrateInfo,
    versions: Vec<VersionInfo>,
}

/// Top-level crate info from crates.io.
#[derive(Debug, serde::Deserialize)]
struct CrateInfo {
    /// Canonical crate name (hyphens preserved, as registered on crates.io).
    name: String,
    max_version: String,
}

/// Per-version info from crates.io.
#[derive(Debug, serde::Deserialize)]
struct VersionInfo {
    num: String,
    yanked: bool,
}

/// Fetches an external crate from crates.io, extracts it, and generates rustdoc JSON.
///
/// Returns `(json_path, canonical_name, resolved_version)` on success. The canonical
/// name is the crate name as registered on crates.io (hyphens preserved).
///
/// # Errors
///
/// Returns `GroxError::ExternalFetchFailed` on network errors, extraction failures,
/// or if the crate/version doesn't exist.
/// Returns `GroxError::CrateNotFound` if the crate doesn't exist on crates.io.
pub(crate) fn fetch_external_crate(
    name: &str,
    version_opt: Option<&str>,
    features: &FeatureFlags,
    private: bool,
) -> Result<(PathBuf, String, String)> {
    let (canonical_name, exact_version) = resolve_crate_name_and_version(name, version_opt)?;
    let cache_dir = external_cache_dir()?;
    let crate_dir = cache_dir.join(format!("{canonical_name}-{exact_version}"));
    let json_path = compute_json_path(&crate_dir, &canonical_name, features);

    if let Some(cached) = check_json_cache(&json_path, &canonical_name, &exact_version) {
        return Ok((cached, canonical_name, exact_version));
    }

    ensure_source_available(&canonical_name, &exact_version, &crate_dir)?;
    let generated_path = generate_rustdoc_json_external(
        &crate_dir,
        &canonical_name,
        &exact_version,
        features,
        private,
    )?;
    cache_feature_json(&canonical_name, features, &generated_path, &json_path)?;
    let final_path = select_output_path(&canonical_name, &json_path, &generated_path)?;

    Ok((final_path, canonical_name, exact_version))
}

/// Resolves the canonical crate name and exact version, querying crates.io.
///
/// Returns `(canonical_name, exact_version)`. The canonical name comes from the
/// crates.io API response, ensuring correct casing and hyphenation.
fn resolve_crate_name_and_version(
    name: &str,
    version_opt: Option<&str>,
) -> Result<(String, String)> {
    let response = query_crates_io(name)?;
    let canonical_name = response.crate_info.name.clone();
    let exact_version = match version_opt {
        Some(v) => resolve_version(&response, name, v)?,
        None => response.crate_info.max_version,
    };
    Ok((canonical_name, exact_version))
}

/// Returns the cached JSON path if it already exists, or `None` if a rebuild is needed.
fn check_json_cache(json_path: &Path, name: &str, version: &str) -> Option<PathBuf> {
    if json_path.exists() {
        eprintln!("[grox] Using cached {name} {version}");
        Some(json_path.to_path_buf())
    } else {
        None
    }
}

/// Downloads and extracts the crate source if not already present on disk.
fn ensure_source_available(name: &str, version: &str, crate_dir: &Path) -> Result<()> {
    if !crate_dir.join("Cargo.toml").exists() {
        eprintln!("[grox] Fetching {name} {version} from crates.io...");
        download_and_extract(name, version, crate_dir)?;
    }
    Ok(())
}

/// Copies generated JSON to a feature-suffixed path when non-default features are used.
fn cache_feature_json(
    name: &str,
    features: &FeatureFlags,
    generated_path: &Path,
    json_path: &Path,
) -> Result<()> {
    if features.is_default() || generated_path == json_path {
        return Ok(());
    }
    if let Some(parent) = json_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("failed to create directory: {e}"),
        })?;
    }
    std::fs::copy(generated_path, json_path).map_err(|e| GroxError::ExternalFetchFailed {
        name: name.to_string(),
        details: format!("failed to copy JSON for feature cache: {e}"),
    })?;
    Ok(())
}

/// Verifies that rustdoc JSON was generated and returns the best available path.
fn select_output_path(name: &str, json_path: &Path, generated_path: &Path) -> Result<PathBuf> {
    if json_path.exists() {
        return Ok(json_path.to_path_buf());
    }
    if generated_path.exists() {
        return Ok(generated_path.to_path_buf());
    }
    Err(GroxError::ExternalFetchFailed {
        name: name.to_string(),
        details: "rustdoc JSON was not generated".to_string(),
    })
}

/// Resolves a version string, handling exact, partial, and pre-release versions.
fn resolve_version(response: &CratesIoResponse, name: &str, version_input: &str) -> Result<String> {
    // Case 1: Complete semver
    if semver::Version::parse(version_input).is_ok() {
        return Ok(version_input.to_string());
    }

    // Case 2: Partial semver (e.g., "1.40" or "1")
    if is_partial_version(version_input) {
        let parts: Vec<&str> = version_input.split('.').collect();

        let mut matching: Vec<semver::Version> = response
            .versions
            .iter()
            .filter(|v| !v.yanked)
            .filter_map(|v| semver::Version::parse(&v.num).ok())
            .filter(|v| version_matches_partial(v, &parts))
            .collect();

        matching.sort();
        matching.reverse();

        if matching.is_empty() {
            return Err(GroxError::ExternalFetchFailed {
                name: name.to_string(),
                details: format!("no version matching '{version_input}' found"),
            });
        }

        return Ok(matching[0].to_string());
    }

    // Case 3: Other (pre-release, etc.) — use as-is
    Ok(version_input.to_string())
}

/// Queries crates.io API for crate information.
fn query_crates_io(name: &str) -> Result<CratesIoResponse> {
    let url = format!("https://crates.io/api/v1/crates/{name}");
    let agent = build_http_agent();

    let mut response = agent
        .get(&url)
        .call()
        .map_err(|e| GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("crates.io API error: {e}"),
        })?;

    if response.status() == 404 {
        return Err(GroxError::CrateNotFound {
            name: name.to_string(),
            suggestions: vec![],
        });
    }

    response
        .body_mut()
        .read_json::<CratesIoResponse>()
        .map_err(|e| GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("failed to parse crates.io response: {e}"),
        })
}

/// Builds an HTTP agent with appropriate timeouts and user-agent.
fn build_http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS)))
        .timeout_recv_body(Some(std::time::Duration::from_secs(READ_TIMEOUT_SECS)))
        .user_agent(format!("grox/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

/// Returns whether a version string is a partial semver (1 or 2 numeric components).
fn is_partial_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    (parts.len() == 1 || parts.len() == 2) && parts.iter().all(|p| p.parse::<u64>().is_ok())
}

/// Returns whether a full version matches a partial version pattern.
fn version_matches_partial(version: &semver::Version, parts: &[&str]) -> bool {
    match parts.len() {
        1 => parts[0]
            .parse::<u64>()
            .is_ok_and(|major| version.major == major),
        2 => {
            let major_ok = parts[0]
                .parse::<u64>()
                .is_ok_and(|major| version.major == major);
            let minor_ok = parts[1]
                .parse::<u64>()
                .is_ok_and(|minor| version.minor == minor);
            major_ok && minor_ok
        }
        _ => false,
    }
}

/// Downloads a crate tarball from crates.io and extracts it.
fn download_and_extract(name: &str, version: &str, target_dir: &Path) -> Result<()> {
    let url = format!("https://crates.io/api/v1/crates/{name}/{version}/download");
    let agent = build_http_agent();

    let mut response = agent
        .get(&url)
        .call()
        .map_err(|e| GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("download failed: {e}"),
        })?;

    // Read body with size limit
    let mut body = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_DOWNLOAD_BYTES)
        .read_to_end(&mut body)
        .map_err(|e| GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("download read failed: {e}"),
        })?;

    // String append instead of with_extension: versioned dirs like "serde-1.0.0"
    // would lose the last segment ("serde-1.0.tmp") with Path::with_extension.
    let temp_dir = PathBuf::from(format!("{}.tmp", target_dir.display()));

    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).map_err(|e| GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("failed to clean temp dir: {e}"),
        })?;
    }
    std::fs::create_dir_all(&temp_dir).map_err(|e| GroxError::ExternalFetchFailed {
        name: name.to_string(),
        details: format!("failed to create temp dir: {e}"),
    })?;

    // Extract tarball
    let result = extract_tarball(&body, name, version, &temp_dir);
    if let Err(e) = &result {
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("extraction failed: {e}"),
        });
    }

    // Atomic rename
    if target_dir.exists() {
        std::fs::remove_dir_all(target_dir).map_err(|e| GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("failed to remove old dir: {e}"),
        })?;
    }
    std::fs::rename(&temp_dir, target_dir).map_err(|e| {
        let _ = std::fs::remove_dir_all(&temp_dir);
        GroxError::ExternalFetchFailed {
            name: name.to_string(),
            details: format!("rename failed: {e}"),
        }
    })?;

    Ok(())
}

/// Extracts a gzipped tarball into the target directory with security checks.
fn extract_tarball(
    data: &[u8],
    name: &str,
    version: &str,
    temp_dir: &Path,
) -> std::result::Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(data);
    let mut archive = tar::Archive::new(decoder);
    let prefix = format!("{name}-{version}");

    let canonical_temp = temp_dir
        .canonicalize()
        .map_err(|e| format!("canonicalize temp dir: {e}"))?;

    for entry_result in archive
        .entries()
        .map_err(|e| format!("read tar entries: {e}"))?
    {
        let mut entry = entry_result.map_err(|e| format!("read tar entry: {e}"))?;
        let entry_type = entry.header().entry_type();

        if is_unsafe_entry_type(entry_type) {
            continue;
        }

        let raw_path = entry.path().map_err(|e| format!("entry path: {e}"))?;

        let Some(stripped) = strip_tar_prefix(&raw_path, &prefix) else {
            continue;
        };

        let target_path = temp_dir.join(&stripped);

        validate_target_path(&target_path, &canonical_temp, &raw_path)?;

        if entry_type.is_file() || entry_type.is_dir() {
            entry
                .unpack(&target_path)
                .map_err(|e| format!("unpack {}: {e}", stripped.display()))?;
        }
    }

    Ok(())
}

/// Returns whether a tar entry type should be skipped for security reasons.
///
/// Symlinks and hard links are skipped to prevent path-based attacks.
fn is_unsafe_entry_type(entry_type: tar::EntryType) -> bool {
    entry_type.is_symlink() || entry_type.is_hard_link()
}

/// Strips the expected top-level prefix from a tar entry path.
///
/// Returns `None` for entries outside the expected `{name}-{version}/` prefix.
fn strip_tar_prefix(raw_path: &Path, prefix: &str) -> Option<PathBuf> {
    raw_path.strip_prefix(prefix).ok().map(Path::to_path_buf)
}

/// Creates parent directories and validates the target path stays within the extraction root.
///
/// Returns an error if a path traversal attempt is detected.
fn validate_target_path(
    target_path: &Path,
    canonical_temp: &Path,
    raw_path: &Path,
) -> std::result::Result<(), String> {
    let Some(parent) = target_path.parent() else {
        return Ok(());
    };

    std::fs::create_dir_all(parent).map_err(|e| format!("create parent dir: {e}"))?;

    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("canonicalize parent: {e}"))?;

    let canonical_target = match target_path.file_name() {
        Some(file_name) => canonical_parent.join(file_name),
        None => canonical_parent,
    };

    if !canonical_target.starts_with(canonical_temp) {
        return Err(format!(
            "path traversal attempt detected: {}",
            raw_path.display()
        ));
    }

    Ok(())
}

/// Computes the expected JSON output path for an external crate.
///
/// For default features, the path is the standard rustdoc output path.
/// For non-default features, uses a feature-suffixed filename.
fn compute_json_path(crate_dir: &Path, name: &str, features: &FeatureFlags) -> PathBuf {
    let normalized = name.replace('-', "_");
    if features.is_default() {
        crate_dir
            .join("target")
            .join("doc")
            .join(format!("{normalized}.json"))
    } else {
        let suffix = features.cache_suffix();
        crate_dir
            .join("target")
            .join("doc")
            .join(format!("{normalized}{suffix}.json"))
    }
}

/// Returns the global cache directory for external crates.
fn external_cache_dir() -> Result<PathBuf> {
    dirs::cache_dir()
        .map(|d| d.join("groxide"))
        .ok_or_else(|| GroxError::ExternalFetchFailed {
            name: String::new(),
            details: "could not determine cache directory".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::write::GzEncoder;

    use super::*;

    /// Compresses raw tar data with gzip.
    fn gzip_compress(tar_data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(tar_data).expect("gzip write");
        encoder.finish().expect("gzip finish")
    }

    // ---- Version resolution helpers (offline tests) ----

    #[test]
    fn is_partial_version_returns_true_for_single_digit() {
        assert!(is_partial_version("1"));
    }

    #[test]
    fn is_partial_version_returns_true_for_major_minor() {
        assert!(is_partial_version("1.40"));
    }

    #[test]
    fn is_partial_version_returns_false_for_full_semver() {
        assert!(!is_partial_version("1.40.0"));
    }

    #[test]
    fn is_partial_version_returns_false_for_non_numeric() {
        assert!(!is_partial_version("abc"));
    }

    #[test]
    fn is_partial_version_returns_false_for_prerelease() {
        assert!(!is_partial_version("1.0.0-alpha"));
    }

    // ---- version_matches_partial ----

    #[test]
    fn version_matches_partial_matches_major_only() {
        let v = semver::Version::parse("1.40.0").expect("valid semver");
        assert!(version_matches_partial(&v, &["1"]));
        assert!(!version_matches_partial(&v, &["2"]));
    }

    #[test]
    fn version_matches_partial_matches_major_minor() {
        let v = semver::Version::parse("1.40.0").expect("valid semver");
        assert!(version_matches_partial(&v, &["1", "40"]));
        assert!(!version_matches_partial(&v, &["1", "39"]));
    }

    #[test]
    fn version_matches_partial_returns_false_for_empty() {
        let v = semver::Version::parse("1.40.0").expect("valid semver");
        let empty: &[&str] = &[];
        assert!(!version_matches_partial(&v, empty));
    }

    #[test]
    fn version_matches_partial_returns_false_for_three_parts() {
        let v = semver::Version::parse("1.40.0").expect("valid semver");
        assert!(!version_matches_partial(&v, &["1", "40", "0"]));
    }

    // ---- Path traversal protection ----

    #[test]
    fn extract_tarball_rejects_path_traversal() {
        // Build a tar archive with a raw header to bypass the tar crate's
        // built-in `..` validation in `set_path`.
        let malicious_path = b"crate-1.0.0/../../../tmp/evil_groxide_test";
        let body = b"evil!";

        let mut tar_bytes = Vec::new();

        // Build a 512-byte tar header manually
        let mut header_block = [0u8; 512];
        // name field: bytes 0..100
        header_block[..malicious_path.len()].copy_from_slice(malicious_path);
        // mode: bytes 100..108
        header_block[100..107].copy_from_slice(b"0000644");
        // uid: bytes 108..116
        header_block[108..115].copy_from_slice(b"0001000");
        // gid: bytes 116..124
        header_block[116..123].copy_from_slice(b"0001000");
        // size: bytes 124..136 (octal, 5 bytes)
        header_block[124..135].copy_from_slice(b"00000000005");
        // mtime: bytes 136..148
        header_block[136..147].copy_from_slice(b"00000000000");
        // typeflag: byte 156 ('0' = regular file)
        header_block[156] = b'0';

        // Compute checksum (sum of all bytes in header, treating chksum field as spaces)
        // chksum field: bytes 148..156
        header_block[148..156].copy_from_slice(b"        ");
        let cksum: u32 = header_block.iter().map(|&b| u32::from(b)).sum();
        let cksum_str = format!("{cksum:06o}\0 ");
        header_block[148..156].copy_from_slice(cksum_str.as_bytes());

        tar_bytes.extend_from_slice(&header_block);

        // Data block (padded to 512 bytes)
        let mut data_block = [0u8; 512];
        data_block[..body.len()].copy_from_slice(body);
        tar_bytes.extend_from_slice(&data_block);

        // End-of-archive marker (two zero blocks)
        tar_bytes.extend_from_slice(&[0u8; 1024]);

        let gz_data = gzip_compress(&tar_bytes);

        let tmp = tempfile::TempDir::new().expect("tempdir");
        let result = extract_tarball(&gz_data, "crate", "1.0.0", tmp.path());

        // After stripping "crate-1.0.0/", the path becomes "../../tmp/evil_groxide_test"
        // which should be caught by the traversal check or skipped.
        // The extraction should either fail with an error (path traversal, permission
        // denied, or canonicalization error) or succeed but NOT write outside the temp dir.
        match result {
            Err(_msg) => {
                // Any error is acceptable — the malicious path was blocked
            }
            Ok(()) => {
                // Verify no file was written outside temp dir
                assert!(
                    !PathBuf::from("/tmp/evil_groxide_test").exists(),
                    "file should not have been written outside temp dir"
                );
            }
        }
    }

    #[test]
    fn extract_tarball_skips_symlinks() {
        let mut builder = tar::Builder::new(Vec::new());

        let mut header = tar::Header::new_gnu();
        header.set_path("crate-1.0.0/src/lib.rs").expect("set path");
        header.set_size(13);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_cksum();

        builder
            .append(&header, b"fn main() {}\n" as &[u8])
            .expect("append file");

        let mut sym_header = tar::Header::new_gnu();
        sym_header.set_path("crate-1.0.0/link").expect("set path");
        sym_header.set_size(0);
        sym_header.set_entry_type(tar::EntryType::Symlink);
        sym_header
            .set_link_name("/etc/passwd")
            .expect("set link name");
        sym_header.set_mode(0o777);
        sym_header.set_cksum();

        builder
            .append(&sym_header, &[] as &[u8])
            .expect("append symlink");

        let tar_data = builder.into_inner().expect("finish tar");
        let gz_data = gzip_compress(&tar_data);

        let tmp = tempfile::TempDir::new().expect("tempdir");
        let result = extract_tarball(&gz_data, "crate", "1.0.0", tmp.path());
        assert!(result.is_ok(), "extraction should succeed: {result:?}");

        assert!(tmp.path().join("src/lib.rs").exists());
        assert!(!tmp.path().join("link").exists());
    }

    #[test]
    fn extract_tarball_extracts_regular_files() {
        let mut builder = tar::Builder::new(Vec::new());

        let mut header = tar::Header::new_gnu();
        header.set_path("crate-1.0.0/Cargo.toml").expect("set path");
        let content = b"[package]\nname = \"crate\"\n";
        header.set_size(content.len() as u64);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_mode(0o644);
        header.set_cksum();

        builder
            .append(&header, content as &[u8])
            .expect("append file");

        let tar_data = builder.into_inner().expect("finish tar");
        let gz_data = gzip_compress(&tar_data);

        let tmp = tempfile::TempDir::new().expect("tempdir");
        let result = extract_tarball(&gz_data, "crate", "1.0.0", tmp.path());
        assert!(result.is_ok(), "extraction should succeed: {result:?}");

        let cargo_toml = tmp.path().join("Cargo.toml");
        assert!(cargo_toml.exists(), "Cargo.toml should be extracted");

        let contents = std::fs::read_to_string(&cargo_toml).expect("read Cargo.toml");
        assert!(contents.contains("[package]"));
    }

    // ---- Cache directory structure ----

    #[test]
    fn external_cache_dir_returns_path_under_groxide() {
        let dir = external_cache_dir().expect("cache dir");
        assert!(
            dir.to_str().expect("valid utf8").contains("groxide"),
            "cache dir should contain 'groxide': {dir:?}"
        );
    }

    #[test]
    fn compute_json_path_uses_target_doc() {
        let crate_dir = Path::new("/cache/serde-1.0.210");
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let path = compute_json_path(crate_dir, "serde", &features);
        assert_eq!(
            path,
            PathBuf::from("/cache/serde-1.0.210/target/doc/serde.json")
        );
    }

    #[test]
    fn compute_json_path_normalizes_hyphens() {
        let crate_dir = Path::new("/cache/rmp-serde-1.3.0");
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let path = compute_json_path(crate_dir, "rmp-serde", &features);
        assert_eq!(
            path,
            PathBuf::from("/cache/rmp-serde-1.3.0/target/doc/rmp_serde.json")
        );
    }

    #[test]
    fn compute_json_path_includes_feature_suffix() {
        let crate_dir = Path::new("/cache/tokio-1.40.0");
        let features = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        let path = compute_json_path(crate_dir, "tokio", &features);
        let filename = path
            .file_name()
            .expect("has filename")
            .to_str()
            .expect("utf8");
        assert!(
            filename.starts_with("tokio-feat_"),
            "should include feature suffix: {filename}"
        );
        assert!(
            Path::new(filename)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("json")),
            "should have .json extension: {filename}"
        );
    }

    // ---- Network tests (behind #[ignore]) ----

    #[test]
    #[ignore = "requires network access"]
    fn version_resolution_for_known_crate() {
        let response = query_crates_io("itoa").expect("should query crates.io");
        assert!(!response.crate_info.max_version.is_empty());
        assert!(!response.versions.is_empty());

        let resolved = resolve_version(&response, "itoa", "1").expect("should resolve");
        let v = semver::Version::parse(&resolved).expect("valid semver");
        assert_eq!(v.major, 1);
    }

    #[test]
    #[ignore = "requires network access"]
    fn nonexistent_crate_returns_error() {
        let result = query_crates_io("this_crate_definitely_does_not_exist_xyz_123");
        assert!(result.is_err());
        match result.unwrap_err() {
            GroxError::CrateNotFound { name, .. } => {
                assert_eq!(name, "this_crate_definitely_does_not_exist_xyz_123");
            }
            other => panic!("expected CrateNotFound, got: {other:?}"),
        }
    }

    #[test]
    #[ignore = "requires network access"]
    fn latest_version_query_returns_valid_semver() {
        let response = query_crates_io("itoa").expect("should query crates.io");
        let version = &response.crate_info.max_version;
        assert!(
            semver::Version::parse(version).is_ok(),
            "should be valid semver: {version}"
        );
    }

    #[test]
    #[ignore = "requires network access"]
    fn partial_version_resolves_to_latest_matching() {
        let response = query_crates_io("itoa").expect("should query crates.io");
        let resolved = resolve_version(&response, "itoa", "1").expect("should resolve");
        let v = semver::Version::parse(&resolved).expect("valid semver");
        assert_eq!(v.major, 1);
    }

    #[test]
    #[ignore = "requires network access"]
    fn fetch_and_build_small_external_crate() {
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };

        let result = fetch_external_crate("itoa", None, &features, false);
        match result {
            Ok((path, canonical_name, version)) => {
                assert!(path.exists(), "JSON path should exist: {path:?}");
                assert_eq!(canonical_name, "itoa");
                assert!(!version.is_empty());
                eprintln!("Successfully fetched itoa {version} at {path:?}");
            }
            Err(e) => {
                if matches!(e, GroxError::NightlyNotAvailable) {
                    eprintln!("SKIP: nightly not available");
                    return;
                }
                panic!("fetch failed: {e}");
            }
        }
    }
}
