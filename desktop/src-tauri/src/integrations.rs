//! Integrations: the token-authenticated local API, Yap's MCP server, and
//! adding Yap to AI apps, and the endpoint Claude Code runs started by Yap
//! talk back to. How it works: docs/architecture/integrations.md.

pub mod bridge;
pub mod claude_live;
pub mod mcp;
pub mod mcp_clients;
