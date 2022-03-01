// @file pipe.rs
// @author Hajime Suzuki

use std::io::{Read, Result, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

pub struct BashPipe {
    child: Arc<Mutex<Child>>,
}

pub struct BashPipeReader {
    child: Arc<Mutex<Child>>,
}

impl BashPipe {
    fn new(command: &str) -> BashPipe {
        let mut child = Command::new("bash")
            .args(&["-c", command])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();

        BashPipe { child: Arc::new(Mutex::new(child)) }
    }

    fn spawn_reader(&self) -> BashPipeReader {
        BashPipeReader { child: Arc::clone(&self.child) }
    }

    fn write_all(&mut self, buf: &[u8]) -> Result<usize> {
        let child = self.child.lock().unwrap();
        child.stdin.unwrap().write_all(buf)?;
        Ok(buf.len())
    }
}

impl Read for BashPipeReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let child = self.child.lock().unwrap();
        child.stdout.unwrap().read(buf)
    }
}

// enf of pipe.rs
