// @file stride.rs
// @author Hajime Suzuki
// @brief constant-stride slicer

use crate::common::{FetchSegments, ReadBlock, Segment, BLOCK_SIZE};

pub struct ConstStrideSlicer {
    src: Box<dyn ReadBlock>,
    buf: Vec<u8>,
    offset: usize,
    next: usize,
    eof: usize,
    width: usize,
    margin: (usize, usize),
    merge: usize,
    segments: Vec<Segment>,
}

impl ConstStrideSlicer {
    pub fn new(src: Box<dyn ReadBlock>, width: usize, margin: (isize, isize), merge: isize) -> Self {
        let mut slicer = ConstStrideSlicer {
            src,
            buf: Vec::new(),
            offset: 0,
            next: 0,
            eof: usize::MAX,
            width,
            margin,
            merge,
            segments: Vec::new(),
        };

        slicer.init_segments();
        slicer
    }

    fn extend_segments(&mut self, upto: usize) {
        let start = self.segments.len();
        for i in start..upto {
            self.segments.push(Segment {
                offset: i * self.width,
                len: self.width,
            });
        }
    }

    fn init_segments(&mut self) {
        let upto = 2 * BLOCK_SIZE / self.width;

        let overlap = self.margin.0 + self.margin.1;
        let thresh = -self.merge;
        if overlap >= thresh {}

        self.segments.push(Segment {
            offset: 0,
            len: self.width + self.margin.1,
        });
    }

    fn fill_buf(&mut self) -> Option<bool> {
        // FIXME: extend segment vector when buf.len() gets longer
        while self.buf.len() < BLOCK_SIZE {
            let len = self.src.read_block(&mut self.buf)?;
            if len == 0 {
                return Some(true);
            }
        }
        Some(false)
    }
}

impl FetchSegments for ConstStrideSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        if self.next >= self.eof {
            return Some((self.eof, &self.buf[..0], &self.segments[..0]));
        }

        let tail = self.buf.len();
        self.buf.copy_within(self.next..tail, 0);
        self.buf.truncate(tail - self.next);
        self.offset += self.next;

        let is_eof = self.fill_buf()?;
        let count = self.buf.len() / self.width;
        let rem = self.buf.len() % self.width;

        // add margin for vectorized dump
        if self.buf.capacity() < self.width {
            self.buf.reserve(self.width);
        }

        if is_eof {
            self.next = self.buf.len();
            self.eof = self.buf.len();
            self.segments[count].len = rem;

            let count = count + (rem > 0) as usize;
            return Some((self.offset, &self.buf, &self.segments[..count]));
        }

        self.next = count * self.width;
        Some((self.offset, &self.buf[..self.next], &self.segments[..count]))
    }
}

// end of stride.rs
