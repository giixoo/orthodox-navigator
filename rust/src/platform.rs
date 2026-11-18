//! Everything that differs between Windows and Linux/macOS lives here.

use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// Shift / Ctrl / Alt held right now. Terminals don't report a modifier pressed on its own,
/// so on Windows we read the keyboard; elsewhere this is unknown (none).
pub fn modifiers() -> crossterm::event::KeyModifiers {
    use crossterm::event::KeyModifiers as M;
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        let down = |vk: i32| (unsafe { GetAsyncKeyState(vk) } as u16) & 0x8000 != 0;
        let mut m = M::NONE;
        if down(0x10) {
            m |= M::SHIFT;
        }
        if down(0x11) {
            m |= M::CONTROL;
        }
        if down(0x12) {
            m |= M::ALT;
        }
        m
    }
    #[cfg(not(windows))]
    M::NONE
}

/// The system's double-click time in milliseconds.
pub fn double_click_ms() -> u128 {
    #[cfg(windows)]
    {
        (unsafe { windows_sys::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime() } as u128).max(300)
    }
    #[cfg(not(windows))]
    500
}

pub fn default_shell() -> String {
    if cfg!(windows) {
        "cmd.exe".into()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    }
}

/// Hidden: Windows hidden/system attribute, or a leading dot on Unix.
#[allow(unused_variables)]
pub fn is_hidden(name: &str, md: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        md.file_attributes() & 0x6 != 0
    }
    #[cfg(not(windows))]
    {
        name.starts_with('.')
    }
}

/// Executable bit (Unix only; on Windows executables are recognised by extension).
#[allow(unused_variables)]
pub fn is_exec(md: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        md.is_file() && md.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub fn free_space(p: &Path) -> Option<u64> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
        let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut avail = 0u64;
        let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut avail, std::ptr::null_mut(), std::ptr::null_mut()) };
        if ok != 0 { Some(avail) } else { None }
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(p.as_os_str().as_bytes()).ok()?;
        let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut st) } == 0 {
            Some(st.f_bavail as u64 * st.f_frsize as u64)
        } else {
            None
        }
    }
}

/// "C:" on Windows; the mount point (e.g. "/home") on Unix.
pub fn volume_of(p: &Path) -> String {
    #[cfg(windows)]
    {
        match p.components().next() {
            Some(std::path::Component::Prefix(pre)) => pre.as_os_str().to_string_lossy().to_string(),
            _ => String::new(),
        }
    }
    #[cfg(not(windows))]
    {
        volumes()
            .into_iter()
            .filter(|m| p.starts_with(m))
            .max_by_key(|m| m.as_os_str().len())
            .map(|m| m.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".into())
    }
}

/// Drive letters for the "[ A C D ]" strip (Windows only).
pub fn drive_letters() -> Vec<char> {
    #[cfg(windows)]
    {
        let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };
        (0..26).filter(|i| mask & (1 << i) != 0).map(|i| (b'A' + i as u8) as char).collect()
    }
    #[cfg(not(windows))]
    {
        vec![]
    }
}

/// Roots for the drive menu: drive letters on Windows, mount points on Unix.
pub fn volumes() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        drive_letters().into_iter().map(|c| PathBuf::from(format!("{c}:\\"))).collect()
    }
    #[cfg(not(windows))]
    {
        let mut v = vec![PathBuf::from("/")];
        if let Ok(text) = std::fs::read_to_string("/proc/mounts") {
            for line in text.lines() {
                let mut parts = line.split_whitespace();
                let (Some(dev), Some(mnt)) = (parts.next(), parts.next()) else { continue };
                // real devices only (skip proc, sysfs, cgroup, ...)
                if !dev.starts_with("/dev/") || mnt.starts_with("/boot") || mnt.starts_with("/snap") {
                    continue;
                }
                let mnt = PathBuf::from(mnt.replace("\\040", " "));
                if !v.contains(&mnt) {
                    v.push(mnt);
                }
            }
        } else if let Ok(rd) = std::fs::read_dir("/Volumes") {
            v.extend(rd.flatten().map(|e| e.path())); // macOS
        }
        v
    }
}

/// Open a file with its default application.
pub fn open_default(p: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new("cmd").raw_arg(format!("/c start \"\" \"{}\"", p.display())).spawn().map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(p).spawn().map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open").arg(p).spawn().map(|_| ())
    }
}

pub fn move_to_trash(p: &Path) -> Result<(), String> {
    trash::delete(p).map_err(|e| e.to_string())
}

/// Make sure the console understands ANSI sequences (Windows).
pub fn enable_vt() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{GetConsoleMode, GetStdHandle, SetConsoleMode, ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_OUTPUT_HANDLE};
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(h, &mut mode) != 0 {
            SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

/// Build a command that runs `line` in the given shell.
pub fn shell_command(shell: &str, line: &str) -> Command {
    let mut c = Command::new(shell);
    if is_powershell(shell) {
        // -EncodedCommand avoids every quoting problem
        let utf16: Vec<u8> = line.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        c.args(["-NoLogo", "-NoProfile", "-EncodedCommand", &crate::util::base64(&utf16)]);
    } else if is_cmd(shell) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.raw_arg(format!("/s /c \"{line}\""));
        }
    } else {
        c.arg("-c").arg(line);
    }
    c
}

fn stem(shell: &str) -> String {
    Path::new(shell).file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn is_powershell(shell: &str) -> bool {
    matches!(stem(shell).as_str(), "pwsh" | "powershell")
}

pub fn is_cmd(shell: &str) -> bool {
    stem(shell) == "cmd"
}

/// Quote a single argument for the given shell.
pub fn quote(shell: &str, s: &str) -> String {
    if is_powershell(shell) {
        format!("'{}'", s.replace('\'', "''"))
    } else if is_cmd(shell) {
        if s.contains([' ', '&', '(', ')', '^', ',', ';']) { format!("\"{s}\"") } else { s.to_string() }
    } else if s.chars().all(|c| c.is_ascii_alphanumeric() || "._-/+".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Command line that runs a program in the current folder.
pub fn run_file(shell: &str, name: &str) -> String {
    if is_powershell(shell) {
        format!("& '.\\{}'", name.replace('\'', "''"))
    } else if is_cmd(shell) {
        quote(shell, name)
    } else {
        format!("./{}", quote(shell, name).trim_start_matches("./"))
    }
}

/// Put text on the Windows clipboard. Returns false where there is none (other systems use OSC 52).
#[cfg(windows)]
pub fn set_clipboard(text: &str) -> bool {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: standard clipboard sequence; the memory belongs to the clipboard once SetClipboardData succeeds
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return false;
        }
        EmptyClipboard();
        let h = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
        let mut ok = false;
        if !h.is_null() {
            let p = GlobalLock(h) as *mut u16;
            if !p.is_null() {
                std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                GlobalUnlock(h);
                ok = !SetClipboardData(CF_UNICODETEXT, h).is_null();
            }
            if !ok {
                GlobalFree(h);
            }
        }
        CloseClipboard();
        ok
    }
}

#[cfg(not(windows))]
pub fn set_clipboard(_text: &str) -> bool {
    false
}
