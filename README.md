# Snor — lightweight native IDE in full Rust

Goal: beat Zed's ~980MB RAM with a minimal native IDE.
V1 done: file tree, editor tabs + highlight, ConPTY terminal, project search, git status + diff.
V1.1 done: tree-sitter highlight, terminal colors + cursor, glow diet under 250MB.

Stack: `eframe 0.36/egui (glow) + ropey + tree-sitter-highlight + portable-pty (ConPTY) + vt100 + notify + ignore + git2`.

## Run
```powershell
cargo run
cargo run --release
```

## Use
- Explorer (left): browse, `+ file`, `+ dir`, rename, delete, auto-refresh via watcher. Double-click opens in editor.
- Editor (center): tabs, tree-sitter highlight (rust/json/js/toml, keyword fallback for the rest, 100KB TS cap), Ctrl+S to save, large-file guard 500KB.
- Terminal (bottom): real ConPTY `powershell.exe -NoLogo -NoProfile` with colors + block cursor. Answers DSR/CPR/DA queries so PSReadLine unblocks (this was the silent-terminal bug). Type commands, quick buttons for `dir` and `opencode --help`. Run `opencode` inside to drive your agent. Collapse with `-` when you need editor space.
- Search + Git (right): text search across repo (ignore rules, 2000-hit cap), git branch + status + diff, click to open.

## Memory vs Zed 980MB (Windows 11, WorkingSet64)
- V1 debug full: ~306-313MB, V1 release: ~331MB, 17MB exe
- V1.1 debug: ~173MB, V1.1 release: ~167MB, 12.5MB exe (glow + strip + thin LTO)
- Result: ~6x lighter than Zed. Sub-250MB goal smashed.

## Notes
- Release builds hide the console window (`windows_subsystem`); debug keeps it.
- Bottom zone (terminal + status) is laid out manually inside the central
  panel — `Panel::bottom` never painted in this setup, so it was dropped.
- `cargo test` covers file tree listing, editor open/edit/save, project search.

## Measure
```powershell
Get-Process Snor | Select-Object Name, @{N='MB';E={[math]::Round($_.WorkingSet64/1MB,1)}}
```

License: MIT OR Apache-2.0
