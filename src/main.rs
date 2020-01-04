use anyhow::Result;
use std::env;
use tracing::Level;
use std::io;

fn main() -> Result<()> {
    cargo_no_std_check::run(env::args().skip(1).collect())?;
    Ok(())
}
