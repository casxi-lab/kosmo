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
texts. They read the file with `include_str!`, so the tests need no package
other than kalyx.

The tests check, by term, that each `ok` answer of the copy that fits one
insertion is what the kalyx version in `framework/Cargo.lock` computes for
its input, but not that the copy is byte for byte that version's file, nor
its error cases. A change that moves kosmo to another kalyx version replaces
the copy with the file of that version's package and updates the SHA-256
here.
