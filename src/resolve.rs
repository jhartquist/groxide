use std::path::{Path, PathBuf};

use cargo_metadata::{Metadata, MetadataCommand, Package, PackageId};

use crate::cli::CrateSpec;
use crate::error::{GroxError, Result};
use crate::types::is_stdlib_crate;

/// Where a crate's source lives — determines how rustdoc JSON is generated.
#[derive(Debug, Clone)]
pub(crate) enum CrateSource {
    /// The current project crate (from Cargo.toml in CWD or --manifest-path).
    CurrentCrate {
        /// Absolute path to the crate's Cargo.toml.
        manifest_path: PathBuf,
        /// Package name (hyphens preserved, as declared in Cargo.toml).
        name: String,
        /// Crate version string.
        version: String,
    },
    /// A dependency (direct, workspace, or transitive).
    Dependency {
        /// Absolute path to the dependency's Cargo.toml.
        manifest_path: PathBuf,
        /// Package name (hyphens preserved, as declared in Cargo.toml).
        name: String,
        /// Crate version string.
        version: String,
    },
    /// A standard library crate (std, core, alloc).
    Stdlib {
        /// Crate name ("std", "core", or "alloc").
        name: String,
    },
    /// An external crate to fetch from crates.io.
    External {
        /// Crate name.
        name: String,
        /// Optional pinned version.
        version: Option<String>,
    },
}

impl CrateSource {
    /// Returns the package name (hyphens preserved).
    pub(crate) fn name(&self) -> &str {
        match self {
            Self::CurrentCrate { name, .. }
            | Self::Dependency { name, .. }
            | Self::Stdlib { name }
            | Self::External { name, .. } => name,
        }
    }

    /// Returns the version string, if known.
    pub(crate) fn version(&self) -> Option<&str> {
        match self {
            Self::CurrentCrate { version, .. } | Self::Dependency { version, .. } => Some(version),
            Self::External { version, .. } => version.as_deref(),
            Self::Stdlib { .. } => None,
        }
    }
}

/// Project context discovered from Cargo.toml and cargo metadata.
///
/// Holds the full dependency graph and workspace info needed for crate resolution.
pub(crate) struct ProjectContext {
    /// Full cargo metadata output.
    metadata: Metadata,
    /// The current (root) package ID.
    current_package_id: PackageId,
}

impl ProjectContext {
    /// Discovers the project context from a Cargo.toml.
    ///
    /// Two modes:
    /// - **Explicit path**: If `manifest_path_override` is `Some`, use that `Cargo.toml` directly.
    /// - **Auto-discovery**: Walk up from CWD checking each directory for `Cargo.toml`.
    ///
    /// After locating the manifest, invokes `cargo metadata` for the full dependency graph.
    pub(crate) fn discover(manifest_path_override: Option<&Path>) -> Result<Self> {
        let manifest_path = match manifest_path_override {
            Some(path) => path.to_path_buf(),
            None => find_cargo_toml()?,
        };

        let metadata = MetadataCommand::new()
            .manifest_path(&manifest_path)
            .exec()
            .map_err(|e| GroxError::CargoMetadataFailed {
                details: e.to_string(),
            })?;

        let current_package_id = determine_current_package(&metadata)?;

        Ok(Self {
            metadata,
            current_package_id,
        })
    }

    /// Returns the manifest path for the current package.
    pub(crate) fn current_manifest_path(&self) -> PathBuf {
        self.metadata[&self.current_package_id]
            .manifest_path
            .clone()
            .into_std_path_buf()
    }

    /// Returns true if the workspace has no root package (virtual manifest).
    pub(crate) fn is_virtual_workspace(&self) -> bool {
        self.metadata
            .resolve
            .as_ref()
            .and_then(|r| r.root.as_ref())
            .is_none()
    }

    /// Returns all workspace member packages, sorted by name.
    pub(crate) fn workspace_member_packages(&self) -> Vec<&Package> {
        let mut members: Vec<_> = self
            .metadata
            .workspace_members
            .iter()
            .map(|id| &self.metadata[id])
            .collect();
        members.sort_by_key(|p| &p.name);
        members
    }

    /// Resolves a `CrateSpec` to a `CrateSource`.
    ///
    /// Follows the resolution chain: current crate -> direct deps -> workspace members
    /// -> transitive deps -> stdlib -> external (auto-fetch).
    pub(crate) fn resolve_crate(&self, spec: &CrateSpec) -> CrateSource {
        match spec {
            CrateSpec::CurrentCrate => {
                let pkg = &self.metadata[&self.current_package_id];
                CrateSource::CurrentCrate {
                    manifest_path: pkg.manifest_path.clone().into_std_path_buf(),
                    name: pkg.name.to_string(),
                    version: pkg.version.to_string(),
                }
            }
            CrateSpec::Versioned { name, version } => CrateSource::External {
                name: name.clone(),
                version: Some(version.clone()),
            },
            CrateSpec::Named(name) => self.resolve_named(name),
        }
    }

    /// Resolves a named crate through the full resolution chain.
    fn resolve_named(&self, query: &str) -> CrateSource {
        // Step 1: Current crate name match
        let current_pkg = &self.metadata[&self.current_package_id];
        if crate_names_match(query, &current_pkg.name) {
            return CrateSource::CurrentCrate {
                manifest_path: current_pkg.manifest_path.clone().into_std_path_buf(),
                name: current_pkg.name.to_string(),
                version: current_pkg.version.to_string(),
            };
        }

        // Step 2: Direct dependencies (via resolve graph, handles renames)
        if let Some(source) = self.find_in_direct_deps(query) {
            return source;
        }

        // Step 3: Workspace members
        if let Some(source) = self.find_in_workspace(query) {
            return source;
        }

        // Step 4: Transitive dependencies (all packages in metadata)
        if let Some(source) = self.find_in_all_packages(query) {
            return source;
        }

        // Step 5: Standard library
        if is_stdlib_crate(query) {
            return CrateSource::Stdlib {
                name: query.to_string(),
            };
        }

        // Step 6: Auto-fetch from crates.io
        CrateSource::External {
            name: query.to_string(),
            version: None,
        }
    }

    /// Searches direct dependencies of the current crate via the resolve graph.
    ///
    /// Uses `NodeDep::name` which handles renamed dependencies.
    fn find_in_direct_deps(&self, query: &str) -> Option<CrateSource> {
        let resolve = self.metadata.resolve.as_ref()?;
        let root_node = resolve
            .nodes
            .iter()
            .find(|n| n.id == self.current_package_id)?;

        for dep in &root_node.deps {
            // dep.name is the rename (or original name if not renamed)
            if crate_names_match(query, &dep.name) {
                let pkg = &self.metadata[&dep.pkg];
                return Some(CrateSource::Dependency {
                    manifest_path: pkg.manifest_path.clone().into_std_path_buf(),
                    name: pkg.name.to_string(),
                    version: pkg.version.to_string(),
                });
            }
        }
        None
    }

    /// Searches workspace members by crate name.
    fn find_in_workspace(&self, query: &str) -> Option<CrateSource> {
        for member_id in &self.metadata.workspace_members {
            if *member_id == self.current_package_id {
                continue; // Already checked in step 1
            }
            let pkg = &self.metadata[member_id];
            if crate_names_match(query, &pkg.name) {
                return Some(CrateSource::Dependency {
                    manifest_path: pkg.manifest_path.clone().into_std_path_buf(),
                    name: pkg.name.to_string(),
                    version: pkg.version.to_string(),
                });
            }
        }
        None
    }

    /// Searches all packages in metadata (transitive dependencies).
    fn find_in_all_packages(&self, query: &str) -> Option<CrateSource> {
        for pkg in &self.metadata.packages {
            if pkg.id == self.current_package_id {
                continue; // Already checked
            }
            if crate_names_match(query, &pkg.name) {
                return Some(CrateSource::Dependency {
                    manifest_path: pkg.manifest_path.clone().into_std_path_buf(),
                    name: pkg.name.to_string(),
                    version: pkg.version.to_string(),
                });
            }
        }
        None
    }
}

/// Walks up from CWD to find `Cargo.toml`.
fn find_cargo_toml() -> Result<PathBuf> {
    let cwd = std::env::current_dir().map_err(|_| GroxError::ManifestNotFound)?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists() {
            return Ok(candidate);
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Err(GroxError::ManifestNotFound),
        }
    }
}

/// Determines the current package from metadata using the three-tier fallback.
///
/// 1. Root package (non-virtual workspace)
/// 2. Closest workspace member to CWD
/// 3. First workspace member
fn determine_current_package(metadata: &Metadata) -> Result<PackageId> {
    // Tier 1: root package
    if let Some(resolve) = &metadata.resolve {
        if let Some(root) = &resolve.root {
            return Ok(root.clone());
        }
    }

    // Tier 2: closest workspace member to CWD
    let cwd = std::env::current_dir().ok();
    if let Some(cwd) = &cwd {
        let mut best: Option<(&PackageId, usize)> = None;
        for member_id in &metadata.workspace_members {
            let pkg = &metadata[member_id];
            let pkg_dir = pkg.manifest_path.parent().map(Path::new);
            if let Some(pkg_dir) = pkg_dir {
                let distance = path_distance(cwd, pkg_dir);
                if best.is_none() || distance < best.expect("invariant: checked is_none").1 {
                    best = Some((member_id, distance));
                }
            }
        }
        if let Some((id, _)) = best {
            return Ok(id.clone());
        }
    }

    // Tier 3: first workspace member
    metadata
        .workspace_members
        .first()
        .cloned()
        .ok_or(GroxError::ManifestNotFound)
}

/// Computes the "distance" between two paths as number of differing components.
fn path_distance(a: &Path, b: &Path) -> usize {
    let a_components: Vec<_> = a.components().collect();
    let b_components: Vec<_> = b.components().collect();

    let common = a_components
        .iter()
        .zip(b_components.iter())
        .take_while(|(x, y)| x == y)
        .count();

    (a_components.len() - common) + (b_components.len() - common)
}

/// Returns whether two crate names match with hyphen/underscore normalization.
fn crate_names_match(query: &str, package_name: &str) -> bool {
    query == package_name
        || normalize_crate_name(query) == package_name
        || query == normalize_crate_name(package_name)
        || normalize_crate_name(query) == normalize_crate_name(package_name)
}

/// Normalizes a crate name: replaces hyphens with underscores.
pub(crate) fn normalize_crate_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Resolves a `CrateSpec` without a project context.
///
/// Only stdlib and external (auto-fetch) are available when there's no project.
pub(crate) fn resolve_crate_without_context(spec: &CrateSpec) -> Result<CrateSource> {
    match spec {
        CrateSpec::CurrentCrate => Err(GroxError::ManifestNotFound),
        CrateSpec::Versioned { name, version } => Ok(CrateSource::External {
            name: name.clone(),
            version: Some(version.clone()),
        }),
        CrateSpec::Named(name) => {
            if is_stdlib_crate(name) {
                Ok(CrateSource::Stdlib { name: name.clone() })
            } else {
                Ok(CrateSource::External {
                    name: name.clone(),
                    version: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- crate_names_match ----

    #[test]
    fn crate_names_match_returns_true_for_exact() {
        assert!(crate_names_match("serde", "serde"));
    }

    #[test]
    fn crate_names_match_returns_true_for_hyphen_to_underscore() {
        assert!(crate_names_match("serde-json", "serde_json"));
    }

    #[test]
    fn crate_names_match_returns_true_for_underscore_to_hyphen() {
        assert!(crate_names_match("serde_json", "serde-json"));
    }

    #[test]
    fn crate_names_match_returns_false_for_different() {
        assert!(!crate_names_match("serde", "tokio"));
    }

    // ---- normalize_crate_name ----

    #[test]
    fn normalize_replaces_hyphens_with_underscores() {
        assert_eq!(normalize_crate_name("serde-json"), "serde_json");
    }

    #[test]
    fn normalize_leaves_underscores_unchanged() {
        assert_eq!(normalize_crate_name("serde_json"), "serde_json");
    }

    #[test]
    fn normalize_no_change_for_simple_name() {
        assert_eq!(normalize_crate_name("serde"), "serde");
    }

    // ---- path_distance ----

    #[test]
    fn path_distance_returns_zero_for_same_path() {
        let p = Path::new("/a/b/c");
        assert_eq!(path_distance(p, p), 0);
    }

    #[test]
    fn path_distance_returns_correct_for_parent_child() {
        let parent = Path::new("/a/b");
        let child = Path::new("/a/b/c");
        assert_eq!(path_distance(parent, child), 1);
        assert_eq!(path_distance(child, parent), 1);
    }

    #[test]
    fn path_distance_returns_correct_for_siblings() {
        let a = Path::new("/a/b/c");
        let b = Path::new("/a/b/d");
        assert_eq!(path_distance(a, b), 2);
    }

    // ---- ProjectContext::discover ----

    #[test]
    fn discover_finds_project_in_groxide_repo() {
        // Running from within the groxide project, discovery should succeed
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::CurrentCrate);
        assert_eq!(source.name(), "groxide");
    }

    #[test]
    fn discover_uses_explicit_manifest_path() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let ctx = ProjectContext::discover(Some(&manifest)).expect("should use explicit path");
        let source = ctx.resolve_crate(&CrateSpec::CurrentCrate);
        assert_eq!(source.name(), "groxide");
    }

    #[test]
    fn discover_returns_manifest_path() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let path = ctx.current_manifest_path();
        assert!(path.ends_with("Cargo.toml"));
        assert!(path.exists());
    }

    // ---- resolve_crate with known dependencies ----

    #[test]
    fn resolve_crate_returns_current_crate_for_current_spec() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::CurrentCrate);
        assert_eq!(source.name(), "groxide");
        assert!(matches!(source, CrateSource::CurrentCrate { .. }));
    }

    #[test]
    fn resolve_crate_returns_current_when_name_matches() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::Named("groxide".to_string()));
        assert!(
            matches!(source, CrateSource::CurrentCrate { .. }),
            "expected CurrentCrate, got {source:?}"
        );
    }

    #[test]
    fn resolve_crate_finds_direct_dependency() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::Named("clap".to_string()));
        assert_eq!(source.name(), "clap");
        assert!(matches!(source, CrateSource::Dependency { .. }));
    }

    #[test]
    fn resolve_crate_finds_dependency_with_underscore() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::Named("cargo_metadata".to_string()));
        assert_eq!(source.name(), "cargo_metadata");
        assert!(matches!(source, CrateSource::Dependency { .. }));
    }

    // ---- Hyphen normalization in resolution ----

    #[test]
    fn resolve_crate_preserves_hyphens_in_dep_lookup() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        // rmp-serde is a hyphenated dependency — package name preserved
        let source = ctx.resolve_crate(&CrateSpec::Named("rmp-serde".to_string()));
        assert_eq!(source.name(), "rmp-serde");
    }

    #[test]
    fn resolve_crate_finds_transitive_dependency() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        // `unicode-ident` is a transitive dep (via syn -> proc-macro2 -> unicode-ident)
        let source = ctx.resolve_crate(&CrateSpec::Named("unicode-ident".to_string()));
        assert!(matches!(source, CrateSource::Dependency { .. }));
    }

    // ---- stdlib recognition ----

    #[test]
    fn resolve_crate_returns_stdlib_for_std() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::Named("std".to_string()));
        assert!(matches!(source, CrateSource::Stdlib { .. }));
        assert_eq!(source.name(), "std");
    }

    #[test]
    fn resolve_crate_returns_stdlib_for_core() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::Named("core".to_string()));
        assert!(matches!(source, CrateSource::Stdlib { .. }));
    }

    #[test]
    fn resolve_crate_returns_stdlib_for_alloc() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::Named("alloc".to_string()));
        assert!(matches!(source, CrateSource::Stdlib { .. }));
    }

    // ---- Unknown crate returns External (auto-fetch) ----

    #[test]
    fn resolve_crate_returns_external_for_unknown() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::Named("nonexistent_crate_xyz".to_string()));
        assert!(
            matches!(source, CrateSource::External { version: None, .. }),
            "expected External, got {source:?}"
        );
        assert_eq!(source.name(), "nonexistent_crate_xyz");
    }

    // ---- Versioned spec goes directly to External ----

    #[test]
    fn resolve_crate_returns_external_for_versioned() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let source = ctx.resolve_crate(&CrateSpec::Versioned {
            name: "serde".to_string(),
            version: "1.0.210".to_string(),
        });
        assert!(matches!(source, CrateSource::External { .. }));
        assert_eq!(source.version(), Some("1.0.210"));
    }

    // ---- resolve_crate_without_context ----

    #[test]
    fn without_context_returns_error_for_current_crate() {
        let result = resolve_crate_without_context(&CrateSpec::CurrentCrate);
        assert!(result.is_err());
    }

    #[test]
    fn without_context_returns_stdlib_for_std() {
        let source = resolve_crate_without_context(&CrateSpec::Named("std".to_string()))
            .expect("should resolve std");
        assert!(matches!(source, CrateSource::Stdlib { .. }));
    }

    #[test]
    fn without_context_returns_external_for_unknown() {
        let source = resolve_crate_without_context(&CrateSpec::Named("tokio".to_string()))
            .expect("should return External");
        assert!(matches!(source, CrateSource::External { .. }));
    }

    #[test]
    fn without_context_returns_external_for_versioned() {
        let source = resolve_crate_without_context(&CrateSpec::Versioned {
            name: "serde".to_string(),
            version: "1.0.210".to_string(),
        })
        .expect("should resolve versioned");
        assert!(matches!(source, CrateSource::External { .. }));
        assert_eq!(source.version(), Some("1.0.210"));
    }

    // ---- CrateSource accessors ----

    #[test]
    fn crate_source_name_returns_name_for_all_variants() {
        let current = CrateSource::CurrentCrate {
            manifest_path: PathBuf::new(),
            name: "foo".to_string(),
            version: "1.0.0".to_string(),
        };
        assert_eq!(current.name(), "foo");

        let dep = CrateSource::Dependency {
            manifest_path: PathBuf::new(),
            name: "bar".to_string(),
            version: "2.0.0".to_string(),
        };
        assert_eq!(dep.name(), "bar");

        let stdlib = CrateSource::Stdlib {
            name: "std".to_string(),
        };
        assert_eq!(stdlib.name(), "std");

        let ext = CrateSource::External {
            name: "baz".to_string(),
            version: None,
        };
        assert_eq!(ext.name(), "baz");
    }

    #[test]
    fn crate_source_version_returns_correctly() {
        let current = CrateSource::CurrentCrate {
            manifest_path: PathBuf::new(),
            name: "foo".to_string(),
            version: "1.0.0".to_string(),
        };
        assert_eq!(current.version(), Some("1.0.0"));

        let stdlib = CrateSource::Stdlib {
            name: "std".to_string(),
        };
        assert_eq!(stdlib.version(), None);

        let ext = CrateSource::External {
            name: "baz".to_string(),
            version: Some("3.0.0".to_string()),
        };
        assert_eq!(ext.version(), Some("3.0.0"));

        let ext_none = CrateSource::External {
            name: "baz".to_string(),
            version: None,
        };
        assert_eq!(ext_none.version(), None);
    }

    // ---- is_virtual_workspace ----

    #[test]
    fn is_virtual_workspace_returns_false_for_normal_crate() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        assert!(
            !ctx.is_virtual_workspace(),
            "groxide is a normal crate, not a virtual workspace"
        );
    }

    // ---- workspace_member_packages ----

    #[test]
    fn workspace_member_packages_returns_at_least_current() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let members = ctx.workspace_member_packages();
        assert!(
            !members.is_empty(),
            "should have at least one workspace member"
        );
        assert!(
            members.iter().any(|p| p.name == "groxide"),
            "should include groxide"
        );
    }

    #[test]
    fn workspace_member_packages_returns_sorted_by_name() {
        let ctx = ProjectContext::discover(None).expect("should find Cargo.toml");
        let members = ctx.workspace_member_packages();
        let names: Vec<&str> = members.iter().map(|p| p.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "members should be sorted by name");
    }
}
