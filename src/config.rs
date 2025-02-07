use crate::{debug::debug, paths};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
  pub description: Option<String>,
  pub regex: String,
  pub severity: String,
}

#[derive(Debug, PartialEq, Ord, PartialOrd, Eq)]
enum SeverityLevel {
  Low,
  Medium,
  High,
  Critical,
}

impl From<&str> for SeverityLevel {
  fn from(s: &str) -> Self {
    match s.to_lowercase().as_str() {
      "critical" => SeverityLevel::Critical,
      "high" => SeverityLevel::High,
      "medium" => SeverityLevel::Medium,
      _ => SeverityLevel::Low,
    }
  }
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
  #[serde(default)]
  pub patterns: HashMap<String, Pattern>,
  pub ignore_patterns: Option<Vec<String>>,
  pub ignore_paths: Option<Vec<String>>,
  pub severity: Option<String>,
  #[serde(skip)]
  severity_filter: Option<SeverityLevel>,
  #[serde(skip)]
  computed_severity: Option<SeverityLevel>,
}

impl Config {
  pub fn load_with_path(
    config_path: Option<PathBuf>,
  ) -> Result<Self, ConfigError> {
    // Load base config
    let mut base_config = if let Some(path) = config_path {
      debug(&format!("Loading config from: {}", path.display()));
      Self::load_from_path(path)?
    } else {
      Self::load_base_config()?
    };

    // Initialize base config's computed severity
    if let Some(ref sev) = base_config.severity {
      base_config.computed_severity = Some(SeverityLevel::from(sev.as_str()));
    }

    // Try to load and merge local config
    if let Ok(mut local_config) = Self::load_local_config() {
      debug("Merging local config with base config");

      // Initialize local config's computed severity
      if let Some(ref sev) = local_config.severity {
        local_config.computed_severity =
          Some(SeverityLevel::from(sev.as_str()));
      }

      // Merge configs
      let mut final_config = local_config;

      // Only add patterns from base that don't exist in local
      for (name, pattern) in base_config.patterns {
        final_config.patterns.entry(name).or_insert(pattern);
      }

      // Use local ignore lists and severity if present, otherwise use base
      if final_config.ignore_patterns.is_none() {
        final_config.ignore_patterns = base_config.ignore_patterns;
      }
      if final_config.ignore_paths.is_none() {
        final_config.ignore_paths = base_config.ignore_paths;
      }
      if final_config.computed_severity.is_none() {
        final_config.computed_severity = base_config.computed_severity;
        final_config.severity = base_config.severity;
      }

      Ok(final_config)
    } else {
      debug("Using base config");
      Ok(base_config)
    }
  }

  fn load_from_path(path: PathBuf) -> Result<Self, ConfigError> {
    if !path.exists() {
      return Err(ConfigError::IoError(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("Config file not found: {}", path.display()),
      )));
    }

    Ok(serde_yaml::from_str(&fs::read_to_string(path)?)?)
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
      debug("No base config found");
      return Ok(Self::default());
    }

    Ok(serde_yaml::from_str(&fs::read_to_string(
      &base_config_path,
    )?)?)
  }

  fn load_local_config() -> Result<Self, ConfigError> {
    let local_path = PathBuf::from(".ssq.yml");

    if !local_path.exists() {
      return Ok(Self::default());
    }

    debug(&format!("Found local config at: {}", local_path.display()));
    Ok(serde_yaml::from_str(&fs::read_to_string(&local_path)?)?)
  }

  pub fn set_severity_filter(&mut self, level: &str) {
    // CLI flag takes precedence over config file
    self.severity_filter = Some(SeverityLevel::from(level));
    // Ensure computed_severity is cached
    if self.computed_severity.is_none() {
      self.computed_severity =
        self.severity.as_deref().map(SeverityLevel::from);
    }
  }

  fn get_effective_severity(&self) -> Option<&SeverityLevel> {
    // Use CLI-set filter if present, otherwise use config file setting
    self
      .severity_filter
      .as_ref()
      .or(self.computed_severity.as_ref())
  }

  fn meets_severity(&self, pattern: &Pattern) -> bool {
    if let Some(min_severity) = self.get_effective_severity() {
      let pattern_severity = SeverityLevel::from(pattern.severity.as_str());
      pattern_severity >= *min_severity
    } else {
      true
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
    for (name, pattern) in &self.patterns {
      if !self.meets_severity(pattern) {
        continue;
      }

      let severity_style = match pattern.severity.to_lowercase().as_str() {
        "critical" => style(&pattern.severity).red().bold(),
        "high" => style(&pattern.severity).red(),
        "medium" => style(&pattern.severity).yellow(),
        _ => style(&pattern.severity).dim(),
      };
      println!("  - {name} ({severity_style})");
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
  use tempfile::{NamedTempFile, TempDir};

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
  github:
    description: 'GitHub personal access token'
    regex: '[A-Za-z0-9]{{40}}'
    severity: critical
ignore_patterns:
  - 'TEST_API_KEY=.*'
ignore_paths:
  - 'tests/fixtures/*'
"
    )?;

    let config = Config::load_from_path(temp.path().to_path_buf())?;
    assert_eq!(config.patterns.len(), 1);
    assert_eq!(config.patterns["github"].severity, "critical");
    assert_eq!(
      config.patterns["github"].description,
      Some("GitHub personal access token".to_string())
    );
    assert_eq!(config.ignore_patterns.unwrap().len(), 1);
    assert_eq!(config.ignore_paths.unwrap().len(), 1);

    Ok(())
  }

  #[test]
  fn test_config_override() -> Result<(), Box<dyn std::error::Error>> {
    let temp_dir = TempDir::new()?;
    std::env::set_current_dir(&temp_dir)?;

    // Create base config
    let base_config = r"
patterns:
  github:
    description: 'GitHub token'
    regex: '[A-Za-z0-9]{40}'
    severity: critical
  aws:
    description: 'AWS key'
    regex: 'AKIA.*'
    severity: high
ignore_patterns:
  - 'TEST_.*'
ignore_paths:
  - 'tests/*'
";
    std::fs::write("config.yml", base_config)?;

    // Create local config
    let local_config = r"
patterns:
  github:
    description: 'GitHub PAT'
    regex: 'gh[pat]-[0-9a-f]{40}'
    severity: high
  npm:
    description: 'NPM token'
    regex: 'npm_[A-Za-z0-9]{64}'
    severity: critical
ignore_patterns:
  - 'DUMMY_.*'
ignore_paths:
  - 'examples/*'
";
    std::fs::write(".ssq.yml", local_config)?;

    // Load config (this should load both and merge correctly)
    let config = Config::load_with_path(Some(PathBuf::from("config.yml")))?;

    // Verify pattern merging
    assert_eq!(config.patterns.len(), 3); // github (overridden) + aws (preserved) + npm (new)

    // Check github pattern was overridden
    assert_eq!(config.patterns["github"].regex, "gh[pat]-[0-9a-f]{40}");
    assert_eq!(config.patterns["github"].severity, "high");
    assert_eq!(
      config.patterns["github"].description,
      Some("GitHub PAT".to_string())
    );

    // Check aws pattern was preserved
    assert_eq!(config.patterns["aws"].regex, "AKIA.*");
    assert_eq!(config.patterns["aws"].severity, "high");

    // Check npm pattern was added
    assert_eq!(config.patterns["npm"].regex, "npm_[A-Za-z0-9]{64}");
    assert_eq!(config.patterns["npm"].severity, "critical");

    // Verify ignore lists were replaced
    assert_eq!(config.ignore_patterns, Some(vec!["DUMMY_.*".to_string()]));
    assert_eq!(config.ignore_paths, Some(vec!["examples/*".to_string()]));

    Ok(())
  }
}
