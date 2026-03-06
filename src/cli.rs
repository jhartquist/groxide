use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::error::{GroxError, Result};
use crate::types::ItemKind;

const HELP_EXAMPLES: &str = "\
EXAMPLES:
    grox serde::Deserialize          Struct docs with methods
grox tokio::sync::Mutex::lock    Full method documentation
    grox tokio -S \"spawn\"            Search across crate documentation
    grox -s tokio::sync::Mutex::new  View source code
    grox axum::Router                Auto-fetch external crate from crates.io
    grox std::collections::HashMap   Query standard library
    grox --json serde::Serialize     JSON output for programmatic use
    grox serde@1.0.210::Deserialize  Pin to specific version";

/// Query Rust crate documentation from the terminal
#[derive(Parser, Debug)]
#[command(name = "grox")]
#[command(version)]
#[command(about = "Query Rust crate documentation from the terminal", long_about = None)]
#[command(after_long_help = HELP_EXAMPLES)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// Rust path to query (e.g., `tokio::sync::Mutex`, `serde@1.0`)
    pub path: Option<String>,

    /// Show only item names (compact output)
    #[arg(short = 'b', long, conflicts_with_all = ["docs", "source", "search", "impls"])]
    pub brief: bool,

    /// Show full rendered documentation per item
    #[arg(short = 'd', long, conflicts_with_all = ["brief", "source", "search", "impls"])]
    pub docs: bool,

    /// Show source code instead of docs
    #[arg(short = 's', long, conflicts_with_all = ["brief", "docs", "impls"])]
    pub source: bool,

    /// Full-text search across documentation
    #[arg(short = 'S', long, conflicts_with_all = ["brief", "docs", "source", "impls"])]
    pub search: Option<String>,

    /// Filter by item kind
    #[arg(short = 'k', long, ignore_case = true)]
    pub kind: Option<KindFilter>,

    /// Include non-public items
    #[arg(short = 'p', long)]
    pub private: bool,

    /// JSON Lines output
    #[arg(short = 'j', long)]
    pub json: bool,

    /// Show trait implementations, optionally filtered by trait name
    #[arg(short = 'i', long, conflicts_with_all = ["brief", "docs", "source"],
          num_args = 0..=1, default_missing_value = "")]
    pub impls: Option<String>,

    /// List all public items recursively in a crate or module tree
    #[arg(short = 'r', long, conflicts_with_all = ["impls", "search"])]
    pub recursive: bool,

    /// Show the crate's README
    #[arg(long, conflicts_with_all = ["source", "search", "impls", "recursive"])]
    pub readme: bool,

    /// Path to Cargo.toml
    #[arg(long)]
    pub manifest_path: Option<PathBuf>,

    /// Comma-separated list of features to activate
    #[arg(long, value_delimiter = ',')]
    pub features: Vec<String>,

    /// Activate all available features
    #[arg(long)]
    pub all_features: bool,

    /// Do not activate the `default` feature
    #[arg(long)]
    pub no_default_features: bool,

    /// Clear the global documentation cache and exit
    #[arg(long)]
    pub clear_cache: bool,
}

/// Item kinds accepted by the --kind flag.
///
/// Parsed case-insensitively by clap's `ValueEnum`.
#[derive(Debug, Clone, Copy, ValueEnum)]
#[value(rename_all = "lowercase")]
pub enum KindFilter {
    /// Functions and methods
    Fn,
    /// Structs
    Struct,
    /// Enums
    Enum,
    /// Traits (includes `TraitAlias`)
    Trait,
    /// Type aliases (includes `AssocType`, `ForeignType`)
    Type,
    /// Constants (includes `AssocConst`)
    Const,
    /// Modules
    Mod,
    /// Macros (includes `ProcMacro`)
    Macro,
}

impl From<KindFilter> for ItemKind {
    fn from(filter: KindFilter) -> Self {
        match filter {
            KindFilter::Fn => Self::Function,
            KindFilter::Struct => Self::Struct,
            KindFilter::Enum => Self::Enum,
            KindFilter::Trait => Self::Trait,
            KindFilter::Type => Self::TypeAlias,
            KindFilter::Const => Self::Constant,
            KindFilter::Mod => Self::Module,
            KindFilter::Macro => Self::Macro,
        }
    }
}

/// Parsed query path from CLI input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct QueryPath {
    /// How the crate was specified.
    pub(crate) crate_spec: CrateSpec,
    /// Item segments after the crate name (e.g., `["sync", "Mutex"]`).
    pub(crate) item_segments: Vec<String>,
}

/// How the crate was specified in the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CrateSpec {
    /// No path provided — query the current crate root.
    CurrentCrate,
    /// First segment is a crate name (e.g., "tokio" from `tokio::sync::Mutex`).
    Named(String),
    /// Explicit version pin: "tokio@1.40.0" or "tokio@1.40".
    Versioned {
        /// The crate name.
        name: String,
        /// The version string.
        version: String,
    },
}

impl QueryPath {
    /// Parses a CLI input string into a `QueryPath`.
    ///
    /// Returns an error for invalid syntax like `@crate` or `crate@`.
    pub(crate) fn parse(input: &str) -> Result<Self> {
        let input = input.trim();

        if input.is_empty() {
            return Ok(Self {
                crate_spec: CrateSpec::CurrentCrate,
                item_segments: Vec::new(),
            });
        }

        // Check for leading @ — used to be supported, now an error
        if let Some(rest) = input.strip_prefix('@') {
            return Err(GroxError::InvalidQuery {
                message: format!("the @ prefix is no longer supported, use: grox {rest}"),
            });
        }

        // Split into segments on `::`
        let segments: Vec<&str> = input.split("::").collect();
        let first = segments[0];

        // Check for `@` in the first segment (versioned crate spec)
        if let Some(at_pos) = first.find('@') {
            let name = &first[..at_pos];
            let version = &first[at_pos + 1..];

            if version.is_empty() {
                return Err(GroxError::InvalidQuery {
                    message: "missing version after @".to_string(),
                });
            }

            let item_segments = segments[1..].iter().map(|s| (*s).to_string()).collect();
            return Ok(Self {
                crate_spec: CrateSpec::Versioned {
                    name: name.to_string(),
                    version: version.to_string(),
                },
                item_segments,
            });
        }

        // Named crate with optional item segments
        let item_segments = segments[1..].iter().map(|s| (*s).to_string()).collect();
        Ok(Self {
            crate_spec: CrateSpec::Named(first.to_string()),
            item_segments,
        })
    }
}

/// Feature flags for doc generation.
pub(crate) struct FeatureFlags {
    /// Activate all available features.
    pub(crate) all_features: bool,
    /// Do not activate the `default` feature.
    pub(crate) no_default_features: bool,
    /// Specific features to activate.
    pub(crate) features: Vec<String>,
}

impl FeatureFlags {
    /// Creates feature flags from CLI arguments.
    pub(crate) fn from_cli(cli: &Cli) -> Self {
        Self {
            all_features: cli.all_features,
            no_default_features: cli.no_default_features,
            features: cli.features.clone(),
        }
    }

    /// Returns true if no feature flags were explicitly set by the user.
    pub(crate) fn is_default(&self) -> bool {
        !self.all_features && !self.no_default_features && self.features.is_empty()
    }

    /// Computes a stable cache suffix.
    ///
    /// Returns "" for default flags, "-feat_<16-hex-hash>" for non-default.
    /// Uses DJB2 hash on a canonical string representation.
    pub(crate) fn cache_suffix(&self) -> String {
        if self.is_default() {
            return String::new();
        }

        let mut canonical = String::new();
        if self.all_features {
            canonical.push_str("all_features;");
        }
        if self.no_default_features {
            canonical.push_str("no_default;");
        }
        if !self.features.is_empty() {
            let mut sorted = self.features.clone();
            sorted.sort();
            canonical.push_str("features=");
            canonical.push_str(&sorted.join(","));
            canonical.push(';');
        }

        let hash = djb2_hash(&canonical);
        format!("-feat_{hash:016x}")
    }
}

/// DJB2 hash function for deterministic cache suffixes.
fn djb2_hash(s: &str) -> u64 {
    let mut hash: u64 = 5381;
    for byte in s.bytes() {
        hash = hash.wrapping_mul(33).wrapping_add(u64::from(byte));
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- QueryPath::parse ----

    #[test]
    fn parse_returns_current_crate_when_empty() {
        let qp = QueryPath::parse("").unwrap();
        assert_eq!(qp.crate_spec, CrateSpec::CurrentCrate);
        assert!(qp.item_segments.is_empty());
    }

    #[test]
    fn parse_returns_current_crate_when_whitespace() {
        let qp = QueryPath::parse("  ").unwrap();
        assert_eq!(qp.crate_spec, CrateSpec::CurrentCrate);
        assert!(qp.item_segments.is_empty());
    }

    #[test]
    fn parse_returns_named_with_items_for_path() {
        let qp = QueryPath::parse("tokio::sync::Mutex").unwrap();
        assert_eq!(qp.crate_spec, CrateSpec::Named("tokio".to_string()));
        assert_eq!(qp.item_segments, vec!["sync", "Mutex"]);
    }

    #[test]
    fn parse_returns_named_no_items_for_single_segment() {
        let qp = QueryPath::parse("serde").unwrap();
        assert_eq!(qp.crate_spec, CrateSpec::Named("serde".to_string()));
        assert!(qp.item_segments.is_empty());
    }

    #[test]
    fn parse_returns_versioned_with_items() {
        let qp = QueryPath::parse("tokio@1.40.0::sync").unwrap();
        assert_eq!(
            qp.crate_spec,
            CrateSpec::Versioned {
                name: "tokio".to_string(),
                version: "1.40.0".to_string()
            }
        );
        assert_eq!(qp.item_segments, vec!["sync"]);
    }

    #[test]
    fn parse_returns_versioned_no_items() {
        let qp = QueryPath::parse("serde@1.0.210").unwrap();
        assert_eq!(
            qp.crate_spec,
            CrateSpec::Versioned {
                name: "serde".to_string(),
                version: "1.0.210".to_string()
            }
        );
        assert!(qp.item_segments.is_empty());
    }

    #[test]
    fn parse_returns_error_for_at_prefix() {
        let err = QueryPath::parse("@serde").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("the @ prefix is no longer supported"),
            "unexpected error message: {msg}"
        );
        assert!(msg.contains("grox serde"), "should suggest: grox serde");
    }

    #[test]
    fn parse_returns_error_for_missing_version() {
        let err = QueryPath::parse("crate@").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("missing version after @"),
            "unexpected error message: {msg}"
        );
    }

    #[test]
    fn parse_handles_versioned_with_multiple_items() {
        let qp = QueryPath::parse("tokio@1.40.0::sync::Mutex").unwrap();
        assert_eq!(
            qp.crate_spec,
            CrateSpec::Versioned {
                name: "tokio".to_string(),
                version: "1.40.0".to_string()
            }
        );
        assert_eq!(qp.item_segments, vec!["sync", "Mutex"]);
    }

    #[test]
    fn parse_handles_prerelease_version() {
        let qp = QueryPath::parse("foo@1.0.0-alpha.1::Bar").unwrap();
        assert_eq!(
            qp.crate_spec,
            CrateSpec::Versioned {
                name: "foo".to_string(),
                version: "1.0.0-alpha.1".to_string()
            }
        );
        assert_eq!(qp.item_segments, vec!["Bar"]);
    }

    // ---- KindFilter → ItemKind conversion ----

    #[test]
    fn kind_filter_converts_fn_to_function() {
        assert_eq!(ItemKind::from(KindFilter::Fn), ItemKind::Function);
    }

    #[test]
    fn kind_filter_converts_struct_to_struct() {
        assert_eq!(ItemKind::from(KindFilter::Struct), ItemKind::Struct);
    }

    #[test]
    fn kind_filter_converts_enum_to_enum() {
        assert_eq!(ItemKind::from(KindFilter::Enum), ItemKind::Enum);
    }

    #[test]
    fn kind_filter_converts_trait_to_trait() {
        assert_eq!(ItemKind::from(KindFilter::Trait), ItemKind::Trait);
    }

    #[test]
    fn kind_filter_converts_type_to_type_alias() {
        assert_eq!(ItemKind::from(KindFilter::Type), ItemKind::TypeAlias);
    }

    #[test]
    fn kind_filter_converts_const_to_constant() {
        assert_eq!(ItemKind::from(KindFilter::Const), ItemKind::Constant);
    }

    #[test]
    fn kind_filter_converts_mod_to_module() {
        assert_eq!(ItemKind::from(KindFilter::Mod), ItemKind::Module);
    }

    #[test]
    fn kind_filter_converts_macro_to_macro() {
        assert_eq!(ItemKind::from(KindFilter::Macro), ItemKind::Macro);
    }

    // ---- FeatureFlags::cache_suffix ----

    #[test]
    fn cache_suffix_returns_empty_for_defaults() {
        let flags = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        assert_eq!(flags.cache_suffix(), "");
    }

    #[test]
    fn cache_suffix_returns_hash_for_all_features() {
        let flags = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        let suffix = flags.cache_suffix();
        assert!(suffix.starts_with("-feat_"), "suffix: {suffix}");
        assert_eq!(suffix.len(), 22); // "-feat_" (6) + 16 hex chars
    }

    #[test]
    fn cache_suffix_deterministic() {
        let flags1 = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: vec!["fs".to_string(), "net".to_string()],
        };
        let flags2 = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: vec!["fs".to_string(), "net".to_string()],
        };
        assert_eq!(flags1.cache_suffix(), flags2.cache_suffix());
    }

    #[test]
    fn cache_suffix_order_independent() {
        let flags1 = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: vec!["net".to_string(), "fs".to_string()],
        };
        let flags2 = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: vec!["fs".to_string(), "net".to_string()],
        };
        assert_eq!(
            flags1.cache_suffix(),
            flags2.cache_suffix(),
            "feature order should not affect suffix"
        );
    }

    #[test]
    fn cache_suffix_different_flags_produce_different_hashes() {
        let flags1 = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        let flags2 = FeatureFlags {
            all_features: false,
            no_default_features: true,
            features: Vec::new(),
        };
        assert_ne!(flags1.cache_suffix(), flags2.cache_suffix());
    }

    // ---- FeatureFlags::is_default ----

    #[test]
    fn is_default_returns_true_for_no_flags() {
        let flags = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        assert!(flags.is_default());
    }

    #[test]
    fn is_default_returns_false_for_all_features() {
        let flags = FeatureFlags {
            all_features: true,
            no_default_features: false,
            features: Vec::new(),
        };
        assert!(!flags.is_default());
    }

    #[test]
    fn is_default_returns_false_for_features() {
        let flags = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: vec!["fs".to_string()],
        };
        assert!(!flags.is_default());
    }

    // ---- Clap parsing (via try_parse_from) ----

    #[test]
    fn clap_parses_help_flag() {
        let result = Cli::try_parse_from(["grox", "--help"]);
        // --help exits with code 0, which clap represents as an error
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn clap_parses_version_flag() {
        let result = Cli::try_parse_from(["grox", "--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn clap_rejects_source_with_impls() {
        let result = Cli::try_parse_from(["grox", "--source", "--impls"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_rejects_search_with_source() {
        let result = Cli::try_parse_from(["grox", "--search", "foo", "--source"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_rejects_search_with_impls() {
        let result = Cli::try_parse_from(["grox", "--search", "foo", "--impls"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_rejects_readme_with_source() {
        let result = Cli::try_parse_from(["grox", "--readme", "--source"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_rejects_readme_with_search() {
        let result = Cli::try_parse_from(["grox", "--readme", "--search", "foo"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_rejects_brief_with_docs() {
        let result = Cli::try_parse_from(["grox", "--brief", "--docs"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_rejects_brief_with_source() {
        let result = Cli::try_parse_from(["grox", "--brief", "--source"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_rejects_docs_with_source() {
        let result = Cli::try_parse_from(["grox", "--docs", "--source"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_accepts_brief_alone() {
        let cli = Cli::try_parse_from(["grox", "--brief", "something"]).unwrap();
        assert!(cli.brief);
        assert!(!cli.docs);
        assert!(!cli.source);
    }

    #[test]
    fn clap_accepts_docs_alone() {
        let cli = Cli::try_parse_from(["grox", "--docs", "something"]).unwrap();
        assert!(cli.docs);
        assert!(!cli.brief);
        assert!(!cli.source);
    }

    #[test]
    fn clap_rejects_readme_with_impls() {
        let result = Cli::try_parse_from(["grox", "--readme", "--impls"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn clap_parses_impls_bare_flag() {
        let cli = Cli::try_parse_from(["grox", "something", "--impls"]).unwrap();
        assert_eq!(cli.impls, Some(String::new()));
        assert_eq!(cli.path, Some("something".to_string()));
    }

    #[test]
    fn clap_parses_impls_with_trait_filter() {
        let cli = Cli::try_parse_from(["grox", "something", "--impls", "Clone"]).unwrap();
        assert_eq!(cli.impls, Some("Clone".to_string()));
        assert_eq!(cli.path, Some("something".to_string()));
    }

    #[test]
    fn clap_rejects_invalid_kind_value() {
        let result = Cli::try_parse_from(["grox", "--kind", "widget"]);
        assert!(result.is_err());
    }

    #[test]
    fn clap_accepts_valid_kind_values() {
        for kind in [
            "fn", "struct", "enum", "trait", "type", "const", "mod", "macro",
        ] {
            let result = Cli::try_parse_from(["grox", "--kind", kind, "something"]);
            assert!(result.is_ok(), "kind '{kind}' should be accepted");
        }
    }

    #[test]
    fn clap_kind_is_case_insensitive() {
        let result = Cli::try_parse_from(["grox", "--kind", "FN", "something"]);
        assert!(result.is_ok());
    }

    #[test]
    fn clap_allows_json_with_search() {
        let result = Cli::try_parse_from(["grox", "--json", "--search", "foo"]);
        assert!(result.is_ok());
    }

    #[test]
    fn clap_parses_features_with_comma_delimiter() {
        let cli = Cli::try_parse_from(["grox", "--features", "fs,net", "tokio"]).unwrap();
        assert_eq!(cli.features, vec!["fs", "net"]);
    }

    #[test]
    fn clap_parses_manifest_path() {
        let cli =
            Cli::try_parse_from(["grox", "--manifest-path", "/tmp/Cargo.toml", "foo"]).unwrap();
        assert_eq!(cli.manifest_path, Some(PathBuf::from("/tmp/Cargo.toml")));
    }

    #[test]
    fn clap_parses_path_argument() {
        let cli = Cli::try_parse_from(["grox", "tokio::sync::Mutex"]).unwrap();
        assert_eq!(cli.path, Some("tokio::sync::Mutex".to_string()));
    }

    #[test]
    fn clap_parses_no_path_argument() {
        let cli = Cli::try_parse_from(["grox"]).unwrap();
        assert_eq!(cli.path, None);
    }

    // ---- Snapshot test for --help ----

    #[test]
    fn help_output_matches_snapshot() {
        let result = Cli::try_parse_from(["grox", "--help"]);
        let err = result.unwrap_err();
        let help_text = err.to_string();
        insta::assert_snapshot!(help_text);
    }
}
