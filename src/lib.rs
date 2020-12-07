//! Simple indentation adapters for [`io::Write`][std::io::Write] and
//! [`fmt::Write`][std::fmt::Write]. Each adapter wraps a writer object, and
//! inserts an indentation at the front of each non-empty line written to that
//! writer.
//!
//! See [`fmt::IndentWriter`] and [`io::IndentWriter`] for examples.

pub mod fmt;
pub mod io;

trait Inspect<T> {
    fn inspect(self, func: impl FnOnce(&T)) -> Self;
}

impl<T> Inspect<T> for Option<T> {
    #[inline]
    fn inspect(self, func: impl FnOnce(&T)) -> Self {
        if let Some(ref value) = self {
            func(value)
        }

        self
    }
}

impl<T, E> Inspect<T> for Result<T, E> {
    #[inline]
    fn inspect(self, func: impl FnOnce(&T)) -> Self {
        if let Ok(ref value) = self {
            func(value)
        }

        self
    }
}
