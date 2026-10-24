//! The app shell: the tray, the windows' webviews (context menus, memory,
//! theme), portable mode, saving stores, helper processes, test and robot
//! mode, and `yap.exe bench`. How it works: docs/architecture/app-shell.md.

pub mod bench;
pub mod context_menu;
pub mod e2e;
pub mod portable;
pub mod procs;
pub mod realtest;
pub mod saving;
pub mod theme;
pub mod tray;
pub mod webview_memory;
