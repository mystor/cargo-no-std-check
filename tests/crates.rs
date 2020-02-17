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
