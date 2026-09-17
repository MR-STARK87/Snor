# Snor — lightweight native IDE in full Rust

![ci](https://github.com/MR-STARK87/Snor/actions/workflows/ci.yml/badge.svg)

Goal: beat Zed's ~980MB RAM with a minimal native IDE.
Done: file tree, editor tabs + highlight, multi-session ConPTY terminal, find in file.
Deliberately no git — use your own git client, Snor stays lean.

> **Windows-only for now.** The terminal is ConPTY with `powershell.exe`, and
> extras like Dim Mode talk to Windows (WMI) directly. Portability is later;
> honesty first.

Stack: `eframe 0.36/egui (glow) + ropey + tree-sitter-highlight + portable-pty (ConPTY) + vt100 + notify + rfd`.

## Run
```powershell
cargo run
cargo run --release
```

## Use
- Top bar: current folder + `open folder` to switch projects. `≡` (or Ctrl+B) hides/shows the Explorer, which is also resizable by dragging its edge.
- Explorer (left): browse, right-click a folder/file for new/rename/delete, `open terminal here` to drop a shell into that folder, `↻` to refresh, auto-refresh via watcher. Double-click opens in editor. The tree re-roots itself at the focused terminal's directory, so it always describes the project of the shell you are typing into.
- Editor (center): badge tabs, line-number gutter, tree-sitter highlight (rust/json/js/toml, keyword fallback for the rest, 100KB TS cap), Ctrl+S to save, Ctrl+F to find in the current file (Enter next, Shift+Enter prev, Esc close), Run button sends `cargo run` to the terminal, large-file guard 500KB.
- Terminal: real ConPTY `powershell.exe -NoLogo -NoProfile` with colors + block cursor. Multiple sessions behind a tab strip (`+` opens, click switches, `x` closes — closing the last one hides the panel, Ctrl+Tab brings it back). Click it and type directly — Enter, arrows, Tab, Backspace, Ctrl+C all go to the shell. Answers DSR/CPR/DA queries so PSReadLine unblocks. Drag the divider to resize, chevron collapses, `max` fills the center column, Ctrl+Tab hides/shows it entirely. Run `opencode` inside to drive your agent.
- Dim Mode: lowers the physical backlight so long agent sessions can run with the screen dark (status-bar moon while active; machines without brightness control just get a notice).

## Memory vs Zed 980MB (Windows 11)

Read the number as a peak, not a level: `WorkingSet64` is resident pages and
Windows trims it freely for a backgrounded window, so it swings without the
app releasing anything. Measured on one long-lived debug process:
`WorkingSetSize` 83.8 MB, `PeakWorkingSetSize` 155.8 MB, commit
(`PagefileUsage`) 147.3 MB. The ~155 MB quoted is the peak; commit is the
number that stays put. Release exe is ~13MB (strip + thin LTO).

- V1 debug full: ~306-313MB, V1 release: ~331MB, 17MB exe
- V1.1 debug: ~173MB, V1.1 release: ~167MB, 12.5MB exe (glow + strip + thin LTO)
- Now: ~155MB peak, ~147MB commit — still ~6x lighter than Zed.

## Notes
- Release builds hide the console window (`windows_subsystem`); debug keeps it.
- Terminal + editor share the central panel (manual split) — `Panel::bottom`
  never painted in this setup, so it was dropped.
- `cargo test` (63 tests) covers file tree listing, editor open/edit/save,
  in-file find, terminal key mapping, tab bookkeeping, Flow pane sizing,
  window placement math, clickable-label cursor behavior, and a live
  PowerShell echo roundtrip. `cargo clippy --all-targets -- -D warnings`
  must be clean before every commit; CI enforces both on `main` and `dev`.

## Measure
```powershell
Get-Process Snor | Select-Object Name, @{N='MB';E={[math]::Round($_.WorkingSet64/1MB,1)}}
```

## Contributing
- One logical change per commit, `fix:` / `feat:` prefixes.
- Keep it lean: no git integration, no telemetry, no emojis in code or commits.
- GUI behavior can't be verified headlessly — rebuild, relaunch, confirm by hand.

License: MIT OR Apache-2.0 (see `LICENSE-MIT` and `LICENSE-APACHE`).
