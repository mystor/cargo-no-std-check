use anyhow::Result;
use std::env;
use std::io;
use tracing::Level;

fn main() -> Result<()> {
    tracing_subscriber::fmt::Subscriber::builder()
        .with_writer(io::stderr)
        .with_max_level(Level::TRACE)
        .compact()
        .init();
    cargo_no_std_check::run(env::args().skip(1).collect())?;
    Ok(())
}
