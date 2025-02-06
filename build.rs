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
    std::env::var("APPDATA")
      .ok()
      .map(|appdata| PathBuf::from(appdata).join("secret-squirrel"))
  } else {
    std::env::var("HOME")
      .ok()
      .map(|home| PathBuf::from(home).join(".config").join("secret-squirrel"))
  }
}

fn main() {
  if let Some(config_dir) = get_config_dir() {
    let config_src = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
      .join("config")
      .join("ssq.yml");

    if !config_src.exists() {
      println!(
        "cargo:warning=Source config file not found at: {}",
        config_src.display()
      );
      return;
    }

    println!(
      "cargo:warning=Installing config to: {}",
      config_dir.display()
    );

    if let Err(e) = fs::create_dir_all(&config_dir) {
      println!("cargo:warning=Failed to create config directory: {e}");
      return;
    }

    let config_dest = config_dir.join("config.yml");

    match fs::copy(&config_src, &config_dest) {
      Ok(_) => println!("cargo:warning=Config file installed successfully"),
      Err(e) => println!("cargo:warning=Failed to copy config file: {e}"),
    }
  }
}
