//! Pretty-printing JSON and XML (viewer F5), keeping key order and number text exactly as written.

use crate::syntax::Lang;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Json,
    Xml,
}

pub fn kind_of(lang: Option<&Lang>) -> Option<Kind> {
    match lang?.name {
        "JSON" => Some(Kind::Json),
        "XML/HTML" => Some(Kind::Xml),
        _ => None,
    }
}

pub fn pretty(kind: Kind, text: &str) -> Result<String, String> {
    match kind {
        Kind::Json => pretty_json(text),
        Kind::Xml => pretty_xml(text),
    }
}

/// Re-indent JSON (2 spaces). Checks that brackets and strings are balanced; `//` and `/* */`
/// comments (JSONC) are dropped.
pub fn pretty_json(text: &str) -> Result<String, String> {
    let c: Vec<char> = text.trim_start_matches('\u{feff}').chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut stack: Vec<char> = Vec::new();
    let mut line = 1usize;
    let mut i = 0;
    let indent = |out: &mut String, n: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n(' ', n * 2));
    };
    let next_sig = |c: &[char], mut j: usize| {
        while j < c.len() && c[j].is_whitespace() {
            j += 1;
        }
        c.get(j).copied()
    };
    while i < c.len() {
        let ch = c[i];
        match ch {
            '\n' => line += 1,
            _ if ch.is_whitespace() => {}
            '"' => {
                let start = i;
                i += 1;
                while i < c.len() && c[i] != '"' {
                    if c[i] == '\\' {
                        i += 1;
                    }
                    if c.get(i) == Some(&'\n') {
                        return Err(format!("Line {line}: a string is not closed."));
                    }
                    i += 1;
                }
                if i >= c.len() {
                    return Err(format!("Line {line}: a string is not closed."));
                }
                out.extend(&c[start..=i]);
            }
            '/' if c.get(i + 1) == Some(&'/') => {
                while i < c.len() && c[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if c.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < c.len() && !(c[i] == '*' && c[i + 1] == '/') {
                    if c[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i += 1;
            }
            '{' | '[' => {
                let close = if ch == '{' { '}' } else { ']' };
                out.push(ch);
                if next_sig(&c, i + 1) == Some(close) {
                    // empty {} or []
                    while c[i] != close {
                        i += 1;
                    }
                    out.push(close);
                } else {
                    stack.push(close);
                    indent(&mut out, stack.len());
                }
            }
            '}' | ']' => {
                match stack.pop() {
                    Some(want) if want == ch => {}
                    Some(want) => return Err(format!("Line {line}: expected '{want}' but found '{ch}'.")),
                    None => return Err(format!("Line {line}: '{ch}' without an opening bracket.")),
                }
                // drop a trailing comma before the bracket
                if out.ends_with(',') {
                    out.pop();
                }
                indent(&mut out, stack.len());
                out.push(ch);
            }
            ',' => {
                out.push(',');
                if !matches!(next_sig(&c, i + 1), Some('}') | Some(']')) {
                    indent(&mut out, stack.len());
                }
            }
            ':' => out.push_str(": "),
            _ => {
                // numbers, true, false, null: copy as written
                let start = i;
                while i < c.len() && !c[i].is_whitespace() && !matches!(c[i], ',' | '}' | ']' | ':' | '"') {
                    i += 1;
                }
                out.extend(&c[start..i]);
                continue;
            }
        }
        i += 1;
    }
    if let Some(want) = stack.pop() {
        return Err(format!("The text ends before its closing '{want}'."));
    }
    Ok(out)
}

/// JSON with all whitespace outside strings removed (keeps key order).
pub fn compact_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_str = false;
    let mut esc = false;
    for c in text.chars() {
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
        } else if c == '"' {
            in_str = true;
            out.push(c);
        } else if !c.is_whitespace() {
            out.push(c);
        }
    }
    out
}

enum XmlTok {
    Open(String),
    Close(String),
    Empty(String),
    Text(String),
    Other(String), // comment, <?xml ?>, <!DOCTYPE>, CDATA
}

fn xml_tokens(text: &str) -> Result<Vec<XmlTok>, String> {
    let mut v = Vec::new();
    let mut rest = text.trim_start_matches('\u{feff}');
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("<!--") {
            let end = r.find("-->").ok_or("A comment <!-- is not closed.")?;
            v.push(XmlTok::Other(format!("<!--{}-->", &r[..end])));
            rest = &r[end + 3..];
        } else if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let end = r.find("]]>").ok_or("A <![CDATA[ section is not closed.")?;
            v.push(XmlTok::Other(format!("<![CDATA[{}]]>", &r[..end])));
            rest = &r[end + 3..];
        } else if rest.starts_with('<') {
            // find the end of the tag, skipping quoted attribute values
            let b = rest.as_bytes();
            let mut j = 1;
            let mut quote = 0u8;
            while j < b.len() {
                match b[j] {
                    q @ (b'"' | b'\'') if quote == 0 => quote = q,
                    q if q == quote => quote = 0,
                    b'>' if quote == 0 => break,
                    _ => {}
                }
                j += 1;
            }
            if j >= b.len() {
                return Err("A tag is not closed with '>'.".into());
            }
            let tag = &rest[..=j];
            let name = |t: &str| t.trim_start_matches(['<', '/']).split(|c: char| c.is_whitespace() || c == '>' || c == '/').next().unwrap_or("").to_string();
            v.push(if tag.starts_with("<?") || tag.starts_with("<!") {
                XmlTok::Other(tag.to_string())
            } else if tag.starts_with("</") {
                XmlTok::Close(name(tag))
            } else if tag.ends_with("/>") {
                XmlTok::Empty(tag.to_string())
            } else {
                XmlTok::Open(tag.to_string())
            });
            rest = &rest[j + 1..];
        } else {
            let end = rest.find('<').unwrap_or(rest.len());
            let t = rest[..end].trim();
            if !t.is_empty() {
                v.push(XmlTok::Text(t.split_whitespace().collect::<Vec<_>>().join(" ")));
            }
            rest = &rest[end..];
        }
    }
    Ok(v)
}

/// Re-indent XML (2 spaces); an element holding only text stays on one line.
pub fn pretty_xml(text: &str) -> Result<String, String> {
    let toks = xml_tokens(text)?;
    let mut out = String::new();
    let mut stack: Vec<String> = Vec::new();
    let pad = |n: usize| " ".repeat(n * 2);
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            XmlTok::Open(tag) => {
                let name = tag.trim_start_matches('<').split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or("").to_string();
                // <a>text</a> on one line
                if let (Some(XmlTok::Text(t)), Some(XmlTok::Close(n))) = (toks.get(i + 1), toks.get(i + 2)) {
                    if *n == name {
                        out.push_str(&format!("{}{tag}{t}</{n}>\n", pad(stack.len())));
                        i += 3;
                        continue;
                    }
                }
                if let Some(XmlTok::Close(n)) = toks.get(i + 1) {
                    if *n == name {
                        out.push_str(&format!("{}{tag}</{n}>\n", pad(stack.len())));
                        i += 2;
                        continue;
                    }
                }
                out.push_str(&format!("{}{tag}\n", pad(stack.len())));
                stack.push(name);
            }
            XmlTok::Close(n) => {
                match stack.pop() {
                    Some(open) if open == *n => {}
                    Some(open) => return Err(format!("</{n}> closes <{open}>.")),
                    None => return Err(format!("</{n}> without an opening tag.")),
                }
                out.push_str(&format!("{}</{n}>\n", pad(stack.len())));
            }
            XmlTok::Empty(t) | XmlTok::Other(t) | XmlTok::Text(t) => out.push_str(&format!("{}{t}\n", pad(stack.len()))),
        }
        i += 1;
    }
    if let Some(open) = stack.pop() {
        return Err(format!("<{open}> is not closed."));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json() {
        let p = pretty_json(r#"{"a":1,"b":[true,null,{}],"c":{"d":"x,{y}"},}"#).unwrap();
        assert_eq!(p, "{\n  \"a\": 1,\n  \"b\": [\n    true,\n    null,\n    {}\n  ],\n  \"c\": {\n    \"d\": \"x,{y}\"\n  }\n}");
        assert!(pretty_json("{\"a\": [1, 2}").is_err());
        assert!(pretty_json("{\"a\": \"x").is_err());
    }

    #[test]
    fn xml() {
        let p = pretty_xml(r#"<?xml version="1.0"?><a x="1>2"><b>text</b><c/><d><e></e></d></a>"#).unwrap();
        assert_eq!(p, "<?xml version=\"1.0\"?>\n<a x=\"1>2\">\n  <b>text</b>\n  <c/>\n  <d>\n    <e></e>\n  </d>\n</a>\n");
        assert!(pretty_xml("<a><b></a>").is_err());
    }
}
