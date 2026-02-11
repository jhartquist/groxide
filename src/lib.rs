#[allow(dead_code)]
mod cache;
#[allow(dead_code)]
pub mod cli;
#[allow(dead_code)]
mod docgen;
pub mod error;
#[allow(dead_code)]
mod external;
#[allow(dead_code)]
mod index_builder;
#[allow(dead_code)]
mod query;
#[allow(dead_code)]
mod render;
#[allow(dead_code)]
mod resolve;
#[allow(dead_code)]
mod search;
#[allow(dead_code)]
mod signature;
#[allow(dead_code)]
mod stdlib;
#[allow(dead_code)]
mod types;

use cli::Cli;
use error::Result;

/// Runs the groxide CLI with the given parsed arguments.
///
/// Returns `Ok(())` on success, `Err(GroxError)` on failure.
///
/// # Errors
///
/// Returns `GroxError` if crate resolution, doc generation, or querying fails.
pub fn run(_cli: &Cli) -> Result<()> {
    Ok(())
}
