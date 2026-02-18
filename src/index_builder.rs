use std::collections::{HashMap, HashSet};

use rustdoc_types::{Crate, Id, ItemEnum, Visibility};
use serde::Deserialize;

use crate::signature::render_signature;
use crate::types::{ChildRef, DocIndex, IndexItem, ItemKind, SourceSpan, TraitImplInfo};

/// Parses rustdoc JSON with disabled recursion limit.
///
/// Uses `serde_json::Deserializer::from_str` with recursion limit disabled
/// to handle deeply nested types (e.g., `typenum`). Safe because the input
/// is trusted output from `cargo rustdoc`.
pub(crate) fn parse_rustdoc_json(json: &str) -> crate::error::Result<Crate> {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    deserializer.disable_recursion_limit();
    Crate::deserialize(&mut deserializer).map_err(|e| crate::error::GroxError::JsonParseFailed {
        details: e.to_string(),
    })
}

/// Builds a `DocIndex` from a parsed rustdoc `Crate`.
///
/// Runs four sequential passes:
/// 1. Parent map construction (child → parent reverse lookup)
/// 2. Path computation (seed from `krate.paths`, glob re-export hoisting, impl/trait paths, fallback)
/// 3. Item conversion (`ItemEnum` → `IndexItem`, visibility, re-exports, feature gates)
/// 4. Children & relationships (impl methods, trait impls, module children)
pub(crate) fn build_index(krate: &Crate, crate_name: &str, crate_version: &str) -> DocIndex {
    let mut builder = IndexBuilder {
        krate,
        index: DocIndex::new(crate_name.to_string(), crate_version.to_string()),
        id_to_index: HashMap::new(),
        id_to_path: HashMap::new(),
        blanket_impl_items: HashSet::new(),
        child_to_parent: HashMap::new(),
    };

    builder.pass1_build_parent_map();
    builder.pass2_compute_paths();
    builder.pass3_convert_items();
    builder.pass4_link_relationships();

    builder.index
}

struct IndexBuilder<'a> {
    krate: &'a Crate,
    index: DocIndex,
    id_to_index: HashMap<Id, usize>,
    id_to_path: HashMap<Id, String>,
    blanket_impl_items: HashSet<Id>,
    child_to_parent: HashMap<Id, Id>,
}

impl IndexBuilder<'_> {
    // ---- Pass 1: Parent Map ----

    fn pass1_build_parent_map(&mut self) {
        for (parent_id, item) in &self.krate.index {
            let child_ids = self.collect_child_ids_for_parent(item);
            for child_id in child_ids {
                self.child_to_parent.insert(child_id, *parent_id);
            }
        }
    }

    fn collect_child_ids_for_parent(&self, item: &rustdoc_types::Item) -> Vec<Id> {
        match &item.inner {
            ItemEnum::Module(m) => m.items.clone(),
            ItemEnum::Struct(s) => {
                let mut ids = struct_field_ids(s);
                self.extend_with_impl_items(&mut ids, &s.impls);
                ids
            }
            ItemEnum::Enum(e) => {
                let mut ids = e.variants.clone();
                self.extend_with_impl_items(&mut ids, &e.impls);
                ids
            }
            ItemEnum::Union(u) => {
                let mut ids = Vec::new();
                self.extend_with_impl_items(&mut ids, &u.impls);
                ids
            }
            ItemEnum::Trait(t) => t.items.clone(),
            ItemEnum::Impl(i) => i.items.clone(),
            _ => Vec::new(),
        }
    }

    fn extend_with_impl_items(&self, ids: &mut Vec<Id>, impls: &[Id]) {
        for impl_id in impls {
            if let Some(impl_item) = self.krate.index.get(impl_id) {
                if let ItemEnum::Impl(impl_data) = &impl_item.inner {
                    ids.extend(impl_data.items.iter().copied());
                }
            }
        }
    }

    // ---- Pass 2: Path Computation ----

    fn pass2_compute_paths(&mut self) {
        // 2.1: Seed from krate.paths
        for (id, summary) in &self.krate.paths {
            self.id_to_path.insert(*id, summary.path.join("::"));
        }

        // 2.2: Glob re-export hoisting (must run before impl path computation)
        self.hoist_glob_reexport_paths();

        // 2.3: Impl block path computation
        self.compute_impl_paths();

        // 2.4: Trait item path computation
        self.compute_trait_item_paths();

        // 2.5: Fallback — reconstruct remaining paths via parent chain
        let missing_ids: Vec<(Id, String)> = self
            .krate
            .index
            .iter()
            .filter(|(id, _)| !self.id_to_path.contains_key(*id))
            .filter_map(|(id, item)| self.reconstruct_path(*id, item).map(|path| (*id, path)))
            .collect();

        for (id, path) in missing_ids {
            self.id_to_path.entry(id).or_insert(path);
        }
    }

    fn hoist_glob_reexport_paths(&mut self) {
        let mut overrides = Vec::new();

        for (id, item) in &self.krate.index {
            let ItemEnum::Module(m) = &item.inner else {
                continue;
            };
            let Some(parent_path) = self.id_to_path.get(id) else {
                continue;
            };
            let parent_path = parent_path.clone();

            let mut sorted_children: Vec<_> = m.items.clone();
            sorted_children.sort();

            for child_id in &sorted_children {
                let Some(child) = self.krate.index.get(child_id) else {
                    continue;
                };
                let ItemEnum::Use(use_item) = &child.inner else {
                    continue;
                };
                if !use_item.is_glob {
                    continue;
                }
                let Some(target_id) = &use_item.id else {
                    continue;
                };
                let Some(target) = self.krate.index.get(target_id) else {
                    continue;
                };
                let ItemEnum::Module(target_module) = &target.inner else {
                    continue;
                };

                let mut sorted_target_children: Vec<_> = target_module.items.clone();
                sorted_target_children.sort();

                for tc_id in &sorted_target_children {
                    let Some(tc) = self.krate.index.get(tc_id) else {
                        continue;
                    };
                    let Some(tc_name) = &tc.name else {
                        continue;
                    };
                    overrides.push((*tc_id, format!("{parent_path}::{tc_name}")));
                }
            }
        }

        for (id, path) in overrides {
            self.id_to_path.insert(id, path);
        }
    }

    fn compute_impl_paths(&mut self) {
        let mut new_paths = Vec::new();

        for item in self.krate.index.values() {
            let ItemEnum::Impl(impl_data) = &item.inner else {
                continue;
            };
            let Some(parent_path) = self.resolve_type_path(&impl_data.for_) else {
                continue;
            };

            for child_id in &impl_data.items {
                if self.id_to_path.contains_key(child_id) {
                    continue;
                }
                let Some(child) = self.krate.index.get(child_id) else {
                    continue;
                };
                let Some(child_name) = &child.name else {
                    continue;
                };
                new_paths.push((*child_id, format!("{parent_path}::{child_name}")));
            }
        }

        for (id, path) in new_paths {
            self.id_to_path.entry(id).or_insert(path);
        }
    }

    fn resolve_type_path(&self, ty: &rustdoc_types::Type) -> Option<String> {
        match ty {
            rustdoc_types::Type::ResolvedPath(path) => self
                .id_to_path
                .get(&path.id)
                .cloned()
                .or_else(|| Some(path.path.clone())),
            _ => None,
        }
    }

    fn compute_trait_item_paths(&mut self) {
        let mut new_paths = Vec::new();

        for (trait_id, item) in &self.krate.index {
            let ItemEnum::Trait(t) = &item.inner else {
                continue;
            };
            let Some(trait_path) = self.id_to_path.get(trait_id) else {
                continue;
            };
            let trait_path = trait_path.clone();

            for child_id in &t.items {
                if self.id_to_path.contains_key(child_id) {
                    continue;
                }
                let Some(child) = self.krate.index.get(child_id) else {
                    continue;
                };
                let Some(child_name) = &child.name else {
                    continue;
                };
                new_paths.push((*child_id, format!("{trait_path}::{child_name}")));
            }
        }

        for (id, path) in new_paths {
            self.id_to_path.entry(id).or_insert(path);
        }
    }

    fn reconstruct_path(&self, id: Id, item: &rustdoc_types::Item) -> Option<String> {
        let item_name = item.name.as_deref()?;
        let mut segments = vec![item_name.to_string()];
        let mut current_id = id;
        let mut depth = 0;

        loop {
            depth += 1;
            if depth > 20 {
                break;
            }

            let Some(&parent_id) = self.child_to_parent.get(&current_id) else {
                break;
            };
            let Some(parent_item) = self.krate.index.get(&parent_id) else {
                break;
            };

            if let Some(parent_path) = self.id_to_path.get(&parent_id) {
                return Some(format!("{}::{}", parent_path, segments.join("::")));
            }

            let Some(parent_name) = &parent_item.name else {
                break;
            };
            segments.insert(0, parent_name.clone());
            current_id = parent_id;
        }

        Some(segments.join("::"))
    }

    // ---- Pass 3: Item Conversion ----

    fn pass3_convert_items(&mut self) {
        self.collect_blanket_impl_items();

        // Sort items by ID for deterministic order
        let mut sorted_items: Vec<_> = self.krate.index.iter().collect();
        sorted_items.sort_by(|(a, _), (b, _)| a.cmp(b));

        for (id, item) in sorted_items {
            if self.blanket_impl_items.contains(id) {
                continue;
            }

            let index_item = if let ItemEnum::Use(_) = &item.inner {
                self.convert_use_item(*id, item)
            } else {
                self.convert_regular_item(*id, item)
            };

            if let Some(index_item) = index_item {
                let idx = self.index.items.len();
                self.id_to_index.insert(*id, idx);
                self.index.add_item(index_item);
            }
        }
    }

    fn collect_blanket_impl_items(&mut self) {
        for item in self.krate.index.values() {
            if let ItemEnum::Impl(impl_data) = &item.inner {
                if impl_data.trait_.is_some()
                    && (impl_data.blanket_impl.is_some() || impl_data.is_synthetic)
                {
                    for child_id in &impl_data.items {
                        self.blanket_impl_items.insert(*child_id);
                    }
                }
            }
        }
    }

    fn convert_regular_item(&self, id: Id, item: &rustdoc_types::Item) -> Option<IndexItem> {
        let path = self.id_to_path.get(&id)?.clone();
        let name = item.name.as_deref()?.to_string();
        let kind = convert_item_kind(&item.inner)?;
        let signature = render_signature(item, self.krate)
            .unwrap_or_else(|| fallback_signature(&item.visibility, kind, &name));
        let docs = item.docs.clone().unwrap_or_default();
        let summary = extract_summary(&docs);
        let span = extract_span(item);
        let is_public = check_visibility(item);
        let has_body = matches!(&item.inner, ItemEnum::Function(f) if f.has_body);
        let feature_gate = extract_feature_gate(item);

        Some(IndexItem {
            path,
            name,
            kind,
            signature,
            docs,
            summary,
            span,
            children: Vec::new(),
            is_public,
            has_body,
            feature_gate,
            reexport_source: None,
        })
    }

    fn convert_use_item(&self, id: Id, item: &rustdoc_types::Item) -> Option<IndexItem> {
        let ItemEnum::Use(use_item) = &item.inner else {
            return None;
        };

        // Skip glob re-exports (handled by path hoisting and module child resolution)
        if use_item.is_glob {
            return None;
        }

        // Skip non-public items
        if !matches!(item.visibility, Visibility::Public) {
            return None;
        }

        let name = use_item.name.clone();
        if name.is_empty() {
            return None;
        }

        // Deduplication: if the referenced item already has the same path, skip
        if let Some(ref_id) = &use_item.id {
            if let Some(existing_path) = self.id_to_path.get(ref_id) {
                if let Some(this_path) = self.id_to_path.get(&id) {
                    if this_path == existing_path {
                        return None;
                    }
                }
            }
        }

        // Build path: try id_to_path, then parent module, then crate root
        let path = self.id_to_path.get(&id).cloned().unwrap_or_else(|| {
            // Try parent module path
            if let Some(parent_id) = self.child_to_parent.get(&id) {
                if let Some(parent_path) = self.id_to_path.get(parent_id) {
                    return format!("{parent_path}::{name}");
                }
            }
            // Fallback to crate root
            let root_path = self
                .id_to_path
                .get(&self.krate.root)
                .cloned()
                .unwrap_or_default();
            format!("{root_path}::{name}")
        });

        let kind = self.resolve_use_kind(use_item);

        // Build source path for docs/signature
        let source = use_item
            .id
            .as_ref()
            .and_then(|ref_id| self.id_to_path.get(ref_id))
            .cloned()
            .unwrap_or_else(|| use_item.source.clone());

        // Try to get real signature/docs from the referenced item (in-crate re-export)
        let (signature, docs, summary, has_body) =
            if let Some(ref_item) = use_item.id.as_ref().and_then(|id| self.krate.index.get(id)) {
                // In-crate re-export: use the real signature and docs
                let sig = render_signature(ref_item, self.krate)
                    .unwrap_or_else(|| fallback_signature(&ref_item.visibility, kind, &name));

                // Use the pub use item's own docs if present, otherwise the referenced item's docs
                let docs = if item.docs.is_some() {
                    item.docs.clone().unwrap_or_default()
                } else {
                    ref_item.docs.clone().unwrap_or_default()
                };

                let summary = extract_summary(&docs);
                let has_body = matches!(&ref_item.inner, ItemEnum::Function(f) if f.has_body);
                (sig, docs, summary, has_body)
            } else {
                // Cross-crate re-export: keep stub signature
                let docs = item.docs.clone().unwrap_or_default();
                let summary = extract_summary(&docs);
                let signature = format!("pub use {source} as {name}");
                (signature, docs, summary, false)
            };

        let feature_gate = extract_feature_gate(item);

        Some(IndexItem {
            path,
            name,
            kind,
            signature,
            docs,
            summary,
            span: extract_span(item),
            children: Vec::new(),
            is_public: true,
            has_body,
            feature_gate,
            reexport_source: Some(source),
        })
    }

    fn resolve_use_kind(&self, use_item: &rustdoc_types::Use) -> ItemKind {
        if let Some(ref_id) = &use_item.id {
            if let Some(ref_item) = self.krate.index.get(ref_id) {
                if let Some(kind) = convert_item_kind(&ref_item.inner) {
                    return kind;
                }
            }
            if let Some(summary) = self.krate.paths.get(ref_id) {
                if let Some(kind) = convert_item_summary_kind(summary.kind) {
                    return kind;
                }
            }
        }
        ItemKind::Struct
    }

    // ---- Pass 4: Children & Relationships ----

    fn pass4_link_relationships(&mut self) {
        let krate_items: Vec<_> = self.krate.index.iter().collect();

        for (id, item) in &krate_items {
            let Some(&parent_idx) = self.id_to_index.get(*id) else {
                continue;
            };

            let resolved_ids = match &item.inner {
                ItemEnum::Module(m) => self.resolve_module_children(m),
                ItemEnum::Struct(s) => {
                    let mut ids = struct_field_ids(s);
                    ids.extend(self.resolve_inherent_impl_items(&s.impls));
                    ids
                }
                ItemEnum::Enum(e) => {
                    let mut ids = e.variants.clone();
                    ids.extend(self.resolve_inherent_impl_items(&e.impls));
                    ids
                }
                ItemEnum::Union(u) => self.resolve_inherent_impl_items(&u.impls),
                ItemEnum::Trait(t) => t.items.clone(),
                ItemEnum::Use(use_item) => {
                    // For in-crate re-exports, copy children from the referenced item
                    self.resolve_use_children(use_item, parent_idx);
                    continue;
                }
                _ => Vec::new(),
            };

            let mut children = Vec::new();
            for cid in &resolved_ids {
                let Some(&cidx) = self.id_to_index.get(cid) else {
                    continue;
                };
                let child_item = &self.index.items[cidx];
                children.push(ChildRef {
                    index: cidx,
                    kind: child_item.kind,
                    name: child_item.name.clone(),
                });
            }

            if !children.is_empty() {
                self.index.items[parent_idx].children = children;
            }

            let trait_impls = self.extract_trait_impls(item);
            if !trait_impls.is_empty() {
                self.index.trait_impls.insert(parent_idx, trait_impls);
            }
        }
    }

    fn resolve_module_children(&self, module: &rustdoc_types::Module) -> Vec<Id> {
        let mut result = Vec::new();
        for child_id in &module.items {
            let Some(child) = self.krate.index.get(child_id) else {
                continue;
            };
            if let ItemEnum::Use(use_item) = &child.inner {
                if use_item.is_glob {
                    if let Some(target_id) = &use_item.id {
                        if let Some(target) = self.krate.index.get(target_id) {
                            if let ItemEnum::Module(target_module) = &target.inner {
                                result.extend(target_module.items.iter().copied());
                            }
                        }
                    }
                    continue; // skip the Use item itself
                }
            }
            result.push(*child_id);
        }
        result
    }

    /// Copies children and trait impls from a referenced item to a `Use` re-export.
    fn resolve_use_children(&mut self, use_item: &rustdoc_types::Use, parent_idx: usize) {
        let Some(ref_id) = &use_item.id else {
            return;
        };
        let Some(ref_item) = self.krate.index.get(ref_id) else {
            return;
        };

        // Collect child IDs from the referenced item (struct fields, enum variants, impl methods)
        let child_ids = match &ref_item.inner {
            ItemEnum::Struct(s) => {
                let mut ids = struct_field_ids(s);
                ids.extend(self.resolve_inherent_impl_items(&s.impls));
                ids
            }
            ItemEnum::Enum(e) => {
                let mut ids = e.variants.clone();
                ids.extend(self.resolve_inherent_impl_items(&e.impls));
                ids
            }
            ItemEnum::Union(u) => self.resolve_inherent_impl_items(&u.impls),
            ItemEnum::Trait(t) => t.items.clone(),
            _ => Vec::new(),
        };

        let mut children = Vec::new();
        for cid in &child_ids {
            let Some(&cidx) = self.id_to_index.get(cid) else {
                continue;
            };
            let child_item = &self.index.items[cidx];
            children.push(ChildRef {
                index: cidx,
                kind: child_item.kind,
                name: child_item.name.clone(),
            });
        }

        if !children.is_empty() {
            self.index.items[parent_idx].children = children;
        }

        // Also copy trait impls from the referenced item
        let trait_impls = self.extract_trait_impls(ref_item);
        if !trait_impls.is_empty() {
            self.index.trait_impls.insert(parent_idx, trait_impls);
        }
    }

    fn resolve_inherent_impl_items(&self, impl_ids: &[Id]) -> Vec<Id> {
        let mut result = Vec::new();
        for impl_id in impl_ids {
            let Some(impl_item) = self.krate.index.get(impl_id) else {
                continue;
            };
            if let ItemEnum::Impl(impl_data) = &impl_item.inner {
                if impl_data.trait_.is_none() {
                    result.extend(impl_data.items.iter().copied());
                }
            }
        }
        result
    }

    fn extract_trait_impls(&self, item: &rustdoc_types::Item) -> Vec<TraitImplInfo> {
        let impls_list = match &item.inner {
            ItemEnum::Struct(s) => &s.impls,
            ItemEnum::Enum(e) => &e.impls,
            ItemEnum::Union(u) => &u.impls,
            _ => return Vec::new(),
        };

        let mut result = Vec::new();
        for impl_id in impls_list {
            let Some(impl_item) = self.krate.index.get(impl_id) else {
                continue;
            };
            let ItemEnum::Impl(impl_data) = &impl_item.inner else {
                continue;
            };
            let Some(trait_ref) = &impl_data.trait_ else {
                continue;
            };
            // Filter out blanket impls
            if impl_data.blanket_impl.is_some() {
                continue;
            }

            result.push(TraitImplInfo {
                trait_path: trait_ref.path.clone(),
                is_synthetic: impl_data.is_synthetic,
            });
        }

        result
    }
}

/// Extracts field IDs from a struct.
fn struct_field_ids(s: &rustdoc_types::Struct) -> Vec<Id> {
    match &s.kind {
        rustdoc_types::StructKind::Plain {
            fields,
            has_stripped_fields: _,
        } => fields.clone(),
        rustdoc_types::StructKind::Tuple(fields) => fields.iter().copied().flatten().collect(),
        rustdoc_types::StructKind::Unit => Vec::new(),
    }
}

/// Converts `ItemEnum` variant to `ItemKind`.
fn convert_item_kind(inner: &ItemEnum) -> Option<ItemKind> {
    match inner {
        ItemEnum::Module(_) => Some(ItemKind::Module),
        ItemEnum::Struct(_) => Some(ItemKind::Struct),
        ItemEnum::Enum(_) => Some(ItemKind::Enum),
        ItemEnum::Union(_) => Some(ItemKind::Union),
        ItemEnum::Trait(_) => Some(ItemKind::Trait),
        ItemEnum::TraitAlias(_) => Some(ItemKind::TraitAlias),
        ItemEnum::Function(_) => Some(ItemKind::Function),
        ItemEnum::TypeAlias(_) => Some(ItemKind::TypeAlias),
        ItemEnum::AssocType { .. } => Some(ItemKind::AssocType),
        ItemEnum::AssocConst { .. } => Some(ItemKind::AssocConst),
        ItemEnum::Constant { .. } => Some(ItemKind::Constant),
        ItemEnum::Static(_) => Some(ItemKind::Static),
        ItemEnum::Macro(_) => Some(ItemKind::Macro),
        ItemEnum::ProcMacro(_) => Some(ItemKind::ProcMacro),
        ItemEnum::Variant(_) => Some(ItemKind::Variant),
        ItemEnum::StructField(_) => Some(ItemKind::Field),
        ItemEnum::ExternType => Some(ItemKind::ForeignType),
        ItemEnum::Primitive(_) => Some(ItemKind::Primitive),
        // Skip: Impl, Use, ExternCrate
        ItemEnum::Impl(_) | ItemEnum::Use(_) | ItemEnum::ExternCrate { .. } => None,
    }
}

/// Converts `rustdoc_types::ItemKind` (from `krate.paths`) to our `ItemKind`.
fn convert_item_summary_kind(kind: rustdoc_types::ItemKind) -> Option<ItemKind> {
    match kind {
        rustdoc_types::ItemKind::Module => Some(ItemKind::Module),
        rustdoc_types::ItemKind::Struct => Some(ItemKind::Struct),
        rustdoc_types::ItemKind::Enum => Some(ItemKind::Enum),
        rustdoc_types::ItemKind::Union => Some(ItemKind::Union),
        rustdoc_types::ItemKind::Trait => Some(ItemKind::Trait),
        rustdoc_types::ItemKind::TraitAlias => Some(ItemKind::TraitAlias),
        rustdoc_types::ItemKind::Function => Some(ItemKind::Function),
        rustdoc_types::ItemKind::TypeAlias => Some(ItemKind::TypeAlias),
        rustdoc_types::ItemKind::AssocType => Some(ItemKind::AssocType),
        rustdoc_types::ItemKind::AssocConst => Some(ItemKind::AssocConst),
        rustdoc_types::ItemKind::Constant => Some(ItemKind::Constant),
        rustdoc_types::ItemKind::Static => Some(ItemKind::Static),
        rustdoc_types::ItemKind::Macro => Some(ItemKind::Macro),
        rustdoc_types::ItemKind::ProcAttribute | rustdoc_types::ItemKind::ProcDerive => {
            Some(ItemKind::ProcMacro)
        }
        rustdoc_types::ItemKind::Variant => Some(ItemKind::Variant),
        rustdoc_types::ItemKind::StructField => Some(ItemKind::Field),
        rustdoc_types::ItemKind::ExternType => Some(ItemKind::ForeignType),
        rustdoc_types::ItemKind::Primitive => Some(ItemKind::Primitive),
        rustdoc_types::ItemKind::ExternCrate
        | rustdoc_types::ItemKind::Use
        | rustdoc_types::ItemKind::Impl
        | rustdoc_types::ItemKind::Keyword
        | rustdoc_types::ItemKind::Attribute => None,
    }
}

/// Checks item visibility according to spec §5.4.
fn check_visibility(item: &rustdoc_types::Item) -> bool {
    match &item.visibility {
        Visibility::Public => true,
        Visibility::Default => {
            // Enum variants are implicitly public; trait methods use Default visibility
            matches!(
                &item.inner,
                ItemEnum::Variant(_) | ItemEnum::Function(_) | ItemEnum::StructField(_)
            )
        }
        Visibility::Crate | Visibility::Restricted { .. } => false,
    }
}

/// Extracts the first sentence from a doc string.
fn extract_summary(docs: &str) -> String {
    if docs.is_empty() {
        return String::new();
    }

    let mut chars = docs.char_indices().peekable();

    while let Some((byte_pos, ch)) = chars.next() {
        if (ch == '!' || ch == '?') && chars.peek().is_none_or(|(_, c)| c.is_whitespace()) {
            return docs[..byte_pos + ch.len_utf8()].to_string();
        }

        if ch == '.' {
            // At end of string
            if chars.peek().is_none() {
                return docs[..=byte_pos].to_string();
            }
            // Followed by whitespace
            if let Some((_, next_ch)) = chars.peek() {
                if next_ch.is_whitespace() {
                    let remaining = &docs[byte_pos + 1..];
                    let next_non_ws = remaining.chars().find(|c| !c.is_whitespace());
                    if next_non_ws.is_none_or(char::is_uppercase) {
                        return docs[..=byte_pos].to_string();
                    }
                }
            }
        }
    }

    // No sentence terminator found: take first line
    let first_line = docs.split('\n').next().unwrap_or(docs);
    if first_line.len() > 100 {
        format!("{}...", &first_line[..100])
    } else {
        first_line.to_string()
    }
}

/// Extracts source span from a rustdoc item.
fn extract_span(item: &rustdoc_types::Item) -> SourceSpan {
    match &item.span {
        Some(span) => SourceSpan {
            file: span.filename.to_string_lossy().to_string(),
            #[allow(clippy::cast_possible_truncation)]
            line_start: span.begin.0 as u32,
            #[allow(clippy::cast_possible_truncation)]
            line_end: span.end.0 as u32,
        },
        None => SourceSpan {
            file: String::new(),
            line_start: 0,
            line_end: 0,
        },
    }
}

/// Extracts a feature gate from item attributes.
fn extract_feature_gate(item: &rustdoc_types::Item) -> Option<String> {
    for attr in &item.attrs {
        if let rustdoc_types::Attribute::Other(s) = attr {
            // Match: #[doc(cfg(feature = "feature_name"))]
            if let Some(start) = s.find("cfg(feature") {
                let rest = &s[start..];
                if let Some(quote_start) = rest.find('"') {
                    let after_quote = &rest[quote_start + 1..];
                    if let Some(quote_end) = after_quote.find('"') {
                        return Some(after_quote[..quote_end].to_string());
                    }
                }
            }
        }
    }
    None
}

/// Generates a fallback signature when `render_signature` returns `None`.
fn fallback_signature(vis: &Visibility, kind: ItemKind, name: &str) -> String {
    let vis_str = match vis {
        Visibility::Public => "pub ",
        _ => "",
    };
    format!("{vis_str}{} {name}", kind.short_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Loads the fixture crate JSON using the recursion-safe parser.
    fn load_fixture() -> Crate {
        let json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test-fixtures/groxide_test_api.json"),
        )
        .expect("fixture JSON should exist");
        parse_rustdoc_json(&json).expect("fixture JSON should parse")
    }

    /// Builds the index from the fixture crate.
    fn build_fixture_index() -> DocIndex {
        let krate = load_fixture();
        build_index(&krate, "groxide_test_api", "0.1.0")
    }

    // ---- parse_rustdoc_json ----

    #[test]
    fn parse_succeeds_for_fixture() {
        let json = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("test-fixtures/groxide_test_api.json"),
        )
        .expect("fixture JSON should exist");
        let krate = parse_rustdoc_json(&json);
        assert!(
            krate.is_ok(),
            "parse_rustdoc_json failed: {:?}",
            krate.err()
        );
    }

    #[test]
    fn parse_returns_error_for_invalid_json() {
        let result = parse_rustdoc_json("not valid json");
        assert!(result.is_err());
    }

    // ---- Correct item count ----

    #[test]
    fn build_index_produces_nonzero_items() {
        let index = build_fixture_index();
        assert!(index.len() > 10, "expected >10 items, got {}", index.len());
    }

    // ---- Path map has expected paths ----

    #[test]
    fn path_map_contains_crate_root() {
        let index = build_fixture_index();
        assert!(
            index.path_map.contains_key("groxide_test_api"),
            "path_map should contain crate root"
        );
    }

    #[test]
    fn path_map_contains_top_level_struct() {
        let index = build_fixture_index();
        assert!(
            index
                .path_map
                .contains_key("groxide_test_api::SimpleStruct"),
            "path_map should contain SimpleStruct"
        );
    }

    #[test]
    fn path_map_contains_module() {
        let index = build_fixture_index();
        assert!(
            index.path_map.contains_key("groxide_test_api::containers"),
            "path_map should contain containers module"
        );
    }

    #[test]
    fn path_map_contains_nested_item() {
        let index = build_fixture_index();
        assert!(
            index
                .path_map
                .contains_key("groxide_test_api::containers::Stack"),
            "path_map should contain containers::Stack"
        );
    }

    #[test]
    fn path_map_contains_deeply_nested_function() {
        let index = build_fixture_index();
        assert!(
            index
                .path_map
                .contains_key("groxide_test_api::deeply::nested::deep_fn"),
            "path_map should contain deeply::nested::deep_fn"
        );
    }

    // ---- Suffix map generates correct suffixes ----

    #[test]
    fn suffix_map_contains_simple_name() {
        let index = build_fixture_index();
        assert!(
            index.suffix_map.contains_key("simplestruct"),
            "suffix_map should contain 'simplestruct'"
        );
    }

    #[test]
    fn suffix_map_contains_partial_path_suffix() {
        let index = build_fixture_index();
        assert!(
            index.suffix_map.contains_key("containers::stack"),
            "suffix_map should contain 'containers::stack'"
        );
    }

    #[test]
    fn suffix_map_contains_deeply_nested_suffix() {
        let index = build_fixture_index();
        assert!(
            index.suffix_map.contains_key("nested::deep_fn"),
            "suffix_map should contain 'nested::deep_fn'"
        );
    }

    // ---- Name map is case-insensitive ----

    #[test]
    fn name_map_lowercases_keys() {
        let index = build_fixture_index();
        assert!(
            index.name_map.contains_key("simplestruct"),
            "name_map should contain lowercased 'simplestruct'"
        );
        assert!(
            !index.name_map.contains_key("SimpleStruct"),
            "name_map should NOT contain original-case 'SimpleStruct'"
        );
    }

    #[test]
    fn name_map_contains_function_names() {
        let index = build_fixture_index();
        assert!(
            index.name_map.contains_key("add"),
            "name_map should contain 'add'"
        );
    }

    #[test]
    fn name_map_lookup_finds_items() {
        let index = build_fixture_index();
        let indices = index.name_map.get("stack").expect("should find 'stack'");
        assert!(!indices.is_empty());
        let item = index.get(indices[0]);
        assert_eq!(item.name, "Stack");
    }

    // ---- Re-exported items have correct paths ----

    #[test]
    fn reexport_helper_has_reexports_path() {
        let index = build_fixture_index();
        assert!(
            index
                .path_map
                .contains_key("groxide_test_api::reexports::Helper"),
            "path_map should contain reexported Helper at reexports path"
        );
    }

    #[test]
    fn glob_reexported_items_have_correct_path() {
        let index = build_fixture_index();
        assert!(
            index
                .path_map
                .contains_key("groxide_test_api::reexports::GlobItem"),
            "path_map should contain glob-reexported GlobItem at reexports path"
        );
    }

    // ---- Public vs private items correctly flagged ----

    #[test]
    fn public_items_flagged_correctly() {
        let index = build_fixture_index();
        let indices = index
            .path_map
            .get("groxide_test_api::SimpleStruct")
            .expect("should find SimpleStruct");
        let item = index.get(indices[0]);
        assert!(item.is_public, "SimpleStruct should be public");
    }

    #[test]
    fn public_function_flagged_correctly() {
        let index = build_fixture_index();
        let indices = index
            .path_map
            .get("groxide_test_api::add")
            .expect("should find add");
        let item = index.get(indices[0]);
        assert!(item.is_public, "add function should be public");
    }

    #[test]
    fn enum_variants_flagged_as_public() {
        let index = build_fixture_index();
        let indices = index
            .path_map
            .get("groxide_test_api::Direction::North")
            .expect("should find Direction::North");
        let item = index.get(indices[0]);
        assert!(item.is_public, "enum variant North should be public");
    }

    // ---- Trait impls stored in DocIndex.trait_impls ----

    #[test]
    fn trait_impls_stored_on_docindex() {
        let index = build_fixture_index();
        let stack_indices = index
            .path_map
            .get("groxide_test_api::containers::Stack")
            .expect("should find Stack");
        let stack_idx = stack_indices[0];
        let impls = index.item_trait_impls(stack_idx);
        let trait_names: Vec<&str> = impls.iter().map(|i| i.trait_path.as_str()).collect();
        assert!(
            trait_names.contains(&"Default"),
            "Stack should have Default impl, got: {trait_names:?}"
        );
    }

    #[test]
    fn trait_impls_not_on_index_item() {
        let index = build_fixture_index();
        let stack_indices = index
            .path_map
            .get("groxide_test_api::containers::Stack")
            .expect("should find Stack");
        let stack_item = index.get(stack_indices[0]);
        for child in &stack_item.children {
            assert_ne!(
                child.kind,
                ItemKind::Trait,
                "children should not include traits, found: {}",
                child.name
            );
        }
    }

    // ---- Children correctly linked ----

    #[test]
    fn struct_has_method_children() {
        let index = build_fixture_index();
        let stack_indices = index
            .path_map
            .get("groxide_test_api::containers::Stack")
            .expect("should find Stack");
        let stack_item = index.get(stack_indices[0]);
        let child_names: Vec<&str> = stack_item
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert!(
            child_names.contains(&"new"),
            "Stack should have 'new' method child, got: {child_names:?}"
        );
        assert!(
            child_names.contains(&"push"),
            "Stack should have 'push' method child, got: {child_names:?}"
        );
        assert!(
            child_names.contains(&"pop"),
            "Stack should have 'pop' method child, got: {child_names:?}"
        );
    }

    #[test]
    fn module_has_children() {
        let index = build_fixture_index();
        let containers_indices = index
            .path_map
            .get("groxide_test_api::containers")
            .expect("should find containers module");
        let containers_item = index.get(containers_indices[0]);
        let child_names: Vec<&str> = containers_item
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert!(
            child_names.contains(&"Stack"),
            "containers module should have Stack child, got: {child_names:?}"
        );
        assert!(
            child_names.contains(&"Pair"),
            "containers module should have Pair child, got: {child_names:?}"
        );
    }

    // ---- Enum has variant children ----

    #[test]
    fn enum_has_variant_children() {
        let index = build_fixture_index();
        let direction_indices = index
            .path_map
            .get("groxide_test_api::Direction")
            .expect("should find Direction");
        let direction_item = index.get(direction_indices[0]);
        let variant_names: Vec<&str> = direction_item
            .children
            .iter()
            .filter(|c| c.kind == ItemKind::Variant)
            .map(|c| c.name.as_str())
            .collect();
        assert!(variant_names.contains(&"North"), "should have North");
        assert!(variant_names.contains(&"South"), "should have South");
        assert!(variant_names.contains(&"East"), "should have East");
        assert!(variant_names.contains(&"West"), "should have West");
    }

    #[test]
    fn enum_shape_has_all_variants() {
        let index = build_fixture_index();
        let shape_indices = index
            .path_map
            .get("groxide_test_api::Shape")
            .expect("should find Shape");
        let shape_item = index.get(shape_indices[0]);
        let variant_names: Vec<&str> = shape_item
            .children
            .iter()
            .filter(|c| c.kind == ItemKind::Variant)
            .map(|c| c.name.as_str())
            .collect();
        assert!(variant_names.contains(&"Circle"), "should have Circle");
        assert!(
            variant_names.contains(&"Rectangle"),
            "should have Rectangle"
        );
        assert!(variant_names.contains(&"Point"), "should have Point");
    }

    // ---- Trait has method children ----

    #[test]
    fn trait_has_method_children() {
        let index = build_fixture_index();
        let stringify_indices = index
            .path_map
            .get("groxide_test_api::traits::Stringify")
            .expect("should find Stringify");
        let stringify_item = index.get(stringify_indices[0]);
        let child_names: Vec<&str> = stringify_item
            .children
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert!(
            child_names.contains(&"stringify"),
            "Stringify should have 'stringify', got: {child_names:?}"
        );
        assert!(
            child_names.contains(&"debug_string"),
            "Stringify should have 'debug_string', got: {child_names:?}"
        );
    }

    // ---- has_body flag ----

    #[test]
    fn trait_required_method_has_body_false() {
        let index = build_fixture_index();
        let indices = index
            .path_map
            .get("groxide_test_api::traits::Stringify::stringify")
            .expect("should find stringify method");
        let item = index.get(indices[0]);
        assert!(!item.has_body, "required method should have has_body=false");
    }

    #[test]
    fn trait_provided_method_has_body_true() {
        let index = build_fixture_index();
        let indices = index
            .path_map
            .get("groxide_test_api::traits::Stringify::debug_string")
            .expect("should find debug_string method");
        let item = index.get(indices[0]);
        assert!(item.has_body, "provided method should have has_body=true");
    }

    // ---- Summary extraction ----

    #[test]
    fn extract_summary_first_sentence() {
        assert_eq!(
            extract_summary("Adds two numbers together. Returns the sum."),
            "Adds two numbers together."
        );
    }

    #[test]
    fn extract_summary_no_terminator() {
        assert_eq!(
            extract_summary("A simple struct with no generics"),
            "A simple struct with no generics"
        );
    }

    #[test]
    fn extract_summary_empty_docs() {
        assert_eq!(extract_summary(""), "");
    }

    #[test]
    fn extract_summary_truncates_long_first_line() {
        let long_line = "a".repeat(200);
        let result = extract_summary(&long_line);
        assert_eq!(result.len(), 103); // 100 + "..."
        assert!(result.ends_with("..."));
    }

    #[test]
    fn extract_summary_period_in_version_number() {
        assert_eq!(
            extract_summary("Requires version 1.56.0 to compile"),
            "Requires version 1.56.0 to compile"
        );
    }

    // ---- Feature gate extraction ----

    #[test]
    fn extract_feature_gate_from_attrs() {
        let item = rustdoc_types::Item {
            id: Id(0),
            crate_id: 0,
            name: Some("test".to_string()),
            span: None,
            visibility: Visibility::Public,
            docs: None,
            links: HashMap::new(),
            attrs: vec![rustdoc_types::Attribute::Other(
                "#[doc(cfg(feature = \"unstable\"))]".to_string(),
            )],
            deprecation: None,
            inner: ItemEnum::Constant {
                type_: rustdoc_types::Type::Primitive("bool".to_string()),
                const_: rustdoc_types::Constant {
                    expr: String::new(),
                    value: None,
                    is_literal: false,
                },
            },
        };
        assert_eq!(extract_feature_gate(&item), Some("unstable".to_string()));
    }

    #[test]
    fn extract_feature_gate_none_when_absent() {
        let item = rustdoc_types::Item {
            id: Id(0),
            crate_id: 0,
            name: Some("test".to_string()),
            span: None,
            visibility: Visibility::Public,
            docs: None,
            links: HashMap::new(),
            attrs: vec![],
            deprecation: None,
            inner: ItemEnum::Constant {
                type_: rustdoc_types::Type::Primitive("bool".to_string()),
                const_: rustdoc_types::Constant {
                    expr: String::new(),
                    value: None,
                    is_literal: false,
                },
            },
        };
        assert_eq!(extract_feature_gate(&item), None);
    }

    // ---- All maps populated ----

    #[test]
    fn all_maps_populated() {
        let index = build_fixture_index();
        assert!(!index.path_map.is_empty(), "path_map should not be empty");
        assert!(!index.name_map.is_empty(), "name_map should not be empty");
        assert!(
            !index.suffix_map.is_empty(),
            "suffix_map should not be empty"
        );
    }

    // ---- Crate metadata ----

    #[test]
    fn crate_name_and_version_set() {
        let index = build_fixture_index();
        assert_eq!(index.crate_name, "groxide_test_api");
        assert_eq!(index.crate_version, "0.1.0");
    }

    // ---- Item kinds present ----

    #[test]
    fn index_contains_expected_item_kinds() {
        let index = build_fixture_index();
        let kinds: HashSet<ItemKind> = index.items.iter().map(|i| i.kind).collect();

        assert!(kinds.contains(&ItemKind::Module), "should have modules");
        assert!(kinds.contains(&ItemKind::Struct), "should have structs");
        assert!(kinds.contains(&ItemKind::Enum), "should have enums");
        assert!(kinds.contains(&ItemKind::Function), "should have functions");
        assert!(kinds.contains(&ItemKind::Constant), "should have constants");
        assert!(
            kinds.contains(&ItemKind::TypeAlias),
            "should have type aliases"
        );
        assert!(kinds.contains(&ItemKind::Static), "should have statics");
        assert!(kinds.contains(&ItemKind::Macro), "should have macros");
        assert!(kinds.contains(&ItemKind::Trait), "should have traits");
        assert!(kinds.contains(&ItemKind::Union), "should have unions");
        assert!(kinds.contains(&ItemKind::Variant), "should have variants");
        assert!(kinds.contains(&ItemKind::Field), "should have fields");
    }

    // ---- Signatures populated ----

    #[test]
    fn items_have_nonempty_signatures() {
        let index = build_fixture_index();
        for item in &index.items {
            assert!(
                !item.signature.is_empty(),
                "item {} ({:?}) should have non-empty signature",
                item.path,
                item.kind
            );
        }
    }

    // ---- Docs populated ----

    #[test]
    fn documented_items_have_docs() {
        let index = build_fixture_index();
        let add_indices = index
            .path_map
            .get("groxide_test_api::add")
            .expect("should find add");
        let add_item = index.get(add_indices[0]);
        assert!(!add_item.docs.is_empty(), "add should have documentation");
        assert!(!add_item.summary.is_empty(), "add should have a summary");
    }

    // ---- Constant items ----

    #[test]
    fn constants_have_correct_kind() {
        let index = build_fixture_index();
        let indices = index
            .path_map
            .get("groxide_test_api::MAX_BUFFER_SIZE")
            .expect("should find MAX_BUFFER_SIZE");
        let item = index.get(indices[0]);
        assert_eq!(item.kind, ItemKind::Constant);
    }

    // ---- Associated types and consts from traits ----

    #[test]
    fn trait_assoc_type_has_path() {
        let index = build_fixture_index();
        assert!(
            index
                .path_map
                .contains_key("groxide_test_api::traits::Processor::Input"),
            "Processor::Input should have a path"
        );
    }

    #[test]
    fn trait_assoc_const_has_path() {
        let index = build_fixture_index();
        assert!(
            index
                .path_map
                .contains_key("groxide_test_api::traits::Processor::MAX_ITEMS"),
            "Processor::MAX_ITEMS should have a path"
        );
    }
}
