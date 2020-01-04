use anyhow::{bail, ensure, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rustc_version::Channel;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Output, Stdio};
use std::str;
use walkdir::WalkDir;

// FIXME: Hold some sort of lock while doing operations on our custom sysroot,
// like cargo-xbuild does.

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

fn cargo_command(args: Vec<String>) -> Result<()> {
    let current_exe = env::current_exe()?;

    let rustc_meta = rustc_version::version_meta()?;
    match rustc_meta.channel {
        Channel::Nightly | Channel::Dev => {}
        channel => bail!("{:?} channel not supported", channel),
    }
    // XXX: Pull this from `--target` arguments?
    let target = &rustc_meta.host;

    let mut meta_cmd = cargo_metadata::MetadataCommand::new();
    if let Some(path) = manifest_path_arg(&args) {
        meta_cmd.manifest_path(path);
    }
    let cargo_meta = meta_cmd.exec()?;

    // XXX: Allow configuring the path?
    let nostd_sysroot = cargo_meta.workspace_root.join("target/nostd_sysroot");

    // Build our new sysroot.
    // FIXME: Support caching?
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
        .arg("--target")
        .arg("no_std-fake-target")
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
    {
        let mut args_iter = args.iter_mut();
        while let Some(arg) = args_iter.next() {
            if arg == "--target" {
                match args_iter.next() {
                    Some(fake_target) if fake_target == "no_std-fake-target" => {
                        *fake_target = target.clone();
                        found_target = true;
                    }
                    _ => {}
                }
                break;
            }

            if arg == "--target=no_std-fake-target" {
                *arg = format!("--target={}", target);
                found_target = true;
                break;
            }
        }
    }

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

pub fn run(args: Vec<String>) -> Result<()> {
    match env::var("CARGO_NOSTD_CHECK") {
        Ok(_) => rustc_wrapper(args),
        Err(_) => cargo_command(args),
    }
}
