# Snor Setup

The install and uninstall wizard for Snor, in the app's own design language.

`snor-setup.exe` is the wizard; `snor-pack.exe` appends the app to it so the
shipped artifact is one file:

```powershell
cargo build --release                        # the app
cargo build --release -p snor-installer      # the wizard and the packer
target/release/snor-pack.exe target/release/snor-setup.exe target/release/snor.exe dist/SnorSetup.exe
```

## What an install does

- Copies `snor.exe` and the two licence files into
  `%LOCALAPPDATA%\Programs\Snor` (per-user: no elevation, no UAC prompt). A
  different folder can be chosen on the options screen.
- Creates a Start Menu shortcut, and a desktop one if asked. Shortcuts go
  through PowerShell's known-folder API, so a redirected Desktop still
  resolves.
- Writes one key under
  `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Snor` so Windows
  Settings > Apps can uninstall it.
- Copies its own prefix — everything before the appended payload — into the
  install folder as `uninstall.exe`, and records the shortcut paths in
  `uninstall.info` beside it. Uninstall re-runs this same wizard with
  `--uninstall`, removes exactly what the manifest records, and hands the
  folder's own removal to a detached `cmd` after the window closes.

## The design language

`src/lib.rs` includes the app's `theme.rs`, `fonts.rs` and `icons.rs` straight
out of `../src/` via `#[path]`. The palette, the faces and the bar glyphs are
one set of files compiled into two binaries — not copies — so a colour or a
line width cannot drift between the editor and its installer.

## The bundle format

```text
[ snor-setup.exe ][ snor.exe ][ magic 8 | version 4 | len 8 | fnv 8 ]
```

The footer sits at the end of the file and is read back from there, so nothing
has to parse PE headers; the app's bytes are FNV-1a checked before they are
written anywhere. Packing is idempotent — an existing bundle is stripped
before the new payload is appended. `snor-pack --verify dist\SnorSetup.exe`
checks a bundle without launching it.

For development, a plain `target\debug\snor-setup.exe` also runs with an
`snor.exe` beside it (sidecar mode) — no repack needed. The wizard prefers its
own appended payload and only falls back to the sidecar when there is one; a
directory containing `uninstall.info` is read as an uninstall, never as a
sidecar install.
