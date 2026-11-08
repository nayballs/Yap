//! Integrations: the token-authenticated local API, Yap's MCP server, and
//! adding Yap to AI apps, the endpoint Claude Code runs started by Yap talk
//! back to, and connectors (preview: Yap as an MCP client). How it works:
//! docs/architecture/integrations.md.

pub mod bridge;
pub mod claude_live;
pub mod connectors;
pub mod mcp;
pub mod mcp_clients;
