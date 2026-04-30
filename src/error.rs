use std::collections::HashSet;
use std::fmt::Write;
use std::io;
use std::path::PathBuf;

/// Exit code for successful operations.
pub const EXIT_SUCCESS: i32 = 0;

/// Exit code when a crate or item is not found.
pub const EXIT_NOT_FOUND: i32 = 1;

/// Exit code for errors (I/O, config, toolchain, etc.).
pub const EXIT_ERROR: i32 = 2;

/// Formats suggestion list for error messages.
///
/// Deduplicates, caps at 5 entries, appends "... N more" if truncated.
/// Returns empty string if no suggestions.
pub(crate) fn format_suggestions(suggestions: &[String]) -> String {
    if suggestions.is_empty() {
        return String::new();
    }

    // Dedup while preserving insertion order
    let mut seen = HashSet::new();
    let unique: Vec<_> = suggestions.iter().filter(|s| seen.insert(*s)).collect();

    let total = unique.len();
    let display_count = total.min(5);
    let mut result = String::from("\n\nDid you mean:");
    for s in &unique[..display_count] {
        result.push_str("\n  ");
        result.push_str(s);
    }
    if total > 5 {
        let _ = write!(result, "\n  ... {} more", total - 5);
    }
    result
}

/// All error types for groxide. Each variant maps to an exit code.
#[derive(Debug, thiserror::Error)]
pub enum GroxError {
    /// No Cargo.toml found in the current directory or any parent.
    #[error("not in a Rust project\n\nRun grox in a Rust project directory, or specify --manifest-path.\nTo query external crates: grox <crate>\nTo query stdlib: grox std, grox core, grox alloc")]
    ManifestNotFound,

    /// cargo metadata invocation failed.
    #[error("failed to read cargo metadata\n\n{details}")]
    CargoMetadataFailed {
        /// Stderr or error message from cargo metadata.
        details: String,
    },

    /// Crate name not found among dependencies or known crates.
    #[error("crate '{name}' not found{}", format_suggestions(suggestions))]
    CrateNotFound {
        /// The crate name that was queried.
        name: String,
        /// Near-match suggestions.
        suggestions: Vec<String>,
    },

    /// Nightly Rust toolchain is required but not installed.
    #[error("nightly toolchain required\n\nRun: rustup toolchain install nightly")]
    NightlyNotAvailable,

    /// No item matched the query path.
    #[error("no item matching \"{query}\"{}{}", format_crate_ctx(crate_name.as_deref()), format_suggestions(suggestions))]
    ItemNotFound {
        /// The query string that didn't match.
        query: String,
        /// The crate being searched, if known.
        crate_name: Option<String>,
        /// Near-match suggestions.
        suggestions: Vec<String>,
    },

    /// `cargo rustdoc` failed.
    #[error("rustdoc generation failed\n\n{stderr}")]
    RustdocFailed {
        /// Stderr output from rustdoc.
        stderr: String,
    },

    /// The current crate has no library target — only a binary or example.
    #[error("'{name}' has no library target — grox indexes library APIs only.\n\nIf this crate has dependencies you want to query, try `grox <dep>`.")]
    NoLibraryTarget {
        /// The crate name.
        name: String,
    },

    /// rust-src component not installed.
    #[error("standard library source not available\n\nRun: rustup component add rust-src")]
    StdLibSourceMissing,

    /// Failed to fetch crate from crates.io.
    #[error("failed to fetch '{name}' from crates.io\n\n{details}")]
    ExternalFetchFailed {
        /// The crate name.
        name: String,
        /// Error details.
        details: String,
    },

    /// Invalid query syntax.
    #[error("{message}")]
    InvalidQuery {
        /// Human-readable description of the syntax error.
        message: String,
    },

    /// Failed to read a rustdoc JSON file from disk.
    #[error("failed to read {}: {source}", path.display())]
    JsonReadFailed {
        /// Path to the JSON file.
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },

    /// Rustdoc JSON parsed but had unexpected structure.
    #[error("failed to parse rustdoc JSON: {details}")]
    JsonParseFailed {
        /// Description of what went wrong.
        details: String,
    },

    /// Cache serialization or deserialization failed.
    #[error("cache error: {message}")]
    CacheSerializationFailed {
        /// What went wrong.
        message: String,
    },

    /// Generic I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// Formats the crate context for `ItemNotFound` error messages.
fn format_crate_ctx(crate_name: Option<&str>) -> String {
    match crate_name {
        Some(name) => format!(" in crate '{name}'"),
        None => String::new(),
    }
}

impl GroxError {
    /// Returns the process exit code for this error variant.
    #[must_use]
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::CrateNotFound { .. } | Self::ItemNotFound { .. } => EXIT_NOT_FOUND,
            Self::ManifestNotFound
            | Self::CargoMetadataFailed { .. }
            | Self::NightlyNotAvailable
            | Self::RustdocFailed { .. }
            | Self::NoLibraryTarget { .. }
            | Self::StdLibSourceMissing
            | Self::ExternalFetchFailed { .. }
            | Self::InvalidQuery { .. }
            | Self::JsonReadFailed { .. }
            | Self::JsonParseFailed { .. }
            | Self::CacheSerializationFailed { .. }
            | Self::Io(_) => EXIT_ERROR,
        }
    }
}

/// Convenience result alias for groxide operations.
pub type Result<T> = std::result::Result<T, GroxError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_manifest_not_found_contains_expected_text() {
        let err = GroxError::ManifestNotFound;
        let msg = err.to_string();
        assert!(msg.contains("not in a Rust project"));
        assert!(msg.contains("--manifest-path"));
    }

    #[test]
    fn error_display_cargo_metadata_failed_contains_details() {
        let err = GroxError::CargoMetadataFailed {
            details: "could not find Cargo.toml".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("failed to read cargo metadata"));
        assert!(msg.contains("could not find Cargo.toml"));
    }

    #[test]
    fn error_display_crate_not_found_with_suggestions() {
        let err = GroxError::CrateNotFound {
            name: "tokoi".to_string(),
            suggestions: vec!["tokio".to_string()],
        };
        let msg = err.to_string();
        assert!(msg.contains("crate 'tokoi' not found"));
        assert!(msg.contains("Did you mean:"));
        assert!(msg.contains("tokio"));
    }

    #[test]
    fn error_display_crate_not_found_no_suggestions() {
        let err = GroxError::CrateNotFound {
            name: "zzz".to_string(),
            suggestions: vec![],
        };
        let msg = err.to_string();
        assert!(msg.contains("crate 'zzz' not found"));
        assert!(!msg.contains("Did you mean"));
    }

    #[test]
    fn error_display_item_not_found_with_crate_context() {
        let err = GroxError::ItemNotFound {
            query: "Mutx".to_string(),
            crate_name: Some("tokio".to_string()),
            suggestions: vec!["Mutex".to_string()],
        };
        let msg = err.to_string();
        assert!(msg.contains("no item matching \"Mutx\""));
        assert!(msg.contains("in crate 'tokio'"));
        assert!(msg.contains("Did you mean:"));
        assert!(msg.contains("Mutex"));
    }

    #[test]
    fn error_display_item_not_found_without_crate_context() {
        let err = GroxError::ItemNotFound {
            query: "Foo".to_string(),
            crate_name: None,
            suggestions: vec![],
        };
        let msg = err.to_string();
        assert!(msg.contains("no item matching \"Foo\""));
        assert!(!msg.contains("in crate"));
    }

    #[test]
    fn error_display_nightly_not_available() {
        let err = GroxError::NightlyNotAvailable;
        let msg = err.to_string();
        assert!(msg.contains("nightly toolchain required"));
        assert!(msg.contains("rustup toolchain install nightly"));
    }

    #[test]
    fn error_display_rustdoc_failed() {
        let err = GroxError::RustdocFailed {
            stderr: "error[E0001]: some error".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("rustdoc generation failed"));
        assert!(msg.contains("error[E0001]"));
    }

    #[test]
    fn error_display_stdlib_source_missing() {
        let err = GroxError::StdLibSourceMissing;
        let msg = err.to_string();
        assert!(msg.contains("standard library source not available"));
        assert!(msg.contains("rustup component add rust-src"));
    }

    #[test]
    fn error_display_invalid_query() {
        let err = GroxError::InvalidQuery {
            message: "missing version after @".to_string(),
        };
        assert_eq!(err.to_string(), "missing version after @");
    }

    #[test]
    fn error_display_json_read_failed() {
        let err = GroxError::JsonReadFailed {
            path: PathBuf::from("/tmp/test.json"),
            source: io::Error::new(io::ErrorKind::NotFound, "file not found"),
        };
        let msg = err.to_string();
        assert!(msg.contains("failed to read /tmp/test.json"));
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn exit_code_crate_not_found_returns_1() {
        let err = GroxError::CrateNotFound {
            name: "x".to_string(),
            suggestions: vec![],
        };
        assert_eq!(err.exit_code(), EXIT_NOT_FOUND);
    }

    #[test]
    fn exit_code_item_not_found_returns_1() {
        let err = GroxError::ItemNotFound {
            query: "x".to_string(),
            crate_name: None,
            suggestions: vec![],
        };
        assert_eq!(err.exit_code(), EXIT_NOT_FOUND);
    }

    #[test]
    fn exit_code_manifest_not_found_returns_2() {
        assert_eq!(GroxError::ManifestNotFound.exit_code(), EXIT_ERROR);
    }

    #[test]
    fn exit_code_nightly_not_available_returns_2() {
        assert_eq!(GroxError::NightlyNotAvailable.exit_code(), EXIT_ERROR);
    }

    #[test]
    fn exit_code_io_error_returns_2() {
        let err = GroxError::Io(io::Error::other("boom"));
        assert_eq!(err.exit_code(), EXIT_ERROR);
    }

    #[test]
    fn exit_code_rustdoc_failed_returns_2() {
        let err = GroxError::RustdocFailed {
            stderr: String::new(),
        };
        assert_eq!(err.exit_code(), EXIT_ERROR);
    }

    #[test]
    fn format_suggestions_empty_returns_empty_string() {
        assert_eq!(format_suggestions(&[]), "");
    }

    #[test]
    fn format_suggestions_deduplicates() {
        let suggestions = vec!["a".to_string(), "b".to_string(), "a".to_string()];
        let result = format_suggestions(&suggestions);
        // Should contain "a" only once
        let count = result.matches("\n  a").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn format_suggestions_caps_at_5_with_ellipsis() {
        let suggestions: Vec<String> = (0..8).map(|i| format!("item{i}")).collect();
        let result = format_suggestions(&suggestions);
        assert!(result.contains("Did you mean:"));
        assert!(result.contains("item0"));
        assert!(result.contains("item4"));
        assert!(!result.contains("item5"));
        assert!(result.contains("... 3 more"));
    }

    #[test]
    fn format_suggestions_exactly_5_no_ellipsis() {
        let suggestions: Vec<String> = (0..5).map(|i| format!("item{i}")).collect();
        let result = format_suggestions(&suggestions);
        assert!(result.contains("item4"));
        assert!(!result.contains("..."));
    }

    #[test]
    fn format_suggestions_single_item() {
        let suggestions = vec!["Mutex".to_string()];
        let result = format_suggestions(&suggestions);
        assert!(result.contains("Did you mean:"));
        assert!(result.contains("Mutex"));
    }
}
