use anyhow::{bail, ensure, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rustc_version::Channel;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::str;
use tempdir::TempDir;
use walkdir::WalkDir;

fn get_sysroot() -> Result<PathBuf> {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(rustc)
        .args(&["--print", "sysroot"])
        .stdout(Stdio::piped())
        .spawn()?
        .wait_with_output()?;
    ensure!(output.status.success(), "failed to get sysroot");

    let stdout = str::from_utf8(&output.stdout)?.trim_end();
    Ok(stdout.into())
}

fn cargo_bar_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template("{prefix:>12.bold.cyan} [{bar:60}] {pos}/{len}: {msg}")
        .progress_chars("=> ")
}

fn build_sysroot(target: &str, src_sysroot: &Path, dst_sysroot: &Path) -> Result<()> {
    eprintln!(
        "{:>12} #![no_std] sysroot",
        console::style("Creating").bold().green()
    );

    // Root Paths.
    let src_root = src_sysroot.join("lib/rustlib").join(target);
    let dst_root = dst_sysroot.join("lib/rustlib").join(target);

    // List of source/dst entries to copy.
    let mut to_copy = <Vec<(PathBuf, PathBuf)>>::new();

    // Copy over `bin` entries.
    let src_bin = src_root.join("bin");
    let dst_bin = dst_root.join("bin");
    for entry in WalkDir::new(&src_bin) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let suffix = entry.path().strip_prefix(&src_bin).unwrap();
            to_copy.push((entry.path().to_owned(), dst_bin.join(suffix)));
        }
    }

    // Copy over `lib` entries.
    let src_lib = src_root.join("lib");
    let dst_lib = dst_root.join("lib");
    for entry in WalkDir::new(&src_lib) {
        let entry = entry?;
        if entry.file_type().is_file() {
            // Copy everything but `libstd` to `dst_lib`.
            let lib_name = entry
                .file_name()
                .to_str()
                .unwrap_or("")
                .split(|c| c == '-' || c == '.')
                .next()
                .unwrap();

            // XXX: Support blocking other libs?
            if lib_name != "libstd" {
                let suffix = entry.path().strip_prefix(&src_lib).unwrap();
                to_copy.push((entry.path().to_owned(), dst_lib.join(suffix)));
            }
        }
    }

    // Perform the copies.
    let pb = ProgressBar::new(to_copy.len() as u64);
    pb.set_style(cargo_bar_style());
    pb.set_prefix("Copying");
    for (from, to) in &to_copy {
        let suffix = to.strip_prefix(&dst_root).unwrap().to_str().unwrap();
        pb.set_message(suffix);
        fs::create_dir_all(to.parent().unwrap())?;
        fs::copy(from, to)?;
        pb.inc(1);
    }
    pb.finish_with_message("done");

    Ok(())
}

fn get_flag(args: &[String], name: &str) -> bool {
    args.iter().position(|arg| arg == name).is_some()
}

fn get_arg<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            return iter.next().map(|x| &x[..]);
        }
        if arg.starts_with(name) && arg[name.len()..].starts_with('=') {
            return Some(&arg[name.len() + 1..]);
        }
    }
    None
}

fn cargo_command(mut args: Vec<String>) -> Result<i32> {
    // Cargo passes the subcommand name as the first argument, remove it if found.
    match args.first() {
        Some(first) if first == "no-std-check" => {
            args.remove(0);
        }
        _ => {}
    }

    // --help
    if get_flag(&args, "-h") || get_flag(&args, "--help") {
        println!(
            "\
Wrapper for `cargo check` that prevents linking against libstd.

USAGE:
    cargo no-std-check [OPTIONS]

OPTIONS:
    -h, --help          Prints help information and exit
    --version           Prints version information and exit

    Any additional options are directly passed to `cargo check` (see
    `cargo check --help` for possible options).

TARGETS:
    `cargo no-std-check` only checks the package's library target, and
    will not attempt to build any tests, examples, or binaries.
"
        );
        return Ok(0);
    }

    // --version
    if get_flag(&args, "--version") {
        println!(concat!("cargo-no-std-check ", env!("CARGO_PKG_VERSION")));
        return Ok(0);
    }

    let rustc_meta = rustc_version::version_meta()?;
    match rustc_meta.channel {
        Channel::Nightly | Channel::Dev => {}
        channel => bail!("{:?} channel not supported", channel),
    }

    // Ensure there is a --target argument,
    // Determine which target we're building for, and replace it with our fake target.
    let target = get_arg(&args, "--target")
        .map(|val| val.to_owned())
        .unwrap_or_else(|| {
            args.push(format!("--target={}", rustc_meta.host));
            rustc_meta.host
        });

    // XXX: Consider putting this in the target dir, and caching it?
    let nostd_sysroot = TempDir::new("nostd_sysroot")?;
    let sysroot = get_sysroot()?;
    build_sysroot(&target, &sysroot, nostd_sysroot.path())?;

    eprintln!(
        "{:>12} {} ({})",
        console::style("Sysroot").bold().yellow(),
        target,
        nostd_sysroot.path().display(),
    );

    // build RUSTFLAGS, which will only be used for target libraries due to the
    // explicit --target argument.
    let mut rustflags = env::var_os("RUSTFLAGS").unwrap_or_default();
    rustflags.push(" --sysroot=");
    rustflags.push(nostd_sysroot.path());

    // Run cargo build
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .arg("check")
        .arg("--lib")
        .args(&args)
        .env("RUSTFLAGS", rustflags)
        .status()?;
    match status.code() {
        Some(code) => Ok(code),
        None => bail!("exited with signal"),
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    process::exit(cargo_command(args)?)
}
