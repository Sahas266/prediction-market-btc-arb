use anyhow::{Context, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

pub fn ensure_data_dirs() -> Result<()> {
    for path in [
        "data/analysis",
        "data/chainlink",
        "data/historical",
        "data/live",
        "data/paper",
        "data/replay",
        "data/runs",
        "scripts",
        "tests/fixtures",
    ] {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

pub fn append_jsonl<P: AsRef<Path>, T: Serialize>(path: P, value: &T) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(value)?)?;
    Ok(())
}

pub fn write_json<P: AsRef<Path>, T: Serialize>(path: P, value: &T) -> Result<()> {
    if let Some(parent) = path.as_ref().parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

pub fn read_jsonl<P: AsRef<Path>, T: DeserializeOwned>(path: P) -> Result<Vec<T>> {
    let file = fs::File::open(path.as_ref())
        .with_context(|| format!("failed to open {}", path.as_ref().display()))?;
    let reader = BufReader::new(file);
    let mut rows = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(&line)?);
    }
    Ok(rows)
}

pub fn resolve_input_path(input: &str) -> PathBuf {
    let path = PathBuf::from(input);
    if path.is_dir() {
        path.join("pair_scans.ndjson")
    } else {
        path
    }
}
