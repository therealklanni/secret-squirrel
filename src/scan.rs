use crate::config::{Config, Pattern};
use anyhow::Result;
use console::style;
use crossterm::terminal;
use crossterm::{
  cursor, queue,
  terminal::{Clear, ClearType},
};
use grep_matcher::Matcher;
use grep_regex::RegexMatcher;
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::WalkBuilder;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use memmap2::Mmap;
use parking_lot::Mutex;
use rayon::prelude::*;
use regex::Regex;
use std::collections::HashSet;
use std::io::{stdout, Write};
use std::path::Path;
use std::sync::Arc;

const LARGE_FILE_THRESHOLD: u64 = 1024 * 1024; // 1MB
const BINARY_CHECK_BYTES: usize = 512; // Check first 512 bytes for binary content
const MAX_CONCURRENT_SCANS: usize = 6; // Limit parallel scans
const MIN_HEADER_LINES: usize = 6; // Minimum lines needed for header

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

#[derive(Debug)]
struct ScanProgress {
  multi: MultiProgress,
  completed: Vec<String>,
  has_matches: Vec<bool>,
  total_files: usize, // Change from Option<usize> to usize
  processed_files: usize,
}

impl ScanProgress {
  fn new(path: &Path, ignore_matcher: Option<&Gitignore>) -> Result<Self> {
    // Clear screen and hide cursor
    let mut stdout = stdout();
    queue!(
      stdout,
      Clear(ClearType::All),
      cursor::MoveTo(0, 0),
      cursor::Hide,
    )?;
    stdout.flush()?;

    // Build walker
    let walker = WalkBuilder::new(path)
      .hidden(false)
      .ignore(true)
      .git_ignore(true)
      .build();

    // Count total files, excluding ignored paths
    let total_files = walker
      .filter_map(Result::ok)
      .filter(|e| {
        let path = e.path();
        path.is_file()
          && ignore_matcher
            .map_or(true, |m| !m.matched(path, false).is_ignore())
      })
      .count();

    let progress = Self {
      multi: MultiProgress::new(),
      completed: Vec::with_capacity(total_files),
      has_matches: Vec::with_capacity(total_files),
      total_files,
      processed_files: 0,
    };

    progress.print_summary();
    Ok(progress)
  }

  fn start_file(
    &mut self,
    file_path: &str,
    patterns_count: usize,
  ) -> ProgressBar {
    // Reserve space for: "⟳ " + path + " [=>] 99/99 checking-very-long-pattern-name"
    const RESERVED_SPACE: usize = 50;

    let pb = self.multi.add(ProgressBar::new(patterns_count as u64));

    // Get terminal width and calculate max path length
    let term_width = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let max_path_len = term_width.saturating_sub(RESERVED_SPACE);

    // More aggressive path truncation
    let display_path = if file_path.len() > max_path_len {
      let parts: Vec<&str> = file_path.split('/').collect();
      if parts.len() > 2 {
        // Only keep last path segment
        format!(".../{}", parts.last().unwrap_or(&""))
      } else {
        // Fallback to simple truncation for short paths
        format!(
          "...{}",
          &file_path[file_path.len().saturating_sub(max_path_len - 3)..]
        )
      }
    } else {
      file_path.to_string()
    };

    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} {prefix:.cyan} [{bar:10.cyan/blue}] {pos}/{len} {msg}")
            .expect("Failed to set progress bar template")
            .progress_chars("=>-"),
    );
    pb.set_prefix(format!("⟳ {display_path}"));
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
    let mut stdout = stdout();
    let problem_files: Vec<_> = self
      .completed
      .iter()
      .zip(self.has_matches.iter())
      .filter(|(_, &has_match)| has_match)
      .collect();

    // Move to top and clear screen
    queue!(stdout, cursor::MoveTo(0, 0), Clear(ClearType::All),).unwrap();

    // Print header content
    println!(
      "Progress: {}/{} files",
      self.processed_files, self.total_files
    );

    if !problem_files.is_empty() {
      println!("\nProblematic files:");
      println!("──────────────────");
      for (path, _) in problem_files {
        println!(" {} {}", style("●").red(), path);
      }
    }

    // Only add one blank line before progress bars
    println!();

    stdout.flush().unwrap();
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
    println!("\n{} {} files scanned", style("🔍"), self.total_files);
    if issues > 0 {
      println!(
        "{} {} files contained potential secrets",
        style("🚨").red().bold(),
        issues
      );
    }

    Ok(())
  }
}

struct CompiledPattern {
  name: String,
  pattern: Pattern,
  regex: Regex,
}

impl Scanner<'_> {
  #[allow(clippy::too_many_lines)]
  pub fn scan_path(&mut self, path: &Path) -> Result<()> {
    // Pre-compile all regex patterns
    let patterns: Vec<CompiledPattern> = self
      .config
      .patterns
      .iter()
      .filter(|(_, p)| self.config.meets_severity(p))
      .map(|(name, pattern)| {
        Ok(CompiledPattern {
          name: name.clone(),
          pattern: pattern.clone(),
          regex: Regex::new(&pattern.regex)?,
        })
      })
      .collect::<Result<Vec<_>>>()?;

    // Build gitignore-style matcher for ignore_paths first
    let ignore_matcher = if let Some(ref paths) = self.config.ignore_paths {
      let mut builder = GitignoreBuilder::new(path);
      for pattern in paths {
        builder.add_line(None, pattern)?;
      }
      Some(builder.build()?)
    } else {
      None
    };

    let progress = ScanProgress::new(path, ignore_matcher.as_ref())?;
    let ignore_pattern_matcher =
      if let Some(ref patterns) = self.config.ignore_patterns {
        Some(RegexMatcher::new(&patterns.join("|"))?)
      } else {
        None
      };

    // Create walker filtered by ignore paths
    let walker = WalkBuilder::new(path)
      .hidden(false)
      .ignore(true)
      .git_ignore(true)
      .build()
      .filter_map(Result::ok)
      .filter(|e| {
        let path = e.path();
        path.is_file()
          && ignore_matcher
            .as_ref()
            .map_or(true, |m| !m.matched(path, false).is_ignore())
      });

    // Create thread-safe progress and matches
    let progress = Arc::new(Mutex::new(progress));
    let matches = Arc::new(Mutex::new(Vec::new()));
    let scanned_files = Arc::new(Mutex::new(HashSet::new()));

    // Collect files first to avoid directory traversal overhead in parallel
    let files: Vec<_> = walker
      .map(|entry: ignore::DirEntry| Some(entry))
      .filter(|e| {
        let path = e.as_ref().unwrap().path();
        path.is_file()
          && ignore_matcher
            .as_ref()
            .map_or(true, |m| !m.matched(path, false).is_ignore())
      })
      .collect();

    // Get terminal height and calculate max concurrent scans
    let (_, term_height) = terminal::size()?;
    let max_scans = ((term_height as usize).saturating_sub(MIN_HEADER_LINES))
      .min(MAX_CONCURRENT_SCANS);

    // Process files in batches
    for chunk in files.chunks(max_scans) {
      chunk.into_par_iter().for_each(|entry| {
        let binding = entry.as_ref().unwrap();
        let path = binding.path();
        let file_path = path.display().to_string();

        // Quick binary check and size check
        if let Ok(metadata) = path.metadata() {
          // Skip large files
          if metadata.len() > LARGE_FILE_THRESHOLD {
            if let Ok(file) = std::fs::File::open(path) {
              if let Ok(mmap) = unsafe { Mmap::map(&file) } {
                let pb = progress.lock().start_file(&file_path, patterns.len());
                let mut found_match = false;
                for (i, line) in
                  String::from_utf8_lossy(&mmap).lines().enumerate()
                {
                  for pattern in &patterns {
                    if pattern.regex.is_match(line) {
                      found_match = true;
                      matches.lock().push(Match {
                        pattern_name: pattern.name.clone(),
                        file_path: path.to_string_lossy().to_string(),
                        line_number: (i + 1) as u64,
                        line: line.to_string(),
                        pattern: pattern.pattern.clone(),
                      });
                    }
                    pb.inc(1);
                  }
                }
                pb.finish_with_message(if found_match {
                  "found matches"
                } else {
                  "clean"
                });
                return;
              }
            }
          }

          // Quick binary check
          if let Ok(file) = std::fs::File::open(path) {
            use std::io::Read;
            let mut buffer = vec![0; BINARY_CHECK_BYTES];
            if file
              .take(BINARY_CHECK_BYTES as u64)
              .read(&mut buffer)
              .is_ok()
              && buffer.iter().any(|&b| b == 0)
            {
              return; // Skip binary files
            }
          }
        }

        let pb = progress.lock().start_file(&file_path, patterns.len());
        let mut found_match = false;

        // Regular file scanning with compiled patterns
        let mut searcher = SearcherBuilder::new()
          .binary_detection(BinaryDetection::quit(b'\x00'))
          .line_number(true)
          .build();

        for pattern in &patterns {
          pb.set_message(format!("checking {}", pattern.name));

          if let Ok(()) = searcher.search_path(
            grep_regex::RegexMatcher::new(pattern.regex.as_str()).unwrap(),
            path,
            UTF8(|line_number, line| {
              if let Some(ref ignore) = &ignore_pattern_matcher {
                if ignore.is_match(line.as_bytes()).unwrap_or(false) {
                  return Ok(true);
                }
              }

              found_match = true;
              matches.lock().push(Match {
                pattern_name: pattern.name.clone(),
                file_path: path.to_string_lossy().to_string(),
                line_number,
                line: line.to_string(),
                pattern: pattern.pattern.clone(),
              });
              Ok(true) // Continue searching for more matches
            }),
          ) {}

          pb.inc(1);
        }

        progress
          .lock()
          .complete_file(file_path.clone(), found_match, &pb);
        scanned_files.lock().insert(file_path);
      });

      // Small pause between batches to allow screen updates
      std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Move results back to scanner
    self.matches = Arc::try_unwrap(matches)
      .expect("Matches still have multiple owners")
      .into_inner();
    self.scanned_files = Arc::try_unwrap(scanned_files)
      .expect("Scanned files still have multiple owners")
      .into_inner();

    Arc::try_unwrap(progress)
      .expect("Progress still has multiple owners")
      .into_inner()
      .finish()?;

    Ok(())
  }

  pub fn print_results(&self) {
    if self.matches.is_empty() {
      println!("\n{}", style("No matches found.").green());
      return;
    }

    println!("\n{}", style("Matches found:").red().bold());
    println!("{}", style("══════════════").red());

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
