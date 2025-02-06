use std::path::PathBuf;

fn is_wsl() -> bool {
  std::fs::read_to_string("/proc/version")
    .map(|s| s.to_lowercase().contains("microsoft"))
    .unwrap_or(false)
}

/// Returns the config directory path based on platform:
/// - Windows (not WSL): %APPDATA%/secret-squirrel
/// - macOS: ~/.config/secret-squirrel
/// - Linux: ~/.config/secret-squirrel
/// - WSL: ~/.config/secret-squirrel
pub fn get_config_dir() -> Option<PathBuf> {
  if cfg!(windows) && !is_wsl() {
    // Windows-specific path (not in WSL)
    std::env::var("APPDATA")
      .ok()
      .map(|appdata| PathBuf::from(appdata).join("secret-squirrel"))
  } else {
    // Linux-style path for Linux, macOS, and WSL
    std::env::var("HOME")
      .ok()
      .map(|home| PathBuf::from(home).join(".config").join("secret-squirrel"))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::env;

  #[test]
  fn test_windows_path() {
    if cfg!(windows) && !is_wsl() {
      env::set_var("APPDATA", r"C:\Users\test\AppData\Roaming");
      assert_eq!(
        get_config_dir().unwrap(),
        PathBuf::from(r"C:\Users\test\AppData\Roaming\secret-squirrel")
      );
    }
  }

  #[test]
  fn test_unix_style_path() {
    if !cfg!(windows) || is_wsl() {
      env::set_var("HOME", "/home/user");
      assert_eq!(
        get_config_dir().unwrap(),
        PathBuf::from("/home/user/.config/secret-squirrel")
      );
    }
  }
}
