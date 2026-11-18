//! Lightweight syntax highlighting for the viewer, painted with the DN 16-colour palette.
//!
//! Not a full parser: a small per-language tokenizer that knows comments, strings, numbers,
//! keywords, types and a few language-specific things (preprocessor lines, shell variables,
//! XML tags, Markdown headings). State (inside a block comment / multi-line string / tag)
//! carries from one line to the next.

use crate::screen::C;
use crate::theme::Theme;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    Text,
    Keyword,
    Type,
    Str,
    Comment,
    Number,
    Meta,
}

/// Where a line starts: inside a block comment, a string, an XML tag, ...
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum State {
    #[default]
    Normal,
    Comment(&'static str),
    Str(&'static str),
    Tag,
    TagStr(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Code,
    Markup,
    Ini,
    Yaml,
    Markdown,
}

pub struct Lang {
    pub name: &'static str,
    kind: Kind,
    line: &'static [&'static str],
    block: &'static [(&'static str, &'static str)],
    /// string delimiters, longest first
    strings: &'static [&'static str],
    escape: Option<char>,
    /// no escapes inside '...' (shell, PowerShell)
    raw_single: bool,
    /// "..." and '...' may continue on the next line
    ml_strings: bool,
    /// ' starts a char literal only when it closes ('a', '\n'); otherwise it's a lifetime (Rust)
    char_lit: bool,
    keywords: &'static str,
    types: &'static str,
    nocase: bool,
    /// identifiers starting with a capital letter are types
    cap_types: bool,
    /// a line starting with one of these is a preprocessor / attribute / label line
    meta: &'static [&'static str],
    /// $name, ${name}
    vars: bool,
    /// %name% (batch)
    pct_vars: bool,
    /// Verb-Noun and -Param words (PowerShell)
    dash_words: bool,
    /// a word or string followed by ':' is a key (JSON, CSS, Makefile targets)
    keys: bool,
    /// name! is a macro (Rust)
    macros: bool,
    /// line comments only at the start of a line (batch `rem`)
    comment_at_start: bool,
}

const BASE: Lang = Lang {
    name: "",
    kind: Kind::Code,
    line: &[],
    block: &[],
    strings: &["\"", "'"],
    escape: Some('\\'),
    raw_single: false,
    ml_strings: false,
    char_lit: false,
    keywords: "",
    types: "",
    nocase: false,
    cap_types: false,
    meta: &[],
    vars: false,
    pct_vars: false,
    dash_words: false,
    keys: false,
    macros: false,
    comment_at_start: false,
};

const C_COMMENTS: &[(&str, &str)] = &[("/*", "*/")];

static RUST: Lang = Lang {
    name: "Rust",
    line: &["//"],
    block: C_COMMENTS,
    strings: &["\"", "'"],
    ml_strings: true,
    char_lit: true,
    keywords: "as async await break const continue crate dyn else enum extern false fn for if impl in let loop match mod move mut pub ref return self Self static struct super trait true type unsafe use where while",
    types: "bool char str u8 u16 u32 u64 u128 usize i8 i16 i32 i64 i128 isize f32 f64",
    cap_types: true,
    meta: &["#[", "#!["],
    macros: true,
    ..BASE
};

static CLANG: Lang = Lang {
    name: "C/C++",
    line: &["//"],
    block: C_COMMENTS,
    keywords: "auto break case catch class const constexpr continue default delete do else enum explicit extern false for friend goto if inline namespace new noexcept nullptr operator override private protected public return sizeof static struct switch template this throw true try typedef typename union using virtual volatile while NULL",
    types: "void bool char short int long float double signed unsigned size_t ssize_t wchar_t int8_t int16_t int32_t int64_t uint8_t uint16_t uint32_t uint64_t std string vector",
    meta: &["#"],
    ..BASE
};

static CSHARP: Lang = Lang {
    name: "C#",
    line: &["//"],
    block: C_COMMENTS,
    strings: &["\"\"\"", "\"", "'"],
    keywords: "abstract as async await base break case catch checked class const continue default delegate do else enum event explicit extern false finally fixed for foreach get goto if implicit in init interface internal is lock namespace new null operator out override params private protected public readonly record ref return sealed set sizeof stackalloc static struct switch this throw true try typeof unchecked unsafe using var virtual volatile when where while yield",
    types: "bool byte char decimal double float int long object sbyte short string uint ulong ushort void dynamic nint nuint",
    cap_types: true,
    meta: &["#"],
    ..BASE
};

static JAVA: Lang = Lang {
    name: "Java/Kotlin",
    line: &["//"],
    block: C_COMMENTS,
    strings: &["\"\"\"", "\"", "'"],
    keywords: "abstract assert break case catch class const continue default do else enum extends final finally for goto if implements import instanceof interface native new null package private protected public return static super switch synchronized this throw throws transient true false try volatile while var val fun object when is in as data sealed open override companion lateinit def",
    types: "boolean byte char double float int long short void",
    cap_types: true,
    meta: &["@"],
    ..BASE
};

static JS: Lang = Lang {
    name: "JavaScript",
    line: &["//"],
    block: C_COMMENTS,
    strings: &["\"", "'", "`"],
    keywords: "async await break case catch class const continue debugger default delete do else export extends false finally for from function get if import in instanceof let new null of return set static super switch this throw true try typeof undefined var void while with yield as interface type enum implements private public protected readonly declare namespace abstract keyof",
    types: "any boolean number string never unknown object symbol bigint",
    cap_types: true,
    ..BASE
};

static GO: Lang = Lang {
    name: "Go",
    line: &["//"],
    block: C_COMMENTS,
    strings: &["\"", "`", "'"],
    keywords: "break case chan const continue default defer else fallthrough for func go goto if import interface map package range return select struct switch type var true false nil iota",
    types: "bool byte complex64 complex128 error float32 float64 int int8 int16 int32 int64 rune string uint uint8 uint16 uint32 uint64 uintptr any",
    ..BASE
};

static PYTHON: Lang = Lang {
    name: "Python",
    line: &["#"],
    strings: &["\"\"\"", "'''", "\"", "'"],
    keywords: "and as assert async await break class continue def del elif else except False finally for from global if import in is lambda None nonlocal not or pass raise return True try while with yield match case self",
    types: "int float str bool list dict set tuple bytes object type",
    meta: &["@"],
    ..BASE
};

static SHELL: Lang = Lang {
    name: "Shell",
    line: &["#"],
    raw_single: true,
    ml_strings: true,
    keywords: "if then else elif fi case esac for while until do done in function return local export readonly declare unset shift exit source alias set echo cd test",
    vars: true,
    ..BASE
};

static POWERSHELL: Lang = Lang {
    name: "PowerShell",
    line: &["#"],
    block: &[("<#", "#>")],
    escape: Some('`'),
    raw_single: true,
    ml_strings: true,
    keywords: "begin break catch class continue data do dynamicparam else elseif end enum exit filter finally for foreach function hidden if in param process return static switch throw trap try until using while -and -or -not -xor -eq -ne -gt -ge -lt -le -like -notlike -match -notmatch -contains -notcontains -in -notin -replace -split -join -is -isnot -as -f $true $false $null",
    nocase: true,
    vars: true,
    dash_words: true,
    ..BASE
};

static BATCH: Lang = Lang {
    name: "Batch",
    line: &["::", "rem ", "@rem "],
    escape: None,
    keywords: "if else for in do goto call exit set setlocal endlocal echo off on not exist defined errorlevel equ neq lss leq gtr geq shift pushd popd cd start",
    nocase: true,
    meta: &[":"],
    pct_vars: true,
    comment_at_start: true,
    ..BASE
};

static SQL: Lang = Lang {
    name: "SQL",
    line: &["--"],
    block: C_COMMENTS,
    escape: None,
    ml_strings: true,
    keywords: "select from where and or not insert into values update set delete create table alter drop index view join inner left right outer full cross on as group by order having limit offset distinct union all null is in exists between like case when then else end primary key foreign references default begin commit rollback transaction if declare procedure function returns return with top asc desc go",
    types: "int integer bigint smallint tinyint varchar nvarchar char nchar text date time datetime datetime2 timestamp boolean bit decimal numeric float real money uniqueidentifier",
    nocase: true,
    ..BASE
};

static JSON: Lang = Lang { name: "JSON", line: &["//"], block: C_COMMENTS, strings: &["\""], keywords: "true false null", keys: true, ..BASE };

static INI: Lang = Lang {
    name: "INI/TOML",
    kind: Kind::Ini,
    line: &["#", ";"],
    strings: &["\"\"\"", "'''", "\"", "'"],
    keywords: "true false",
    ..BASE
};

static YAML: Lang = Lang { name: "YAML", kind: Kind::Yaml, line: &["#"], keywords: "true false null yes no on off", ..BASE };

static XML: Lang = Lang { name: "XML/HTML", kind: Kind::Markup, ..BASE };

static CSS: Lang = Lang { name: "CSS", line: &["//"], block: C_COMMENTS, keywords: "important", keys: true, meta: &["@"], ..BASE };

static MARKDOWN: Lang = Lang { name: "Markdown", kind: Kind::Markdown, ..BASE };

static LUA: Lang = Lang {
    name: "Lua",
    line: &["--"],
    block: &[("--[[", "]]")],
    keywords: "and break do else elseif end false for function goto if in local nil not or repeat return then true until while",
    ..BASE
};

static RUBY: Lang = Lang {
    name: "Ruby",
    line: &["#"],
    ml_strings: true,
    keywords: "alias and begin break case class def defined do else elsif end ensure false for if in module next nil not or redo rescue retry return self super then true undef unless until when while yield require require_relative attr_accessor attr_reader",
    cap_types: true,
    ..BASE
};

static MAKE: Lang = Lang {
    name: "Makefile",
    line: &["#"],
    keywords: "ifeq ifneq ifdef ifndef else endif include define endef export override",
    vars: true,
    keys: true,
    ..BASE
};

static DOCKER: Lang = Lang {
    name: "Dockerfile",
    line: &["#"],
    keywords: "from run cmd label expose env add copy entrypoint volume user workdir arg onbuild stopsignal healthcheck shell as",
    nocase: true,
    vars: true,
    ..BASE
};

/// Pick a language from the file name, or the #! line.
pub fn detect(path: &Path, first_line: &str) -> Option<&'static Lang> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    let by_name = match name.as_str() {
        "makefile" | "gnumakefile" => Some(&MAKE),
        "dockerfile" | "containerfile" => Some(&DOCKER),
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".bash_aliases" | ".zprofile" | ".zshenv" => Some(&SHELL),
        ".gitconfig" | ".editorconfig" | ".gitmodules" | "cargo.lock" => Some(&INI),
        _ => None,
    };
    if by_name.is_some() {
        return by_name;
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let by_ext: Option<&'static Lang> = match ext {
        "rs" => Some(&RUST),
        "c" | "h" | "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" | "ino" | "m" | "mm" => Some(&CLANG),
        "cs" | "csx" => Some(&CSHARP),
        "java" | "kt" | "kts" | "scala" | "groovy" | "gradle" => Some(&JAVA),
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" | "mts" | "cts" => Some(&JS),
        "go" => Some(&GO),
        "py" | "pyw" | "pyi" => Some(&PYTHON),
        "sh" | "bash" | "zsh" | "ksh" => Some(&SHELL),
        "ps1" | "psm1" | "psd1" => Some(&POWERSHELL),
        "bat" | "cmd" => Some(&BATCH),
        "sql" => Some(&SQL),
        "json" | "jsonc" | "json5" | "ipynb" | "code-workspace" => Some(&JSON),
        "toml" | "ini" | "cfg" | "conf" | "properties" | "inf" | "reg" | "editorconfig" | "gitconfig" => Some(&INI),
        "yaml" | "yml" => Some(&YAML),
        "xml" | "html" | "htm" | "xhtml" | "svg" | "xaml" | "axaml" | "csproj" | "vbproj" | "fsproj" | "vcxproj" | "props" | "targets"
        | "config" | "resx" | "nuspec" | "plist" | "manifest" | "xsd" | "xsl" | "xslt" | "wsdl" | "vue" => Some(&XML),
        "css" | "scss" | "sass" | "less" => Some(&CSS),
        "md" | "markdown" => Some(&MARKDOWN),
        "lua" => Some(&LUA),
        "rb" | "rake" | "gemspec" => Some(&RUBY),
        "mk" => Some(&MAKE),
        "dockerfile" => Some(&DOCKER),
        _ => None,
    };
    if by_ext.is_some() {
        return by_ext;
    }
    let shebang = first_line.strip_prefix("#!")?;
    if shebang.contains("python") {
        Some(&PYTHON)
    } else if shebang.contains("pwsh") {
        Some(&POWERSHELL)
    } else if shebang.contains("sh") {
        Some(&SHELL)
    } else if shebang.contains("node") {
        Some(&JS)
    } else if shebang.contains("ruby") {
        Some(&RUBY)
    } else {
        None
    }
}

/// The colour of a token in the given theme.
pub fn color(t: &Theme, k: Tok) -> C {
    match k {
        Tok::Text => t.viewer_fg,
        Tok::Keyword => t.syn_keyword,
        Tok::Type => t.syn_type,
        Tok::Str => t.syn_string,
        Tok::Comment => t.syn_comment,
        Tok::Number => t.syn_number,
        Tok::Meta => t.syn_meta,
    }
}

/// The state at the start of every line (for drawing any line without scanning from the top).
pub fn line_states(lang: &Lang, lines: &[String]) -> Vec<State> {
    let mut out = Vec::with_capacity(lines.len());
    let mut toks = Vec::new();
    let mut st = State::Normal;
    for l in lines {
        out.push(st);
        st = scan(lang, l, st, &mut toks);
    }
    out
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn at(c: &[char], i: usize, s: &str, nocase: bool) -> bool {
    let mut k = i;
    for ch in s.chars() {
        match c.get(k) {
            Some(&x) if x == ch || (nocase && x.eq_ignore_ascii_case(&ch)) => k += 1,
            _ => return false,
        }
    }
    true
}

fn paint(out: &mut [Tok], a: usize, b: usize, t: Tok) {
    let b = b.min(out.len());
    if a < b {
        out[a..b].fill(t);
    }
}

fn has(list: &str, w: &str, nocase: bool) -> bool {
    list.split_whitespace().any(|k| if nocase { k.eq_ignore_ascii_case(w) } else { k == w })
}

fn next_non_space(c: &[char], i: usize) -> Option<char> {
    c[i.min(c.len())..].iter().copied().find(|ch| !ch.is_whitespace())
}

/// Colour one line: fills `out` with a token per char; returns the state for the next line.
pub fn scan(lang: &Lang, line: &str, st: State, out: &mut Vec<Tok>) -> State {
    let c: Vec<char> = line.chars().collect();
    out.clear();
    out.resize(c.len(), Tok::Text);
    match lang.kind {
        Kind::Markup => scan_markup(&c, st, out),
        Kind::Markdown => scan_markdown(&c, st, out),
        _ => scan_code(lang, &c, st, out),
    }
}

fn scan_code(lang: &Lang, c: &[char], mut st: State, out: &mut [Tok]) -> State {
    let n = c.len();
    let first = c.iter().position(|ch| !ch.is_whitespace()).unwrap_or(n);
    let mut i = 0;

    // whole-line rules, only when the line doesn't start inside a comment or string
    if st == State::Normal && first < n {
        if lang.line.iter().any(|m| at(c, first, m, lang.nocase) || (lang.comment_at_start && at(c, first, m.trim_end(), lang.nocase) && first + m.trim_end().chars().count() == n)) {
            paint(out, first, n, Tok::Comment);
            return State::Normal;
        }
        if lang.meta.iter().any(|m| at(c, first, m, false)) {
            paint(out, first, n, Tok::Meta);
            return State::Normal;
        }
        match lang.kind {
            Kind::Ini => {
                if c[first] == '[' {
                    paint(out, first, n, Tok::Meta);
                    return State::Normal;
                }
                if let Some(k) = c.iter().position(|&ch| ch == '=') {
                    paint(out, first, k, Tok::Keyword);
                    i = k + 1;
                }
            }
            Kind::Yaml => {
                if at(c, first, "---", false) || at(c, first, "...", false) {
                    paint(out, first, n, Tok::Meta);
                    return State::Normal;
                }
                let mut k = first;
                if c[k] == '-' && c.get(k + 1).is_none_or(|ch| *ch == ' ') {
                    paint(out, k, k + 1, Tok::Meta);
                    k += 1;
                    while k < n && c[k] == ' ' {
                        k += 1;
                    }
                }
                if k < n && c[k] != '"' && c[k] != '\'' && c[k] != '#' {
                    let colon = (k..n).find(|&j| c[j] == ':' && c.get(j + 1).is_none_or(|ch| *ch == ' '));
                    if let Some(j) = colon {
                        paint(out, k, j, Tok::Keyword);
                        i = j + 1;
                    } else {
                        i = k;
                    }
                }
            }
            _ => {}
        }
    }

    while i < n {
        match st {
            State::Comment(end) => {
                let found = (i..n).find(|&j| at(c, j, end, false));
                match found {
                    Some(j) => {
                        let e = j + end.chars().count();
                        paint(out, i, e, Tok::Comment);
                        i = e;
                        st = State::Normal;
                    }
                    None => {
                        paint(out, i, n, Tok::Comment);
                        return st;
                    }
                }
                continue;
            }
            State::Str(d) => {
                let single = d.chars().count() == 1;
                let esc = if lang.raw_single && d == "'" { None } else { lang.escape };
                let mut j = i;
                let mut end = None;
                while j < n {
                    if Some(c[j]) == esc {
                        j += 2;
                        continue;
                    }
                    if at(c, j, d, false) {
                        if single && at(c, j + 1, d, false) {
                            j += 2; // doubled quote = escaped quote ('it''s')
                            continue;
                        }
                        end = Some(j + d.chars().count());
                        break;
                    }
                    j += 1;
                }
                match end {
                    Some(e) => {
                        let start = i;
                        paint(out, i, e, Tok::Str);
                        i = e;
                        st = State::Normal;
                        // "key": in JSON
                        if lang.keys && next_non_space(c, e) == Some(':') {
                            let s0 = (0..start).rev().find(|&k| !matches!(out[k], Tok::Str)).map_or(0, |k| k + 1);
                            paint(out, s0, e, Tok::Keyword);
                        }
                    }
                    None => {
                        paint(out, i, n, Tok::Str);
                        let multi = !single || d == "`" || lang.ml_strings;
                        return if multi { st } else { State::Normal };
                    }
                }
                continue;
            }
            _ => {}
        }

        if let Some(&(open, close)) = lang.block.iter().find(|(o, _)| at(c, i, o, false)) {
            let e = i + open.chars().count();
            paint(out, i, e, Tok::Comment);
            i = e;
            st = State::Comment(close);
            continue;
        }
        if !lang.comment_at_start
            && lang.line.iter().any(|m| at(c, i, m, lang.nocase) && (*m != "#" || i == 0 || (!is_ident(c[i - 1]) && c[i - 1] != '$')))
        {
            paint(out, i, n, Tok::Comment);
            return State::Normal;
        }
        if let Some(&d) = lang.strings.iter().find(|d| at(c, i, d, false)) {
            if lang.char_lit && d == "'" {
                // 'x' or '\n' is a char; 'a (no closing quote) is a lifetime
                let close = if c.get(i + 1) == Some(&'\\') {
                    (i + 2..n.min(i + 12)).find(|&j| c[j] == '\'')
                } else if c.get(i + 2) == Some(&'\'') {
                    Some(i + 2)
                } else {
                    None
                };
                match close {
                    Some(j) => {
                        paint(out, i, j + 1, Tok::Str);
                        i = j + 1;
                    }
                    None => {
                        let e = (i + 1..n).find(|&j| !is_ident(c[j])).unwrap_or(n);
                        paint(out, i, e, Tok::Meta);
                        i = e.max(i + 1);
                    }
                }
                continue;
            }
            let e = i + d.chars().count();
            paint(out, i, e, Tok::Str);
            i = e;
            st = State::Str(d);
            continue;
        }

        let ch = c[i];
        if lang.vars && ch == '$' && i + 1 < n {
            let e = match c[i + 1] {
                '{' | '(' => {
                    let close = if c[i + 1] == '{' { '}' } else { ')' };
                    (i + 2..n).find(|&j| c[j] == close).map_or(n, |j| j + 1)
                }
                x if is_ident(x) => (i + 1..n).find(|&j| !is_ident(c[j]) && c[j] != ':').unwrap_or(n),
                _ => i + 2, // $? $@ $# $$ $_
            };
            let word: String = c[i..e].iter().collect();
            paint(out, i, e, if has(lang.keywords, &word, lang.nocase) { Tok::Keyword } else { Tok::Meta });
            i = e;
            continue;
        }
        if lang.pct_vars && ch == '%' && i + 1 < n {
            let e = if c[i + 1] == '~' || c[i + 1].is_ascii_digit() || c[i + 1] == '%' {
                (i + 2..n).find(|&j| !is_ident(c[j])).unwrap_or(n)
            } else {
                (i + 1..n).find(|&j| c[j] == '%').map_or(i + 1, |j| j + 1)
            };
            paint(out, i, e, Tok::Meta);
            i = e;
            continue;
        }
        if ch.is_ascii_digit() && (i == 0 || !is_ident(c[i - 1])) {
            let mut e = i + 1;
            while e < n && (is_ident(c[e]) || (c[e] == '.' && c.get(e + 1).is_some_and(|x| x.is_ascii_digit()))) {
                e += 1;
            }
            paint(out, i, e, Tok::Number);
            i = e;
            continue;
        }
        let dash_word = lang.dash_words && ch == '-' && c.get(i + 1).is_some_and(|x| x.is_alphabetic()) && (i == 0 || c[i - 1].is_whitespace() || c[i - 1] == '(');
        if (is_ident(ch) && !ch.is_ascii_digit()) || dash_word {
            let mut e = i + 1;
            while e < n && (is_ident(c[e]) || (lang.dash_words && c[e] == '-' && c.get(e + 1).is_some_and(|x| is_ident(*x)))) {
                e += 1;
            }
            let word: String = c[i..e].iter().collect();
            let after = c.get(e).copied();
            let tok = if has(lang.keywords, &word, lang.nocase) {
                Tok::Keyword
            } else if has(lang.types, &word, lang.nocase) {
                Tok::Type
            } else if lang.macros && after == Some('!') && c.get(e + 1) != Some(&'=') {
                e += 1;
                Tok::Meta
            } else if (lang.cap_types && ch.is_uppercase()) || (lang.dash_words && word.contains('-')) {
                Tok::Type // MyType; Get-ChildItem, -Path
            } else if lang.keys && next_non_space(c, e) == Some(':') && !at(c, e, "::", false) {
                Tok::Keyword
            } else {
                Tok::Text
            };
            paint(out, i, e, tok);
            i = e;
            continue;
        }
        i += 1;
    }
    st
}

fn scan_markup(c: &[char], mut st: State, out: &mut [Tok]) -> State {
    let n = c.len();
    let mut i = 0;
    let name_char = |ch: char| is_ident(ch) || matches!(ch, '-' | ':' | '.');
    while i < n {
        match st {
            State::Comment(end) => match (i..n).find(|&j| at(c, j, end, false)) {
                Some(j) => {
                    let e = j + end.chars().count();
                    paint(out, i, e, Tok::Comment);
                    i = e;
                    st = State::Normal;
                }
                None => {
                    paint(out, i, n, Tok::Comment);
                    return st;
                }
            },
            State::TagStr(d) | State::Str(d) => match (i..n).find(|&j| at(c, j, d, false)) {
                Some(j) => {
                    paint(out, i, j + 1, Tok::Str);
                    i = j + 1;
                    st = if matches!(st, State::TagStr(_)) { State::Tag } else { State::Normal };
                }
                None => {
                    paint(out, i, n, Tok::Str);
                    return st;
                }
            },
            State::Tag => {
                let ch = c[i];
                if ch == '>' || at(c, i, "/>", false) || at(c, i, "?>", false) {
                    let e = if ch == '>' { i + 1 } else { i + 2 };
                    paint(out, i, e, Tok::Keyword);
                    i = e;
                    st = State::Normal;
                } else if ch == '"' || ch == '\'' {
                    paint(out, i, i + 1, Tok::Str);
                    i += 1;
                    st = State::TagStr(if ch == '"' { "\"" } else { "'" });
                } else if name_char(ch) {
                    let e = (i..n).find(|&j| !name_char(c[j])).unwrap_or(n);
                    paint(out, i, e, Tok::Type);
                    i = e;
                } else {
                    i += 1;
                }
            }
            State::Normal => {
                if at(c, i, "<!--", false) {
                    paint(out, i, i + 4, Tok::Comment);
                    i += 4;
                    st = State::Comment("-->");
                } else if at(c, i, "<![CDATA[", false) {
                    paint(out, i, i + 9, Tok::Meta);
                    i += 9;
                    st = State::Str("]]>");
                } else if c[i] == '<' && c.get(i + 1).is_some_and(|&x| x.is_alphabetic() || matches!(x, '/' | '?' | '!')) {
                    let mut e = i + 1;
                    if matches!(c[e], '/' | '?' | '!') {
                        e += 1;
                    }
                    while e < n && name_char(c[e]) {
                        e += 1;
                    }
                    paint(out, i, e, Tok::Keyword);
                    i = e;
                    st = State::Tag;
                } else if c[i] == '&' {
                    match (i + 1..n.min(i + 12)).find(|&j| c[j] == ';') {
                        Some(j) => {
                            paint(out, i, j + 1, Tok::Number);
                            i = j + 1;
                        }
                        None => i += 1,
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    st
}

fn scan_markdown(c: &[char], st: State, out: &mut [Tok]) -> State {
    let n = c.len();
    let first = c.iter().position(|ch| !ch.is_whitespace()).unwrap_or(n);
    let fence = at(c, first, "```", false) || at(c, first, "~~~", false);
    if st != State::Normal {
        // inside a ``` code block
        if fence {
            paint(out, first, n, Tok::Meta);
            return State::Normal;
        }
        paint(out, 0, n, Tok::Str);
        return st;
    }
    if fence {
        paint(out, first, n, Tok::Meta);
        return State::Str("```");
    }
    if first < n && c[first] == '#' {
        paint(out, first, n, Tok::Keyword);
        return State::Normal;
    }
    if first < n && c[first] == '>' {
        paint(out, first, n, Tok::Comment);
        return State::Normal;
    }
    let mut i = first;
    // list markers: "- ", "* ", "+ ", "1. "
    if i < n && matches!(c[i], '-' | '*' | '+') && c.get(i + 1) == Some(&' ') {
        paint(out, i, i + 1, Tok::Meta);
        i += 2;
    } else {
        let d = (i..n).find(|&j| !c[j].is_ascii_digit()).unwrap_or(n);
        if d > i && c.get(d) == Some(&'.') && c.get(d + 1) == Some(&' ') {
            paint(out, i, d + 1, Tok::Meta);
            i = d + 2;
        }
    }
    while i < n {
        if c[i] == '`' {
            let e = (i + 1..n).find(|&j| c[j] == '`').map_or(n, |j| j + 1);
            paint(out, i, e, Tok::Str);
            i = e;
        } else if c[i] == ']' && c.get(i + 1) == Some(&'(') {
            let e = (i + 2..n).find(|&j| c[j] == ')').map_or(n, |j| j + 1);
            paint(out, i + 1, e, Tok::Type);
            i = e;
        } else if at(c, i, "**", false) || at(c, i, "__", false) {
            let mark: String = c[i..i + 2].iter().collect();
            match (i + 2..n).find(|&j| at(c, j, &mark, false)) {
                Some(j) => {
                    paint(out, i, j + 2, Tok::Number);
                    i = j + 2;
                }
                None => i += 2,
            }
        } else {
            i += 1;
        }
    }
    State::Normal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(lang: &Lang, line: &str) -> Vec<Tok> {
        let mut v = Vec::new();
        scan(lang, line, State::Normal, &mut v);
        v
    }

    #[test]
    fn rust_line() {
        let t = toks(&RUST, "let x: u32 = 42; // hi");
        assert_eq!(t[0], Tok::Keyword); // let
        assert_eq!(t[7], Tok::Type); // u32
        assert_eq!(t[13], Tok::Number); // 42
        assert_eq!(t[17], Tok::Comment);
    }

    #[test]
    fn rust_lifetime_and_char() {
        let t = toks(&RUST, "fn f<'a>(c: char) -> bool { c == 'x' }");
        assert_eq!(t[5], Tok::Meta); // 'a
        assert_eq!(t[8], Tok::Text); // (
        let x = "fn f<'a>(c: char) -> bool { c == 'x' }".find("'x'").unwrap();
        assert_eq!(t[x], Tok::Str);
        assert_eq!(t[x + 3], Tok::Text);
    }

    #[test]
    fn block_comment_spans_lines() {
        let mut v = Vec::new();
        let st = scan(&CLANG, "int a; /* start", State::Normal, &mut v);
        assert_eq!(st, State::Comment("*/"));
        let st = scan(&CLANG, "still */ int b;", st, &mut v);
        assert_eq!(st, State::Normal);
        assert_eq!(v[0], Tok::Comment);
        assert_eq!(v[9], Tok::Type); // int
    }

    #[test]
    fn json_keys() {
        let t = toks(&JSON, r#"  "name": "on", "n": 1"#);
        assert_eq!(t[2], Tok::Keyword);
        assert_eq!(t[10], Tok::Str);
        assert_eq!(t[21], Tok::Number);
    }

    #[test]
    fn shell_quotes_and_vars() {
        let t = toks(&SHELL, r"echo 'a\' $HOME # c");
        assert_eq!(t[8], Tok::Str); // closing quote: no escapes in '...'
        assert_eq!(t[9], Tok::Text);
        assert_eq!(t[10], Tok::Meta); // $HOME
        assert_eq!(t[16], Tok::Comment);
    }

    #[test]
    fn powershell_words() {
        let t = toks(&POWERSHELL, "Get-ChildItem -Path $x -eq 1");
        assert_eq!(t[0], Tok::Type);
        assert_eq!(t[14], Tok::Type); // -Path
        assert_eq!(t[20], Tok::Meta); // $x
        assert_eq!(t[23], Tok::Keyword); // -eq
    }

    #[test]
    fn xml_tag_spans_lines() {
        let mut v = Vec::new();
        let st = scan(&XML, r#"<Project Sdk="x""#, State::Normal, &mut v);
        assert_eq!(st, State::Tag);
        assert_eq!(v[1], Tok::Keyword);
        assert_eq!(v[9], Tok::Type);
        assert_eq!(v[13], Tok::Str);
        let st = scan(&XML, "  a='1'> text", st, &mut v);
        assert_eq!(st, State::Normal);
        assert_eq!(v[2], Tok::Type);
        assert_eq!(v[7], Tok::Keyword);
        assert_eq!(v[10], Tok::Text);
    }

    #[test]
    fn detect_by_name() {
        assert_eq!(detect(Path::new("x/main.rs"), "").map(|l| l.name), Some("Rust"));
        assert_eq!(detect(Path::new("Makefile"), "").map(|l| l.name), Some("Makefile"));
        assert_eq!(detect(Path::new("run"), "#!/usr/bin/env bash").map(|l| l.name), Some("Shell"));
        assert!(detect(Path::new("notes.txt"), "").is_none());
    }
}
