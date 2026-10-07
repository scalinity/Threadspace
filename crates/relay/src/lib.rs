//! Private local transport between the companion and its native clients
//! (SPEC §8.1): one owner-only runtime directory, `SOCK_STREAM` Unix sockets,
//! length-prefixed versioned JSON frames, effective-UID and peer-PID checks,
//! and an atomically replaced owner-only locator. No TCP listener exists.
//! The fail-open capture path (`capture`, `events`, `spool`, and the
//! `threadspace-hook` executable) delivers observations to the event socket
//! or, failing that, to the bounded local spool.

pub mod capture;
pub mod client;
pub mod events;
pub mod frame;
pub mod locator;
pub mod paths;
pub mod peer;
pub mod runtime;
pub mod spool;
