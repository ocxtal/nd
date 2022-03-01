// @file stride.rs
// @author Hajime Suzuki
// @brief constant-stride slicer

use crate::common::{FetchSegments, ReadBlock, Segment, BLOCK_SIZE, CHUNK_SIZE};

struct ConstStrideSegments {
    segments: Vec<Segment>, // precalculated segment array

    // states
    n_clipped: Option<usize>,
    consumed: usize,
    in_use: usize,

    // parameters
    margin: (usize, usize),
    pitch: usize,
    len: usize,
}

impl ConstStrideSegments {
    fn new(margin: (usize, usize), pitch: usize, len: usize) -> Self {
        assert!(margin.0 < len && margin.1 < len);

        let mut segments = Self::build_array(margin.0, 0, pitch, len);
        let n_clipped = if segments.is_empty() { None } else { Some(segments.len()) };

        Self::extend_array(&mut segments, 2 * BLOCK_SIZE, pitch, len);

        ConstStrideSegments {
            segments,
            n_clipped,
            consumed: 0,
            in_use: 0,
            margin,
            pitch,
            len,
        }
    }

    fn min_fill_bytes(&self) -> usize {
        self.pitch.max(2 * self.len)
    }

    fn build_array(margin: usize, upto: usize, pitch: usize, len: usize) -> Vec<Segment> {
        let mut segments = Vec::new();

        let upto = upto as isize;
        let mut offset = -(margin as isize);

        while offset < upto {
            segments.push(Segment {
                offset: offset.max(0) as usize,
                len,
            });
            offset += pitch as isize;
        }
        segments
    }

    fn extend_array(segments: &mut Vec<Segment>, upto: usize, pitch: usize, len: usize) {
        debug_assert!(!segments.is_empty());

        let mut offset = if let Some(segment) = segments.last() {
            segment.offset + pitch
        } else {
            0
        };
        while offset < upto {
            segments.push(Segment { offset, len });
            offset += pitch;
        }
    }

    fn forward(&mut self, count: usize) -> Option<usize> {
        assert!(self.consumed + count <= self.segments.len());

        // clear the current lending state
        self.in_use = 0;

        if count == 0 {
            // nothing changes
            return Some(0);
        }
        self.consumed += count;
        assert!(self.consumed > 0 && self.consumed <= self.segments.len());

        if let Some(n_clipped) = self.n_clipped {
            if self.consumed < n_clipped {
                // keep the all elements if it's still in the head margin
                return Some(0);
            }

            // it gets out of the head margin
            let bytes = self.segments[self.consumed - 1].tail();

            // clear and reconstruct the segment array
            self.n_clipped = None;
            self.consumed = 0;
            self.segments.clear();
            Self::extend_array(&mut self.segments, 2 * BLOCK_SIZE, self.pitch, self.len);

            return Some(bytes);
        }

        Some(self.segments[self.consumed - 1].tail())
    }

    fn patch_segments(&mut self, bytes: usize, count: usize) -> usize {
        let clip = self.segments[self.consumed].offset as usize + bytes;
        let mut count = count;
        let mut array = &mut self.segments[self.consumed..];
        assert!(array.len() >= count);

        while count > 0 && array[count - 1].tail() <= clip + self.margin.1 {
            count -= 1;
        }

        while count > 0 && array[count - 1].tail() <= clip {
            debug_assert!(array[count - 1].offset < clip);

            array[count - 1].len = clip - array[count - 1].offset;
            count -= 1;
        }
        count
    }

    fn slice(&mut self, is_eof: bool, bytes: usize) -> Option<(&[Segment], usize)> {
        assert!(self.in_use == 0);

        if bytes < self.len {
            return Some((&self.segments[..0], 0));
        }

        // extend precalculated segment array if it's not enough
        let count = (bytes - self.len) / self.pitch + 1;
        if self.segments.len() < self.consumed + count {
            let upto = (self.consumed + count + 1).next_power_of_two();
            Self::extend_array(&mut self.segments, upto, self.pitch, self.len);
        }
        assert!(self.consumed + count <= self.segments.len());

        // patch the tail margin if eof
        self.in_use = if is_eof { self.patch_segments(bytes, count) } else { count };

        if self.in_use == 0 {
            return Some((&self.segments[..0], 0));
        }

        let segments = &self.segments[self.consumed..self.consumed + self.in_use];
        let bytes = self.pitch * (self.in_use - 1) + self.len;
        Some((segments, bytes))
    }
}

pub struct ConstStrideSlicer {
    src: Box<dyn ReadBlock>,
    buf: Vec<u8>,
    offset: usize,
    consumed: usize,
    eof: usize,
    segments: ConstStrideSegments,
}

impl ConstStrideSlicer {
    pub fn new(src: Box<dyn ReadBlock>, margin: (usize, usize), pitch: usize, len: usize) -> Self {
        ConstStrideSlicer {
            src,
            buf: Vec::new(),
            offset: 0, // global offset of the stream
            consumed: 0,
            eof: usize::MAX, // #elements available in buf
            segments: ConstStrideSegments::new(margin, pitch, len),
        }
    }

    fn fill_buf(&mut self) -> Option<(bool, usize)> {
        let base_len = self.buf.len();

        // buf.len() > BLOCK_SIZE indicates the consumer needs more bytes to transition its state
        let upto = base_len + self.segments.min_fill_bytes();
        let upto = (upto + 1).next_power_of_two().max(BLOCK_SIZE);

        let mut read_block = || -> Option<bool> {
            while self.buf.len() < upto {
                let len = self.src.read_block(&mut self.buf)?;
                if len == 0 {
                    return Some(true);
                }
            }
            Some(false)
        };

        let is_eof = read_block()?;
        if is_eof {
            self.eof = self.buf.len();
        }

        self.buf.reserve(64); // add margin for vectorized dump (FIXME: magic number)
        Some((is_eof, self.buf.len() - base_len))
    }
}

impl FetchSegments for ConstStrideSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        eprintln!("fetch: {:?}, {:?}, {:?}", self.consumed, self.eof, self.offset);
        if self.consumed >= self.eof {
            let (segments, _) = self.segments.slice(true, 0)?;
            debug_assert!(segments.is_empty());

            let buf = &self.buf[..0];
            return Some((self.eof, buf, segments));
        }

        let (is_eof, loaded) = self.fill_buf()?;
        eprintln!("{:?}, {:?}", is_eof, loaded);
        let (segments, sliced) = self.segments.slice(is_eof, loaded)?;
        eprintln!("{:?}, {:?}", segments.len(), sliced);

        let buf = &self.buf[self.consumed..self.consumed + sliced];
        Some((self.offset, buf, segments))
    }

    fn forward_segments(&mut self, count: usize) -> Option<()> {
        let len = self.segments.forward(count)?;
        self.offset += len;
        self.consumed += len;

        let tail = self.buf.len();
        if self.consumed + CHUNK_SIZE < tail {
            return Some(());
        }

        self.buf.copy_within(self.consumed..tail, 0);
        self.consumed = 0;
        Some(())
    }
}

// end of stride.rs
