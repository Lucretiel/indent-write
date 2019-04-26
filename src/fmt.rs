use std::fmt;

pub trait IndentableWrite: Sized {
    fn indent_with_rules(
        self,
        prefix: &str,
        initial_indent: bool,
    ) -> IndentedWrite<Self>;

    #[inline]
    fn indent_with(self, prefix: &str) -> IndentedWrite<Self> {
        self.indent_with_rules(prefix, true)
    }

    #[inline]
    fn indent(self) -> IndentedWrite<'static, Self> {
        self.indent_with("\t")
    }
}

impl<W: fmt::Write> IndentableWrite for W {
    fn indent_with_rules(
        self,
        prefix: &str,
        initial_indent: bool,
    ) -> IndentedWrite<Self> {
        IndentedWrite {
            writer: self,
            prefix,
            insert_indent: initial_indent,
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndentedWrite<'a, W> {
    writer: W,
    prefix: &'a str,

    // True if we need to insert an indent before our next write
    insert_indent: bool,
}

impl<'a, W> IndentedWrite<'a, W> {
    pub fn dedent(self) -> W {
        self.writer
    }
}

impl<'a, W: fmt::Write> fmt::Write for IndentedWrite<'a, W> {
    fn write_str(&mut self, mut buf: &str) -> Result<(), fmt::Error> {
        // TODO: this is a highly stateful algorithm. Make sure it's panic-safe
        // against self.writer.write_str

        while !buf.is_empty() {
            if self.insert_indent {
                self.writer.write_str(self.prefix)?;
                self.insert_indent = false;
            }

            // This +1 is safe because string lengths must fit in a usize, so
            // the index of the newline character is necessarily less than USIZE_MAX
            match buf.find('\n').map(|idx| idx + 1) {
                None => return self.writer.write_str(buf),
                Some(newline_boundary) => {
                    self.writer
                        .write_str(unsafe { buf.get_unchecked(..newline_boundary) })?;
                    self.insert_indent = true;
                    buf = unsafe { buf.get_unchecked(newline_boundary..) };
                }
            }
        }

        Ok(())
    }
}
