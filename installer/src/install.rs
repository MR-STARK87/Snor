//! Installing and uninstalling, for real.
//!
//! Everything here is Windows-specific on purpose — Snor is — and everything
//! it does is per-user: files under `%LOCALAPPDATA%\Programs\Snor`, shortcuts
//! in the user's own Start Menu and Desktop, and one key in `HKCU`'s Uninstall
//! hive. No elevation is ever requested, which also means no UAC prompt can
//! appear mid-wizard.
//!
//! External tools, and why each one is the right tool:
//!
//! - `powershell.exe` creates the `.lnk` files. A shortcut is a COM object
//!   (`WScript.Shell`), and the known-folder call it makes
//!   (`[Environment]::GetFolderPath`) resolves a redirected Desktop correctly
//!   where a hardcoded `%USERPROFILE%\Desktop` does not. The app already talks
//!   to PowerShell for WMI brightness — the same precedent.
//! - `reg.exe` writes the Uninstall key. There is no registry API in std, and
//!   a winapi dependency for ten well-known values is not worth it.
//! - `cmd.exe` removes the install folder *after* this process exits, because
//!   the uninstaller runs from inside the directory it has to delete.
//!
//! The shortcut paths are recorded in `uninstall.info` at install time, and
//! the *recorded* paths are what uninstall removes — never a re-derived guess,
//! so a shortcut the user moved by hand is left alone instead of a second file
//! being deleted from where the shortcut used to live.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::payload::Bundle;

/// Hidden console for every helper process: the wizard has no console of its
/// own, and a black window flashing between stages reads as a crash.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The key Windows shows in Settings > Apps. `HKCU`, because the install is
/// per-user.
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Snor";

/// Written into the install directory. Also the marker that tells a copied
/// `uninstall.exe` (which never carries a payload) what it is — see
/// `Launch::detect`.
pub const MANIFEST: &str = "uninstall.info";

pub const INSTALL_STAGES: [&str; 3] = [
    "Copying files",
    "Creating shortcuts",
    "Registering with Windows",
];
pub const UNINSTALL_STAGES: [&str; 3] = [
    "Removing shortcuts",
    "Unregistering with Windows",
    "Removing files",
];

/// What the user chose on the options screen.
#[derive(Clone, Debug)]
pub struct Plan {
    pub dir: PathBuf,
    pub start_menu: bool,
    pub desktop: bool,
}

/// `%LOCALAPPDATA%\Programs\Snor`: the per-user location modern Windows apps
/// use, chosen because it is writable without elevation. The fallbacks keep a
/// stripped environment resolving somewhere sane rather than nowhere.
pub fn default_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|home| PathBuf::from(home).join("AppData").join("Local"))
        })
        .unwrap_or_else(|| PathBuf::from("C:\\"));
    base.join("Programs").join("Snor")
}

/// Write one file, with the one failure worth spelling out spelled out: a
/// locked `snor.exe` is the error a user actually hits (Snor still open), and
/// the raw "os error 5" does not say what to do about it.
fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| {
        if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "could not write {} (access denied — is Snor still running?)",
                path.display()
            )
        } else {
            format!("could not write {}: {e}", path.display())
        }
    })
}
/// Install Snor. `report(i)` is called as stage `i` starts.
///
/// There is no rollback. The steps that can fail mid-way are file writes,
/// which a retry overwrites cleanly, and the error text names the fix; a
/// half-set-up state that a second run completes is a better deal than a
/// rollback that can itself fail.
pub fn install(
    plan: &Plan,
    bundle: &Bundle,
    own_exe: &Path,
    report: &mut dyn FnMut(usize),
) -> Result<(), String> {
    let dir = &plan.dir;

    report(0);
    std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let exe = dir.join("snor.exe");
    write_file(&exe, &bundle.exe)?;
    // The licences ship beside the app, visibly, rather than buried in an
    // about box.
    write_file(&dir.join("LICENSE-MIT"), include_bytes!("../../LICENSE-MIT"))?;
    write_file(
        &dir.join("LICENSE-APACHE"),
        include_bytes!("../../LICENSE-APACHE"),
    )?;

    report(1);
    let shortcuts = create_shortcuts(dir, &exe, plan.start_menu, plan.desktop)?;

    report(2);
    // The uninstaller: this binary without its payload. `prefix_len` is where
    // the app's bytes begin, so the copy is exactly the setup binary —
    // clamped, so a wrong count can never read past the end of the file.
    let own = std::fs::read(own_exe).map_err(|e| format!("could not read {}: {e}", own_exe.display()))?;
    let prefix_len = (bundle.prefix_len as usize).min(own.len());
    write_file(&dir.join("uninstall.exe"), &own[..prefix_len])?;
    write_manifest(dir, env!("CARGO_PKG_VERSION"), &shortcuts)?;
    register(dir, &exe)?;
    Ok(())
}

/// Uninstall Snor. The installed `uninstall.exe` is running from inside `dir`,
/// which is why the directory's own removal is handed off rather than done.
pub fn uninstall(dir: &Path, report: &mut dyn FnMut(usize)) -> Result<(), String> {
    report(0);
    for shortcut in read_manifest(dir).shortcuts {
        // Best effort: a shortcut the user already deleted is the outcome we
        // wanted, not an error to report on the last screen.
        let _ = std::fs::remove_file(shortcut);
    }

    report(1);
    // A key that is already gone is also the outcome we wanted.
    let _ = reg_delete(UNINSTALL_KEY);

    report(2);
    for name in ["snor.exe", "LICENSE-MIT", "LICENSE-APACHE", MANIFEST] {
        let path = dir.join(name);
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!(
                        "could not remove {} — close Snor, then run the uninstaller again",
                        path.display()
                    )
                } else {
                    format!("could not remove {}: {e}", path.display())
                }
            })?;
        }
    }
    schedule_cleanup(dir);
    Ok(())
}

/// Hand the directory's own removal to a detached `cmd`, which waits for this
/// process to exit and then takes the folder — including the running
/// uninstaller. The script goes into a batch file rather than a
/// `cmd /C "<script>"` argument, and that is not a style choice: Rust wraps
/// any argument containing spaces in quotes and escapes the quotes inside it
/// with `\"`, which `cmd.exe` does not understand. The first live run of this
/// uninstaller deleted every file but left the folder behind, because the
/// `rmdir` argument arrived mangled. In a file, the quoting is ours alone.
///
/// The working directory moves off the folder first, or the handle this
/// process holds on it would keep `rmdir` from succeeding.
pub fn schedule_cleanup(dir: &Path) {
    let batch = std::env::temp_dir().join(format!("snor-uninstall-{}.cmd", std::process::id()));
    if std::fs::write(&batch, cleanup_script(dir)).is_err() {
        return;
    }
    let _ = Command::new("cmd.exe")
        .args(["/C", &batch.display().to_string()])
        .current_dir(std::env::temp_dir())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// The batch file the cleanup runs.
///
/// `ping` is the delay because it is the one always-present command that
/// cannot fail on a redirected stdin (the classic `timeout` hop can), and the
/// `for` loop re-tries because the uninstaller's own image can stay locked for
/// a beat after it exits — one attempt is a race, thirty is not. The last
/// `del` is the file deleting itself, including on the give-up path.
fn cleanup_script(dir: &Path) -> String {
    // `%` is legal in a Windows directory name and is batch syntax; doubling
    // it is the escape. Nothing else inside a quoted path is special.
    let dir = dir.display().to_string().replace('%', "%%");
    format!(
        "@echo off\r\n\
         rem Removes the Snor install folder once its last process has exited, then itself.\r\n\
         ping -n 2 127.0.0.1 >NUL\r\n\
         for /L %%i in (1,1,30) do (\r\n\
         \x20 rmdir /S /Q \"{dir}\" >NUL 2>&1\r\n\
         \x20 if not exist \"{dir}\" (del \"%~f0\" & exit /b)\r\n\
         \x20 ping -n 2 127.0.0.1 >NUL\r\n\
         )\r\n\
         del \"%~f0\"\r\n"
    )
}

/// The entry Windows shows in Settings > Apps, so a per-user install is still
/// removable from the place people look for it.
fn register(dir: &Path, exe: &Path) -> Result<(), String> {
    let quoted_uninstaller = format!("\"{}\" --uninstall", dir.join("uninstall.exe").display());
    let size_kb = dir_size_kb(dir);
    let values: [(&str, &str, String); 10] = [
        ("DisplayName", "REG_SZ", "Snor".into()),
        ("DisplayVersion", "REG_SZ", env!("CARGO_PKG_VERSION").into()),
        ("Publisher", "REG_SZ", "Snor".into()),
        ("URLInfoAbout", "REG_SZ", "https://github.com/MR-STARK87/Snor".into()),
        ("InstallLocation", "REG_SZ", dir.display().to_string()),
        ("DisplayIcon", "REG_SZ", format!("{},0", exe.display())),
        ("UninstallString", "REG_SZ", quoted_uninstaller),
        ("NoModify", "REG_DWORD", "1".into()),
        ("NoRepair", "REG_DWORD", "1".into()),
        ("EstimatedSize", "REG_DWORD", size_kb.to_string()),
    ];
    for (name, kind, value) in values {
        reg_add(UNINSTALL_KEY, name, kind, &value)?;
    }
    Ok(())
}

fn dir_size_kb(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.metadata().ok())
                .filter(|meta| meta.is_file())
                .map(|meta| meta.len())
                .sum::<u64>()
                / 1024
        })
        .unwrap_or(0)
}
/// Create the shortcuts (WScript.Shell through PowerShell) and return the
/// paths that were written, so uninstall can remove exactly those.
fn create_shortcuts(
    dir: &Path,
    exe: &Path,
    start_menu: bool,
    desktop: bool,
) -> Result<Vec<PathBuf>, String> {
    if !start_menu && !desktop {
        return Ok(Vec::new());
    }
    let target = exe.display().to_string();
    let working = dir.display().to_string();
    let icon = format!("{},0", exe.display());
    let mut script = String::from(
        "$ErrorActionPreference = 'Stop'\n\
         [Console]::OutputEncoding = [System.Text.Encoding]::UTF8\n\
         $ws = New-Object -ComObject WScript.Shell\n\
         $made = @()\n",
    );
    let mut make = |known_folder: &str| {
        script += &format!(
            "$p = Join-Path ([Environment]::GetFolderPath('{known_folder}')) 'Snor.lnk'\n\
             $s = $ws.CreateShortcut($p)\n\
             $s.TargetPath = {}\n\
             $s.WorkingDirectory = {}\n\
             $s.IconLocation = {}\n\
             $s.Save()\n\
             $made += $p\n",
            ps_quote(&target),
            ps_quote(&working),
            ps_quote(&icon)
        );
    };
    if desktop {
        make("Desktop");
    }
    if start_menu {
        make("Programs");
    }
    script += "$made | ForEach-Object { Write-Output $_ }\n";

    let out = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run powershell: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "could not create the shortcuts: {}",
            first_line(&out.stderr)
        ));
    }
    Ok(decode_console(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// A PowerShell single-quoted string. Single quotes are legal in Windows paths
/// and PowerShell escapes them by doubling them; nothing else inside single
/// quotes needs escaping, so this is the whole of the quoting problem.
fn ps_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn reg_add(key: &str, name: &str, kind: &str, value: &str) -> Result<(), String> {
    let out = Command::new("reg.exe")
        .args(["add", key, "/v", name, "/t", kind, "/d", value, "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run reg.exe: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "could not write the uninstall entry ({name}): {}",
            first_line(&out.stderr)
        ))
    }
}

fn reg_delete(key: &str) -> Result<(), String> {
    let out = Command::new("reg.exe")
        .args(["delete", key, "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("could not run reg.exe: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(first_line(&out.stderr))
    }
}

fn first_line(bytes: &[u8]) -> String {
    decode_console(bytes)
        .lines()
        .next()
        .unwrap_or("failed without a message")
        .trim()
        .to_string()
}

/// Windows console tools write UTF-16LE when their output goes to a pipe; read
/// as UTF-8 that comes back with a NUL between every letter. Anything that is
/// not that shape is treated as UTF-8 — which is also correct, because the
/// shortcut script sets `[Console]::OutputEncoding` to UTF-8 first.
fn decode_console(bytes: &[u8]) -> String {
    let looks_utf16 = bytes.len() >= 2
        && bytes.len().is_multiple_of(2)
        && bytes.iter().skip(1).step_by(2).filter(|&&b| b == 0).count() >= bytes.len() / 4;
    if looks_utf16 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}
/// The record install leaves for uninstall to read: which shortcuts were
/// actually created, and against which version. Written in the app's own
/// `settings.rs` key=value dialect — hand-rolled, total, human-editable.
fn write_manifest(dir: &Path, version: &str, shortcuts: &[PathBuf]) -> Result<(), String> {
    let mut text = String::from("# Written by Snor Setup; read by uninstall.exe.\n");
    text.push_str(&format!("version={version}\n"));
    for shortcut in shortcuts {
        text.push_str(&format!("shortcut={}\n", shortcut.display()));
    }
    write_file(&dir.join(MANIFEST), text.as_bytes())
}

/// Total parser, in the same tradition: unknown keys are ignored and every
/// field is optional, because the worst case of a damaged manifest is an
/// uninstall that removes no shortcuts — never one that fails to run.
fn read_manifest(dir: &Path) -> Manifest {
    let mut manifest = Manifest::default();
    let Ok(text) = std::fs::read_to_string(dir.join(MANIFEST)) else {
        return manifest;
    };
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "shortcut" {
            manifest.shortcuts.push(PathBuf::from(value.trim()));
        }
    }
    manifest
}

#[derive(Default)]
struct Manifest {
    shortcuts: Vec<PathBuf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("snor-installer-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn the_default_directory_is_the_per_user_programs_root() {
        let dir = default_dir();
        assert!(
            dir.is_absolute(),
            "the default install folder must be absolute: {}",
            dir.display()
        );
        assert!(
            dir.ends_with(Path::new("Programs").join("Snor")),
            "unexpected dir: {}",
            dir.display()
        );
    }

    #[test]
    fn the_manifest_round_trips_and_ignores_what_it_does_not_know() {
        let dir = temp_dir("manifest");
        let shortcuts = vec![
            PathBuf::from(r"C:\Users\someone\Desktop\Snor.lnk"),
            PathBuf::from(r"C:\Users\someone\Start Menu\Programs\Snor.lnk"),
        ];
        write_manifest(&dir, "9.9.9", &shortcuts).expect("write");
        // A future writer adding a key must not break this reader.
        let text =
            std::fs::read_to_string(dir.join(MANIFEST)).expect("read") + "future_key=whatever\n";
        std::fs::write(dir.join(MANIFEST), text).expect("append");
        assert_eq!(read_manifest(&dir).shortcuts, shortcuts);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_manifest_is_an_empty_one() {
        let dir = temp_dir("missing-manifest");
        assert!(read_manifest(&dir).shortcuts.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A path with a quote in it must not be able to end the PowerShell string
    /// early — that would leave a broken script or, worse, a runnable one.
    #[test]
    fn ps_quoting_doubles_single_quotes() {
        assert_eq!(ps_quote(r"C:\plain\path"), r"'C:\plain\path'");
        assert_eq!(ps_quote(r"C:\it's here\a.exe"), r"'C:\it''s here\a.exe'");
    }

    #[test]
    fn the_cleanup_script_quotes_the_folder_waits_and_retries() {
        let script = cleanup_script(Path::new(r"C:\Users\someone\AppData\Local\Programs\Snor"));
        assert!(
            script.contains("ping -n 2"),
            "the delay is load-bearing: {script}"
        );
        assert!(
            script.contains(r#"rmdir /S /Q "C:\Users\someone\AppData\Local\Programs\Snor""#),
            "unexpected script: {script}"
        );
        // A lock that outlives the first attempt is what the first live run
        // showed: one `rmdir` is a race, so it has to be retried.
        assert!(script.contains("for /L %%i in"), "unexpected script: {script}");
        assert!(
            script.contains(r#"if not exist "C:\Users\someone\AppData\Local\Programs\Snor""#),
            "unexpected script: {script}"
        );
        // And the batch file removes the evidence.
        assert!(script.contains(r#"del "%~f0""#), "unexpected script: {script}");
    }

    /// A `%` is legal in a directory name and is batch syntax; unescaped it
    /// would expand as a variable and delete who-knows-what.
    #[test]
    fn a_percent_in_the_install_path_cannot_expand_as_a_variable() {
        let script = cleanup_script(Path::new(r"C:\100% real\Snor"));
        assert!(
            script.contains(r#""C:\100%% real\Snor""#),
            "unexpected script: {script}"
        );
    }

    #[test]
    fn utf16_console_output_does_not_come_back_null_interleaved() {
        let utf16: Vec<u8> = "ERROR: access denied"
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        assert_eq!(decode_console(&utf16), "ERROR: access denied");
        assert_eq!(decode_console(b"plain utf-8"), "plain utf-8");
    }
}