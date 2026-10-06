//! Native macOS mechanisms used by the companion (SPEC §4.5, §13.3, §19.3):
//! kernel process incarnation sampling and validated ancestry, character-device
//! TTY identity, bounded argv subprocess execution and Terminal.app
//! dictionary/inventory/focus support. Nothing here types into a terminal,
//! reads terminal contents or interpolates data into script source.

pub mod ancestry;
pub mod exec;
pub mod process;
pub mod terminal;
pub mod tty;
