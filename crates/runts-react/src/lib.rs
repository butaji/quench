//! runts-react — React SSR plugin for runts
//!
//! Provides React Server-Side Rendering with streaming support.
//! Target features:
//! - React class components
//! - React.lazy + Suspense
//! - renderToPipeableStream
//! - HTTP server with streaming

pub mod plugin;

pub use plugin::ReactPlugin;
