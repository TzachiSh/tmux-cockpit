//! cockpit-tui, tmux-cockpit's optional board. The module list is frozen (#61):
//! see "Contracts" in docs/tui.md.
// The lib serves only the cockpit-tui binary and its docs are built with
// --document-private-items (make tui-test), so a pub item's doc may link the
// private helper it leans on (#739).
#![allow(rustdoc::private_intra_doc_links)]
pub mod backend;
pub mod basket;
pub mod board;
pub mod chrome;
pub mod data;
pub mod fleet;
pub mod highlight;
pub mod info;
pub mod mdview;
pub mod panel;
pub mod pick;
pub mod push;
pub mod refs;
pub mod theme;

// branch popup (#246)
pub mod branch;
// sessions view (#245)
pub mod sessions;
// file browser (#363)
pub mod files;
