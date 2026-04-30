use std::io::Write;

use crate::cli::{Cli, FeatureFlags};
use crate::error::{GroxError, Result};
use crate::resolve::{CrateSource, ProjectContext};
use crate::types::{DocIndex, ItemKind, QueryResult};
use crate::{query, reexport, render, types};

/// Handles `--source` mode.
pub(crate) fn handle_source(
    w: &mut impl Write,
    result: &QueryResult,
    index: &DocIndex,
    source: &CrateSource,
    include_docs: bool,
    kind_filter: Option<ItemKind>,
) -> Result<()> {
    match result {
        QueryResult::Found { index: idx } => {
            let item = index.get(*idx);

            // When a kind filter is set and the item is a module, show source
            // for matching children instead of the module's own source.
            if kind_filter.is_some() && item.kind == ItemKind::Module {
                let matching: Vec<_> = item
                    .children
                    .iter()
                    .map(|c| index.get(c.index))
                    .filter(|c| c.is_public)
                    .filter(|c| kind_filter.is_none_or(|k| c.kind.matches_filter(k)))
                    .collect();

                if matching.is_empty() {
                    return Err(GroxError::ItemNotFound {
                        query: item.path.clone(),
                        crate_name: Some(index.crate_name.clone()),
                        suggestions: Vec::new(),
                    });
                }

                let items_with_source: Vec<_> = matching
                    .iter()
                    .map(|child| {
                        let content = read_source_content(child, source);
                        (*child, content)
                    })
                    .collect();

                let refs: Vec<_> = items_with_source
                    .iter()
                    .map(|(item, content)| (*item, content.as_deref()))
                    .collect();

                let output = render::ambiguous::render_source_ambiguous(&refs, include_docs);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
                return Ok(());
            }

            let content = read_source_content(item, source);
            let output = render::ambiguous::render_source(item, content.as_deref(), include_docs);
            writeln!(w, "{output}").map_err(GroxError::Io)?;
            Ok(())
        }
        QueryResult::Ambiguous { indices, .. } => {
            let items_with_source: Vec<_> = indices
                .iter()
                .map(|&idx| {
                    let item = index.get(idx);
                    let content = read_source_content(item, source);
                    (item, content)
                })
                .collect();

            // Need to convert owned strings to refs for the render function
            let refs: Vec<_> = items_with_source
                .iter()
                .map(|(item, content)| (*item, content.as_deref()))
                .collect();

            let output = render::ambiguous::render_source_ambiguous(&refs, include_docs);
            writeln!(w, "{output}").map_err(GroxError::Io)?;
            Ok(())
        }
        QueryResult::NotFound {
            query, suggestions, ..
        } => Err(GroxError::ItemNotFound {
            query: query.clone(),
            crate_name: Some(index.crate_name.clone()),
            suggestions: suggestions.clone(),
        }),
    }
}

/// Handles `--recursive --source` mode: dumps docs + source for all items.
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_recursive_source(
    w: &mut impl Write,
    result: &QueryResult,
    render_ctx: &render::dispatch::RenderContext<'_>,
    source: &CrateSource,
    cli: &Cli,
    ctx: Option<&ProjectContext>,
    features: &FeatureFlags,
    feature_suffix: &str,
) -> Result<()> {
    match result {
        QueryResult::Found { index: idx } => {
            let item = render_ctx.index.get(*idx);

            // Follow cross-crate re-export stubs
            let (effective_index, effective_idx) = if query::is_reexport_stub(item) {
                if let Some((source_index, canonical_idx)) = reexport::try_follow_reexport(
                    item,
                    ctx,
                    features,
                    feature_suffix,
                    render_ctx.include_private,
                ) {
                    (Some(source_index), canonical_idx)
                } else {
                    (None, *idx)
                }
            } else {
                (None, *idx)
            };

            // Build context for the effective index (may differ when following re-exports)
            let effective_ctx = if let Some(ref eff_index) = effective_index {
                render::dispatch::RenderContext {
                    index: eff_index,
                    limits: types::DisplayLimits::default(),
                    mode: render_ctx.mode,
                    kind_filter: render_ctx.kind_filter,
                    include_private: render_ctx.include_private,
                }
            } else {
                render::dispatch::RenderContext {
                    index: render_ctx.index,
                    limits: types::DisplayLimits::default(),
                    mode: render_ctx.mode,
                    kind_filter: render_ctx.kind_filter,
                    include_private: render_ctx.include_private,
                }
            };

            render::dispatch::render_recursive_source(
                w,
                &effective_ctx,
                effective_idx,
                source,
                cli.docs,
            )
        }
        QueryResult::Ambiguous { indices, .. } => {
            // For ambiguous results, just show source for each match
            let items_with_source: Vec<_> = indices
                .iter()
                .map(|&idx| {
                    let item = render_ctx.index.get(idx);
                    let content = read_source_content(item, source);
                    (item, content)
                })
                .collect();

            let refs: Vec<_> = items_with_source
                .iter()
                .map(|(item, content)| (*item, content.as_deref()))
                .collect();

            let output = render::ambiguous::render_source_ambiguous(&refs, cli.docs);
            writeln!(w, "{output}").map_err(GroxError::Io)?;
            Ok(())
        }
        QueryResult::NotFound {
            query, suggestions, ..
        } => Err(GroxError::ItemNotFound {
            query: query.clone(),
            crate_name: Some(render_ctx.index.crate_name.clone()),
            suggestions: suggestions.clone(),
        }),
    }
}

/// Reads source code for an item from the filesystem.
pub(crate) fn read_source_content(item: &types::IndexItem, source: &CrateSource) -> Option<String> {
    let span = &item.span;
    if span.file.is_empty() || (span.line_start == 0 && span.line_end == 0) {
        return None;
    }

    let file_path = match source {
        CrateSource::CurrentCrate { manifest_path, .. }
        | CrateSource::Dependency { manifest_path, .. } => {
            // Span paths are emitted relative to the cargo invocation root.
            // For workspace members, that's the workspace root, not the
            // package directory — so walk up ancestors of the package dir
            // until span.file resolves to an existing file.
            resolve_span_file(manifest_path.parent()?, &span.file)?
        }
        CrateSource::External { name, version } => {
            let cache_dir = dirs::cache_dir()?;
            let ver = version.as_deref().unwrap_or("latest");
            cache_dir
                .join("groxide")
                .join(format!("{name}-{ver}"))
                .join(&span.file)
        }
        CrateSource::Stdlib { .. } => {
            let sysroot = crate::stdlib::get_sysroot().ok()?;
            crate::stdlib::stdlib_library_path(&sysroot)
                .ok()?
                .join(&span.file)
        }
    };

    let content = std::fs::read_to_string(&file_path).ok()?;
    let lines: Vec<&str> = content.lines().collect();

    let start = (span.line_start as usize).saturating_sub(1);
    let end = (span.line_end as usize).min(lines.len());
    if start >= lines.len() || start >= end {
        return None;
    }

    Some(lines[start..end].join("\n"))
}

/// Resolves a span file path against a package directory, walking up to the
/// workspace root if necessary.
///
/// Cargo emits span paths relative to the directory it was invoked from. For
/// workspace members that's the workspace root, so a span like
/// `crate-a/src/lib.rs` won't resolve under the package directory directly.
/// Absolute paths (e.g. registry sources) are returned as-is by `Path::join`.
fn resolve_span_file(package_dir: &std::path::Path, span_file: &str) -> Option<std::path::PathBuf> {
    let mut dir = Some(package_dir);
    while let Some(d) = dir {
        let candidate = d.join(span_file);
        if candidate.is_file() {
            return Some(candidate);
        }
        dir = d.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ItemKind, SourceSpan};

    fn make_item(file: &str, line_start: u32, line_end: u32) -> types::IndexItem {
        types::IndexItem {
            path: "demo".into(),
            name: "demo".into(),
            kind: ItemKind::Function,
            signature: String::new(),
            docs: String::new(),
            summary: String::new(),
            span: SourceSpan {
                file: file.into(),
                line_start,
                line_end,
            },
            children: Vec::new(),
            is_public: true,
            has_body: true,
            feature_gate: None,
            reexport_source: None,
        }
    }

    #[test]
    fn reads_workspace_member_span_relative_to_workspace_root() {
        // Mimics a virtual workspace:
        //   <ws>/Cargo.toml
        //   <ws>/member-a/Cargo.toml
        //   <ws>/member-a/src/lib.rs
        // Cargo emits span paths relative to the workspace root, so for the
        // member-a package the span will be "member-a/src/lib.rs".
        let tmp = tempfile::TempDir::new().unwrap();
        let ws = tmp.path();
        let pkg_dir = ws.join("member-a");
        std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
        std::fs::write(ws.join("Cargo.toml"), "[workspace]\nmembers=[\"member-a\"]\n").unwrap();
        std::fs::write(pkg_dir.join("Cargo.toml"), "[package]\nname=\"member-a\"\n").unwrap();
        std::fs::write(
            pkg_dir.join("src").join("lib.rs"),
            "line one\nline two\nline three\n",
        )
        .unwrap();

        let source = CrateSource::Dependency {
            manifest_path: pkg_dir.join("Cargo.toml"),
            name: "member-a".into(),
            version: "0.1.0".into(),
        };
        let item = make_item("member-a/src/lib.rs", 1, 2);

        let got = read_source_content(&item, &source).expect("source should be readable");
        assert_eq!(got, "line one\nline two");
    }

    #[test]
    fn reads_single_crate_span_relative_to_package_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let pkg_dir = tmp.path();
        std::fs::create_dir_all(pkg_dir.join("src")).unwrap();
        std::fs::write(pkg_dir.join("Cargo.toml"), "[package]\nname=\"solo\"\n").unwrap();
        std::fs::write(pkg_dir.join("src").join("lib.rs"), "alpha\nbeta\n").unwrap();

        let source = CrateSource::CurrentCrate {
            manifest_path: pkg_dir.join("Cargo.toml"),
            name: "solo".into(),
            version: "0.1.0".into(),
        };
        let item = make_item("src/lib.rs", 1, 2);

        let got = read_source_content(&item, &source).expect("source should be readable");
        assert_eq!(got, "alpha\nbeta");
    }
}
