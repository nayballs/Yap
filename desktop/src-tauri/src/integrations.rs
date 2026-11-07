//! Integrations: the token-authenticated local API, Yap's MCP server, and
//! adding Yap to AI apps, and the endpoint Claude Code runs started by Yap
//! talk back to. How it works: docs/architecture/integrations.md.

pub mod bridge;
// Its API (`register_run`, `restore`…) is for the Claude Code runner (Chat on
// your Claude plan, Jobs), still to be built: until then only its tests and
// the approve commands call it.
#[allow(dead_code)]
pub mod claude_live;
pub mod mcp;
pub mod mcp_clients;
