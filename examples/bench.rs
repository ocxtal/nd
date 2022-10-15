use anyhow::{anyhow, Result};
use std::io::Write;
use std::process::Command;
use std::time::Instant;
use tempfile::NamedTempFile;

fn setup_file(cmd: &str) -> Result<(NamedTempFile, String)> {
    let file = NamedTempFile::new().unwrap();
    let name = file.path().to_str().unwrap().to_string();

    let cmd = format!("{} > {}", cmd, &name);
    let status = Command::new("bash").args(["-c", &cmd]).status()?;
    if !status.success() {
        return Err(anyhow!("\"bash -c {}\" returned an error or aborted", &cmd));
    }

    Ok((file, name))
}

fn header(out: &mut impl Write, labels: &[&str]) {
    for (i, label) in labels.iter().enumerate() {
        let is_last = i == labels.len() - 1;

        write!(out, "{:?}{}", label, if is_last { '\n' } else { '\t' }).unwrap();
    }
}

fn body(out: &mut impl Write, results: &[Option<f64>]) {
    for (i, result) in results.iter().enumerate() {
        let is_last = i == results.len() - 1;

        if let Some(result) = result {
            write!(out, "{:.4}{}", result, if is_last { '\n' } else { '\t' }).unwrap();
        } else {
            write!(out, "-{}", if is_last { '\n' } else { '\t' }).unwrap();
        }
    }
}

fn lat(cmd: &str) -> Result<Option<f64>> {
    let start = Instant::now();

    let status = Command::new("bash").args(["-c", &cmd]).status()?;
    if !status.success() {
        return Err(anyhow!("\"bash -c {}\" returned an error or aborted", &cmd));
    }

    let end = Instant::now();
    let elapsed = end.duration_since(start);

    Ok(Some(elapsed.as_micros() as f64))
}

fn thr(cmd: &str, size: f64) -> Result<Option<f64>> {
    let elapsed = lat(cmd)?;

    // megabytes per second
    Ok(Some(size / elapsed.unwrap()))
}

fn bench_format(out: &mut impl Write) -> Result<()> {
    let blob_size = 256 * 1024 * 1024;
    let widths = [2, 6, 16, 48, 128, 384, 1024, 3072, 8192, 24576, 65536];

    let (_file, name) = setup_file(&format!("cat /dev/urandom | head -c {}", blob_size))?;

    header(out, &["width", "nd", "xxd -g1", "od -tx1", "hexdump -C"]);

    let blob_size = blob_size as f64;
    for w in widths {
        body(
            out,
            &[
                Some(w as f64),
                thr(&format!("./target/release/nd -w{} {} > /dev/null", w, name), blob_size)?,
                thr(&format!("xxd -g1 -c{} {} > /dev/null", w, name), blob_size)?,
                thr(&format!("od -tx1 -w{} {} > /dev/null", w, name), blob_size)?,
                if w == 16 {
                    thr(&format!("hexdump -C {} > /dev/null", name), blob_size)?
                } else {
                    None
                },
            ],
        );
    }

    Ok(())
}

fn bench_parse(out: &mut impl Write) -> Result<()> {
    let blob_size = 256 * 1024 * 1024;
    let widths = [2, 6, 16, 48, 128, 384, 1024, 3072, 8192, 24576, 65536];

    header(out, &["width", "nd -Fx -fb", "xxd -g1 -r"]);
    for w in widths {
        let nd = {
            let (_file, name) = setup_file(&format!("cat /dev/urandom | head -c{} | ./target/release/nd -w{}", blob_size, w))?;
            thr(&format!("./target/release/nd -Fx -fb {} > /dev/null", &name), blob_size as f64)?
        };

        let xxd = if w <= 256 {
            let (_file, name) = setup_file(&format!("cat /dev/urandom | head -c{} | xxd -g1 -c{}", blob_size, w))?;
            thr(&format!("xxd -g1 -r -c{} {} > /dev/null", w, &name), blob_size as f64)?
        } else {
            None
        };

        body(out, &[Some(w as f64), nd, xxd]);
    }

    Ok(())
}

fn main() -> Result<()> {
    let status = Command::new("mkdir").args(["-p", "results"]).status()?;
    if !status.success() {
        return Err(anyhow!("\"mkdir -p results\" returned an error or aborted"));
    }

    let mut format = std::fs::File::create("results/format.tsv")?;
    bench_format(&mut format)?;

    let mut parse = std::fs::File::create("results/parse.tsv")?;
    bench_parse(&mut parse)?;

    Ok(())
}
