use anyhow::{bail, ensure, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rustc_version::Channel;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::str;
use walkdir::WalkDir;

// FIXME: Hold some sort of lock while doing operations on our custom sysroot,
// like cargo-xbuild does.

const FAKE_TARGET: &str = "no_std-fake-target";

fn get_sysroot() -> Result<PathBuf> {
    let rustc = env::var("RUSTC").unwrap_or("rustc".to_owned());
    let output = Command::new(rustc)
        .args(&["--print", "sysroot"])
        .stdout(Stdio::piped())
        .spawn()?
        .wait_with_output()?;
    ensure!(output.status.success(), "failed to get sysroot");

    let stdout = str::from_utf8(&output.stdout)?.trim_end();
    Ok(stdout.into())
}

fn with_flag<F>(flag_names: &[&str], args: &mut Vec<String>, callback: F)
where
    F: FnOnce(bool) -> bool,
{
    for idx in 0..args.len() {
        for flag_name in flag_names {
            if args[idx] == flag_name {
                if !callback(true) {
                    args.remove(idx);
                }
                return;
            }
        }
    }
    if callback(false) {
        args.push(flag_names[0].to_owned());
    }
}

// Super hacky method for manipulating the argument list.
fn with_arg_equals<F>(arg_name: &str, args: &mut Vec<String>, callback: F)
where
    F: FnOnce(Option<String>) -> Option<String>,
{
    for idx in 0..args.len() {
        // --name value
        if args[idx] == arg_name && idx + 1 < args.len() {
            let value = args[idx + 1].clone();
            if let Some(new_value) = callback(Some(value)) {
                args[idx + 1] = new_value;
            } else {
                args.drain(idx..=idx + 1);
            }
            return;
        }

        // --name=value
        if args[idx].starts_with(arg_name) && args[idx][arg_name.len()..].starts_with("=") {
            let value = args[idx][arg_name.len() + 1..].to_owned();
            if let Some(new_value) = callback(Some(value)) {
                args[idx] = format!("{}={}", arg_name, new_value);
            } else {
                args.remove(idx);
            }
            return;
        }
    }

    if let Some(new_value) = callback(None) {
        args.push(arg_name.to_owned());
        args.push(new_value);
    }
}

fn get_arg_equals(arg_name: &str, args: &[String]) -> Option<String> {
    let mut args_iter = args.iter();
    while let Some(arg) = args_iter.next() {
        if arg == arg_name {
            return args_iter.next().cloned();
        }
        if arg.starts_with(arg_name) && arg[arg_name.len()..].starts_with("=") {
            return Some(arg[arg_name.len() + 1..].to_owned());
        }
    }
    None
}

fn manifest_path_arg(args: &[String]) -> Option<String> {
    get_arg_equals("--manifest-path", args)
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

fn cargo_command(mut args: Vec<String>) -> Result<()> {
    // Important helper flags.
    match args.first().map(|s| s.as_str()) {
        Some("-h") | Some("--help") => {
            println!("\
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
");
            return Ok(());
        }
        Some("--version") => {
            println!(concat!("cargo-no-std-check ", env!("CARGO_PKG_VERSION")));
            return Ok(());
        }
        _ => {}
    }

    let current_exe = env::current_exe()?;

    let rustc_meta = rustc_version::version_meta()?;
    match rustc_meta.channel {
        Channel::Nightly | Channel::Dev => {}
        channel => bail!("{:?} channel not supported", channel),
    }

    // Replace the provided target argument with our fake target, and extract
    // which target we're going to be building for.
    let mut target = rustc_meta.host.clone();
    with_arg_equals("--target", &mut args, |target_arg| {
        if let Some(target_arg) = target_arg {
            target = target_arg;
        }
        Some(FAKE_TARGET.to_owned())
    });

    let mut meta_cmd = cargo_metadata::MetadataCommand::new();
    if let Some(path) = manifest_path_arg(&args) {
        meta_cmd.manifest_path(path);
    }
    let cargo_meta = meta_cmd.exec()?;

    // XXX: Allow configuring the path?
    let nostd_sysroot = cargo_meta.target_directory.join("nostd_sysroot");

    // Build our new sysroot.
    // FIXME: Support caching? Lock the directory?
    let _ = fs::remove_dir_all(&nostd_sysroot);
    let sysroot = get_sysroot()?;
    build_sysroot(&target, &sysroot, &nostd_sysroot)?;

    eprintln!(
        "{:>12} {} ({})",
        console::style("Sysroot OK").bold().yellow(),
        target,
        nostd_sysroot.display(),
    );

    // Run cargo build
    let cargo = env::var("CARGO").unwrap_or("cargo".to_owned());
    let status = Command::new(cargo)
        .arg("check")
        .arg("--lib")
        .args(&args)
        .env("RUSTC_WRAPPER", &current_exe)
        .env("CARGO_NOSTD_CHECK", "1")
        .env("CARGO_NOSTD_TARGET", &target)
        .env("CARGO_NOSTD_SYSROOT", &nostd_sysroot)
        .status()?;

    if !status.success() {
        eprintln!(
            "{:>12} {}",
            console::style("Errored").bold().red(),
            console::style(status).bold(),
        );
        process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn rustc_wrapper(mut args: Vec<String>) -> Result<()> {
    ensure!(!args.is_empty(), "expected rustc argument");

    let sysroot = env::var("CARGO_NOSTD_SYSROOT")?;
    let target = env::var("CARGO_NOSTD_TARGET")?;
    let verbose = env::var("CARGO_NOSTD_VERBOSE").unwrap_or_default() == "1";

    // Correct the target to the correct target.
    let mut found_target = false;
    with_arg_equals("--target", &mut args, |arg| match arg {
        Some(fake_target) if fake_target == FAKE_TARGET => {
            found_target = true;
            Some(target.clone())
        }
        arg => arg,
    });

    if found_target {
        // Add our modified artificial sysroot to flags.
        args.push("--sysroot".to_owned());
        args.push(sysroot);
    }

    if verbose {
        eprint!(
            "{:>12} `{}",
            console::style("Running").bold().yellow(),
            shell_escape::escape((&args[0]).into()),
        );
        for arg in &args[1..] {
            eprint!(" {}", shell_escape::escape(arg.into()));
        }
        eprintln!("`");
    }

    let status = Command::new(&args[0]).args(&args[1..]).status()?;

    match status.code() {
        Some(code) => process::exit(code),
        None => bail!("rustc exited with signal"),
    }
}

fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    match env::var("CARGO_NOSTD_CHECK") {
        Ok(_) => rustc_wrapper(args),
        Err(_) => cargo_command(args),
    }
}
