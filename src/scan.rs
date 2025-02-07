use crate::config::{Config, Pattern};
use anyhow::Result;
use console::style;
use crossterm::{
  cursor, queue,
  terminal::{Clear, ClearType},
};
use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashSet;
use std::io::{stdout, Write};
use std::path::Path;

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
  patterns_count: usize,
}

impl<'a> Scanner<'a> {
  pub fn new(config: &'a Config) -> Self {
    Self {
      config,
      matches: Vec::new(),
      scanned_files: HashSet::new(),
      patterns_count: config.patterns.len(),
    }
  }
}

struct ScanProgress {
  multi: MultiProgress,
  completed: Vec<String>,
  has_matches: Vec<bool>,
  total_files: usize, // Change from Option<usize> to usize
  processed_files: usize,
}

impl ScanProgress {
  fn new(path: &Path) -> Result<Self> {
    // Clear screen and hide cursor
    let mut stdout = stdout();
    queue!(
      stdout,
      Clear(ClearType::All),
      cursor::MoveTo(0, 0),
      cursor::Hide,
    )?;
    stdout.flush()?;

    // Count total files before starting
    let total_files = WalkBuilder::new(path)
      .hidden(false)
      .ignore(true)
      .git_ignore(true)
      .build()
      .filter_map(Result::ok)
      .filter(|e| e.path().is_file())
      .count();

    let progress = Self {
      multi: MultiProgress::new(),
      completed: Vec::with_capacity(total_files),
      has_matches: Vec::with_capacity(total_files),
      total_files,
      processed_files: 0,
    };

    // Print initial state
    progress.print_summary();

    Ok(progress)
  }

  fn start_file(
    &mut self,
    file_path: &str,
    patterns_count: usize,
  ) -> ProgressBar {
    let pb = self.multi.add(ProgressBar::new(patterns_count as u64));
    pb.set_style(
      ProgressStyle::default_bar()
        .template("{spinner:.green} {prefix:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
        .expect("Failed to set progress bar template")
        .progress_chars("=> "),
    );
    pb.set_prefix(format!("⟳ {file_path}"));
    pb
  }

  fn complete_file(
    &mut self,
    file_path: String,
    has_match: bool,
    pb: &ProgressBar,
  ) {
    self.processed_files += 1;
    self.completed.push(file_path);
    self.has_matches.push(has_match);
    pb.finish_and_clear();
    self.print_summary();
  }

  fn print_summary(&self) {
    // Move to top of screen and print summary
    let mut stdout = stdout();
    queue!(
      stdout,
      cursor::MoveTo(0, 0),
      Clear(ClearType::FromCursorDown),
    )
    .unwrap();
    stdout.flush().unwrap();

    println!(
      "Progress: {}/{} files\n",
      self.processed_files, self.total_files
    );

    // Print completed files
    println!("Completed files:");
    println!("---------------");
    for (path, has_match) in self.completed.iter().zip(self.has_matches.iter())
    {
      let status = if *has_match { "❌" } else { "✓" };
      println!("{status} {path}");
    }
    println!(); // Space for active scans
  }

  fn finish(self) -> Result<()> {
    // Show cursor again
    let mut stdout = stdout();
    queue!(stdout, cursor::Show)?;
    stdout.flush()?;

    // Final summary
    let issues = self
      .has_matches
      .iter()
      .filter(|&&has_match| has_match)
      .count();
    println!(
      "\n{} {} files scanned",
      style("✓").green(),
      self.total_files
    );
    if issues > 0 {
      println!(
        "{} {} files contained potential secrets",
        style("!").red(),
        issues
      );
    }

    Ok(())
  }
}

impl Scanner<'_> {
  #[allow(clippy::too_many_lines)]
  pub fn scan_path(&mut self, path: &Path) -> Result<()> {
    let mut progress = ScanProgress::new(path)?;
    let mut searcher = SearcherBuilder::new()
      .binary_detection(BinaryDetection::quit(b'\x00'))
      .line_number(true)
      .build();

    let walker = WalkBuilder::new(path)
      .hidden(false)
      .ignore(true)
      .git_ignore(true)
      .build();

    let ignore_pattern_matcher =
      if let Some(ref patterns) = self.config.ignore_patterns {
        Some(RegexMatcher::new(&patterns.join("|"))?)
      } else {
        None
      };

    let ignore_path_matcher = if let Some(ref paths) = self.config.ignore_paths
    {
      Some(RegexMatcher::new(&paths.join("|"))?)
    } else {
      None
    };

    for result in walker {
      let entry = result?;
      let path = entry.path();
      if !path.is_file() {
        continue;
      }

      let file_path = path.display().to_string();

      let pb = progress.start_file(&file_path, self.patterns_count);

      if let Some(ref matcher) = ignore_path_matcher {
        if matcher.is_match(path.to_string_lossy().as_bytes())? {
          progress.complete_file(file_path.clone(), false, &pb);
          continue;
        }
      }

      let mut found_match = false;

      for (name, pattern) in &self.config.patterns {
        if !self.config.meets_severity(pattern) {
          pb.inc(1);
          continue;
        }

        pb.set_message(format!("checking {name}"));

        let matcher = RegexMatcher::new(&pattern.regex)?;
        searcher.search_path(
          &matcher,
          path,
          UTF8(|line_number, line| {
            if let Some(ref ignore) = ignore_pattern_matcher {
              if ignore.is_match(line.as_bytes()).unwrap_or(false) {
                return Ok(true);
              }
            }

            found_match = true;
            self.matches.push(Match {
              pattern_name: name.clone(),
              file_path: path.to_string_lossy().to_string(),
              line_number,
              line: line.to_string(),
              pattern: Pattern {
                description: pattern.description.clone(),
                regex: pattern.regex.clone(),
                severity: pattern.severity.clone(),
              },
            });
            Ok(true)
          }),
        )?;

        pb.inc(1);
      }

      progress.complete_file(file_path.clone(), found_match, &pb);
      self.scanned_files.insert(file_path);
    }

    progress.finish()?;

    Ok(())
  }

  pub fn print_results(&self) {
    if self.matches.is_empty() {
      println!("\n{}", style("No matches found.").green());
      return;
    }

    println!("\n{}", style("Matches found:").red().bold());
    println!("{}", style("==============").red());

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

      // Highlight location with cyan color and make line number bold
      println!(
        "{} {}:{}",
        style("Location:").bold(),
        style(&m.file_path).cyan(),
        style(m.line_number).cyan().bold()
      );

      // Show matched line with the matched portion highlighted
      println!("{} {}", style("Match:").bold(), style(m.line.trim()).dim());
    }

    println!(
      "\n{} {} potential secrets found",
      style("WARNING:").red().bold(),
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
        regex: "^API_KEY=([A-Za-z0-9]+)$".into(), // More precise regex
        severity: "HIGH".into(),
      },
    );
    config.patterns.insert(
      "password".into(),
      Pattern {
        description: Some("Password in file".into()),
        regex: "^password=([^\\s]+)$".into(), // More precise regex
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
