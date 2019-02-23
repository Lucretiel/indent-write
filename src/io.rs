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

pub trait IndentableWrite: Sized + io::Write {
    fn indent_with_rules(
        self,
        prefix: &str,
        initial_indent: bool,
        trim_user_indents: bool,
    ) -> IndentedWrite<Self>;

    #[inline]
    fn indent_with(self, prefix: &str) -> IndentedWrite<Self> {
        self.indent_with_rules(prefix, true, false)
    }

    #[inline]
    fn indent(self) -> IndentedWrite<'static, Self> {
        self.indent_with("\t")
    }
}

impl<W: io::Write> IndentableWrite for W {
    fn indent_with_rules(
        self,
        prefix: &str,
        initial_indent: bool,
        trim_user_indents: bool,
    ) -> IndentedWrite<Self> {
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
                trim_user_indents,
                is_trimming_indents: trim_user_indents && initial_indent,
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

    // FIXME: We never actually validate that prefix is valid UTF-8, since it's
    // basically deterministic when to insert it. Figure out if we should require
    // this to be a str anyway.
    prefix: &'a str,

    // In the event that the underlying writer successfully writes only part
    // of a code point, store the unwritten bytes here, so we can try to write
    // them next time.
    unwritten_continuation_bytes: ArrayVec<[u8; 3]>,

    // If this is not empty, it is a (potentially partial) prefix we need to insert
    // before any of our next writes
    unwritten_prefix: &'a [u8],

    // True if we want to strip any user-provided indentation.
    trim_user_indents: bool,

    // True if we're in the middle of stripping off a user indent.
    is_trimming_indents: bool,
}

impl<'a, W: io::Write> IndentedStrWrite<'a, W> {
    fn flush(&mut self) -> io::Result<()> {
        while !self.unwritten_continuation_bytes.is_empty() {
            match self.writer.write(&self.unwritten_continuation_bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => self.unwritten_continuation_bytes.drain(..n),
                Err(ref err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(err),
            };
        }

        // Right now, we don't flush the unwritten_prefix, because we haven't
        // made any promises to the client that it's been written, and as a rule
        // we want to be as conservative as possible when it comes to writing the
        // prefix.

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
        // "Write" the indent. Trimming means that we don't write those bytes to
        // the underlying writer, but we still have to report them as written to
        // the user.
        if self.is_trimming_indents {
            let trimmed = buf.trim_start();
            if trimmed.len() != buf.len() {
                return Ok(buf.len() - trimmed.len());
            }
            self.is_trimming_indents = false;
        }

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
                    if self.trim_user_indents {
                        self.is_trimming_indents = true;
                    }
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
                .take_while(|&b| b >= 0b1000_0000 && b <= 0b1011_1111),
        );
        written += self.unwritten_continuation_bytes.len();

        Ok(written)
    }
}

impl<'a, W: io::Write> Drop for IndentedStrWrite<'a, W> {
    fn drop(&mut self) {
        let _result = self.flush();
    }
}
// TODO: add a Drop implementation. Is it appropriate to allow unprocessed_user_suffix
// to be silently dropped, even though we previously indicated a successful write?

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

        match self.unprocessed_user_suffix.first().cloned() {
            None => match partial_from_utf8(buf) {
                // At this point, since unprocessed_user_suffix is empty, the incoming
                // bytes should be valid UTF-8, possibly cut off in the middle of a code point.
                Ok(("", suffix)) => {
                    self.unprocessed_user_suffix.extend(suffix.iter().cloned());
                    Ok(suffix.len())
                }
                Ok((valid_utf8, _)) => self.str_writer.write_str(valid_utf8),
                Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
            },
            Some(b) => {
                let current_length = self.unprocessed_user_suffix.len();
                let target_length = match b & 0b1111_0000 {
                    0b1100_0000 => 2,
                    0b1110_0000 => 3,
                    0b1111_0000 => 4,
                    _ => unreachable!(
                        "unprocessed_user_suffix has an invalid leading UTF-8 byte: {:#X}",
                        b
                    ),
                };
                self.unprocessed_user_suffix
                    .extend(buf.iter().take(target_length - current_length).cloned());

                match partial_from_utf8(&self.unprocessed_user_suffix) {
                    Err(err) => {
                        // In this case, the new bytes were invalid. Truncate them and return the error.
                        self.unprocessed_user_suffix.truncate(current_length);

                        // TODO: this err is based on invisible state in the Writer, is it
                        // appropriate to include it?
                        Err(io::Error::new(io::ErrorKind::InvalidData, err))
                    }
                    Ok(("", _)) => {
                        // In this case, we didn't have enough new data. Mark the bytes as written
                        // (since we put them in the unprocessed_user_suffix)
                        Ok(self.unprocessed_user_suffix.len() - current_length)
                    }
                    Ok((data, &[])) => match self.str_writer.write_str(data) {
                        Ok(written) if written > 0 => {
                            // It shouldn't be possible for written to be a different length
                            // than unprocessed_user_suffix.len(), since write_str guarentees at
                            // least that an integer number of code points are written, and we
                            // only have one code point to offer
                            debug_assert_eq!(self.unprocessed_user_suffix.len(), written);

                            self.unprocessed_user_suffix.clear();

                            // Subtract current_len, since we already told the user that those bytes
                            // were written during a previous write call.
                            Ok(written - current_length)
                        }
                        result => {
                            // Failed to write the new bytes. We can't record them as being
                            // written, so truncate the unprocessed_user_suffix back to its
                            // original size
                            self.unprocessed_user_suffix.truncate(current_length);
                            result
                        }
                    },
                    Ok(_) => unreachable!(
                        "unprocessed_user_suffix had bytes from more than one code point: {:?}",
                        self.unprocessed_user_suffix
                    ),
                }
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
