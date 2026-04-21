mod analyzer;
mod args;
mod cdjsafe;
mod cli;
mod config;
mod processor;
mod rbsort;
mod report;
mod scanner;
mod updater;
mod xmlutil;

use anyhow::Result;

fn main() -> Result<()> {
    cli::run()
}
