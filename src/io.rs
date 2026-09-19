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

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineClass {
    /// The line was only content
    Content,

    /**
    The line includes at least one newline. This means that iff the *whole* line
    was written, we need to be ready to start writing an indent if any content
    appears.
    */
    Terminated,
}

/**
Get the rest of a line from the front of a buffer, along with an indication if
it had any amount of terminator. The line may have been torn, so there may be 0
or more content. Get as much of the line as possible.
*/
#[inline]
#[must_use]
fn get_line(buf: &[u8]) -> (&[u8], LineClass) {
    let cursor = Split::new(buf);
    match cursor.resplit_tail(is_newline) {
        None => (buf, LineClass::Content),
        Some(content) => match content.resplit_tail(is_content) {
            None => (buf, LineClass::Terminated),
            Some(line) => (line.head(), LineClass::Terminated),
        },
    }
}

/**
Get any leading newlines from the buffer (which would be trailing a line-in-
progress). Returns `None` iff the buffer *starts* with a non-newline character.
*/
#[inline]
#[must_use]
fn get_leading_newlines(buf: &[u8]) -> Option<&[u8]> {
    match buf.iter().position(is_content) {
        None => Some(buf),
        Some(0) => None,
        Some(idx) => Some(&buf[..idx]),
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
    /// Get the indent we're currently trying to write, if any. Guaranteed
    /// to return a non-empty slice, or None.
    #[inline]
    #[must_use]
    fn indent_state(&self) -> Option<&'i [u8]> {
        self.indent
            .as_bytes()
            .get(self.index_state..)
            .filter(|b| !b.is_empty())
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
            // exception.
            break match self.indent_state() {
                Some(indent) => match get_leading_newlines(buf) {
                    // If there are leading newlines here, they're a part of
                    // the previous line. Write them out before attempting to
                    // write any indent.
                    Some(leading) => self.writer.write(leading),

                    // This is the normal happy path case: a new, non-empty
                    // line that we want to prefix with an indent. Do the
                    // vectored write and update the state appropriately.
                    None => {
                        let (line, class) = get_line(buf);
                        let total_len = line.len() + indent.len();
                        let written = self
                            .writer
                            .write_vectored(&[IoSlice::new(indent), IoSlice::new(line)])?;

                        // Update the state; if this line had a terminator,
                        // AND we wrote the entire line, reset the state to
                        // begin writing a new indent.
                        self.index_state = match (written, class) {
                            (0, _) => return Ok(0),
                            (n, LineClass::Terminated) if n >= total_len => 0,
                            _ => self.index_state.saturating_add(written),
                        };

                        // Return from this write call only if we successfully
                        // wrote any user bytes; otherwise, loop around and
                        // try to write some more. The indent is definitely
                        // non-empty, so no issues with WriteZero here.
                        match written.checked_sub(indent.len()) {
                            None | Some(0) => continue,
                            Some(buf_written) => Ok(buf_written),
                        }
                    }
                },

                // We tore a write somewhere; finish writing the current line
                // before we try the indent thing again
                None => {
                    let (line, class) = get_line(buf);
                    self.writer.write(line).inspect(|&written| {
                        if class == LineClass::Terminated && written >= line.len() {
                            self.index_state = 0;
                        }
                    })
                }
            };
        }
    }

    // TODO: specialize write_vectored. Our opportunities to improve upon it
    // are limited, but it's probably worth it anyway in the common case where
    // lines boundaries lie on entry boundaries.

    fn flush(&mut self) -> io::Result<()> {
        // If we've written a partial indent, we should try to finish it
        while let Some(indent) = self.indent_state() {
            // Don't write a whole new indent, only complete one in progress
            if indent.len() >= self.indent.len() {
                break;
            }

            // Can't use `write_all` because we need to keep the state
            // consistent in the event of an error.
            self.index_state = match self.writer.write(indent) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(n) => self.index_state.saturating_add(n),
                Err(err) if err.kind() == io::ErrorKind::Interrupted => self.index_state,
                Err(err) => return Err(err),
            }
        }

        self.writer.flush()
    }
}
