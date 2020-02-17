# cargo no-std-check

[![CI](https://github.com/mystor/cargo-no-std-check/workflows/CI/badge.svg)](https://github.com/mystor/cargo-no-std-check/actions?query=workflow%3ACI+branch%3Amaster)
[![Latest Version](https://img.shields.io/crates/d/cargo-no-std-check.svg)](https://crates.io/crates/cargo-no-std-check)

cargo no-std-check is a wrapper for `cargo check`, which ensures that your
library does not link against `libstd`.

## Installation

`cargo no-std-check` can be built with any stable version of rust, but its
operation requires a nightly compiler.

```
$ cargo install cargo-no-std-check
```

## Usage

Run this command on a crate to build it's lib target without access to `std`.
Attempts to use `std` in the final library's dependency hierarchy will produce a
build error.

### Passing Example

```
$ cargo no-std-check --manifest-path nostd/Cargo.toml
    Creating #![no_std] sysroot
     Copying [============================================================] 154/154: done
     Sysroot x86_64-unknown-linux-gnu (/tmp/nostd_sysroot.YhFkabJ2tXeK)
    Finished dev [unoptimized + debuginfo] target(s) in 0.01s
```

### Failing Example

```
$ cargo no-std-check --manifest-path withstd/Cargo.toml
    Creating #![no_std] sysroot
     Copying [============================================================] 154/154: done
     Sysroot x86_64-unknown-linux-gnu (/tmp/nostd_sysroot.uYDnxo4ZNOLs)
    Checking withstd v0.1.0 (/crates/withstd)
error[E0463]: can't find crate for `std`

error: aborting due to previous error

For more information about this error, try `rustc --explain E0463`.
error: could not compile `withstd`.

To learn more, run the command again with --verbose.
```
