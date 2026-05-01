//! A test fixture crate exercising all Rust item kinds for groxide testing.
//!
//! This crate exists solely to produce rustdoc JSON that covers every item kind
//! groxide needs to handle: structs, enums, traits, functions, constants, type aliases,
//! macros, statics, unions, re-exports, nested modules, and more.

pub mod containers;
pub mod cross_reexports;
pub mod deeply;
pub mod reexports;
pub mod traits;

/// A generic container holding a single value.
///
/// # Examples
///
/// ```
/// use groxide_test_api::GenericStruct;
/// let gs = GenericStruct::new(42);
/// assert_eq!(*gs.value(), 42);
/// ```
pub struct GenericStruct<T: Clone> {
    value: T,
}

impl<T: Clone> GenericStruct<T> {
    /// Creates a new `GenericStruct` with the given value.
    pub fn new(value: T) -> Self {
        Self { value }
    }

    /// Returns a reference to the inner value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Converts into the inner value, consuming self.
    pub fn into_inner(self) -> T {
        self.value
    }
}

/// A simple struct with no generics.
pub struct SimpleStruct {
    /// The name field.
    pub name: String,
    /// The count field.
    pub count: u32,
}

/// An enum representing cardinal directions.
///
/// Used for testing enum variant rendering.
pub enum Direction {
    /// Points north.
    North,
    /// Points south.
    South,
    /// Points east.
    East,
    /// Points west.
    West,
}

/// An enum with data-carrying variants.
pub enum Shape {
    /// A circle with a radius.
    Circle(f64),
    /// A rectangle with width and height.
    Rectangle {
        /// Width of the rectangle.
        width: f64,
        /// Height of the rectangle.
        height: f64,
    },
    /// A point with no data.
    Point,
}

/// Adds two numbers together.
///
/// # Examples
///
/// ```
/// assert_eq!(groxide_test_api::add(2, 3), 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// A function with multiple generic parameters.
pub fn generic_fn<T: std::fmt::Display, U: Into<String>>(value: T, _label: U) -> String {
    format!("{value}")
}

/// The maximum buffer size in bytes.
pub const MAX_BUFFER_SIZE: usize = 4096;

/// The default greeting string.
pub const DEFAULT_GREETING: &str = "hello";

/// A type alias for results in this crate.
pub type Result<T> = std::result::Result<T, String>;

/// A type alias for a callback function.
pub type Callback = fn(i32) -> bool;

/// A global counter for testing static items.
pub static GLOBAL_COUNTER: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// A simple static string.
pub static VERSION: &str = "0.1.0";

/// A union for testing union item rendering.
///
/// # Safety
///
/// Accessing union fields requires `unsafe` since the compiler cannot guarantee
/// which field was last written.
pub union IntOrFloat {
    /// The integer interpretation.
    pub i: i32,
    /// The float interpretation.
    pub f: f32,
}

/// Generates a greeting string.
///
/// # Examples
///
/// ```
/// let msg = groxide_test_api::greet!("world");
/// assert_eq!(msg, "Hello, world!");
/// ```
#[macro_export]
macro_rules! greet {
    ($name:expr) => {
        format!("Hello, {}!", $name)
    };
}

/// An item only available with the `unstable` feature.
#[cfg(feature = "unstable")]
pub fn unstable_api() -> &'static str {
    "unstable"
}

/// A feature-gated struct.
#[cfg(feature = "unstable")]
pub struct UnstableStruct {
    /// An experimental field.
    pub experimental: bool,
}

/// A function demonstrating unicode in documentation.
///
/// Supports multi-language output: "こんにちは" (Japanese), "café" (French),
/// "naïve" (English), and emoji: 🦀🔧.
///
/// Mathematical symbols: ∀x ∈ ℝ, x² ≥ 0.
pub fn unicode_docs() -> &'static str {
    "こんにちは"
}

// Private items (should appear with --private flag)
fn private_helper() -> i32 {
    42
}

struct PrivateStruct {
    _secret: String,
}
