use std::io::{stdout, Write};

use anyhow::Result;
use ratatui::{
  crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{
      disable_raw_mode, enable_raw_mode, EnterAlternateScreen,
      LeaveAlternateScreen,
    },
  },
  layout::{Constraint, Direction, Layout},
  style::{Color, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Paragraph},
  Frame, Terminal,
};

const MIN_PATH_WIDTH: usize = 20;
const PROGRESS_WIDTH: usize = 12; // [███░░░░░] 99/99
const SPINNER_WIDTH: usize = 2; // "⟳ "
const SPACING: usize = 2; // spaces between columns

pub struct ScanUI {
  terminal: Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
  total_files: usize,
  processed_files: usize,
  problem_files: Vec<String>,
  active_scans: Vec<(String, String, f32)>, // (path, message, progress)
}

impl ScanUI {
  pub fn cleanup() {
    let mut stdout = stdout();
    let _ = disable_raw_mode();
    let _ = execute!(stdout, LeaveAlternateScreen, Show);
    let _ = stdout.flush();
  }

  pub fn new(total_files: usize) -> Result<Self> {
    let backend = ratatui::backend::CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;

    Ok(Self {
      terminal,
      total_files,
      processed_files: 0,
      problem_files: Vec::new(),
      active_scans: Vec::new(),
    })
  }

  pub fn render(&mut self) -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, Hide)?;

    self.terminal.draw(|f| {
      Self::draw_frame(
        f,
        self.total_files,
        self.processed_files,
        &self.problem_files,
        &self.active_scans,
      );
    })?;

    disable_raw_mode()?;
    execute!(stdout(), Show)?;

    Ok(())
  }

  fn draw_frame(
    f: &mut Frame,
    total_files: usize,
    processed_files: usize,
    problem_files: &[String],
    active_scans: &[(String, String, f32)],
  ) {
    let area = f.area();

    // Calculate optimal layout based on content
    let has_problems = !problem_files.is_empty();
    let problems_height = if has_problems {
      // Header (2) + files + padding (1)
      (problem_files.len() + 3).min((area.height as usize / 2).max(6))
    } else {
      0
    };

    // Create main layout
    let chunks = Layout::default()
      .direction(Direction::Vertical)
      .margin(1)
      .constraints(if has_problems {
        vec![
          #[allow(clippy::cast_possible_truncation)]
          Constraint::Length(problems_height as u16),
          Constraint::Min(4),
        ]
      } else {
        vec![Constraint::Min(4)]
      })
      .split(area);

    // Draw problem files section if any exist
    if has_problems {
      let problems: Vec<Line> = problem_files
        .iter()
        .map(|path| {
          Line::from(vec![
            Span::styled("● ", Style::default().fg(Color::Red)),
            Span::raw(path),
          ])
        })
        .collect();

      f.render_widget(
        Paragraph::new(problems).block(
          Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red))
            .title("Files with potential secrets:"),
        ),
        chunks[0],
      );
    }

    // Calculate max message length
    let msg_width = active_scans
      .iter()
      .map(|(_, msg, _)| msg.len())
      .max()
      .unwrap_or(20);

    // Calculate optimal column widths
    let total_width = f.area().width as usize;
    let available_path_width = total_width
      .saturating_sub(SPINNER_WIDTH + PROGRESS_WIDTH + msg_width + SPACING * 3);
    let path_width = std::cmp::max(MIN_PATH_WIDTH, available_path_width);

    // Draw active scans in remaining space
    let scans: Vec<Line> = active_scans
      .iter()
      .map(|(path, msg, progress)| {
        let width = 10;
        #[allow(
          clippy::cast_possible_truncation,
          clippy::cast_sign_loss,
          clippy::cast_precision_loss
        )]
        let filled = (progress * width as f32) as usize;
        let bar =
          format!("[{}{}]", "█".repeat(filled), "░".repeat(width - filled));

        // Create fixed-width columns using max message width
        let path_part = truncate_path(path, path_width);
        let status_part = format!("{msg:<msg_width$}");

        Line::from(vec![
          Span::styled("⟳ ", Style::default().fg(Color::Green)),
          Span::styled(
            format!("{path_part:<path_width$}"),
            Style::default().fg(Color::Cyan),
          ),
          Span::raw(" "),
          Span::raw(status_part),
          Span::raw(" "),
          Span::styled(bar, Style::default().fg(Color::Blue)),
        ])
      })
      .collect();

    f.render_widget(
      Paragraph::new(scans).block(
        Block::default()
          .borders(Borders::ALL)
          .border_style(Style::default())
          .title(Line::from("Active Scans").left_aligned())
          .title(
            Line::from(format!("Progress: {processed_files}/{total_files}"))
              .right_aligned(),
          ),
      ),
      if has_problems { chunks[1] } else { chunks[0] },
    );
  }

  pub fn add_problem_file(&mut self, path: String) {
    self.problem_files.push(path);
    self.render().unwrap();
  }

  pub fn update_scan(&mut self, path: String, message: String, progress: f32) {
    if let Some(scan) = self.active_scans.iter_mut().find(|(p, ..)| p == &path)
    {
      *scan = (path, message, progress);
    } else {
      self.active_scans.push((path, message, progress));
    }
    self.render().unwrap();
  }

  pub fn complete_scan(&mut self, path: &str) {
    self.processed_files += 1;
    self.active_scans.retain(|(p, ..)| p != path);
    self.render().unwrap();
  }
}

impl Drop for ScanUI {
  fn drop(&mut self) {
    Self::cleanup();
  }
}

fn truncate_path(path: &str, max_len: usize) -> String {
  if path.len() <= max_len {
    path.to_string()
  } else {
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() > 2 {
      let end = parts.last().unwrap_or(&"");
      format!(".../{end}")
    } else {
      format!("...{}", &path[path.len().saturating_sub(max_len - 3)..])
    }
    .truncate(max_len)
  }
}

// Add String extension trait for truncation
trait StringExt {
  fn truncate(&self, max_len: usize) -> String;
}

impl StringExt for String {
  fn truncate(&self, max_len: usize) -> String {
    if self.len() <= max_len {
      self.clone()
    } else {
      format!("{}...", &self[..max_len.saturating_sub(3)])
    }
  }
}
