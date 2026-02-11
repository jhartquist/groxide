//! A module for testing deeply nested paths.
//!
//! Contains nested sub-modules to verify groxide handles 3+ level paths.

pub mod nested;

/// A marker type at the `deeply` module level.
pub struct DeepMarker;
