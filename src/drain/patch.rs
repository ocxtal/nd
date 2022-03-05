// @file patch.rs
// @author Hajime Suzuki

use crate::common::{ConsumeSegments, FetchSegments, FillUninit, InoutFormat, Segment, StreamBuf, BLOCK_SIZE};
use crate::source::PatchStream;
use std::io::{BufRead, Read, Result, Seek, SeekFrom, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tempfile::SpooledTempFile;

struct CacheStream {
    src: Box<dyn FetchSegments>,
    cache: Arc<Mutex<SpooledTempFile>>,
    skip: usize,
}

struct CacheStreamReader {
    cache: Arc<Mutex<SpooledTempFile>>,
    buf: StreamBuf,
    offset: usize,
}

impl CacheStream {
    fn new(src: Box<dyn FetchSegments>) -> Self {
        let cache = SpooledTempFile::new(128 * BLOCK_SIZE);
        CacheStream {
            src,
            cache: Arc::new(Mutex::new(cache)),
            skip: 0,
        }
    }

    fn spawn_reader(&self) -> CacheStreamReader {
        CacheStreamReader {
            cache: Arc::clone(&self.cache),
            buf: StreamBuf::new(),
            offset: 0,
        }
    }

    fn fill_segment_buf(&mut self) -> Result<(&[u8], &[Segment])> {
        loop {
            let (block, segments) = self.src.fill_segment_buf()?;
            if block.len() <= self.skip {
                self.src.consume(0)?;
                continue;
            }

            return Ok((block, segments));
        }
    }
}

impl FetchSegments for CacheStream {
    fn fill_segment_buf(&mut self) -> Result<(&[u8], &[Segment])> {
        let (block, segments) = self.fill_segment_buf()?;

        if let Ok(mut cache) = self.cache.lock() {
            cache.write_all(&block[self.skip..]).unwrap();
        }
        self.skip = 0;

        Ok((block, segments))
    }

    fn consume(&mut self, request: usize) -> Result<usize> {
        let consumed = self.src.consume(request)?;
        self.skip = request - consumed;
        Ok(consumed)
    }
}

impl Read for CacheStreamReader {
    fn read(&mut self, _: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

impl BufRead for CacheStreamReader {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        self.buf.fill_buf(|buf| {
            if let Ok(mut cache) = self.cache.lock() {
                cache.seek(SeekFrom::Start(self.offset as u64)).unwrap();
                buf.fill_uninit(BLOCK_SIZE, |buf| cache.read(buf))?;
            }
            Ok(())
        })
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
        self.offset += amount;
    }
}

struct BashPipe {
    child: Child,
    input: Option<ChildStdin>,
}

struct BashPipeReader {
    output: ChildStdout,
    buf: StreamBuf,
}

impl BashPipe {
    fn new(command: &str) -> Self {
        let mut child = Command::new("bash")
            .args(&["-c", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        let input = child.stdin.take().unwrap();
        let input = Some(input);
        BashPipe { child, input }
    }

    fn spawn_reader(&mut self) -> BashPipeReader {
        let output = self.child.stdout.take().unwrap();
        BashPipeReader {
            output,
            buf: StreamBuf::new(),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<usize> {
        self.input.as_ref().unwrap().write_all(buf)?;
        Ok(buf.len())
    }

    fn close(&mut self) {
        self.input.take();
    }
}

impl Read for BashPipeReader {
    fn read(&mut self, _: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

impl BufRead for BashPipeReader {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        self.buf.fill_buf(|buf| {
            buf.fill_uninit(BLOCK_SIZE, |buf| self.output.read(buf))?;
            Ok(())
        })
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

pub struct PatchDrain {
    src: Box<dyn FetchSegments>,
    buf: Vec<u8>,
    pipe: BashPipe,
    drain: Option<JoinHandle<()>>,
}

impl PatchDrain {
    pub fn new(src: Box<dyn FetchSegments>, dst: Box<dyn Write + Send>, format: &InoutFormat, command: &str) -> Self {
        let format = *format;

        let src = Box::new(CacheStream::new(src));
        let cache_reader = src.spawn_reader();

        let mut pipe = BashPipe::new(command);
        let pipe_reader = pipe.spawn_reader();

        let drain = std::thread::spawn(move || {
            let mut dst = dst;
            let mut patch = PatchStream::new(Box::new(cache_reader), Box::new(pipe_reader), &format);

            loop {
                let stream = patch.fill_buf().unwrap();
                if stream.len() == 0 {
                    break;
                }
                dst.write_all(&stream).unwrap();

                let consume_len = stream.len();
                patch.consume(consume_len);
            }
        });

        PatchDrain {
            src,
            buf: Vec::new(),
            pipe,
            drain: Some(drain),
        }
    }

    fn consume_segments_impl(&mut self) -> Result<usize> {
        debug_assert!(!self.buf.is_empty());

        while self.buf.len() < BLOCK_SIZE {
            let (block, segments) = self.src.fill_segment_buf()?;
            if block.is_empty() {
                self.pipe.write_all(&self.buf).unwrap();
                self.pipe.close();

                self.buf.clear();
                return Ok(0);
            }

            for s in segments {
                self.buf.extend_from_slice(&block[s.as_range()]);
            }
            self.src.consume(block.len())?;
        }

        self.pipe.write_all(&self.buf).unwrap();
        self.buf.clear();
        Ok(1)
    }
}

impl ConsumeSegments for PatchDrain {
    fn consume_segments(&mut self) -> Result<usize> {
        loop {
            let ret = self.consume_segments_impl();
            if ret.is_err() || ret.unwrap() == 0 {
                let drain = self.drain.take().unwrap();
                drain.join().unwrap();
                return ret;
            }
        }
    }
}

// end of patch.rs
