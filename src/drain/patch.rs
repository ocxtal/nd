// @file patch.rs
// @author Hajime Suzuki

use crate::byte::{ByteStream, PatchStream};
use crate::drain::StreamDrain;
use crate::filluninit::FillUninit;
use crate::params::BLOCK_SIZE;
use crate::segment::SegmentStream;
use crate::streambuf::StreamBuf;
use crate::text::TextFormatter;
use std::io::{Read, Result, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread::JoinHandle;

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

impl ByteStream for BashPipeReader {
    fn fill_buf(&mut self) -> Result<usize> {
        self.buf.fill_buf(|buf| {
            buf.fill_uninit(BLOCK_SIZE, |buf| self.output.read(buf))?;
            Ok(false)
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
    patch: Box<dyn SegmentStream>,
    offset: usize,
    formatter: TextFormatter,
    buf: Vec<u8>,
    pipe: BashPipe,
    drain: Option<JoinHandle<()>>,
}

impl PatchDrain {
    pub fn new(
        patch: Box<dyn SegmentStream>,
        original: Box<dyn ByteStream + Send>,
        command: &str,
        formatter: TextFormatter,
        dst: Box<dyn Write + Send>,
    ) -> Self {
        let mut pipe = BashPipe::new(command);
        let pipe_reader = pipe.spawn_reader();

        let format = formatter.format();
        let drain = std::thread::spawn(move || {
            let mut dst = dst;
            let mut patch = PatchStream::new(original, Box::new(pipe_reader), &format);

            loop {
                let len = patch.fill_buf().unwrap();
                if len == 0 {
                    break;
                }

                let stream = patch.as_slice();
                dst.write_all(&stream[..len]).unwrap();
                patch.consume(len);
            }
        });

        PatchDrain {
            patch,
            offset: 0,
            formatter,
            buf: Vec::new(),
            pipe,
            drain: Some(drain),
        }
    }

    fn consume_segments_impl(&mut self) -> Result<usize> {
        debug_assert!(!self.buf.is_empty());

        while self.buf.len() < BLOCK_SIZE {
            let (bytes, _) = self.patch.fill_segment_buf()?;
            if bytes == 0 {
                self.pipe.write_all(&self.buf).unwrap();
                self.pipe.close();

                self.buf.clear();
                return Ok(0);
            }

            let (stream, segments) = self.patch.as_slices();
            self.formatter.format_segments(self.offset, stream, segments, &mut self.buf);
            self.offset += self.patch.consume(bytes)?.0;
        }

        self.pipe.write_all(&self.buf).unwrap();
        self.buf.clear();
        Ok(1)
    }
}

impl StreamDrain for PatchDrain {
    fn consume_segments(&mut self) -> Result<usize> {
        let mut core_impl = || -> Result<usize> {
            loop {
                let ret = self.consume_segments_impl()?;
                if ret == 0 {
                    return Ok(ret);
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
