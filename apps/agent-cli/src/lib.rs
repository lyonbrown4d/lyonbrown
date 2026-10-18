pub mod backend;
mod bootstrap;
pub mod config;
pub mod model;
pub mod theme;
pub mod update;
pub mod view;

mod entry;
pub mod task_command;
pub mod task_management;
mod terminal;
pub use entry::run;
agent_browser_runtime::windows_bootstrap_entry!(run);
