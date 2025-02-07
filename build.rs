use std::env;
use std::fs;
use std::path::PathBuf;

fn is_wsl() -> bool {
  std::fs::read_to_string("/proc/version")
    .map(|s| s.to_lowercase().contains("microsoft"))
    .unwrap_or(false)
}

fn get_config_dir() -> Option<PathBuf> {
  if cfg!(windows) && !is_wsl() {
    env::var("APPDATA")
      .ok()
      .map(|appdata| PathBuf::from(appdata).join("secret-squirrel"))
  } else {
    env::var("HOME")
      .ok()
      .map(|home| PathBuf::from(home).join(".config").join("secret-squirrel"))
  }
}

fn main() {
  let manifest_dir =
    env::var("CARGO_MANIFEST_DIR").expect("Failed to get manifest dir");
  let config_src = PathBuf::from(&manifest_dir).join("config").join("ssq.yml");

  assert!(
    config_src.exists(),
    "Source config not found at: {}",
    config_src.display()
  );

  if let Some(config_dir) = get_config_dir() {
    fs::create_dir_all(&config_dir).expect("Failed to create config directory");
    let config_dest = config_dir.join("config.yml");

    fs::copy(&config_src, &config_dest).expect("Failed to copy config file");
  } else {
    panic!("Could not determine config directory");
  }
}
