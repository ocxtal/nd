// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod hamming;
mod merger;
mod regex;
mod stride;
mod strip;

pub use self::hamming::HammingSlicer;
pub use self::merger::SliceMerger;
pub use self::regex::RegexSlicer;
pub use self::stride::ConstSlicer;
pub use self::strip::SliceStripper;

use std::io::Result;
use std::ops::Range;

#[cfg(test)]
use crate::params::MARGIN_SIZE;

#[cfg(test)]
use rand::Rng;

#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct Segment {
    pub pos: usize,
    pub len: usize,
}

impl Segment {
    pub fn tail(&self) -> usize {
        self.pos + self.len
    }

    pub fn as_range(&self) -> Range<usize> {
        self.pos..self.tail()
    }

    pub fn unwind(&self, adj: usize) -> Self {
        debug_assert!(adj >= self.pos);
        Segment {
            pos: self.pos - adj,
            len: self.len,
        }
    }
}

impl From<Range<usize>> for Segment {
    fn from(other: Range<usize>) -> Self {
        Segment {
            pos: other.start,
            len: other.len(),
        }
    }
}

pub trait SegmentStream {
    // chunked iterator
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)>; // #bytes, #segments
    fn as_slices(&self) -> (&[u8], &[Segment]);
    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)>; // #bytes, #segments
}

impl<T: SegmentStream + ?Sized> SegmentStream for Box<T> {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        (**self).fill_segment_buf()
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (**self).as_slices()
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        (**self).consume(bytes)
    }
}

#[cfg(test)]
pub fn test_segment_random_len<F>(pattern: &[u8], slicer: &F, expected: &[Segment])
where
    F: Fn(&[u8]) -> Box<dyn SegmentStream>,
{
    let mut rng = rand::thread_rng();
    let mut src = slicer(pattern);
    let mut len_acc = 0;
    let mut count_acc = 0;
    loop {
        let (len, count) = src.fill_segment_buf().unwrap();
        if len == 0 {
            assert_eq!(count, 0);
            break;
        }

        let (stream, segments) = src.as_slices();
        assert!(stream.len() >= len + MARGIN_SIZE);
        assert_eq!(&stream[..len], &pattern[len_acc..len_acc + len]);

        for (s, e) in segments.iter().zip(&expected[count_acc..]) {
            assert_eq!(s.len, e.len);
            assert_eq!(&stream[s.as_range()], &pattern[e.as_range()]);
        }

        let bytes_to_consume = rng.gen_range(1..=len);
        let (len_fwd, count_fwd) = src.consume(bytes_to_consume).unwrap();

        len_acc += len_fwd;
        count_acc += count_fwd;
    }

    assert_eq!(len_acc, pattern.len());
    assert_eq!(count_acc, expected.len());
}

#[cfg(test)]
pub fn test_segment_occasional_consume<F>(pattern: &[u8], slicer: &F, expected: &[Segment])
where
    F: Fn(&[u8]) -> Box<dyn SegmentStream>,
{
    let mut rng = rand::thread_rng();
    let mut src = slicer(pattern);
    let mut prev_len = 0;
    let mut len_acc = 0;
    let mut count_acc = 0;
    loop {
        let (len, count) = src.fill_segment_buf().unwrap();
        if len == 0 {
            assert_eq!(count, 0);
            break;
        }

        let (stream, segments) = src.as_slices();
        assert!(stream.len() >= len + MARGIN_SIZE);
        assert_eq!(&stream[..len], &pattern[len_acc..len_acc + len]);

        if rng.gen::<bool>() {
            continue;
        }

        for (s, e) in segments.iter().zip(&expected[count_acc..]) {
            assert_eq!(s.len, e.len);
            assert_eq!(&stream[s.as_range()], &pattern[e.as_range()]);
        }

        let consume = if len == prev_len { len } else { (len + 1) / 2 };
        let (len_fwd, count_fwd) = src.consume(consume).unwrap();

        prev_len = len;
        len_acc += len_fwd;
        count_acc += count_fwd;
    }

    assert_eq!(len_acc, pattern.len());
    assert_eq!(count_acc, expected.len());
}

#[cfg(test)]
pub fn test_segment_all_at_once<F>(pattern: &[u8], slicer: &F, expected: &[Segment])
where
    F: Fn(&[u8]) -> Box<dyn SegmentStream>,
{
    let mut src = slicer(pattern);
    let mut prev_len = 0;
    loop {
        let (len, _) = src.fill_segment_buf().unwrap();
        if len == prev_len {
            break;
        }

        let (len_fwd, _) = src.consume(0).unwrap();
        assert_eq!(len_fwd, 0);

        prev_len = len;
    }

    let (len, count) = src.fill_segment_buf().unwrap();
    assert_eq!(len, pattern.len());
    assert_eq!(count, expected.len());

    let (stream, segments) = src.as_slices();
    assert_eq!(&stream[..len], pattern);
    assert_eq!(&segments[..count], expected);

    let (len_fwd, count_fwd) = src.consume(len).unwrap();
    assert_eq!(len_fwd, len);
    assert_eq!(count_fwd, count);

    let (len, count) = src.fill_segment_buf().unwrap();
    assert_eq!(len, 0);
    assert_eq!(count, 0);
}

#[cfg(test)]
pub mod tester {
    #[allow(unused_imports)]
    pub use super::{test_segment_all_at_once, test_segment_occasional_consume, test_segment_random_len, SegmentStream};
}

// end of mod.rs