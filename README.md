# Snor — lightweight native IDE in full Rust

Goal: beat Zed's ~980MB RAM with a minimal native IDE.
V1: file tree, tabs + syntax highlight, built-in terminal (opencode), project search, git status + diff.

Stack: `eframe/egui + wgpu + tokio + ropey + tree-sitter + portable-pty (ConPTY) + alacritty_terminal + notify + ignore + grep-searcher + git2`.

## Run
```powershell
cargo run
cargo run --release
```

## Memory check
```powershell
Get-Process Snor | Select-Object Name, WorkingSet64
```

License: MIT OR Apache-2.0
