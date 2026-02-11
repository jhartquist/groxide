use clap::Parser;
use groxide::cli::Cli;
use groxide::error::EXIT_SUCCESS;
use std::process;

fn main() {
    let cli = Cli::parse();
    match groxide::run(&cli) {
        Ok(()) => process::exit(EXIT_SUCCESS),
        Err(e) => {
            eprintln!("{e}");
            process::exit(e.exit_code());
        }
    }
}
