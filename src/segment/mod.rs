// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod bridge;
mod exact;
mod extend;
mod guided;
mod merge;
mod regex;
mod stride;
// mod strip;
mod walk;

pub use self::bridge::BridgeStream;
pub use self::exact::ExactMatchSlicer;
pub use self::extend::ExtendStream;
pub use self::guided::GuidedSlicer;
pub use self::merge::MergeStream;
pub use self::regex::RegexSlicer;
pub use self::stride::{ConstSlicer, ConstSlicerParams};
// pub use self::strip::StripStream;
pub use self::walk::WalkSlicer;

use anyhow::Result;
use std::ops::Range;

#[cfg(test)]
use crate::params::MARGIN_SIZE;

#[cfg(test)]
use rand::Rng;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
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
}

impl From<Range<usize>> for Segment {
    fn from(other: Range<usize>) -> Self {
        Segment {
            pos: other.start,
            len: other.len(),
        }
    }
}

pub trait SegmentStream: Send {
    // chunked iterator

    // returns (is_eof, #bytes, #segments, first segment offset in the next chunk)
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)>;

    // (byte stream, segment stream)
    fn as_slices(&self) -> (&[u8], &[Segment]);

    // (#bytes, #segments)
    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)>;
}

impl<T: SegmentStream + ?Sized> SegmentStream for Box<T> {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
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
    let mut prev_is_eof = false;
    let mut prev_len = 0;
    let mut len_acc = 0;
    let mut count_acc = 0;
    let mut last_spos = -1;
    loop {
        let (is_eof, len, count, max_consume) = src.fill_segment_buf().unwrap();
        if is_eof && len == 0 {
            assert_eq!(count, 0);
            assert_eq!(max_consume, 0);
            break;
        }

        if prev_is_eof {
            assert!(is_eof);
            assert_eq!(prev_len, len);
        }

        let (stream, segments) = src.as_slices();
        assert!(stream.len() >= len + MARGIN_SIZE);
        assert_eq!(&stream[..len], &pattern[len_acc..len_acc + len]);

        // pos must be strong-monotonic increasing between different fill_segment_buf units
        if segments.len() > 0 {
            assert!(segments[0].pos as isize > last_spos);
        }

        // pos must be weak-monotonic increasing in one fill_segment_buf unit
        assert!(segments.windows(2).all(|x| x[0].pos <= x[1].pos));

        let mut spos = Vec::new();
        for (s, e) in segments.iter().zip(&expected[count_acc..]) {
            assert_eq!(s.len, e.len);
            assert_eq!(&stream[s.as_range()], &pattern[e.as_range()]);
            spos.push(s.pos as isize);
        }

        let bytes_to_consume = rng.gen_range(1..=std::cmp::max(1, max_consume));
        let (len_fwd, count_fwd) = src.consume(bytes_to_consume).unwrap();

        prev_is_eof = is_eof;
        prev_len = len - len_fwd;

        len_acc += len_fwd;
        count_acc += count_fwd;
        if count_fwd > 0 {
            last_spos = spos[count_fwd - 1] - len_fwd as isize;
        }
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
    let mut prev_is_eof = false;
    let mut prev_len = 0;
    let mut len_acc = 0;
    let mut count_acc = 0;
    let mut last_spos = -1;
    loop {
        let (is_eof, len, count, max_consume) = src.fill_segment_buf().unwrap();
        if is_eof && len == 0 {
            assert_eq!(count, 0);
            assert_eq!(max_consume, 0);
            break;
        }

        if prev_is_eof {
            assert!(is_eof);
            assert_eq!(prev_len, len);
        }

        let (stream, segments) = src.as_slices();
        assert!(stream.len() >= len + MARGIN_SIZE);
        assert_eq!(&stream[..len], &pattern[len_acc..len_acc + len]);

        // pos must be strong-monotonic increasing between different fill_segment_buf units
        if segments.len() > 0 {
            assert!(segments[0].pos as isize > last_spos);
        }

        // pos must be weak-monotonic increasing in one fill_segment_buf unit
        assert!(segments.windows(2).all(|x| x[0].pos <= x[1].pos));

        if rng.gen::<bool>() {
            continue;
        }

        let mut spos = Vec::new();
        for (s, e) in segments.iter().zip(&expected[count_acc..]) {
            assert_eq!(s.len, e.len);
            assert_eq!(&stream[s.as_range()], &pattern[e.as_range()]);
            spos.push(s.pos as isize);
        }

        let consume = if is_eof { max_consume } else { (max_consume + 1) / 2 };
        let (len_fwd, count_fwd) = src.consume(consume).unwrap();

        prev_is_eof = is_eof;
        prev_len = len - len_fwd;

        len_acc += len_fwd;
        count_acc += count_fwd;
        if count_fwd > 0 {
            last_spos = spos[count_fwd - 1] - len_fwd as isize;
        }
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
        let (is_eof, len, _, _) = src.fill_segment_buf().unwrap();
        if is_eof && len == prev_len {
            break;
        }

        let (len_fwd, _) = src.consume(0).unwrap();
        assert_eq!(len_fwd, 0);

        prev_len = len;
    }

    let (is_eof, len, count, max_consume) = src.fill_segment_buf().unwrap();
    assert!(is_eof);
    assert_eq!(len, pattern.len());
    assert_eq!(count, expected.len());
    assert_eq!(len, max_consume);

    let (stream, segments) = src.as_slices();
    assert_eq!(&stream[..len], pattern);
    assert_eq!(&segments[..count], expected);

    let (len_fwd, count_fwd) = src.consume(len).unwrap();
    assert_eq!(len_fwd, len);
    assert_eq!(count_fwd, count);

    let (is_eof, len, count, max_consume) = src.fill_segment_buf().unwrap();
    assert!(is_eof);
    assert_eq!(len, 0);
    assert_eq!(count, 0);
    assert_eq!(max_consume, 0);
}

#[cfg(test)]
pub mod tester {
    #[allow(unused_imports)]
    pub use super::{test_segment_all_at_once, test_segment_occasional_consume, test_segment_random_len, SegmentStream};
}

// end of mod.rs
