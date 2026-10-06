//! Private local transport between the companion and its native clients
//! (SPEC §8.1): one owner-only runtime directory, `SOCK_STREAM` Unix sockets,
//! length-prefixed versioned JSON frames, effective-UID and peer-PID checks,
//! and an atomically replaced owner-only locator. No TCP listener exists.

pub mod client;
pub mod frame;
pub mod locator;
pub mod paths;
pub mod peer;
pub mod runtime;
