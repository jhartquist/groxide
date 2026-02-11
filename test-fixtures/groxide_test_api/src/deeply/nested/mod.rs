//! The deepest level of nesting for path resolution tests.
//!
//! Accessible as `groxide_test_api::deeply::nested`.

/// A function at the deepest nesting level.
///
/// Useful for testing suffix matching: querying `nested::deep_fn` should
/// find `groxide_test_api::deeply::nested::deep_fn`.
pub fn deep_fn() -> &'static str {
    "deep"
}

/// A struct at 3 levels of nesting.
pub struct InnerItem {
    /// A label for this item.
    pub label: String,
}

/// A constant at the deepest level.
pub const DEPTH: u32 = 3;
