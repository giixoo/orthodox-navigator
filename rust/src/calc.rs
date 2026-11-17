//! DN-style calculator: type an expression, see DEC / HEX / BIN / OCT / EXP at once,
//! and copy the chosen form to the command line.
//!
//! Numbers: 42, 3.14, 1e6, 0xFF or $FF (hex), 0b1010, 0o17. Operators: + - * / % ^ (power),
//! & | << >> (on integers), parentheses. Functions: sqrt sin cos tan ln log abs round int exp.
//! Constants: pi, e.

use crate::app::{App, Input};
use crate::dialogs::{self, LineEdit};
use crate::screen::{Screen, C};
use crate::theme;
use crate::util::fit;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

// --------------------------------------------------------------------- parser

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

type R = Result<f64, String>;

impl Parser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, t: &str) -> bool {
        self.ws();
        if self.s[self.i..].starts_with(t.as_bytes()) {
            self.i += t.len();
            true
        } else {
            false
        }
    }

    fn int(v: f64) -> Result<i64, String> {
        if v.fract() != 0.0 || v.abs() > 9.2e18 { Err("needs whole numbers".into()) } else { Ok(v as i64) }
    }

    // or := and ('|' and)*
    fn or(&mut self) -> R {
        let mut v = self.and()?;
        while self.peek() == Some(b'|') {
            self.i += 1;
            v = (Self::int(v)? | Self::int(self.and()?)?) as f64;
        }
        Ok(v)
    }

    fn and(&mut self) -> R {
        let mut v = self.shift()?;
        while self.peek() == Some(b'&') {
            self.i += 1;
            v = (Self::int(v)? & Self::int(self.shift()?)?) as f64;
        }
        Ok(v)
    }

    fn shift(&mut self) -> R {
        let mut v = self.add()?;
        loop {
            if self.eat("<<") {
                v = (Self::int(v)?.wrapping_shl(Self::int(self.add()?)? as u32)) as f64;
            } else if self.eat(">>") {
                v = (Self::int(v)?.wrapping_shr(Self::int(self.add()?)? as u32)) as f64;
            } else {
                return Ok(v);
            }
        }
    }

    fn add(&mut self) -> R {
        let mut v = self.mul()?;
        loop {
            match self.peek() {
                Some(b'+') => {
                    self.i += 1;
                    v += self.mul()?;
                }
                Some(b'-') => {
                    self.i += 1;
                    v -= self.mul()?;
                }
                _ => return Ok(v),
            }
        }
    }

    fn mul(&mut self) -> R {
        let mut v = self.unary()?;
        loop {
            match self.peek() {
                Some(b'*') => {
                    self.i += 1;
                    v *= self.unary()?;
                }
                Some(b'/') => {
                    self.i += 1;
                    let d = self.unary()?;
                    if d == 0.0 {
                        return Err("division by zero".into());
                    }
                    v /= d;
                }
                Some(b'%') => {
                    self.i += 1;
                    let d = self.unary()?;
                    if d == 0.0 {
                        return Err("division by zero".into());
                    }
                    v %= d;
                }
                _ => return Ok(v),
            }
        }
    }

    fn unary(&mut self) -> R {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                Ok(-self.unary()?)
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    // power is right-associative: 2^3^2 = 2^9
    fn power(&mut self) -> R {
        let base = self.primary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            let exp = self.unary()?;
            return Ok(base.powf(exp));
        }
        Ok(base)
    }

    fn primary(&mut self) -> R {
        match self.peek() {
            Some(b'(') => {
                self.i += 1;
                let v = self.or()?;
                if !self.eat(")") {
                    return Err("missing )".into());
                }
                Ok(v)
            }
            Some(b'$') => {
                self.i += 1;
                self.radix(16)
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() => self.word(),
            Some(c) => Err(format!("unexpected '{}'", c as char)),
            None => Err("expression is incomplete".into()),
        }
    }

    fn radix(&mut self, base: u32) -> R {
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i] as char).is_digit(base) {
            self.i += 1;
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        i64::from_str_radix(text, base).map(|v| v as f64).map_err(|_| "bad number".into())
    }

    fn number(&mut self) -> R {
        if self.s[self.i..].len() > 2 && self.s[self.i] == b'0' {
            match self.s[self.i + 1].to_ascii_lowercase() {
                b'x' => {
                    self.i += 2;
                    return self.radix(16);
                }
                b'b' => {
                    self.i += 2;
                    return self.radix(2);
                }
                b'o' => {
                    self.i += 2;
                    return self.radix(8);
                }
                _ => {}
            }
        }
        let start = self.i;
        while self.i < self.s.len() && (self.s[self.i].is_ascii_digit() || self.s[self.i] == b'.') {
            self.i += 1;
        }
        // exponent: 1e6, 2.5E-3
        if self.i < self.s.len() && (self.s[self.i] | 0x20) == b'e' {
            let save = self.i;
            self.i += 1;
            if self.i < self.s.len() && (self.s[self.i] == b'+' || self.s[self.i] == b'-') {
                self.i += 1;
            }
            if self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                    self.i += 1;
                }
            } else {
                self.i = save;
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
        text.parse::<f64>().map_err(|_| format!("bad number '{text}'"))
    }

    fn word(&mut self) -> R {
        let start = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_alphanumeric() {
            self.i += 1;
        }
        let w = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("").to_lowercase();
        match w.as_str() {
            "pi" => return Ok(std::f64::consts::PI),
            "e" => return Ok(std::f64::consts::E),
            _ => {}
        }
        if !self.eat("(") {
            return Err(format!("unknown name '{w}'"));
        }
        let a = self.or()?;
        if !self.eat(")") {
            return Err("missing )".into());
        }
        Ok(match w.as_str() {
            "sqrt" => a.sqrt(),
            "sin" => a.sin(),
            "cos" => a.cos(),
            "tan" => a.tan(),
            "ln" => a.ln(),
            "log" => a.log10(),
            "abs" => a.abs(),
            "round" => a.round(),
            "int" | "trunc" => a.trunc(),
            "exp" => a.exp(),
            _ => return Err(format!("unknown function '{w}'")),
        })
    }
}

pub fn eval(expr: &str) -> R {
    let mut p = Parser { s: expr.as_bytes(), i: 0 };
    let v = p.or()?;
    if p.peek().is_some() {
        return Err(format!("unexpected '{}'", p.s[p.i] as char));
    }
    if !v.is_finite() {
        return Err("result is not a number".into());
    }
    Ok(v)
}

// ------------------------------------------------------------------- formats

const FORMATS: [&str; 5] = ["DEC", "HEX", "BIN", "OCT", "EXP"];

fn as_int(v: f64) -> Option<i64> {
    (v.fract() == 0.0 && v.abs() < 9.2e18).then_some(v as i64)
}

fn radix(v: f64, f: usize) -> String {
    let Some(n) = as_int(v) else { return "—".into() };
    let sign = if n < 0 { "-" } else { "" };
    let m = n.unsigned_abs();
    match f {
        1 => format!("{sign}{m:X}"),
        2 => format!("{sign}{m:b}"),
        _ => format!("{sign}{m:o}"),
    }
}

/// DN-like exponent form: 4.000000000000000E+0000
fn exp_form(v: f64) -> String {
    let s = format!("{v:.15E}");
    match s.split_once('E') {
        Some((m, e)) => {
            let e: i32 = e.parse().unwrap_or(0);
            format!("{m}E{}{:04}", if e < 0 { '-' } else { '+' }, e.abs())
        }
        None => s,
    }
}

pub fn format(v: f64, f: usize) -> String {
    match f {
        0 => match as_int(v) {
            Some(n) => n.to_string(),
            None => {
                let s = format!("{v:.12}");
                s.trim_end_matches('0').trim_end_matches('.').to_string()
            }
        },
        1..=3 => radix(v, f),
        _ => exp_form(v),
    }
}

// --------------------------------------------------------------------- window

const BUTTONS: [&str; 3] = ["Evaluate", "Copy", "Close"];

/// Show the calculator. Returns the text to put on the command line, if the user chose Copy.
pub fn show(app: &mut App) -> Option<String> {
    let mut ed = LineEdit::new("");
    let mut fmt = 0usize;
    // focus: 0 = expression, 1..=3 = buttons
    let mut focus = 0usize;
    loop {
        let (sw, sh) = (app.screen.w, app.screen.h);
        let w = (sw - 2).min(64);
        let h = 15;
        let (x, y) = (((sw - w) / 2).max(0), ((sh - h) / 2).max(0));
        let result = if ed.value().trim().is_empty() { Err(String::new()) } else { eval(&ed.value()) };
        let (cur_fmt, cur_focus) = (fmt, focus);
        let res = result.clone();
        let input = app.modal_input(
            &mut |s: &mut Screen| {
                let t = theme::current();
                dialogs::frame_at(s, x, y, w, h, "Calculator");
                let (fg, bg) = (t.dlg_fg, t.dlg_bg);
                // expression
                dialogs::hot_text(s, x + 4, y + 2, "Expression", t.dlg_title, t.dlg_hot, bg);
                s.boxed(x + 3, y + 3, w - 6, 3, t.dlg_frame, bg, false);
                ed.draw(s, x + 5, y + 4, w - 10, t.input_fg, t.input_bg);
                if cur_focus != 0 {
                    s.cursor = None;
                }
                // results with the "copy as" choice
                s.text(x + 4, y + 7, "Copy As", fg, bg);
                s.boxed(x + 14, y + 6, w - 18, 7, t.dlg_frame, bg, false);
                for (i, name) in FORMATS.iter().enumerate() {
                    let ry = y + 7 + i as i32;
                    let mark = if i == cur_fmt { "(•)" } else { "( )" };
                    s.text(x + 4, ry, mark, fg, bg);
                    s.text(x + 8, ry, name, t.dlg_hot, bg);
                    let value = match &res {
                        Ok(v) => format(*v, i),
                        Err(e) if i == 0 && !e.is_empty() => e.clone(),
                        Err(_) => String::new(),
                    };
                    let vw = (w - 22) as usize;
                    let shown = if value.chars().count() > vw { fit(&value, vw) } else { format!("{value:>vw$}") };
                    let color = if res.is_err() { C::Red } else { fg };
                    s.text(x + 16, ry, &shown, color, bg);
                }
                // buttons
                let mut bx = x + 4;
                for (i, b) in BUTTONS.iter().enumerate() {
                    dialogs::button(s, bx, y + h - 2, b, cur_focus == i + 1, bg);
                    bx += dialogs::button_width(b) + 3;
                }
            },
            None,
        );

        let activate = |which: usize, ed: &mut LineEdit| -> Option<Option<String>> {
            match which {
                // Evaluate: replace the expression with its value so you can keep calculating
                0 | 1 => {
                    if let Ok(v) = &result {
                        ed.set(&format(*v, 0));
                    }
                    None
                }
                2 => Some(result.as_ref().ok().map(|v| format(*v, fmt))),
                _ => Some(None),
            }
        };

        match input {
            Input::Key(k) => match k.code {
                KeyCode::Esc => return None,
                KeyCode::Tab => focus = (focus + 1) % 4,
                KeyCode::BackTab => focus = (focus + 3) % 4,
                KeyCode::Up => fmt = (fmt + FORMATS.len() - 1) % FORMATS.len(),
                KeyCode::Down => fmt = (fmt + 1) % FORMATS.len(),
                KeyCode::Enter => {
                    if let Some(r) = activate(focus, &mut ed) {
                        return r;
                    }
                }
                _ if focus == 0 => {
                    ed.handle(&k);
                }
                KeyCode::Left => focus = if focus > 1 { focus - 1 } else { 3 },
                KeyCode::Right => focus = if focus < 3 { focus + 1 } else { 1 },
                _ => {}
            },
            Input::Mouse(m) if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                if my == y + h - 2 {
                    let mut bx = x + 4;
                    for i in 0..BUTTONS.len() {
                        let bw = dialogs::button_width(BUTTONS[i]);
                        if mx >= bx && mx < bx + bw {
                            if let Some(r) = activate(i + 1, &mut ed) {
                                return r;
                            }
                        }
                        bx += bw + 3;
                    }
                } else if (y + 7..y + 12).contains(&my) && mx < x + 14 {
                    fmt = (my - y - 7) as usize;
                } else if my == y + 4 {
                    focus = 0;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic() {
        assert_eq!(eval("2+2").unwrap(), 4.0);
        assert_eq!(eval("2+3*4").unwrap(), 14.0);
        assert_eq!(eval("(2+3)*4").unwrap(), 20.0);
        assert_eq!(eval("-2^2").unwrap(), -4.0);
        assert_eq!(eval("2^3^2").unwrap(), 512.0);
        assert_eq!(eval("10 % 4").unwrap(), 2.0);
        assert_eq!(eval("1e3 + .5").unwrap(), 1000.5);
        assert!(eval("1/0").is_err());
        assert!(eval("2+").is_err());
    }

    #[test]
    fn bases_and_functions() {
        assert_eq!(eval("0xFF").unwrap(), 255.0);
        assert_eq!(eval("$10 + 0b11 + 0o10").unwrap(), 27.0);
        assert_eq!(eval("0xF0 | 0x0F").unwrap(), 255.0);
        assert_eq!(eval("1 << 10").unwrap(), 1024.0);
        assert_eq!(eval("sqrt(16) + abs(-1)").unwrap(), 5.0);
        assert!((eval("sin(pi/2)").unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn formats() {
        assert_eq!(format(4.0, 0), "4");
        assert_eq!(format(4.0, 1), "4");
        assert_eq!(format(4.0, 2), "100");
        assert_eq!(format(255.0, 1), "FF");
        assert_eq!(format(4.0, 4), "4.000000000000000E+0000");
        assert_eq!(format(0.5, 0), "0.5");
    }
}
