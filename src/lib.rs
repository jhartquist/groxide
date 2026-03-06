mod cache;
pub mod cli;
mod docgen;
pub mod error;
mod external;
mod index_builder;
mod query;
mod reexport;
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
use types::{DisplayLimits, DocIndex, ItemKind, QueryResult};

/// Runs the groxide CLI with the given parsed arguments.
///
/// Returns `Ok(())` on success, `Err(GroxError)` on failure.
///
/// # Errors
///
/// Returns `GroxError` if crate resolution, doc generation, or querying fails.
///
/// # Panics
///
/// Panics if virtual workspace detection succeeds but the context is unexpectedly `None`
/// (should be unreachable).
pub fn run(cli: &Cli) -> Result<()> {
    if cli.clear_cache {
        if let Some(path) = cache::clear_global_cache() {
            eprintln!("[grox] Cleared cache at {}", path.display());
        } else {
            eprintln!("[grox] No cache directory found");
        }
        return Ok(());
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // Step 1: Discover project context
    let ctx = if cli.manifest_path.is_some() {
        Some(ProjectContext::discover(cli.manifest_path.as_deref())?)
    } else {
        ProjectContext::discover(None).ok()
    };

    // Step 1b: Virtual workspace detection — show all workspace members
    if cli.path.is_none()
        && cli.search.is_none()
        && !cli.readme
        && ctx
            .as_ref()
            .is_some_and(ProjectContext::is_virtual_workspace)
    {
        // SAFETY: is_some_and guarantees ctx is Some.
        let ctx = ctx.as_ref().expect("invariant: checked is_some");
        return handle_workspace(&mut out, ctx, cli);
    }

    // Step 2: Parse query path
    let query_path = QueryPath::parse(cli.path.as_deref().unwrap_or(""))?;

    // Step 3: Resolve crate source
    let (source, query_path) = resolve_crate_source(ctx.as_ref(), query_path)?;

    // Step 4: Load or build DocIndex
    let features = FeatureFlags::from_cli(cli);
    let feature_suffix = features.cache_suffix();
    let (index, source) =
        load_or_build_index(source, &features, &feature_suffix, cli.private, false)?;

    // Step 5: Handle --readme (early return)
    if cli.readme {
        handle_readme(&mut out, &source, ctx.as_ref())?;
    } else if let Some(search_query) = &cli.search {
        // Step 6: Handle --search
        handle_search(
            &mut out,
            &index,
            search_query,
            cli,
            Some(&source),
            &features,
        )?;
    } else {
        // Step 7: Resolve item in index
        // When --recursive is set, don't filter by kind during resolution —
        // the kind filter is applied to the recursive listing instead.
        let kind_filter = if cli.recursive {
            None
        } else {
            cli.kind.map(ItemKind::from)
        };
        let mut result = resolve_item(&query_path, &index, kind_filter);

        // Step 7b: On NotFound, try resolving via re-export stubs
        if matches!(result, QueryResult::NotFound { .. }) {
            if let Some(resolved) =
                reexport::try_resolve_reexport_on_not_found(&query_path, &index, kind_filter)
            {
                result = resolved;
            }
        }

        if cli.recursive && cli.source {
            // Step 8a: Handle --recursive --source (dump everything)
            handle_recursive_source(
                &mut out,
                &result,
                &index,
                &source,
                cli,
                ctx.as_ref(),
                &features,
                &feature_suffix,
            )?;
        } else if cli.source {
            // Step 8b: Handle --source
            let kind_filter = cli.kind.map(ItemKind::from);
            handle_source(&mut out, &result, &index, &source, cli.docs, kind_filter)?;
        } else {
            // Step 9: Render output
            render::dispatch::handle_output(
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
pub(crate) fn resolve_crate_source(
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
                        CrateSource::CurrentCrate { .. }
                        | CrateSource::Dependency { .. }
                        | CrateSource::Stdlib { .. } => None,
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
pub(crate) fn load_or_build_index(
    source: CrateSource,
    features: &FeatureFlags,
    feature_suffix: &str,
    private: bool,
    quiet: bool,
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
    if !quiet {
        eprint!("[grox] Building index for {name} {version}...");
    }

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
    if !quiet {
        eprintln!(" done ({elapsed:.1}s)");
    }

    Ok((index, source))
}

/// Resolves an item with all fallback strategies.
pub(crate) fn resolve_item(
    query: &QueryPath,
    index: &DocIndex,
    kind_filter: Option<ItemKind>,
) -> QueryResult {
    let crate_name = &index.crate_name;

    // If no item segments, look up crate root.
    // Skip kind filter here — the root is always a module, and the kind filter
    // should apply to the children listing, not reject the root itself.
    if query.item_segments.is_empty() {
        let result = query::lookup(index, crate_name, None);
        if !matches!(result, QueryResult::NotFound { .. }) {
            return result;
        }
        // Crate root not found by name, try hyphen-normalized
        let normalized = crate_name.replace('-', "_");
        if normalized != *crate_name {
            let result = query::lookup(index, &normalized, None);
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
///
/// When the search returns 0 results and the crate has non-default features,
/// rebuilds the index with `--all-features` and hints the user if more items
/// are found.
fn handle_search(
    w: &mut impl Write,
    index: &DocIndex,
    search_query: &str,
    cli: &Cli,
    source: Option<&CrateSource>,
    features: &FeatureFlags,
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

    // Hint: if 0 results and not already using --all-features, try rebuilding
    // with all features to see if feature-gated items match.
    // Skip external crates (rebuilding is too slow).
    if total == 0 && !features.all_features {
        if let Some(src) = source {
            if !matches!(src, CrateSource::External { .. }) {
                let all_features = FeatureFlags {
                    all_features: true,
                    no_default_features: false,
                    features: Vec::new(),
                };
                let all_suffix = all_features.cache_suffix();
                if let Ok((all_index, _)) =
                    load_or_build_index(src.clone(), &all_features, &all_suffix, cli.private, false)
                {
                    let all_results = search::search(&all_index, search_query, kind_filter)?;
                    if !all_results.is_empty() {
                        eprintln!(
                            "hint: {} items found with --all-features",
                            all_results.len()
                        );
                    }
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
fn handle_recursive_source(
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

/// Handles workspace-wide querying when no path is given in a virtual workspace.
///
/// Builds all workspace member indices first (single progress line), then
/// renders each crate's top-level view separated by blank lines.
fn handle_workspace(w: &mut impl Write, ctx: &ProjectContext, cli: &Cli) -> Result<()> {
    let members = ctx.workspace_member_packages();
    let features = FeatureFlags::from_cli(cli);
    let feature_suffix = features.cache_suffix();

    // Filter to library crates only (rustdoc can't generate docs for binary-only crates)
    let lib_members: Vec<_> = members
        .into_iter()
        .filter(|pkg| pkg.targets.iter().any(cargo_metadata::Target::is_lib))
        .collect();

    // Phase 1: Build all indices with a single progress line
    let start = Instant::now();
    eprint!("[grox] Building workspace indices...");

    let mut built: Vec<(&cargo_metadata::Package, DocIndex, CrateSource)> = Vec::new();
    let mut errors: Vec<(String, GroxError)> = Vec::new();

    for pkg in &lib_members {
        let source = CrateSource::CurrentCrate {
            manifest_path: pkg.manifest_path.clone().into_std_path_buf(),
            name: pkg.name.to_string(),
            version: pkg.version.to_string(),
        };

        match load_or_build_index(source, &features, &feature_suffix, cli.private, true) {
            Ok((index, source)) => built.push((pkg, index, source)),
            Err(e) => errors.push((pkg.name.to_string(), e)),
        }
    }

    let elapsed = start.elapsed().as_secs_f64();
    eprintln!(" done ({elapsed:.1}s)");

    for (name, e) in &errors {
        eprintln!("[grox] Failed to build index for {name}: {e}");
    }

    // Phase 2: Render all results
    let mut first = true;
    for (pkg, index, source) in &built {
        let query_path = QueryPath {
            crate_spec: CrateSpec::CurrentCrate,
            item_segments: Vec::new(),
        };
        let result = resolve_item(&query_path, index, None);

        if !first {
            // Double blank line between crates for visual separation
            write!(w, "\n\n").map_err(GroxError::Io)?;
        }
        first = false;

        match result {
            QueryResult::Found { index: idx } => {
                if cli.recursive && cli.source {
                    render::dispatch::render_recursive_source(w, index, idx, source, cli)?;
                } else if cli.recursive {
                    render::dispatch::render_recursive(w, index, idx, cli)?;
                } else {
                    let kind_filter = cli.kind.map(ItemKind::from);
                    let display = render::build_display_item(index, idx, cli.private, kind_filter);
                    if cli.json {
                        let output = render::json::render_json(&display);
                        writeln!(w, "{output}").map_err(GroxError::Io)?;
                    } else if cli.brief {
                        let output = render::brief::render_brief(&display);
                        writeln!(w, "{output}").map_err(GroxError::Io)?;
                    } else {
                        let limits = DisplayLimits::default();
                        let output = render::text::render_text(&display, &limits);
                        writeln!(w, "{output}").map_err(GroxError::Io)?;
                    }
                }
            }
            QueryResult::NotFound { .. } | QueryResult::Ambiguous { .. } => {
                eprintln!("[grox] Could not resolve crate root for {}", pkg.name);
            }
        }
    }

    Ok(())
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

        let mut result = resolve_item(&query_path, index, kind_filter);

        // Mirror the re-export fallback from run()
        if matches!(result, QueryResult::NotFound { .. }) {
            if let Some(resolved) =
                reexport::try_resolve_reexport_on_not_found(&query_path, index, kind_filter)
            {
                result = resolved;
            }
        }

        let mut stdout_buf = Vec::new();

        if cli.search.is_some() {
            let search_query = cli.search.as_deref().unwrap();
            match handle_search(&mut stdout_buf, index, search_query, &cli, None, &features) {
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
            let kind_filter = cli.kind.map(ItemKind::from);
            match handle_source(
                &mut stdout_buf,
                &result,
                index,
                &source,
                cli.docs,
                kind_filter,
            ) {
                Ok(()) => {
                    let output = String::from_utf8(stdout_buf).expect("valid utf8");
                    return (Ok(output), String::new());
                }
                Err(e) => return (Err(e), String::new()),
            }
        }

        match render::dispatch::handle_output(
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
        let result = resolve_item(&query_path, &index, None);

        let mut buf = Vec::new();
        let cli = Cli::try_parse_from(["grox"]).expect("parses");
        let r = render::dispatch::handle_output(
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
        let features = FeatureFlags::from_cli(&cli);
        let result = handle_search(&mut buf, &index, "add", &cli, None, &features);
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
            output.contains("variants:") || output.contains("North"),
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
        let features = FeatureFlags::from_cli(&cli);
        let result = handle_search(&mut buf, &index, "add", &cli, None, &features);
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
        let features = FeatureFlags::from_cli(&cli);
        let result = handle_search(&mut buf, &index, "", &cli, None, &features);
        assert!(result.is_err(), "empty search should fail");
    }

    // ---- Re-export resolution on NotFound ----

    #[test]
    fn resolve_reexport_finds_item_by_name_when_path_not_found() {
        let index = load_fixture_index();
        // Query with a wrong module prefix — the item exists as Helper under reexports
        let query_path = QueryPath {
            crate_spec: CrateSpec::CurrentCrate,
            item_segments: vec!["nonexistent_mod".to_string(), "Helper".to_string()],
        };
        let result = reexport::try_resolve_reexport_on_not_found(&query_path, &index, None);
        assert!(
            result.is_some(),
            "should find Helper via re-export fallback"
        );
        let idx = match result.unwrap() {
            QueryResult::Found { index: idx } => idx,
            QueryResult::Ambiguous { indices, .. } => indices[0],
            other @ QueryResult::NotFound { .. } => {
                panic!("expected Found or Ambiguous, got {other:?}")
            }
        };
        assert_eq!(index.items[idx].name, "Helper");
    }

    #[test]
    fn resolve_reexport_returns_none_for_truly_missing_item() {
        let index = load_fixture_index();
        let query_path = QueryPath {
            crate_spec: CrateSpec::CurrentCrate,
            item_segments: vec!["TotallyFakeItem99".to_string()],
        };
        let result = reexport::try_resolve_reexport_on_not_found(&query_path, &index, None);
        assert!(
            result.is_none(),
            "should return None for truly missing item"
        );
    }
}
