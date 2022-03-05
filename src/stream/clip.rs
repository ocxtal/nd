// @file clip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use std::io::{BufRead, Read, Result};

pub struct ClipStream {
    src: Box<dyn BufRead>,
    pad: isize,
    offset: isize,
    tail: isize,
}

impl ClipStream {
    pub fn new(src: Box<dyn BufRead>, pad: usize, skip: usize, len: usize) -> Self {
        assert!(skip < isize::MAX as usize);

        // FIXME: better handling of infinite stream
        let len = if len > isize::MAX as usize { isize::MAX } else { len as isize };

        ClipStream {
            src,
            pad: pad as isize,
            offset: -(skip as isize),
            tail: len as isize,
        }
    }
}

impl Read for ClipStream {
    fn read(&mut self, _: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

impl BufRead for ClipStream {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        while self.offset < self.pad {
            let stream = self.src.fill_buf()?;
            if stream.len() == 0 {
                return Ok(stream);
            }

            let consume_len = std::cmp::min(self.pad - self.offset, stream.len() as isize);
            debug_assert!(consume_len >= 0);

            self.src.consume(consume_len as usize);
            self.offset += consume_len;
        }

        let stream = self.src.fill_buf()?;
        if self.offset + stream.len() as isize >= self.tail {
            debug_assert!(self.offset <= self.tail);

            let (stream, _) = stream.split_at((self.tail - self.offset) as usize);
            return Ok(stream);
        }
        Ok(stream)
    }

    fn consume(&mut self, amount: usize) {
        self.offset += amount as isize;
        self.src.consume(amount);
    }
}

// end of clip.rs
