#![feature(stdsimd)]

pub mod aarch64;
pub mod x86_64;

use clap::{arg, Arg, App, AppSettings, ColorChoice};
use std::io::{Read, Write};
use std::ops::Range;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
use aarch64::{encode::*, decode::*};

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
use x86_64::encode::*;

static USAGE: &str = "zd [options] <input.bin>... > <output.txt>";

static HELP_TEMPLATE: &str = "
  {bin} {version} -- {about}

USAGE:

  {usage}

OPTIONS:

  Input and output formats (apply to all input / output streams)

    -i, --in-format XYZ     input format signatures [b]
    -o, --out-format XYZ    output format signatures [xxx]
    -j, --patch-format XYZ  patchfile format signatures [xxx]

  Constructing input stream

    -c, --cat W             concat all input streams into one with W-byte words (default) [1]
    -z, --zip W             zip all input streams into one with W-byte words

    -s, --seek N            skip first N bytes and clear stream offset [0]
    -r, --range N:M         drop bytes out of the range [0:inf]

    -p, --patch FILE        patch the input stream with the patchfile (after all of the above)
    -b, --patch-op OP       do binary operation on patching

  Manipulating the input stream

    -B, --op W:OP           apply unary bit operation to every W-byte word
    -L, --splice W:INDICES  output only bytes at the indices in every W-byte word
    -Y, --shuffle W:INDICES shuffle bytes with the indices in W-byte words
    -X, --multiply W:MATRIX multiply bitmatrix to every W-byte word
    -S, --substitute W:MAP  substitute word with the map

    -H, --histogram W:B:E   compute a histogram of bits B:E in W-byte words on finite input stream
    -O, --sort W:B:E        sort finite input stream of W-byte words by bits from N to M
    -D, --cluster W:B:E     cluster binary pattern by bits B:E in W-byte words
    -R, --reverse W         reverse finite input stream of W-byte words
    -T, --transpose W:SHAPE:INDICES transpose finite input stream with the shape and its indices

  Slicing the manipulated stream

    -w, --width N           slice into N bytes (default) [16]
    -m, --match PATTERN[:K] slice out every matches whose Hamming distance is less than K from the pattern
    -e, --regex PATTERN     slice out every matches with regular expression (in PCRE)

    -G, --margin N:M        extend every slice to left and right by N and M bytes [0:0]
    -M, --merge N           merge two slices whose overlap is longer than N bytes (after extension) [0]
    -P, --pad N:M[:X]       pad slices left and right by N and M bytes (after extension and merging) [0:0]

  Grouping and post-processing the slices

    -l, --group N           group N slices (default) [1]
    -u, --uniq B:E          group slices whose bits from B to E are the same [0:inf]

    -f, --map CMD           apply shell command to every grouped slices []
    -d, --reduce CMD        pass outputs of --map command to another as input files []

  Miscellaneous

    -h, --help              print help (this) message
    -v, --version           print version information
    -V, --verbose           print stream diagram and verbose logs
";

fn main() {
    let m = App::new("zd")
        .version("0.0.1")
        .about("streamed binary processor")
        .help_template(HELP_TEMPLATE)
        .override_usage(USAGE)
        .color(ColorChoice::Never)
        .setting(AppSettings::TrailingVarArg)
        .setting(AppSettings::DontDelimitTrailingValues)
        .setting(AppSettings::InferLongArgs)
        .args([
            Arg::new("inputs").help("input files").value_name("input.bin").multiple_occurrences(true).default_value("-"),
            Arg::new("in-format").short('i').long("in-format").help("input format signatures [b]").takes_value(true).number_of_values(1),
            Arg::new("out-format").short('o').long("out-format").help("output format signatures [xxx]").takes_value(true).number_of_values(1),
            Arg::new("patch-format").short('j').long("patch-format").help("patchfile format signatures [xxx]").takes_value(true).number_of_values(1),
            Arg::new("cat").short('c').long("cat").help("concat all input streams into one with W-byte words (default) [1]").takes_value(true).min_values(0).possible_values(["1", "2", "4", "8", "16"]).default_missing_value("1"),
            Arg::new("zip").short('z').long("zip").help("zip all input streams into one with W-byte words").takes_value(true).min_values(0).possible_values(["1", "2", "4", "8", "16"]).default_missing_value("1").conflicts_with("cat"),
            Arg::new("seek").short('s').long("seek").help("skip first N bytes and clear stream offset [0]").takes_value(true).number_of_values(1),
            Arg::new("range").short('r').long("range").help("drop bytes out of the range [0:inf]").takes_value(true),
            Arg::new("patch").short('p').long("patch").help("patch the input stream with the patchfile (after all of the above)").takes_value(true),
            Arg::new("patch-op").short('b').long("patch-op").help("do binary operation on patching").takes_value(true),
            Arg::new("op").short('B').long("op").help("apply unary bit operation to every W-byte word").takes_value(true),
            Arg::new("splice").short('L').long("splice").help("output only bytes at the indices in every W-byte word").takes_value(true),
            Arg::new("shuffle").short('Y').long("shuffle").help("shuffle bytes with the indices in W-byte words").takes_value(true),
            Arg::new("multiply").short('X').long("multiply").help("multiply bitmatrix to every W-byte word").takes_value(true),
            Arg::new("substitute").short('S').long("substitute").help("substitute word with the map").takes_value(true),
            Arg::new("histogram").short('H').long("histogram").help("compute a histogram of bits B:E in W-byte words on finite input stream").takes_value(true),
            Arg::new("sort").short('O').long("sort").help("sort finite input stream of W-byte words by bits from N to M").takes_value(true),
            Arg::new("cluster").short('D').long("cluster").help("cluster binary pattern by bits B:E in W-byte words").takes_value(true),
            Arg::new("reverse").short('R').long("reverse").help("reverse finite input stream of W-byte words").takes_value(true),
            Arg::new("transpose").short('T').long("transpose").help("transpose finite input stream with the shape and its indices").takes_value(true),
            Arg::new("width").short('w').long("width").help("slice into N bytes (default) [16]").takes_value(true),
            Arg::new("match").short('m').long("match").help("slice out every matches whose Hamming distance is less than K from the pattern").takes_value(true),
            Arg::new("regex").short('e').long("regex").help("slice out every matches with regular expression (in PCRE)").takes_value(true),
            Arg::new("margin").short('G').long("margin").help("extend every slice to left and right by N and M bytes [0:0]").takes_value(true),
            Arg::new("merge").short('M').long("merge").help("merge two slices whose overlap is longer than N bytes (after extension) [0]").takes_value(true),
            Arg::new("pad").short('P').long("pad").help("pad slices left and right by N and M bytes (after extension and merging) [0:0]").takes_value(true),
            Arg::new("group").short('l').long("group").help("group N slices (default) [1]").takes_value(true),
            Arg::new("uniq").short('u').long("uniq").help("group slices whose bits from B to E are the same [0:inf]").takes_value(true),
            Arg::new("map").short('f').long("map").help("apply shell command to every grouped slices []").takes_value(true),
            Arg::new("reduce").short('d').long("reduce").help("pass outputs of --map command to another as input files []").takes_value(true),
            Arg::new("help").short('h').long("help").help("print help (this) message"),
            Arg::new("version").short('v').long("version").help("print version information"),
            Arg::new("verbose").short('V').long("verbose").help("print stream diagram and verbose logs"),
        ])
        .get_matches();

    let inputs: Vec<&str> = m.values_of("inputs").unwrap().collect();
    println!("{:?}", inputs);

    let input_format = if let Some(x) = m.value_of("in-format") {
        InoutFormat {
            offset: x.bytes().nth(0),
            length: x.bytes().nth(1),
            body: x.bytes().nth(2),
        }
    } else {
        InoutFormat {
            offset: Some(b'x'),
            length: Some(b'x'),
            body: Some(b'x'),
        }
    };
    println!("{:?}", input_format);

    let inputs: Vec<Box<dyn ReadBlock>> = inputs.iter().map(|x| -> Box<dyn ReadBlock> {
        let src = create_source(x);
        if input_format.offset == Some(b'b') {
            Box::new(BinaryStream::new(src, &input_format))
        } else {
            Box::new(HexStream::new(src, &input_format))
        }
    }).collect();

    let input: Box<dyn ReadBlock> = if let Some(x) = m.value_of_t("zip").ok() {
        Box::new(ZipStream::new(inputs, x))
    } else {
        Box::new(CatStream::new(inputs, m.value_of_t("cat").unwrap_or(1)))
    };

    let mut input: Box<dyn ReadBlock> = if let Some(x) = m.value_of_t("seek").ok() {
        Box::new(ClipStream::new(input, 0, x, isize::MAX as usize - 1))
    } else {
        input
    };

    let mut drain: Box<dyn DumpBlock> = Box::new(HexDrain::new(input, 0));
    while let Some(len) = drain.dump_block() {
        if len == 0 {
            break;
        }
    }
    println!("done");

    // if let Some(input_format) = m.value_of("zip") {
    //     println!("{:?}", input_format);
    // } else {
    //     println!("not found");
    // }

    // let opt = Opt::parse();
    // let app = <Opt as IntoApp>::into_app().help_template("{version}");
    // let opt = Opt::parse();

    // let elems_per_line = 16;
    // let header_width = 12;
    // let elems_per_chunk = 2 * 1024 * 1024;

    // let lines_per_chunk = (elems_per_chunk + elems_per_line - 1) / elems_per_line;
    // let bytes_per_in_line = elems_per_line;
    // let bytes_per_out_line = 16 + header_width + 5 * elems_per_line;

    // let bytes_per_in_chunk = bytes_per_in_line * lines_per_chunk;
    // let bytes_per_out_chunk = bytes_per_out_line * lines_per_chunk;

    // let in_buf_size = bytes_per_in_chunk + 256;
    // let out_buf_size = bytes_per_out_chunk + 256;

    // let mut in_buf = Vec::new();
    // let mut out_buf = Vec::new();

    // in_buf.resize(in_buf_size, 0);
    // out_buf.resize(out_buf_size, 0);

    // let args: Vec<String> = std::env::args().collect();
    // let mut src = create_source(&args[1]);
    // let mut dst = std::io::stdout();

    // let mut offset = 0;
    // loop {
    //     let len = src.read(&mut in_buf[..bytes_per_in_chunk]).unwrap();
    //     if len == 0 {
    //         break;
    //     }

    //     let mut p = 0;
    //     let mut q = 0;
    //     while q < len {
    //         p += format_line(&mut out_buf[p..], &in_buf[q..], offset, elems_per_line);
    //         q += elems_per_line;
    //         offset += elems_per_line;
    //     }

    //     if (len % elems_per_line) != 0 {
    //         patch_line(&mut out_buf[..p], len % elems_per_line, elems_per_line);
    //     }

    //     dst.write_all(&out_buf[..p]).unwrap();
    // }
}

fn format_line(dst: &mut [u8], src: &[u8], offset: usize, elems_per_line: usize) -> usize {
    // header; p is the current offset in the dst buffer
    let mut p = format_hex_single(dst, offset, 6);
    p += format_hex_single(&mut dst[p..], elems_per_line, 1);

    dst[p] = b'|';
    p += 1;
    dst[p] = b' ';
    p += 1;

    let n_blks = (elems_per_line + 0x0f) >> 4;
    let n_rem = 0usize.wrapping_sub(elems_per_line) & 0x0f;

    // body
    for i in 0..n_blks {
        p += format_hex_body(&mut dst[p..], &src[i * 16..]);
    }
    p -= 4 * n_rem;

    dst[p] = b'|';
    p += 1;
    dst[p] = b' ';
    p += 1;

    // mosaic
    for i in 0..n_blks {
        p += format_mosaic(&mut dst[p..], &src[i * 16..]);
    }
    p -= n_rem;

    dst[p] = b'\n';
    p + 1
}

fn patch_line(dst: &mut [u8], valid_elements: usize, elems_per_line: usize) {
    debug_assert!(valid_elements < elems_per_line);
    debug_assert!(dst.len() > 4 * elems_per_line);

    let last_line_offset = dst.len() - 4 * elems_per_line - 3;
    let (_, last) = dst.split_at_mut(last_line_offset);

    let (body, mosaic) = last.split_at_mut(3 * elems_per_line);
    for i in valid_elements..elems_per_line {
        body[3 * i] = b' ';
        body[3 * i + 1] = b' ';
        mosaic[i + 2] = b' ';
    }
    // body[4 * valid_elements + 1] = b'|';
}

trait WithUninit {
    fn with_uninit<T, F>(&mut self, len: usize, f: F) -> Option<T> where T: Sized, F: FnMut(&mut [u8]) -> Option<(T, usize)>;
}

impl WithUninit for Vec<u8> {
    fn with_uninit<T, F>(&mut self, len: usize, f: F) -> Option<T>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Option<(T, usize)>,
    {
        let mut f = f;

        if self.capacity() < self.len() + len {
            let shift = (self.len() + len).leading_zeros() as usize;
            debug_assert!(shift > 0);

            let new_len = 0x8000000000000000 >> (shift.min(56) - 1);
            self.reserve(new_len - self.len());
        }

        let arr = self.spare_capacity_mut();
        let arr = unsafe { std::mem::transmute::<&mut [std::mem::MaybeUninit::<u8>], &mut [u8]>(arr) };
        let ret = f(&mut arr[..len]);
        let clip = match ret {
            Some((_, clip)) => clip,
            None => 0,
        };
        unsafe { self.set_len(self.len() + clip) };

        match ret {
            Some((ret, _)) => Some(ret),
            None => None,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct InoutFormat {
    offset: Option<u8>,     // in {'b', 'd', 'x'}
    length: Option<u8>,     // in {'b', 'd', 'x'}
    body: Option<u8>,       // in {'b', 'd', 'x', 'a'}
}

struct HexdumpParser {
    src: Box<dyn Read>,

    // buffered reader
    buf: Vec<u8>,
    loaded: usize,
    consumed: usize,

    // parser for non-binary streams; bypassed for binary streams (though the functions are valid)
    parse_offset: fn(&[u8]) -> Option<(u64, usize)>,
    parse_length: fn(&[u8]) -> Option<(u64, usize)>,
    parse_body: fn(&[u8], &mut [u8]) -> Option<(usize, usize)>,
}

impl HexdumpParser {
    const MIN_MARGIN: usize = 128;

    fn new(src: Box<dyn Read>, format: &InoutFormat) -> HexdumpParser {
        let offset_key = format.offset.unwrap_or(b'x') as usize;
        let length_key = format.length.unwrap_or(b'x') as usize;
        let body_key = format.body.unwrap_or(b'x') as usize;
        assert!(offset_key != b'b' as usize);

        let header_parsers = {
            let mut t: [Option<fn(&[u8]) -> Option<(u64, usize)>>; 256] = [None; 256];
            t[b'd' as usize] = Some(parse_hex_single);  // parse_dec_single
            t[b'x' as usize] = Some(parse_hex_single);
            t[b'n' as usize] = Some(parse_hex_single);  // parse_none_single
            t
        };

        let body_parsers = {
            let mut t: [Option<fn(&[u8], &mut [u8]) -> Option<(usize, usize)>>; 256] = [None; 256];
            t[b'a' as usize] = Some(parse_hex_body);    // parse_contigous_hex_body
            t[b'd' as usize] = Some(parse_hex_body);    // parse_dec_body
            t[b'x' as usize] = Some(parse_hex_body);
            t[b'n' as usize] = Some(parse_hex_body);    // parse_none_body
            t
        };

        let mut buf = Vec::new();
        buf.resize(4 * 1024 * 1024, 0);
        HexdumpParser {
            src,
            buf,
            loaded: 0,
            consumed: 0,
            parse_offset: header_parsers[offset_key].expect("unrecognized parser key for header.offset"),
            parse_length: header_parsers[length_key].expect("unrecognized parser key for header.length"),
            parse_body: body_parsers[body_key].expect("unrecognized parser key for body"),
        }
    }

    fn fill_buf(&mut self) -> Option<usize> {
        debug_assert!(self.loaded <= 2 * self.consumed);
        self.loaded -= self.consumed;

        let (rem, src) = self.buf.split_at_mut(self.consumed);
        let (dst, _) = rem.split_at_mut(self.loaded);
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), self.loaded);
        }

        self.consumed -= self.consumed;
        self.loaded += self.src.read(&mut self.buf[self.loaded..]).ok()?;

        Some(self.loaded)
    }

    fn read_line(&mut self, buf: &mut Vec<u8>) -> Option<(usize, usize)> {
        if self.loaded < self.consumed + Self::MIN_MARGIN {
            self.fill_buf();
        }

        let (offset, fwd) = (self.parse_offset)(&self.buf[self.consumed..])?;
        self.consumed += fwd;

        let (length, fwd) = (self.parse_offset)(&self.buf[self.consumed..])?;
        self.consumed += fwd;

        assert!(self.buf[self.consumed] == b'|');
        assert!(self.buf[self.consumed + 1] == b'|');
        self.consumed += 2;

        loop {
            let src_fwd = buf.with_uninit(Self::MIN_MARGIN, |arr: &mut [u8]| {
                (self.parse_body)(&self.buf[self.consumed..], arr)
            })?;

            self.consumed += src_fwd;
            if self.loaded <= self.consumed + Self::MIN_MARGIN {
                self.fill_buf();
            }

            if src_fwd < 48 {
                break;
            }
        }

        Some((offset as usize, length as usize))
    }
}

const BLOCK_SIZE: usize = 2 * 1024 * 1024;

trait ReadBlock {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize>;
}

// struct PatchStream {
//     inner: HexdumpParser,
// }

// impl PatchStream {
//     fn new(src: Box<dyn Read>, format: &InoutFormat) {

//     }
// }

struct GaplessHexStream {
    inner: HexdumpParser,
}

impl GaplessHexStream {
    fn new(src: Box<dyn Read>, format: &InoutFormat) -> GaplessHexStream {
        assert!(format.offset == Some(b'x'));
        GaplessHexStream {
            inner: HexdumpParser::new(src, format),
        }
    }
}

impl ReadBlock for GaplessHexStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let mut acc = 0;
        while buf.len() < BLOCK_SIZE {
            let (_, len) = self.inner.read_line(buf)?;
            acc += len;

            if len == 0 {
                break;
            }
        }
        Some(acc)
    }
}

struct HexStreamCache {
    offset: Range<usize>,
    buf: Vec<u8>,
}

impl HexStreamCache {
    fn new() -> HexStreamCache {
        HexStreamCache {
            offset: 0..0,
            buf: Vec::new(),
        }
    }

    fn fill_buf(&mut self, src: &mut HexdumpParser) -> Option<usize> {
        self.buf.clear();

        let (offset, len) = src.read_line(&mut self.buf)?;
        self.offset = offset..offset + len;

        Some(len)
    }
}

struct HexStream {
    inner: HexdumpParser,
    curr: HexStreamCache,
    prev: HexStreamCache,
}

impl HexStream {
    fn new(src: Box<dyn Read>, format: &InoutFormat) -> HexStream {
        HexStream {
            inner: HexdumpParser::new(src, format),
            curr: HexStreamCache::new(),
            prev: HexStreamCache::new(),
        }
    }
}

impl ReadBlock for HexStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let mut acc = 0;
        while buf.len() < BLOCK_SIZE {
            self.curr.fill_buf(&mut self.inner)?;
            if self.curr.offset.start < self.prev.offset.start {
                panic!("offsets must be sorted in the ascending order");
            }

            // flush the previous line
            let flush_len = self.prev.offset.end.min(self.curr.offset.start) - self.prev.offset.start;
            buf.extend_from_slice(&self.prev.buf[..flush_len]);
            acc += flush_len;

            // pad the flushed line if they have a gap between
            let gap_len = self.curr.offset.start.saturating_sub(self.prev.offset.end);
            buf.resize(buf.len() + gap_len, 0);
            acc += gap_len;

            std::mem::swap(&mut self.curr, &mut self.prev);
        }
        Some(acc)
    }
}

struct BinaryStream {
    src: Box<dyn Read>,
}

impl BinaryStream {
    fn new(src: Box<dyn Read>, format: &InoutFormat) -> BinaryStream {
        assert!(format.offset == Some(b'b'));
        assert!(format.length.is_none());
        assert!(format.body.is_none());
        BinaryStream { src }
    }
}

impl ReadBlock for BinaryStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let len = buf.with_uninit(BLOCK_SIZE, |arr: &mut [u8]| {
            let len = self.src.read(arr).ok()?;
            Some((len, len))
        })?;

        Some(len)
    }
}

struct CatStream {
    srcs: Vec<Box<dyn ReadBlock>>,
    index: usize,
    align: usize,
}

impl CatStream {
    fn new(srcs: Vec<Box<dyn ReadBlock>>, align: usize) -> CatStream {
        CatStream {
            srcs,
            index: 0,
            align,
        }
    }
}

impl ReadBlock for CatStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let base_len = buf.len();
        while self.index < self.srcs.len() {
            let len = self.srcs[self.index].read_block(buf)?;
            if len == 0 {
                let aligned = ((buf.len() + self.align - 1) / self.align) * self.align;
                buf.resize(aligned, 0);
                self.index += 1;
            }

            if buf.len() > BLOCK_SIZE {
                break;
            }
        }
        return Some(buf.len() - base_len);
    }
}

struct ZipStreamCache {
    src: Box<dyn ReadBlock>,
    buf: Vec<u8>,
    avail: usize,
    consumed: usize,
}

impl ZipStreamCache {
    fn new(src: Box<dyn ReadBlock>) -> ZipStreamCache {
        ZipStreamCache {
            src,
            buf: Vec::new(),
            avail: 0,
            consumed: 0,
        }
    }

    fn fill_buf(&mut self, align: usize) -> Option<usize> {
        if self.buf.len() > self.consumed + BLOCK_SIZE {
            return Some(0);
        }

        let tail = self.buf.len();
        self.buf.copy_within(self.consumed..tail, 0);
        self.buf.truncate(tail - self.consumed);

        self.avail -= self.consumed;
        self.consumed = 0;

        while self.buf.len() < BLOCK_SIZE {
            let len = self.src.read_block(&mut self.buf)?;

            if len == 0 {
                let padded_len = (self.buf.len() + align - 1) & !(align - 1);
                self.buf.resize(padded_len, 0);
                break;
            }
        }

        self.avail = self.buf.len() & !(align - 1);
        Some(self.avail - self.consumed)
    }
}

struct ZipStream {
    srcs: Vec<ZipStreamCache>,
    ptrs: Vec<*const u8>,       // pointer cache (only for use in the gather function)
    gather: fn(&mut Self, &mut Vec<u8>) -> Option<usize>,
    align: usize,
}

macro_rules! gather {
    ( $name: ident, $w: expr ) => {
        fn $name(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
            // bulk_len is the minimum valid slice length among the source buffers
            let bulk_len = self.srcs.iter().map(|x| x.buf.len() - x.consumed).min().unwrap_or(0);
            let bulk_len = bulk_len & !($w - 1);

            if bulk_len == 0 {
                return Some(0);
            }

            // we always re-initialize the pointer cache (for safety of the loop below)
            for (src, ptr) in self.srcs.iter().zip(self.ptrs.iter_mut()) {
                *ptr = src.buf[src.consumed..].as_ptr();
            }

            buf.with_uninit(self.srcs.len() * bulk_len, |arr: &mut [u8]| {
                let mut dst = arr.as_mut_ptr();
                for _ in 0..bulk_len / $w {
                    for ptr in self.ptrs.iter_mut() {
                        unsafe { std::ptr::copy_nonoverlapping(*ptr, dst, $w) };
                        *ptr = ptr.wrapping_add($w);
                        dst = dst.wrapping_add($w);
                    }
                }

                let len = self.srcs.len() * bulk_len;
                Some((len, len))
            });

            for src in &mut self.srcs {
                src.consumed += bulk_len;
            }
            Some(self.srcs.len() * bulk_len)
        }
    };
}

impl ZipStream {
    fn new(srcs: Vec<Box<dyn ReadBlock>>, align: usize) -> ZipStream {
        assert!(srcs.len() > 0);
        assert!(align.is_power_of_two() && align <= 16);

        let gathers = [
            Self::gather_w1,
            Self::gather_w2,
            Self::gather_w4,
            Self::gather_w8,
            Self::gather_w16,
        ];
        let index = align.trailing_zeros() as usize;
        debug_assert!(index < 5);

        let len = srcs.len();
        ZipStream {
            srcs: srcs.into_iter().map(|x| ZipStreamCache::new(x)).collect(),
            ptrs: (0..len).map(|_| std::ptr::null::<u8>()).collect(),
            gather: gathers[index],
            align
        }
    }

    gather!(gather_w1, 1);
    gather!(gather_w2, 2);
    gather!(gather_w4, 4);
    gather!(gather_w8, 8);
    gather!(gather_w16, 16);
}

impl ReadBlock for ZipStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        let mut acc = 0;
        while buf.len() < BLOCK_SIZE {
            acc += (self.gather)(self, buf)?;

            let len = self.srcs.iter_mut().map(|src| src.fill_buf(self.align)).min();
            let len = len.unwrap_or(Some(0))?;
            if len == 0 {
                break;
            }
        }
        Some(acc)
    }
}

struct ClipStream {
    src: Box<dyn ReadBlock>,
    pad: usize,
    offset: isize,
    tail: isize,
}

impl ClipStream {
    fn new(src: Box<dyn ReadBlock>, pad: usize, skip: usize, len: usize) -> ClipStream {
        assert!(skip < isize::MAX as usize);
        assert!(len < isize::MAX as usize);

        ClipStream {
            src,
            pad,
            offset: -(skip as isize),
            tail: len as isize,
        }
    }
}

impl ReadBlock for ClipStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        if self.pad > 0 {
            let len = self.pad.min(BLOCK_SIZE - buf.len());
            buf.resize(buf.len() + len, 0);
            self.pad -= len;
            return Some(len);
        }

        if self.offset >= self.tail {
            return Some(0);
        }

        let base_len = buf.len();
        while buf.len() < BLOCK_SIZE {
            let len = self.src.read_block(buf)?;
            debug_assert!(len < isize::MAX as usize);

            if len == 0 {
                break;
            }

            self.offset += len as isize;
            if self.offset <= 0 {
                // still in the head skip. drop the current read
                buf.truncate(buf.len() - len);
                continue;
            }

            if self.offset < len as isize {
                let clipped_len = self.offset as usize;
                let tail = buf.len();
                let src = tail - clipped_len;
                let dst = tail - len;

                buf.copy_within(src..tail, dst);
                buf.truncate(dst + clipped_len);
            }

            if self.offset >= self.tail {
                let drop_len = (self.offset - self.tail) as usize;
                buf.truncate(buf.len() - drop_len);
                break;
            }
        }
        Some(buf.len() - base_len)
    }
}

trait DumpBlock {
    fn dump_block(&mut self) -> Option<usize>;
}

struct HexDrain {
    src: Box<dyn ReadBlock>,

    in_buf: Vec<u8>,
    consumed: usize,

    out_buf: Vec<u8>,
    offset: usize,
}

impl HexDrain {
    fn new(src: Box<dyn ReadBlock>, offset: usize) -> HexDrain {
        let mut out_buf = Vec::new();
        out_buf.resize(2 * 128 * BLOCK_SIZE, 0);

        HexDrain {
            src,
            in_buf: Vec::new(),
            consumed: 0,
            out_buf,
            offset,
        }
    }
}

impl DumpBlock for HexDrain {
    fn dump_block(&mut self) -> Option<usize> {
        let tail = self.in_buf.len();
        self.in_buf.copy_within(self.consumed..tail, 0);
        self.in_buf.truncate(tail - self.consumed);

        let mut is_eof = false;
        while self.in_buf.len() < BLOCK_SIZE {
            let len = self.src.read_block(&mut self.in_buf)?;
            if len == 0 {
                is_eof = true;
                break;
            }
        }

        if self.in_buf.len() == 0 {
            return Some(0);
        }

        let mut p = 0;
        let mut q = 0;
        for _ in 0..self.in_buf.len() / 16 {
            p += format_line(&mut self.out_buf[p..], &self.in_buf[q..], self.offset, 16);
            q += 16;
            self.offset += 16;
        }

        let len = self.in_buf.len();
        let rem = len & 15;
        if is_eof && rem > 0 {
            self.in_buf.resize(len + 16, 0);
            p += format_line(&mut self.out_buf[p..], &self.in_buf[q..], self.offset, 16);
            q += len & 15;

            patch_line(&mut self.out_buf[..p], len & 15, 16);
            self.in_buf.truncate(len);
        }
        self.consumed = q;

        std::io::stdout().write_all(&self.out_buf[..p]).unwrap();
        Some(q)
    }
}

fn create_source(name: &str) -> Box<dyn Read> {
    if name == "-" {
        return Box::new(std::io::stdin());
    }

    let path = std::path::Path::new(name);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(ret) => panic!("{:?}", ret),
    };
    Box::new(file)
}
