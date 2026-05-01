//! Cross-crate re-exports — exercises:
//!
//! * `try_resolve_via_glob_reexport` (wildcard `pub use other::*`)
//! * `try_resolve_via_prefix_reexport` (module re-export walked through)
//! * `try_follow_reexport` + `follow_by_name` (terminal stubs)

/// Terminal cross-crate stub. Queries for
/// `groxide_test_api::cross_reexports::CrossStruct` should follow the stub
/// to the canonical struct in `groxide_test_inner`.
pub use groxide_test_inner::CrossStruct;

/// Terminal cross-crate stub for a function.
pub use groxide_test_inner::cross_fn;

/// Cross-crate module re-export. Queries for
/// `groxide_test_api::cross_reexports::cross_mod::ModItem` exercise
/// the prefix descent: the lookup of "cross_mod" finds a stub here,
/// the code follows it into `groxide_test_inner`, and resolves
/// "ModItem" inside the inner crate's `cross_mod`.
pub use groxide_test_inner::cross_mod;

/// Wildcard re-export of every public item in `groxide_test_inner`. Items
/// re-exported this way don't get individual paths in the index — instead
/// they're discovered via `glob_uses` at query time. Queries for items NOT
/// also re-exported terminally (e.g. `CROSS_CONST`) only resolve through
/// the glob path.
pub use groxide_test_inner::*;
