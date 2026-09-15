# Snor — lightweight native IDE in full Rust

Goal: beat Zed's ~980MB RAM with a minimal native IDE.
Done: file tree, editor tabs + highlight, ConPTY terminal, find in file.
Deliberately no git — use your own git client, Snor stays lean.

Stack: `eframe 0.36/egui (glow) + ropey + tree-sitter-highlight + portable-pty (ConPTY) + vt100 + notify + rfd`.

## Run
```powershell
cargo run
cargo run --release
```

## Use
- Top bar: current folder + `open folder` to switch projects.
- Explorer (left): browse, right-click a folder/file for new/rename/delete, `↻` to refresh, auto-refresh via watcher. Double-click opens in editor.
- Editor (center): tabs, tree-sitter highlight (rust/json/js/toml, keyword fallback for the rest, 100KB TS cap), Ctrl+S to save, Ctrl+F to find in the current file (Enter next, Shift+Enter prev, Esc close), large-file guard 500KB.
- Terminal: real ConPTY `powershell.exe -NoLogo -NoProfile` with colors + block cursor. Click it and type directly — Enter, arrows, Tab, Backspace, Ctrl+C all go to the shell. Answers DSR/CPR/DA queries so PSReadLine unblocks. `max` fills the center column, `-` collapses. Run `opencode` inside to drive your agent.

## Memory vs Zed 980MB (Windows 11, WorkingSet64)
- V1 debug full: ~306-313MB, V1 release: ~331MB, 17MB exe
- V1.1 debug: ~173MB, V1.1 release: ~167MB, 12.5MB exe (glow + strip + thin LTO)
- Result: ~6x lighter than Zed. Sub-250MB goal smashed.

## Notes
- Release builds hide the console window (`windows_subsystem`); debug keeps it.
- Terminal + editor share the central panel (manual split) — `Panel::bottom`
  never painted in this setup, so it was dropped.
- `cargo test` covers file tree listing, editor open/edit/save, in-file find,
  terminal key mapping.

## Measure
```powershell
Get-Process Snor | Select-Object Name, @{N='MB';E={[math]::Round($_.WorkingSet64/1MB,1)}}
```

License: MIT OR Apache-2.0
