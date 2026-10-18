#![cfg(feature = "agent")]

use scope_ide::agent::{
    DecisionModel, EditRun, SemanticDiscoveryOptions, SemanticEditOptions,
    run_semantic_edit_with_discovery,
};
use scope_ide::scratch::ScratchBuffer;
use scope_ide::source_edit::{AgentEditRequest, SourceEditController};
use scope_ide::tool::semantic::SemanticAuthorization;
use scope_language::LanguageService;

#[test]
fn interactive_host_configuration_is_public() {
    let _entrypoint = |registry: scope_ide::language::LanguageRegistry,
                       editing: scope_ide::cli::InteractiveEditConfig| {
        scope_ide::cli::run_with_registry_and_editing(["--help".to_owned()], registry, editing)
    };
}

#[test]
fn semantic_discovery_is_available_through_agent() {
    let _options = SemanticDiscoveryOptions::default();
    let _entrypoint = |model: &mut dyn DecisionModel,
                       controller: &mut SourceEditController,
                       target: &ScratchBuffer,
                       service: &dyn LanguageService,
                       request: AgentEditRequest<'_>,
                       options: &SemanticEditOptions,
                       discovery: &SemanticDiscoveryOptions,
                       policy: fn(SemanticAuthorization<'_>) -> bool|
     -> EditRun {
        run_semantic_edit_with_discovery(
            model, controller, target, service, request, options, discovery, policy,
        )
    };
}
