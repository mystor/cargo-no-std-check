use assert_cmd::{cargo::cargo_bin, Command};
use std::env;
use std::path::{Path, PathBuf};
use tempdir::TempDir;

fn crate_path(krate: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/crates")
        .join(krate)
}

fn direct_command() -> Command {
    Command::cargo_bin("cargo-no-std-check").unwrap()
}

// Helper for running `cargo no-std-check` as an actual cargo subcommand.
fn cargo_command() -> Command {
    // Build up a PATH which has `cargo-no-std-check`'s directory at the front.
    let bin = cargo_bin("cargo-no-std-check");
    let mut paths = vec![bin.parent().unwrap().to_owned()];
    let old_path = env::var_os("PATH").unwrap_or_default();
    paths.extend(env::split_paths(&old_path));
    let new_path = env::join_paths(paths).unwrap();

    // Use a temporary fake .cargo, as otherwise `cargo` may search
    // `.cargo/bin` for subcommands before `$PATH`, which could lead to the
    // wrong subcommand being run.
    let temp_home = TempDir::new("cargo_home").unwrap();

    let mut command = Command::new("cargo");
    command
        .arg("no-std-check")
        .env("PATH", dbg!(new_path))
        .env("CARGO_HOME", dbg!(temp_home.path()));
    command
}

// Ensure cargo_command tests run the correct `cargo-no-std-check` executable.
#[test]
fn check_version() {
    let expected = String::from_utf8(direct_command().arg("--version").unwrap().stdout).unwrap();
    let actual = String::from_utf8(cargo_command().arg("--version").unwrap().stdout).unwrap();
    assert_eq!(expected, actual);
}

macro_rules! basic {
    ($krate:ident, $what:ident) => {
        #[test]
        fn $krate() {
            direct_command()
                .current_dir(crate_path(stringify!($krate)))
                .assert()
                .$what();

            cargo_command()
                .current_dir(crate_path(stringify!($krate)))
                .assert()
                .$what();
        }
    };
}

basic!(nostd, success);
basic!(externstd, failure);
basic!(withstd, failure);
basic!(nostd_dep_nostd, success);
basic!(nostd_dep_externstd, failure);
basic!(nostd_dep_withstd, failure);
basic!(macro_user, success);
basic!(nostd_buildrs, success);
