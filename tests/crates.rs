use assert_cmd::{assert::Assert, cargo::cargo_bin, Command};
use predicates::prelude::*;
use std::env;
use std::path::{Path, PathBuf};
use tempdir::TempDir;

fn crate_path(krate: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/crates")
        .join(krate)
}

fn test_command(test: impl Fn(&mut Command) -> Assert) -> (Assert, Assert) {
    // Run the subcommand directly
    let direct_call = test(&mut Command::cargo_bin("cargo-no-std-check").unwrap());

    // Build up a PATH which has `cargo-no-std-check`'s directory at the front.
    let bin = cargo_bin("cargo-no-std-check");
    assert_eq!(
        bin.file_name().unwrap(),
        &format!("cargo-no-std-check{}", env::consts::EXE_SUFFIX)[..]
    );
    let mut paths = vec![bin.parent().unwrap().to_owned()];
    let old_path = env::var_os("PATH").unwrap_or_default();
    paths.extend(env::split_paths(&old_path));
    let new_path = env::join_paths(paths).unwrap();

    // Use a temporary fake .cargo, as otherwise `cargo` may search
    // `.cargo/bin` for subcommands before `$PATH`, which could lead to the
    // wrong subcommand being run.
    let temp_home = TempDir::new("cargo_home").unwrap();

    let cargo_call = test(
        Command::new("cargo")
            .arg("no-std-check")
            .env("PATH", dbg!(new_path))
            .env("CARGO_HOME", dbg!(temp_home.path())),
    );

    (direct_call, cargo_call)
}

// Ensure cargo_command tests run the correct `cargo-no-std-check` executable.
#[test]
fn check_version() {
    let (direct_call, cargo_call) = test_command(|cmd| {
        cmd.arg("--version")
            .assert()
            .success()
            .stdout(predicates::str::starts_with("cargo-no-std-check "))
    });

    // The --version command should be identical for both runs, as we should be
    // running the same binary.
    assert_eq!(direct_call.get_output(), cargo_call.get_output());
}

macro_rules! success {
    ($assert:expr) => {
        $assert
            .success()
            .stderr(predicates::str::contains("can't find crate").not())
    };
}

macro_rules! failure {
    ($assert:expr) => {
        $assert
            .failure()
            .stderr(predicates::str::contains("can't find crate"))
    };
}

macro_rules! basic {
    ($krate:ident, $what:ident) => {
        #[test]
        fn $krate() {
            let cwd = crate_path(stringify!($krate));
            test_command(|cmd| {
                Command::new("cargo")
                    .arg("clean")
                    .current_dir(&cwd)
                    .assert()
                    .success();
                let assert = cmd.arg("--verbose").current_dir(&cwd).assert();
                $what!(assert)
            });
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
