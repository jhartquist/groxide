#[allow(dead_code)]
pub mod cli;
pub mod error;
#[allow(dead_code)]
mod index_builder;
#[allow(dead_code)]
mod signature;
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
