//! File colours by name: <config>/orthodox-navigator/colors.txt has lines `colour = masks`; the first line
//! whose masks match a file name gives its colour. The colour is a part of the theme (exec, archive,
//! doc, image, junk, file, dir, hidden, keyword, type, string, comment, number, meta, error), one of the
//! 16 colours (light green, yellow, ...) or #rrggbb. Without the file, ON uses the built-in groups below.

use crate::screen::C;
use crate::theme::{self, Theme};
use crate::util::wildcard;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Color {
    Role(Role),
    Fixed(C),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    File, Dir, Hidden, Exec, Archive, Doc, Image, Junk, Keyword, Type, Str, Comment, Number, Meta, Error,
}

impl Color {
    pub fn get(self, t: &Theme) -> C {
        match self {
            Color::Fixed(c) => c,
            Color::Role(r) => match r {
                Role::File => t.file,
                Role::Dir => t.dir,
                Role::Hidden => t.hidden,
                Role::Exec => t.exec,
                Role::Archive => t.archive,
                Role::Doc => t.doc,
                Role::Image => t.image,
                Role::Junk => t.junk,
                Role::Keyword => t.syn_keyword,
                Role::Type => t.syn_type,
                Role::Str => t.syn_string,
                Role::Comment => t.syn_comment,
                Role::Number => t.syn_number,
                Role::Meta => t.syn_meta,
                Role::Error => t.error,
            },
        }
    }
}

pub struct Group {
    pub color: Color,
    pub masks: Vec<String>,
}

pub const DEFAULT: &str = "\
# Orthodox Navigator: file colours in the panels.   colour = masks (; between them)
# The first line whose masks match a file name gives its colour (folders keep theirs).
# Save the file to see the change.
#
# A colour is one of the theme's named colours (they change with the colour theme, Ctrl+T):
#   file      plain files (text is the same)
#   dir       folders
#   hidden    hidden files
#   exec      programs and scripts
#   archive   archives
#   doc       documents
#   image     pictures
#   junk      backups, temporary files, logs
#   keyword   keywords in the editor and viewer
#   type      type names in the editor and viewer
#   string    strings in the editor and viewer
#   comment   comments in the editor and viewer
#   number    numbers in the editor and viewer
#   meta      attributes, macros, preprocessor lines in the editor and viewer
#   error     error messages
# or one of the 16 colours: black, blue, green, cyan, red, magenta, brown, light gray, dark gray,
#   light blue, light green, light cyan, light red, light magenta, yellow, white
# or #rrggbb.

exec    = *.exe;*.com;*.bat;*.cmd;*.ps1;*.msi;*.sh
archive = *.zip;*.rar;*.7z;*.arj;*.gz;*.tar;*.cab;*.lzh;*.tgz;*.ha;*.xz;*.bz2;*.zst
doc     = *.txt;*.diz;*.doc;*.docx;*.md;*.me;*.nfo;*.rtf;*.pdf;*.htm;*.html;*.ini
image   = *.ico;*.bmp;*.gif;*.png;*.jpg;*.jpeg;*.pcx;*.svg;*.webp;*.tif
junk    = *.bak;*.tmp;*.log;*.old;*~

# source code, for example:
# type  = *.rs;*.c;*.h;*.cpp;*.cs;*.go;*.java;*.kt;*.py;*.js;*.ts;*.lua;*.rb
# light magenta = README*;TODO*;*.1st
";

fn named(s: &str) -> Option<C> {
    let s = s.to_lowercase().replace(['_', '-'], " ");
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(match s.as_str() {
        "black" => C::Black,
        "blue" => C::Blue,
        "green" => C::Green,
        "cyan" => C::Cyan,
        "red" => C::Red,
        "magenta" => C::Magenta,
        "brown" => C::Brown,
        "light gray" | "light grey" | "gray" | "grey" => C::LightGray,
        "dark gray" | "dark grey" => C::DarkGray,
        "light blue" => C::LightBlue,
        "light green" => C::LightGreen,
        "light cyan" => C::LightCyan,
        "light red" => C::LightRed,
        "light magenta" => C::LightMagenta,
        "yellow" => C::Yellow,
        "white" => C::White,
        _ => return None,
    })
}

pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#').filter(|h| h.len() == 6) {
        let v = u32::from_str_radix(hex, 16).ok()?;
        return Some(Color::Fixed(C::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)));
    }
    let role = match s.to_lowercase().as_str() {
        "file" | "text" => Role::File,
        "dir" | "folder" => Role::Dir,
        "hidden" => Role::Hidden,
        "exec" => Role::Exec,
        "archive" => Role::Archive,
        "doc" => Role::Doc,
        "image" => Role::Image,
        "junk" => Role::Junk,
        "keyword" => Role::Keyword,
        "type" => Role::Type,
        "string" => Role::Str,
        "comment" => Role::Comment,
        "number" => Role::Number,
        "meta" => Role::Meta,
        "error" => Role::Error,
        _ => return named(s).map(Color::Fixed),
    };
    Some(Color::Role(role))
}

pub fn parse(text: &str) -> Vec<Group> {
    text.lines()
        .map(str::trim)
        // a # line is a comment, unless it is a #rrggbb colour
        .filter(|l| !l.is_empty() && (!l.starts_with('#') || l.split_once('=').is_some_and(|(c, _)| c.trim().len() == 7 && c.trim()[1..].chars().all(|h| h.is_ascii_hexdigit()))))
        .filter_map(|l| {
            let (c, m) = l.split_once('=')?;
            let color = parse_color(c)?;
            let masks = m.split([';', ',']).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            Some(Group { color, masks })
        })
        .collect()
}

pub fn file() -> Option<PathBuf> {
    crate::usermenu::config_dir().map(|d| d.join("colors.txt"))
}

static GROUPS: RwLock<Option<Vec<Group>>> = RwLock::new(None);

/// Read colors.txt again (after editing it).
pub fn reload() {
    let text = file().and_then(|f| fs::read_to_string(f).ok()).unwrap_or_else(|| DEFAULT.to_string());
    if let Ok(mut g) = GROUPS.write() {
        *g = Some(parse(&text));
    }
}

/// The colour of a file by its name, if a group matches.
pub fn of_name(name: &str) -> Option<C> {
    if GROUPS.read().map(|g| g.is_none()).unwrap_or(false) {
        reload();
    }
    let g = GROUPS.read().ok()?;
    let groups = g.as_ref()?;
    let t = theme::current();
    groups.iter().find(|gr| gr.masks.iter().any(|m| wildcard(m, name))).map(|gr| gr.color.get(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups() {
        let g = parse("# x\nexec = *.exe;*.bat\nlight green = README*\n#00ff80 = *.rs\nnonsense = *.x\n");
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].color, Color::Role(Role::Exec));
        assert_eq!(g[1].color, Color::Fixed(C::LightGreen));
        assert_eq!(g[2].color, Color::Fixed(C::Rgb(0, 255, 128)));
        assert_eq!(parse(DEFAULT).len(), 5);
    }
}
