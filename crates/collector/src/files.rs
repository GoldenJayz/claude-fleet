//! Receiving files from other PCs.
//!
//! A dashboard streams the file body to `PUT /v1/files?name=…&from=…`. It is written to a hidden
//! `.part` file in the inbox (default `Downloads\Claude Fleet`) and renamed into place only once
//! the whole body has arrived, so a cancelled or broken transfer never leaves a half file behind.
//! Names are cleaned so a sender can't write outside the inbox or overwrite an existing file, and
//! received files get Windows' "downloaded from the internet" mark, so SmartScreen and Office
//! Protected View treat them like downloads.

use std::path::{Path, PathBuf};

/// Keep at least this much free on the inbox drive.
pub const FREE_SPACE_MARGIN: u64 = 1024 * 1024 * 1024;

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// A safe file name: last path component only, no characters Windows forbids, no reserved device
/// names, no trailing dots or spaces, at most 180 characters.
pub fn sanitize_name(raw: &str) -> String {
    let last = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let mut s: String = last
        .chars()
        .map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c })
        .collect();
    s = s.trim().trim_end_matches(['.', ' ']).to_string();
    if s.is_empty() {
        s = "file".into();
    }
    let stem_upper = s.split('.').next().unwrap_or("").trim().to_ascii_uppercase();
    if RESERVED.contains(&stem_upper.as_str()) {
        s.insert(0, '_');
    }
    if s.chars().count() > 180 {
        let (stem, ext) = split_ext(&s);
        let keep = 180usize.saturating_sub(ext.chars().count());
        s = stem.chars().take(keep).collect::<String>() + ext;
    }
    s
}

/// ("report", ".pdf"); a leading dot or no dot means no extension.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && i < name.len() - 1 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

/// `dir\name`, or `dir\name (2).ext`, … if that already exists.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = split_ext(name);
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !p.exists())
        .expect("unbounded")
}

/// Free bytes on the drive that holds `path` (the longest matching mount point).
pub fn free_space(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list().ok()?;
    let target = path.to_string_lossy().to_ascii_lowercase();
    (&disks)
        .into_iter()
        .filter(|d| target.starts_with(&d.mount_point().to_string_lossy().to_ascii_lowercase()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// Mark the file as coming from another computer (Zone 3, like a browser download).
pub fn mark_of_the_web(path: &Path, from: &str) {
    #[cfg(windows)]
    {
        let mut ads = path.as_os_str().to_owned();
        ads.push(":Zone.Identifier");
        let clean: String = from.chars().filter(|c| !c.is_control()).collect();
        let body = format!("[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=claude-fleet://{clean}\r\n");
        if let Err(e) = std::fs::write(PathBuf::from(ads), body) {
            tracing::debug!("couldn't mark {}: {e}", path.display());
        }
    }
    #[cfg(not(windows))]
    let _ = (path, from);
}

/// Hidden temp file next to the final destination.
pub fn part_path(dir: &Path) -> PathBuf {
    let mut b = [0u8; 8];
    rand::fill(&mut b);
    let id: String = b.iter().map(|x| format!("{x:02x}")).collect();
    dir.join(format!(".claude-fleet-{id}.part"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(sanitize_name("report.pdf"), "report.pdf");
        assert_eq!(sanitize_name("..\\..\\Windows\\evil.exe"), "evil.exe");
        assert_eq!(sanitize_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_name("C:\\x\\a:b?.txt"), "a_b_.txt");
        assert_eq!(sanitize_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_name("nul"), "_nul");
        assert_eq!(sanitize_name("trailing. . "), "trailing");
        assert_eq!(sanitize_name(""), "file");
        assert_eq!(sanitize_name(".."), "file");
        assert_eq!(sanitize_name(".gitignore"), ".gitignore");
        let long = format!("{}.zip", "a".repeat(400));
        let s = sanitize_name(&long);
        assert_eq!(s.chars().count(), 180);
        assert!(s.ends_with(".zip"));
    }

    #[test]
    fn unique() {
        let dir = std::env::temp_dir().join(format!("cf-uniq-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a.txt"));
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a (2).txt"));
        std::fs::write(dir.join("a (2).txt"), "x").unwrap();
        assert_eq!(unique_path(&dir, "a.txt"), dir.join("a (3).txt"));
        assert_eq!(unique_path(&dir, "noext"), dir.join("noext"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
