//! Saved passwords, encrypted with a key made from the master password (PBKDF2-HMAC-SHA256, then
//! ChaCha20-Poly1305), and the "unlock" file that keeps that key between ON's restarts around shell
//! commands (one per ON window, next to its session file; DPAPI-wrapped on Windows, 0600 elsewhere).

use crate::remote::profiles::Profiles;
use crate::tools::base64_decode;
use crate::util::base64;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub type Key = [u8; 32];

const ROUNDS: u32 = 300_000;
const CHECK: &str = "orthodox-navigator";

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("system random numbers");
    b
}

fn derive(master: &str, salt: &[u8]) -> Key {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(master.as_bytes(), salt, ROUNDS, &mut key);
    key
}

/// A new master password: its key and the (salt, check value) to keep in the profiles file.
pub fn new_vault(master: &str) -> (Key, (String, String)) {
    let salt: [u8; 16] = random();
    let key = derive(master, &salt);
    let check = seal(&key, CHECK);
    (key, (base64(&salt), check))
}

/// The key for this master password, or None when it is the wrong one.
pub fn unlock(master: &str, vault: &(String, String)) -> Option<Key> {
    let salt = base64_decode(&vault.0).ok()?;
    let key = derive(master, &salt);
    (open(&key, &vault.1).as_deref() == Some(CHECK)).then_some(key)
}

/// Encrypt: base64(nonce ‖ ciphertext).
pub fn seal(key: &Key, plain: &str) -> String {
    let nonce: [u8; 12] = random();
    let ct = ChaCha20Poly1305::new(key.into()).encrypt(Nonce::from_slice(&nonce), plain.as_bytes()).expect("encrypt");
    let mut all = nonce.to_vec();
    all.extend(ct);
    base64(&all)
}

/// Decrypt what `seal` made; None for a wrong key or damaged text.
pub fn open(key: &Key, sealed: &str) -> Option<String> {
    let all = base64_decode(sealed.trim()).ok()?;
    if all.len() < 12 + 16 {
        return None;
    }
    let pt = ChaCha20Poly1305::new(key.into()).decrypt(Nonce::from_slice(&all[..12]), &all[12..]).ok()?;
    String::from_utf8(pt).ok()
}

/// Re-encrypt every saved password with a new key (after changing the master password).
pub fn rekey(old: &Key, new: &Key, profiles: &mut Profiles) {
    for p in profiles.list.iter_mut() {
        p.password = p.password.as_deref().and_then(|s| open(old, s)).map(|pw| seal(new, &pw));
        if p.password.is_none() {
            p.save_password = false;
        }
    }
}

/// The file that keeps the key between restarts: "<session file>.unlock".
pub fn unlock_file(session_file: &Path) -> PathBuf {
    let mut s = session_file.as_os_str().to_owned();
    s.push(".unlock");
    PathBuf::from(s)
}

pub fn store_key(file: &Path, key: &Key) -> std::io::Result<()> {
    let data = protect(key)?;
    crate::remote::profiles::write_private(file, &data)
}

pub fn load_key(file: &Path) -> Option<Key> {
    let data = fs::read(file).ok()?;
    unprotect(&data)?.try_into().ok()
}

/// Delete unlock files left by an ON that ended without cleaning up: their session file is gone, or
/// they are older than a day.
pub fn clean_leftovers(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let day = Duration::from_secs(24 * 3600);
    for de in rd.flatten() {
        let p = de.path();
        let name = p.to_string_lossy().to_string();
        let Some(session) = name.strip_suffix(".unlock") else { continue };
        let old = de.metadata().and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok()).is_none_or(|age| age > day);
        if old || !Path::new(session).exists() {
            let _ = fs::remove_file(&p);
        }
    }
}

#[cfg(windows)]
fn protect(data: &[u8]) -> std::io::Result<Vec<u8>> {
    dpapi(data, true).ok_or_else(std::io::Error::last_os_error)
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    dpapi(data, false)
}

/// Windows DPAPI for the current user: CryptProtectData / CryptUnprotectData.
#[cfg(windows)]
fn dpapi(data: &[u8], encrypt: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    let ok = unsafe {
        if encrypt {
            CryptProtectData(&input, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        } else {
            CryptUnprotectData(&input, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
        }
    };
    if ok == 0 || out.pbData.is_null() {
        return None;
    }
    let v = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe { LocalFree(out.pbData as _) };
    Some(v)
}

#[cfg(not(windows))]
fn protect(data: &[u8]) -> std::io::Result<Vec<u8>> {
    Ok(data.to_vec())
}

#[cfg(not(windows))]
fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    Some(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_and_open() {
        let (key, vault) = new_vault("correct horse");
        let s = seal(&key, "p@ss wörd");
        assert_ne!(s, seal(&key, "p@ss wörd")); // a fresh nonce each time
        assert_eq!(open(&key, &s).as_deref(), Some("p@ss wörd"));
        assert_eq!(unlock("correct horse", &vault), Some(key));
    }

    #[test]
    fn wrong_master_detected() {
        let (_, vault) = new_vault("right");
        assert_eq!(unlock("wrong", &vault), None);
        assert_eq!(open(&[7u8; 32], &seal(&[1u8; 32], "x")), None);
        assert_eq!(open(&[7u8; 32], "not base64 !"), None);
    }

    #[test]
    fn rekey_keeps_passwords() {
        use crate::remote::profiles::{Profile, Profiles, Proto};
        let (old, _) = new_vault("a");
        let (new, vault) = new_vault("b");
        let mut p = Profiles { vault: None, list: vec![] };
        p.list.push(Profile { name: "x".into(), proto: Proto::Ftp, host: "h".into(), port: None, user: "u".into(), folder: String::new(), save_password: true, password: Some(seal(&old, "pw")), insecure: false });
        rekey(&old, &new, &mut p);
        assert_eq!(open(&new, p.list[0].password.as_deref().unwrap()).as_deref(), Some("pw"));
        let _ = vault;
    }

    #[test]
    fn key_file_round_trip() {
        let dir = std::env::temp_dir().join(format!("on-vault-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = unlock_file(&dir.join("session.txt"));
        store_key(&f, &[9u8; 32]).unwrap();
        #[cfg(windows)]
        assert_ne!(std::fs::read(&f).unwrap(), vec![9u8; 32]); // wrapped with DPAPI
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&f).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert_eq!(load_key(&f), Some([9u8; 32]));
        clean_leftovers(&dir); // session.txt doesn't exist, so its unlock file goes
        assert!(!f.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
