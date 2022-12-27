
# nd test script

## Version information

```console
$ nd --version
$ nd -V
$ ! (nd -version 2>&1)
```

## Help message

```console
$ nd --help
$ nd -h | head -5
$ nd -help | head -5  # -h -e "lp"
$ ! (nd -H 2>&1)
```

## Input handling

Reads the input file(s) if any.

```console
$ nd test/hello.txt
$ nd test/hello.txt test/world.txt
```

If none, the default is stdin.

```console
$ cat test/hello.txt | nd
```

`-` means stdin.

```console
$ cat test/hello.txt | nd -
$ cat test/hello.txt | nd /dev/stdin
```

Multiple stdins are not allowed.

```console
$ ! (cat test/hello.txt | nd - - 2>&1)
$ ! (cat test/hello.txt | nd - /dev/stdin 2>&1)
```

## Output handling

The default output is stdout.

```console
$ nd test/hello.txt
$ nd test/hello.txt > /dev/null
$ nd test/hello.txt > /dev/stdout
$ nd test/hello.txt > out.txt && cat out.txt && rm out.txt
```

`-o` specifies the output file.

```console
$ nd test/hello.txt -o -
$ nd test/hello.txt -o /dev/stdout
$ nd test/hello.txt -o out.txt && cat out.txt && rm out.txt
```

`-o` is a template.

```console
$ nd test/hello.txt --width 3 -o "out.{l}.txt" && ls out.*.txt && cat out.1.txt out.0.txt && rm out.*.txt
$ nd test/hello.txt --width 3 -o "out.{n}.txt" && ls out.*.txt && cat out.3.txt out.0.txt && rm out.*.txt
$ nd test/hello.txt --width 3 -o "out.{n:02x}.txt" && ls out.*.txt && cat out.03.txt out.00.txt && rm out.*.txt
$ nd test/hello.txt --width 3 -o "out.{(n+8):02x}.txt" && ls out.*.txt && cat out.0b.txt out.08.txt && rm out.*.txt
```

Multiple `-o` s are not allowed.

```console
$ ! (nd test/hello.txt -o out1.txt -o out2.txt 2>&1)
```

`--pager` feeds the output to another command.

```console
$ nd test/hello.txt --pager cat
$ nd test/hello.txt --pager "sed s/H/h/"
```

`--patch-back` feeds the output to another command and then applies its output to the original stream as a patch.

```console
$ nd --patch-back cat test/hello.txt
$ nd --patch-back "sed s/48/68/" test/hello.txt
```

## work-in-progress

```console
$ nd -c1 test/quick.txt test/quick.txt
$ nd -c7 test/quick.txt test/quick.txt
$ nd -z1 test/quick.txt test/quick.txt
$ nd -z7 test/quick.txt test/quick.txt
$ nd --patch=<(echo "000000000010 0003 | 66 72 6f 67") test/quick.txt
$ nd --patch=<(echo "000000000010 0003 | 66 72 6f 67") test/quick.txt test/quick.txt
```
