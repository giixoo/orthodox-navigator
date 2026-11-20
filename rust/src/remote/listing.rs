//! Parsing folder listings from servers: `ls -la` (sftp) and FTP `LIST` (Unix or DOS style), and `MLSD`.

use chrono::{DateTime, Local};

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_link: bool,
    pub size: Option<u64>,
    pub modified: Option<DateTime<Local>>,
    pub hidden: bool,
    pub exec: bool,
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// The whitespace-separated words of a line with their byte offsets.
fn words(line: &str) -> Vec<(usize, &str)> {
    let mut out = vec![];
    let mut start = None;
    for (i, c) in line.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push((s, &line[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, &line[s..]));
    }
    out
}

/// What follows word `w` and the whitespace after it: the name, kept exactly (inner and end spaces).
fn rest_after<'a>(line: &'a str, w: (usize, &str)) -> &'a str {
    let end = w.0 + w.1.len();
    let rest = &line[end..];
    // skip one run of separating whitespace
    let skip = rest.len() - rest.trim_start().len();
    &rest[skip..]
}

fn local(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> Option<DateTime<Local>> {
    use chrono::TimeZone;
    Local.with_ymd_and_hms(y, mo, d, h, mi, 0).earliest()
}

fn month(s: &str) -> Option<u32> {
    let s = s.to_lowercase();
    MONTHS.iter().position(|m| *m == s).map(|i| i as u32 + 1)
}

fn is_num(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `ls -la` / FTP LIST in Unix style. "Mon DD HH:MM" means `now_year` (the year before if that would be
/// more than a day in the future).
pub fn parse_unix_ls(text: &str, now_year: i32) -> Vec<RemoteEntry> {
    let now = Local::now();
    let mut out = vec![];
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let w = words(line);
        if w.len() < 6 || line.starts_with("total ") {
            continue;
        }
        let perms = w[0].1;
        if perms.len() < 10 {
            continue;
        }
        // find "<size> <Mon> <day> <time|year>" (the owner / group columns vary)
        let Some(i) = (3..w.len().saturating_sub(2)).find(|&i| month(w[i].1).is_some() && is_num(w[i - 1].1) && is_num(w[i + 1].1)) else { continue };
        let (mo, day, ty) = (month(w[i].1).unwrap_or(1), w[i + 1].1.parse().unwrap_or(1), w[i + 2].1);
        let modified = if let Some((h, m)) = ty.split_once(':') {
            let (h, m) = (h.parse().unwrap_or(0), m.parse().unwrap_or(0));
            local(now_year, mo, day, h, m).map(|t| {
                if t > now + chrono::Duration::days(1) { local(now_year - 1, mo, day, h, m).unwrap_or(t) } else { t }
            })
        } else {
            ty.parse().ok().and_then(|y| local(y, mo, day, 0, 0))
        };
        let mut name = rest_after(line, w[i + 2]).to_string();
        let kind = perms.as_bytes()[0];
        let is_link = kind == b'l';
        if is_link {
            if let Some(p) = name.find(" -> ") {
                name.truncate(p);
            }
        }
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let is_dir = kind == b'd';
        out.push(RemoteEntry {
            hidden: name.starts_with('.'),
            exec: !is_dir && perms[1..].contains('x'),
            size: (!is_dir).then(|| w[i - 1].1.parse().unwrap_or(0)),
            name,
            is_dir,
            is_link,
            modified,
        });
    }
    out
}

/// FTP LIST in DOS / IIS style: `11-18-26  09:00AM  <DIR>  name` or `... 1234 name`.
pub fn parse_dos_list(text: &str) -> Vec<RemoteEntry> {
    let mut out = vec![];
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let w = words(line);
        if w.len() < 4 {
            continue;
        }
        let d: Vec<&str> = w[0].1.split(['-', '/']).collect();
        if d.len() != 3 {
            continue;
        }
        let (mo, day, y) = (d[0].parse().unwrap_or(1), d[1].parse().unwrap_or(1), d[2].parse::<i32>().unwrap_or(0));
        let year = if y >= 100 { y } else if y < 70 { 2000 + y } else { 1900 + y };
        let t = w[1].1.to_uppercase();
        let pm = t.ends_with("PM");
        let (h, m) = t.trim_end_matches(['A', 'M', 'P']).split_once(':').unwrap_or(("0", "0"));
        let mut h: u32 = h.parse().unwrap_or(0);
        if pm && h < 12 {
            h += 12;
        } else if !pm && t.ends_with("AM") && h == 12 {
            h = 0;
        }
        let is_dir = w[2].1.eq_ignore_ascii_case("<DIR>");
        let name = rest_after(line, w[2]).to_string();
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        out.push(RemoteEntry {
            hidden: name.starts_with('.'),
            exec: false,
            size: if is_dir { None } else { w[2].1.parse().ok() },
            modified: local(year, mo, day, h, m.parse().unwrap_or(0)),
            name,
            is_dir,
            is_link: false,
        });
    }
    out
}

/// FTP MLSD: `type=file;size=5;modify=20261118090000; name` (modify is UTC).
pub fn parse_mlsd(text: &str) -> Vec<RemoteEntry> {
    let mut out = vec![];
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let Some((facts, name)) = line.split_once(' ') else { continue };
        let (mut kind, mut size, mut modified, mut exec) = ("", None, None, false);
        for f in facts.split(';') {
            let Some((k, v)) = f.split_once('=') else { continue };
            match k.to_lowercase().as_str() {
                "type" => kind = v,
                "size" => size = v.parse().ok(),
                "modify" if v.len() >= 14 => {
                    modified = chrono::NaiveDateTime::parse_from_str(&v[..14], "%Y%m%d%H%M%S")
                        .ok()
                        .map(|t| DateTime::<chrono::Utc>::from_naive_utc_and_offset(t, chrono::Utc).with_timezone(&Local));
                }
                "unix.mode" => exec = u32::from_str_radix(v, 8).is_ok_and(|m| m & 0o111 != 0),
                _ => {}
            }
        }
        let kind = kind.to_lowercase();
        if kind == "cdir" || kind == "pdir" || name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let is_dir = kind == "dir";
        out.push(RemoteEntry {
            hidden: name.starts_with('.'),
            exec: exec && !is_dir,
            size: if is_dir { None } else { size },
            name: name.to_string(),
            is_dir,
            is_link: kind.starts_with("os.unix=slink"),
            modified,
        });
    }
    out
}

/// FTP LIST in either style, chosen by the first line.
pub fn parse_list(text: &str, now_year: i32) -> Vec<RemoteEntry> {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    if first.starts_with(|c: char| c.is_ascii_digit()) {
        parse_dos_list(text)
    } else {
        parse_unix_ls(text, now_year)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_ls() {
        let t = "total 12\n\
drwxr-xr-x    2 user  grp   4096 Jan  5 10:30 my dir\n\
-rw-r--r--    1 user  grp  12345 Mar  3  2021 report 2021.pdf\n\
-rwxr-xr-x    1 user  grp     10 Nov 18 09:00 run.sh\n\
lrwxrwxrwx    1 user  grp      7 Nov 18 09:00 cur -> /var/x\n\
-rw-r--r--    1 user  grp      0 Nov 18 09:00 .hidden\n\
drwxr-xr-x    5 user  grp   4096 Nov 18 09:00 .\n";
        let e = parse_unix_ls(t, 2026);
        assert_eq!(e.len(), 5);
        assert!(e[0].is_dir && e[0].name == "my dir");
        assert_eq!(e[1].size, Some(12345));
        assert_eq!(e[1].modified.unwrap().format("%Y").to_string(), "2021");
        assert!(e[2].exec);
        assert!(e[3].is_link && e[3].name == "cur");
        assert!(e[4].hidden);
    }

    #[test]
    fn unix_ls_odd_columns() {
        // sftp on some servers prints "?" for links and permissions masked with '*'
        let t = "drwx******    ? 1001     1001         4096 Nov 20 09:57 x  y \n";
        let e = parse_unix_ls(t, 2026);
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].name, "x  y ");
        assert!(e[0].is_dir);
    }

    #[test]
    fn dos_and_mlsd() {
        let d = parse_dos_list("11-18-26  09:00AM       <DIR>          Docs\n11-18-26  10:15PM              1234 a b.txt\n");
        assert!(d[0].is_dir && d[0].name == "Docs");
        assert_eq!((d[1].name.as_str(), d[1].size), ("a b.txt", Some(1234)));
        let m = parse_mlsd("type=cdir; .\ntype=dir;modify=20261118090000; src\ntype=file;size=5;modify=20261118090000; a;b.txt\n");
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].name, "a;b.txt");
        assert_eq!(m[1].size, Some(5));
    }

    #[test]
    fn list_picks_format() {
        assert!(parse_list("11-18-26  09:00AM       <DIR>          Docs\n", 2026)[0].is_dir);
        assert_eq!(parse_list("-rw-r--r-- 1 u g 3 Nov 18 09:00 a\n", 2026)[0].size, Some(3));
    }
}
