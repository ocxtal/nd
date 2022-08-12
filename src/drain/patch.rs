// @file patch.rs
// @author Hajime Suzuki

use crate::byte::{ByteStream, PatchStream, RawStream};
use crate::params::BLOCK_SIZE;
use crate::segment::SegmentStream;
use crate::text::{InoutFormat, TextFormatter};
use std::io::Write;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread::JoinHandle;

struct BashPipe {
    child: Child,
}

struct BashPipeWriter {
    input: Option<ChildStdin>,
}

impl BashPipe {
    fn new(command: &str) -> Self {
        let child = Command::new("bash")
            .args(&["-c", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        BashPipe { child }
    }

    fn spawn_reader(&mut self) -> RawStream {
        let output = self.child.stdout.take().unwrap();
        RawStream::new(Box::new(output), 1)
    }

    fn spawn_writer(&mut self) -> BashPipeWriter {
        let input = self.child.stdin.take().unwrap();
        BashPipeWriter { input: Some(input) }
    }
}

impl BashPipeWriter {
    fn write_all(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.input.as_ref().unwrap().write_all(buf)?;
        Ok(buf.len())
    }

    fn close(&mut self) {
        self.input.take();
    }
}

pub struct PatchDrain {
    patch: PatchStream,
    prev_bytes: usize,
    pipe: BashPipe,
    thread: Option<JoinHandle<()>>,
}

impl PatchDrain {
    pub fn new(patch: Box<dyn SegmentStream>, original: Box<dyn ByteStream>, command: &str, format: &InoutFormat) -> Self {
        let mut pipe = BashPipe::new(command);
        let mut writer = pipe.spawn_writer();
        let formatter = TextFormatter::new(format, (0, 0), 0);

        let thread = std::thread::spawn(move || {
            let mut patch = patch;
            let mut buf = Vec::new();
            let mut offset = 0;

            loop {
                let (is_eof, bytes, _, max_consume) = patch.fill_segment_buf().unwrap();
                if is_eof && bytes == 0 {
                    break;
                }

                let (stream, segments) = patch.as_slices();
                formatter.format_segments(offset, stream, segments, &mut buf);
                offset += patch.consume(max_consume).unwrap().0;

                if buf.len() >= BLOCK_SIZE {
                    writer.write_all(&buf).unwrap();
                    buf.clear();
                }
            }

            writer.write_all(&buf).unwrap();
            writer.close();
        });

        let reader = pipe.spawn_reader();
        let patch = PatchStream::new(original, Box::new(reader), format);

        PatchDrain {
            patch,
            prev_bytes: 0,
            pipe,
            thread: Some(thread),
        }
    }
}

impl ByteStream for PatchDrain {
    fn fill_buf(&mut self) -> std::io::Result<usize> {
        let bytes = self.patch.fill_buf()?;

        if bytes == self.prev_bytes {
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }

            self.pipe.child.wait().unwrap();
        }

        self.prev_bytes = bytes;
        Ok(bytes)
    }

    fn as_slice(&self) -> &[u8] {
        self.patch.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.patch.consume(amount)
    }
}

// end of patch.rs
