# Test data

`r02.tsv` is a byte-for-byte copy of the answer vectors of kalyx 0.1.0, the
version of kalyx that this crate depends on:

| | |
| --- | --- |
| Source | the package `kalyx` 0.1.0 on crates.io, the file `vectors/r02.tsv` in it |
| SHA-256 | `102053fe9f632f8da7f20d9fa1467a425d011e96a8a11de572737b8cd6fb8102` |
| License | MIT, Copyright (c) 2026 The casξ Authors, the same as kosmo's |

kalyx's `vectors/README.md` describes the format: seven tab-separated
columns, `id`, `operation`, `variable`, `input`, `result`, `answer` and
`limits`, with a header row. The read-back tests in `tests/read_back.rs` take
the answers of the cases whose result is `ok`, read them back into lines,
convert those lines and compare the terms that kalyx 0.1.0 reads from both
texts. The display tests in `tests/displayed_values.rs` compute the value
lines of the same cases and compare the terms of their display texts with
the answers. Both read the file through `tests/support/mod.rs` with
`include_str!`, so the tests need no package other than kalyx.

The tests check, by term, that each `ok` answer of the copy that fits one
insertion is what the kalyx version in `framework/Cargo.lock` computes for
its input, but not that the copy is byte for byte that version's file, nor
its error cases. A change that moves kosmo to another kalyx version replaces
the copy with the file of that version's package and updates the SHA-256
here.

`long-value.txt` is the expected rendering of the long value in
`tests/bounded_display.rs`, followed by a line feed. Under the raised limits
of that test, kalyx 0.1.0 computes `expand x (x+1)^600` and writes a value
line of 82,971 bytes. Its display text of 82,958 bytes is longer than the
bound, so the rendering as the current result keeps the first 65,536 bytes
and ends in ` ... (82958 bytes)`, 65,584 bytes in all. The file was written
from that line by a script outside the repository, not by kosmo's code;
without its final line feed, its SHA-256 is
`cfb287d1b4c5e6e0c5ad47510c1ed30f141d445c43c0f3d730edadba12debe45`. The test
compares the rendering with the whole file. A change that moves kosmo to
another kalyx version, or changes the bound or how a cut text ends, rewrites
the file.
