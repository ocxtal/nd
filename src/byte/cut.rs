// @file cut.rs
// @author Hajime Suzuki

use super::ByteStream;
use crate::mapper::RangeMapper;
use crate::streambuf::StreamBuf;
use anyhow::Result;
use std::cmp::Reverse;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use rand::Rng;

struct Cutter {
    filters: Vec<RangeMapper>,      // filters that both ends are start-anchored
    tail_filters: Vec<RangeMapper>, // filters that have tail-anchored ends
    trans_offset: usize,            // minimum start offset among {StartAnchored(x)..EndAnchored(y)}
    tail_margin: usize,             // #bytes to be left at the tail
}

impl Cutter {
    fn from_str(exprs: &str) -> Result<Self> {
        let mut filters = Vec::new();
        let mut tail_filters = Vec::new();

        if !exprs.is_empty() {
            for expr in exprs.strip_suffix(',').unwrap_or(exprs).split(',') {
                let expr = RangeMapper::from_str(expr)?;
                if expr.has_right_anchor() {
                    tail_filters.push(expr);
                } else {
                    filters.push(expr);
                }
            }
        }
        filters.sort_by_key(|x| Reverse(x.sort_key()));

        let trans_offset = tail_filters.iter().map(|x| x.trans_offset()).min().unwrap_or(usize::MAX);
        let tail_margin = tail_filters.iter().map(|x| x.tail_margin()).max().unwrap_or(0);

        Ok(Cutter {
            filters,
            tail_filters,
            trans_offset,
            tail_margin,
        })
    }

    fn min_fill_bytes(&self, request: usize) -> usize {
        request + self.tail_margin + 1
    }

    fn is_empty(&self) -> bool {
        self.tail_filters.is_empty() && self.filters.is_empty()
    }

    fn accumulate(&mut self, offset: usize, is_eof: bool, bytes: usize, stream: &[u8], v: &mut Vec<u8>) -> Result<usize> {
        // when it reached EOF, convert all right-anchored and mixed ranges to
        // left-anchored ones, as the absolute offset finally got known
        if is_eof && !self.tail_filters.is_empty() {
            for filter in &self.tail_filters {
                self.filters.push(filter.to_left_anchored(offset + bytes));
            }

            self.tail_filters.clear();
            self.filters.sort_by_key(|x| Reverse(x.sort_key()));
        }

        // if not reached EOF, we can forward the pointer up to the trans_offset
        // (body -> tail transition offset) at most. we use `clip` for clipping
        // the ranges in the loop below.
        let scan_upto = if is_eof {
            bytes
        } else {
            let bytes = bytes.saturating_sub(self.tail_margin);
            let clip = self.trans_offset.saturating_sub(offset);
            std::cmp::min(bytes, clip)
        };

        let mut last_scanned = 0;
        while let Some(filter) = self.filters.pop() {
            // evaluate the filter range into a relative offsets on the current segment array
            let range = filter.to_range(offset);

            // becomes an empty range if the whole `range.start..range.end` is before the pointer
            // (i.e., the range is completely covered by one of the previous ranges)
            let start = std::cmp::max(range.start, last_scanned);
            let end = std::cmp::max(range.end, last_scanned);

            // becomes an empty range if the whole `start..end` is after the clipping offset
            // (i.e., the range is completely out of the current window)
            let start = std::cmp::min(start, scan_upto);
            let end = std::cmp::min(end, scan_upto);

            v.extend_from_slice(&stream[start..end]);
            last_scanned = end;

            // if not all consumed, the remainders are postponed to the next call
            if !is_eof && range.end > scan_upto {
                self.filters.push(filter);
                break;
            }
        }
        Ok(scan_upto)
    }
}

pub struct CutStream {
    src: Box<dyn ByteStream>,
    src_consumed: usize, // absolute offset in bytes from the head

    buf: StreamBuf,
    cutter: Cutter,
}

impl CutStream {
    pub fn new(src: Box<dyn ByteStream>, exprs: &str) -> Result<Self> {
        Ok(CutStream {
            src,
            src_consumed: 0,
            buf: StreamBuf::new(),
            cutter: Cutter::from_str(exprs)?,
        })
    }
}

impl ByteStream for CutStream {
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        self.buf.fill_buf(request, |request, buf| {
            let request = self.cutter.min_fill_bytes(request);

            let (is_eof, bytes) = self.src.fill_buf(request)?;
            let stream = self.src.as_slice();

            let scanned = self.cutter.accumulate(self.src_consumed, is_eof, bytes, stream, buf)?;
            self.src.consume(scanned);
            self.src_consumed += scanned;

            let is_eof = is_eof || self.cutter.is_empty();
            Ok(is_eof)
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, bytes: usize) {
        self.buf.consume(bytes);
    }
}

#[cfg(test)]
macro_rules! test_impl {
    ( $inner: ident, $input: expr, $exprs: expr, $expected: expr ) => {
        let src = Box::new(MockSource::new($input));
        let src = CutStream::new(src, $exprs).unwrap();
        $inner(src, $expected);
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // pass all
            test_impl!($inner, b"", "..", b"");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "..", b"abcdefghijklmnopqrstu");

            // trailing ',' allowed
            test_impl!($inner, b"abcdefghijklmnopqrstu", "..,", b"abcdefghijklmnopqrstu");

            // pass none
            test_impl!($inner, b"abcdefghijklmnopqrstu", "s..s", b"");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "e..e", b"");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "s - 1..s", b"");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "s + 1..s", b"");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "e..e - 1", b"");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "e..e + 1", b"");

            // left-anchored
            test_impl!($inner, b"abcdefghijklmnopqrstu", "s..s + 3", b"abc");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "s + 10..s + 13", b"klm");
            test_impl!(
                $inner,
                b"abcdefghijklmnopqrstu",
                "s..s + 3,s + 5..s + 10,s + 10..s + 13",
                b"abcfghijklm"
            );

            // left-anchored; overlaps
            test_impl!(
                $inner,
                b"abcdefghijklmnopqrstu",
                "s..s + 3,s + 1..s + 5,s + 10..s + 13,s + 12..s + 15",
                b"abcdeklmno"
            );
            test_impl!(
                $inner,
                b"abcdefghijklmnopqrstu",
                "s + 10..s + 20,s + 12..s + 15",
                b"klmnopqrst"
            );
            test_impl!(
                $inner,
                b"abcdefghijklmnopqrstu",
                "s + 10..s + 20,s + 12..s + 15,s + 17..s + 21",
                b"klmnopqrstu"
            );

            // left- and right-anchored
            test_impl!($inner, b"abcdefghijklmnopqrstu", "s..e - 11", b"abcdefghij");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "e - 11..s + 13", b"klm");
            test_impl!($inner, b"abcdefghijklmnopqrstu", "e - 11..e - 8", b"klm");
            test_impl!(
                $inner,
                b"abcdefghijklmnopqrstu",
                "s..s + 3,e - 16..s + 10,e - 11..e - 8",
                b"abcfghijklm"
            );
        }
    };
}

test!(test_cut_random_len, test_stream_random_len);
test!(test_cut_random_consume, test_stream_random_consume);
test!(test_cut_all_at_once, test_stream_all_at_once);

#[cfg(test)]
fn gen_pattern(len: usize, count: usize) -> (Vec<u8>, String, Vec<u8>) {
    let mut rng = rand::thread_rng();

    // first generate random slices
    let mut s = String::new();
    let mut v = Vec::new();

    for _ in 0..count {
        let pos1 = rng.gen_range(0..len);
        let pos2 = rng.gen_range(0..len);
        if pos1 == pos2 {
            continue;
        }

        let (start, end) = if pos1 < pos2 { (pos1, pos2) } else { (pos2, pos1) };
        v.push(start..end);

        let anchor_range = if start < len / 2 { 1 } else { 4 };

        // gen anchors and format string
        let dup = rng.gen_range(0..10) == 0;
        let mut push = || match rng.gen_range(0..anchor_range) {
            0 => s.push_str(&format!("s+{}..s+{},", start, end)),
            1 => s.push_str(&format!("s+{}..e-{},", start, len - end)),
            2 => s.push_str(&format!("e-{}..s+{},", len - start, end)),
            _ => s.push_str(&format!("e-{}..e-{},", len - start, len - end)),
        };

        push();
        if dup {
            push();
        }
    }
    v.sort_by_key(|x| (x.start, x.end));
    v.dedup();

    // merge the slices
    if !v.is_empty() {
        let mut i = 0;
        for j in 1..v.len() {
            if v[i].end < v[j].start {
                i += 1;
                v[i] = v[j].clone();
                continue;
            }
            v[i].end = std::cmp::max(v[i].end, v[j].end);
        }
        v.truncate(i + 1);
    }

    // generate random string
    let t = (0..len).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

    // slice and concatenate the string
    let mut u = Vec::new();
    for r in &v {
        u.extend_from_slice(&t[r.clone()]);
    }

    (t, s, u)
}

#[cfg(test)]
macro_rules! test_long_impl {
    ( $inner: ident, $len: expr, $count: expr ) => {
        let (input, exprs, expected) = gen_pattern($len, $count);
        let src = Box::new(MockSource::new(&input));
        let src = CutStream::new(src, &exprs).unwrap();
        $inner(src, &expected);
    };
}

macro_rules! test_long {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_long_impl!($inner, 0, 0);
            test_long_impl!($inner, 10, 0);
            test_long_impl!($inner, 10, 1);

            test_long_impl!($inner, 1000, 0);
            test_long_impl!($inner, 1000, 10);

            // try longer, multiple times
            test_long_impl!($inner, 100000, 100);
            test_long_impl!($inner, 100000, 100);
            test_long_impl!($inner, 100000, 100);
            test_long_impl!($inner, 100000, 100);
            test_long_impl!($inner, 100000, 100);
        }
    };
}

test_long!(test_cut_long_random_len, test_stream_random_len);
test_long!(test_cut_long_random_consume, test_stream_random_consume);
test_long!(test_cut_long_all_at_once, test_stream_all_at_once);

#[cfg(test)]
macro_rules! test_inf_impl {
    ( $exprs: expr, $expected: expr ) => {
        let src = Box::new(std::fs::File::open("/dev/zero").unwrap());
        let src = Box::new(RawStream::new(src, 1, 0));
        let mut src = Box::new(CutStream::new(src, $exprs).unwrap());

        let mut v = Vec::new();
        loop {
            let (is_eof, bytes) = src.fill_buf(1).unwrap();
            if is_eof && bytes == 0 {
                break;
            }

            let stream = src.as_slice();
            v.extend_from_slice(&stream[..bytes]);

            src.consume(bytes);
        }

        assert_eq!(v.len(), $expected);
    };
}

#[test]
fn test_cut_inf() {
    test_inf_impl!("0..16", 16);
    test_inf_impl!("100..116", 16);
    test_inf_impl!("10000..10016", 16);
    test_inf_impl!("1000000..1000016", 16);
}
// end of cut.rs
