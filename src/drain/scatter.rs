// @file scatter.rs
// @author Hajime Suzuki

use crate::common::{ConsumeSegments, FetchSegments, BLOCK_SIZE};
use std::io::{Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::thread::JoinHandle;

fn create_pipe(args: &str) -> (Child, ChildStdin, ChildStdout) {
    let mut child = Command::new("bash")
        .args(&["-c", args])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();

    let input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();

    (child, input, output)
}

pub struct ScatterDrain {
    src: Box<dyn FetchSegments>,
    command: String,
    sender: Sender<Option<(Child, ChildStdin, ChildStdout)>>,
    drain: Option<JoinHandle<()>>,
}

impl ScatterDrain {
    pub fn new(src: Box<dyn FetchSegments>, dst: Box<dyn Write + Send>, command: &str) -> Self {
        let command = command.to_string();

        let (sender, reciever) = channel::<Option<(Child, ChildStdin, ChildStdout)>>();
        let drain = std::thread::spawn(move || {
            let mut dst = dst;
            let mut buf = Vec::with_capacity(2 * BLOCK_SIZE);
            while let Some((_, _, output)) = reciever.recv().unwrap() {
                let mut output = output;
                while let Ok(len) = output.read(&mut buf) {
                    if len == 0 {
                        break;
                    }
                    dst.write_all(&buf[..len]).unwrap();
                }
            }
        });
        let drain = Some(drain);

        ScatterDrain {
            src,
            command,
            sender,
            drain,
        }
    }
}

impl ConsumeSegments for ScatterDrain {
    fn consume_segments(&mut self) -> Option<usize> {
        let (_, block, segments) = self.src.fetch_segments()?;
        if block.is_empty() {
            self.sender.send(None).unwrap();

            let drain = self.drain.take().unwrap();
            drain.join().unwrap();
            return Some(0);
        }

        for s in segments {
            let (child, input, output) = create_pipe(&self.command);
            let mut input = input;
            input.write_all(&block[s.as_range()]).ok()?;
            self.sender.send(Some((child, input, output))).unwrap();
        }

        let count = segments.len();
        self.src.forward_segments(count);

        Some(1)
    }
}

// end of scatter.rs
