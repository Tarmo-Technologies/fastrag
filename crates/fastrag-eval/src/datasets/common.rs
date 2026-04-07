use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use indicatif::{ProgressBar, ProgressStyle};
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::{EvalError, EvalResult};

pub fn cache_root(name: &str) -> EvalResult<PathBuf> {
    let base = dirs::cache_dir().ok_or(EvalError::NoCacheDir)?;
    let root = base.join("fastrag/eval-datasets").join(name);
    fs::create_dir_all(&root)?;
    Ok(root)
}

pub fn read_jsonl<T>(path: &Path) -> EvalResult<Vec<T>>
where
    T: DeserializeOwned,
{
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut rows = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value = serde_json::from_str(line).map_err(|err| {
            EvalError::MalformedDataset(format!(
                "failed to parse JSONL row {} in {}: {}",
                index + 1,
                path.display(),
                err
            ))
        })?;
        rows.push(value);
    }
    Ok(rows)
}

pub fn read_gz_json<T>(path: &Path) -> EvalResult<T>
where
    T: DeserializeOwned,
{
    let file = File::open(path)?;
    let mut decoder = GzDecoder::new(file);
    let mut raw = String::new();
    decoder
        .read_to_string(&mut raw)
        .map_err(|err| EvalError::Gzip(err.to_string()))?;
    let value = serde_json::from_str(&raw)?;
    Ok(value)
}

pub fn sha256_file(path: &Path) -> EvalResult<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn download_to_path(url: &str, dest: &Path, expected_sha256: Option<&str>) -> EvalResult<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|err| EvalError::Http(err.to_string()))?;
    let mut response = client
        .get(url)
        .send()
        .map_err(|err| EvalError::Http(err.to_string()))?;
    if !response.status().is_success() {
        return Err(EvalError::Http(format!(
            "{} returned {}",
            url,
            response.status()
        )));
    }

    let tmp = dest.with_extension("tmp");
    let mut file = File::create(&tmp)?;
    let mut hasher = Sha256::new();
    let total = response.content_length();
    let pb = progress_bar(total, url);
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|err| EvalError::Http(err.to_string()))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])?;
        hasher.update(&buffer[..read]);
        pb.inc(read as u64);
    }
    file.flush()?;
    pb.finish_and_clear();

    let got = format!("{:x}", hasher.finalize());
    if let Some(expected) = expected_sha256
        && got != expected
    {
        let _ = fs::remove_file(&tmp);
        return Err(EvalError::ChecksumMismatch {
            path: dest.to_path_buf(),
            expected: expected.to_string(),
            got,
        });
    }

    fs::rename(&tmp, dest)?;
    Ok(())
}

pub fn extract_zip(zip_path: &Path, dest_dir: &Path) -> EvalResult<()> {
    fs::create_dir_all(dest_dir)?;
    let file = File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| EvalError::Archive(err.to_string()))?;
    for index in 0..archive.len() {
        let mut zipped = archive
            .by_index(index)
            .map_err(|err| EvalError::Archive(err.to_string()))?;
        let path = zipped
            .enclosed_name()
            .ok_or_else(|| EvalError::Archive("zip entry has invalid path".to_string()))?;
        let out_path = dest_dir.join(path);
        if zipped.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&out_path)?;
        std::io::copy(&mut zipped, &mut out).map_err(|err| EvalError::Archive(err.to_string()))?;
    }
    Ok(())
}

pub fn find_path(root: &Path, needle: &Path) -> EvalResult<PathBuf> {
    if root.ends_with(needle) && root.is_file() {
        return Ok(root.to_path_buf());
    }

    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.ends_with(needle) {
                return Ok(path);
            }
        }
    }

    Err(EvalError::MalformedDataset(format!(
        "could not find {} under {}",
        needle.display(),
        root.display()
    )))
}

pub fn file_name_from_url(url: &str) -> EvalResult<String> {
    url.rsplit('/')
        .next()
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_string())
        .ok_or_else(|| EvalError::MalformedDataset(format!("invalid URL: {url}")))
}

fn progress_bar(total: Option<u64>, message: &str) -> ProgressBar {
    let pb = match total {
        Some(len) => ProgressBar::new(len),
        None => ProgressBar::new_spinner(),
    };
    let style =
        ProgressStyle::with_template("{msg} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap_or_else(|_| ProgressStyle::default_bar());
    pb.set_style(style);
    pb.set_message(message.to_string());
    pb
}
