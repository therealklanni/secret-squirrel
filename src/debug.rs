pub fn debug(msg: &str) {
  if std::env::var("DEBUG").is_ok() {
    eprintln!("DEBUG: {msg}");
  }
}
