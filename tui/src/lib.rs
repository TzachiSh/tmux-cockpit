//! cockpit-tui, tmux-cockpit's optional board. The module list is frozen (#61):
//! see "Contracts" in docs/tui.md.
pub mod backend;
pub mod board;
pub mod chrome;
pub mod data;
pub mod fleet;
pub mod info;
pub mod panel;
pub mod pick;
pub mod push;
pub mod theme;

// branch popup (#246)
pub mod branch;
// sessions view (#245)
pub mod sessions;
// file browser (#363)
pub mod files;
