//! Shared pieces of the `model` command line client.
//!
//! [`api`] holds the read-only query logic, with no argument parsing and no
//! I/O beyond the analyses themselves. The binary and the MCP server both use
//! it, so `model context …` and the `model_context` tool answer the same way.
//! [`load`] reads and writes `model.json`; [`output`] defines the JSON shapes.
//! [`graph`] holds the versioned graph projection, the Markdown report and
//! the shared shapes used by the CLI and MCP graph tools.

pub mod api;
pub mod commands;
pub mod graph;
pub mod load;
pub mod output;
