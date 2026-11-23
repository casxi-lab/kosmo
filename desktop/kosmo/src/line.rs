//! Reading whole lines with a byte limit.

use std::io::{self, BufRead, ErrorKind};

/// The most bytes of a line that the terminal reads: its bytes before the
/// line feed, a carriage return at its end included. It equals the most
/// items of the current line, so an expression that fits a line never has
/// more items than the current line holds.
pub const MAX_LINE_BYTES: usize = 4096;

/// What reading a line gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    /// A line, now in the buffer, without its line feed and without one
    /// carriage return at its end.
    Line,
    /// A line of more than [`MAX_LINE_BYTES`] bytes. Its bytes are
    /// discarded up to its line feed, and the buffer is empty.
    TooLong,
    /// The input has ended.
    End,
}

/// Reads the next line of `input` into `line`, which it empties first. The
/// last line of the input may lack its line feed.
///
/// The buffer never holds more than [`MAX_LINE_BYTES`] bytes, and it grows
/// to at most that many: a longer line is read on to its line feed in the
/// pieces that `input` holds and discarded, without being kept. An
/// interrupted read is retried; any other error of `input` is returned, and
/// the bytes of the line read so far are lost.
pub(crate) fn read_line(input: &mut dyn BufRead, line: &mut Vec<u8>) -> io::Result<Read> {
    line.clear();
    line.reserve_exact(MAX_LINE_BYTES);
    let mut read_any = false;
    let mut too_long = false;
    loop {
        let available = match input.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            break;
        }
        read_any = true;
        let (part, used, ended) = match available.iter().position(|&byte| byte == b'\n') {
            Some(at) => (&available[..at], at + 1, true),
            None => (available, available.len(), false),
        };
        if !too_long {
            if part.len() <= MAX_LINE_BYTES - line.len() {
                line.extend_from_slice(part);
            } else {
                too_long = true;
                line.clear();
            }
        }
        input.consume(used);
        if ended {
            break;
        }
    }
    if !read_any {
        return Ok(Read::End);
    }
    if too_long {
        return Ok(Read::TooLong);
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(Read::Line)
}

#[cfg(test)]
mod tests {
    use std::io::BufReader;

    use super::*;

    /// Every read of `input`, with a reader that holds `capacity` bytes.
    fn reads(input: &[u8], capacity: usize) -> Vec<(Read, Vec<u8>)> {
        let mut reader = BufReader::with_capacity(capacity, input);
        let mut line = Vec::new();
        let mut reads = Vec::new();
        loop {
            let read = read_line(&mut reader, &mut line).unwrap();
            reads.push((read, line.clone()));
            if read == Read::End {
                return reads;
            }
        }
    }

    fn line(bytes: &[u8]) -> (Read, Vec<u8>) {
        (Read::Line, bytes.to_vec())
    }

    const END: (Read, Vec<u8>) = (Read::End, Vec::new());
    const TOO_LONG: (Read, Vec<u8>) = (Read::TooLong, Vec::new());

    #[test]
    fn lines_end_at_line_feeds_and_the_last_may_lack_one() {
        for capacity in [1, 3, 64] {
            assert_eq!(
                reads(b"1/3\n\nx^2", capacity),
                [line(b"1/3"), line(b""), line(b"x^2"), END]
            );
            assert_eq!(reads(b"", capacity), [END]);
            assert_eq!(reads(b"\n", capacity), [line(b""), END]);
        }
    }

    #[test]
    fn one_carriage_return_at_the_end_of_a_line_is_dropped() {
        assert_eq!(
            reads(b"1\r\n2\r\r\n3\r", 64),
            [line(b"1"), line(b"2\r"), line(b"3"), END]
        );
        assert_eq!(reads(b"a\rb\n", 64), [line(b"a\rb"), END]);
    }

    #[test]
    fn a_line_of_the_limit_is_read_and_one_byte_more_is_too_long() {
        let limit = vec![b' '; MAX_LINE_BYTES];
        let mut input = limit.clone();
        input.push(b'\n');
        assert_eq!(reads(&input, 64), [line(&limit), END]);

        // The carriage return counts toward the limit.
        let mut input = limit.clone();
        input.extend_from_slice(b"\r\n");
        assert_eq!(reads(&input, 64), [TOO_LONG, END]);
        let mut input = vec![b'1'; MAX_LINE_BYTES + 1];
        input.extend_from_slice(b"\n1/2\n");
        assert_eq!(reads(&input, 64), [TOO_LONG, line(b"1/2"), END]);
    }

    #[test]
    fn a_line_without_a_line_feed_beyond_the_limit_never_grows_the_buffer_past_it() {
        let input = vec![b'1'; 2 * MAX_LINE_BYTES];
        for capacity in [1, 100, 3000, 8 * 1024] {
            let mut reader = BufReader::with_capacity(capacity, &input[..]);
            let mut buffer = Vec::new();
            assert_eq!(read_line(&mut reader, &mut buffer).unwrap(), Read::TooLong);
            assert!(buffer.capacity() <= MAX_LINE_BYTES, "{}", buffer.capacity());
            assert!(buffer.is_empty());
            assert_eq!(read_line(&mut reader, &mut buffer).unwrap(), Read::End);
        }
    }

    /// A reader that is interrupted once and then fails, after its bytes.
    struct Unsteady {
        bytes: &'static [u8],
        interrupted: bool,
    }

    impl io::Read for Unsteady {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::Error::from(ErrorKind::Interrupted));
            }
            if self.bytes.is_empty() {
                return Err(io::Error::other("the input failed"));
            }
            let n = self.bytes.len().min(buf.len());
            buf[..n].copy_from_slice(&self.bytes[..n]);
            self.bytes = &self.bytes[n..];
            Ok(n)
        }
    }

    #[test]
    fn an_interrupted_read_is_retried_and_another_error_is_returned() {
        let mut reader = BufReader::new(Unsteady {
            bytes: b"1/2\n3",
            interrupted: false,
        });
        let mut buffer = Vec::new();
        assert_eq!(read_line(&mut reader, &mut buffer).unwrap(), Read::Line);
        assert_eq!(buffer, b"1/2");
        let error = read_line(&mut reader, &mut buffer).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::Other);
    }
}
