use crate::{debug::debug, paths};
use anyhow::Result;
use console::style;
use serde::{Deserialize, Serialize};
// Add serde_with for custom serialization
use serde_with::{serde_as, DisplayFromStr};
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

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct Pattern {
  pub description: Option<String>,
  pub regex: String,
  pub severity: String,
}

#[derive(Debug, PartialEq, Ord, PartialOrd, Eq)]
pub enum SeverityLevel {
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
#[serde(rename_all = "snake_case")]
pub struct Config {
  #[serde(default)]
  pub patterns: HashMap<String, Pattern>,
  #[serde(default)]
  pub ignore_patterns: Option<Vec<String>>,
  #[serde(default)]
  pub ignore_paths: Option<Vec<String>>,
  #[serde(default)]
  pub severity: Option<String>,
  #[serde(default = "default_ignore_behavior")]
  pub ignore_pattern_behavior: String,
  #[serde(default = "default_ignore_behavior")]
  pub ignore_paths_behavior: String,
  #[serde(skip)]
  severity_filter: Option<SeverityLevel>,
  #[serde(skip)]
  pub computed_severity: Option<SeverityLevel>,
}

fn default_ignore_behavior() -> String {
  "merge".to_string()
}

impl Config {
  fn merge_config(&mut self, other: &Self) {
    // Apply local config's behavior settings first
    if other.ignore_pattern_behavior == "replace" {
      self.ignore_pattern_behavior = other.ignore_pattern_behavior.to_string();
    }
    if other.ignore_paths_behavior == "replace" {
      self.ignore_paths_behavior = other.ignore_paths_behavior.to_string();
    }

    // Then merge or replace according to the behavior settings
    if other.ignore_patterns.is_some() {
      self.ignore_patterns = if self.ignore_pattern_behavior == "replace" {
        other.ignore_patterns.clone()
      } else {
        let mut merged = self.ignore_patterns.clone().unwrap_or_default();
        if let Some(patterns) = &other.ignore_patterns {
          merged.extend(patterns.clone());
        }
        Some(merged)
      };
    }

    if other.ignore_paths.is_some() {
      self.ignore_paths = if self.ignore_paths_behavior == "replace" {
        debug("Replacing ignore paths with local config");
        other.ignore_paths.clone()
      } else {
        debug("Merging ignore paths with base config");
        let mut merged = self.ignore_paths.clone().unwrap_or_default();
        if let Some(paths) = &other.ignore_paths {
          merged.extend(paths.clone());
        }
        Some(merged)
      };
    }
  }

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
    if let Ok(local_config) = Self::load_local_config() {
      debug("Merging local config with base config");
      base_config.merge_config(&local_config);

      // Update severity if local config has one
      if let Some(ref sev) = local_config.severity {
        base_config.severity = Some(sev.clone());
        base_config.computed_severity = Some(SeverityLevel::from(sev.as_str()));
      }

      Ok(base_config)
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
    // CLI flag updates both the filter and the base severity
    let level = level.to_string().to_uppercase();
    self.severity_filter = Some(SeverityLevel::from(level.as_str()));
    self.severity = Some(level.clone());
    self.computed_severity = Some(SeverityLevel::from(level.as_str()));
  }

  pub fn get_effective_severity(&self) -> Option<&SeverityLevel> {
    // CLI filter takes precedence, then computed severity from config
    self
      .severity_filter
      .as_ref()
      .or(self.computed_severity.as_ref())
  }

  pub fn meets_severity(&self, pattern: &Pattern) -> bool {
    if let Some(min_severity) = self.get_effective_severity() {
      let pattern_severity = SeverityLevel::from(pattern.severity.as_str());
      pattern_severity >= *min_severity
    } else {
      true
    }
  }

  pub fn get_effective_config(&self) -> ConfigDisplay {
    ConfigDisplay {
      severity: self
        .get_effective_severity()
        .map_or("LOW".to_string(), |s| {
          match s {
            SeverityLevel::Critical => "CRITICAL",
            SeverityLevel::High => "HIGH",
            SeverityLevel::Medium => "MEDIUM",
            SeverityLevel::Low => "LOW",
          }
          .to_string()
        }),
      ignore_pattern_behavior: self.ignore_pattern_behavior.clone(),
      ignore_paths_behavior: self.ignore_paths_behavior.clone(),
      ignore_patterns: self.ignore_patterns.clone().unwrap_or_default(),
      ignore_paths: self.ignore_paths.clone().unwrap_or_default(),
      patterns: self
        .patterns
        .iter()
        .filter(|(_, p)| self.meets_severity(p))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect(),
    }
  }

  pub fn print(&self) {
    println!("{}", style("Current Configuration:").bold().cyan());
    println!("{}", style("======================").cyan());
    println!();

    // Just serialize the effective config directly
    let yaml = serde_yaml::to_string(&self.get_effective_config())
      .expect("Failed to serialize config");

    // Print the YAML with styling
    for line in yaml.lines() {
      if line.starts_with("severity:") {
        let (key, value) = line.split_once(": ").unwrap();
        println!("{}: {}", key, style(value).yellow());
      } else if line.contains("severity:") {
        let (indent, rest) = line.split_at(line.find("severity:").unwrap());
        let (key, value) = rest.split_once(": ").unwrap();
        let severity_style = match value.to_lowercase().as_str() {
          "critical" => style(value).red().bold(),
          "high" => style(value).red(),
          "medium" => style(value).yellow(),
          _ => style(value).dim(),
        };
        println!("{indent}{key}: {severity_style}");
      } else if line.ends_with(':') {
        println!("{line}");
      } else if line.starts_with("- ") {
        println!("  {line}");
      } else {
        println!("{line}");
      }
    }
  }
}

// Helper struct to control YAML serialization order
#[serde_as]
#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ConfigDisplay {
  severity: String,
  #[serde_as(as = "DisplayFromStr")]
  ignore_pattern_behavior: String,
  #[serde_as(as = "DisplayFromStr")]
  ignore_paths_behavior: String,
  ignore_patterns: Vec<String>,
  ignore_paths: Vec<String>,
  patterns: HashMap<String, Pattern>,
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
