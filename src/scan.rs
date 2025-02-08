use crate::config::{Config, Pattern};
use crate::ui::ScanUI;
use anyhow::Result;
use console::style;
use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::gitignore::GitignoreBuilder;
use ignore::WalkBuilder;
use parking_lot::Mutex;
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

const LARGE_FILE_THRESHOLD: u64 = 1024 * 1024; // 1MB
const BINARY_CHECK_BYTES: usize = 512; // Check first 512 bytes for binary content
const MAX_CONCURRENT_SCANS: usize = 10; // Limit parallel scans

#[derive(Debug)]
pub struct Match {
  pub pattern_name: String,
  pub file_path: String,
  pub line_number: u64,
  pub line: String,
  pub pattern: Pattern,
}

pub struct Scanner<'a> {
  config: &'a Config,
  matches: Vec<Match>,
  scanned_files: HashSet<String>,
}

impl<'a> Scanner<'a> {
  pub fn new(config: &'a Config) -> Self {
    Self {
      config,
      matches: Vec::new(),
      scanned_files: HashSet::new(),
    }
  }
}

struct CompiledPattern {
  name: String,
  pattern: Pattern,
}

impl Scanner<'_> {
  #[allow(clippy::too_many_lines)]
  pub fn scan_path(&mut self, path: &Path) -> Result<()> {
    // Pre-compile patterns and setup matchers
    let patterns: Vec<CompiledPattern> = self
      .config
      .patterns
      .iter()
      .filter(|(_, p)| self.config.meets_severity(p))
      .map(|(name, pattern)| {
        Ok(CompiledPattern {
          name: name.clone(),
          pattern: pattern.clone(),
        })
      })
      .collect::<Result<Vec<_>>>()?;

    // Setup ignore pattern matcher
    let mut gitignore_builder = GitignoreBuilder::new(path);
    if let Some(ref ignore_paths) = self.config.ignore_paths {
      for pattern in ignore_paths {
        gitignore_builder.add_line(None, pattern)?;
      }
    }
    let ignore_matcher = gitignore_builder.build()?;

    // Setup ignore pattern matcher
    let ignore_pattern_matcher =
      if let Some(ref ignore_patterns) = self.config.ignore_patterns {
        let pattern = ignore_patterns.join("|");
        Some(RegexMatcher::new(&pattern)?)
      } else {
        None
      };

    // Count total files first
    let total_files = WalkBuilder::new(path)
      .hidden(false)
      .ignore(true)
      .git_ignore(true)
      .build()
      .filter_map(Result::ok)
      .filter(|e| {
        let path = e.path();
        path.is_file() && !ignore_matcher.matched(path, false).is_ignore()
      })
      .count();

    // Initialize UI
    let ui = Arc::new(Mutex::new(ScanUI::new(total_files)?));
    let matches = Arc::new(Mutex::new(Vec::new()));
    let scanned_files = Arc::new(Mutex::new(HashSet::new()));

    // Collect files from walker
    let files: Vec<_> = WalkBuilder::new(path)
      .hidden(false)
      .ignore(true)
      .git_ignore(true)
      .build()
      .filter_map(Result::ok)
      .filter(|e| {
        let path = e.path();
        path.is_file() && !ignore_matcher.matched(path, false).is_ignore()
      })
      .collect();

    // Process files in parallel with new UI updates
    for chunk in files.chunks(MAX_CONCURRENT_SCANS) {
      chunk.into_par_iter().for_each(|entry| {
        let binding = entry;
        let path = binding.path();
        let file_path = path.display().to_string();

        // Get file metadata and handle large/binary files
        if let Ok(metadata) = path.metadata() {
          // Handle large files with mmap
          if metadata.len() > LARGE_FILE_THRESHOLD {
            return;
          }

          // Skip binary files
          if Self::is_binary_file(path) {
            return;
          }
        }

        #[allow(clippy::cast_precision_loss)]
        let pattern_count = patterns.len() as f32;
        let mut current_pattern = 0f32;

        // Regular file scanning
        for pattern in &patterns {
          current_pattern += 1.0;
          let progress = current_pattern / pattern_count;

          ui.lock().update_scan(
            file_path.clone(),
            format!("checking {}", pattern.name),
            progress,
          );

          if let Ok(matcher) = RegexMatcher::new(&pattern.pattern.regex) {
            if let Ok(()) = SearcherBuilder::new()
              .binary_detection(BinaryDetection::quit(b'\x00'))
              .line_number(true)
              .build()
              .search_path(
                &matcher,
                path,
                UTF8(|line_number, line| {
                  if Self::should_ignore_match(
                    line,
                    ignore_pattern_matcher.as_ref(),
                  ) {
                    return Ok(true);
                  }

                  matches.lock().push(Match {
                    pattern_name: pattern.name.clone(),
                    file_path: path.to_string_lossy().to_string(),
                    line_number,
                    line: line.to_string(),
                    pattern: pattern.pattern.clone(),
                  });

                  // Add to problem files in UI
                  ui.lock().add_problem_file(file_path.clone());

                  Ok(true)
                }),
              )
            {}
          }
        }

        ui.lock().complete_scan(&file_path);
        scanned_files.lock().insert(file_path);
      });
    }

    // Move results back
    self.matches = Arc::try_unwrap(matches)
      .expect("Matches still have multiple owners")
      .into_inner();
    self.scanned_files = Arc::try_unwrap(scanned_files)
      .expect("Scanned files still have multiple owners")
      .into_inner();

    Ok(())
  }

  fn is_binary_file(path: &Path) -> bool {
    if let Ok(file) = std::fs::File::open(path) {
      use std::io::Read;
      let mut buffer = vec![0; BINARY_CHECK_BYTES];
      if file
        .take(BINARY_CHECK_BYTES as u64)
        .read(&mut buffer)
        .is_ok()
        && buffer.iter().any(|&b| b == 0)
      {
        return true;
      }
    }
    false
  }

  fn should_ignore_match(
    line: &str,
    ignore_matcher: Option<&RegexMatcher>,
  ) -> bool {
    if let Some(ignore) = ignore_matcher {
      ignore.is_match(line.as_bytes()).unwrap_or(false)
    } else {
      false
    }
  }

  pub fn print_results(&self) {
    if self.matches.is_empty() {
      println!("\n{}", style("No matches found.").green());
      return;
    }

    // Show problematic files first
    let unique_files: HashSet<_> =
      self.matches.iter().map(|m| &m.file_path).collect();

    println!("\n{}", style("Problematic files:").red().bold());
    println!("{}", style("──────────────────").red());
    for file in unique_files {
      println!(" {} {}", style("●").red(), file);
    }

    // Then show detailed matches
    println!("\n{}", style("Detailed matches:").red().bold());
    println!("{}", style("═════════════════").red());

    for m in &self.matches {
      let severity_style = match m.pattern.severity.to_lowercase().as_str() {
        "critical" => style(&m.pattern.severity).red().bold(),
        "high" => style(&m.pattern.severity).red(),
        "medium" => style(&m.pattern.severity).yellow(),
        _ => style(&m.pattern.severity).dim(),
      };

      println!(
        "\n{} {} ({})",
        style("Pattern:").bold(),
        &m.pattern_name,
        severity_style
      );
      if let Some(ref desc) = m.pattern.description {
        println!("{} {}", style("Description:").bold(), desc);
      }

      println!(
        "{} {}:{}",
        style("Location:").bold(),
        style(&m.file_path).cyan(),
        style(m.line_number).cyan().bold()
      );

      println!("{} {}", style("Match:").bold(), style(m.line.trim()).dim());
    }

    // Final summary
    let files_with_matches: HashSet<_> =
      self.matches.iter().map(|m| &m.file_path).collect();
    let issues = files_with_matches.len();

    println!(
      "\n{} {} files scanned",
      style("🔍"),
      self.scanned_files.len()
    );

    if issues > 0 {
      println!(
        "{} {} files contained potential secrets",
        style("🚨"),
        issues
      );
    }

    println!(
      "{} {} potential secrets found",
      style("🐿️"),
      self.matches.len()
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  fn create_test_config() -> Config {
    let mut config = Config::default();

    // Configure test patterns with exact matches
    config.patterns.insert(
      "test-key".into(),
      Pattern {
        description: Some("Test API Key".into()),
        regex: "^API_KEY=([A-Za-z0-9]+)$".into(),
        severity: "HIGH".into(),
      },
    );
    config.patterns.insert(
      "password".into(),
      Pattern {
        description: Some("Password in file".into()),
        regex: "^password=([^\\s]+)$".into(),
        severity: "MEDIUM".into(),
      },
    );
    config
  }

  fn create_test_files(dir: &TempDir) -> Result<()> {
    // Create each pattern on its own line
    fs::write(
      dir.path().join("config.txt"),
      "API_KEY=abc123\npassword=secret123\n",
    )?;

    // Other test files...
    fs::write(dir.path().join("test.txt"), "TEST_API_KEY=ignored123\n")?;
    fs::write(dir.path().join("clean.txt"), "nothing to see here\n")?;

    Ok(())
  }

  #[test]
  fn test_basic_scan() -> Result<()> {
    let temp = TempDir::new()?;
    create_test_files(&temp)?;

    let config = create_test_config(); // Remove mut as we don't modify it
    let mut scanner = Scanner::new(&config);
    scanner.scan_path(temp.path())?;

    // Debug output
    println!("Found matches: {:#?}", scanner.matches);

    assert_eq!(scanner.matches.len(), 2, "Expected exactly 2 matches");

    // Check exact matches
    let matches: Vec<_> =
      scanner.matches.iter().map(|m| &m.pattern_name).collect();
    assert!(
      matches.contains(&&"test-key".to_string()),
      "Should find test-key pattern"
    );
    assert!(
      matches.contains(&&"password".to_string()),
      "Should find password pattern"
    );

    Ok(())
  }

  #[test]
  fn test_severity_filter() -> Result<()> {
    let temp = TempDir::new()?;
    create_test_files(&temp)?;

    let mut config = create_test_config();

    // First verify we get both matches
    let mut scanner = Scanner::new(&config);
    scanner.scan_path(temp.path())?;
    assert_eq!(
      scanner.matches.len(),
      2,
      "Should find both patterns initially"
    );

    // Clear scanner and set severity
    config.set_severity_filter("high");
    let mut scanner = Scanner::new(&config);
    scanner.scan_path(temp.path())?;

    // Debug output
    println!("Found matches with HIGH filter: {:#?}", scanner.matches);

    assert_eq!(
      scanner.matches.len(),
      1,
      "Should only find HIGH severity pattern"
    );
    assert_eq!(scanner.matches[0].pattern_name, "test-key");
    assert_eq!(scanner.matches[0].pattern.severity, "HIGH");

    Ok(())
  }

  #[test]
  fn test_ignore_patterns() -> Result<()> {
    let temp = TempDir::new()?;
    create_test_files(&temp)?;

    let mut config = create_test_config();
    config.ignore_patterns = Some(vec!["TEST_API_KEY=.*".into()]);

    let mut scanner = Scanner::new(&config);
    scanner.scan_path(temp.path())?;

    assert!(!scanner
      .matches
      .iter()
      .any(|m| m.line.contains("TEST_API_KEY")));

    Ok(())
  }

  #[test]
  fn test_ignore_paths() -> Result<()> {
    let temp = TempDir::new()?;
    create_test_files(&temp)?;

    // Create a directory that should be ignored
    let ignored_dir = temp.path().join("tests");
    fs::create_dir(&ignored_dir)?;
    fs::write(
      ignored_dir.join("test.txt"),
      "API_KEY=should_not_find_this\n",
    )?;

    let mut config = create_test_config();
    config.ignore_paths = Some(vec!["tests/*".into()]);

    let mut scanner = Scanner::new(&config);
    scanner.scan_path(temp.path())?;

    assert!(!scanner
      .matches
      .iter()
      .any(|m| m.file_path.contains("tests/")));

    Ok(())
  }
}
