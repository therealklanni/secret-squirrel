mod config;
mod debug;
mod paths;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ssq")]
#[command(about = "Secret Squirrel - Find potential secrets in your code")]
struct Cli {
  /// Override default config file location
  #[arg(long, global = true)]
  config: Option<PathBuf>,

  /// Path to repository (defaults to current directory)
  #[arg(default_value = ".")]
  path: PathBuf,

  /// Only scan staged files
  #[arg(long)]
  staged: bool,

  /// Scan git history
  #[arg(long)]
  history: bool,

  /// Print current configuration
  #[arg(long)]
  print_config: bool,

  /// Only show patterns of this severity or higher
  #[arg(long, value_parser = ["low", "medium", "high", "critical"], ignore_case = true)]
  severity: Option<String>,
}

fn run() -> Result<()> {
  let cli = Cli::parse();
  let mut config = config::Config::load_with_path(cli.config)?;

  // Apply severity filter if provided
  if let Some(severity) = cli.severity {
    config.set_severity_filter(&severity);
  }

  if cli.print_config {
    config.print();
    return Ok(());
  }

  println!("Scanning path: {}", cli.path.display());
  if cli.staged {
    println!("Scanning only staged files");
  }
  if cli.history {
    println!("Scanning git history");
  }

  // TODO: Implement scanning logic

  Ok(())
}

fn main() {
  if let Err(e) = run() {
    eprintln!("Error: {e:#}");
    std::process::exit(1);
  }
}
