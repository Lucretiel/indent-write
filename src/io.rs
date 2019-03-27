use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::str::{from_utf8, from_utf8_unchecked, Utf8Error};

use arrayvec::ArrayVec;

// TODO: for the love of god, coverage test this
// TODO: make this panic safe, or indicate somehow that it's not panic safe.

// Attempt to convert a byte slice to a string, in a context where the bytes
// are valid UTF-8 that was potentially cut off halfway through. If successful,
// the function will return the longest UTF-8 string possible, as well as the
// suffix of the bytes which are a partial UTF-8 code point. If any invalid
// bytes are encountered, return the Utf8Error.
fn partial_from_utf8(buf: &[u8]) -> Result<(&str, &[u8]), Utf8Error> {
    match from_utf8(buf) {
        Ok(buf_str) => Ok((buf_str, &[])),
        Err(err) if err.error_len().is_some() => Err(err),
        Err(err) => {
            let valid_utf8_boundary = err.valid_up_to();
            let good_part =
                unsafe { from_utf8_unchecked(buf.get_unchecked(..valid_utf8_boundary)) };
            let bad_part = unsafe { buf.get_unchecked(valid_utf8_boundary..) };
            Ok((good_part, bad_part))
        }
    }
}

// This wrapper for Utf8Error adjusts the reported offsets to be consistent
// with data passed by the user
#[derive(Debug, Clone)]
struct AdjustedUtf8Error {
    error: Utf8Error,
    offset: usize,
}

impl AdjustedUtf8Error {
    fn valid_up_to(&self) -> usize {
        self.error.valid_up_to() - self.offset
    }

    fn error_len(&self) -> Option<usize> {
        self.error.error_len().map(move |len| len - self.offset)
    }
}

impl Display for AdjustedUtf8Error {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self.error_len() {
            Some(error_len) => write!(
                f,
                "invalid utf-8 sequence of {} bytes from index {}",
                error_len,
                self.valid_up_to()
            ),
            None => write!(
                f,
                "incomplete utf-8 byte sequence from index {}",
                self.valid_up_to()
            ),
        }
    }
}

impl Error for AdjustedUtf8Error {
    fn cause(&self) -> Option<&dyn Error> {
        Some(&self.error)
    }
}

pub trait IndentableWrite: Sized + io::Write {
    fn indent_with_rules(self, prefix: &str, initial_indent: bool) -> IndentedWrite<Self>;

    #[inline]
    fn indent_with(self, prefix: &str) -> IndentedWrite<Self> {
        self.indent_with_rules(prefix, true)
    }

    #[inline]
    fn indent(self) -> IndentedWrite<'static, Self> {
        self.indent_with("\t")
    }
}

impl<W: io::Write> IndentableWrite for W {
    fn indent_with_rules(self, prefix: &str, initial_indent: bool) -> IndentedWrite<Self> {
        IndentedWrite {
            unprocessed_user_suffix: ArrayVec::new(),
            str_writer: IndentedStrWrite {
                writer: self,
                prefix,
                unwritten_continuation_bytes: ArrayVec::new(),
                unwritten_prefix: if initial_indent {
                    prefix.as_bytes()
                } else {
                    &[]
                },
            },
        }
    }
}

// We have to separate the implementation of IndentedWrite into a separate struct,
// called IndentedStrWrite, because part of the implementation of IndentedWrite::write
// calls the function write_str (which takes a mutable reference) using the contents of
// unprocessed_user_suffix. This could violate the borrow checker, so we split
// unprocessed_user_suffix into a separate struct, so that the mutable self in write_str doesn't
// touch it.
#[derive(Debug, Clone)]
struct IndentedStrWrite<'a, W: io::Write> {
    writer: W,

    // In the event that the underlying writer successfully writes only part
    // of a code point, store the unwritten bytes here, so we can try to write
    // them next time.
    unwritten_continuation_bytes: ArrayVec<[u8; 3]>,

    // FIXME: We never actually validate that prefix is valid UTF-8, since it's
    // basically deterministic when to insert it. Figure out if we should require
    // this to be a str anyway.
    prefix: &'a str,

    // If this is not empty, it is a (potentially partial) prefix we need to insert
    // before any of our next writes
    unwritten_prefix: &'a [u8],
}

impl<'a, W: io::Write> IndentedStrWrite<'a, W> {
    fn flush_unwritten(&mut self) -> io::Result<()> {
        while !self.unwritten_continuation_bytes.is_empty() {
            match self.writer.write(&self.unwritten_continuation_bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => self.unwritten_continuation_bytes.drain(..n),
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            };
        }

        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_unwritten()?;
        self.writer.flush()
    }

    // Write a string to the writer. This method allows us to separate the
    // complicated bytes <-> UTF-8 logic from the slightly less complicated
    // indentation logic. Returns the number of bytes written, which is guarenteed
    // to represent a whole number of code points.
    //
    // This function is mostly identical to fmt::IndentedWrite::write_str, with
    // the caveat that io::Write::write's contract requires us to report partial
    // success, which means we need to return early on partial success, just in
    // case a subsequent write call will contain an error.
    //
    // In order to guarentee that this method never writes a partial code point,
    // unwritten continuation bytes (that is, continuation bytes that were not
    // written by self.writer.write) are stored in unwritten_continuation_bytes
    // and reported to the caller as written
    fn write_str(&mut self, buf: &str) -> io::Result<usize> {
        // A note on ordering: it shouldn't be possible for bot unwritten_continuation_bytes
        // and unwritten_prefix to be non empty, so it doesn't matter what order these
        // two while loops happen in.

        while !self.unwritten_continuation_bytes.is_empty() {
            match self.writer.write(&self.unwritten_continuation_bytes) {
                Ok(n) if n != 0 => {
                    self.unwritten_continuation_bytes.drain(..n);
                }
                result => return result,
            }
        }

        while !self.unwritten_prefix.is_empty() {
            match self.writer.write(self.unwritten_prefix) {
                Ok(n) if n != 0 => {
                    // TODO: can we use get_unchecked here?
                    self.unwritten_prefix = &self.unwritten_prefix[n..];
                }
                result => return result,
            }
        }

        let buf_bytes = buf.as_bytes();

        let mut written = match buf.find('\n').map(|idx| idx + 1) {
            None => self.writer.write(buf_bytes)?,
            Some(newline_boundary) => {
                let upto_newline = unsafe { buf_bytes.get_unchecked(..newline_boundary) };
                let written = self.writer.write(upto_newline)?;

                if written == upto_newline.len() {
                    self.unwritten_prefix = self.prefix.as_bytes();
                    // We can return early cause we know that what was written
                    // was a whole number of code points, since it's precisely
                    // the length of upto_newline.
                    return Ok(written);
                }

                written
            }
        };

        // If there are any unwritten continuation bytes, buffer them
        // to unwritten_continuation_bytes, so that we can report that a whole
        // number of code points were written to the caller
        self.unwritten_continuation_bytes.extend(
            // TODO: can we use get_unchecked here?
            buf_bytes[written..]
                .iter()
                .cloned()
                .take_while(|&b| b & 0b1100_0000 == 0b1000_0000),
        );
        written += self.unwritten_continuation_bytes.len();

        Ok(written)
    }
}

impl<'a, W: io::Write> Drop for IndentedStrWrite<'a, W> {
    fn drop(&mut self) {
        let _result = self.flush_unwritten();
    }
}

// TODO: We can probably rename this to something like "Uft8Writer", since none of
// the logic in this struct has anything to do with the indentation part (it's all tied
// to fixing broken utf8 boundaries)
#[derive(Debug, Clone)]
pub struct IndentedWrite<'a, W: io::Write> {
    str_writer: IndentedStrWrite<'a, W>,
    // In the event the user supplies truncated UTF-8 as input, store the unwritten
    // bytes here, so that we can try to write them next time.
    unprocessed_user_suffix: ArrayVec<[u8; 4]>,
}

impl<'a, W: io::Write> io::Write for IndentedWrite<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Note to implementors: it is very important that this function fullfill the
        // Write contract: If this function returns an error, it means that 0 bytes
        // were written. This means that, in general, any successful writes of user
        // bytes need to result in an immediate successful return.
        // TODO: this is a very complicated algorithm; make sure it's panic-safe

        if buf.is_empty() {
            return Ok(0);
        }

        if self.unprocessed_user_suffix.is_empty() {
            match partial_from_utf8(buf) {
                Ok(("", suffix)) => {
                    self.unprocessed_user_suffix.extend(suffix.iter().cloned());
                    Ok(suffix.len())
                }
                Ok((valid_utf8, _)) => self.str_writer.write_str(valid_utf8),
                Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
            }
        } else {
            let original_unprocessed_len = self.unprocessed_user_suffix.len();
            self.unprocessed_user_suffix.extend(buf.iter().cloned());

            match partial_from_utf8(&self.unprocessed_user_suffix) {
                // The new bytes were bad. Truncate them and return the error.
                Err(err) => {
                    self.unprocessed_user_suffix
                        .truncate(original_unprocessed_len);

                    Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        AdjustedUtf8Error {
                            error: err,
                            offset: original_unprocessed_len,
                        },
                    ))
                }

                // The new bytes were good, but not enough for a code point.
                // Mark them as written (since we put them in the buffer)
                Ok(("", suffix)) => Ok(suffix.len() - original_unprocessed_len),

                // We have 1 or more code points! Try to write them
                Ok((data, _)) => match self.str_writer.write_str(data) {
                    // We successfully wrote something
                    Ok(written) if written > 0 => {
                        self.unprocessed_user_suffix.clear();
                        Ok(written - original_unprocessed_len)
                    }

                    // Failed to write the new bytes. We can't report them
                    // as having been written, since we need to pass our
                    // error back to the caller, so truncate.
                    result => {
                        self.unprocessed_user_suffix
                            .truncate(original_unprocessed_len);
                        result
                    }
                },
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.str_writer.flush()
    }
}

#[cfg(test)]
mod tests {
    mod test_partial_from_utf8 {
        use crate::io::partial_from_utf8;

        #[test]
        fn test_empty() {
            assert_eq!(partial_from_utf8(b""), Ok(("", b"" as &[u8])));
        }

        #[test]
        fn test_simple_string() {
            assert_eq!(
                partial_from_utf8(&[0x61, 0xC3, 0xA9]),
                Ok(("aé", b"" as &[u8]))
            );
        }

        #[test]
        fn test_partial_string() {
            // UTF-8 equivelent of "😀😀", minus the last byte
            assert_eq!(
                partial_from_utf8(&[0xF0, 0x9F, 0x98, 0x80, 0xF0, 0x9F, 0x98]),
                Ok(("😀", &[0xF0u8, 0x9Fu8, 0x98u8] as &[u8]))
            );
        }

        #[test]
        fn test_not_unicode() {
            match partial_from_utf8(&[0x61, 0xFF]) {
                Ok(_) => assert!(false),
                Err(err) => {
                    assert_eq!(err.valid_up_to(), 1);
                    assert!(err.error_len().is_some());
                }
            }
        }

        #[test]
        fn test_bad_unicode() {
            match partial_from_utf8(&[0x61, 0xF0, 0x9F, 0xF0]) {
                Ok(_) => assert!(false),
                Err(err) => {
                    assert_eq!(err.valid_up_to(), 1);
                    assert!(err.error_len().is_some());
                }
            }
        }
    }
}
