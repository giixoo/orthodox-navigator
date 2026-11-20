//! Remote panels on screen: the connection form, password and master-password dialogs, the answers to
//! ssh's questions, and connecting a panel.

use crate::app::{App, Input};
use crate::dialogs::{self, LineEdit};
use crate::remote::askpass::{kind, PromptKind};
use crate::remote::profiles::{self, Profile, Proto};
use crate::remote::vault::{self, Key};
use crate::remote::{RemoteView, R};
use crate::theme;
use crate::util::fit;
use crossterm::event::{KeyCode, MouseButton, MouseEventKind};

// ------------------------------------------------------------------ a small form

enum Field {
    Text { label: &'static str, ed: LineEdit, masked: bool },
    Choice { label: &'static str, options: Vec<&'static str>, i: usize },
    Check { label: &'static str, on: bool },
}

impl Field {
    fn text(label: &'static str, v: &str) -> Field {
        Field::Text { label, ed: LineEdit::new(v), masked: false }
    }
    fn secret(label: &'static str) -> Field {
        Field::Text { label, ed: LineEdit::new(""), masked: true }
    }
    fn value(&self) -> String {
        match self {
            Field::Text { ed, .. } => ed.value(),
            Field::Choice { options, i, .. } => options[*i].to_string(),
            Field::Check { on, .. } => if *on { "yes".into() } else { "no".into() },
        }
    }
    fn on(&self) -> bool {
        matches!(self, Field::Check { on: true, .. })
    }
}

/// A dialog with fields; Enter accepts (when `check` says the values are fine), Esc cancels.
/// Tab / ↑↓ move between fields, Space or ←→ change a choice, Space ticks a check box.
fn form(app: &mut App, title: &str, note: &str, fields: &mut [Field], check: &mut dyn FnMut(&mut App, &[Field]) -> bool) -> bool {
    app.over(|app| form_over(app, title, note, fields, check))
}

fn form_over(app: &mut App, title: &str, note: &str, fields: &mut [Field], check: &mut dyn FnMut(&mut App, &[Field]) -> bool) -> bool {
    let mut focus = 0usize;
    let lw = fields
        .iter()
        .map(|f| match f {
            Field::Text { label, .. } | Field::Choice { label, .. } => label.chars().count(),
            Field::Check { .. } => 0,
        })
        .max()
        .unwrap_or(0) as i32
        + 2;
    loop {
        let w = (app.screen.w - 4).clamp(30, 66);
        let note_lines: Vec<&str> = if note.is_empty() { vec![] } else { note.lines().collect() };
        let h = fields.len() as i32 + note_lines.len() as i32 + 5;
        let (x, y) = (((app.screen.w - w) / 2).max(0), ((app.screen.h - h) / 2).max(0));
        let cur = focus;
        let fields_ref = &mut *fields;
        let inp = app.modal_input(
            &mut |s| {
                let t = theme::current();
                dialogs::frame_at(s, x, y, w, h, title);
                let mut ry = y + 2;
                for l in &note_lines {
                    s.text(x + 4, ry, &fit(l, (w - 8) as usize), t.dlg_fg, t.dlg_bg);
                    ry += 1;
                }
                let mut cursor = None;
                for (i, f) in fields_ref.iter_mut().enumerate() {
                    let on = i == cur;
                    match f {
                        Field::Text { label, ed, masked } => {
                            s.text(x + 4, ry, label, if on { t.dlg_hot } else { t.dlg_fg }, t.dlg_bg);
                            let fw = w - 8 - lw;
                            if *masked {
                                ed.draw_masked(s, x + 4 + lw, ry, fw, t.input_fg, t.input_bg);
                            } else {
                                ed.draw(s, x + 4 + lw, ry, fw, t.input_fg, t.input_bg);
                            }
                            if on {
                                cursor = s.cursor;
                            }
                        }
                        Field::Choice { label, options, i: k } => {
                            s.text(x + 4, ry, label, if on { t.dlg_hot } else { t.dlg_fg }, t.dlg_bg);
                            let v = format!("◄ {} ►", options[*k]);
                            s.text(x + 4 + lw, ry, &v, t.input_fg, t.input_bg);
                        }
                        Field::Check { label, on: c } => {
                            let v = format!("[{}] {label}", if *c { 'x' } else { ' ' });
                            s.text(x + 4, ry, &fit(&v, (w - 8) as usize), if on { t.dlg_hot } else { t.dlg_fg }, t.dlg_bg);
                            if on {
                                cursor = Some((x + 5, ry));
                            }
                        }
                    }
                    ry += 1;
                }
                s.text(x + 4, y + h - 2, &fit("Enter OK   Esc cancel   Tab next field", (w - 8) as usize), t.dlg_fg, t.dlg_bg);
                s.cursor = cursor;
            },
            None,
        );
        match inp {
            Input::Key(k) => match k.code {
                KeyCode::Esc => return false,
                KeyCode::Enter => {
                    if check(app, fields) {
                        return true;
                    }
                }
                KeyCode::Tab | KeyCode::Down => focus = (focus + 1) % fields.len(),
                KeyCode::BackTab | KeyCode::Up => focus = (focus + fields.len() - 1) % fields.len(),
                _ => match &mut fields[focus] {
                    Field::Text { ed, .. } => {
                        ed.handle(&k);
                    }
                    Field::Choice { options, i, .. } => match k.code {
                        KeyCode::Left => *i = (*i + options.len() - 1) % options.len(),
                        KeyCode::Right | KeyCode::Char(' ') => *i = (*i + 1) % options.len(),
                        _ => {}
                    },
                    Field::Check { on, .. } => {
                        if k.code == KeyCode::Char(' ') {
                            *on = !*on;
                        }
                    }
                },
            },
            Input::Mouse(m) if matches!(m.kind, MouseEventKind::Down(MouseButton::Left)) => {
                let (mx, my) = (m.column as i32, m.row as i32);
                if dialogs::is_close_click(x, y, mx, my) || mx < x || mx >= x + w || my < y || my >= y + h {
                    return false;
                }
                let first = y + 2 + note_lines.len() as i32;
                let r = my - first;
                if r >= 0 && (r as usize) < fields.len() {
                    focus = r as usize;
                    if let Field::Check { on, .. } = &mut fields[focus] {
                        *on = !*on;
                    }
                }
            }
            _ => {}
        }
    }
}

/// A password dialog, optionally with a check box (returns the password and the box's state).
pub fn secret(app: &mut App, title: &str, prompt: &str, check: Option<(&'static str, bool)>) -> Option<(String, bool)> {
    let mut fields = vec![Field::secret("Password")];
    if let Some((label, on)) = check {
        fields.push(Field::Check { label, on });
    }
    if !form(app, title, prompt.trim(), &mut fields, &mut |_, _| true) {
        return None;
    }
    Some((fields[0].value(), fields.get(1).is_some_and(Field::on)))
}

// ------------------------------------------------------------------ the master password

/// A new master password, typed twice.
fn new_master(app: &mut App) -> Option<String> {
    loop {
        let mut fields = vec![Field::secret("Master password"), Field::secret("Once more")];
        let note = "Saved passwords are encrypted with a master password.\nON asks for it once, when it first needs a saved password.";
        if !form(app, "New master password", note, &mut fields, &mut |_, _| true) {
            return None;
        }
        let (a, b) = (fields[0].value(), fields[1].value());
        if a.is_empty() {
            dialogs::message(app, "Master password", "The master password can't be empty.", true);
        } else if a != b {
            dialogs::message(app, "Master password", "The two passwords are different.", true);
        } else {
            return Some(a);
        }
    }
}

/// Keep the key for this ON window (between restarts around shell commands).
fn remember_key(app: &mut App, key: Key) {
    app.vault_key = Some(key);
    if let Some(f) = app.unlock_file() {
        let _ = vault::store_key(&f, &key);
    }
}

/// The key for the saved passwords: remembered, from this window's unlock file, or asked (3 tries).
pub fn unlock_vault(app: &mut App) -> Option<Key> {
    let v = app.remote_profiles.vault.clone()?;
    match app.vault_key {
        Some(k) if vault::unlock_check(&k, &v) => return Some(k),
        // the master password was changed in another ON window
        Some(_) => app.vault_key = None,
        None => {}
    }
    if let Some(k) = app.unlock_file().and_then(|f| vault::load_key(&f)) {
        if vault::unlock_check(&k, &v) {
            app.vault_key = Some(k);
            return Some(k);
        }
    }
    for i in 0..3 {
        let prompt = if i == 0 { "The master password for saved passwords:" } else { "Wrong master password. Try again:" };
        let (m, _) = secret(app, "Master password", prompt, None)?;
        if let Some(k) = vault::unlock(&m, &v) {
            remember_key(app, k);
            return Some(k);
        }
    }
    None
}

/// The key, making the vault first if there is none yet (asks for a new master password).
fn vault_for_saving(app: &mut App) -> Option<Key> {
    // another ON window may have made the vault meanwhile
    app.remote_profiles = profiles::load();
    if app.remote_profiles.vault.is_some() {
        return unlock_vault(app);
    }
    let m = new_master(app)?;
    let (k, v) = vault::new_vault(&m);
    let mine = v.clone();
    update_profiles(app, move |p| {
        if p.vault.is_none() {
            p.vault = Some(v);
        }
    });
    if app.remote_profiles.vault.as_ref() != Some(&mine) {
        // made in another window at the same moment: use that one
        return unlock_vault(app);
    }
    remember_key(app, k);
    Some(k)
}

/// Change the connections file as it is now (another ON window may have changed it).
fn update_profiles(app: &mut App, change: impl FnOnce(&mut profiles::Profiles)) {
    match profiles::update(change) {
        Ok(p) => app.remote_profiles = p,
        Err(e) => dialogs::message(app, "Remote", &format!("Can't save the connections: {e}"), true),
    }
}

/// Options → Change master password.
pub fn change_master(app: &mut App) {
    app.remote_profiles = profiles::load();
    if app.remote_profiles.vault.is_none() {
        return dialogs::message(app, "Master password", "There are no saved passwords yet.\nThe master password is set when you save the first one.", false);
    }
    let Some(old) = unlock_vault(app) else { return };
    let Some(m) = new_master(app) else { return };
    let (k, v) = vault::new_vault(&m);
    update_profiles(app, |p| {
        vault::rekey(&old, &k, p);
        p.vault = Some(v);
    });
    remember_key(app, k);
    dialogs::message(app, "Master password", "The master password was changed.", false);
}

/// Options → Forget saved passwords (also the way out of a forgotten master password).
pub fn forget_passwords(app: &mut App) {
    let q = "Forget all saved passwords and the master password?";
    if dialogs::choice(app, "Saved passwords", &[q, "They are asked again on the next connection."], &["Forget", "Cancel"], true) != Some(0) {
        return;
    }
    update_profiles(app, |all| {
        for p in all.list.iter_mut() {
            p.password = None;
        }
        all.vault = None;
    });
    app.vault_key = None;
    if let Some(f) = app.unlock_file() {
        let _ = std::fs::remove_file(f);
    }
}

// ------------------------------------------------------------------ ssh's questions

/// Answer a question from ssh / the FTP login for a profile. The first password question of an
/// operation gets the typed or saved password without a dialog; another one means it was wrong.
pub fn answer(app: &mut App, profile: &Profile, prompt: &str) -> Option<String> {
    match kind(prompt) {
        PromptKind::HostKey => {
            let mut lines: Vec<&str> = prompt.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("Are you sure") && !l.starts_with("Please type")).collect();
            lines.push("");
            lines.push("Trust this server and remember its key?");
            (dialogs::choice(app, "New server", &lines, &["Yes", "No"], false) == Some(0)).then(|| "yes".to_string())
        }
        PromptKind::Password => {
            let n = app.prompt_count.entry(profile.name.clone()).or_insert(0);
            *n += 1;
            let first = *n == 1;
            let saved = app.remote_profiles.list.iter().find(|p| p.name == profile.name).cloned().unwrap_or_else(|| profile.clone());
            if first {
                if let Some(pw) = app.typed_passwords.get(&profile.name) {
                    return Some(pw.clone());
                }
                if let Some(sealed) = saved.password.clone().filter(|_| saved.save_password) {
                    if let Some(pw) = unlock_vault(app).and_then(|k| vault::open(&k, &sealed)) {
                        return Some(pw);
                    }
                }
            }
            let prompt = if first { prompt.to_string() } else { format!("Wrong password.\n{}", prompt.trim()) };
            let (pw, save) = secret(app, &profile.name, &prompt, Some(("Save password", saved.save_password)))?;
            app.typed_passwords.insert(profile.name.clone(), pw.clone());
            let sealed = if save { vault_for_saving(app).map(|k| vault::seal(&k, &pw)) } else { None };
            update_profiles(app, |all| {
                if let Some(p) = all.list.iter_mut().find(|p| p.name == profile.name) {
                    p.save_password = save && sealed.is_some();
                    p.password = sealed;
                }
            });
            Some(pw)
        }
        PromptKind::Passphrase | PromptKind::Other => secret(app, &profile.name, prompt, None).map(|(s, _)| s),
    }
}

// ------------------------------------------------------------------ profiles

const PROTOS: [&str; 4] = ["SFTP", "FTP", "FTPS", "FTPS implicit (port 990)"];

/// The connection form. `taken`: names other profiles have.
pub fn edit_profile(app: &mut App, p: Option<&Profile>, taken: &[String]) -> Option<Profile> {
    let d = p.cloned().unwrap_or(Profile { name: String::new(), proto: Proto::Sftp, host: String::new(), port: None, user: String::new(), folder: String::new(), save_password: false, password: None, insecure: false });
    let mut fields = vec![
        Field::text("Name", &d.name),
        Field::Choice { label: "Protocol", options: PROTOS.to_vec(), i: Proto::ALL.iter().position(|x| *x == d.proto).unwrap_or(0) },
        Field::text("Host", &d.host),
        Field::text("Port", &d.port.map(|n| n.to_string()).unwrap_or_default()),
        Field::text("User", &d.user),
        Field::text("Start folder", &d.folder),
        Field::Check { label: "Save password (encrypted with a master password)", on: d.save_password },
        Field::Check { label: "FTPS: don't check the server's certificate", on: d.insecure },
    ];
    let note = "SFTP uses your ssh setup: the host can be a name from ~/.ssh/config,\nand user / port may stay empty. The password is asked when you connect.";
    let title = if p.is_some() { "Edit connection" } else { "New connection" };
    let ok = form(app, title, note, &mut fields, &mut |app, f| {
        let name = f[0].value().trim().to_string();
        let msg = if name.is_empty() {
            "Give the connection a name."
        } else if taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
            "Another connection has this name."
        } else if f[2].value().trim().is_empty() {
            "Which host?"
        } else if !f[3].value().trim().is_empty() && f[3].value().trim().parse::<u16>().is_err() {
            "The port is a number (or empty for the default)."
        } else {
            return true;
        };
        dialogs::message(app, title, msg, true);
        false
    });
    if !ok {
        return None;
    }
    let proto = Proto::ALL[PROTOS.iter().position(|x| *x == fields[1].value()).unwrap_or(0)];
    let save = fields[6].on();
    Some(Profile {
        name: fields[0].value().trim().to_string(),
        proto,
        host: fields[2].value().trim().to_string(),
        port: fields[3].value().trim().parse().ok(),
        user: fields[4].value().trim().to_string(),
        folder: fields[5].value().trim().to_string(),
        save_password: save,
        password: if save { d.password } else { None },
        insecure: fields[7].on(),
    })
}

/// Connect with the profile of this name. A certificate that can't be verified asks "Connect anyway"
/// once and remembers the answer in the profile.
pub fn connect(app: &mut App, name: &str) -> R<RemoteView> {
    // as it is on disk now (another ON window may have changed it)
    app.remote_profiles = profiles::load();
    let mut p = app.remote_profiles.list.iter().find(|p| p.name == name).cloned().ok_or("No such connection.")?;
    app.prompt_count.remove(&p.name);
    let conn = match crate::remote::connect(app, &p) {
        Ok(c) => c,
        Err(e) if e.starts_with(crate::remote::ftp::CERT_ERR) => {
            let detail: Vec<&str> = e.lines().collect();
            if dialogs::choice(app, "Certificate", &detail, &["Connect anyway", "Cancel"], true) != Some(0) {
                return Err("Cancelled.".into());
            }
            p.insecure = true;
            update_profiles(app, |all| {
                if let Some(q) = all.list.iter_mut().find(|q| q.name == p.name) {
                    q.insecure = true;
                }
            });
            app.prompt_count.remove(&p.name);
            crate::remote::connect(app, &p)?
        }
        Err(e) => return Err(e),
    };
    let mut view = RemoteView { profile: p.name.clone(), cwd: "/".into(), conn, base: profiles::title(&p, "") .trim_end_matches('/').to_string() };
    view.cwd = view.conn.home(app)?;
    if !p.folder.is_empty() {
        view.cwd = if p.folder.starts_with('/') || p.proto != Proto::Sftp { format!("/{}", p.folder.trim_start_matches('/')) } else { crate::remote::join(&view.cwd, &p.folder) };
    }
    Ok(view)
}
