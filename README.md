# Snor — lightweight native IDE in full Rust

Goal: beat Zed's ~980MB RAM with a minimal native IDE.
V1 done: file tree, editor tabs + highlight, ConPTY terminal, project search, git status + diff.

Stack: `eframe 0.36/egui + wgpu + ropey + portable-pty (ConPTY) + vt100 + notify + ignore + git2`.

## Run
```powershell
cargo run
cargo run --release
```

## Use
- Explorer (left): browse, `+ file`, `+ dir`, rename, delete, auto-refresh via watcher. Double-click opens in editor.
- Editor (center): tabs, keyword highlight (rs/toml/json/js/ts/py), Ctrl+S to save, large-file guard 500KB.
- Terminal (bottom): real ConPTY `powershell.exe -NoLogo`, type commands, quick buttons for `dir` and `opencode --help`. Run `opencode` inside to drive your agent.
- Search + Git (right): text search across repo (ignore rules, 2000-hit cap), git branch + status + diff, click to open.

## Memory vs Zed 980MB (Windows 11, WorkingSet64)
- debug shell: 368MB
- debug full V1: ~306-313MB
- release V1: ~331MB, 17MB exe
- Result: ~3x lighter than Zed (66% less). Under the 250MB stretch goal? Not yet — wgpu + fonts + pty keep us ~330MB. Next wins: `glow` renderer, trim eframe features, lazy terminal spawn.

## Measure
```powershell
Get-Process Snor | Select-Object Name, @{N='MB';E={[math]::Round($_.WorkingSet64/1MB,1)}}
```

License: MIT OR Apache-2.0
