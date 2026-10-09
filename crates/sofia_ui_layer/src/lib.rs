#![recursion_limit = "512"]
//! GPUI Kit client for Sofia's always-visible voice pill.

pub mod ipc_client;
pub mod pill;
pub mod theme;

mod speech_flow;

pub mod window;
mod content_windows;
