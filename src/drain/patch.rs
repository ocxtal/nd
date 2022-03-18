// @file patch.rs
// @author Hajime Suzuki

use crate::common::{ConsumeSegments, SegmentStream, FillUninit, InoutFormat, Segment, Stream, StreamBuf, BLOCK_SIZE};
use crate::source::PatchStream;
use std::io::{Read, Result, Seek, SeekFrom, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tempfile::SpooledTempFile;

struct CacheStream {
    src: Box<dyn SegmentStream>,
    cache: Arc<Mutex<SpooledTempFile>>,
    skip: usize,
}

struct CacheStreamReader {
    cache: Arc<Mutex<SpooledTempFile>>,
    buf: StreamBuf,
    offset: usize,
}

impl CacheStream {
    fn new(src: Box<dyn SegmentStream>) -> Self {
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
}

impl SegmentStream for CacheStream {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        let (mut len, mut count) = self.src.fill_segment_buf()?;
        let mut prev_len = 0;

        while len > prev_len {
            if self.skip + BLOCK_SIZE >= len {
                return Ok((len, count));
            }
            self.src.consume(0)?;

            let (next_len, next_count) = self.src.fill_segment_buf()?;
            (prev_len, len, count) = (len, next_len, next_count);
        }

        Ok((len, count))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        self.src.as_slices()
    }

    fn consume(&mut self, request: usize) -> Result<usize> {
        let (stream, _) = self.src.as_slices();
        debug_assert!(stream.len() >= self.skip + request);

        if let Ok(mut cache) = self.cache.lock() {
            cache.write_all(&stream[self.skip..self.skip + request]).unwrap();
        }

        let consumed = self.src.consume(request)?;
        self.skip = request - consumed;

        Ok(consumed)
    }
}

impl Stream for CacheStreamReader {
    fn fill_buf(&mut self) -> Result<usize> {
        self.buf.fill_buf(|buf| {
            if let Ok(mut cache) = self.cache.lock() {
                cache.seek(SeekFrom::Start(self.offset as u64)).unwrap();
                buf.fill_uninit(BLOCK_SIZE, |buf| cache.read(buf))?;
            }
            Ok(())
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
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

impl Stream for BashPipeReader {
    fn fill_buf(&mut self) -> Result<usize> {
        self.buf.fill_buf(|buf| {
            buf.fill_uninit(BLOCK_SIZE, |buf| self.output.read(buf))?;
            Ok(())
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

pub struct PatchDrain {
    src: Box<dyn SegmentStream>,
    buf: Vec<u8>,
    pipe: BashPipe,
    drain: Option<JoinHandle<()>>,
}

impl PatchDrain {
    pub fn new(src: Box<dyn SegmentStream>, dst: Box<dyn Write + Send>, format: &InoutFormat, command: &str) -> Self {
        let format = *format;

        let src = Box::new(CacheStream::new(src));
        let cache_reader = src.spawn_reader();

        let mut pipe = BashPipe::new(command);
        let pipe_reader = pipe.spawn_reader();

        let drain = std::thread::spawn(move || {
            let mut dst = dst;
            let mut patch = PatchStream::new(Box::new(cache_reader), Box::new(pipe_reader), &format);

            loop {
                let len = patch.fill_buf().unwrap();
                if len == 0 {
                    break;
                }
                dst.write_all(patch.as_slice()).unwrap();
                patch.consume(len);
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
            let (stream_len, _) = self.src.fill_segment_buf()?;
            if stream_len == 0 {
                self.pipe.write_all(&self.buf).unwrap();
                self.pipe.close();

                self.buf.clear();
                return Ok(0);
            }

            let (stream, segments) = self.src.as_slices();
            for s in segments {
                self.buf.extend_from_slice(&stream[s.as_range()]);
            }
            self.src.consume(stream_len)?;
        }

        self.pipe.write_all(&self.buf).unwrap();
        self.buf.clear();
        Ok(1)
    }
}

impl ConsumeSegments for PatchDrain {
    fn consume_segments(&mut self) -> Result<usize> {
        let mut core_impl = || -> Result<usize> {
            loop {
                let ret = self.consume_segments_impl();
                if ret.is_err() {
                    return ret;
                }
                if let Ok(_len @ 0) = ret {
                    return ret;
                }
            }
        };

        let ret = core_impl();
        let drain = self.drain.take().unwrap();
        drain.join().unwrap();
        ret
    }
}

// end of patch.rs
