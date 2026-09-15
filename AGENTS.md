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
| `fonts.rs` | Vendors Space Grotesk (three static weights under `assets/fonts/`) and registers it as the *proportional* family, with egui's built-ins kept as fallbacks. Monospace is deliberately untouched — the gutter and the terminal grid depend on a fixed advance width. `MEDIUM`/`BOLD` are exposed as named families because `RichText::strong()` only swaps colour in egui, never the face. |
| `app.rs` | Shell: title bar, left panel, status bar, center split (editor + terminal), global shortcuts (Ctrl+Tab terminal, Ctrl+B explorer), Run-button wiring (`editor.want_run` -> `terminal.send_line("cargo run")`). Owns `show_explorer` (the status-bar sidebar toggle). Editor renders before terminal every frame — terminal reads input events after the editor. |
| `editor.rs` | Tabs (`OpenBuffer`), keyword-fallback highlighter, find (Ctrl+F), gutter, cursor readout (`cursor_line/col`), `want_run` flag. `ui(&mut self, ui, workdir)` — workdir seeds the `+` file picker. |
| `file_tree.rs` | Explorer tree (depth cap 8, skips `target/.git/node_modules/.idea`, 2000-entry cap), create/rename/delete + confirm modal, `notify` watcher with 300ms debounce, `opened_file` handoff to the editor. |
| `terminal.rs` | Multi-session ConPTY. `Terminal` owns `Vec<Session>` + `active_tab`; each `Session` owns its own pty, reader thread, `vt100::Parser` scrollback and query-responder buffer. Tab strip spawns/switches/closes shells; `collapsed`/`fullscreen`/`hidden` + drag height. |
| `syntax.rs` | Tree-sitter highlight to `LayoutJob` (rust/json/js/toml, 100KB cap), `None` on unsupported/over-limit so the caller falls back. |
| `theme.rs` | Calm dark-green palette, three surface tones (`surface_title` / `surface_body` / `surface_recessed`), `file_badge()` (letter + colors per extension). |

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

## Terminal tabs (multi-session)

`Terminal` is a container; every shell is a `Session` holding its own pty,
reader thread, `vt100::Parser` and query buffer. Rules that are load-bearing:

- **`poll()` drains every session, not just the visible one.** A background
  shell still answers prompts and still writes to its pty; leaving its channel
  unread grows it without bound. This is verified live: `ping -n 8` started in
  one tab, switched away from, then revisited, shows all eight replies.
- **The last tab cannot be closed** (`close_tab` refuses at `len() <= 1`). An
  empty terminal panel would need its own empty state, and the whole input
  design assumes there is a shell to talk to. Restarting a wedged shell is what
  the refresh button is for.
- **`close_tab` clamps `active_tab`** rather than recomputing it, so closing a
  tab *before* the active one keeps the same shell selected. Guarded by test
  `closing_an_earlier_tab_keeps_the_same_shell_selected`.
- **Tab labels count shells ever spawned, not tabs open** (`shell_title(n)`,
  numbering from 2 so a lone tab never reads "powershell 1"). Reusing a number
  would put two identical labels on screen at once. Guarded by test
  `tab_numbers_are_not_reused`.
- **The tab strip is a bounded `ScrollArea`** (`max_width` less
  `TERM_HDR_RIGHT_W`), because it shares its line with the right-hand controls:
  an unbounded scroll area claims the whole line and pushes them off it.
- **Mutations are deferred.** The strip's closure cannot hold `&mut self` while
  iterating `self.sessions`, so clicks record `switch_to` / `close_tab` /
  `open_tab` locals that are applied after the closure returns.
- `new_tab` clears `collapsed` — a new shell you cannot see is not a new shell.

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
- **`Ui::horizontal` inherits the parent's direction.**
  `horizontal_with_main_wrap_dyn` reads `self.placer.prefer_right_to_left()`,
  so a `horizontal` nested inside a `right_to_left` row is *also* right-to-left
  and draws its children in reverse. To force left-to-right inside a
  right-to-left row, use `ui.with_layout(Layout::left_to_right(Align::Center), ..)`.
  This silently reordered the explorer header and squeezed its folder glyph to
  nothing at narrow widths.
- `Layout::advance_cursor` does `RightToLeft => region.cursor.max.x -= amount`,
  so `ui.add_space(n)` moves the cursor *left* in a right-to-left layout.
- `Label::truncate()` sets `TextWrapMode::Truncate`; without it an over-long
  label overflows its slot instead of shrinking to fit.
- `RichText::strong()` only swaps the colour for `strong_text_color()` — it
  does not change the face. Real weight needs a named `FontFamily`.
- `Frame::side_top_panel(style)` is `inner_margin(Margin::symmetric(8, 2))`
  plus `panel_fill`, so an absolute inset is measured from the panel's
  *content* edge, not the window edge. `Panel::frame(..)` replaces it entirely.

## Tests

- `cargo test` must stay green (21 tests): editor roundtrip, find, unicode
  highlight, key mapping, query responder, file listing, both focus-mechanism
  tests, the four terminal-tab tests (`tabs_spawn_switch_and_refuse_to_empty_the_panel`,
  `closing_an_earlier_tab_keeps_the_same_shell_selected`, `tab_numbers_are_not_reused`),
  and `pty_powershell_echo_roundtrip` (Windows-only, spawns a real shell;
  bounded ~20s; proves spawn/write/poll/responder end to end).
- Tab bookkeeping is tested through `Terminal::open_stub_tab` (a `#[cfg(test)]`
  twin of `new_tab` minus the pty) so the tests stay hermetic. It shares
  `shell_title` with the real path, so a numbering change cannot pass the
  tests while breaking the app.
- GUI behavior itself (clicks, drags, pixels) cannot be verified headlessly:
  after UI changes, rebuild, relaunch, and have a human confirm with a
  screenshot. Never claim a visual fix works without that.
- `.workbuddy-ai/tools/snor_ui_probe.py` drives the real window through
  user32: `info`, `shot`, `click`, `move`, `drag`, `place`, `maximize`, `type`,
  `key`. Coordinates are egui points relative to the client area.
  `.workbuddy-ai/tools/tab_bbox.py` reports the tab strip's ink bounding boxes
  in points, which is how click targets are aimed instead of guessed — a click
  that misses by a few points reads as "the feature is broken".

## Conventions

- `cargo clippy --all-targets -- -D warnings` clean, always.
- No emojis in code, commits, or artifacts. Professional tone in files;
  traumatic honesty in code comments about *why* (cite the mechanism).
- Commit style: `fix:` / `feat:` prefixes, one logical change per commit.
- Deliberately no git integration in the app; no telemetry; stay lean.
