mod config;
mod debug;
mod paths;
mod scan;
mod ui;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
  let running = Arc::new(AtomicBool::new(true));
  let r = running.clone();

  ctrlc::set_handler(move || {
    r.store(false, Ordering::SeqCst);
    // Clean up terminal state immediately
    ui::ScanUI::cleanup();
    println!("\nScan interrupted.");
    std::process::exit(0);
  })?;

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
    // TODO: Implement staged files scanning
  }
  if cli.history {
    println!("Scanning git history");
    // TODO: Implement git history scanning
  }

  let mut scanner = scan::Scanner::new(&config, running);
  let result = scanner.scan_path(&cli.path);

  // Only print results if we weren't interrupted
  if result.is_ok() {
    scanner.print_results();
  }

  result
}

fn main() {
  let hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |info| {
    ui::ScanUI::cleanup();
    hook(info);
  }));

  if let Err(e) = run() {
    ui::ScanUI::cleanup();
    eprintln!("\nError: {e:#}");
    std::process::exit(1);
  }
}
