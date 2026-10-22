//! coaster-queue: live ride wait times for UK theme parks. The program lives in this
//! library so the integration tests in `tests/` can reach it; `main.rs` is just the
//! entry point.

pub mod config;
pub mod groups;
pub mod led;
pub mod queue;
pub mod settings;
pub mod store;
pub mod web;
