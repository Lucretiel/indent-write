/*!
Simple indentation adapters for [`io::Write`][std::io::Write],
[`fmt::Write`][std::fmt::Write], and [`Display`][std::fmt::Display]. Each
adapter wraps a writer or writable object, and inserts an indentation at
the front of each non-empty line.

See [`fmt::IndentWriter`], [`io::IndentWriter`], and
[`indentable::Indentable`] for examples.
*/

#![no_std]

#[cfg(feature = "std")]
extern crate std;

pub mod fmt;
pub mod indentable;

// Currently this is only used by `io`; remove the cfg when we update `fmt` to
// also use it
#[cfg(feature = "std")]
mod split;

#[cfg(feature = "std")]
pub mod io;
