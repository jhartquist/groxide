use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// Represents the kind of a documented Rust item.
///
/// Every documented item has exactly one kind. This enum handles display names,
/// CLI filter matching, category grouping, and crate-root prioritization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub(crate) enum ItemKind {
    Module,
    Struct,
    Enum,
    Union,
    Trait,
    TraitAlias,
    Function,
    TypeAlias,
    AssocType,
    ForeignType,
    Constant,
    AssocConst,
    Static,
    Macro,
    ProcMacro,
    Variant,
    Field,
    Primitive,
}

impl ItemKind {
    /// Returns the user-facing short name: "fn", "struct", "mod", etc.
    pub(crate) fn short_name(self) -> &'static str {
        match self {
            Self::Module => "mod",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Union => "union",
            Self::Trait => "trait",
            Self::TraitAlias => "trait alias",
            Self::Function => "fn",
            Self::TypeAlias | Self::AssocType | Self::ForeignType => "type",
            Self::Constant | Self::AssocConst => "const",
            Self::Static => "static",
            Self::Macro => "macro",
            Self::ProcMacro => "proc macro",
            Self::Variant => "variant",
            Self::Field => "field",
            Self::Primitive => "primitive",
        }
    }

    /// Returns whether this kind matches a CLI `--kind` filter value.
    ///
    /// Grouping rules:
    /// - `fn` matches `Function`
    /// - `struct` matches `Struct`
    /// - `enum` matches `Enum`
    /// - `trait` matches `Trait`, `TraitAlias`
    /// - `type` matches `TypeAlias`, `AssocType`, `ForeignType`
    /// - `const` matches `Constant`, `AssocConst`
    /// - `mod` matches `Module`
    /// - `macro` matches `Macro`, `ProcMacro`
    ///
    /// All other kinds match only themselves.
    pub(crate) fn matches_filter(self, filter: Self) -> bool {
        match filter {
            Self::Trait => matches!(self, Self::Trait | Self::TraitAlias),
            Self::TypeAlias => {
                matches!(self, Self::TypeAlias | Self::AssocType | Self::ForeignType)
            }
            Self::Constant => matches!(self, Self::Constant | Self::AssocConst),
            Self::Macro => matches!(self, Self::Macro | Self::ProcMacro),
            other => self == other,
        }
    }

    /// Maps this kind to a [`KindCategory`] for display grouping.
    pub(crate) fn category(self) -> KindCategory {
        match self {
            Self::Module => KindCategory::Modules,
            Self::Struct => KindCategory::Structs,
            Self::Enum => KindCategory::Enums,
            Self::Union => KindCategory::Unions,
            Self::Trait | Self::TraitAlias => KindCategory::Traits,
            Self::Function => KindCategory::Functions,
            Self::TypeAlias | Self::AssocType | Self::ForeignType => KindCategory::TypeAliases,
            Self::Constant | Self::AssocConst => KindCategory::Constants,
            Self::Static => KindCategory::Statics,
            Self::Macro | Self::ProcMacro => KindCategory::Macros,
            Self::Variant => KindCategory::Variants,
            Self::Field => KindCategory::Fields,
            Self::Primitive => KindCategory::Primitives,
        }
    }

    /// Returns whether this is a "primary" kind for crate-root auto-selection.
    ///
    /// Primary kinds: `Struct`, `Enum`, `Union`, `Trait`, `TraitAlias`, `TypeAlias`.
    /// When an ambiguous query at the crate root has exactly one primary match,
    /// it auto-selects to Found instead of Ambiguous.
    pub(crate) fn is_primary(self) -> bool {
        matches!(
            self,
            Self::Struct
                | Self::Enum
                | Self::Union
                | Self::Trait
                | Self::TraitAlias
                | Self::TypeAlias
        )
    }
}

/// Groups items by kind for display in module/crate listings.
///
/// Variant order defines display order. `BTreeMap<KindCategory, _>` auto-sorts
/// in display order because `Ord` uses discriminant order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum KindCategory {
    Modules,
    Structs,
    Enums,
    Unions,
    Traits,
    Functions,
    TypeAliases,
    Constants,
    Statics,
    Macros,
    Variants,
    Fields,
    Primitives,
}

impl KindCategory {
    /// Returns the section header text: "Modules:", "Structs:", etc.
    pub(crate) fn header(self) -> &'static str {
        match self {
            Self::Modules => "Modules:",
            Self::Structs => "Structs:",
            Self::Enums => "Enums:",
            Self::Unions => "Unions:",
            Self::Traits => "Traits:",
            Self::Functions => "Functions:",
            Self::TypeAliases => "Type Aliases:",
            Self::Constants => "Constants:",
            Self::Statics => "Statics:",
            Self::Macros => "Macros:",
            Self::Variants => "Variants:",
            Self::Fields => "Fields:",
            Self::Primitives => "Primitives:",
        }
    }

    /// Returns whether items in this group show signature+summary (true) vs name+summary (false).
    pub(crate) fn uses_signature_display(self) -> bool {
        matches!(
            self,
            Self::Functions | Self::TypeAliases | Self::Constants | Self::Statics
        )
    }
}

/// Items grouped by category for display. Empty categories are absent from the map.
pub(crate) type GroupedItems<'a> = BTreeMap<KindCategory, Vec<&'a IndexItem>>;

/// Groups items by category and sorts alphabetically within each group.
pub(crate) fn group_items<'a>(items: &[&'a IndexItem]) -> GroupedItems<'a> {
    let mut groups: GroupedItems<'a> = BTreeMap::new();
    for &item in items {
        groups.entry(item.kind.category()).or_default().push(item);
    }
    for items_in_group in groups.values_mut() {
        items_in_group.sort_by(|a, b| a.name.cmp(&b.name));
    }
    groups
}

/// One entry per documented item. Stored in `DocIndex.items`, serialized to cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct IndexItem {
    /// Full path, e.g., `tokio::sync::Mutex`.
    pub(crate) path: String,
    /// Simple name, e.g., "Mutex".
    pub(crate) name: String,
    /// The kind of item.
    pub(crate) kind: ItemKind,
    /// Rendered signature, e.g., "pub struct Mutex<T: ?Sized>".
    pub(crate) signature: String,
    /// Full doc comment, raw text (not markdown).
    pub(crate) docs: String,
    /// First sentence of docs.
    pub(crate) summary: String,
    /// Source file location.
    pub(crate) span: SourceSpan,
    /// Children (methods, variants, fields, etc.).
    pub(crate) children: Vec<ChildRef>,
    /// Whether the item is publicly visible.
    pub(crate) is_public: bool,
    /// For trait methods: true = provided, false = required.
    pub(crate) has_body: bool,
    /// Feature gate, e.g., "fs" from `#[cfg(feature = "fs")]`.
    pub(crate) feature_gate: Option<String>,
}

/// References a child item within a [`DocIndex`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ChildRef {
    /// Index into `DocIndex.items`.
    pub(crate) index: usize,
    /// The kind of the child item.
    pub(crate) kind: ItemKind,
    /// The simple name of the child.
    pub(crate) name: String,
}

/// Source file location for an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SourceSpan {
    /// Relative path, e.g., "src/lib.rs". Empty if unavailable.
    pub(crate) file: String,
    /// 1-based start line. 0 if unavailable.
    pub(crate) line_start: u32,
    /// 1-based end line, inclusive. 0 if unavailable.
    pub(crate) line_end: u32,
}

/// Describes a trait implementation on a type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TraitImplInfo {
    /// Trait path, e.g., "Clone", `std::fmt::Debug`.
    pub(crate) trait_path: String,
    /// Whether this is a synthetic auto-trait (Send, Sync, etc.).
    pub(crate) is_synthetic: bool,
}

/// The queryable index for one crate. Built from rustdoc JSON, cached to disk.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct DocIndex {
    /// All items in the index.
    pub(crate) items: Vec<IndexItem>,

    /// Full path -> item indices. Original case. Multiple items can share a path.
    pub(crate) path_map: HashMap<String, Vec<usize>>,

    /// Lowercased simple name -> item indices.
    pub(crate) name_map: HashMap<String, Vec<usize>>,

    /// Lowercased path suffix -> item indices.
    /// `tokio::sync::Mutex` generates: "mutex", `sync::mutex`, `tokio::sync::mutex`.
    pub(crate) suffix_map: HashMap<String, Vec<usize>>,

    /// Trait implementations keyed by parent type's item index.
    pub(crate) trait_impls: HashMap<usize, Vec<TraitImplInfo>>,

    /// Normalized crate name (hyphens -> underscores).
    pub(crate) crate_name: String,

    /// Crate version, e.g., "1.0.210". Empty if unknown.
    pub(crate) crate_version: String,
}

impl DocIndex {
    /// Creates a new empty index for the given crate.
    pub(crate) fn new(crate_name: String, crate_version: String) -> Self {
        Self {
            items: Vec::new(),
            path_map: HashMap::new(),
            name_map: HashMap::new(),
            suffix_map: HashMap::new(),
            trait_impls: HashMap::new(),
            crate_name,
            crate_version,
        }
    }

    /// Adds an item and updates `path_map`, `name_map`, and `suffix_map`.
    pub(crate) fn add_item(&mut self, item: IndexItem) {
        let index = self.items.len();

        // path_map: original case
        self.path_map
            .entry(item.path.clone())
            .or_default()
            .push(index);

        // name_map: lowercased simple name
        self.name_map
            .entry(item.name.to_lowercase())
            .or_default()
            .push(index);

        // suffix_map: all lowercased suffixes of the path
        let segments: Vec<&str> = item.path.split("::").collect();
        for i in 0..segments.len() {
            let suffix = segments[i..].join("::").to_lowercase();
            self.suffix_map.entry(suffix).or_default().push(index);
        }

        self.items.push(item);
    }

    /// Returns a reference to the item at the given index.
    pub(crate) fn get(&self, index: usize) -> &IndexItem {
        &self.items[index]
    }

    /// Returns trait implementations for the given item index, or an empty slice.
    pub(crate) fn item_trait_impls(&self, index: usize) -> &[TraitImplInfo] {
        self.trait_impls.get(&index).map_or(&[], Vec::as_slice)
    }
}

/// Output of the query engine's lookup pipeline.
#[derive(Debug)]
pub(crate) enum QueryResult {
    /// Exactly one item matched.
    Found {
        /// Index into `DocIndex.items`.
        index: usize,
    },

    /// Multiple items matched.
    Ambiguous {
        /// Item indices ordered by priority.
        indices: Vec<usize>,
        /// The original query string.
        query: String,
    },

    /// Nothing matched.
    NotFound {
        /// The original query string.
        query: String,
        /// Near-match suggestions (Levenshtein <= 3, max 5).
        suggestions: Vec<String>,
    },
}

/// Minimal search result from the search engine.
#[derive(Debug)]
pub(crate) struct SearchResult {
    /// Index into `DocIndex.items`.
    pub(crate) index: usize,
    /// Score: higher is better. Tiers: 100/75/40/30/20.
    pub(crate) score: u32,
}

/// Built from [`DocIndex`] + [`IndexItem`], consumed by renderers. Never stored.
#[derive(Debug)]
pub(crate) enum DisplayItem<'a> {
    /// Crate root display.
    Crate {
        /// The crate root item.
        item: &'a IndexItem,
        /// Public children grouped by category.
        children: GroupedItems<'a>,
    },
    /// Module display.
    Module {
        /// The module item.
        item: &'a IndexItem,
        /// Public children grouped by category.
        children: GroupedItems<'a>,
    },
    /// Type display (struct, enum, union).
    Type {
        /// The type item.
        item: &'a IndexItem,
        /// Associated methods.
        methods: Vec<&'a IndexItem>,
        /// Enum variants (empty for struct/union).
        variants: Vec<&'a IndexItem>,
        /// Trait implementations.
        trait_impls: &'a [TraitImplInfo],
    },
    /// Trait display.
    Trait {
        /// The trait item.
        item: &'a IndexItem,
        /// Required methods (`has_body == false`).
        required_methods: Vec<&'a IndexItem>,
        /// Provided methods (`has_body == true`).
        provided_methods: Vec<&'a IndexItem>,
    },
    /// Leaf item (function, constant, macro, etc.).
    Leaf {
        /// The leaf item.
        item: &'a IndexItem,
    },
}

/// Controls truncation limits for display output.
#[derive(Debug)]
pub(crate) struct DisplayLimits {
    /// Maximum number of methods to display. Default: 15.
    pub(crate) max_methods: usize,
    /// Maximum number of trait impls to display. Default: 5.
    pub(crate) max_trait_impls: usize,
    /// Maximum doc text length in characters. Default: 1500.
    pub(crate) max_doc_length: usize,
    /// `--all` flag disables all truncation.
    pub(crate) expand_all: bool,
}

impl Default for DisplayLimits {
    fn default() -> Self {
        Self {
            max_methods: 15,
            max_trait_impls: 5,
            max_doc_length: 1500,
            expand_all: false,
        }
    }
}

#[cfg(test)]
impl DocIndex {
    /// Returns the number of items in the index.
    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::make_item;

    // ---- ItemKind::short_name ----

    #[test]
    fn short_name_returns_correct_value_for_all_variants() {
        assert_eq!(ItemKind::Module.short_name(), "mod");
        assert_eq!(ItemKind::Struct.short_name(), "struct");
        assert_eq!(ItemKind::Enum.short_name(), "enum");
        assert_eq!(ItemKind::Union.short_name(), "union");
        assert_eq!(ItemKind::Trait.short_name(), "trait");
        assert_eq!(ItemKind::TraitAlias.short_name(), "trait alias");
        assert_eq!(ItemKind::Function.short_name(), "fn");
        assert_eq!(ItemKind::TypeAlias.short_name(), "type");
        assert_eq!(ItemKind::AssocType.short_name(), "type");
        assert_eq!(ItemKind::ForeignType.short_name(), "type");
        assert_eq!(ItemKind::Constant.short_name(), "const");
        assert_eq!(ItemKind::AssocConst.short_name(), "const");
        assert_eq!(ItemKind::Static.short_name(), "static");
        assert_eq!(ItemKind::Macro.short_name(), "macro");
        assert_eq!(ItemKind::ProcMacro.short_name(), "proc macro");
        assert_eq!(ItemKind::Variant.short_name(), "variant");
        assert_eq!(ItemKind::Field.short_name(), "field");
        assert_eq!(ItemKind::Primitive.short_name(), "primitive");
    }

    // ---- ItemKind::matches_filter ----

    #[test]
    fn matches_filter_trait_matches_trait_and_alias() {
        assert!(ItemKind::Trait.matches_filter(ItemKind::Trait));
        assert!(ItemKind::TraitAlias.matches_filter(ItemKind::Trait));
        assert!(!ItemKind::Struct.matches_filter(ItemKind::Trait));
    }

    #[test]
    fn matches_filter_type_matches_type_alias_and_assoc_and_foreign() {
        assert!(ItemKind::TypeAlias.matches_filter(ItemKind::TypeAlias));
        assert!(ItemKind::AssocType.matches_filter(ItemKind::TypeAlias));
        assert!(ItemKind::ForeignType.matches_filter(ItemKind::TypeAlias));
        assert!(!ItemKind::Struct.matches_filter(ItemKind::TypeAlias));
    }

    #[test]
    fn matches_filter_const_matches_constant_and_assoc_const() {
        assert!(ItemKind::Constant.matches_filter(ItemKind::Constant));
        assert!(ItemKind::AssocConst.matches_filter(ItemKind::Constant));
        assert!(!ItemKind::Static.matches_filter(ItemKind::Constant));
    }

    #[test]
    fn matches_filter_macro_matches_macro_and_proc_macro() {
        assert!(ItemKind::Macro.matches_filter(ItemKind::Macro));
        assert!(ItemKind::ProcMacro.matches_filter(ItemKind::Macro));
        assert!(!ItemKind::Function.matches_filter(ItemKind::Macro));
    }

    #[test]
    fn matches_filter_fn_matches_only_function() {
        assert!(ItemKind::Function.matches_filter(ItemKind::Function));
        assert!(!ItemKind::Macro.matches_filter(ItemKind::Function));
    }

    #[test]
    fn matches_filter_struct_matches_only_struct() {
        assert!(ItemKind::Struct.matches_filter(ItemKind::Struct));
        assert!(!ItemKind::Enum.matches_filter(ItemKind::Struct));
    }

    #[test]
    fn matches_filter_mod_matches_only_module() {
        assert!(ItemKind::Module.matches_filter(ItemKind::Module));
        assert!(!ItemKind::Function.matches_filter(ItemKind::Module));
    }

    #[test]
    fn matches_filter_enum_matches_only_enum() {
        assert!(ItemKind::Enum.matches_filter(ItemKind::Enum));
        assert!(!ItemKind::Struct.matches_filter(ItemKind::Enum));
    }

    #[test]
    fn matches_filter_other_kinds_match_only_themselves() {
        assert!(ItemKind::Variant.matches_filter(ItemKind::Variant));
        assert!(!ItemKind::Variant.matches_filter(ItemKind::Field));
        assert!(ItemKind::Field.matches_filter(ItemKind::Field));
        assert!(ItemKind::Static.matches_filter(ItemKind::Static));
        assert!(ItemKind::Primitive.matches_filter(ItemKind::Primitive));
        assert!(ItemKind::Union.matches_filter(ItemKind::Union));
    }

    // ---- ItemKind::category ----

    #[test]
    fn category_maps_all_kinds_correctly() {
        assert_eq!(ItemKind::Module.category(), KindCategory::Modules);
        assert_eq!(ItemKind::Struct.category(), KindCategory::Structs);
        assert_eq!(ItemKind::Enum.category(), KindCategory::Enums);
        assert_eq!(ItemKind::Union.category(), KindCategory::Unions);
        assert_eq!(ItemKind::Trait.category(), KindCategory::Traits);
        assert_eq!(ItemKind::TraitAlias.category(), KindCategory::Traits);
        assert_eq!(ItemKind::Function.category(), KindCategory::Functions);
        assert_eq!(ItemKind::TypeAlias.category(), KindCategory::TypeAliases);
        assert_eq!(ItemKind::AssocType.category(), KindCategory::TypeAliases);
        assert_eq!(ItemKind::ForeignType.category(), KindCategory::TypeAliases);
        assert_eq!(ItemKind::Constant.category(), KindCategory::Constants);
        assert_eq!(ItemKind::AssocConst.category(), KindCategory::Constants);
        assert_eq!(ItemKind::Static.category(), KindCategory::Statics);
        assert_eq!(ItemKind::Macro.category(), KindCategory::Macros);
        assert_eq!(ItemKind::ProcMacro.category(), KindCategory::Macros);
        assert_eq!(ItemKind::Variant.category(), KindCategory::Variants);
        assert_eq!(ItemKind::Field.category(), KindCategory::Fields);
        assert_eq!(ItemKind::Primitive.category(), KindCategory::Primitives);
    }

    // ---- ItemKind::is_primary ----

    #[test]
    fn is_primary_returns_true_for_expected_kinds() {
        assert!(ItemKind::Struct.is_primary());
        assert!(ItemKind::Enum.is_primary());
        assert!(ItemKind::Union.is_primary());
        assert!(ItemKind::Trait.is_primary());
        assert!(ItemKind::TraitAlias.is_primary());
        assert!(ItemKind::TypeAlias.is_primary());
    }

    #[test]
    fn is_primary_returns_false_for_non_primary_kinds() {
        assert!(!ItemKind::Module.is_primary());
        assert!(!ItemKind::Function.is_primary());
        assert!(!ItemKind::Constant.is_primary());
        assert!(!ItemKind::AssocConst.is_primary());
        assert!(!ItemKind::Static.is_primary());
        assert!(!ItemKind::Macro.is_primary());
        assert!(!ItemKind::ProcMacro.is_primary());
        assert!(!ItemKind::Variant.is_primary());
        assert!(!ItemKind::Field.is_primary());
        assert!(!ItemKind::Primitive.is_primary());
        assert!(!ItemKind::AssocType.is_primary());
        assert!(!ItemKind::ForeignType.is_primary());
    }

    // ---- KindCategory::header ----

    #[test]
    fn header_returns_expected_text_for_all_categories() {
        assert_eq!(KindCategory::Modules.header(), "Modules:");
        assert_eq!(KindCategory::Structs.header(), "Structs:");
        assert_eq!(KindCategory::Enums.header(), "Enums:");
        assert_eq!(KindCategory::Unions.header(), "Unions:");
        assert_eq!(KindCategory::Traits.header(), "Traits:");
        assert_eq!(KindCategory::Functions.header(), "Functions:");
        assert_eq!(KindCategory::TypeAliases.header(), "Type Aliases:");
        assert_eq!(KindCategory::Constants.header(), "Constants:");
        assert_eq!(KindCategory::Statics.header(), "Statics:");
        assert_eq!(KindCategory::Macros.header(), "Macros:");
        assert_eq!(KindCategory::Variants.header(), "Variants:");
        assert_eq!(KindCategory::Fields.header(), "Fields:");
        assert_eq!(KindCategory::Primitives.header(), "Primitives:");
    }

    // ---- KindCategory ordering ----

    #[test]
    fn category_ordering_matches_display_order() {
        assert!(KindCategory::Modules < KindCategory::Structs);
        assert!(KindCategory::Structs < KindCategory::Enums);
        assert!(KindCategory::Enums < KindCategory::Unions);
        assert!(KindCategory::Unions < KindCategory::Traits);
        assert!(KindCategory::Traits < KindCategory::Functions);
        assert!(KindCategory::Functions < KindCategory::TypeAliases);
        assert!(KindCategory::TypeAliases < KindCategory::Constants);
        assert!(KindCategory::Constants < KindCategory::Statics);
        assert!(KindCategory::Statics < KindCategory::Macros);
    }

    // ---- KindCategory::uses_signature_display ----

    #[test]
    fn uses_signature_display_correct_for_all_categories() {
        assert!(!KindCategory::Modules.uses_signature_display());
        assert!(!KindCategory::Structs.uses_signature_display());
        assert!(!KindCategory::Enums.uses_signature_display());
        assert!(!KindCategory::Unions.uses_signature_display());
        assert!(!KindCategory::Traits.uses_signature_display());
        assert!(KindCategory::Functions.uses_signature_display());
        assert!(KindCategory::TypeAliases.uses_signature_display());
        assert!(KindCategory::Constants.uses_signature_display());
        assert!(KindCategory::Statics.uses_signature_display());
        assert!(!KindCategory::Macros.uses_signature_display());
        assert!(!KindCategory::Variants.uses_signature_display());
        assert!(!KindCategory::Fields.uses_signature_display());
        assert!(!KindCategory::Primitives.uses_signature_display());
    }

    // ---- GroupedItems ----

    #[test]
    fn group_items_groups_by_category_and_sorts_alphabetically() {
        let items = [
            make_item("Zebra", "crate::Zebra", ItemKind::Struct),
            make_item("alpha", "crate::alpha", ItemKind::Function),
            make_item("Apple", "crate::Apple", ItemKind::Struct),
            make_item("beta", "crate::beta", ItemKind::Function),
        ];
        let refs: Vec<&IndexItem> = items.iter().collect();
        let groups = group_items(&refs);

        assert_eq!(groups.len(), 2);

        let structs = &groups[&KindCategory::Structs];
        assert_eq!(structs.len(), 2);
        assert_eq!(structs[0].name, "Apple");
        assert_eq!(structs[1].name, "Zebra");

        let fns = &groups[&KindCategory::Functions];
        assert_eq!(fns.len(), 2);
        assert_eq!(fns[0].name, "alpha");
        assert_eq!(fns[1].name, "beta");
    }

    #[test]
    fn group_items_empty_categories_absent() {
        let items = [make_item("Foo", "crate::Foo", ItemKind::Struct)];
        let refs: Vec<&IndexItem> = items.iter().collect();
        let groups = group_items(&refs);

        assert_eq!(groups.len(), 1);
        assert!(groups.contains_key(&KindCategory::Structs));
        assert!(!groups.contains_key(&KindCategory::Functions));
    }

    // ---- DocIndex::add_item ----

    #[test]
    fn add_item_populates_all_three_maps() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let item = make_item("Mutex", "mycrate::sync::Mutex", ItemKind::Struct);
        index.add_item(item);

        assert_eq!(index.len(), 1);

        // path_map
        assert_eq!(index.path_map.get("mycrate::sync::Mutex"), Some(&vec![0]));

        // name_map (lowercased)
        assert_eq!(index.name_map.get("mutex"), Some(&vec![0]));

        // suffix_map (lowercased suffixes)
        assert_eq!(index.suffix_map.get("mutex"), Some(&vec![0]));
        assert_eq!(index.suffix_map.get("sync::mutex"), Some(&vec![0]));
        assert_eq!(index.suffix_map.get("mycrate::sync::mutex"), Some(&vec![0]));
    }

    #[test]
    fn add_item_multiple_items_same_name() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_item("new", "mycrate::Foo::new", ItemKind::Function));
        index.add_item(make_item("new", "mycrate::Bar::new", ItemKind::Function));

        assert_eq!(index.len(), 2);
        assert_eq!(index.name_map.get("new"), Some(&vec![0, 1]));
    }

    // ---- DocIndex suffix map ----

    #[test]
    fn suffix_map_generates_correct_keys_for_deep_path() {
        let mut index = DocIndex::new("tokio".to_string(), "1.0.0".to_string());
        let item = make_item("Mutex", "tokio::sync::Mutex", ItemKind::Struct);
        index.add_item(item);

        // Should have 3 suffix entries
        assert!(index.suffix_map.contains_key("mutex"));
        assert!(index.suffix_map.contains_key("sync::mutex"));
        assert!(index.suffix_map.contains_key("tokio::sync::mutex"));
        // Should NOT have partial segment matches
        assert!(!index.suffix_map.contains_key("::mutex"));
        assert!(!index.suffix_map.contains_key("io::sync::mutex"));
    }

    #[test]
    fn suffix_map_single_segment_path() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        let item = make_item("mycrate", "mycrate", ItemKind::Module);
        index.add_item(item);

        assert!(index.suffix_map.contains_key("mycrate"));
        assert_eq!(index.suffix_map.len(), 1);
    }

    #[test]
    fn suffix_map_case_insensitive() {
        let mut index = DocIndex::new("mycrate".to_string(), "0.1.0".to_string());
        index.add_item(make_item("MyStruct", "mycrate::MyStruct", ItemKind::Struct));

        assert!(index.suffix_map.contains_key("mystruct"));
        assert!(index.suffix_map.contains_key("mycrate::mystruct"));
        assert!(!index.suffix_map.contains_key("MyStruct"));
    }

    // ---- DocIndex::get ----

    #[test]
    fn get_returns_correct_item() {
        let mut index = DocIndex::new("c".to_string(), String::new());
        index.add_item(make_item("A", "c::A", ItemKind::Struct));
        index.add_item(make_item("B", "c::B", ItemKind::Enum));

        assert_eq!(index.get(0).name, "A");
        assert_eq!(index.get(1).name, "B");
    }

    // ---- DocIndex::item_trait_impls ----

    #[test]
    fn item_trait_impls_returns_empty_when_none() {
        let index = DocIndex::new("c".to_string(), String::new());
        assert!(index.item_trait_impls(0).is_empty());
    }

    #[test]
    fn item_trait_impls_returns_impls_when_present() {
        let mut index = DocIndex::new("c".to_string(), String::new());
        index.add_item(make_item("Foo", "c::Foo", ItemKind::Struct));
        index.trait_impls.insert(
            0,
            vec![TraitImplInfo {
                trait_path: "Clone".to_string(),
                is_synthetic: false,
            }],
        );

        let impls = index.item_trait_impls(0);
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].trait_path, "Clone");
    }

    // ---- DisplayLimits defaults ----

    #[test]
    fn display_limits_default_values() {
        let limits = DisplayLimits::default();
        assert_eq!(limits.max_methods, 15);
        assert_eq!(limits.max_trait_impls, 5);
        assert_eq!(limits.max_doc_length, 1500);
        assert!(!limits.expand_all);
    }
}
