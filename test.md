
# nd test script

## Running this script

1. Install [exec-commands](https://github.com/ocxtal/exec-commands)
   * `cargo install exec-commands --git https://github.com/ocxtal/exec-commands`
2. Build nd
   * `cargo build`
3. Run exec-commands in the root directory of this repository.
   * `exec-commands --diff --ignore-default-config --path target/debug --pwd . test.md`
   * It overwrites this document when run without `--diff`.

## Version information

```console
$ nd --version
nd 0.0.1
$ nd -V
nd 0.0.1
$ ! (nd -version 2>&1)  # --invert "ersion"
error: parsing failed at a variable in "ersion"

Usage: nd [options] FILE ...

For more information try --help
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
        --pager PAGER       feed the stream to PAGER (ignored in the --inplace mode) [less -S -F -X]
$ nd -h | head -3
nd 0.0.1 -- streamed blob manipulator

USAGE:
$ nd -help | head -3  # --help --regex "lp"
nd 0.0.1 -- streamed blob manipulator

USAGE:
$ ! (nd -H 2>&1)
error: unexpected argument '-H' found

  tip: to pass '-H' as a value, use '-- -H'

Usage: nd [options] FILE ...

For more information, try '--help'.
```

Printed to stdout when requested.

```console
$ nd --help >/dev/null
$ nd --help 2>/dev/null | head -3
nd 0.0.1 -- streamed blob manipulator

USAGE:
```

Printed to stderr on error.

```console
$ ! (nd -H 2>&1 >/dev/null | head -1)
error: unexpected argument '-H' found
$ ! (nd -H 2>/dev/null)
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
$ ! (cat test/hello.txt | nd - -          2>&1)
error: stdin ("-" or "/dev/stdin") must not be used more than once

Usage: nd [options] FILE ...

For more information try --help
$ ! (cat test/hello.txt | nd - /dev/stdin 2>&1)
error: stdin ("-" or "/dev/stdin") must not be used more than once

Usage: nd [options] FILE ...

For more information try --help
$ ! (nd test/hello.txt  | nd --patch=-          - 2>&1)
error: stdin ("-" or "/dev/stdin") must not be used more than once

Usage: nd [options] FILE ...

For more information try --help
$ ! (nd test/hello.txt  | nd --patch=/dev/stdin - 2>&1)
error: stdin ("-" or "/dev/stdin") must not be used more than once

Usage: nd [options] FILE ...

For more information try --help
$ ! (nd test/hello.txt  | nd --guide=-          - 2>&1)
error: stdin ("-" or "/dev/stdin") must not be used more than once

Usage: nd [options] FILE ...

For more information try --help
$ ! (nd test/hello.txt  | nd --guide=/dev/stdin - 2>&1)
error: stdin ("-" or "/dev/stdin") must not be used more than once

Usage: nd [options] FILE ...

For more information try --help
$ ! (nd test/hello.txt  | nd --patch=- --guide=- test/world.txt 2>&1)
error: stdin ("-" or "/dev/stdin") must not be used more than once

Usage: nd [options] FILE ...

For more information try --help
```

## Output handling

The default output is stdout.

```console
$ nd test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt >/dev/null
$ nd test/hello.txt >/dev/stdout
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ (nd test/hello.txt >out.txt && cat out.txt); rm -f out.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
```

`--output` specifies the output file.

```console
$ nd test/hello.txt --output -
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt -o       -
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt --output /dev/stdout
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt -o       /dev/stdout
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ trap "rm -f out.txt" EXIT; \
  (nd test/hello.txt -o out.txt && cat out.txt)
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
```

`--output` is a template.

```console
$ function setup () { trap "rm -f out.*.txt" EXIT; }
$ function check () { ls out.*.txt && tail -n +1 out.*.txt; }
$ (setup; nd test/hello.txt -w3 -o "out.{l}.txt"         && check)
out.0.txt
out.1.txt
==> out.0.txt <==
000000000000 0003 | 48 65 6c | Hel

==> out.1.txt <==
000000000003 0003 | 6c 6f 0a | lo.
$ (setup; nd test/hello.txt -w3 -o "out.{n}.txt"         && check)
out.0.txt
out.3.txt
==> out.0.txt <==
000000000000 0003 | 48 65 6c | Hel

==> out.3.txt <==
000000000003 0003 | 6c 6f 0a | lo.
$ (setup; nd test/hello.txt -w3 -o "out.{n:02x}.txt"     && check)
out.00.txt
out.03.txt
==> out.00.txt <==
000000000000 0003 | 48 65 6c | Hel

==> out.03.txt <==
000000000003 0003 | 6c 6f 0a | lo.
$ (setup; nd test/hello.txt -w3 -o "out.{(n+8):02x}.txt" && check)
out.08.txt
out.0b.txt
==> out.08.txt <==
000000000000 0003 | 48 65 6c | Hel

==> out.0b.txt <==
000000000003 0003 | 6c 6f 0a | lo.
```

Multiple `--output` s are not allowed.

```console
$ ! (nd test/hello.txt -o out.1.txt -o out.2.txt 2>&1)
error: the argument '--output <FILE>' cannot be used multiple times

Usage: nd [options] FILE ...

For more information, try '--help'.
```

`--pager` feeds the output to another command.

```console
$ nd test/hello.txt --pager cat
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt --pager "sed s/H/h/"
000000000000 0006 | 48 65 6c 6c 6f 0a                               | hello.          
```

It recognizes `PAGER`.

```console
$ PAGER="sed s/H/h/" nd test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | hello.          
```

`--patch-back` feeds the output to another command and then applies its output to the original stream as a patch.

```console
$ nd --patch-back cat            test/hello.txt
Hello
$ nd -P           cat            test/hello.txt
Hello
$ nd --patch-back "sed s/48/68/" test/hello.txt
hello
$ nd -P           "sed s/48/68/" test/hello.txt
hello
```

`--inplace` overwrites the input file.

```console
$ function setup () { trap "rm -f tmp.*.txt" EXIT; }
$ function prep () { printf "%s\n" $* | xargs -I% -n1 cp test/hello.txt tmp.%.txt; }
$ function check () { ls tmp.*.txt && tail -n +1 tmp.*.txt; }
$ (setup; prep 1     && nd -P "sed s/6c/4c/" --inplace tmp.1.txt && check)
tmp.1.txt
HeLlo
$ (setup; prep 1     && nd -P "sed s/6c/4c/" -i        tmp.1.txt && check)
tmp.1.txt
HeLlo
```

(cont'd) Applied for each file.

```console continued
$ (setup; prep 1 2 3 && nd -P "sed s/6c/4c/" --inplace tmp.1.txt tmp.2.txt tmp.3.txt && check)
tmp.1.txt
tmp.2.txt
tmp.3.txt
==> tmp.1.txt <==
HeLlo

==> tmp.2.txt <==
HeLlo

==> tmp.3.txt <==
HeLlo
```

(cont'd) The file list is deduped when `--inplace`.

```console continued
$ (setup; prep 1     && nd -P "sed s/6c/4c/" --inplace tmp.1.txt tmp.1.txt tmp.1.txt && check)
tmp.1.txt
HeLlo
```

## Output format

Raw and hex are supported. Hex without offset/lengths is todo.

```console
$ nd --out-format xxx test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd -f xxx           test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd --out-format x   test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd -f x             test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd --out-format nnx test/hello.txt  # FIXME
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd -f nnx           test/hello.txt  # FIXME
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd --out-format nnb test/hello.txt
Hello
$ nd -f nnb           test/hello.txt
Hello
$ nd --out-format b   test/hello.txt
Hello
$ nd -f b             test/hello.txt
Hello
$ ! (nd --out-format xx   test/hello.txt 2>&1)
error: invalid value 'xx' for '--out-format <FORMAT>': unrecognized input / output format signature: "xx"

For more information, try '--help'.
$ ! (nd --out-format xxxx test/hello.txt 2>&1 | head -1)
error: invalid value 'xxxx' for '--out-format <FORMAT>': unrecognized input / output format signature: "xxxx"
$ ! (nd --out-format nxx  test/hello.txt 2>&1 | head -1)
error: invalid value 'nxx' for '--out-format <FORMAT>': unrecognized input / output format signature: "nxx"
$ ! (nd --out-format bbb  test/hello.txt 2>&1 | head -1)
error: invalid value 'bbb' for '--out-format <FORMAT>': unrecognized input / output format signature: "bbb"
```

## Input format

Raw and hex with/without offset/lengths are supported.

```console
$ nd -a6 -w6 test/hello.txt | tail -1 | nd --in-format xxx
000000000000 000c | 00 00 00 00 00 00 48 65 6c 6c 6f 0a             | ......Hello.    
$ nd -a6 -w6 test/hello.txt | tail -1 | nd -F xxx
000000000000 000c | 00 00 00 00 00 00 48 65 6c 6c 6f 0a             | ......Hello.    
$ nd -a6 -w6 test/hello.txt | tail -1 | nd --in-format x
000000000000 000c | 00 00 00 00 00 00 48 65 6c 6c 6f 0a             | ......Hello.    
$ nd -a6 -w6 test/hello.txt | tail -1 | nd -F x
000000000000 000c | 00 00 00 00 00 00 48 65 6c 6c 6f 0a             | ......Hello.    
$ nd -a6 -w6 test/hello.txt | tail -1 | nd --in-format nnx
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd -a6 -w6 test/hello.txt | tail -1 | nd -F nnx
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd test/hello.txt | nd --in-format b   | head -2
000000000000 0010 | 30 30 30 30 30 30 30 30 30 30 30 30 20 30 30 30 | 000000000000 000
000000000010 0010 | 36 20 7c 20 34 38 20 36 35 20 36 63 20 36 63 20 | 6 | 48 65 6c 6c 
$ nd test/hello.txt | nd -F b            | head -2
000000000000 0010 | 30 30 30 30 30 30 30 30 30 30 30 30 20 30 30 30 | 000000000000 000
000000000010 0010 | 36 20 7c 20 34 38 20 36 35 20 36 63 20 36 63 20 | 6 | 48 65 6c 6c 
$ nd test/hello.txt | nd --in-format nnb | head -2
000000000000 0010 | 30 30 30 30 30 30 30 30 30 30 30 30 20 30 30 30 | 000000000000 000
000000000010 0010 | 36 20 7c 20 34 38 20 36 35 20 36 63 20 36 63 20 | 6 | 48 65 6c 6c 
$ nd test/hello.txt | nd -F nnb          | head -2
000000000000 0010 | 30 30 30 30 30 30 30 30 30 30 30 30 20 30 30 30 | 000000000000 000
000000000010 0010 | 36 20 7c 20 34 38 20 36 35 20 36 63 20 36 63 20 | 6 | 48 65 6c 6c 
$ ! (nd test/hello.txt | nd -F xx   2>&1)
error: invalid value 'xx' for '--in-format <FORMAT>': unrecognized input / output format signature: "xx"

For more information, try '--help'.
$ ! (nd test/hello.txt | nd -F xxxx 2>&1 | head -1)
error: invalid value 'xxxx' for '--in-format <FORMAT>': unrecognized input / output format signature: "xxxx"
$ ! (nd test/hello.txt | nd -F nxx  2>&1 | head -1)
error: invalid value 'nxx' for '--in-format <FORMAT>': unrecognized input / output format signature: "nxx"
$ ! (nd test/hello.txt | nd -F bbb  2>&1 | head -1)
error: invalid value 'bbb' for '--in-format <FORMAT>': unrecognized input / output format signature: "bbb"
```

TODO: test input parser.

## Filler

`--filler` overwrites padding value.

```console
$ nd -a2,2                            test/hello.txt
000000000000 000a | 00 00 48 65 6c 6c 6f 0a 00 00                   | ..Hello...      
$ nd -a2,2 --filler 0                 test/hello.txt
000000000000 000a | 00 00 48 65 6c 6c 6f 0a 00 00                   | ..Hello...      
$ nd -a2,2 --filler 128               test/hello.txt
000000000000 000a | 80 80 48 65 6c 6c 6f 0a 80 80                   | ..Hello...      
$ nd -a2,2 --filler 0xff              test/hello.txt
000000000000 000a | ff ff 48 65 6c 6c 6f 0a ff ff                   | ..Hello...      
$ nd -a2,2 --filler "0xffff - 0xff00" test/hello.txt
000000000000 000a | ff ff 48 65 6c 6c 6f 0a ff ff                   | ..Hello...      
$ nd -a2,2 --filler "0xff & -1"       test/hello.txt
000000000000 000a | ff ff 48 65 6c 6c 6f 0a ff ff                   | ..Hello...      
$ ! (nd -a2,2 --filler 256     test/hello.txt 2>&1)
error: filler must be within [0, 256) (got: 256)

Usage: nd [options] FILE ...

For more information try --help
$ ! (nd -a2,2 --filler "0 - 1" test/hello.txt 2>&1)
error: invalid value '0 - 1' for '--filler <N>': negative value is not allowed for this option ("0 - 1" gave -1).

For more information, try '--help'.
```

It applies to all operations that pad.

```console
$ nd --cat 4   --filler 128 test/hello.txt test/world.txt
000000000000 0010 | 48 65 6c 6c 6f 0a 80 80 77 6f 72 6c 64 0a 80 80 | Hello...world...
$ nd --zip 4   --filler 128 test/hello.txt test/world.txt
000000000000 0010 | 48 65 6c 6c 77 6f 72 6c 6f 0a 80 80 64 0a 80 80 | Hellworlo...d...
$ nd --pad 4   --filler 128 test/hello.txt
000000000000 000a | 80 80 80 80 48 65 6c 6c 6f 0a                   | ....Hello.      
$ nd --pad 0,4 --filler 128 test/hello.txt
000000000000 000a | 48 65 6c 6c 6f 0a 80 80 80 80                   | Hello.....      
$ nd -a6 -w6 test/hello.txt | tail -1 | nd --in-format x --filler 128
000000000000 000c | 80 80 80 80 80 80 48 65 6c 6c 6f 0a             | ......Hello.    
```

## Input stream multiplexing

The default is `--cat 1`.

```console
$ nd test/hello.txt test/world.txt
000000000000 000c | 48 65 6c 6c 6f 0a 77 6f 72 6c 64 0a             | Hello.world.    
$ cat test/world.txt | nd test/hello.txt -
000000000000 000c | 48 65 6c 6c 6f 0a 77 6f 72 6c 64 0a             | Hello.world.    
$ cat test/hello.txt | nd - test/world.txt
000000000000 000c | 48 65 6c 6c 6f 0a 77 6f 72 6c 64 0a             | Hello.world.    
```

An arbitrary positive value is allowed.

```console
$ nd --cat 5           test/hello.txt test/world.txt
000000000000 0010 | 48 65 6c 6c 6f 0a 00 00 00 00 77 6f 72 6c 64 0a | Hello.....world.
000000000010 0004 | 00 00 00 00                                     | ....            
$ nd -c 5              test/hello.txt test/world.txt
000000000000 0010 | 48 65 6c 6c 6f 0a 00 00 00 00 77 6f 72 6c 64 0a | Hello.....world.
000000000010 0004 | 00 00 00 00                                     | ....            
$ nd --cat "65536 + 1" test/hello.txt test/world.txt 2>&1 >/dev/null
$ nd -c "65536 + 1"    test/hello.txt test/world.txt 2>&1 >/dev/null
$ cat test/hello.txt | nd --cat 4 - test/world.txt
000000000000 0010 | 48 65 6c 6c 6f 0a 00 00 77 6f 72 6c 64 0a 00 00 | Hello...world...
$ ! (nd --cat "0 - 1" test/hello.txt test/world.txt 2>&1)
error: invalid value '0 - 1' for '--cat <N>': negative value is not allowed for this option ("0 - 1" gave -1).

For more information, try '--help'.
```

`--zip` as well.

```console
$ nd --zip 5           test/hello.txt test/world.txt
000000000000 0010 | 48 65 6c 6c 6f 77 6f 72 6c 64 0a 00 00 00 00 0a | Helloworld......
000000000010 0004 | 00 00 00 00                                     | ....            
$ nd -z 5              test/hello.txt test/world.txt
000000000000 0010 | 48 65 6c 6c 6f 77 6f 72 6c 64 0a 00 00 00 00 0a | Helloworld......
000000000010 0004 | 00 00 00 00                                     | ....            
$ nd --zip "65536 + 1" test/hello.txt test/world.txt 2>&1 >/dev/null
$ nd -z    "65536 + 1" test/hello.txt test/world.txt 2>&1 >/dev/null
$ cat test/hello.txt | nd --zip 4 - test/world.txt
000000000000 0010 | 48 65 6c 6c 77 6f 72 6c 6f 0a 00 00 64 0a 00 00 | Hellworlo...d...
$ ! (nd --zip "0 - 1" test/hello.txt test/world.txt 2>&1)
error: invalid value '0 - 1' for '--zip <N>': negative value is not allowed for this option ("0 - 1" gave -1).

For more information, try '--help'.
```

## Seek and pad

`--cut` slices and concatenates the stream.

```console
$ nd --cut 1..2      test/hello.txt
000000000000 0001 | 65                                              | e               
$ nd --cut 1..2,4..5 test/hello.txt
000000000000 0002 | 65 6f                                           | eo              
$ nd --cut  ..2,4..  test/hello.txt
000000000000 0004 | 48 65 6f 0a                                     | Heo.            
$ nd --cut  ..       test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd -n 1..2         test/hello.txt
000000000000 0001 | 65                                              | e               
$ nd -n 1..2,4..5    test/hello.txt
000000000000 0002 | 65 6f                                           | eo              
$ nd -n  ..2,4..     test/hello.txt
000000000000 0004 | 48 65 6f 0a                                     | Heo.            
$ nd -n  ..          test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ nd --cut 2..2      test/hello.txt
```

Empty ranges and negative indices are allowed for now (may be changed in the future).

```console
$ nd --cut   2..1 test/hello.txt
$ nd --cut 0-1..1 test/hello.txt
000000000000 0001 | 48                                              | H               
```

Trailing comma allowed.

```console
$ nd --cut 1..2, test/hello.txt
000000000000 0001 | 65                                              | e               
```

`--pad` adds filler bytes.

```console
$ nd --pad 2,2 test/hello.txt
000000000000 000a | 00 00 48 65 6c 6c 6f 0a 00 00                   | ..Hello...      
$ nd --pad 2   test/hello.txt
000000000000 0008 | 00 00 48 65 6c 6c 6f 0a                         | ..Hello.        
$ nd --pad 2,  test/hello.txt
000000000000 0008 | 00 00 48 65 6c 6c 6f 0a                         | ..Hello.        
$ nd --pad  ,2 test/hello.txt
000000000000 0008 | 48 65 6c 6c 6f 0a 00 00                         | Hello...        
$ nd -a 2,2    test/hello.txt
000000000000 000a | 00 00 48 65 6c 6c 6f 0a 00 00                   | ..Hello...      
$ nd -a 2      test/hello.txt
000000000000 0008 | 00 00 48 65 6c 6c 6f 0a                         | ..Hello.        
$ nd -a 2,     test/hello.txt
000000000000 0008 | 00 00 48 65 6c 6c 6f 0a                         | ..Hello.        
$ nd -a  ,2    test/hello.txt
000000000000 0008 | 48 65 6c 6c 6f 0a 00 00                         | Hello...        
$ ! (nd --pad=-2   test/hello.txt 2>&1)
error: invalid value '-2' for '--pad <N,M>': negative values are not allowed for this option ("-2" gave -2 and 0).

For more information, try '--help'.
$ ! (nd --pad 2,2, test/hello.txt 2>&1)
error: invalid value '2,2,' for '--pad <N,M>': "N,M" format expected for this option.

For more information, try '--help'.
$ ! (nd --pad ,,   test/hello.txt 2>&1 | head -1)
error: invalid value ',,' for '--pad <N,M>': "N,M" format expected for this option.
$ ! (nd --pad xx   test/hello.txt 2>&1 | head -1)
error: invalid value 'xx' for '--pad <N,M>': failed to parse "xx" at "xx": parsing failed at a variable in "xx"
```

`--pad` is applied after `--cut`.

```console
$ nd --pad 2,2 --cut 1..2,4..5 test/hello.txt
000000000000 0006 | 00 00 65 6f 00 00                               | ..eo..          
$ nd --cut 1..2,4..5 --pad 2,2 test/hello.txt
000000000000 0006 | 00 00 65 6f 00 00                               | ..eo..          
```

## Patch

Substitution.

```console
$ echo "02 02 | 68" | nd --patch -          test/hello.txt
000000000000 0005 | 48 65 68 6f 0a                                  | Heho.           
$ echo "02 02 | 68" | nd --patch /dev/stdin test/hello.txt
000000000000 0005 | 48 65 68 6f 0a                                  | Heho.           
$ echo "02 04 | 68" | nd --patch -          test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68" | nd --patch -          test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
```

Spaces and trailing comments are ignored.

```console
$ echo "02 05 | 68 "        | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68 |"       | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68 | "      | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68 |abcde"  | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68 | abcde" | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68  |"      | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68   |"     | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68    |"    | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ echo "02 05 | 68     |"   | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
```

The last line doesn't have to have the trailing newline.

```console
$ printf "02 05 | 68"             | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ printf "02 01 | 68\n03 01 | 68" | nd --patch - test/hello.txt
000000000000 0006 | 48 65 68 68 6f 0a                               | Hehho.          
```

Offset and length fields can be as long as 15 digits.

```console
$    echo "2 5 | 68"                | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$    echo "00000000000002 5 | 68"   | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$    echo "000000000000002 5 | 68"  | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ ! (echo "0000000000000002 5 | 68" | nd --patch - test/hello.txt 2>&1)
thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: failed to parse the header at record "0000000000000002 5 | 68"', src/byte/patch.rs:75:26
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
$    echo "2 00000000000005 | 68"   | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$    echo "2 000000000000005 | 68"  | nd --patch - test/hello.txt
000000000000 0003 | 48 65 68                                        | Heh             
$ ! (echo "2 0000000000000005 | 68" | nd --patch - test/hello.txt 2>&1)
thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: failed to parse the header at record "2 0000000000000005 | 68"', src/byte/patch.rs:75:26
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

Insertion.

```console
$ echo "00 00 | 6c 6c" | nd --patch - test/hello.txt
000000000000 0008 | 6c 6c 48 65 6c 6c 6f 0a                         | llHello.        
$ echo "02 00 | 6c 6c" | nd --patch - test/hello.txt
000000000000 0008 | 48 65 6c 6c 6c 6c 6f 0a                         | Hellllo.        
$ echo "06 00 | 6c 6c" | nd --patch - test/hello.txt  # FIXME
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
$ echo "07 00 | 6c 6c" | nd --patch - test/hello.txt
000000000000 0006 | 48 65 6c 6c 6f 0a                               | Hello.          
```

Deletion.

```console
$ echo "02 02 | " | nd --patch - test/hello.txt
000000000000 0004 | 48 65 6f 0a                                     | Heo.            
$ echo "02 04 | " | nd --patch - test/hello.txt
000000000000 0002 | 48 65                                           | He              
$ echo "02 05 | " | nd --patch - test/hello.txt
000000000000 0002 | 48 65                                           | He              
```

The array can be omitted but any partial delimiter is not allowed.

```console
$    echo "02 02"     | nd --patch - test/hello.txt
000000000000 0004 | 48 65 6f 0a                                     | Heo.            
$ ! (echo "02 02 "    | nd --patch - test/hello.txt)
$ ! (echo "02 02 |"   | nd --patch - test/hello.txt)
$ ! (echo "02 02 |  " | nd --patch - test/hello.txt)
000000000000 0004 | 48 65 6f 0a                                     | Heo.            
```

