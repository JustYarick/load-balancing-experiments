//! CSV and JSON output.

use std::path::Path;

pub fn ensure_parent(path: &str) -> std::io::Result<()> {
    if let Some(dir) = Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    Ok(())
}

pub fn write_csv(path: &str, header: &[&str], rows: &[Vec<String>]) -> std::io::Result<()> {
    ensure_parent(path)?;
    let mut s = String::with_capacity(rows.len() * 64);
    s.push_str(&header.join(","));
    s.push('\n');
    for r in rows {
        s.push_str(&r.join(","));
        s.push('\n');
    }
    std::fs::write(path, s)
}

pub fn write_text(path: &str, text: &str) -> std::io::Result<()> {
    ensure_parent(path)?;
    std::fs::write(path, text)
}
