//! Native macOS mechanisms used by the companion (SPEC §4.5, §13.3, §19.3):
//! kernel process incarnation sampling, bounded argv subprocess execution and
//! Terminal.app dictionary/inventory support. Nothing here focuses a window,
//! types into a terminal or interpolates data into script source.

pub mod exec;
pub mod process;
pub mod terminal;
