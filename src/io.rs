use std::cmp::min;
use std::hint::unreachable_unchecked;
use std::io;
use std::str::{from_utf8, from_utf8_unchecked, Utf8Error};

use arrayvec::ArrayVec;

//TODO: for the love of god, coverage test this

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

pub trait IndentableWrite: Sized {
    fn indent_with_rules(
        self,
        prefix: &[u8],
        initial_indent: bool,
        trim_user_indents: bool,
    ) -> IndentedWrite<Self>;

    #[inline]
    fn indent_with(self, prefix: &[u8]) -> IndentedWrite<Self> {
        self.indent_with_rules(prefix, true, false)
    }

    #[inline]
    fn indent(self) -> IndentedWrite<'static, Self> {
        self.indent_with(&[b'\t'])
    }
}

impl<W: io::Write> IndentableWrite for W {
    fn indent_with_rules(
        self,
        prefix: &[u8],
        initial_indent: bool,
        trim_user_indents: bool,
    ) -> IndentedWrite<Self> {
        IndentedWrite {
            unprocessed_user_suffix: ArrayVec::new(),
            str_writer: IndentedStrWrite {
                writer: self,
                prefix,
                unwritten_continuation_bytes: ArrayVec::new(),
                insert_indent: initial_indent,
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
struct IndentedStrWrite<'a, W> {
    writer: W,

    // FIXME: We never actually validate that prefix is valid UTF-8, since it's
    // basically deterministic when to insert it. Figure out if we should require
    // this to be a str anyway.
    prefix: &'a [u8],

    // In the event that the underlying writer successfully writes only part
    // of a code point, store the unwritten bytes here, so we can try to write
    // them next time.
    unwritten_continuation_bytes: ArrayVec<[u8; 3]>,

    // True if we need to insert an indent before our next write
    insert_indent: bool,

    // True if we want to strip any user-provided indentation.
    trim_user_indents: bool,

    // True if we're in the middle of stripping off a user indent.
    is_trimming_indents: bool,
}

impl<'a, W: io::Write> IndentedStrWrite<'a, W> {
    fn flush_continuation_bytes(&mut self) -> io::Result<()> {
        while self.unwritten_continuation_bytes.len() > 0 {
            match self.writer.write(&self.unwritten_continuation_bytes) {
                Err(err) => return Err(err),
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(len) => self.unwritten_continuation_bytes.drain(..len),
            };
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_continuation_bytes()?;
        self.writer.flush()
    }
    // Write a string to the writer. This method allows us to separate the complicated
    // bytes <-> UTF-8 logic from the slightly less indentation logic. Returns the number
    // of bytes written, which is guarenteed to represent a whole number of code points.
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
        self.flush_continuation_bytes()?;

        if self.is_trimming_indents {
            let trimmed = buf.trim_start();
            if trimmed.len() != buf.len() {
                return Ok(buf.len() - trimmed.len());
            }
            self.is_trimming_indents = false;
        }

        if self.insert_indent {
            self.writer.write_all(self.prefix)?;
            self.insert_indent = false;
        }

        let buf_bytes = buf.as_bytes();

        let mut written = match buf.find('\n').map(|idx| idx + 1) {
            None => self.writer.write(buf_bytes)?,
            Some(newline_boundary) => {
                let upto_newline = unsafe { buf_bytes.get_unchecked(..newline_boundary) };
                let written = self.writer.write(upto_newline)?;

                if written == upto_newline.len() {
                    self.insert_indent = true;
                    if self.trim_user_indents {
                        self.is_trimming_indents = true;
                    }
                    return Ok(written);
                }

                written
            }
        };

        let unwritten_part = unsafe { buf_bytes.get_unchecked(written..) };
        // If there are any unwritten continuation bytes, add them
        // to unwritten_continuation_bytes.
        for &b in unwritten_part {
            if b >= 0b1000_0000 && b <= 0b1011_1111 {
                self.unwritten_continuation_bytes.push(b);

                // This is effectively a write, since the user shouldn't
                // retry this byte.
                written += 1;
            } else {
                break;
            }
        }

        Ok(written)
    }
}

// TODO: We can probably rename this to something like "Uft8Writer", since none of
// the logic in this struct has anything to do with the indentation part (it's all tied
// to fixing broken utf8 boundaries)
#[derive(Debug, Clone)]
pub struct IndentedWrite<'a, W> {
    str_writer: IndentedStrWrite<'a, W>,
    // In the event the user supplies truncated UTF-8 as input, store the unwritten
    // bytes here, so that we can try to write them next time.
    unprocessed_user_suffix: ArrayVec<[u8; 4]>,
}

impl<'a, W> IndentedWrite<'a, W> {
    pub fn dedent(self) -> W {
        self.str_writer.writer
    }
}

impl<'a, W: io::Write> io::Write for IndentedWrite<'a, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Note to implementors: it is very important that this function fullfill the
        // Write contract: If this function returns an error, it means that 0 bytes
        // were written. This means that, in general, any successful writes of user
        // bytes need to result in an immediate return.
        // TODO: this is a very complicated algorithm; make sure it's panic-safe

        // If we have an unprocessed user suffix, try to add bytes to it until we have at least a
        // whole code point, then try to write that one code point with write_str.
        if !self.unprocessed_user_suffix.is_empty() {
            let current_length = self.unprocessed_user_suffix.len();
            let target_length = match self.unprocessed_user_suffix.first().cloned() {
                Some(0b1100_0000...0b1101_1111) => 2,
                Some(0b1110_0000...0b1110_1111) => 3,
                Some(0b1111_0000...0b1111_0111) => 4,
                Some(b) => unreachable!(
                    "Invalid byte '{:#X}' in the unprocessed_user_suffix buffer",
                    b
                ),
                None => unsafe { unreachable_unchecked() },
            };

            let ext = &buf[..min(target_length - current_length, buf.len())];
            self.unprocessed_user_suffix.extend(ext.iter().cloned());

            match partial_from_utf8(&self.unprocessed_user_suffix) {
                Err(err) => {
                    // In this case, the new bytes were invalid. Truncate them and return the error.
                    self.unprocessed_user_suffix.truncate(current_length);
                    Err(io::Error::new(io::ErrorKind::InvalidData, err))
                }
                Ok(("", _)) => {
                    // In this case, we didn't have enough new data. Mark the bytes as written
                    // (since we put them in the unprocessed_user_suffix)
                    Ok(ext.len())
                }
                Ok((data, &[])) => {
                    // We have a complete code point. Write it with write_str. If there's an error,
                    // re-truncate and pass it back to the client.
                    match self.str_writer.write_str(data) {
                        Err(err) => {
                            self.unprocessed_user_suffix.truncate(current_length);
                            Err(err)
                        }
                        Ok(written) => {
                            // It shouldn't be possible for written to be a different length
                            // than unprocessed_user_suffix.len(), since write_str guarentees at
                            // least that an integer number of code points are written.
                            debug_assert_eq!(self.unprocessed_user_suffix.len(), written);

                            // This means that it shouldn't be possible that 0 new bytes that we
                            // got from the user were written (because it adds unwritten bytes to
                            // unwritten_continuation_bytes)
                            debug_assert_ne!(written - current_length, 0);

                            self.unprocessed_user_suffix.clear();

                            // Subtract current_len, since we already told the user that those bytes
                            // were written during a previous write call.
                            Ok(written - current_length)
                        }
                    }
                }
                Ok(_) => unreachable!(
                    "unprocessed_user_suffix had bytes from more than one code point: {:?}",
                    self.unprocessed_user_suffix
                ),
            }
        } else {
            // At this point, both unwritten_continuation_bytes and unprocessed_user_suffix
            // are empty, which means that the incoming buf SHOULD be a valid UTF-8 string (or, at
            // least, it starts with one; the ending code point may be cut off).
            match partial_from_utf8(buf) {
                Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
                Ok(("", suffix)) => {
                    self.unprocessed_user_suffix.extend(suffix.iter().cloned());
                    Ok(suffix.len())
                }
                Ok((valid_utf8, _)) => {
                    // Note: we could check if write_str wrote all the bytes of valid_utf8, and if
                    // so, store the suffix bytes and report the whole thing as written. However,
                    // we'd rather if we never hit the code path for unprocessed_user_suffix.
                    self.str_writer.write_str(valid_utf8)
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
