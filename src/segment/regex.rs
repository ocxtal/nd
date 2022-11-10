// @file regex.rs
// @author Hajime Suzuki
// @brief regex slicer

use super::{Segment, SegmentStream};
use anyhow::Result;
use regex::bytes::{Match, Regex};

pub struct RegexSlicer {
    src: Box<dyn SegmentStream>,
    matches: Vec<Segment>,
    scanned: usize,
    re: Regex,
}

impl RegexSlicer {
    pub fn new(src: Box<dyn SegmentStream>, pattern: &str) -> Self {
        RegexSlicer {
            src,
            matches: Vec::new(),
            scanned: 0,
            re: Regex::new(pattern).unwrap(),
        }
    }
}

impl SegmentStream for RegexSlicer {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let to_segment = |m: Match, pos: usize| -> Segment {
            Segment {
                pos: pos + m.start(),
                len: m.range().len(),
            }
        };

        let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;

        let (stream, segments) = self.src.as_slices();
        for s in &segments[..count] {
            if s.pos < self.scanned {
                continue;
            }

            self.matches
                .extend(self.re.find_iter(&stream[s.as_range()]).map(|x| to_segment(x, s.pos)));
        }

        self.scanned += bytes;

        Ok((is_eof, bytes, self.matches.len(), max_consume))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.matches)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let (bytes, _) = self.src.consume(bytes)?;
        if bytes == 0 {
            return Ok((0, 0));
        }

        // determine how many bytes to consume...
        let from = self.matches.partition_point(|x| x.pos < bytes);
        let to = self.matches.len();

        self.matches.copy_within(from..to, 0);
        self.matches.truncate(to - from);

        for m in &mut self.matches {
            m.pos -= bytes;
        }

        self.scanned -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
mod tests {
    use super::RegexSlicer;
    use crate::segment::tester::*;

    macro_rules! bind {
        ( $pattern: expr ) => {
            |input: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(input));
                let src = Box::new(ConstSlicer::from_raw(src, (0, -3), (false, false), 4, 6));
                Box::new(RegexSlicer::new(src, $pattern))
            }
        };
    }

    macro_rules! test {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                $inner(b"aaaaaaaaaa", &bind!("a+"), &[(0..6).into(), (4..10).into()]);
                $inner(b"abcabcabca", &bind!("a.+c"), &[(0..6).into(), (6..9).into()]);
                $inner(b"abcabcabca", &bind!("abc"), &[(0..3).into(), (3..6).into(), (6..9).into()]);
                $inner(b"abcabcabca", &bind!("abcabc"), &[(0..6).into()]);
                $inner(b"abcdefabcd", &bind!("abc"), &[(0..3).into(), (6..9).into()]);
                $inner(b"abcdefabcd", &bind!("abcd"), &[(0..4).into(), (6..10).into()]);
                $inner(b"abcdefabcd", &bind!("abcde"), &[(0..5).into()]);

                $inner(b"abcdefabcd", &bind!("^abc"), &[(0..3).into()]);
                $inner(b"abcdabcdab", &bind!("^abc"), &[(0..3).into(), (4..7).into()]);
                $inner(b"cabcabcabc", &bind!("^abc"), &[(4..7).into()]);

                // TODO: we need a lot more...
            }
        };
    }

    test!(test_regex_all_at_once, test_segment_all_at_once);
    test!(test_regex_random_len, test_segment_random_len);
    test!(test_regex_occasional_consume, test_segment_occasional_consume);
}

// end of regex.rs
