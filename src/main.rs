mod analyzer;
mod args;
mod cli;
mod config;
mod processor;
mod report;
mod scanner;

use anyhow::Result;

fn main() -> Result<()> {
    cli::run()
}
