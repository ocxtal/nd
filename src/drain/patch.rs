// @file patch.rs
// @author Hajime Suzuki

use crate::common::{ConsumeSegments, FetchSegments, InoutFormat, ReadBlock, ReserveAndFill, Segment, BLOCK_SIZE};
use crate::source::PatchStream;
use std::io::{Read, Result, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tempfile::SpooledTempFile;

struct CacheStream {
    src: Box<dyn FetchSegments>,
    cache: Arc<Mutex<SpooledTempFile>>,
    offset: usize,
}

struct CacheStreamReader {
    cache: Arc<Mutex<SpooledTempFile>>,
}

impl CacheStream {
    fn new(src: Box<dyn FetchSegments>) -> Self {
        let cache = SpooledTempFile::new(128 * BLOCK_SIZE);
        CacheStream {
            src,
            cache: Arc::new(Mutex::new(cache)),
            offset: 0,
        }
    }

    fn spawn_reader(&self) -> CacheStreamReader {
        CacheStreamReader {
            cache: Arc::clone(&self.cache),
        }
    }
}

impl FetchSegments for CacheStream {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        let (offset, block, segments) = self.src.fetch_segments()?;
        assert!(self.offset >= offset);

        let skip = self.offset - offset;
        if let Ok(mut cache) = self.cache.lock() {
            cache.write_all(&block[skip..]).ok()?;
        }
        self.offset = offset + block.len();

        Some((offset, block, segments))
    }

    fn forward_segments(&mut self, count: usize) -> Option<()> {
        self.src.forward_segments(count)
    }
}

impl ReadBlock for CacheStreamReader {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let base_len = buf.len();
        while buf.len() < BLOCK_SIZE {
            let len = buf.reserve_and_fill(BLOCK_SIZE, |x: &mut [u8]| {
                let len = if let Ok(mut cache) = self.cache.lock() {
                    cache.read(x).ok()?
                } else {
                    0
                };
                Some((len, len))
            })?;

            if len == 0 {
                return Some(buf.len() - base_len);
            }
        }

        Some(buf.len() - base_len)
    }
}

struct BashPipe {
    child: Arc<Mutex<Child>>,
}

struct BashPipeReader {
    child: Arc<Mutex<Child>>,
}

impl BashPipe {
    fn new(command: &str) -> BashPipe {
        let child = Command::new("bash")
            .args(&["-c", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        BashPipe {
            child: Arc::new(Mutex::new(child)),
        }
    }

    fn spawn_reader(&self) -> BashPipeReader {
        BashPipeReader {
            child: Arc::clone(&self.child),
        }
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<usize> {
        let mut child = self.child.lock().unwrap();
        child.stdin.take().unwrap().write_all(buf)?;
        Ok(buf.len())
    }
}

impl Read for BashPipeReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut child = self.child.lock().unwrap();
        child.stdout.take().unwrap().read(buf)
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

        let pipe = BashPipe::new(command);
        let pipe_reader = pipe.spawn_reader();

        let drain = std::thread::spawn(move || {
            let mut dst = dst;
            let mut stream = PatchStream::new(Box::new(cache_reader), Box::new(pipe_reader), &format);

            let mut buf = Vec::with_capacity(2 * BLOCK_SIZE);
            while let Some(len) = stream.read_block(&mut buf) {
                if len == 0 {
                    return;
                }

                dst.write_all(&buf).unwrap();
                buf.clear();
            }
        });

        PatchDrain {
            src,
            buf: Vec::new(),
            pipe,
            drain: Some(drain),
        }
    }

    fn consume_segments_impl(&mut self) -> Option<usize> {
        debug_assert!(!self.buf.is_empty());

        while self.buf.len() < BLOCK_SIZE {
            let (_, block, segments) = self.src.fetch_segments()?;
            if block.is_empty() {
                self.pipe.write_all(&self.buf).ok()?;
                self.buf.clear();
                return Some(0);
            }

            for s in segments {
                self.buf.extend_from_slice(&block[s.as_range()]);
            }

            let forward_count = segments.len();
            self.src.forward_segments(forward_count);
        }

        self.pipe.write_all(&self.buf).ok()?;
        self.buf.clear();
        Some(1)
    }
}

impl ConsumeSegments for PatchDrain {
    fn consume_segments(&mut self) -> Option<usize> {
        while let Some(len) = self.consume_segments_impl() {
            if len == 0 {
                let drain = self.drain.take().unwrap();
                drain.join().unwrap();
                return Some(0);
            }
        }

        let drain = self.drain.take().unwrap();
        drain.join().unwrap();

        None
    }
}

// end of patch.rs
