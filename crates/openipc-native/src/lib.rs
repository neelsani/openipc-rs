//! Native command-line support for `openipc-rs`.
//!
//! The USB/Realtek implementation lives in `openipc-rtl88xx`; this crate keeps a
//! stable native-facing crate name for the CLI and downstream applications.

pub use openipc_rtl88xx::*;
