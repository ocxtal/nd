
# nd test script

## Input handling

Reads the input file(s) if any.

```console
$ nd test/hello.txt
```

If none, the default input is stdin.

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

```console
$ nd test/hello.txt
$ nd test/hello.txt > /dev/null
$ nd test/hello.txt > /dev/stdout
$ nd test/hello.txt > out.txt && cat out.txt && rm out.txt
```

```console
$ nd test/hello.txt -o -
$ nd test/hello.txt -o out.txt && cat out.txt && rm out.txt
$ ! (nd test/hello.txt -o out1.txt -o out2.txt 2>&1)
```

```console
$ nd -c1 test/quick.txt test/quick.txt
$ nd -c7 test/quick.txt test/quick.txt
$ nd -z1 test/quick.txt test/quick.txt
$ nd -z7 test/quick.txt test/quick.txt
$ nd --patch=<(echo "000000000010 0003 | 66 72 6f 67") test/quick.txt
$ nd --patch=<(echo "000000000010 0003 | 66 72 6f 67") test/quick.txt test/quick.txt
```
