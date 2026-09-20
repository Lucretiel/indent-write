use std::io::{self, IoSlice};

use crate::split::Split;

/*
Some terminology, so that we can keep things straight: the indent writer is
concerned with prefixing every *line* with an indent, where a *line* consists of
the *content* (1+ non newlines) followed by the *terminator* (1+ newlines or
eof).
*/

#[inline(always)]
const fn is_newline(&b: &u8) -> bool {
    b == b'\n'
}

#[inline(always)]
const fn is_content(&b: &u8) -> bool {
    b != b'\n'
}

/**
Get the rest of a line from the front of a buffer, along with the number of
content (non-newline) bytes in the line.
*/
#[inline]
#[must_use]
fn get_line(buf: &[u8]) -> (&[u8], usize) {
    let cursor = Split::new(buf);
    match cursor.resplit_tail(is_content) {
        None => (buf, buf.len()),
        Some(content) => (
            content
                .resplit_tail(is_newline)
                .map(|line| line.head())
                .unwrap_or(buf),
            content.head().len(),
        ),
    }
}

/**
Adapter for writers to indent each line.

An `IndentWriter` adapts an [`io::Write`] object to insert an indent before each
non-empty line. Specifically, this means it will insert an indent after a
newline when followed by a non-newline.

These writers can be nested to provide increasing levels of indentation.

# Example

```
# use std::io::Write;
use indent_write::io::IndentWriter;

let output = Vec::new();

let mut indented = IndentWriter::new("\t", output);

// Lines will be indented
write!(indented, "Line 1\nLine 2\n");

// Empty lines will not be indented
write!(indented, "\n\nLine 3\n\n");

assert_eq!(indented.get_ref(), b"\tLine 1\n\tLine 2\n\n\n\tLine 3\n\n");
```
*/
#[derive(Debug, Clone)]
pub struct IndentWriter<'i, W> {
    writer: W,
    indent: &'i str,

    /**
    In general, self.indent[index_state..] is the indent we're actively trying
    to write. If this is >= indent.len(), the indent is empty, which means
    we're in the middle of writing a line and need to finish it before we reset
    the state to 0 to begin the next indent.
    */
    index_state: usize,
}

impl<'i, W: io::Write> IndentWriter<'i, W> {
    /// Create a new [`IndentWriter`].
    #[inline]
    #[must_use]
    pub fn new(indent: &'i str, writer: W) -> Self {
        Self {
            writer,
            indent,
            index_state: 0,
        }
    }

    /**
    Create a new [`IndentWriter`] which will not add an indent to the first
    written line.

    # Example

    ```
    # use std::io::Write;
    use indent_write::io::IndentWriter;

    let mut buffer = Vec::new();
    let mut writer = IndentWriter::new_skip_initial("    ", &mut buffer);

    writeln!(writer, "Line 1").unwrap();
    writeln!(writer, "Line 2").unwrap();
    writeln!(writer, "Line 3").unwrap();

    assert_eq!(buffer, b"Line 1\n    Line 2\n    Line 3\n")
    ```
    */
    #[inline]
    #[must_use]
    pub fn new_skip_initial(indent: &'i str, writer: W) -> Self {
        Self {
            writer,
            indent,
            index_state: indent.len(),
        }
    }
}

impl<'i, W> IndentWriter<'i, W> {
    /// Extract the writer from the [`IndentWriter`], discarding any in-progress
    /// indent state.
    #[inline]
    pub fn into_inner(self) -> W {
        self.writer
    }

    /// Get a reference to the wrapped writer
    #[inline]
    #[must_use]
    pub fn get_ref(&self) -> &W {
        &self.writer
    }

    /// Get the string being used as an indent for each line
    #[inline]
    #[must_use]
    pub fn indent(&self) -> &'i str {
        self.indent
    }
}

impl<'i, W> IndentWriter<'i, W> {
    /// Get the indent we're currently trying to write, if any.
    #[inline]
    #[must_use]
    fn indent_state(&self) -> &'i [u8] {
        self.indent
            .as_bytes()
            .get(self.index_state..)
            .unwrap_or_default()
    }
}

impl<'i, W: io::Write> io::Write for IndentWriter<'i, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Our main task here is to prefix every line with an indent. We use
        // vectored writes, one vectored write per line, and to avoid a lot
        // of complex variations we're always trying to get back into the
        // state where we can write_vectored([indent, line])
        loop {
            // Break up here, since that's the default. `continue` is the
            // exception, for cases where we only write some indent bytes and
            // need to retry writing user bytes.
            break match (get_line(buf), self.indent_state()) {
                // Skip writing an indent if we're in the middle of writing
                // a line (or if the user data is all newlines, we never want
                // to prefix newlines with an indent)
                ((line, content_len), []) | ((line, content_len @ 0), _) => {
                    self.writer.write(line).inspect(|&written| {
                        if written > content_len {
                            self.index_state = 0;
                        }
                    })
                }

                // This is our typical happy path: some indentation preceding a
                // line. Do a vectored write of the indent + line.
                ((line, content_len), indent) => {
                    let written = self
                        .writer
                        .write_vectored(&[IoSlice::new(indent), IoSlice::new(line)])?;

                    if written == 0 {
                        return Ok(0);
                    }

                    let buf_written = written.saturating_sub(indent.len());

                    self.index_state = match buf_written > content_len {
                        true => 0,
                        false => self.index_state.saturating_add(written),
                    };

                    match buf_written {
                        0 => continue,
                        w => Ok(w),
                    }
                }
            };
        }
    }

    // TODO: specialize write_vectored. Our opportunities to improve upon it
    // are limited, but it's probably worth it anyway in the common case where
    // line boundaries lie on entry boundaries.

    fn flush(&mut self) -> io::Result<()> {
        // NOTE: earlier versions of indent flushing tried to finish torn
        // indents, simulating that indents are buffered internally after they
        // start. Currently we try to allow for the possibility of inconsistent
        // user inputs, which means that we *only* try to write more indent
        // when the user is actively writing a new line.

        self.writer.flush()
    }
}
