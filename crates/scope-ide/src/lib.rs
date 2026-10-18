#[cfg(feature = "agent")]
pub mod agent;
#[cfg(feature = "agent")]
pub mod agent_edit;
pub mod app;
pub mod cli;
pub mod document;
pub mod history;
pub mod language;
#[cfg(feature = "logging")]
pub mod logging;
pub mod path;
pub mod proposal;
pub mod scratch;
mod shell_input;
pub mod source_edit;
pub mod task_contract;
#[cfg(test)]
mod test_support;
pub mod text;
#[cfg(feature = "tools")]
pub mod tool;
#[cfg(feature = "tui")]
pub mod tui;
#[cfg(feature = "tools")]
pub mod workbench;
pub mod workspace;
