use assert_cmd::Command;
use std::path::{Path, PathBuf};

fn crate_path(krate: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/crates").join(krate)
}

macro_rules! basic {
    ($krate:ident, $what:ident) => {
        #[test]
        fn $krate() {
            Command::cargo_bin("cargo-no-std-check")
                .unwrap()
                .current_dir(crate_path(stringify!($krate)))
                .assert()
                .$what();
        }
    }
}

basic!(nostd, success);
basic!(externstd, failure);
basic!(withstd, failure);
basic!(nostd_dep_nostd, success);
basic!(nostd_dep_externstd, failure);
basic!(nostd_dep_withstd, failure);


/*
use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

fn bin_dir() -> PathBuf {
    env::var_os("CARGO_NOSTD_BIN_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            env::current_exe().ok().map(|mut path| {
                path.pop();
                if path.ends_with("deps") {
                    path.pop();
                }
                path
            })
        })
        .unwrap_or_else(|| panic!("CARGO_NOSTD_BIN_PATH wasn't set. Cannot continue running test"))
}

fn bin_exe() -> PathBuf {
    bin_dir().join(format!("cargo-no-std-check{}", env::consts::EXE_SUFFIX))
}

fn command() -> Command {
    Command::new(bin_exe())
}

fn build_crate(krate: &str) -> ExitStatus {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/crates").join(krate);
    command().current_dir(path).status().unwrap()
}

macro_rules! basic {
    ($krate:ident, $success:ident) => {
        #[test]
        fn $krate() {
            let status = build_crate(stringify!($krate));
            assert_eq!(status.success(), $success);
        }
    }
}

basic!(nostd, true);
basic!(externstd, false);
basic!(withstd, false);
basic!(nostd_dep_nostd, true);
basic!(nostd_dep_externstd, false);
basic!(nostd_dep_withstd, false);
*/

