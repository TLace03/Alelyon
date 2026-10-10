//! The research archive's core.
//!
//! - [`openalex`], [`arxiv`]: the indexes, each behind [`index::Index`], reading through [`index::Fetch`].
//! - [`archive`]: one record per paper however many indexes returned it.
//! - [`snowball`]: search, then follow citations to closure, accepting papers on text or link evidence.
//! - [`coverage`]: a capture-recapture estimate of the relevant papers nothing has found.
//! - [`graph`]: a subject's knowledge graph: citations, similarity, threads, influence, roles and a layout.
//! - [`gaps`]: where a subject's literature may have gaps worth pursuing, read from its graph and text.
//! - [`hub`]: what to work on next, personalised from this PC's own activity.
//! - [`store`]: the archive on disk, with full-text search and each subject's graph.
//! - [`jobs`]: a harvest or update as a process of its own, talking to the window through files.
//! - [`mcp`]: the archive and its graphs as a read-only MCP server for models and agents.
//! - `net`, `harvest` (feature `net`): the live HTTP transport, and one call that harvests a subject and saves it.

pub mod archive;
pub mod arxiv;
pub mod coverage;
pub mod gaps;
pub mod graph;
pub mod hub;
#[cfg(feature = "net")]
pub mod harvest;
pub mod harvest_date;
pub mod ident;
pub mod index;
pub mod jobs;
pub mod mcp;
#[cfg(feature = "net")]
pub mod net;
pub mod openalex;
pub mod snowball;
pub mod store;
pub mod work;
