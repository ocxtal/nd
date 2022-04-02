// @file segment.rs
// @author Hajime Suzuki
// @date 2022/3/23

use crate::common::Segment;
use std::io::Result;

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

// #[allow(unused_macros)]
// macro_rules! test_segment_random_len {
//     ( $src: expr, $pattern: expr, $segments: expr ) => {{
//         let mut rng = rand::thread_rng();
//         let mut src = $src;
//         let mut expected = $segments.iter();
//         let mut offset = 0;
//         loop {
//             let (len, count) = src.fill_segment_buf().unwrap();
//             if len == 0 {
//                 assert_eq!(count.len(), 0);
//                 break;
//             }

//             let (stream, segments) = src.as_slices();
//             assert!(stream.len() >= len + MARGIN_SIZE);

//             let consume = rng.gen_range(1..len);
//             for s in &segments {
//                 if s.tail() > offset + consume {
//                     break;
//                 }

//                 let e = expected.next();
//                 assert!(e.is_some());

//                 let e = e.unwrap();
//                 assert_eq!(s.len, e.len());
//                 assert_eq!(s, e);
//             }

//             let consumed = src.consume(consume).unwrap();
//             offset += consumed;
//         }

//         assert_eq!(offset, $len);

//         let e = expected.next();
//         assert!(e.is_none());
//     }};
// }

// end of segment.rs
