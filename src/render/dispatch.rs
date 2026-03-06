use std::io::Write;

use crate::cli::Cli;
use crate::error::{GroxError, Result};
use crate::resolve::{CrateSource, ProjectContext};
use crate::types::{DisplayItem, DisplayLimits, DocIndex, ItemKind, QueryResult};
use crate::{cli::FeatureFlags, query, reexport, render, types};

/// Renders items in `--recursive` mode using the appropriate detail tier.
///
/// Collects all children recursively, applies kind filtering, and dispatches
/// to the json/brief/docs/list renderer based on CLI flags.
pub(crate) fn render_recursive(
    w: &mut impl Write,
    index: &DocIndex,
    idx: usize,
    cli: &Cli,
) -> Result<()> {
    let kind_filter = cli.kind.map(ItemKind::from);
    let mut items = render::collect_children_recursive(index, idx, cli.private);
    if let Some(filter) = kind_filter {
        items.retain(|item| item.kind.matches_filter(filter));
    }
    let root_path = &index.get(idx).path;
    let output = if cli.json {
        render::json::render_json_recursive(&items)
    } else if cli.brief {
        render::brief::render_brief_recursive(&items, root_path)
    } else if cli.docs {
        render::docs::render_docs_recursive(&items, root_path)
    } else {
        render::list::render_list_recursive(&items, root_path)
    };
    writeln!(w, "{output}").map_err(GroxError::Io)?;
    Ok(())
}

/// Renders items in `--recursive --source` mode with separator lines.
///
/// Collects all children recursively, applies kind filtering, and renders
/// each item's source code separated by horizontal rules.
pub(crate) fn render_recursive_source(
    w: &mut impl Write,
    index: &DocIndex,
    idx: usize,
    source: &CrateSource,
    cli: &Cli,
) -> Result<()> {
    let kind_filter = cli.kind.map(ItemKind::from);
    let mut items = render::collect_children_recursive(index, idx, cli.private);
    if let Some(filter) = kind_filter {
        items.retain(|item| item.kind.matches_filter(filter));
    }

    let mut first = true;
    for child in &items {
        if !first {
            writeln!(w).map_err(GroxError::Io)?;
            writeln!(w, "────────────────────────────────────────").map_err(GroxError::Io)?;
            writeln!(w).map_err(GroxError::Io)?;
        }
        first = false;
        let content = crate::read_source_content(child, source);
        let rendered = render::ambiguous::render_source(child, content.as_deref(), cli.docs);
        writeln!(w, "{rendered}").map_err(GroxError::Io)?;
    }
    Ok(())
}

/// Handles default/list/json/impls output.
pub(crate) fn handle_output(
    w: &mut impl Write,
    result: &QueryResult,
    index: &DocIndex,
    cli: &Cli,
    ctx: Option<&ProjectContext>,
    features: &FeatureFlags,
    feature_suffix: &str,
) -> Result<()> {
    match result {
        QueryResult::Found { index: idx } => {
            let item = index.get(*idx);

            // Follow cross-crate re-export stubs to the canonical item in the
            // source crate. This gives all output modes (text, json, list, impls)
            // access to real docs, methods, and trait implementations.
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

            // --recursive: collect all items recursively and render
            if cli.recursive {
                return render_recursive(w, using_index, effective_idx, cli);
            }

            let kind_filter = cli.kind.map(ItemKind::from);
            let display =
                render::build_display_item(using_index, effective_idx, cli.private, kind_filter);

            if let Some(ref filter) = cli.impls {
                let trait_filter = if filter.is_empty() {
                    None
                } else {
                    Some(filter.as_str())
                };
                let output = render_impls(&display, trait_filter);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else if cli.json {
                let output = render::json::render_json(&display);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else if cli.brief {
                let output = render::brief::render_brief(&display);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else {
                let limits = DisplayLimits::default();
                let canonical_output = render::text::render_text(&display, &limits);
                // For followed re-exports, annotate with the stub path and source note
                let output = if effective_index.is_some() {
                    let source_path = reexport::parse_reexport_source(item).unwrap_or_default();
                    reexport::annotate_reexport(&canonical_output, &item.path, &source_path)
                } else {
                    canonical_output
                };
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            }
            Ok(())
        }
        QueryResult::Ambiguous { indices, query } => {
            if cli.json {
                let items: Vec<&types::IndexItem> = indices.iter().map(|&i| index.get(i)).collect();
                let output = render::json::render_json_ambiguous(&items);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else {
                let output = render::ambiguous::render_ambiguous(index, indices, query);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            }
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

/// Renders the `--impls` view for a display item.
pub(crate) fn render_impls(display: &DisplayItem<'_>, trait_filter: Option<&str>) -> String {
    match display {
        DisplayItem::Type {
            item, trait_impls, ..
        } => render::ambiguous::render_impls_type(item, trait_impls, trait_filter),
        DisplayItem::Trait { item, .. } => {
            // For traits, gather implementors from the index (not stored yet, return empty)
            render::ambiguous::render_impls_trait(item, &[])
        }
        DisplayItem::Crate { item, .. }
        | DisplayItem::Module { item, .. }
        | DisplayItem::Leaf { item } => render::ambiguous::render_impls_other(item),
    }
}
