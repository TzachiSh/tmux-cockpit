//! cockpit-tui: tmux-cockpit's optional Rust view. It reads tmux and the
//! transcripts, decides, and prints one action line; a shell wrapper performs
//! it. The binary is additive -- a missing one costs one `display-message` and
//! `prefix r` still opens the fleet.
//!
//! # The frozen module list (issue #61, workflow.md §3)
//!
//! No lane adds, removes or renames a module without a comment on #61.
//!
//! | module  | owns                                                             |
//! |---------|------------------------------------------------------------------|
//! | `backend`| the VS16 wide-glyph shim over the Crossterm backend, and `init()` |
//! | `data`  | every tmux and transcript read, `Pane`, the truncation helpers    |
//! | `board` | the tile grid (phase 1) and its event loop                        |
//! | `fleet` | the headless `--rows` / `--json` formatters and the list widths   |
//! | `theme` | the `@thm_*` colour roles and the glyph tiers                     |
//! | `push`  | `@cockpit_tui_pid`, SIGUSR1 wakeups, the exit-path cleanup        |
//!
//! `lib.rs` re-exports and nothing else; no workspace, no crate split.
pub mod backend;
pub mod board;
pub mod data;
pub mod fleet;
pub mod push;
pub mod theme;
