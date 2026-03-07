use std::io::Write;

use crate::cli::{Cli, OutputMode};
use crate::error::{GroxError, Result};
use crate::resolve::{CrateSource, ProjectContext};
use crate::types::{DisplayItem, DisplayLimits, DocIndex, ItemKind, QueryResult};
use crate::{cli::FeatureFlags, query, reexport, render, types};

/// Bundles commonly-threaded render parameters into a single context.
///
/// Avoids passing overlapping parameter sets like `(index, limits, mode,
/// kind_filter, private)` through every dispatch function.
pub(crate) struct RenderContext<'a> {
    pub index: &'a DocIndex,
    pub limits: DisplayLimits,
    pub mode: OutputMode,
    pub kind_filter: Option<ItemKind>,
    pub include_private: bool,
}

impl<'a> RenderContext<'a> {
    /// Creates a render context from CLI flags and an index.
    pub fn from_cli(index: &'a DocIndex, cli: &Cli) -> Self {
        Self {
            index,
            limits: DisplayLimits::default(),
            mode: cli.output_mode(),
            kind_filter: cli.kind.map(ItemKind::from),
            include_private: cli.private,
        }
    }
}

/// Renders items in `--recursive` mode using the appropriate detail tier.
///
/// Collects all children recursively, applies kind filtering, and dispatches
/// to the json/brief/docs/list renderer based on CLI flags.
pub(crate) fn render_recursive(
    w: &mut impl Write,
    ctx: &RenderContext<'_>,
    idx: usize,
    include_docs: bool,
) -> Result<()> {
    let mut items = render::collect_children_recursive(ctx.index, idx, ctx.include_private);
    if let Some(filter) = ctx.kind_filter {
        items.retain(|item| item.kind.matches_filter(filter));
    }
    let root_path = &ctx.index.get(idx).path;
    let output = match ctx.mode {
        OutputMode::Json => render::json::render_json_recursive(&items),
        OutputMode::Brief => render::brief::render_brief_recursive(&items, root_path),
        OutputMode::Text if include_docs => render::docs::render_docs_recursive(&items, root_path),
        OutputMode::Text => render::list::render_list_recursive(&items, root_path),
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
    ctx: &RenderContext<'_>,
    idx: usize,
    source: &CrateSource,
    include_docs: bool,
) -> Result<()> {
    let mut items = render::collect_children_recursive(ctx.index, idx, ctx.include_private);
    if let Some(filter) = ctx.kind_filter {
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
        let content = crate::source::read_source_content(child, source);
        let rendered = render::ambiguous::render_source(child, content.as_deref(), include_docs);
        writeln!(w, "{rendered}").map_err(GroxError::Io)?;
    }
    Ok(())
}

/// Handles default/list/json/impls output.
pub(crate) fn handle_output(
    w: &mut impl Write,
    result: &QueryResult,
    ctx: &RenderContext<'_>,
    cli: &Cli,
    project_ctx: Option<&ProjectContext>,
    features: &FeatureFlags,
    feature_suffix: &str,
) -> Result<()> {
    match result {
        QueryResult::Found { index: idx } => {
            let item = ctx.index.get(*idx);

            // Follow cross-crate re-export stubs to the canonical item in the
            // source crate. This gives all output modes (text, json, list, impls)
            // access to real docs, methods, and trait implementations.
            let (effective_index, effective_idx) = if query::is_reexport_stub(item) {
                if let Some((source_index, canonical_idx)) = reexport::try_follow_reexport(
                    item,
                    project_ctx,
                    features,
                    feature_suffix,
                    ctx.include_private,
                ) {
                    (Some(source_index), canonical_idx)
                } else {
                    (None, *idx)
                }
            } else {
                (None, *idx)
            };

            let using_index = effective_index.as_ref().unwrap_or(ctx.index);

            // --recursive: collect all items recursively and render
            if cli.recursive {
                // Build a new context for the effective index (may differ from
                // ctx.index when following re-exports).
                let effective_ctx = RenderContext {
                    index: using_index,
                    limits: DisplayLimits::default(),
                    mode: ctx.mode,
                    kind_filter: ctx.kind_filter,
                    include_private: ctx.include_private,
                };
                return render_recursive(w, &effective_ctx, effective_idx, cli.docs);
            }

            let display = render::build_display_item(
                using_index,
                effective_idx,
                ctx.include_private,
                ctx.kind_filter,
            );

            if let Some(ref filter) = cli.impls {
                let trait_filter = if filter.is_empty() {
                    None
                } else {
                    Some(filter.as_str())
                };
                let output = render_impls(&display, trait_filter);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else {
                let output = match ctx.mode {
                    OutputMode::Json => render::json::render_json(&display),
                    OutputMode::Brief => render::brief::render_brief(&display),
                    OutputMode::Text => {
                        let canonical_output = render::text::render_text(&display, &ctx.limits);
                        // For followed re-exports, annotate with the stub path and source note
                        if effective_index.is_some() {
                            let source_path =
                                reexport::parse_reexport_source(item).unwrap_or_default();
                            reexport::annotate_reexport(&canonical_output, &item.path, &source_path)
                        } else {
                            canonical_output
                        }
                    }
                };
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            }
            Ok(())
        }
        QueryResult::Ambiguous { indices, query } => {
            let output = match ctx.mode {
                OutputMode::Json => {
                    let items: Vec<&types::IndexItem> =
                        indices.iter().map(|&i| ctx.index.get(i)).collect();
                    render::json::render_json_ambiguous(&items)
                }
                OutputMode::Brief | OutputMode::Text => {
                    render::ambiguous::render_ambiguous(ctx.index, indices, query)
                }
            };
            writeln!(w, "{output}").map_err(GroxError::Io)?;
            Ok(())
        }
        QueryResult::NotFound {
            query, suggestions, ..
        } => Err(GroxError::ItemNotFound {
            query: query.clone(),
            crate_name: Some(ctx.index.crate_name.clone()),
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
