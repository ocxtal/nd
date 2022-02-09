
# zd -- streamed binary editor

zd is a stream-oriented hex editor. The combination of four stages: stream construction, manipulation, slicing, and post-processing enables fast and flexible hex editing.

## Examples

### Hexdump: one or more inputs:

```
% zd quick.txt
000000000000 10 | 54 68 65 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 | The quick brown
000000000010 10 | 66 6f 78 20 6a 75 6d 70 73 20 6f 76 65 72 20 74 | fox jumps over t
000000000020 0d | 68 65 20 6c 61 7a 79 20 64 6f 67 2e 0a          | he lazy dog..
% zd quick.txt quick.txt
000000000000 10 | 54 68 65 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 | The quick brown
000000000010 10 | 66 6f 78 20 6a 75 6d 70 73 20 6f 76 65 72 20 74 | fox jumps over t
000000000020 10 | 68 65 20 6c 61 7a 79 20 64 6f 67 2e 0a 54 68 65 | he lazy dog..The
000000000030 10 | 20 71 75 69 63 6b 20 62 72 6f 77 6e 20 66 6f 78 |  quick brown fox
000000000040 10 | 20 6a 75 6d 70 73 20 6f 76 65 72 20 74 68 65 20 |  jumps over the
000000000050 0a | 6c 61 7a 79 20 64 6f 67 2e 0a                   | lazy dog..
```

## Copyright and License

2022, Hajime Suzuki. Licensed under MIT.
