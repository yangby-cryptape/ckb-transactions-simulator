mod client;
mod config;
mod error;
mod runtime;
mod storage;
mod types;

use config::AppConfig;

fn main() -> anyhow::Result<()> {
    env_logger::init();
    log::info!("Starting ...");

    AppConfig::load()?.execute()?;

    log::info!("Done.");
    Ok(())
}
