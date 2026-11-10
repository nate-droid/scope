#[cfg(feature = "agent")]
pub mod agent;
#[cfg(feature = "agent")]
pub mod agent_edit;
#[cfg(feature = "tui")]
pub mod analysis;
pub mod app;
pub mod cli;
#[cfg(feature = "tools")]
pub mod closure_inspection;
pub mod document;
pub mod history;
pub mod language;
#[cfg(feature = "logging")]
pub mod logging;
pub mod path;
pub mod proposal;
#[cfg(feature = "experimental-sprout-closure")]
pub mod registered_document_closure;
pub mod scratch;
#[cfg(feature = "agent")]
pub mod semantic_context;
#[cfg(feature = "tools")]
pub mod semantic_inspection;
mod shell_input;
pub mod source_context;
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
