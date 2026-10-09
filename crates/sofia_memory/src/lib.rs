//! Sofia persistent memory layer (Hindsight-inspired architecture).
//! Provides entity graph, temporal experiences with emotional/affective state,
//! tool/document interaction history, user preferences, and hybrid FTS5 search.

pub mod error;
pub mod mcp;
pub mod models;
pub mod store;

pub use error::MemoryError;
pub use models::*;
pub use store::MemoryStore;
