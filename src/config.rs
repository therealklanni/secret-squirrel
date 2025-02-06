use crate::{debug::debug, paths};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
  #[error("Failed to read config file: {0}")]
  IoError(#[from] std::io::Error),
  #[error("Failed to parse config file: {0}")]
  ParseError(#[from] serde_yaml::Error),
  #[error("No base config found")]
  NoBaseConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Pattern {
  pub pattern: String,
  pub severity: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
  #[serde(default)]
  pub patterns: Vec<Pattern>,
  pub ignore_patterns: Option<Vec<String>>,
  pub ignore_paths: Option<Vec<String>>,
}

impl Config {
  pub fn load_with_path(
    config_path: Option<PathBuf>,
  ) -> Result<Self, ConfigError> {
    let mut config = match config_path {
      Some(path) => Self::load_from_path(path)?,
      None => Self::load_base_config()?,
    };

    // Try to load local config and merge
    if let Ok(local_config) = Self::load_local_config() {
      config.merge(local_config);
    }

    Ok(config)
  }

  fn load_from_path(path: PathBuf) -> Result<Self, ConfigError> {
    if !path.exists() {
      return Err(ConfigError::IoError(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("Config file not found: {}", path.display()),
      )));
    }

    let contents = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&contents)?)
  }

  fn load_base_config() -> Result<Self, ConfigError> {
    let config_dir =
      paths::get_config_dir().ok_or(ConfigError::NoBaseConfig)?;

    let base_config_path = config_dir.join("config.yml");
    debug(&format!(
      "Loading base config from: {}",
      base_config_path.display()
    ));

    if !base_config_path.exists() {
      debug(&format!(
        "No base config found at: {}",
        base_config_path.display()
      ));
      return Ok(Self::default());
    }

    let contents = fs::read_to_string(base_config_path)?;
    Ok(serde_yaml::from_str(&contents)?)
  }

  fn load_local_config() -> Result<Self, ConfigError> {
    let local_path = PathBuf::from("./.ssq.yml");

    if !local_path.exists() {
      return Ok(Self::default());
    }

    let contents = fs::read_to_string(local_path)?;
    Ok(serde_yaml::from_str(&contents)?)
  }

  fn merge(&mut self, other: Self) {
    self.patterns.extend(other.patterns);
  }

  pub fn print(&self) {
    println!("{}", style("Current Configuration:").bold().cyan());
    println!("{}", style("======================").cyan());

    if let Some(ref ignore_patterns) = self.ignore_patterns {
      println!("\n{}", style("Ignored Patterns:").bold());
      for pattern in ignore_patterns {
        println!("  - {pattern}");
      }
    }

    if let Some(ref ignore_paths) = self.ignore_paths {
      println!("\n{}", style("Ignored Paths:").bold());
      for path in ignore_paths {
        println!("  - {path}");
      }
    }

    if self.patterns.is_empty() {
      println!("\n{}", style("No detection patterns configured.").italic());
      return;
    }

    println!("\n{}", style("Detection Patterns:").bold());
    for pattern in &self.patterns {
      let severity_style = match pattern.severity.to_lowercase().as_str() {
        "critical" => style(&pattern.severity).red().bold(),
        "high" => style(&pattern.severity).red(),
        "medium" => style(&pattern.severity).yellow(),
        _ => style(&pattern.severity).dim(),
      };
      println!("  - {} ({})", pattern.pattern, severity_style);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::io::Write;
  use tempfile::NamedTempFile;

  #[test]
  fn test_empty_config() {
    let config = Config::default();
    assert!(config.patterns.is_empty());
    assert!(config.ignore_patterns.is_none());
    assert!(config.ignore_paths.is_none());
  }

  #[test]
  fn test_load_with_custom_path() -> Result<(), Box<dyn std::error::Error>> {
    let mut temp = NamedTempFile::new()?;
    write!(
      temp,
      r"
patterns:
  - pattern: '[A-Za-z0-9]{{40}}'
    severity: critical
ignore_patterns:
  - 'TEST_API_KEY=.*'
ignore_paths:
  - 'tests/fixtures/*'
"
    )?;

    let config = Config::load_with_path(Some(temp.path().to_path_buf()))?;
    assert_eq!(config.patterns.len(), 1);
    assert_eq!(config.patterns[0].severity, "critical");
    assert_eq!(config.ignore_patterns.unwrap().len(), 1);
    assert_eq!(config.ignore_paths.unwrap().len(), 1);

    Ok(())
  }
}
