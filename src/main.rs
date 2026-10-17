mod cli;
mod provider;
mod result;
mod ui;

use std::io::IsTerminal;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;

use cli::Cli;

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("ws: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<ExitCode> {
    let cli = Cli::parse();
    let provider = provider::build(cli.provider)?;
    let results = provider.search(&cli.query())?;

    if results.is_empty() {
        eprintln!("ws: no results from {}", provider.name());
        return Ok(ExitCode::FAILURE);
    }

    if !std::io::stdout().is_terminal() {
        for r in &results {
            println!("{}\n{}", r.title, r.url);
        }
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(i) = ui::select(&results)? {
        let url = &results[i].url;
        println!("Opening {url}");
        open::that_detached(url).with_context(|| format!("failed to open {url}"))?;
    }
    Ok(ExitCode::SUCCESS)
}
