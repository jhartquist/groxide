mod cache;
pub mod cli;
mod docgen;
pub mod error;
mod external;
mod index_builder;
mod query;
mod render;
mod resolve;
mod search;
mod signature;
mod stdlib;
#[cfg(test)]
mod test_utils;
mod types;

use std::io::{self, Write};
use std::path::Path;
use std::time::Instant;

use cli::{Cli, CrateSpec, FeatureFlags, QueryPath};
use error::{GroxError, Result};
use resolve::{CrateSource, ProjectContext};
use types::{DisplayItem, DisplayLimits, DocIndex, ItemKind, QueryResult};

/// Runs the groxide CLI with the given parsed arguments.
///
/// Returns `Ok(())` on success, `Err(GroxError)` on failure.
///
/// # Errors
///
/// Returns `GroxError` if crate resolution, doc generation, or querying fails.
pub fn run(cli: &Cli) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Step 1: Discover project context
    let ctx = if cli.manifest_path.is_some() {
        Some(ProjectContext::discover(cli.manifest_path.as_deref())?)
    } else {
        ProjectContext::discover(None).ok()
    };

    // Step 2: Parse query path
    let query_path = QueryPath::parse(cli.path.as_deref().unwrap_or(""))?;

    // Step 3: Resolve crate source
    let (source, query_path) = resolve_crate_source(ctx.as_ref(), query_path)?;

    // Step 4: Load or build DocIndex
    let features = FeatureFlags::from_cli(cli);
    let feature_suffix = features.cache_suffix();
    let (index, source) = load_or_build_index(source, &features, &feature_suffix, cli.private)?;

    // Step 5: Handle --readme (early return)
    if cli.readme {
        handle_readme(&mut out, &source, ctx.as_ref())?;
    } else if let Some(search_query) = &cli.search {
        // Step 6: Handle --search
        handle_search(&mut out, &index, search_query, cli)?;
    } else {
        // Step 7: Resolve item in index
        let kind_filter = cli.kind.map(ItemKind::from);
        let result = resolve_item(
            &query_path,
            &index,
            kind_filter,
            &features,
            &feature_suffix,
            cli.private,
        );

        if cli.source {
            // Step 8: Handle --source
            handle_source(&mut out, &result, &index, &source)?;
        } else {
            // Step 9: Render output
            handle_output(
                &mut out,
                &result,
                &index,
                cli,
                ctx.as_ref(),
                &features,
                &feature_suffix,
            )?;
        }
    }

    Ok(())
}

/// Resolves `CrateSpec` to `CrateSource`, with single-segment item reinterpretation.
fn resolve_crate_source(
    ctx: Option<&ProjectContext>,
    query: QueryPath,
) -> Result<(CrateSource, QueryPath)> {
    match &query.crate_spec {
        CrateSpec::CurrentCrate => {
            if let Some(ctx) = ctx {
                Ok((ctx.resolve_crate(&CrateSpec::CurrentCrate), query))
            } else {
                Err(GroxError::ManifestNotFound)
            }
        }
        CrateSpec::Versioned { name, version } => Ok((
            CrateSource::External {
                name: name.clone(),
                version: Some(version.clone()),
            },
            query,
        )),
        CrateSpec::Named(name) => {
            if let Some(ctx) = ctx {
                let source = ctx.resolve_crate(&CrateSpec::Named(name.clone()));
                if matches!(&source, CrateSource::External { .. }) {
                    // Not found in project deps. Check single-segment reinterpretation.
                    if query.item_segments.is_empty() && query::looks_like_item_name(name) {
                        // Reinterpret as item in current crate
                        let current_source = ctx.resolve_crate(&CrateSpec::CurrentCrate);
                        let reinterpreted = QueryPath {
                            crate_spec: CrateSpec::CurrentCrate,
                            item_segments: vec![name.clone()],
                        };
                        return Ok((current_source, reinterpreted));
                    }
                    // Auto-fetch
                    let version = match &source {
                        CrateSource::External { version, .. } => version.clone(),
                        _ => None,
                    };
                    print_auto_fetch_message(name, version.as_deref());
                }
                Ok((source, query))
            } else {
                let source =
                    resolve::resolve_crate_without_context(&CrateSpec::Named(name.clone()))?;
                if matches!(&source, CrateSource::External { .. }) {
                    print_auto_fetch_message(name, None);
                }
                Ok((source, query))
            }
        }
    }
}

/// Prints the auto-fetch stderr message.
fn print_auto_fetch_message(name: &str, version: Option<&str>) {
    match version {
        Some(v) => eprintln!("[grox] {name} not in project deps, fetching {v} from crates.io..."),
        None => eprintln!("[grox] {name} not in project deps, fetching latest from crates.io..."),
    }
}

/// Loads `DocIndex` from cache or builds from rustdoc JSON.
fn load_or_build_index(
    source: CrateSource,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
) -> Result<(DocIndex, CrateSource)> {
    // Compute cache path
    let cache_file = cache::cache_path(&source, feature_suffix);

    // Try loading from cache
    if let Some(ref path) = cache_file {
        if let Some(index) = cache::load_cached(path, &source) {
            return Ok((index, source));
        }
    }

    let start = Instant::now();
    let name = source.name().to_string();
    let version = source.version().unwrap_or("").to_string();
    eprint!("[grox] Building index for {name} {version}...");

    // Handle external crates: fetch first
    let (json_path, source) = if let CrateSource::External {
        name: ext_name,
        version: version_opt,
    } = source
    {
        let (json_path, canonical_name, resolved_version) =
            external::fetch_external_crate(&ext_name, version_opt.as_deref(), features, private)?;
        let source = CrateSource::External {
            name: canonical_name,
            version: Some(resolved_version),
        };
        (json_path, source)
    } else {
        let json_path = docgen::generate_rustdoc_json(&source, features, private)?;
        (json_path, source)
    };

    // Parse rustdoc JSON
    let json_str = std::fs::read_to_string(&json_path).map_err(|e| GroxError::JsonReadFailed {
        path: json_path.clone(),
        source: e,
    })?;
    let krate = index_builder::parse_rustdoc_json(&json_str)?;

    // Build index — normalize crate name (hyphens -> underscores) for Rust module paths
    let crate_name = resolve::normalize_crate_name(source.name());
    let crate_version = source.version().unwrap_or("");
    let index = index_builder::build_index(&krate, &crate_name, crate_version);

    // Save to cache (best-effort)
    if let Some(ref path) = cache_file {
        cache::save_to_cache(path, &index, &source);
    } else {
        // Recompute cache path since source may have changed (external version resolved)
        if let Some(path) = cache::cache_path(&source, feature_suffix) {
            cache::save_to_cache(&path, &index, &source);
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    eprintln!(" done ({elapsed:.1}s)");

    Ok((index, source))
}

/// Resolves an item with all fallback strategies.
fn resolve_item(
    query: &QueryPath,
    index: &DocIndex,
    kind_filter: Option<ItemKind>,
    _features: &FeatureFlags,
    _feature_suffix: &str,
    _private: bool,
) -> QueryResult {
    let crate_name = &index.crate_name;

    // If no item segments, look up crate root
    if query.item_segments.is_empty() {
        let result = query::lookup(index, crate_name, kind_filter);
        if !matches!(result, QueryResult::NotFound { .. }) {
            return result;
        }
        // Crate root not found by name, try hyphen-normalized
        let normalized = crate_name.replace('-', "_");
        if normalized != *crate_name {
            let result = query::lookup(index, &normalized, kind_filter);
            if !matches!(result, QueryResult::NotFound { .. }) {
                return result;
            }
        }
        return result;
    }

    let item_path = query.item_segments.join("::");

    // Try 1: full path with crate name prepended
    let full_path = format!("{crate_name}::{item_path}");
    let result = query::lookup(index, &full_path, kind_filter);
    if !matches!(result, QueryResult::NotFound { .. }) {
        return result;
    }

    // Try 2: item segments only (suffix matching)
    let result = query::lookup(index, &item_path, kind_filter);
    if !matches!(result, QueryResult::NotFound { .. }) {
        return result;
    }

    // Try 3: if kind filter was set, retry without it
    if kind_filter.is_some() {
        let result = query::lookup(index, &full_path, None);
        if !matches!(result, QueryResult::NotFound { .. }) {
            return result;
        }
        let result = query::lookup(index, &item_path, None);
        if !matches!(result, QueryResult::NotFound { .. }) {
            return result;
        }
    }

    // Try 4: method lookup (2+ segments)
    if query.item_segments.len() >= 2 {
        let parent_segments: Vec<&str> = query.item_segments[..query.item_segments.len() - 1]
            .iter()
            .map(String::as_str)
            .collect();
        let method_name = &query.item_segments[query.item_segments.len() - 1];

        // Try with crate name prefix
        let mut full_parent: Vec<&str> = vec![crate_name.as_str()];
        full_parent.extend(&parent_segments);
        let result = query::lookup_method(index, &full_parent, method_name, kind_filter);
        if !matches!(result, QueryResult::NotFound { .. }) {
            return result;
        }

        // Try without crate name prefix
        let result = query::lookup_method(index, &parent_segments, method_name, kind_filter);
        if !matches!(result, QueryResult::NotFound { .. }) {
            return result;
        }
    }

    // Final: not found with suggestions
    QueryResult::NotFound {
        query: item_path,
        suggestions: Vec::new(),
    }
}

/// Handles `--readme` mode.
fn handle_readme(
    w: &mut impl Write,
    source: &CrateSource,
    ctx: Option<&ProjectContext>,
) -> Result<()> {
    match source {
        CrateSource::Stdlib { name } => {
            eprintln!("README not available for standard library crate '{name}'");
            Err(GroxError::ItemNotFound {
                query: format!("{name} README"),
                crate_name: Some(name.clone()),
                suggestions: Vec::new(),
            })
        }
        CrateSource::CurrentCrate { manifest_path, .. } => {
            let dir = manifest_path
                .parent()
                .expect("invariant: manifest_path has a parent");
            // Try workspace root first if available
            let search_dir = ctx.map_or_else(
                || dir.to_path_buf(),
                |c| {
                    c.current_manifest_path()
                        .parent()
                        .map_or_else(|| dir.to_path_buf(), Path::to_path_buf)
                },
            );
            find_and_print_readme(w, &search_dir, source.name())
        }
        CrateSource::Dependency { manifest_path, .. } => {
            let dir = manifest_path
                .parent()
                .expect("invariant: manifest_path has a parent");
            find_and_print_readme(w, dir, source.name())
        }
        CrateSource::External { name, version } => {
            let cache_dir = dirs::cache_dir().ok_or(GroxError::ItemNotFound {
                query: format!("{name} README"),
                crate_name: Some(name.clone()),
                suggestions: Vec::new(),
            })?;
            let ver = version.as_deref().unwrap_or("latest");
            let dir = cache_dir.join("groxide").join(format!("{name}-{ver}"));
            find_and_print_readme(w, &dir, name)
        }
    }
}

/// Searches for a README file in the given directory and prints it.
fn find_and_print_readme(w: &mut impl Write, dir: &Path, crate_name: &str) -> Result<()> {
    const README_NAMES: &[&str] = &[
        "README.md",
        "README.MD",
        "Readme.md",
        "readme.md",
        "README",
        "README.txt",
    ];

    for name in README_NAMES {
        let path = dir.join(name);
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            write!(w, "{content}").map_err(GroxError::Io)?;
            return Ok(());
        }
    }

    eprintln!("No README found for {crate_name}");
    Err(GroxError::ItemNotFound {
        query: format!("{crate_name} README"),
        crate_name: Some(crate_name.to_string()),
        suggestions: Vec::new(),
    })
}

/// Handles `--search` mode.
fn handle_search(
    w: &mut impl Write,
    index: &DocIndex,
    search_query: &str,
    cli: &Cli,
) -> Result<()> {
    let kind_filter = cli.kind.map(ItemKind::from);
    let results = search::search(index, search_query, kind_filter)?;
    let total = results.len();

    if cli.json {
        // JSON Lines output
        for r in &results {
            let item = index.get(r.index);
            let obj = serde_json::json!({
                "path": item.path,
                "kind": item.kind.short_name(),
                "signature": item.signature,
                "summary": item.summary,
                "score": r.score,
            });
            writeln!(
                w,
                "{}",
                serde_json::to_string(&obj).expect("invariant: json serializes")
            )
            .map_err(GroxError::Io)?;
        }
    } else {
        // Plain text output
        if total == 0 {
            writeln!(w, "0 results for \"{search_query}\":").map_err(GroxError::Io)?;
        } else {
            writeln!(w, "{total} results for \"{search_query}\":").map_err(GroxError::Io)?;
            writeln!(w).map_err(GroxError::Io)?;

            // Compute column widths
            let max_kind_width = results
                .iter()
                .map(|r| index.get(r.index).kind.short_name().len())
                .max()
                .unwrap_or(0);
            let max_path_width = results
                .iter()
                .map(|r| index.get(r.index).path.len())
                .max()
                .unwrap_or(0);

            for r in &results {
                let item = index.get(r.index);
                let kind = item.kind.short_name();
                let path = &item.path;
                let summary = &item.summary;
                if summary.is_empty() {
                    writeln!(w, "{kind:<max_kind_width$}  {path:<max_path_width$}")
                        .map_err(GroxError::Io)?;
                } else {
                    writeln!(
                        w,
                        "{kind:<max_kind_width$}  {path:<max_path_width$}  {summary}"
                    )
                    .map_err(GroxError::Io)?;
                }
            }
        }
    }

    Ok(())
}

/// Handles `--source` mode.
fn handle_source(
    w: &mut impl Write,
    result: &QueryResult,
    index: &DocIndex,
    source: &CrateSource,
) -> Result<()> {
    match result {
        QueryResult::Found { index: idx } => {
            let item = index.get(*idx);
            let content = read_source_content(item, source);
            let output = render::ambiguous::render_source(item, content.as_deref());
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

            let output = render::ambiguous::render_source_ambiguous(&refs);
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
fn read_source_content(item: &types::IndexItem, source: &CrateSource) -> Option<String> {
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
            let sysroot = stdlib::get_sysroot().ok()?;
            stdlib::stdlib_library_path(&sysroot).ok()?
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

/// Extracts the source path from a re-export signature.
///
/// Parses `"pub use {source}"` or `"pub use {source} as {name}"` and returns
/// the source path (e.g., `"serde_core::de::Deserialize"`).
fn parse_reexport_source(signature: &str) -> Option<String> {
    let rest = signature.strip_prefix("pub use ")?;
    // Handle "pub use source as name"
    let source = if let Some(pos) = rest.find(" as ") {
        &rest[..pos]
    } else {
        rest.trim_end_matches(';').trim()
    };
    if source.is_empty() {
        return None;
    }
    Some(source.to_string())
}

/// Follows a cross-crate re-export to the canonical item in the source crate.
///
/// Returns the source crate's `DocIndex` and the index of the canonical item,
/// or `None` if the re-export cannot be followed (e.g., source crate unavailable).
fn try_follow_reexport(
    stub: &types::IndexItem,
    ctx: Option<&ProjectContext>,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
) -> Option<(DocIndex, usize)> {
    let source_path = parse_reexport_source(&stub.signature)?;

    // Split into crate name + item path on first `::`
    let (crate_name, item_path) = source_path.split_once("::")?;

    // Resolve source crate
    let query_path = QueryPath {
        crate_spec: CrateSpec::Named(crate_name.to_string()),
        item_segments: Vec::new(),
    };
    let (source, _) = resolve_crate_source(ctx, query_path).ok()?;

    // Load source crate index
    let (source_index, _source) =
        load_or_build_index(source, features, feature_suffix, private).ok()?;

    // Query canonical item in source index
    let source_query = QueryPath {
        crate_spec: CrateSpec::CurrentCrate,
        item_segments: item_path.split("::").map(String::from).collect(),
    };
    let result = resolve_item(
        &source_query,
        &source_index,
        None,
        features,
        feature_suffix,
        private,
    );

    match result {
        QueryResult::Found { index: idx } => Some((source_index, idx)),
        _ => None,
    }
}

/// Post-processes rendered output for a followed re-export.
///
/// Replaces the canonical path in the header line with the stub path and inserts
/// a `"Re-exported from {source_path}."` note before the doc text.
fn annotate_reexport(output: &str, stub_path: &str, source_path: &str) -> String {
    use std::fmt::Write as _;

    let lines: Vec<&str> = output.lines().collect();
    if lines.is_empty() {
        return output.to_string();
    }

    // Replace the first line's path with the stub path.
    // The first line is something like "trait serde_core::de::Deserialize"
    // We want "trait serde::Deserialize"
    let first_line = lines[0];
    let Some(space_pos) = first_line.find(' ') else {
        return output.to_string();
    };

    let kind_prefix = &first_line[..space_pos];
    let new_first_line = format!("{kind_prefix} {stub_path}");

    // Find where to insert the re-export note.
    // After the signature (which follows the header after a blank line),
    // look for the next blank line (before docs).
    let mut insert_pos = None;
    let mut blank_count = 0;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.is_empty() {
            blank_count += 1;
            if blank_count == 2 {
                // After header + blank + signature + blank -> insert here
                insert_pos = Some(i + 1);
                break;
            }
        }
    }

    let mut result = String::with_capacity(output.len() + 100);
    result.push_str(&new_first_line);
    result.push('\n');

    for (i, line) in lines.iter().enumerate().skip(1) {
        result.push_str(line);
        result.push('\n');
        if insert_pos == Some(i + 1) {
            let _ = write!(result, "Re-exported from {source_path}.\n\n");
        }
    }

    // Trim trailing newlines (writeln in caller adds one)
    while result.ends_with('\n') {
        result.pop();
    }

    result
}

/// Handles default/list/json/impls output.
fn handle_output(
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

            // Check if this is a cross-crate re-export stub that we should follow
            if query::is_reexport_stub(item) && !cli.json && !cli.list && !cli.impls {
                if let Some((source_index, canonical_idx)) =
                    try_follow_reexport(item, ctx, features, feature_suffix, cli.private)
                {
                    let source_path = parse_reexport_source(&item.signature).unwrap_or_default();
                    let canonical_display =
                        render::build_display_item(&source_index, canonical_idx, cli.private);
                    let limits = if cli.all {
                        DisplayLimits {
                            expand_all: true,
                            ..DisplayLimits::default()
                        }
                    } else {
                        DisplayLimits::default()
                    };
                    let canonical_output = render::text::render_text(&canonical_display, &limits);
                    let output = annotate_reexport(&canonical_output, &item.path, &source_path);
                    writeln!(w, "{output}").map_err(GroxError::Io)?;
                    return Ok(());
                }
            }

            let display = render::build_display_item(index, *idx, cli.private);

            if cli.impls {
                let output = render_impls(&display, index, *idx);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else if cli.list {
                let output = render::list::render_list(&display);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else if cli.json {
                let output = render::json::render_json(&display);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else {
                let limits = if cli.all {
                    DisplayLimits {
                        expand_all: true,
                        ..DisplayLimits::default()
                    }
                } else {
                    DisplayLimits::default()
                };
                let output = render::text::render_text(&display, &limits);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            }
            Ok(())
        }
        QueryResult::Ambiguous { indices, query } => {
            if cli.json {
                let items: Vec<&types::IndexItem> = indices.iter().map(|&i| index.get(i)).collect();
                let output = render::json::render_json_ambiguous(&items);
                writeln!(w, "{output}").map_err(GroxError::Io)?;
            } else if cli.list {
                let output = render::ambiguous::render_ambiguous_list(index, indices);
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
fn render_impls(display: &DisplayItem<'_>, _index: &DocIndex, _item_idx: usize) -> String {
    match display {
        DisplayItem::Type {
            item, trait_impls, ..
        } => render::ambiguous::render_impls_type(item, trait_impls),
        DisplayItem::Trait { item, .. } => {
            // For traits, gather implementors from the index (not stored yet, return empty)
            render::ambiguous::render_impls_trait(item, &[])
        }
        DisplayItem::Crate { item, .. }
        | DisplayItem::Module { item, .. }
        | DisplayItem::Leaf { item } => render::ambiguous::render_impls_other(item),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a `DocIndex` from the fixture JSON.
    fn load_fixture_index() -> DocIndex {
        let json_path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/test-fixtures/groxide_test_api.json"
        );
        let json = std::fs::read_to_string(json_path).expect("fixture JSON exists");
        let krate = index_builder::parse_rustdoc_json(&json).expect("fixture JSON parses");
        index_builder::build_index(&krate, "groxide_test_api", "0.1.0")
    }

    /// Helper: run the full `resolve_item` pipeline and capture output.
    ///
    /// `path` is the item path *within* the fixture crate (e.g., `GenericStruct`),
    /// already resolved — it becomes `item_segments` on a `CurrentCrate` query.
    fn query_fixture(
        index: &DocIndex,
        path: &str,
        cli_args: &[&str],
    ) -> (std::result::Result<String, GroxError>, String) {
        let full_args: Vec<&str> = std::iter::once("grox")
            .chain(cli_args.iter().copied())
            .collect();
        let cli = Cli::try_parse_from(&full_args).expect("args parse");
        // Build a QueryPath as if already resolved to the fixture crate
        let query_path = if path.is_empty() {
            QueryPath {
                crate_spec: CrateSpec::CurrentCrate,
                item_segments: Vec::new(),
            }
        } else {
            QueryPath {
                crate_spec: CrateSpec::CurrentCrate,
                item_segments: path.split("::").map(String::from).collect(),
            }
        };
        let kind_filter = cli.kind.map(ItemKind::from);
        let features = FeatureFlags::from_cli(&cli);
        let feature_suffix = features.cache_suffix();

        let result = resolve_item(
            &query_path,
            index,
            kind_filter,
            &features,
            &feature_suffix,
            cli.private,
        );

        let mut stdout_buf = Vec::new();

        if cli.search.is_some() {
            let search_query = cli.search.as_deref().unwrap();
            match handle_search(&mut stdout_buf, index, search_query, &cli) {
                Ok(()) => {
                    let output = String::from_utf8(stdout_buf).expect("valid utf8");
                    return (Ok(output), String::new());
                }
                Err(e) => return (Err(e), String::new()),
            }
        }

        if cli.source {
            let source = CrateSource::CurrentCrate {
                manifest_path: std::path::PathBuf::from("/tmp/Cargo.toml"),
                name: "groxide_test_api".to_string(),
                version: "0.1.0".to_string(),
            };
            match handle_source(&mut stdout_buf, &result, index, &source) {
                Ok(()) => {
                    let output = String::from_utf8(stdout_buf).expect("valid utf8");
                    return (Ok(output), String::new());
                }
                Err(e) => return (Err(e), String::new()),
            }
        }

        match handle_output(
            &mut stdout_buf,
            &result,
            index,
            &cli,
            None,
            &features,
            &feature_suffix,
        ) {
            Ok(()) => {
                let output = String::from_utf8(stdout_buf).expect("valid utf8");
                (Ok(output), String::new())
            }
            Err(e) => (Err(e), String::new()),
        }
    }

    use clap::Parser;

    // ---- Basic query returns expected output ----

    #[test]
    fn basic_query_returns_expected_output() {
        let index = load_fixture_index();
        let (result, _) = query_fixture(&index, "GenericStruct", &["GenericStruct"]);
        let output = result.expect("query should succeed");
        assert!(
            output.contains("GenericStruct"),
            "output should contain GenericStruct: {output}"
        );
        assert!(
            output.contains("struct"),
            "output should mention struct: {output}"
        );
    }

    // ---- Unknown item returns exit code 1 ----

    #[test]
    fn unknown_item_returns_not_found() {
        let index = load_fixture_index();
        let (result, _) = query_fixture(&index, "NonexistentItem", &["NonexistentItem"]);
        match result {
            Err(GroxError::ItemNotFound { query, .. }) => {
                assert!(
                    query.contains("NonexistentItem"),
                    "error should contain the query: {query}"
                );
            }
            other => panic!("expected ItemNotFound, got {other:?}"),
        }
    }

    // ---- Crate root query (no item segments) ----

    #[test]
    fn crate_root_query_returns_module_listing() {
        let index = load_fixture_index();
        let query_path = QueryPath::parse("").expect("parses");
        let features = FeatureFlags {
            all_features: false,
            no_default_features: false,
            features: Vec::new(),
        };
        let feature_suffix = features.cache_suffix();

        // For crate root, item_segments is empty
        let result = resolve_item(&query_path, &index, None, &features, &feature_suffix, false);

        let mut buf = Vec::new();
        let cli = Cli::try_parse_from(["grox"]).expect("parses");
        let r = handle_output(
            &mut buf,
            &result,
            &index,
            &cli,
            None,
            &features,
            &feature_suffix,
        );
        assert!(r.is_ok(), "crate root query should succeed");
        let output = String::from_utf8(buf).expect("valid utf8");
        assert!(
            output.contains("groxide_test_api"),
            "should contain crate name: {output}"
        );
    }

    // ---- Search query returns results ----

    #[test]
    fn search_query_returns_results() {
        let index = load_fixture_index();
        let cli = Cli::try_parse_from(["grox", "-S", "add"]).expect("parses");
        let mut buf = Vec::new();
        let result = handle_search(&mut buf, &index, "add", &cli);
        assert!(result.is_ok(), "search should succeed");
        let output = String::from_utf8(buf).expect("valid utf8");
        assert!(
            output.contains("add"),
            "search results should contain 'add': {output}"
        );
        assert!(
            output.contains("results for"),
            "should have result count header: {output}"
        );
    }

    // ---- List mode output ----

    #[test]
    fn list_mode_produces_output() {
        let index = load_fixture_index();
        let (result, _) = query_fixture(&index, "GenericStruct", &["--list", "GenericStruct"]);
        let output = result.expect("list mode should succeed");
        assert!(
            output.contains("fn"),
            "list should show fn kind for methods: {output}"
        );
    }

    // ---- JSON mode output ----

    #[test]
    fn json_mode_produces_valid_json() {
        let index = load_fixture_index();
        let (result, _) = query_fixture(&index, "GenericStruct", &["--json", "GenericStruct"]);
        let output = result.expect("json mode should succeed");
        // Should be valid JSON
        let parsed: serde_json::Value =
            serde_json::from_str(&output).expect("output should be valid JSON");
        assert_eq!(parsed["kind"], "struct");
        assert!(
            parsed["path"]
                .as_str()
                .unwrap_or("")
                .contains("GenericStruct"),
            "JSON should contain GenericStruct path"
        );
    }

    // ---- resolve_crate_source with CurrentCrate spec ----

    #[test]
    fn resolve_crate_source_returns_error_without_context() {
        let query = QueryPath::parse("").expect("parses");
        let result = resolve_crate_source(None, query);
        assert!(result.is_err(), "should fail without project context");
    }

    // ---- Single-segment reinterpretation ----

    #[test]
    fn resolve_crate_source_reinterprets_item_like_name() {
        let ctx = ProjectContext::discover(None).ok();
        if ctx.is_none() {
            return; // Can't test without project context
        }
        let query = QueryPath::parse("Mutex").expect("parses");
        let (source, new_query) = resolve_crate_source(ctx.as_ref(), query).expect("resolves");
        // "Mutex" looks like an item name, should be reinterpreted as current crate item
        assert!(
            matches!(source, CrateSource::CurrentCrate { .. }),
            "should reinterpret as current crate: {source:?}"
        );
        assert_eq!(new_query.item_segments, vec!["Mutex"]);
    }

    // ---- Function query returns leaf output ----

    #[test]
    fn function_query_returns_leaf_output() {
        let index = load_fixture_index();
        let (result, _) = query_fixture(&index, "add", &["add"]);
        let output = result.expect("function query should succeed");
        assert!(output.contains("fn"), "should be a function: {output}");
        assert!(
            output.contains("add"),
            "should contain function name: {output}"
        );
    }

    // ---- Enum query shows variants ----

    #[test]
    fn enum_query_shows_variants() {
        let index = load_fixture_index();
        let (result, _) = query_fixture(&index, "Direction", &["Direction"]);
        let output = result.expect("enum query should succeed");
        assert!(
            output.contains("Variants:") || output.contains("North"),
            "enum should show variants: {output}"
        );
    }

    // ---- Module query shows children ----

    #[test]
    fn module_query_shows_children() {
        let index = load_fixture_index();
        let (result, _) = query_fixture(&index, "containers", &["containers"]);
        let output = result.expect("module query should succeed");
        assert!(!output.is_empty(), "module query should produce output");
    }

    // ---- Constant query ----

    #[test]
    fn constant_query_returns_output() {
        let index = load_fixture_index();
        let (result, _) = query_fixture(&index, "MAX_BUFFER_SIZE", &["MAX_BUFFER_SIZE"]);
        let output = result.expect("constant query should succeed");
        assert!(output.contains("const"), "should show const: {output}");
    }

    // ---- Search with JSON mode ----

    #[test]
    fn search_json_mode_produces_valid_json_lines() {
        let index = load_fixture_index();
        let cli = Cli::try_parse_from(["grox", "-S", "add", "--json"]).expect("parses");
        let mut buf = Vec::new();
        let result = handle_search(&mut buf, &index, "add", &cli);
        assert!(result.is_ok(), "search should succeed");
        let output = String::from_utf8(buf).expect("valid utf8");
        // Each line should be valid JSON
        for line in output.lines() {
            let parsed: serde_json::Value =
                serde_json::from_str(line).expect("each line should be valid JSON");
            assert!(parsed.get("score").is_some(), "should have score field");
        }
    }

    // ---- Empty search query returns error ----

    #[test]
    fn empty_search_query_returns_error() {
        let index = load_fixture_index();
        let cli = Cli::try_parse_from(["grox", "-S", ""]).expect("parses");
        let mut buf = Vec::new();
        let result = handle_search(&mut buf, &index, "", &cli);
        assert!(result.is_err(), "empty search should fail");
    }
}
