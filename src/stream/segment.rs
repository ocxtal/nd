// @file segment.rs
// @author Hajime Suzuki
// @date 2022/3/23

use crate::common::Segment;
use std::io::Result;

#[cfg(test)]
use crate::common::MARGIN_SIZE;

#[cfg(test)]
use rand::Rng;

pub trait SegmentStream {
    // chunked iterator
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)>; // #bytes, #segments
    fn as_slices(&self) -> (&[u8], &[Segment]);
    fn consume(&mut self, bytes: usize) -> Result<usize>;
}

impl<T: SegmentStream + ?Sized> SegmentStream for Box<T> {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        (**self).fill_segment_buf()
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (**self).as_slices()
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        (**self).consume(bytes)
    }
}

pub fn test_segment_random_len<F>(pattern: &[u8], slicer: &F, expected: &[Segment])
where
    F: Fn(&[u8]) -> Box<dyn SegmentStream>,
{
    let mut rng = rand::thread_rng();

    let mut src = slicer(pattern);
    let mut expected = expected.iter();

    let mut offset = 0;
    loop {
        let (len, count) = src.fill_segment_buf().unwrap();
        eprintln!("len({:?}), count({:?}), offset({:?})", len, count, offset);
        if len == 0 {
            assert_eq!(count, 0);
            break;
        }

        let (stream, segments) = src.as_slices();
        assert!(stream.len() >= len + MARGIN_SIZE);
        assert_eq!(&stream[..len], &pattern[offset..offset + len]);

        let consume = rng.gen_range(1..=len);
        for s in segments {
            if s.tail() > consume {
                break;
            }

            let e = expected.next();
            assert!(e.is_some());
            eprintln!("s({:?}), e({:?})", s, e);

            let e = e.unwrap();
            assert_eq!(s.len, e.len);
            assert_eq!(&stream[s.as_range()], &pattern[e.as_range()]);
        }

        let consumed = src.consume(consume).unwrap();
        eprintln!("consume({:?}, {:?})", consume, consumed);
        offset += consumed;
    }

    assert_eq!(offset, pattern.len());

    let e = expected.next();
    assert!(e.is_none());
}

// end of segment.rs
