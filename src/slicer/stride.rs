// @file stride.rs
// @author Hajime Suzuki
// @brief constant-stride slicer

use crate::common::Segment;
use crate::stream::{ByteStream, EofStream, SegmentStream};
use std::io::Result;

#[cfg(test)]
use crate::stream::tester::*;

struct ConstStrideSegments {
    segments: Vec<Segment>, // precalculated segment array
    flush_threshold: usize,
    prev_phase: usize,
    phase: usize,
    last_count: usize,
    last_len: usize,
    margin: (usize, usize),
    pitch: usize,
    len: usize,
}

impl ConstStrideSegments {
    fn new(margin: (usize, usize), pitch: usize, len: usize) -> Self {
        assert!(margin.0 < len && margin.1 < len);

        let first_segment_tail = len - margin.0;
        let segments = vec![Segment {
            pos: 0,
            len: first_segment_tail,
        }];

        let (flush_threshold, prev_phase) = if margin.0 == 0 { (0, 0) } else { (first_segment_tail, pitch) };

        ConstStrideSegments {
            segments,
            flush_threshold,
            prev_phase,
            phase: 0,
            last_count: 0,
            last_len: 0,
            margin,
            pitch,
            len,
        }
    }

    fn get_next_tail(&mut self) -> usize {
        if let Some(x) = self.segments.pop() {
            x.tail()
        } else {
            self.phase + self.len
        }
    }

    fn count_segments(&self, len: usize) -> usize {
        (len + self.pitch - 1) / self.pitch
    }

    fn calc_max_fwd(&self, count: usize) -> usize {
        eprintln!("d: {:?}, {:?}", count, self.segments.len());
        assert!(self.segments.len() >= count && count > 0);

        (self.segments[count - 1].tail() + self.pitch).saturating_sub(self.len)
    }

    fn slice_segments_with_clip(&mut self, len: usize) -> Result<(usize, usize)> {
        let mut next_tail = self.get_next_tail();
        eprintln!("b: {:?}, {:?}", len, next_tail);

        if next_tail > len + self.margin.1 {
            self.last_count = 0;
            self.last_len = 0;
            return Ok((0, 0));
        }

        while next_tail <= len + self.margin.1 {
            let pos = next_tail.saturating_sub(self.len);
            let len = std::cmp::min(next_tail, len) - pos;
            self.segments.push(Segment { pos, len });

            next_tail += self.pitch;
        }
        eprintln!("f: {:?}, {:?}", next_tail, self.segments);

        self.last_count = self.segments.len();
        self.last_len = self.calc_max_fwd(self.last_count);
        Ok((len, self.last_count))
    }

    fn slice_segments(&mut self, len: usize) -> Result<(usize, usize)> {
        eprintln!("e: {:?}", len);
        let mut next_tail = self.get_next_tail();

        if next_tail > len {
            let n_extra = self.count_segments(next_tail - len);
            if n_extra > self.segments.len() {
                self.segments.clear();
                self.prev_phase = self.pitch;

                self.last_count = 0;
                self.last_len = self.phase;
                return Ok((len, 0));
            }

            self.last_count = self.segments.len() - n_extra;
            eprintln!("a: {:?}, {:?}, {:?}", self.last_count, self.segments.len(), n_extra);
            self.last_len = self.calc_max_fwd(self.last_count);
            return Ok((len, self.last_count));
        }

        while next_tail <= len {
            self.segments.push(Segment {
                pos: next_tail - self.len,
                len: self.len,
            });
            next_tail += self.pitch;
        }

        self.last_count = self.segments.len();
        self.last_len = self.calc_max_fwd(self.last_count);
        Ok((len, self.last_count))
    }

    fn fill_segment_buf(&mut self, is_eof: bool, len: usize) -> Result<(usize, usize)> {
        eprintln!(
            "fill: {:?}, {:?}, {:?}, {:?}, {:?}",
            is_eof, len, self.flush_threshold, self.phase, self.prev_phase
        );
        if self.flush_threshold > 0 {
            // is still in the head
            if is_eof {
                // EOF found in the head, reconstruct the array
                self.segments.truncate(1);
                return self.slice_segments_with_clip(len);
            }

            // or we just need to extend it...
            return self.slice_segments(len);
        }

        if !is_eof && self.phase == self.prev_phase {
            // reuse the previous segments as the relative positions in the stream is the same
            return self.slice_segments(len);
        }

        // previous segments can't be reused due to phase shift or EOF.
        self.segments.clear();
        if is_eof {
            return self.slice_segments_with_clip(len);
        }
        self.slice_segments(len)
    }

    fn as_slice(&self) -> &[Segment] {
        &self.segments[..self.last_count]
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        eprintln!("g: {:?}, {:?}", bytes, self.last_len);

        // maximum consume length is clipped by the start pos of the next segment
        let bytes = std::cmp::min(bytes, self.last_len);

        if bytes <= self.flush_threshold {
            // still in the head or no bytes consumed
            return Ok(0);
        }

        self.flush_threshold = 0;
        self.prev_phase = self.phase;
        if bytes > 16 * self.pitch {
            // try make the next phase aligned to the previous
            let bytes = (bytes / self.pitch) * self.pitch;
            return Ok(bytes);
        }

        // #consumed bytes is too less or the pitch is too large.
        let shift = bytes % self.pitch;
        self.phase = if shift > self.phase {
            self.phase + self.pitch - shift
        } else {
            self.phase - shift
        };
        Ok(bytes)
    }
}

pub struct ConstStrideSlicer {
    src: EofStream<Box<dyn ByteStream>>,
    segments: ConstStrideSegments,
}

impl ConstStrideSlicer {
    pub fn new(src: Box<dyn ByteStream>, margin: (usize, usize), pitch: usize, len: usize) -> Self {
        assert!(pitch > 0);
        assert!(len > 0);

        ConstStrideSlicer {
            src: EofStream::new(src),
            segments: ConstStrideSegments::new(margin, pitch, len),
        }
    }
}

impl SegmentStream for ConstStrideSlicer {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        // let block_size = std::cmp::max(self.segments.len, BLOCK_SIZE);
        let (is_eof, len) = self.src.fill_buf()?;

        self.segments.fill_segment_buf(is_eof, len)
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (self.src.as_slice(), self.segments.as_slice())
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        let bytes = self.segments.consume(bytes)?;
        self.src.consume(bytes);

        Ok(bytes)
    }
}

#[cfg(test)]
macro_rules! bind {
    ( $margin: expr, $pitch: expr, $len: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            Box::new(ConstStrideSlicer::new(src, $margin, $pitch, $len))
        }
    };
}

#[test]
fn test_stride_random_len() {
    test_segment_random_len(b"", &bind!((0, 0), 1, 1), &[]);
    test_segment_random_len(b"abc", &bind!((0, 0), 1, 1), &[(0..1).into(), (1..2).into(), (2..3).into()]);
}

// end of stride.rs
