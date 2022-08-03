// @file scatter.rs
// @author Hajime Suzuki

use crate::byte::ByteStream;
use crate::segment::SegmentStream;
use crate::streambuf::StreamBuf;
use crate::text::{InoutFormat, TextFormatter};
use std::io::{Read, Write};
// use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
// use std::sync::mpsc::{channel, Sender};
// use std::thread::JoinHandle;

// fn create_pipe(args: &str, offset: usize, line: usize) -> (Child, ChildStdin, ChildStdout) {
//     let mut child = Command::new("bash")
//         .args(&["-c", args])
//         .stdin(Stdio::piped())
//         .stdout(Stdio::piped())
//         .env("n", &format!("{:?}", offset))
//         .env("l", &format!("{:?}", line))
//         .spawn()
//         .unwrap();

//     let input = child.stdin.take().unwrap();
//     let output = child.stdout.take().unwrap();

//     (child, input, output)
// }

pub struct ScatterDrain {
    src: Box<dyn SegmentStream>,
    offset: usize,
    lines: usize,
    formatter: TextFormatter,
    filename: String,
    buf: StreamBuf,
    // command: String,
    // sender: Sender<Option<(Child, ChildStdout)>>,
    // drain: Option<JoinHandle<()>>,
}

impl ScatterDrain {
    pub fn new(
        src: Box<dyn SegmentStream>,
        filename: &str,
        format: &InoutFormat,
        // formatter: TextFormatter,
        // dst: Box<dyn Write + Send>,
    ) -> Self {
        // let command = command.to_string();
        // let (sender, reciever) = channel::<Option<(Child, ChildStdout)>>();

        // let drain = std::thread::spawn(move || {
        //     // let mut dst = dst;
        //     let mut buf = Vec::new();
        //     buf.resize(2 * BLOCK_SIZE, 0);

        //     while let Some((_, output)) = reciever.recv().unwrap() {
        //         let mut output = output;
        //         while let Ok(len) = output.read(&mut buf) {
        //             if len == 0 {
        //                 break;
        //             }
        //             // dst.write_all(&buf[..len]).unwrap();
        //         }
        //     }
        // });
        // let drain = Some(drain);

        let formatter = TextFormatter::new(format, (0, 0), 0);
        let filename = filename.to_string();

        ScatterDrain {
            src,
            offset: 0, // TODO: parameterize?
            lines: 0,  // TODO: parameterize?
            formatter,
            filename,
            buf: StreamBuf::new(),
            // command,
            // sender,
            // drain,
        }
    }

    // fn consume_segments_impl(&mut self) -> std::io::Result<usize> {
    //     let (bytes, count) = self.src.fill_segment_buf()?;
    //     if bytes == 0 {
    //         self.sender.send(None).unwrap();

    //         let drain = self.drain.take().unwrap();
    //         drain.join().unwrap();
    //         return Ok(0);
    //     }

    //     let (stream, segments) = self.src.as_slices();
    //     for (i, s) in segments.chunks(1).enumerate() {
    //         // format to text
    //         self.buf.clear();
    //         self.formatter.format_segments(self.offset, stream, s, &mut self.buf);

    //         // dump
    //         let (child, input, output) = create_pipe(&self.command, self.offset + s[0].pos, self.lines + i);
    //         let mut input = input;

    //         input.write_all(&self.buf).unwrap();
    //         self.sender.send(Some((child, output))).unwrap();
    //     }

    //     let consumed = self.src.consume(bytes)?;
    //     debug_assert!(consumed.1 == count);
    //     self.offset += consumed.0;
    //     self.lines += consumed.1;

    //     Ok(1)
    // }
}

impl ByteStream for ScatterDrain {
    fn fill_buf(&mut self) -> std::io::Result<usize> {
        self.buf.fill_buf(|buf| {
            let (bytes, _) = self.src.fill_segment_buf()?;
            if bytes == 0 {
                return Ok(false);
            }

            let (stream, segments) = self.src.as_slices();
            self.formatter.format_segments(self.offset, stream, segments, buf);

            self.offset += self.src.consume(bytes)?.0;
            Ok(false)
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount)
    }
}

// impl StreamDrain for ScatterDrain {
//     fn consume_segments(&mut self) -> std::io::Result<usize> {
//         loop {
//             let len = self.consume_segments_impl()?;
//             if len == 0 {
//                 return Ok(0);
//             }
//         }
//     }
// }

// end of scatter.rs
