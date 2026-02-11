//! Trait definitions for testing trait rendering.
//!
//! Contains traits with required methods, provided methods, associated types,
//! and associated constants.

/// A trait for items that can be serialized to a string representation.
///
/// # Examples
///
/// Implement `Stringify` for a custom type:
///
/// ```
/// use groxide_test_api::traits::Stringify;
///
/// struct Point(f64, f64);
///
/// impl Stringify for Point {
///     fn stringify(&self) -> String {
///         format!("({}, {})", self.0, self.1)
///     }
/// }
/// ```
pub trait Stringify {
    /// Converts this value to its string representation.
    ///
    /// This is a required method that all implementors must define.
    fn stringify(&self) -> String;

    /// Returns a debug-formatted string representation.
    ///
    /// This is a provided method with a default implementation.
    fn debug_string(&self) -> String {
        format!("[debug: {}]", self.stringify())
    }
}

/// A trait with an associated type and constant.
pub trait Processor {
    /// The type of input this processor accepts.
    type Input;
    /// The type of output this processor produces.
    type Output;

    /// The maximum number of items this processor can handle.
    const MAX_ITEMS: usize;

    /// Processes a single input and returns the output.
    fn process(&self, input: Self::Input) -> Self::Output;

    /// Returns whether this processor is ready.
    fn is_ready(&self) -> bool {
        true
    }
}

/// A trait that extends both `Stringify` and `std::fmt::Debug`.
pub trait Describable: Stringify + std::fmt::Debug {
    /// Returns a human-readable description.
    fn describe(&self) -> String;
}

impl Stringify for String {
    fn stringify(&self) -> String {
        self.clone()
    }
}

impl Stringify for i32 {
    fn stringify(&self) -> String {
        self.to_string()
    }
}
