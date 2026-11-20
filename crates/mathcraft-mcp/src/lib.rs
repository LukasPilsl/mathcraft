//! The Mathcraft MCP server. Layers, from the outside in:
//!
//! - [`stdio`]: the transport (lines of JSON). A second transport only has to call
//!   [`server::Server::handle`].
//! - [`server`]: JSON-RPC 2.0 and the MCP methods, independent of any stream.
//! - [`tools`]: the tool table; one file per tool group.
//! - [`backend`]: the [`backend::DocBackend`] trait that tools use to reach documents;
//!   [`headless`] is the in-process implementation, [`live`] the one for the running app.
//!
//! Worksheet content is data for the client, never instructions; every tool says so.

pub mod backend;
pub mod claude_desktop;
pub mod error;
pub mod headless;
pub mod keywords;
pub mod live;
pub mod paths;
pub mod prompts;
pub mod resources;
pub mod schema;
pub mod server;
pub mod stdio;
pub mod tools;

use std::sync::Arc;

pub use error::ToolError;

/// A headless server with the default tool table over `roots`.
///
/// # Errors
/// When no root directory is usable.
pub fn headless_server(
    root_dirs: &[std::path::PathBuf],
    max_documents: usize,
    limits: tools::Limits,
) -> Result<server::Server, String> {
    let roots = paths::Roots::new(root_dirs)?;
    let backend = Arc::new(headless::HeadlessBackend::new(roots, max_documents));
    Ok(server::Server::new(backend, tools::default_table(), limits))
}

/// A headless server behind the live router: calls go to the running app when the routing rules
/// say so (see [`live::router`]), else to the in-process documents.
///
/// # Errors
/// When no root directory is usable.
pub fn routed_server(
    root_dirs: &[std::path::PathBuf],
    max_documents: usize,
    limits: tools::Limits,
    client: live::router::LiveClient,
) -> Result<live::Router, String> {
    let roots = paths::Roots::new(root_dirs)?;
    let backend = Arc::new(headless::HeadlessBackend::new(roots, max_documents));
    let server = server::Server::new(backend.clone(), tools::default_table(), limits);
    Ok(live::Router::new(server, backend, client))
}
