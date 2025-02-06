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

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Pattern {
  pub name: String,
  pub description: Option<String>,
  pub regex: String,
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
    // Merge ignore_patterns
    if let Some(other_ignores) = other.ignore_patterns {
      match &mut self.ignore_patterns {
        Some(ignores) => ignores.extend(other_ignores),
        None => self.ignore_patterns = Some(other_ignores),
      }
    }

    // Merge ignore_paths
    if let Some(other_paths) = other.ignore_paths {
      match &mut self.ignore_paths {
        Some(paths) => paths.extend(other_paths),
        None => self.ignore_paths = Some(other_paths),
      }
    }

    // Merge patterns, overwriting existing ones with the same name
    for other_pattern in other.patterns {
      if let Some(existing) = self
        .patterns
        .iter_mut()
        .find(|p| p.name == other_pattern.name)
      {
        *existing = other_pattern;
      } else {
        self.patterns.push(other_pattern);
      }
    }
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
      println!("  - {} ({})", pattern.name, severity_style);
      if let Some(desc) = &pattern.description {
        println!("    Description: {}", style(desc).dim());
      }
      println!("    Pattern: {}", pattern.regex);
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
  - name: 'github'
    description: 'GitHub personal access token'
    regex: '[A-Za-z0-9]{{40}}'
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
    assert_eq!(
      config.patterns[0].description,
      Some("GitHub personal access token".to_string())
    );
    assert_eq!(config.ignore_patterns.unwrap().len(), 1);
    assert_eq!(config.ignore_paths.unwrap().len(), 1);

    Ok(())
  }

  #[test]
  fn test_config_merge() {
    let mut base = Config {
      patterns: vec![Pattern {
        name: "github".to_string(),
        description: Some("GitHub token".to_string()),
        regex: "[A-Za-z0-9]{40}".to_string(),
        severity: "critical".to_string(),
      }],
      ignore_patterns: Some(vec!["TEST_.*".to_string()]),
      ignore_paths: Some(vec!["tests/*".to_string()]),
    };
    let local = Config {
      patterns: vec![
        Pattern {
          name: "github".to_string(), // Same name, should overwrite
          description: Some("GitHub PAT".to_string()),
          regex: "gh[pat]-[A-Za-z0-9]{40}".to_string(),
          severity: "high".to_string(),
        },
        Pattern {
          name: "aws".to_string(), // New pattern, should be added
          description: Some("AWS access key".to_string()),
          regex: "AKIA[A-Z0-9]{16}".to_string(),
          severity: "critical".to_string(),
        },
      ],
      ignore_patterns: Some(vec!["DUMMY_.*".to_string()]),
      ignore_paths: Some(vec!["fixtures/*".to_string()]),
    };
    base.merge(local);
    assert_eq!(base.patterns.len(), 2);
    assert_eq!(base.patterns[0].regex, "gh[pat]-[A-Za-z0-9]{40}");
    assert_eq!(base.ignore_patterns.unwrap().len(), 2);
    assert_eq!(base.ignore_paths.unwrap().len(), 2);
  }
}
