use anyhow::{bail, ensure, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rustc_version::Channel;
use std::env;
use std::fs;
use std::iter::{self, IntoIterator};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{self, Command, ExitStatus, Stdio};
use std::str;
use tempdir::TempDir;
use walkdir::WalkDir;

const FAKE_TARGET: &str = "no_std-fake-target";

struct Args {
    args: Vec<String>,
}

impl Args {
    fn new(args: Vec<String>) -> Self {
        Args { args }
    }

    fn push(&mut self, arg: String) {
        self.args.push(arg)
    }

    fn replace<I>(&mut self, range: Range<usize>, args: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.args.splice(range, args);
    }

    fn get_flag(&self, name: &str) -> Option<usize> {
        self.args.iter().position(|arg| arg == name)
    }

    fn get_arg(&self, name: &str) -> Option<(Range<usize>, String)> {
        let mut iter = self.args.iter().enumerate();
        while let Some((idx, arg)) = iter.next() {
            if arg == name {
                return iter.next().map(|(i2, val)| (idx..i2 + 1, val.clone()));
            }
            if arg.starts_with(name) && arg[name.len()..].starts_with('=') {
                return Some((idx..idx + 1, arg[name.len() + 1..].to_owned()));
            }
        }
        None
    }
}

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

fn cargo_command(mut args: Args) -> Result<Option<ExitStatus>> {
    // Cargo passes the subcommand name as the first argument, remove it if found.
    match args.args.first() {
        Some(first) if first == "no-std-check" => {
            args.args.remove(0);
        }
        _ => {}
    }

    // --help
    if args.get_flag("-h").is_some() || args.get_flag("--help").is_some() {
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
        return Ok(None);
    }

    // --version
    if args.get_flag("--version").is_some() {
        println!(concat!("cargo-no-std-check ", env!("CARGO_PKG_VERSION")));
        return Ok(None);
    }

    let current_exe = env::current_exe()?;

    let rustc_meta = rustc_version::version_meta()?;
    match rustc_meta.channel {
        Channel::Nightly | Channel::Dev => {}
        channel => bail!("{:?} channel not supported", channel),
    }

    // Determine which target we're building for, and replace it with our fake target.
    let fake_target = format!("--target={}", FAKE_TARGET);
    let target = if let Some((range, val)) = args.get_arg("--target") {
        args.replace(range, iter::once(fake_target));
        val
    } else {
        args.push(fake_target);
        rustc_meta.host
    };

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

    // Run cargo build
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = Command::new(cargo)
        .arg("check")
        .arg("--lib")
        .args(&args.args)
        .env("RUSTC_WRAPPER", &current_exe)
        .env("CARGO_NOSTD_CHECK", "1")
        .env("CARGO_NOSTD_TARGET", &target)
        .env("CARGO_NOSTD_SYSROOT", nostd_sysroot.path())
        .status()?;
    Ok(Some(status))
}

fn rustc_wrapper(mut args: Args) -> Result<Option<ExitStatus>> {
    ensure!(!args.args.is_empty(), "expected rustc argument");

    let sysroot = env::var("CARGO_NOSTD_SYSROOT")?;
    let target = env::var("CARGO_NOSTD_TARGET")?;
    let verbose = env::var("CARGO_NOSTD_VERBOSE").unwrap_or_default() == "1";

    // Correct the target to the correct target.
    if let Some((range, val)) = args.get_arg("--target") {
        if val == FAKE_TARGET {
            // Replace the target flag with the real target.
            args.replace(range, iter::once(format!("--target={}", target)));
            // Add our modified artificial sysroot to flags.
            args.push(format!("--sysroot={}", sysroot));
        }
    }

    if verbose {
        eprint!(
            "{:>12} `{}",
            console::style("Running").bold().yellow(),
            shell_escape::escape((&args.args[0]).into()),
        );
        for arg in &args.args[1..] {
            eprint!(" {}", shell_escape::escape(arg.into()));
        }
        eprintln!("`");
    }

    Ok(Some(
        Command::new(&args.args[0]).args(&args.args[1..]).status()?,
    ))
}

fn main() -> Result<()> {
    let args = Args::new(env::args().skip(1).collect());
    let status = match env::var("CARGO_NOSTD_CHECK") {
        Ok(_) => rustc_wrapper(args)?,
        Err(_) => cargo_command(args)?,
    };

    if let Some(status) = status {
        match status.code() {
            Some(code) => process::exit(code),
            None => bail!("exited with signal"),
        }
    }
    Ok(())
}
