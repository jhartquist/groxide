//! Module for testing re-export handling.
//!
//! Contains both specific `pub use` re-exports and glob re-exports.

mod inner {
    /// A helper struct defined in a private inner module.
    pub struct Helper {
        /// Identifier for this helper.
        pub id: u32,
    }

    /// A helper function in the inner module.
    pub fn inner_fn() -> i32 {
        99
    }

    /// A constant in the inner module.
    pub const INNER_CONST: &str = "inner";
}

mod glob_source {
    /// An item that will be glob-reexported.
    pub struct GlobItem {
        /// Value field.
        pub value: i32,
    }

    /// Another item for glob re-export.
    pub fn glob_fn() -> bool {
        true
    }

    /// A type alias in the glob source.
    pub type GlobAlias = Vec<String>;
}

/// Re-exported `Helper` from the private inner module.
pub use inner::Helper;

/// Re-exported `inner_fn` from the private inner module.
pub use inner::inner_fn;

/// Glob re-export of everything from `glob_source`.
pub use glob_source::*;
