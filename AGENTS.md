# AGENTS.md — Snor contributor guide

Snor is a lightweight native IDE in full Rust. Goal: beat Zed's ~980MB RAM
with a minimal editor (currently ~155MB WorkingSet64 on Windows 11).

## Stack

- `eframe 0.36 / egui (glow) + ropey + tree-sitter-highlight`
- `portable-pty (ConPTY) + vt100 + notify + rfd`
- Crate versions are pinned in `Cargo.lock`; eframe 0.36.2 is the latest
  0.36.x, so egui APIs below are for exactly that version.

## Commands

```powershell
cargo run              # debug build (keeps console window)
cargo run --release    # release build (no console, strip + thin LTO)
cargo test             # all unit + integration tests (includes a live pty test, ~20s max)
cargo clippy --all-targets -- -D warnings   # must be clean before commit
Get-Process Snor | Select-Object Name, @{N='MB';E={[math]::Round($_.WorkingSet64/1MB,1)}}
```

Windows locks a running exe: `Stop-Process -Name Snor` before rebuilding,
otherwise `cargo build` fails with `os error 5`.

## Architecture (`src/`)

| File | Owns |
|---|---|
| `main.rs` | `eframe::run_native`, window setup. |
| `app.rs` | Shell: title bar, left panel, status bar, center split (editor + terminal + quote rail), global shortcuts (Ctrl+Tab terminal, Ctrl+B explorer), Run-button wiring (`editor.want_run` -> `terminal.send_line("cargo run")`). Editor renders before terminal every frame — terminal reads input events after the editor. |
| `editor.rs` | Tabs (`OpenBuffer`), keyword-fallback highlighter, find (Ctrl+F), gutter, cursor readout (`cursor_line/col`), `want_run` flag. `ui(&mut self, ui, workdir)` — workdir seeds the `+` file picker. |
| `file_tree.rs` | Explorer tree (depth cap 8, skips `target/.git/node_modules/.idea`, 2000-entry cap), create/rename/delete + confirm modal, `notify` watcher with 300ms debounce, `opened_file` handoff to the editor. |
| `terminal.rs` | ConPTY `powershell.exe -NoLogo -NoProfile`, reader thread -> mpsc -> `vt100::Parser`, DSR/CPR/DA query responder, input forwarding, collapse/fullscreen/hide + drag height. |
| `syntax.rs` | Tree-sitter highlight to `LayoutJob` (rust/json/js/toml, 100KB cap), `None` on unsupported/over-limit so the caller falls back. |
| `theme.rs` | Calm dark-green palette, `file_badge()` (letter + colors per extension). |

## Terminal input — read this before touching it

Two verified egui 0.36 behaviors constrain the design:

1. **Dead-man's switch** (`egui::memory::Memory::end_pass`): a focus id that is
   requested but never attached to an *interacted* widget is dropped ~1 frame
   after the click. Gating input on a dummy id therefore loses every keystroke.
   Proven by test `uninteracted_focus_id_does_not_survive`.
2. **Focus-lock filter is unavailable**: `Memory::set_focus_lock_filter`
   exists but takes `EventFilter`, which is crate-private in egui 0.36
   (the `egui_tty` pattern only works on newer egui). Tab/arrows/Escape
   cannot be locked to a custom widget.

Current design (do not regress):

- No input widget, no focus id. Grid click latches `Terminal::active`.
- Forward Text/Paste/Key to the pty only while `active` **and**
  `memory.focused().is_none()` — so editor/find/explorer typing never leaks
  into the shell and vice versa.
- `active` clears the moment any real widget owns focus.
- Tab/Shift+Tab from unfocused state is grabbed by the first focusable
  widget (`Memory::interested_in_focus`); when that happens with no pointer
  click involved, surrender focus back so shell completion keeps working.
  Arrows from an unfocused state are harmless (no origin for nav);
  Escape just defocuses (already None).
- Ctrl+S / Ctrl+F are never consumed by the terminal (editor save/find win).
- `key_to_bytes` maps special keys; plain chars arrive via `Event::Text`.
  `Event::Text` is suppressed for Ctrl combos by egui-winit, so no doubling.
- `send_line()` is the entry point for scripted input (Run button).

## Editor gotchas

- **The fallback highlighter MUST stay char-boundary safe.** Byte-wise
  indexing (`bytes[i] as char`, `&text[i..i+1]`) panics on multi-byte UTF-8
  (e.g. the em-dash in README.md) and crashes the whole app on file open.
  Always advance by `char::len_utf8`, compare newlines via
  `text.as_bytes()[i] != b'\n'`, slice via `text.get(..)`.
  Guarded by test `highlight_unicode_does_not_panic`.
- Same for `syntax.rs`: slice tree-sitter ranges with `text.get(start..end)`.
- Gutter invariant: wrapping is OFF (`job.wrap.max_width = INFINITY`) so one
  buffer line == one visual row; gutter + code share one vertical scroll,
  code scrolls horizontally alone. Gutter renders only up to 6000 lines.
- Cursor readout reads `TextEditState::load(ctx, id)` ->
  `cursor.char_range().primary.index.0` (`CCursor.index` is `CharIndex`, a
  tuple struct). Compute line/col locally, then assign fields (borrowck:
  end the `buf` borrow first).

## egui 0.36 API notes (verified against the registry source)

- `Panel::top/bottom/left(...).show(ui, ...)` — `show_inside` is deprecated.
- `TextEdit::margin(impl Into<Margin>)`; `Margin::ZERO`; `Margin::symmetric(i8, i8)`.
- `Frame::NONE.fill(..).corner_radius(..).inner_margin(..)`; `CornerRadius: From<f32>`.
- `Ui::push_id(salt, closure) -> InnerResponse`; `Response.id` is `pub`.
- `Memory::focused() -> Option<Id>`; `Memory::surrender_focus(id)` only
  clears if that id holds focus; default `SurrenderFocusOn::Clicks`.
- `pointer.any_click()` for click-vs-keyboard disambiguation.
- `Response::on_hover_cursor(CursorIcon::ResizeVertical)` for splitters.

## Tests

- `cargo test` must stay green: editor roundtrip, find, unicode highlight,
  key mapping, query responder, file listing, both focus-mechanism tests,
  and `pty_powershell_echo_roundtrip` (Windows-only, spawns a real shell;
  bounded ~20s; proves spawn/write/poll/responder end to end).
- GUI behavior itself (clicks, drags, pixels) cannot be verified headlessly:
  after UI changes, rebuild, relaunch, and have a human confirm with a
  screenshot. Never claim a visual fix works without that.

## Conventions

- `cargo clippy --all-targets -- -D warnings` clean, always.
- No emojis in code, commits, or artifacts. Professional tone in files;
  traumatic honesty in code comments about *why* (cite the mechanism).
- Commit style: `fix:` / `feat:` prefixes, one logical change per commit.
- Deliberately no git integration in the app; no telemetry; stay lean.
