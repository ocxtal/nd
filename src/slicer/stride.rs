// @file stride.rs
// @author Hajime Suzuki
// @brief constant-stride slicer

use crate::common::{EofReader, FetchSegments, Segment, StreamBuf, BLOCK_SIZE};
use std::io::{BufRead, Result};

struct ConstStrideSegments {
    segments: Vec<Segment>, // precalculated segment array
    flush_threshold: usize,
    prev_phase: usize,
    phase: usize,
    max_consume: usize,
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
            max_consume: 0,
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

    fn calc_max_fwd(&self, n_active: usize) -> usize {
        assert!(self.segments.len() >= n_active && n_active > 0);

        (self.segments[n_active - 1].tail() + self.pitch).saturating_sub(self.len)
    }

    fn slice_segments_with_clip<'a, 'b>(&'a mut self, stream: &'b [u8]) -> Result<(&'b [u8], &'a [Segment])> {
        let mut next_tail = self.get_next_tail();
        while next_tail < stream.len() + self.margin.1 {
            let pos = next_tail.saturating_sub(self.len);
            let len = std::cmp::min(next_tail, stream.len()) - pos;
            self.segments.push(Segment { pos, len });

            next_tail += self.pitch;
        }

        self.max_consume = self.calc_max_fwd(stream.len());
        Ok((stream, &self.segments))
    }

    fn slice_segments<'a, 'b>(&'a mut self, stream: &'b [u8]) -> Result<(&'b [u8], &'a [Segment])> {
        let mut next_tail = self.get_next_tail();

        if next_tail >= stream.len() {
            let n_extra = self.count_segments(next_tail - stream.len());
            if n_extra >= self.segments.len() {
                self.segments.clear();
                self.prev_phase = self.pitch;
                self.max_consume = self.phase;
                return Ok((stream, &self.segments));
            }

            let n_active = self.segments.len() - n_extra;
            self.max_consume = self.calc_max_fwd(n_active);
            return Ok((stream, &self.segments[..n_active]));
        }

        while next_tail < stream.len() {
            self.segments.push(Segment {
                pos: next_tail - self.len,
                len: self.len,
            });
            next_tail += self.pitch;
        }
        self.max_consume = self.calc_max_fwd(self.segments.len());
        Ok((stream, &self.segments))
    }

    fn fill_segment_buf<'a, 'b>(&'a mut self, is_eof: bool, stream: &'b [u8]) -> Result<(&'b [u8], &'a [Segment])> {
        if self.flush_threshold > 0 {
            // is still in the head
            if is_eof {
                // EOF found in the head, reconstruct the array
                self.segments.truncate(1);
                return self.slice_segments_with_clip(stream);
            }

            // or we just need to extend it...
            return self.slice_segments(stream);
        }

        if !is_eof && self.phase == self.prev_phase {
            // reuse the previous segments as the relative positions in the stream is the same
            return self.slice_segments(stream);
        }

        // previous segments can't be reused due to phase shift or EOF.
        self.segments.clear();
        if is_eof {
            return self.slice_segments_with_clip(stream);
        }
        self.slice_segments(stream)
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        // maximum consume length is clipped by the start pos of the next segment
        let bytes = std::cmp::min(bytes, self.max_consume);

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
    src: EofReader<Box<dyn BufRead>>,
    buf: StreamBuf,
    segments: ConstStrideSegments,
}

impl ConstStrideSlicer {
    pub fn new(src: Box<dyn BufRead>, margin: (usize, usize), pitch: usize, len: usize) -> Self {
        ConstStrideSlicer {
            src: EofReader::new(src),
            buf: StreamBuf::new(),
            segments: ConstStrideSegments::new(margin, pitch, len),
        }
    }
}

impl FetchSegments for ConstStrideSlicer {
    fn fill_segment_buf(&mut self) -> Result<(&[u8], &[Segment])> {
        let block_size = std::cmp::max(self.segments.len, BLOCK_SIZE);
        let (is_eof, stream) = self.src.fill_buf(block_size)?;

        self.segments.fill_segment_buf(is_eof, stream)
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        let bytes = self.segments.consume(bytes)?;
        self.src.consume(bytes);

        Ok(bytes)
    }
}

// end of stride.rs
