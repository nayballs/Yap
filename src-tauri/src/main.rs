// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // `yap.exe mcp`: the MCP server AI apps launch (mcp.rs). It speaks over
    // the stdin/stdout pipes they hand it, which a GUI-subsystem exe inherits
    // like any other (and it never flashes a console window), and exits when
    // they hang up, without starting any of the app.
    if std::env::args_os().nth(1).is_some_and(|arg| arg == "mcp") {
        std::process::exit(yap_lib::run_mcp_server());
    }
    yap_lib::run()
}
