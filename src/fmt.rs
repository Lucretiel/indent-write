use core::{fmt, mem};

/**
Iterator over lines of the input. Distinct from the standard library
`str.lines()` because it treats a line as ending with *any* quantity of
newlines.
 */
struct Lines<'a> {
    input: &'a str,
}

impl<'a> Iterator for Lines<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let newline_idx = match self.input.as_bytes().iter().position(|&b| b == b'\n') {
            None if self.input.is_empty() => return None,
            None => return Some(mem::take(&mut self.input)),
            Some(idx) => idx,
        };

        // Safety: `newline_idx` is the index of a newline byte, which is
        // definitely a sound place to split a string
        let tail = unsafe { self.input.get_unchecked(newline_idx..) };
        let next_line_idx = match tail.as_bytes().iter().position(|&b| b != b'\n') {
            None => return Some(mem::take(&mut self.input)),
            Some(idx) => idx,
        };

        let point = newline_idx + next_line_idx;

        // Safety: `next_line_idx` is the index of the first byte of `tail`
        // that is not a newline byte. This means it is either the front
        // of the string or it immediately follows a newline byte, making it
        // a sound place to split a string. `tail` is `input[newline_idx..]`,
        // so their sum is a safe place to split the string as well.
        let line = unsafe { self.input.get_unchecked(..point) };

        // Safety: see previous
        self.input = unsafe { self.input.get_unchecked(point..) };

        Some(line)
    }
}

/**
Adapter for writers to indent each line.

An `IndentWriter` adapts a [`fmt::Write`] object to insert an indent before each
non-empty line. Specifically, this means it will insert an indent between each
newline when followed by a non-newline.

These writers can be nested to provide increasing levels of indentation.

# Example

```
# use std::fmt::Write;
use indent_write::fmt::IndentWriter;

let output = String::new();

let mut indented = IndentWriter::new("\t", output);

// Lines will be indented
write!(indented, "Line 1\nLine 2\n");

// Empty lines will not be indented
write!(indented, "\n\nLine 3\n\n");

assert_eq!(indented.get_ref(), "\tLine 1\n\tLine 2\n\n\n\tLine 3\n\n");
```
*/
#[derive(Debug, Clone)]
pub struct IndentWriter<'i, W> {
    writer: W,
    indent: &'i str,
    need_indent: bool,
}

impl<'i, W: fmt::Write> IndentWriter<'i, W> {
    /// Create a new [`IndentWriter`].
    #[inline]
    pub fn new(indent: &'i str, writer: W) -> Self {
        Self {
            writer,
            indent,
            need_indent: true,
        }
    }

    /**
    Create a new [`IndentWriter`] which will not add an indent to the first
    written line.

    # Example

    ```
    # use std::fmt::Write;
    use indent_write::fmt::IndentWriter;

    let mut buffer = String::new();
    let mut writer = IndentWriter::new_skip_initial("    ", &mut buffer);

    writeln!(writer, "Line 1").unwrap();
    writeln!(writer, "Line 2").unwrap();
    writeln!(writer, "Line 3").unwrap();

    assert_eq!(buffer, "Line 1\n    Line 2\n    Line 3\n")
    ```
    */
    #[inline]
    pub fn new_skip_initial(indent: &'i str, writer: W) -> Self {
        Self {
            writer,
            indent,
            need_indent: false,
        }
    }

    /// Extract the writer from the `IndentWriter`, discarding any in-progress
    /// indent state.
    #[inline]
    pub fn into_inner(self) -> W {
        self.writer
    }

    /// Get a reference to the wrapped writer
    #[inline]
    pub fn get_ref(&self) -> &W {
        &self.writer
    }

    /// Get the string being used as an indent for each line
    #[inline]
    pub fn indent(&self) -> &'i str {
        self.indent
    }
}

impl<'i, W: fmt::Write> fmt::Write for IndentWriter<'i, W> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let mut lines = Lines { input: s };

        let Some(first_line) = lines.next() else {
            return Ok(());
        };

        if self.need_indent && !first_line.starts_with("\n") {
            self.writer.write_str(self.indent)?;
        }
        self.writer.write_str(first_line)?;
        self.need_indent = first_line.ends_with("\n");

        // Write out the remaining lines; prefix them all unconditionally with
        // indents
        for line in lines {
            self.writer.write_str(self.indent)?;
            self.writer.write_str(line)?;
            self.need_indent = line.ends_with("\n");
        }

        Ok(())
    }

    fn write_char(&mut self, c: char) -> fmt::Result {
        // We need an indent, and this is the start of a non-empty line.
        // Insert the indent.
        if self.need_indent && c != '\n' {
            self.writer.write_str(self.indent)?;
            self.need_indent = false;
        }
        // This is the end of a non-empty line. Request an indent.

        if !self.need_indent && c == '\n' {
            self.need_indent = true;
        }

        self.writer.write_char(c)
    }
}
