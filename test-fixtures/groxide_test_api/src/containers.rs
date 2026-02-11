//! Container types for testing module-level items.
//!
//! This module provides various container types used to exercise
//! groxide's rendering of structs, enums, and associated items.

/// A stack data structure backed by a `Vec`.
///
/// # Examples
///
/// ```
/// use groxide_test_api::containers::Stack;
/// let mut s = Stack::new();
/// s.push(1);
/// s.push(2);
/// assert_eq!(s.pop(), Some(2));
/// ```
pub struct Stack<T> {
    elements: Vec<T>,
}

impl<T> Stack<T> {
    /// Creates a new empty stack.
    pub fn new() -> Self {
        Self {
            elements: Vec::new(),
        }
    }

    /// Pushes a value onto the top of the stack.
    pub fn push(&mut self, value: T) {
        self.elements.push(value);
    }

    /// Removes and returns the top element, or `None` if empty.
    pub fn pop(&mut self) -> Option<T> {
        self.elements.pop()
    }

    /// Returns `true` if the stack contains no elements.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Returns the number of elements in the stack.
    pub fn len(&self) -> usize {
        self.elements.len()
    }
}

impl<T> Default for Stack<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A pair of two values, possibly of different types.
pub struct Pair<A, B> {
    first: A,
    second: B,
}

impl<A, B> Pair<A, B> {
    /// Creates a new pair.
    pub fn new(first: A, second: B) -> Self {
        Self { first, second }
    }

    /// Returns a reference to the first element.
    pub fn first(&self) -> &A {
        &self.first
    }

    /// Returns a reference to the second element.
    pub fn second(&self) -> &B {
        &self.second
    }
}

/// The maximum capacity for a fixed-size container.
pub const MAX_CAPACITY: usize = 256;
