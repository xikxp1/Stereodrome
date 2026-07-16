//! Submarine client module with dedicated thread for lock-free operations.
//!
//! This module provides a thread-safe interface to the Subsonic API client
//! using message passing instead of mutexes, eliminating lock contention.

mod handle;
mod messages;
mod thread;

pub use handle::SubsonicClientHandle;
pub use messages::{AlbumListEntry, AlbumListOrder, ClientError, ServerConfig};

/// Spawn the client thread and return its communication and lifecycle handles.
pub fn spawn() -> (SubsonicClientHandle, std::thread::JoinHandle<()>) {
    thread::spawn()
}
