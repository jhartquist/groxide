//! Inner crate for cross-crate re-export tests.
//!
//! Items here are re-exported by `groxide_test_api` in three shapes:
//! - terminal stub: `pub use groxide_test_inner::CrossStruct;`
//! - module re-export: `pub use groxide_test_inner::cross_mod;`
//! - wildcard glob: `pub use groxide_test_inner::*;` (in a sub-module)

/// A struct re-exported terminally and via glob from `groxide_test_api`.
pub struct CrossStruct {
    /// A value carried by the struct.
    pub value: i32,
}

impl CrossStruct {
    /// Constructs a new `CrossStruct`.
    #[must_use]
    pub fn new(value: i32) -> Self {
        Self { value }
    }
}

/// A function re-exported terminally and via glob from `groxide_test_api`.
#[must_use]
pub fn cross_fn() -> &'static str {
    "from inner"
}

/// A constant re-exported terminally from `groxide_test_api`.
pub const CROSS_CONST: u32 = 42;

/// A module that holds an item; re-exported as a module by `groxide_test_api`,
/// which exercises the prefix-reexport-descent path.
pub mod cross_mod {
    /// An item nested inside the re-exported module.
    pub struct ModItem {
        /// A description.
        pub description: &'static str,
    }
}
