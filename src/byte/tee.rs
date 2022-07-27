// @file tee.rs
// @author Hajime Suzuki

use crate::byte::ByteStream;
use crate::filluninit::FillUninit;
use crate::params::BLOCK_SIZE;
use crate::streambuf::StreamBuf;
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::{Arc, Mutex};
use tempfile::SpooledTempFile;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use rand::Rng;

struct TempFile {
    clear_eof: bool, // semaphore with a saturation counter
    file: SpooledTempFile,
}

pub struct TeeStream {
    src: Box<dyn ByteStream>,
    cache: Arc<Mutex<TempFile>>,
}

pub struct TeeStreamReader {
    cache: Arc<Mutex<TempFile>>,
    buf: StreamBuf,
    offset: usize,
}

impl TeeStream {
    pub fn new(src: Box<dyn ByteStream>) -> Self {
        let tempfile = TempFile {
            clear_eof: false,
            file: SpooledTempFile::new(128 * BLOCK_SIZE),
        };

        TeeStream {
            src,
            cache: Arc::new(Mutex::new(tempfile)),
        }
    }

    pub fn spawn_reader(&self) -> TeeStreamReader {
        TeeStreamReader {
            cache: Arc::clone(&self.cache),
            buf: StreamBuf::new(),
            offset: 0,
        }
    }
}

impl ByteStream for TeeStream {
    fn fill_buf(&mut self) -> std::io::Result<usize> {
        self.src.fill_buf()
    }

    fn as_slice(&self) -> &[u8] {
        self.src.as_slice()
    }

    fn consume(&mut self, bytes: usize) {
        let stream = self.src.as_slice();
        debug_assert!(stream.len() >= bytes);

        match self.cache.lock() {
            Ok(mut cache) => {
                cache.file.write_all(&stream[..bytes]).unwrap();
                cache.clear_eof = true; // increment semaphore
            }
            _ => panic!("failed to lock cache."),
        }

        self.src.consume(bytes);
    }
}

impl ByteStream for TeeStreamReader {
    fn fill_buf(&mut self) -> std::io::Result<usize> {
        match self.cache.lock() {
            Ok(mut cache) => {
                if cache.clear_eof {
                    self.buf.clear_eof();
                    cache.clear_eof = false; // decrement semaphore
                }
                self.buf.fill_buf(|buf| {
                    cache.file.seek(SeekFrom::Start(self.offset as u64)).unwrap();
                    self.offset += buf.fill_uninit(BLOCK_SIZE, |buf| cache.file.read(buf))?;
                    Ok(false)
                })
            }
            _ => panic!("failed to lock cache."),
        }
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

#[allow(unused_macros)]
macro_rules! test_through {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let mut rng = rand::thread_rng();
            let pattern = (0..32 * 1024).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

            $inner(TeeStream::new(Box::new(MockSource::new(&pattern))), &pattern);
        }
    };
}

test_through!(test_tee_through_random_len, test_stream_random_len);
test_through!(test_tee_through_random_consume, test_stream_random_consume);
test_through!(test_tee_through_all_at_once, test_stream_all_at_once);

#[allow(unused_macros)]
macro_rules! test_cache_all {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let mut rng = rand::thread_rng();
            let pattern = (0..32 * 1024).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

            let mut stream = TeeStream::new(Box::new(MockSource::new(&pattern)));
            loop {
                let len = stream.fill_buf().unwrap();
                if len == 0 {
                    break;
                }
                stream.consume(rng.gen_range(1..=len));
            }

            $inner(stream.spawn_reader(), &pattern);
        }
    };
}

test_cache_all!(test_tee_reader_random_len, test_stream_random_len);
test_cache_all!(test_tee_reader_random_consume, test_stream_random_consume);
test_cache_all!(test_tee_reader_all_at_once, test_stream_all_at_once);

#[allow(unused_macros)]
macro_rules! test_cache_incremental {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let mut rng = rand::thread_rng();
            let pattern = (0..32 * 1024).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

            let mut stream = TeeStream::new(Box::new(MockSource::new(&pattern)));
            let mut reader = stream.spawn_reader();
            let mut offset = 0;

            while offset < pattern.len() {
                let base = offset;
                let tail = rng.gen_range(base..std::cmp::min(pattern.len(), base + 1024));

                while offset < tail {
                    let len = stream.fill_buf().unwrap();
                    offset += len;
                    stream.consume(len);
                }

                reader = $inner(reader, &pattern[base..offset]);
            }
            assert_eq!(offset, pattern.len());
        }
    };
}

test_cache_incremental!(test_tee_reader_incremental_random_len, test_stream_random_len);
test_cache_incremental!(test_tee_reader_incremental_random_consume, test_stream_random_consume);
test_cache_incremental!(test_tee_reader_incremental_all_at_once, test_stream_all_at_once);

// end of tee.rs
