use anyhow::{anyhow, bail, ensure, Result};
use indicatif::{ProgressBar, ProgressStyle};
use rustc_version::Channel;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::str;
use walkdir::WalkDir;

// FIXME: Hold some sort of lock while doing operations on our custom sysroot,
// like cargo-xbuild does.

trait CommandExt {
    fn capture_stdout(&mut self) -> Result<Output>;
}

impl CommandExt for Command {
    fn capture_stdout(&mut self) -> Result<Output> {
        let output = self.stdout(Stdio::piped()).spawn()?.wait_with_output()?;
        Ok(output)
    }
}

fn rustc() -> Command {
    let rustc = env::var("RUSTC").unwrap_or("rustc".to_owned());
    Command::new(rustc)
}

fn cargo() -> Command {
    let rustc = env::var("CARGO").unwrap_or("cargo".to_owned());
    Command::new(rustc)
}

fn get_sysroot() -> Result<PathBuf> {
    let output = rustc().args(&["--print", "sysroot"]).capture_stdout()?;
    ensure!(output.status.success(), "failed to get sysroot");

    let stdout = str::from_utf8(&output.stdout)?.trim_end();
    Ok(stdout.into())
}

fn get_target_spec_json() -> Result<String> {
    let output = rustc()
        .args(&["-Z", "unstable-options", "--print", "target-spec-json"])
        .capture_stdout()?;
    ensure!(output.status.success(), "failed to get target spec json");
    Ok(String::from_utf8(output.stdout)?)
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

fn build_sysroot(
    host_target: &str,
    nostd_target: &str,
    src_sysroot: &Path,
    dst_sysroot: &Path,
    target_path: &Path,
) -> Result<()> {
    eprintln!(
        "{:>12} #![no_std] sysroot",
        console::style("Creating").bold().green()
    );

    // Root Paths.
    let src_root = src_sysroot.join("lib/rustlib").join(host_target);
    let dst_root = dst_sysroot.join("lib/rustlib");
    let dst_host_root = dst_root.join(host_target);
    let dst_nostd_root = dst_root.join(nostd_target);

    // List of source/dst entries to copy.
    let mut to_copy = <Vec<(PathBuf, PathBuf)>>::new();

    // Copy over `bin` entries.
    let src_bin = src_root.join("bin");
    let dst_host_bin = dst_host_root.join("bin");
    for entry in WalkDir::new(&src_bin) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let suffix = entry.path().strip_prefix(&src_bin).unwrap();
            to_copy.push((entry.path().to_owned(), dst_host_bin.join(suffix)));
        }
    }

    // Copy over `lib` entries.
    let src_lib = src_root.join("lib");
    let dst_host_lib = dst_host_root.join("lib");
    let dst_nostd_lib = dst_nostd_root.join("lib");
    for entry in WalkDir::new(&src_lib) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let lib_name = entry
                .file_name()
                .to_str()
                .unwrap_or("")
                .split(|c| c == '-' || c == '.')
                .next()
                .unwrap();
            let suffix = entry.path().strip_prefix(&src_lib).unwrap();
            to_copy.push((entry.path().to_owned(), dst_host_lib.join(suffix)));

            // Copy everything but `libstd` to `dst_nostd_lib`.
            if lib_name != "libstd" {
                to_copy.push((entry.path().to_owned(), dst_nostd_lib.join(suffix)));
            }
        }
    }

    // Perform the copies.
    let pb = ProgressBar::new(to_copy.len() as u64);
    pb.set_style(cargo_bar_style());
    pb.set_prefix("Copying");
    for (from, to) in &to_copy {
        let suffix = to.strip_prefix(&dst_sysroot).unwrap().to_str().unwrap();
        pb.set_message(suffix);
        fs::create_dir_all(to.parent().unwrap())?;
        fs::copy(from, to)?;
        pb.inc(1);
    }
    pb.finish_and_clear();

    let target_json = get_target_spec_json()?;
    fs::write(target_path, target_json)?;

    Ok(())
}

fn cargo_command(args: Vec<String>) -> Result<()> {
    let current_exe = env::current_exe()?;

    let rustc_meta = rustc_version::version_meta()?;
    match rustc_meta.channel {
        Channel::Nightly | Channel::Dev => {}
        channel => bail!("{:?} channel not supported", channel),
    }

    let mut meta_cmd = cargo_metadata::MetadataCommand::new();
    if let Some(path) = manifest_path_arg(&args) {
        meta_cmd.manifest_path(path);
    }
    let cargo_meta = meta_cmd.exec()?;

    let host_target = &rustc_meta.host;
    let nostd_target = format!("{}-nostd", host_target);

    // XXX: Allow configuring the path?
    let nostd_sysroot = cargo_meta.workspace_root.join("target/nostd_sysroot");
    let target_path = nostd_sysroot.join(format!("{}.json", nostd_target));

    let _ = fs::remove_dir_all(&nostd_sysroot);

    // Build our new sysroot.
    // FIXME: Support caching?
    let sysroot = get_sysroot()?;
    build_sysroot(
        &host_target,
        &nostd_target,
        &sysroot,
        &nostd_sysroot,
        &target_path,
    )?;

    eprintln!(
        "{:>12} {} (sysroot: {})",
        console::style("Target").bold().yellow(),
        nostd_target,
        nostd_sysroot.display(),
    );

    // Run cargo build
    let status = cargo()
        .arg("build")
        .arg("--target")
        .arg("no_std-fake-target")
        .args(&args)
        .env("RUSTC_WRAPPER", &current_exe)
        .env("CARGO_NOSTD_CHECK", &nostd_sysroot)
        .env("CARGO_NOSTD_TARGET", &host_target)
        .status()?;
    ensure!(status.success(), "cargo build exited with failure");

    Ok(())
}

fn rustc_wrapper(mut args: Vec<String>, sysroot: String) -> Result<()> {
    ensure!(!args.is_empty(), "expected rustc argument");

    tracing::info!(?args, ?sysroot, "rustc_wrapper");

    let target = env::var("CARGO_NOSTD_TARGET")?;

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

    tracing::info!(?args, "rustc");

    tracing::info!("{}", args.join(" "));

    let status = Command::new(&args[0]).args(&args[1..]).status()?;

    match status.code() {
        Some(code) => std::process::exit(code),
        None => bail!("rustc exited with signal"),
    }
}

pub fn run(args: Vec<String>) -> Result<()> {
    match env::var("CARGO_NOSTD_CHECK").ok() {
        Some(sysroot) => rustc_wrapper(args, sysroot),
        None => cargo_command(args),
    }
}
