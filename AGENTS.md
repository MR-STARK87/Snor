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

**Read the memory number as a peak, not a level.** `WorkingSet64` is resident
pages and Windows trims it freely for a window that is not in the foreground, so
it swings a long way without the app releasing anything. Measured on one
long-lived debug process: `WorkingSetSize` 83.8 MB, `PeakWorkingSetSize` 155.8 MB,
`PagefileUsage` (commit) 147.3 MB. The ~155 MB quoted above is the peak, and
commit is the number that actually stays put — a low resident reading after the
window has been backgrounded is not a win, and a high one is not a regression.
Query all three via `GetProcessMemoryInfo` before drawing any conclusion.

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
| `widgets.rs` | `clickable_label()` — the one correct way to make a text label behave like a button. See "Clickable labels" below; a bare `Label` + `on_hover_cursor` is wrong in two separate ways. |

## Clickable labels — read this before making anything clickable

Anything that is text but behaves like a button (a tab, a link) must go through
`widgets::clickable_label`. Rolling it by hand reintroduces two bugs that are
invisible until you watch the OS cursor:

1. **A `Label` is selectable text by default.** `Label::ui` resolves
   `selectable` from `style.interaction.selectable_labels` (true) and hands the
   label to egui's text-selection machinery, which writes `CursorIcon::Text`
   from `Label::ui` *and* again from `LabelSelectionState::on_end_pass` while
   it is dragging. The end-of-pass write lands after anything the call site
   asks for, so the tab hovered as a pointing hand and showed the I-beam for
   the whole time the button was held. `.selectable(false)` removes the write
   rather than racing it.
2. **`on_hover_cursor` stops applying during a long press.** A widget that
   senses clicks but not drags has its press abandoned once held past
   `InputOptions::max_click_duration` (0.8s), and `hovered()` goes false with
   it — so the hand decays to the default arrow mid-press. `clickable_label`
   keys off `contains_pointer` instead, which is purely geometric.

Both are covered by tests in `widgets.rs`. They need two frames before the
pointer arrives (egui hit-tests against the *previous* frame's widget rects)
and they read the widget's own rect for the hover point — a `Label`'s rect is
its galley, so a guessed coordinate lands in padding and the assertion
silently measures nothing.

**A cursor bug cannot be caught by a screenshot.** Verify with
`.workbuddy-ai/tools/hover_cursor.py`, which reads `GetCursorInfo` and
classifies the live OS cursor. Its `--hold` mode samples across a press, which
is what distinguishes "never set" from "set and then overwritten".

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
- **The accent dot in the header is the latch indicator, not decoration.**
  When `Terminal::active` is true the header draws a filled accent circle
  (`circle_filled`, r=3.5) in the slot where "click to type" otherwise sits;
  the two are an `if`/`else` on the same state. It is the only confirmation
  that keystrokes are going to the shell rather than to the editor, so do not
  delete it as clutter — that was a wrong call made once by reading a
  screenshot instead of the code. It is painted rather than typed because
  egui's bundled fonts have no dependable bullet coverage and a missing glyph
  renders as tofu. Measured: 9px wide, 15.2pt clear of the "+" button.

## Terminal tabs (multi-session)

`Terminal` is a container; every shell is a `Session` holding its own pty,
reader thread, `vt100::Parser` and query buffer. Rules that are load-bearing:

- **`poll()` drains every session, not just the visible one.** A background
  shell still answers prompts and still writes to its pty; leaving its channel
  unread grows it without bound. This is verified live: `ping -n 8` started in
  one tab, switched away from, then revisited, shows all eight replies.
- **The last tab *can* be closed.** That empties `sessions` and sets
  `hidden`, which takes the whole terminal section away — there is deliberately
  no "a shell must always exist" rule. `Terminal::reveal(cwd)` is the way back:
  it un-hides and spawns a fresh shell if the list is empty. Ctrl+Tab, the
  status-bar toggle and the Run button all go through it, because each of them
  wants a terminal and cannot assume one exists. Guarded by test
  `closing_the_last_tab_hides_the_panel_and_reveal_restores_it`.
- **`sessions` may be empty, so `session()`/`session_mut()` return `Option`.**
  Every access is guarded; `ui()` also returns early on an empty list so no
  call path can index it. Do not reintroduce a bare `self.sessions[i]`.
- **`close_tab` clamps `active_tab`** rather than recomputing it, so closing a
  tab *before* the active one keeps the same shell selected. Guarded by test
  `closing_an_earlier_tab_keeps_the_same_shell_selected`.
- **Tab labels take the lowest free number** (`next_free_title`), numbered from
  2 so a lone tab never reads "powershell 1". Deliberately *not* a monotonic
  counter: that produced "powershell 6", "powershell 7" after the panel had
  been emptied and reopened, a sequence depending on history the user cannot
  see. Emptying the panel restarts the sequence at "powershell". Guarded by
  test `tab_numbers_take_the_lowest_free_slot`.
- **The tab strip is a bounded `ScrollArea`** (`max_width` less
  `TERM_HDR_RIGHT_W`), because it shares its line with the right-hand controls:
  an unbounded scroll area claims the whole line and pushes them off it.
- **The "+" button lives *outside* the `ScrollArea`.** Inside it, the button is
  laid out after the last pill, so once there are more tabs than fit it scrolls
  off with them and there is no longer any way to open another shell. Verified
  at 10 tabs: the strip scrolls, the "+" stays at 1293.6pt, and the right-hand
  cluster holds 1517.6pt.
- **`reveal_active_tab` scrolls the active pill into view** — but only on the
  frame after the selection changed, then it is cleared. Doing it every frame
  would pin the strip and stop the user scrolling it by hand. Without it, a
  shell opened on a full strip appears clipped at the edge.
- **Mutations are deferred.** The strip's closure cannot hold `&mut self` while
  iterating `self.sessions`, so clicks record `switch_to` / `close_tab` /
  `open_tab` locals that are applied after the closure returns.
- `new_tab` clears `collapsed` — a new shell you cannot see is not a new shell.
- **The collapse toggle is a chevron in the right-hand cluster**, not a
  `plus`/`minus` beside the tab strip. There it sat next to the "+", and while
  collapsed it drew a "+" of its own, so the header showed two identical plus
  glyphs side by side. See `icons::chevron_v`.

## Flow Mode — the focus cue is threefold, on purpose

`render_pane` marks the focused pane three ways, and all three are load-bearing.
A single cue was tried and read as "no focus at all" at 1.25 DPI:

1. **Header band fill** — `surface_title()` across the full pane width.
2. **Accent bar** — 2pt wide, `PANE_HEADER_H - 6` tall, at `rect.left() + 3.0`.
3. **Pane outline** — warms from `pane_edge()` to
   `accent().gamma_multiply(0.55)`. The bar says *which* pane; the outline says
   the pane is live.

Measured live in a 3-pane layout at 1.25 DPI (focused pane vs unfocused):

| | focused | unfocused |
|---|---|---|
| header band | `(22,30,28)` `surface_title`, x 10..1590 | none — `(17,24,23)` `surface_recessed` |
| accent bar | x 14..15, y 508..519 (12px) | absent |
| outline | `(112,135,97)` = accent ×0.55 | `(51,61,56)` = `pane_edge` |

- **The header height is reserved whether or not the pane is focused.** The band
  is *painted*, not laid out, so nothing else holds the space open. Making the
  reservation conditional on focus would shift every prompt down the moment
  focus moved. Measured: first prompt ink lands `PANE_HEADER_H + GRID_TOP_GAP`
  below the pane top in both cases (~20pt), so focused and unfocused grids align.
- **The header must not name the pane.** It once read `powershell C:\...`, then a
  prompt-shaped `PS C:\...>`. Both were the shell's own first line written out a
  second time — the shell prints `PS C:\My work folder\Snor>` itself one row
  below, so every pane opened with two identical path entries stacked. The
  shell's line cannot be taken away, so the copy is the one that goes. Dimming it
  only makes the duplication quieter, which is not the same as fixing it.

## Grid padding — `GRID_TOP_GAP` and `GRID_SIDE_GAP`

Both views put air around the shell's grid, and both gaps are spent **before the
shell is sized**, so the PTY is never promised a cell the padded grid does not
draw. That is the same class of invariant as `PANE_TOP_CHROME`, and it fails the
same way: a shell with one column too many wraps its last character onto a row of
its own, which reads as the shell mis-measuring rather than as padding.

- `GRID_TOP_GAP` (5.0) — air above the first row, shared by normal mode and Flow
  panes so the prompt lands the same distance down in either.
- `GRID_SIDE_GAP` (8.0) — air either side of the grid, applied through
  `inset_grid_sides()`. Both `size_panes` and `render_pane` read that one
  function rather than each subtracting the gap by hand.

Measured live at 1.25 DPI, before and after adding the side gap:

| | before | after |
|---|---|---|
| normal mode, glyph from the rule above the tab strip | 9.6 pt | **17.6 pt** |
| Flow pane, glyph from the pane's own outline | **0.8 pt** | **8.8 pt** |

The Flow number is why this exists: the prompt's first glyph sat *one pixel* off
the pane outline. Normal mode was less bad only because the panel's own inner
margin happened to sit between the rule and the grid.

- **It is applied to both sides.** Padding only the left leaves a full-width line
  running into the right edge, which reads as a bug rather than a margin.
- **Verify it with an exact-width wrap test, not by eye.** Ask the shell for its
  size (`$Host.UI.RawUI.WindowSize`), then print a string of exactly that many
  characters and one of `n + 1`. The first must fill one row and the second must
  wrap by exactly one character. Measured: 131 columns, `"B"*131` filled one row,
  `"B"*132` wrapped one character. That is the only way to catch a grid and a PTY
  that disagree, and a screenshot cannot show it.
- `inset_grid_sides` clamps the gap to half the width, so a pane narrower than
  its own padding yields a sane rect instead of a negative width that would clamp
  every column count up to `MIN_TERM_COLS`.

## Auto context switching (`SnorApp::sync_context_root`)

The explorer re-roots itself at the focused terminal's directory, so the file
list always describes the project the shell you are typing into belongs to. Runs
in both modes: `Terminal::context_session(flow)` returns the focused pane's id in
Flow Mode, the active tab's otherwise.

- **It fires on a change of session id, not on the directories differing.** That
  distinction is the whole design. Comparing directories every frame fights the
  user: pick a folder from the header's dialog and the very next frame puts the
  focused shell's directory back, leaving the dialog unable to do anything while
  any terminal is open. Keying on the session id means a manual choice survives
  until focus actually moves — which is what "switch context when I move to
  another terminal" is supposed to mean.
- **Known limit: `session.cwd` is the directory a shell was *spawned* in**, not
  one it has `cd`-ed into since. Nothing here reads the shell's own output, so a
  pane that changes directory by hand keeps the directory it started with and the
  explorer will not follow it. Following a `cd` needs OSC 7 emitted by the shell
  and parsed in `terminal.rs`, which PowerShell does not do by default.
- "Open terminal here" from a folder's context menu is the only way to get two
  terminals that disagree about their directory, so it is also the only way to
  exercise this by hand.
- Verified live end-to-end: right-click `src` → *open terminal here* re-roots the
  explorer from `Snor` to `src` (header reads `src`, lists the 13 `.rs` files);
  clicking back to tab 1 re-roots it to `Snor`. Guarded by tests
  `context_session_follows_the_active_tab_in_normal_mode` and
  `hidden_terminal_reports_no_context`.

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
- **A tab close happens *mid-frame*, so the top-of-`ui()` empty guard cannot
  protect the rest of the frame.** `Editor::ui()` returns early through
  `empty_state()` when `tabs` is empty, but that check runs *before* the tab
  bar is drawn. The close is applied afterwards, so the list goes empty with
  most of the frame still to draw — and the code below the tab bar evaluates
  `self.tabs[self.active.min(self.tabs.len() - 1)]`. `usize` cannot go
  negative, so `len() - 1` on an empty list **overflows and panics** rather
  than producing a bad index: closing the only open file took the whole app
  down with `attempt to subtract with overflow` at `editor.rs`.
  The fix is two-part and both halves are load-bearing:
  1. `close_tab(idx) -> bool` owns the removal, the `active` clamp and
     `find_open` reset, and returns `false` exactly when the list just went
     empty.
  2. `ui()` early-returns when that `false` comes back, so nothing below the
     tab bar runs against the empty list. The next frame redraws through the
     normal empty-state path.
  Guarded by `closing_the_last_tab_empties_the_list_without_underflow`.
  `Terminal::close_tab` already followed this shape (`is_empty` → set
  `hidden` → early return) — that is why the terminal never had the bug.
- Anywhere else a `len() - 1` appears, confirm the `len() >= 1` invariant
  holds on the same path. The ones in `open_file` and `new_tab` sit directly
  after a `push`, and `icons.rs`'s `pts[len() - 1]` is built from `0..=20`,
  so those are statically safe. Do not add a new one without the same proof.

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

## Window shell (title bar, maximise, fullscreen)

`with_decorations(false)` means the OS chrome is off, so everything the OS
would normally do has to be ours. Three rules here are load-bearing and each
one was a bug first:

- **The title bar is drawn by us, so it must be hidden by us.** `SnorApp::ui`
  gates `self.title_bar(ui)` on the viewport's `fullscreen` flag. Leaving it
  unconditional means fullscreen changes nothing the user can see: the window
  grows, the taskbar goes away, and the bar on top looks identical — reported
  as "F11 doesn't work, it just hides the taskbar".
- **`window_resize_bands` must also return early when fullscreen**, not just
  when maximised. The window covers the monitor exactly, so there is nothing
  to drag it out to, and the bands would put resize arrows on the screen's own
  edges.
- **Never send `Fullscreen(true)` while the window is maximised.** A window
  still carrying `WS_MAXIMIZE` keeps the maximised *client* size whatever
  rectangle winit hands `SetWindowPos`: the outer rect reads 1920x1080 and
  `covers screen` is true, but egui is told the old work-area height, paints
  only that far, and the bottom band of the screen keeps stale pixels. The F11
  handler sends `Maximized(false)` first — both commands go to winit's window
  thread and run in order, and the un-maximise is a plain
  `ShowWindow(SW_RESTORE)` — then re-maximises on exit from
  `SnorApp::restore_maximized`, because winit saves the placement *after* our
  restore and would otherwise only bring back the floating rect.

Verifying this needs a **desktop** capture, not `snor_drive.py shot`: a
client-rect capture cannot show whether the taskbar is covered. Also note
`IsWindowVisible` on the taskbar stays true even when a fullscreen window
covers it, so it is not a usable test — compare the window rect against the
monitor rect (`tools/screen_info.py`).

**The 1px line outside our frame is not ours.** Windows 11 draws a
system-accent-coloured border around every top-level window, at the window's
outermost pixel. Sampled live: screen x=160 and x=1759 (and y=160) read
`(30,126,146)` — a teal that appears nowhere in `theme.rs` — while screen x=161
and x=1758 read `(55,62,58)`, which is our `window_edge()`. So the visible frame
is DWM's teal hairline, then our own contrast line, then content. Do not "fix"
the teal by drawing over it; if it clashes with the palette the fix is the user's
system accent colour, not the app. The client rect starts one pixel *below* the
window rect for the same reason.

**The floating window is placed by us, on the first frame.** The default inner
size is 1280x800 *logical*, which at 125% is 1600x1000 physical against a
1080p work area of 1920x1020 — only 20px of vertical slack. `main.rs` sets no
position, so Windows cascades the window wherever it likes; measured live it
landed at y=96, putting the bottom 76px below the work area: the window
covered the taskbar and its last 17px were off the screen entirely. A client
capture of that state ends in 17 rows of pure black, which is the tell.

A *decorated* window would have been clamped to the work area by the OS. This
one is deliberately undecorated, so nothing does it for us —
`SnorApp::fit_window_on_first_frame` runs once and sends `InnerSize` +
`OuterPosition`. Rules:

- **`monitor_size` is the whole monitor, not the work area.** egui has no
  work-area field, so `TASKBAR_RESERVE` (48pt = the Windows default taskbar at
  125%, and the same 48px at 100%) stands in for it. Centring on the full
  monitor height would still tuck the bottom edge under the taskbar.
- **Only the floating case.** Maximised or fullscreen is already placed by the
  OS; moving it would fight the user.
- **Do not latch the flag until the move is actually issued.** `monitor_size`
  and the rects can be `None` on the first frame; a `None` must retry, not
  give up permanently.
- The maths is pure (`fit_to_monitor`) and tested three ways:
  `floating_window_is_placed_fully_on_screen`,
  `a_window_taller_than_the_monitor_is_shrunk_to_fit`, and
  `a_misreported_monitor_cannot_produce_a_negative_size`.

Verified by launching cold eight times
(`tools/launch_check.py N`) — every run lands at (160,10)-(1760,1010),
1600x1000, fully inside the work area.

Note eframe is built without the `persistence` feature, so there is no saved
position to respect and re-centring each launch costs nothing.

**Verifying window placement needs the desktop, not a client capture.** A
client-rect capture cannot show whether the taskbar is covered. `IsWindowVisible`
on the taskbar stays true even when a window covers it, so compare the window
rect against the monitor rect (`tools/screen_info.py`) instead.

## Dim Mode (`dim.rs`, `brightness.rs`)

Lowers the physical backlight so agents can keep running with the screen dark.

- **The overlay only appears when Dim Mode genuinely took effect.** `is_active()`
  is `saved.is_some()`, and `saved` is written only after *both* the read and the
  write succeed (`DimManager::activate` returns early with a `notice` otherwise).
  So on hardware with no WMI brightness control the user gets a small status-bar
  notice and **no overlay at all**. That is deliberate — the overlay exists to
  tell "Dim Mode is on" apart from "dark theme on a dim monitor", and if nothing
  was dimmed there is nothing to disambiguate — but the consequence is worth
  knowing: on a desktop with only external monitors the feature looks like it
  does nothing.
- Verified live end to end on this machine: `CurrentBrightness` read **70**,
  toggling Dim Mode set it to **10**, toggling off restored **70** exactly. The
  status bar shows `Dim` plus an accent-filled moon while active. The overlay's
  pixels disappear completely on exit — 0 non-background pixels left in its
  region, so there is no stale residue.
- Overlay geometry (measured, 1536pt-wide window): 257.6 x 50.4 pt, centred
  horizontally (centre x 767.6 against a window centre of 768.0), bottom edge
  34.4 pt above the window bottom — i.e. it sits just above the status bar.
- Backend is WMI through `powershell.exe`: `WmiMonitorBrightness` to read,
  `WmiMonitorBrightnessMethods.WmiSetBrightness` to write, with `Timeout = 0` so
  Windows cannot silently revert the level and desync the saved restore value.
  `restore_on_exit` is best-effort on shutdown and never panics.
- **A screenshot cannot verify this.** Captures grab the framebuffer, not the
  panel, so a dimmed screen looks identical in a PNG. Confirm the level by
  re-reading `CurrentBrightness` after the toggle, never by looking at a capture.

## Tests

- `cargo test` must stay green (66 tests): editor roundtrip, find, unicode
  highlight, key mapping, query responder, file listing, both focus-mechanism
  tests, the four terminal-tab tests (`tabs_spawn_and_switch`,
  `closing_the_last_tab_hides_the_panel_and_reveal_restores_it`,
  `closing_an_earlier_tab_keeps_the_same_shell_selected`,
  `tab_numbers_take_the_lowest_free_slot`), the grid-padding pair
  (`flow_pane_sizing_reserves_its_header` and its side-gap twin
  `flow_pane_sizing_reserves_its_side_gaps`),
  `closing_the_last_tab_empties_the_list_without_underflow`, the three
  window-placement tests (`floating_window_is_placed_fully_on_screen`,
  `a_window_taller_than_the_monitor_is_shrunk_to_fit`,
  `a_misreported_monitor_cannot_produce_a_negative_size`), and
  `pty_powershell_echo_roundtrip` (Windows-only, spawns a real shell;
  bounded ~20s; proves spawn/write/poll/responder end to end).
- Tab bookkeeping is tested through `Terminal::open_stub_tab` (a `#[cfg(test)]`
  twin of `new_tab` minus the pty) so the tests stay hermetic. It shares
  `next_free_title` with the real path, so a numbering change cannot pass the
  tests while breaking the app.
- GUI behavior itself (clicks, drags, pixels) cannot be verified headlessly:
  after UI changes, rebuild, relaunch, and have a human confirm with a
  screenshot. Never claim a visual fix works without that.
- GUI tooling lives in `.workbuddy-ai/tools/` in the local agent workspace. It
  is gitignored and deliberately not part of the tree, so these paths will not
  resolve in a fresh clone — the three worth knowing are:
  - `snor_ui_probe.py` drives the real window through user32: `info`, `shot`,
    `click`, `move`, `drag`, `place`, `maximize`, `type`, `key`. Coordinates
    are egui points relative to the client area. `key` accepts chords
    (`ctrl+tab`, `ctrl+b`) and holds the modifiers down around the key — egui
    reads modifier state from the event itself, so sending them as separate
    presses arrives as a bare key.
  - `tab_bbox.py` reports the terminal tab strip's ink boxes in points, which
    is how click targets are aimed instead of guessed — a click that misses by
    a few points reads as "the feature is broken". Two traps it handles: the
    explorer tree shares the header's rows to the left, and the separator rule
    under the header out-inks the text row, so a naive densest-row search locks
    onto the rule.
  - `many_tabs.py` adds tabs one at a time and reports where the "+" and the
    right-hand cluster actually are, which is how the overflow behaviour above
    was measured.
  - `snor_drive.py` is the current driver (`focus`, `shot`, `click`, `rclick`,
    `drag`, `key`, `type`, `info`). It taps Alt before raising, because
    `SetForegroundWindow` fails while another app owns the foreground and the
    capture then grabs whatever is on top.
  - **`PrintWindow` is the only capture that works on an occluded window.**
    Both `snor_drive.py shot` and `ImageGrab` grab the *screen*, so they return
    whatever is on top — three attempts to photograph Snor returned the user's
    browser instead, and neither `SetForegroundWindow` nor
    `SetWindowPos(HWND_TOPMOST)` won the foreground back. On a machine someone
    else is using, render the window into a memory DC instead:
    `GetWindowDC` -> `CreateCompatibleDC` -> `CreateCompatibleBitmap` ->
    `SelectObject` -> `PrintWindow(hwnd, mdc, 2)` (PW_RENDERFULLCONTENT) ->
    `GetDIBits` with a negative `biHeight` for a top-down 32-bit BGRA buffer ->
    `Image.frombuffer(..., "BGRA", ...)`. Verified on this OpenGL window:
    returns 1 with real content. Reach for it first; `desktop_shot.py` is only
    for the one question it alone answers, whether a window covers the taskbar.
  - `desktop_shot.py` grabs the whole desktop, for anything about the window's
    own frame or the taskbar.
  - `launch_check.py N` launches the app N times and reports the window rect
    each time — the way the placement fix above was verified. **It opens and
    kills the app N times a few seconds apart, which reads exactly like "Snor
    keeps launching and closing on its own" to anyone watching.** Run it only
    when nobody else is at the machine, and say what it was afterwards.
  - `screen_info.py` prints screen, work area, window rect, window style bits
    and taskbar in one go.
  - `ink_groups.py` finds glyph clusters in a band and prints their centres in
    egui points. Aim clicks with this rather than estimating from a downscaled
    screenshot — a click that lands in padding reads as "the feature is
    broken", and it cost two wasted clicks to learn.
  - `hover_cursor.py` reads `GetCursorInfo` and classifies the live OS cursor.
    A cursor never appears in a screenshot, so this is the only way to check
    one. `--hold` presses the button and samples across the press. It moves
    the pointer with `mouse_event(MOUSEEVENTF_MOVE|ABSOLUTE)` and approaches
    from a nearby point, so the window gets a real move message and the
    position always changes. A bare `SetCursorPos` does neither reliably and
    will report the *previous* point's cursor — an earlier probe built on it
    produced a confident but wrong "IBEAM on the tab" reading.
- **`powershell.exe` must not be launched from the Bash tool** — it is blocked
  there by design, and the block cannot be worked around from inside a Python
  subprocess either. Use the PowerShell tool. In some sessions that tool returns
  **empty stdout** even for commands that ran fine; when that happens, have the
  command write its result to a file and read the file back. That is how the
  Dim Mode brightness readings above were taken, and it costs one extra call
  instead of a lost investigation.
- **Relaunching the app: do not background it with a shell `&`.** A subshell
  launch (`(./target/debug/snor.exe &)`) reports the process alive for a few
  seconds and then it vanishes, because the process dies with the shell that
  spawned it — which looks exactly like a startup crash and is not one. Launch it
  as a managed background task instead, and confirm with `tasklist` *and* a
  window query before concluding anything. Rebuilding also needs
  `Stop-Process -Name Snor` first, or the link fails with `os error 5` on the
  locked exe.
  - `prompt_gap.py` / `scan_row.py` / `scan_col.py` profile colour runs along
    a row or column, for measuring clearances in pixels.
  - All of these need Pillow, which the managed Python 3.13 does not have. Use
    the system Python 3.12.

## Conventions

- `cargo clippy --all-targets -- -D warnings` clean, always.
- No emojis in code, commits, or artifacts. Professional tone in files;
  traumatic honesty in code comments about *why* (cite the mechanism).
- Commit style: `fix:` / `feat:` prefixes, one logical change per commit.
- Deliberately no git integration in the app; no telemetry; stay lean.
