
# nd test script

## Version information

```console
$ nd --version
nd 0.0.1
$ nd -V
nd 0.0.1
$ ! (nd -version 2>&1)
Error: parsing failed at a variable in "ersion"
```

## Help message

```console
$ nd --help

nd 0.0.1 -- streamed blob manipulator

USAGE:

  nd [options] FILE ...

OPTIONS:

  Input and output formats

    -F, --in-format FMT     input format signature (applies to all inputs) [b]
    -f, --out-format FMT    output format signature (applies to --output) [x]

  Constructing input stream (exclusive)

    -c, --cat N             concat all input streams into one with N-byte alignment (default) [1]
    -z, --zip N             zip all input streams into one with N-byte words
    -i, --inplace           edit each input file in-place

  Manipulating the stream (applied in this order)

    -n, --cut S..E[,...]    leave only bytes within the S..E range(s)
    -a, --pad N,M           add N and M bytes of zeros at the head and tail
    -p, --patch FILE        patch the input stream with the patchfile

  Slicing the stream (exclusive)

    -w, --width N[,S..E]    slice into N bytes and map them to S..E (default) [16,s..e]
    -d, --find ARRAY        slice out every ARRAY location
    -k, --walk EXPR[,...]   split the stream into eval(EXPR)-byte chunk(s), repeat it until the end
    -r, --slice S..E[,...]  slice out S..E range(s)
    -g, --guide FILE        slice out [offset, offset + length) ranges loaded from the file

  Manipulating the slices (applied in this order)

    -e, --regex PCRE        match PCRE on every slice and leave the match locations
    -v, --invert S..E[,...] invert slices and map them to S..E range(s)
    -x, --extend S..E[,...] map every slice to S..E range(s)
    -m, --merge N           iteratively merge slices where distance <= N
    -l, --lines S..E[,...]  leave only slices (lines) in the S..E range(s)

  Post-processing the slices (exclusive)

    -o, --output TEMPLATE   render filename from TEMPLATE for each slice, and dump formatted slices to the files
                            ("-" for stdout; default) [-]
    -P, --patch-back CMD    pipe formatted slices to CMD, then feed its output onto the cached stream as patches

  Miscellaneous

    -h, --help              print help (this) message
    -V, --version           print version information
        --filler N          use N (0 <= N < 256) for padding
        --pager PAGER       feed the stream to PAGER (ignored in the --inplace mode) [less -S -F]

$ nd -h | head -5

nd 0.0.1 -- streamed blob manipulator

USAGE:

$ nd -help | head -5  # -h -e "lp"

nd 0.0.1 -- streamed blob manipulator

USAGE:

$ ! (nd -H 2>&1)
error: Found argument '-H' which wasn't expected, or isn't valid in this context

	If you tried to supply `-H` as a value rather than a flag, use `-- -H`

USAGE:
    nd [options] FILE ...

For more information try --help
```

## Input handling

Reads the input file(s) if any.

```console
$ nd test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt test/world.txt
000000000000 000c | 48 65 6c 6c 6f 0a 77 6f 72 6c 64 0a             | Hello.world.    
```

If none, the default is stdin.

```console
$ cat test/hello.txt | nd
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
```

`-` means stdin.

```console
$ cat test/hello.txt | nd -
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ cat test/hello.txt | nd /dev/stdin
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
```

Multiple stdins are not allowed.

```console
$ ! (cat test/hello.txt | nd - - 2>&1)
Error: "-" (stdin) must not appear more than once in the input files.
$ ! (cat test/hello.txt | nd - /dev/stdin 2>&1)
Error: "-" (stdin) must not appear more than once in the input files.
```

## Output handling

The default output is stdout.

```console
$ nd test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt > /dev/null
$ nd test/hello.txt > /dev/stdout
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt > out.txt && cat out.txt && rm out.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
```

`-o` specifies the output file.

```console
$ nd test/hello.txt -o -
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt -o /dev/stdout
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt -o out.txt && cat out.txt && rm out.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
```

`-o` is a template.

```console
$ nd test/hello.txt --width 3 -o "out.{l}.txt" && ls out.*.txt && cat out.1.txt out.0.txt && rm out.*.txt
out.0.txt
out.1.txt
000000000003 0003 | 6c 6f 0a | lo.
000000000000 0003 | 48 65 6c | Hel
$ nd test/hello.txt --width 3 -o "out.{n}.txt" && ls out.*.txt && cat out.3.txt out.0.txt && rm out.*.txt
out.0.txt
out.3.txt
000000000003 0003 | 6c 6f 0a | lo.
000000000000 0003 | 48 65 6c | Hel
$ nd test/hello.txt --width 3 -o "out.{n:02x}.txt" && ls out.*.txt && cat out.03.txt out.00.txt && rm out.*.txt
out.00.txt
out.03.txt
000000000003 0003 | 6c 6f 0a | lo.
000000000000 0003 | 48 65 6c | Hel
$ nd test/hello.txt --width 3 -o "out.{(n+8):02x}.txt" && ls out.*.txt && cat out.0b.txt out.08.txt && rm out.*.txt
out.08.txt
out.0b.txt
000000000003 0003 | 6c 6f 0a | lo.
000000000000 0003 | 48 65 6c | Hel
```

Multiple `-o` s are not allowed.

```console
$ ! (nd test/hello.txt -o out1.txt -o out2.txt 2>&1)
error: The argument '--output <FILE>' was provided more than once, but cannot be used multiple times

USAGE:
    nd [options] FILE ...

For more information try --help
```

`--pager` feeds the output to another command.

```console
$ nd test/hello.txt --pager cat
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt --pager "sed s/H/h/"
000000000000 0006 | 48 65 6c 6c 6f 0a                               | hello.          
```

`--patch-back` feeds the output to another command and then applies its output to the original stream as a patch.

```console
$ nd --patch-back cat test/hello.txt
Hello
$ nd --patch-back "sed s/48/68/" test/hello.txt
hello
```

## work-in-progress

```console
$ nd -c1 test/quick.txt test/quick.txt
000000000000 0010 | 54 68 65 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 | The quick brown 
000000000010 0010 | 66 6f 78 20 6a 75 6d 70 73 20 6f 76 65 72 20 74 | fox jumps over t
000000000020 0010 | 68 65 20 6c 61 7a 79 20 64 6f 67 2e 0a 54 68 65 | he lazy dog..The
000000000030 0010 | 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 66 6f 78 |  quick brown fox
000000000040 0010 | 20 6a 75 6d 70 73 20 6f 76 65 72 20 74 68 65 20 |  jumps over the 
000000000050 000a | 6c 61 7a 79 20 64 6f 67 2e 0a                   | lazy dog..      
$ nd -c7 test/quick.txt test/quick.txt
000000000000 0010 | 54 68 65 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 | The quick brown 
000000000010 0010 | 66 6f 78 20 6a 75 6d 70 73 20 6f 76 65 72 20 74 | fox jumps over t
000000000020 0010 | 68 65 20 6c 61 7a 79 20 64 6f 67 2e 0a 00 00 00 | he lazy dog.....
000000000030 0010 | 00 54 68 65 20 71 75 69 63 6b 20 62 72 6f 77 6e | .The quick brown
000000000040 0010 | 20 66 6f 78 20 6a 75 6d 70 73 20 6f 76 65 72 20 |  fox jumps over 
000000000050 0010 | 74 68 65 20 6c 61 7a 79 20 64 6f 67 2e 0a 00 00 | the lazy dog....
000000000060 0002 | 00 00                                           | ..              
$ nd -z1 test/quick.txt test/quick.txt
000000000000 0010 | 54 54 68 68 65 65 20 20 71 71 75 75 69 69 63 63 | TThhee  qquuiicc
000000000010 0010 | 6b 6b 20 20 62 62 72 72 6f 6f 77 77 6e 6e 20 20 | kk  bbrroowwnn  
000000000020 0010 | 66 66 6f 6f 78 78 20 20 6a 6a 75 75 6d 6d 70 70 | ffooxx  jjuummpp
000000000030 0010 | 73 73 20 20 6f 6f 76 76 65 65 72 72 20 20 74 74 | ss  oovveerr  tt
000000000040 0010 | 68 68 65 65 20 20 6c 6c 61 61 7a 7a 79 79 20 20 | hhee  llaazzyy  
000000000050 000a | 64 64 6f 6f 67 67 2e 2e 0a 0a                   | ddoogg....      
$ nd -z7 test/quick.txt test/quick.txt
000000000000 0010 | 54 68 65 20 71 75 69 54 68 65 20 71 75 69 63 6b | The quiThe quick
000000000010 0010 | 20 62 72 6f 77 63 6b 20 62 72 6f 77 6e 20 66 6f |  browck brown fo
000000000020 0010 | 78 20 6a 6e 20 66 6f 78 20 6a 75 6d 70 73 20 6f | x jn fox jumps o
000000000030 0010 | 76 75 6d 70 73 20 6f 76 65 72 20 74 68 65 20 65 | vumps over the e
000000000040 0010 | 72 20 74 68 65 20 6c 61 7a 79 20 64 6f 6c 61 7a | r the lazy dolaz
000000000050 0010 | 79 20 64 6f 67 2e 0a 00 00 00 00 67 2e 0a 00 00 | y dog......g....
000000000060 0002 | 00 00                                           | ..              
$ nd --patch=<(echo "000000000010 0003 | 66 72 6f 67") test/quick.txt
000000000000 0010 | 54 68 65 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 | The quick brown 
000000000010 0010 | 66 72 6f 67 20 6a 75 6d 70 73 20 6f 76 65 72 20 | frog jumps over 
000000000020 000e | 74 68 65 20 6c 61 7a 79 20 64 6f 67 2e 0a       | the lazy dog..  
$ nd --patch=<(echo "000000000010 0003 | 66 72 6f 67") test/quick.txt test/quick.txt
000000000000 0010 | 54 68 65 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 | The quick brown 
000000000010 0010 | 66 72 6f 67 20 6a 75 6d 70 73 20 6f 76 65 72 20 | frog jumps over 
000000000020 0010 | 74 68 65 20 6c 61 7a 79 20 64 6f 67 2e 0a 54 68 | the lazy dog..Th
000000000030 0010 | 65 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 66 6f | e quick brown fo
000000000040 0010 | 78 20 6a 75 6d 70 73 20 6f 76 65 72 20 74 68 65 | x jumps over the
000000000050 000b | 20 6c 61 7a 79 20 64 6f 67 2e 0a                |  lazy dog..     
```
