# cargo no-std-check
![CI](https://github.com/mystor/cargo-no-std-check/workflows/CI/badge.svg)

cargo no-std-check is a wrapper for `cargo check`, which ensures that your
library does not link against `libstd`.

## Installation

`cargo no-std-check` can be built with any stable version of rust, but its
operation requires a nightly compiler.

```
$ cargo install no-std-check
```

