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
    index: &DocIndex,
    source: &CrateSource,
    cli: &Cli,
    ctx: Option<&ProjectContext>,
    features: &FeatureFlags,
    feature_suffix: &str,
) -> Result<()> {
    match result {
        QueryResult::Found { index: idx } => {
            let item = index.get(*idx);

            // Follow cross-crate re-export stubs
            let (effective_index, effective_idx) = if query::is_reexport_stub(item) {
                if let Some((source_index, canonical_idx)) =
                    reexport::try_follow_reexport(item, ctx, features, feature_suffix, cli.private)
                {
                    (Some(source_index), canonical_idx)
                } else {
                    (None, *idx)
                }
            } else {
                (None, *idx)
            };

            let using_index = effective_index.as_ref().unwrap_or(index);

            render::dispatch::render_recursive_source(w, using_index, effective_idx, source, cli)
        }
        QueryResult::Ambiguous { indices, .. } => {
            // For ambiguous results, just show source for each match
            let items_with_source: Vec<_> = indices
                .iter()
                .map(|&idx| {
                    let item = index.get(idx);
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
            crate_name: Some(index.crate_name.clone()),
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

    let source_root = match source {
        CrateSource::CurrentCrate { manifest_path, .. }
        | CrateSource::Dependency { manifest_path, .. } => manifest_path.parent()?.to_path_buf(),
        CrateSource::External { name, version } => {
            let cache_dir = dirs::cache_dir()?;
            let ver = version.as_deref().unwrap_or("latest");
            cache_dir.join("groxide").join(format!("{name}-{ver}"))
        }
        CrateSource::Stdlib { .. } => {
            let sysroot = crate::stdlib::get_sysroot().ok()?;
            crate::stdlib::stdlib_library_path(&sysroot).ok()?
        }
    };

    let file_path = source_root.join(&span.file);
    let content = std::fs::read_to_string(&file_path).ok()?;
    let lines: Vec<&str> = content.lines().collect();

    let start = (span.line_start as usize).saturating_sub(1);
    let end = (span.line_end as usize).min(lines.len());
    if start >= lines.len() || start >= end {
        return None;
    }

    Some(lines[start..end].join("\n"))
}
