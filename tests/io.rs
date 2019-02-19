use std::io::{self, Write};
use std::str::from_utf8;

use indent_write::io::IndentableWrite;

// This is a wrapper for io::Write that only writes one byte at a time, to test
// the invariants of IndentableWrite
#[derive(Debug, Clone)]
struct OneByteAtATime<W>(W);

impl<W: Write> Write for OneByteAtATime<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(&buf[..1])
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

const CONTENT: &'static [&'static str] =
    &["\t😀 😀 😀", "\t\t😀 😀 😀", "\t😀 😀 😀"];

// Using a function to wrap a writer, run a standard test and check against expected
macro_rules! test_harness {
    ($target:ident => $transform:expr, expect: $result:expr) => {{
        let mut dest = Vec::new();

        {
            let $target = &mut dest;
            let mut indented_dest = $transform;
            for line in CONTENT {
                write!(&mut indented_dest, "{}\n", line).unwrap();
            }
        }

        let result = from_utf8(&dest).expect("Wrote invalid utf8 to dest");
        assert_eq!(result, $result);
    }};
}

#[test]
fn basic_test() {
    test_harness!(w => w.indent(), expect: "\t\t😀 😀 😀\n\t\t\t😀 😀 😀\n\t\t😀 😀 😀\n");
}

#[test]
fn test_prefix() {
    test_harness!(w => w.indent_with(b"    "), expect: "    \t😀 😀 😀\n    \t\t😀 😀 😀\n    \t😀 😀 😀\n")
}

#[test]
fn test_strip_indent() {
    test_harness!(w => w.indent_with_rules(b"\t", true, true), expect: "\t😀 😀 😀\n\t😀 😀 😀\n\t😀 😀 😀\n")
}

#[test]
fn test_multi_indent() {
    let mut dest = Vec::new();
    write!(&mut dest, "{}\n", "😀 😀 😀").unwrap();
    {
        let mut indent1 = (&mut dest).indent();
        write!(&mut indent1, "{}\n", "😀 😀 😀").unwrap();
        {
            let mut indent2 = (&mut indent1).indent();
            write!(&mut indent2, "{}\n", "😀 😀 😀").unwrap();
            {
                let mut indent3 = (&mut indent2).indent();
                write!(&mut indent3, "{}\n", "😀 😀 😀").unwrap();
            }
            write!(&mut indent2, "{}\n", "😀 😀 😀").unwrap();
        }
        write!(&mut indent1, "{}\n", "😀 😀 😀").unwrap();
    }

    let result = from_utf8(&dest).expect("Wrote invalid utf8 to dest");
    assert_eq!(
        result,
        "😀 😀 😀
\t😀 😀 😀
\t\t😀 😀 😀
\t\t\t😀 😀 😀
\t\t😀 😀 😀
\t😀 😀 😀\n"
    )
}

// Technically this doesn't test anything in the crate, it just ensures that OneByteAtATime works
#[test]
fn test_partial_writes() {
    let mut dest = Vec::new();
    {
        let mut partial_writer = OneByteAtATime(&mut dest);
        write!(partial_writer, "Hello, {}!", "World").unwrap();
    }
    assert_eq!(from_utf8(&dest), Ok("Hello, World!"));
}

#[test]
fn test_partial_simple_indent_writes() {
    let mut dest = Vec::new();
    {
        let mut writer = OneByteAtATime(&mut dest).indent();
        write!(writer, "{}\n", "Hello, World").unwrap();
        write!(writer, "{}\n", "😀 😀 😀\n😀 😀 😀").unwrap();
    }
    assert_eq!(
        from_utf8(&dest),
        Ok("\tHello, World\n\t😀 😀 😀\n\t😀 😀 😀\n")
    );
}

#[test]
fn test_partial_simple_indent_writes_inverted() {
    let mut dest = Vec::new();
    {
        let mut writer = OneByteAtATime((&mut dest).indent());
        write!(writer, "{}\n", "Hello, World").unwrap();
        write!(writer, "{}\n", "😀 😀 😀\n😀 😀 😀").unwrap();
    }
    assert_eq!(
        from_utf8(&dest),
        Ok("\tHello, World\n\t😀 😀 😀\n\t😀 😀 😀\n")
    );
}

#[test]
fn test_partial_writes_combined() {
	let mut dest = Vec::new();
	{
		let mut writer = OneByteAtATime(OneByteAtATime(&mut dest).indent_with(b"    "));
		write!(writer, "{}\n", "Hello, World").unwrap();
        write!(writer, "{}\n", "😀 😀 😀\n😀 😀 😀").unwrap();
    }
    assert_eq!(
        from_utf8(&dest),
        Ok("    Hello, World\n    😀 😀 😀\n    😀 😀 😀\n")
    );
}

// TODO: test the error cases
