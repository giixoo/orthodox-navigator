//! Read-only browsing of archives: zip family (.zip, .jar, .nupkg, .docx, ...), .tar, .tar.gz/.tgz.

use chrono::{DateTime, Local, NaiveDate, TimeZone};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct ArcEntry {
    /// Normalised path inside the archive: "dir/sub/file.txt" (no leading or trailing slash).
    pub path: String,
    /// The name exactly as stored, needed to extract it.
    raw: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<DateTime<Local>>,
}

enum Kind {
    Zip,
    Tar,
    TarGz,
}

const ZIP_EXT: &[&str] = &[
    ".zip", ".jar", ".war", ".ear", ".apk", ".nupkg", ".vsix", ".whl", ".xpi", ".epub", ".docx", ".xlsx", ".pptx", ".odt", ".ods",
];

fn kind(name: &str) -> Option<Kind> {
    let n = name.to_lowercase();
    if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
        Some(Kind::TarGz)
    } else if n.ends_with(".tar") {
        Some(Kind::Tar)
    } else if ZIP_EXT.iter().any(|e| n.ends_with(e)) {
        Some(Kind::Zip)
    } else {
        None
    }
}

pub fn is_archive(name: &str) -> bool {
    kind(name).is_some()
}

fn clean(name: &str) -> String {
    let s = name.replace('\\', "/");
    let s = s.trim_start_matches("./").trim_start_matches('/').trim_end_matches('/');
    s.to_string()
}

fn zip_time(d: zip::DateTime) -> Option<DateTime<Local>> {
    let n = NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32)?
        .and_hms_opt(d.hour() as u32, d.minute() as u32, d.second().min(59) as u32)?;
    Local.from_local_datetime(&n).single()
}

fn tar_reader(file: &Path, gz: bool) -> io::Result<tar::Archive<Box<dyn Read>>> {
    let f = BufReader::new(File::open(file)?);
    let r: Box<dyn Read> = if gz { Box::new(flate2::read::GzDecoder::new(f)) } else { Box::new(f) };
    Ok(tar::Archive::new(r))
}

pub fn list(file: &Path) -> io::Result<Vec<ArcEntry>> {
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut out = Vec::new();
    match kind(&name) {
        Some(Kind::Zip) => {
            let mut z = zip::ZipArchive::new(BufReader::new(File::open(file)?)).map_err(io::Error::other)?;
            for i in 0..z.len() {
                let f = z.by_index_raw(i).map_err(io::Error::other)?;
                let raw = f.name().to_string();
                let path = clean(&raw);
                if path.is_empty() {
                    continue;
                }
                out.push(ArcEntry { path, raw, is_dir: f.is_dir(), size: f.size(), modified: f.last_modified().and_then(zip_time) });
            }
        }
        Some(Kind::Tar) | Some(Kind::TarGz) => {
            let mut a = tar_reader(file, matches!(kind(&name), Some(Kind::TarGz)))?;
            for e in a.entries()? {
                let e = e?;
                let raw = e.path()?.to_string_lossy().to_string();
                let path = clean(&raw);
                if path.is_empty() {
                    continue;
                }
                let h = e.header();
                out.push(ArcEntry {
                    path,
                    raw,
                    is_dir: h.entry_type().is_dir(),
                    size: h.size().unwrap_or(0),
                    modified: h.mtime().ok().and_then(|t| Local.timestamp_opt(t as i64, 0).single()),
                });
            }
        }
        None => return Err(io::Error::other("not a supported archive")),
    }
    Ok(out)
}

/// Extract several entries in one pass. `progress` is called before each file; return false to stop.
pub fn extract_many(file: &Path, jobs: &[(ArcEntry, PathBuf)], mut progress: impl FnMut(&str) -> bool) -> io::Result<()> {
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    for (_, dst) in jobs {
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
    }
    match kind(&name) {
        Some(Kind::Zip) => {
            let mut z = zip::ZipArchive::new(BufReader::new(File::open(file)?)).map_err(io::Error::other)?;
            for (e, dst) in jobs {
                if !progress(&e.path) {
                    break;
                }
                let mut f = z.by_name(&e.raw).map_err(io::Error::other)?;
                io::copy(&mut f, &mut File::create(dst)?)?;
            }
        }
        Some(Kind::Tar) | Some(Kind::TarGz) => {
            let wanted: HashMap<&str, &PathBuf> = jobs.iter().map(|(e, d)| (e.raw.as_str(), d)).collect();
            let mut a = tar_reader(file, matches!(kind(&name), Some(Kind::TarGz)))?;
            for e in a.entries()? {
                let mut e = e?;
                let raw = e.path()?.to_string_lossy().to_string();
                if let Some(dst) = wanted.get(raw.as_str()) {
                    if !progress(&clean(&raw)) {
                        break;
                    }
                    io::copy(&mut e, &mut File::create(dst)?)?;
                }
            }
        }
        None => return Err(io::Error::other("not a supported archive")),
    }
    Ok(())
}

/// Extract one file into a fresh temporary folder (for F3 / Enter inside an archive).
pub fn extract_to_temp(file: &Path, e: &ArcEntry) -> io::Result<PathBuf> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join("orthodox-navigator").join(format!("{}-{stamp}", std::process::id()));
    let name = e.path.rsplit('/').next().unwrap_or("file");
    let dst = dir.join(name);
    extract_many(file, &[(e.clone(), dst.clone())], |_| true)?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn zip_roundtrip() {
        let dir = std::env::temp_dir().join(format!("on-arc-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("t.zip");
        {
            let mut w = zip::ZipWriter::new(File::create(&file).unwrap());
            let o = zip::write::SimpleFileOptions::default();
            w.add_directory("docs/", o).unwrap();
            w.start_file("docs/readme.txt", o).unwrap();
            w.write_all(b"hello").unwrap();
            w.start_file("top.txt", o).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let l = list(&file).unwrap();
        assert!(l.iter().any(|e| e.path == "docs" && e.is_dir));
        let readme = l.iter().find(|e| e.path == "docs/readme.txt").unwrap();
        let out = extract_to_temp(&file, readme).unwrap();
        assert_eq!(fs::read_to_string(out).unwrap(), "hello");
        let _ = fs::remove_dir_all(dir);
    }
}
