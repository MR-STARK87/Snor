//! Snor Setup: installing and uninstalling, wearing the app's own design
//! language.
//!
//! The wizard exists so that installing Snor looks like Snor: the same calm
//! dark-green palette, the same Space Grotesk faces and the same
//! decorationless window drawn by hand. That is not a copy — `theme`, `fonts`
//! and `icons` below are the app's *source files*, compiled into this crate
//! through `#[path]` includes. A change to the palette or to the chrome lands
//! in both windows or in neither, which is the only way two binaries stay
//! visually identical over time.
//!
//! The side effects of an install are deliberately small: files under
//! `%LOCALAPPDATA%\Programs\Snor` (per-user, so no elevation is ever asked
//! for), two optional `.lnk` shortcuts, one key under the user's own Uninstall
//! hive, and a copy of this binary as `uninstall.exe` — built from the setup
//! binary's *prefix*, everything before the appended payload, so the
//! uninstaller does not carry the app around with it. `install.rs` has the
//! details; `payload.rs` has the one-file format.

pub mod app;
pub mod install;
pub mod payload;
pub mod ui;

// The app's own source, not a copy of it. `theme.rs` refers to this crate's
// `fonts` in `medium()`, which is exactly why both are included together:
// they are a pair, and neither compiles without the other.
//
// `#[allow(dead_code)]`: each file is the app's *whole* module, so the
// installer carries a few entries it never draws (the file badges, the git
// branch glyph, the bold face). Trimming the shared files down to the
// installer's subset is precisely the drift this include exists to prevent —
// the app is where the leftovers earn their keep.
#[allow(dead_code)]
#[path = "../../src/fonts.rs"]
mod fonts;
#[allow(dead_code)]
#[path = "../../src/icons.rs"]
mod icons;
#[allow(dead_code)]
#[path = "../../src/theme.rs"]
mod theme;
