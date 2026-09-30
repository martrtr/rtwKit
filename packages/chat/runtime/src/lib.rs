//! Component Model adapter for the standard persistent Chat package.
//!
//! The runtime exposes Chat-owned World Systems/Projection through generic service
//! contracts and renders the resulting Principal-filtered state through Portable UI.

use std::{cell::RefCell, collections::BTreeSet};

use rintawa_chat::{
    AddParticipantCommand, CHAT_ACTION_ATTACH_ASSET, CHAT_ACTION_CREATE_CONVERSATION,
    CHAT_ACTION_DELETE_MESSAGE, CHAT_ACTION_EDIT_CANCEL, CHAT_ACTION_EDIT_MESSAGE,
    CHAT_ACTION_EDIT_SUBMIT, CHAT_ACTION_REFRESH, CHAT_ACTION_REMOVE_ATTACHMENT,
    CHAT_ACTION_SELECT_BRANCH, CHAT_ACTION_SELECT_CONVERSATION, CHAT_ACTION_SEND,
    CHAT_ACTION_SHOW_CONVERSATIONS, CHAT_SURFACE_ID, ChatComposerAttachment, ChatConversationView,
    ChatProjectionInput, ContentBlock, CreateConversationCommand, DeleteMessageCommand,
    EditMessageCommand, ParticipantBindingRequest, SelectBranchCommand, SendMessageCommand,
    build_chat_snapshot, chat_add_participant_command_schema_key, chat_all_world_schemas,
    chat_conversation_projection_schema_key, chat_create_conversation_command_schema_key,
    chat_delete_message_command_schema_key, chat_edit_message_command_schema_key,
    chat_request_alternative_command_schema_key, chat_select_branch_command_schema_key,
    chat_send_message_command_schema_key, chat_surface_contribution, conversation_select_node_id,
    evaluate_chat_projection, evaluate_chat_world_system, message_delete_node_id,
    message_edit_cancel_node_id, message_edit_input_node_id, message_edit_node_id,
    message_select_branch_node_id, show_conversations_node_id,
};
use rintawa_sdk::{
    contracts::world_presentation_contract_key,
    ui::{UiActionEvent, UiActionPayload, UiNodeId, UiPatch, UiPatchBatch, UiPlacementHint},
    world::{CommandId, EntityId, SchemaKey, SchemaKind},
    world_projection::{
        WorldProjectionServiceRequest, WorldProjectionServiceResponse,
        world_projection_service_contract_key,
    },
    world_system::{
        WorldSystemServiceRequest, WorldSystemServiceResponse, world_system_service_contract_key,
    },
};
use serde::Serialize;

wit_bindgen::generate!({
    path: "../../../wit",
    world: "task-runtime-plugin",
});

const CHAT_POLL_INTERVAL_MS: u32 = 200;
const MAX_PROJECTION_POLLS: u16 = 100;
const LOCAL_PARTICIPANT_NAME: &str = "You";

thread_local! {
    static STATE: RefCell<RuntimeState> = RefCell::new(RuntimeState::default());
    static REGISTRATION_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    static PRESENTATION: RefCell<PresentationState> = RefCell::new(PresentationState::default());
}

#[derive(Debug, Clone, Default)]
struct RuntimeState {
    task_handle: Option<u64>,
    world_id: Option<String>,
    selected_conversation_id: Option<EntityId>,
    view: Option<ChatConversationView>,
    projection_operation_id: Option<String>,
    projection_polls: u16,
    refresh_needed: bool,
    status: Option<String>,
    editing_message_id: Option<EntityId>,
    attachments: Vec<ChatComposerAttachment>,
    ui_revision: u64,
}

#[derive(Debug, Clone, Default)]
struct PresentationState {
    mounted: bool,
    revision: u64,
    node_ids: BTreeSet<String>,
}

struct ChatRuntime;

impl exports::rintawa::engine::guest::Guest for ChatRuntime {
    fn register() {
        let error = register_runtime().err();
        REGISTRATION_ERROR.with(|slot| {
            *slot.borrow_mut() = error;
        });
    }

    fn start() {
        STATE.with(|slot| {
            *slot.borrow_mut() = RuntimeState::default();
        });
        PRESENTATION.with(|slot| {
            *slot.borrow_mut() = PresentationState::default();
        });

        if let Some(error) = REGISTRATION_ERROR.with(|slot| slot.borrow().clone()) {
            log_error(&error);
            return;
        }

        let context = match rintawa::engine::runtime_context::current() {
            Ok(context) => context,
            Err(error) => {
                log_error(&format!("Chat runtime context read failed: {error:?}"));
                return;
            }
        };
        let Some(world_id) = context.world_id else {
            log_error("Chat runtime requires an exact World scope");
            return;
        };
        STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            state.world_id = Some(world_id);
            state.refresh_needed = true;
            state.status = Some(String::from("Opening Chat…"));
        });

        let task_handle =
            match rintawa::engine::runtime_tasks::spawn_periodic(CHAT_POLL_INTERVAL_MS) {
                Ok(handle) => handle,
                Err(error) => {
                    log_error(&format!("Chat runtime task scheduling failed: {error:?}"));
                    return;
                }
            };
        STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            state.task_handle = Some(task_handle);
        });

        if let Err(error) = render_surface() {
            log_error(&error);
        }
    }

    fn stop() {
        if let Some(task_handle) = STATE.with(|slot| slot.borrow_mut().task_handle.take()) {
            let _ = rintawa::engine::runtime_tasks::cancel(task_handle);
        }
        STATE.with(|slot| {
            *slot.borrow_mut() = RuntimeState::default();
        });
        PRESENTATION.with(|slot| {
            *slot.borrow_mut() = PresentationState::default();
        });
        let _ = rintawa::engine::portable_ui::unmount_surface(CHAT_SURFACE_ID);
    }

    fn on_event(_topic: String, _payload: Vec<u8>) {}

    fn handle_ui_action(action_json: Vec<u8>) {
        let event = match serde_json::from_slice::<UiActionEvent>(&action_json) {
            Ok(event) => event,
            Err(error) => {
                log_error(&format!("Chat received malformed UI action: {error}"));
                return;
            }
        };
        if let Err(error) = handle_ui_action(&event) {
            record_runtime_error(&error);
            return;
        }
        if let Err(error) = render_surface() {
            log_error(&error);
        }
    }

    fn handle_service(contract: String, version: u32, payload: Vec<u8>) -> Vec<u8> {
        handle_service(&contract, version, &payload)
    }
}

impl exports::rintawa::engine::task_handler::Guest for ChatRuntime {
    fn on_task(handle: u64) {
        let matches = STATE.with(|slot| slot.borrow().task_handle == Some(handle));
        if !matches {
            return;
        }
        if let Err(error) = tick() {
            record_runtime_error(&error);
        }
    }
}

fn register_runtime() -> Result<(), String> {
    register_world_contracts()?;
    register_surface()?;
    register_world_presentation()
}

fn register_world_presentation() -> Result<(), String> {
    let contract = world_presentation_contract_key();
    rintawa::engine::registration::provide_contract(
        &contract.id.to_string(),
        contract.version.major(),
        &[],
    )
    .map_err(|error| format!("Chat World presentation registration failed: {error:?}"))?;
    rintawa::engine::portable_ui::register_world_presentation(CHAT_SURFACE_ID, None, None)
        .map_err(|error| format!("Chat World presentation descriptor failed: {error:?}"))
}

fn register_world_contracts() -> Result<(), String> {
    for schema in chat_all_world_schemas()
        .map_err(|error| format!("Chat world schema construction failed: {error}"))?
    {
        let kind = schema_kind(schema.kind());
        rintawa::engine::world_registration::register_world_schema(
            schema.key().id().as_str(),
            schema.key().version().get(),
            kind,
            schema.definition_json(),
        )
        .map_err(|error| format!("Chat world schema registration failed: {error:?}"))?;
    }

    for schema in command_schema_keys()? {
        let contract = world_system_service_contract_key(&schema);
        rintawa::engine::registration::provide_contract(
            &contract.id.to_string(),
            contract.version.major(),
            &[],
        )
        .map_err(|error| format!("Chat World System registration failed: {error:?}"))?;
    }

    let projection_schema = chat_conversation_projection_schema_key()
        .map_err(|error| format!("Chat projection schema construction failed: {error}"))?;
    let contract = world_projection_service_contract_key(&projection_schema);
    rintawa::engine::registration::provide_contract(
        &contract.id.to_string(),
        contract.version.major(),
        &[],
    )
    .map_err(|error| format!("Chat projection registration failed: {error:?}"))?;
    Ok(())
}

fn register_surface() -> Result<(), String> {
    let contribution = chat_surface_contribution();
    let placement = match contribution.placement {
        UiPlacementHint::Primary => rintawa::engine::portable_ui::PlacementHint::Primary,
        UiPlacementHint::Secondary => rintawa::engine::portable_ui::PlacementHint::Secondary,
        UiPlacementHint::Sidebar => rintawa::engine::portable_ui::PlacementHint::Sidebar,
        UiPlacementHint::Settings => rintawa::engine::portable_ui::PlacementHint::Settings,
        UiPlacementHint::Dialog => rintawa::engine::portable_ui::PlacementHint::Dialog,
        UiPlacementHint::Status => rintawa::engine::portable_ui::PlacementHint::Status,
        UiPlacementHint::Overlay => rintawa::engine::portable_ui::PlacementHint::Overlay,
    };
    let semantic_id = contribution
        .semantic
        .as_ref()
        .map(|semantic| semantic.id.to_string());
    let semantic_version = contribution
        .semantic
        .as_ref()
        .map(|semantic| semantic.version.major());
    let activity =
        contribution
            .activity
            .as_ref()
            .map(|activity| rintawa::engine::portable_ui::Activity {
                id: activity.id.as_str().to_string(),
                label: activity.label.clone(),
                icon_slot: activity
                    .icon_slot
                    .as_ref()
                    .map(|icon_slot| icon_slot.as_str().to_string()),
            });
    let traits = contribution
        .traits
        .iter()
        .map(|trait_id| trait_id.as_str().to_string())
        .collect::<Vec<_>>();
    let required_capabilities = contribution
        .required_capabilities
        .iter()
        .map(|capability| capability.as_str().to_string())
        .collect::<Vec<_>>();

    rintawa::engine::portable_ui::register_surface(
        contribution.id.as_str(),
        placement,
        semantic_id.as_deref(),
        semantic_version,
        activity.as_ref(),
        &traits,
        &required_capabilities,
    )
    .map_err(|error| format!("Chat UI registration failed: {error:?}"))
}

fn tick() -> Result<(), String> {
    if poll_projection()? {
        render_surface()?;
    }

    let should_request = STATE.with(|slot| {
        let state = slot.borrow();
        state.refresh_needed && state.projection_operation_id.is_none() && state.world_id.is_some()
    });
    if should_request {
        request_projection()?;
        render_surface()?;
    }
    Ok(())
}

fn request_projection() -> Result<(), String> {
    let (world_id, selected_conversation_id) = STATE.with(|slot| {
        let state = slot.borrow();
        (state.world_id.clone(), state.selected_conversation_id)
    });
    let world_id = world_id.ok_or_else(|| String::from("Chat has no bound World"))?;
    let schema = chat_conversation_projection_schema_key()
        .map_err(|error| format!("Chat projection schema construction failed: {error}"))?;
    let input_json = serde_json::to_vec(&ChatProjectionInput {
        conversation_id: selected_conversation_id,
    })
    .map_err(|error| format!("Chat projection input serialization failed: {error}"))?;
    let accepted = rintawa::engine::world_projections::request_read(
        &rintawa::engine::world_projections::Request {
            world_id,
            schema: schema.to_string(),
            input_json,
        },
    )
    .map_err(|error| format!("Chat projection request failed: {error:?}"))?;

    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.projection_operation_id = Some(accepted.operation_id);
        state.projection_polls = 0;
        state.refresh_needed = false;
        state.status = Some(String::from("Loading Chat…"));
    });
    Ok(())
}

fn poll_projection() -> Result<bool, String> {
    let operation_id = STATE.with(|slot| slot.borrow().projection_operation_id.clone());
    let Some(operation_id) = operation_id else {
        return Ok(false);
    };

    match rintawa::engine::world_projections::read_status(&operation_id) {
        Ok(rintawa::engine::world_projections::ReadState::Pending) => {
            let timed_out = STATE.with(|slot| {
                let mut state = slot.borrow_mut();
                state.projection_polls = state.projection_polls.saturating_add(1);
                state.projection_polls > MAX_PROJECTION_POLLS
            });
            if timed_out {
                clear_projection_with_status("Chat projection timed out before Host completion");
                return Ok(true);
            }
            Ok(false)
        }
        Ok(rintawa::engine::world_projections::ReadState::Succeeded(view)) => {
            let expected_schema = chat_conversation_projection_schema_key()
                .map_err(|error| format!("Chat projection schema construction failed: {error}"))?;
            let active_world = STATE.with(|slot| slot.borrow().world_id.clone());
            if active_world.as_deref() != Some(view.world_id.as_str())
                || view.schema != expected_schema.to_string()
            {
                clear_projection_with_status(
                    "Chat projection returned stale or mismatched World state",
                );
                return Ok(true);
            }
            let conversation_view =
                serde_json::from_slice::<ChatConversationView>(&view.value_json)
                    .map_err(|error| format!("Chat projection decoding failed: {error}"))?;
            STATE.with(|slot| {
                let mut state = slot.borrow_mut();
                state.selected_conversation_id = conversation_view.selected_conversation_id;
                if state.editing_message_id.is_some_and(|editing| {
                    !conversation_view
                        .messages
                        .iter()
                        .any(|message| message.id == editing)
                }) {
                    state.editing_message_id = None;
                }
                state.view = Some(conversation_view);
                state.projection_operation_id = None;
                state.projection_polls = 0;
                state.status = None;
            });
            Ok(true)
        }
        Ok(rintawa::engine::world_projections::ReadState::Failed(diagnostic)) => {
            clear_projection_with_status(&format!("Chat projection failed: {diagnostic}"));
            Ok(true)
        }
        Err(error) => {
            clear_projection_with_status(&format!("Chat projection status failed: {error:?}"));
            Ok(true)
        }
    }
}

fn clear_projection_with_status(status: &str) {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.projection_operation_id = None;
        state.projection_polls = 0;
        state.refresh_needed = false;
        state.status = Some(status.to_string());
    });
}

fn handle_ui_action(event: &UiActionEvent) -> Result<(), String> {
    if event.surface_id.as_str() != CHAT_SURFACE_ID {
        return Err(String::from("Chat action targeted an unexpected surface"));
    }
    let current_revision = PRESENTATION.with(|slot| {
        let presentation = slot.borrow();
        presentation.mounted.then_some(presentation.revision)
    });
    if current_revision != Some(event.surface_revision) {
        return Err(String::from(
            "Chat action targeted a stale surface revision",
        ));
    }

    match event.action_id.as_str() {
        CHAT_ACTION_REFRESH => {
            require_no_payload(event)?;
            mark_refresh("Refreshing Chat…");
            Ok(())
        }
        CHAT_ACTION_CREATE_CONVERSATION => {
            require_no_payload(event)?;
            create_conversation()
        }
        CHAT_ACTION_SELECT_CONVERSATION => {
            require_no_payload(event)?;
            select_conversation(event)
        }
        CHAT_ACTION_SHOW_CONVERSATIONS => {
            require_no_payload(event)?;
            show_conversations(event)
        }
        CHAT_ACTION_SELECT_BRANCH => {
            require_no_payload(event)?;
            select_branch(event)
        }
        CHAT_ACTION_EDIT_MESSAGE => {
            require_no_payload(event)?;
            begin_edit_message(event)
        }
        CHAT_ACTION_EDIT_CANCEL => {
            require_no_payload(event)?;
            cancel_edit_message(event)
        }
        CHAT_ACTION_EDIT_SUBMIT => submit_edit_message(event),
        CHAT_ACTION_DELETE_MESSAGE => {
            require_no_payload(event)?;
            delete_message(event)
        }
        CHAT_ACTION_ATTACH_ASSET => attach_asset(event),
        CHAT_ACTION_REMOVE_ATTACHMENT => {
            require_no_payload(event)?;
            remove_attachment(event)
        }
        CHAT_ACTION_SEND => send_message(event),
        _ => Err(String::from("Chat received an unknown semantic action")),
    }
}

fn create_conversation() -> Result<(), String> {
    let create = submit_command(
        chat_create_conversation_command_schema_key()
            .map_err(|error| format!("Chat command schema construction failed: {error}"))?,
        rintawa::engine::world_commands::Actor::Principal,
        &CreateConversationCommand { title: None },
    )?;
    let command_id = create
        .command_id
        .parse::<CommandId>()
        .map_err(|_| String::from("Host returned an invalid Chat command identity"))?;
    let conversation_id = EntityId::from_bytes(command_id.into_bytes());

    submit_command(
        chat_add_participant_command_schema_key()
            .map_err(|error| format!("Chat command schema construction failed: {error}"))?,
        rintawa::engine::world_commands::Actor::Principal,
        &AddParticipantCommand {
            conversation_id,
            display_name: String::from(LOCAL_PARTICIPANT_NAME),
            binding: ParticipantBindingRequest::CurrentPrincipal,
        },
    )?;

    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.selected_conversation_id = Some(conversation_id);
        state.editing_message_id = None;
        state.refresh_needed = true;
        state.status = Some(String::from("Creating conversation…"));
    });
    Ok(())
}

fn show_conversations(event: &UiActionEvent) -> Result<(), String> {
    if event.node_id != show_conversations_node_id() {
        return Err(String::from(
            "Chat conversation-browser action is stale or spoofed",
        ));
    }
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        let has_multiple = state
            .view
            .as_ref()
            .ok_or_else(|| String::from("Chat has no loaded conversation catalog"))?
            .conversations
            .len()
            > 1;
        if !has_multiple {
            return Err(String::from(
                "Chat conversation browser requires multiple conversations",
            ));
        }
        state.selected_conversation_id = None;
        state.editing_message_id = None;
        state.status = None;
        let view = state
            .view
            .as_mut()
            .ok_or_else(|| String::from("Chat has no loaded conversation catalog"))?;
        view.selected_conversation_id = None;
        view.selected_leaf = None;
        view.participants.clear();
        view.messages.clear();
        Ok(())
    })
}

fn select_conversation(event: &UiActionEvent) -> Result<(), String> {
    let selected = STATE.with(|slot| {
        let state = slot.borrow();
        let view = state.view.as_ref()?;
        view.conversations
            .iter()
            .enumerate()
            .find(|(index, _)| conversation_select_node_id(*index) == event.node_id)
            .map(|(_, conversation)| conversation.id)
    });
    let selected =
        selected.ok_or_else(|| String::from("Chat conversation action is stale or spoofed"))?;
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.selected_conversation_id = Some(selected);
        state.editing_message_id = None;
        state.refresh_needed = true;
        state.status = Some(String::from("Loading conversation…"));
    });
    Ok(())
}

fn select_branch(event: &UiActionEvent) -> Result<(), String> {
    let selection = STATE.with(|slot| {
        let state = slot.borrow();
        let view = state.view.as_ref()?;
        let conversation_id = view.selected_conversation_id?;
        let message_id = view
            .messages
            .iter()
            .enumerate()
            .find(|(index, _)| message_select_branch_node_id(*index) == event.node_id)
            .map(|(_, message)| message.id)?;
        Some((conversation_id, message_id))
    });
    let (conversation_id, message_id) =
        selection.ok_or_else(|| String::from("Chat branch action is stale or spoofed"))?;
    submit_command(
        chat_select_branch_command_schema_key()
            .map_err(|error| format!("Chat command schema construction failed: {error}"))?,
        rintawa::engine::world_commands::Actor::Principal,
        &SelectBranchCommand {
            conversation_id,
            message_id,
        },
    )?;
    STATE.with(|slot| slot.borrow_mut().editing_message_id = None);
    mark_refresh("Selecting branch…");
    Ok(())
}

fn begin_edit_message(event: &UiActionEvent) -> Result<(), String> {
    let message_id = STATE.with(|slot| {
        let state = slot.borrow();
        let view = state.view.as_ref()?;
        view.messages
            .iter()
            .enumerate()
            .find(|(index, message)| {
                message_edit_node_id(*index) == event.node_id && message_is_editable(view, message)
            })
            .map(|(_, message)| message.id)
    });
    let message_id = message_id
        .ok_or_else(|| String::from("Chat edit action is stale, spoofed, or not authorized"))?;
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.editing_message_id = Some(message_id);
        state.status = None;
    });
    Ok(())
}

fn cancel_edit_message(event: &UiActionEvent) -> Result<(), String> {
    let matches = STATE.with(|slot| {
        let state = slot.borrow();
        let editing = state.editing_message_id?;
        let view = state.view.as_ref()?;
        view.messages
            .iter()
            .position(|message| message.id == editing)
            .map(|index| message_edit_cancel_node_id(index) == event.node_id)
    });
    if matches != Some(true) {
        return Err(String::from("Chat edit-cancel action is stale or spoofed"));
    }
    STATE.with(|slot| slot.borrow_mut().editing_message_id = None);
    Ok(())
}

fn submit_edit_message(event: &UiActionEvent) -> Result<(), String> {
    let replacement = match &event.payload {
        UiActionPayload::Text(text) if !text.trim().is_empty() => text.clone(),
        _ => return Err(String::from("Chat message edit requires non-empty content")),
    };
    let context = STATE.with(|slot| {
        let state = slot.borrow();
        let editing = state.editing_message_id?;
        let view = state.view.as_ref()?;
        let (index, message) = view
            .messages
            .iter()
            .enumerate()
            .find(|(_, message)| message.id == editing)?;
        if message_edit_input_node_id(index) != event.node_id || !message_is_editable(view, message)
        {
            return None;
        }
        let participant = view.participants.iter().find(|participant| {
            participant.id == message.author_participant_id && participant.can_send
        })?;
        let block = match message.blocks.as_slice() {
            [ContentBlock::Markdown { .. }] => ContentBlock::Markdown {
                markdown: replacement.clone(),
            },
            [ContentBlock::Text { .. }] => ContentBlock::Text {
                text: replacement.clone(),
            },
            _ => return None,
        };
        Some((message.id, participant.linked_entity, block))
    });
    let (message_id, linked_entity, block) = context
        .ok_or_else(|| String::from("Chat edit action is stale, spoofed, or not authorized"))?;
    let actor = linked_entity.map_or(
        rintawa::engine::world_commands::Actor::Principal,
        |entity_id| rintawa::engine::world_commands::Actor::Entity(entity_id.to_string()),
    );
    submit_command(
        chat_edit_message_command_schema_key()
            .map_err(|error| format!("Chat command schema construction failed: {error}"))?,
        actor,
        &EditMessageCommand {
            message_id,
            blocks: vec![block],
        },
    )?;
    STATE.with(|slot| slot.borrow_mut().editing_message_id = None);
    mark_refresh("Saving message…");
    Ok(())
}

fn delete_message(event: &UiActionEvent) -> Result<(), String> {
    let message_id = STATE.with(|slot| {
        let state = slot.borrow();
        let view = state.view.as_ref()?;
        view.messages
            .iter()
            .enumerate()
            .find(|(index, message)| {
                message_delete_node_id(*index) == event.node_id
                    && view.selected_leaf == Some(message.id)
            })
            .map(|(_, message)| message.id)
    });
    let message_id = message_id.ok_or_else(|| {
        String::from("Chat delete action is stale or does not target the selected leaf")
    })?;
    submit_command(
        chat_delete_message_command_schema_key()
            .map_err(|error| format!("Chat command schema construction failed: {error}"))?,
        rintawa::engine::world_commands::Actor::Principal,
        &DeleteMessageCommand { message_id },
    )?;
    STATE.with(|slot| slot.borrow_mut().editing_message_id = None);
    mark_refresh("Deleting message…");
    Ok(())
}

fn message_is_editable(
    view: &ChatConversationView,
    message: &rintawa_chat::ChatMessageView,
) -> bool {
    let controllable_author = view
        .participants
        .iter()
        .any(|participant| participant.id == message.author_participant_id && participant.can_send);
    controllable_author
        && matches!(
            message.blocks.as_slice(),
            [ContentBlock::Text { .. }] | [ContentBlock::Markdown { .. }]
        )
}

fn attach_asset(event: &UiActionEvent) -> Result<(), String> {
    let UiActionPayload::Asset(reference) = &event.payload else {
        return Err(String::from("Chat attachment requires an asset reference"));
    };
    if event.node_id.as_str() != "composer.attach" {
        return Err(String::from("Chat attachment action is stale or spoofed"));
    }
    let digest = rintawa_artifacts::AssetDigest::parse(&reference.digest)
        .map_err(|error| format!("Chat attachment digest is invalid: {error}"))?;
    let asset =
        rintawa_artifacts::AssetRef::new(digest, reference.size, reference.media_type.clone())
            .map_err(|error| format!("Chat attachment reference is invalid: {error}"))?;
    if !matches!(
        asset.media_type.as_str(),
        "image/png" | "image/jpeg" | "image/webp"
    ) {
        return Err(String::from("Chat attachment media type is unsupported"));
    }
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.attachments.len() >= 8 {
            return Err(String::from("Chat composer attachment limit reached"));
        }
        state.attachments.push(ChatComposerAttachment {
            asset,
            name: reference.name.clone(),
        });
        state.status = None;
        Ok(())
    })
}

fn remove_attachment(event: &UiActionEvent) -> Result<(), String> {
    let index = STATE.with(|slot| {
        let state = slot.borrow();
        state
            .attachments
            .iter()
            .enumerate()
            .find(|(index, _)| rintawa_chat::attachment_remove_node_id(*index) == event.node_id)
            .map(|(index, _)| index)
    });
    let index = index.ok_or_else(|| String::from("Chat attachment action is stale or spoofed"))?;
    STATE.with(|slot| {
        slot.borrow_mut().attachments.remove(index);
    });
    Ok(())
}

fn send_message(event: &UiActionEvent) -> Result<(), String> {
    let text = match &event.payload {
        UiActionPayload::Text(text) => text.clone(),
        _ => return Err(String::from("Chat composer requires text payload")),
    };
    let attachments = STATE.with(|slot| slot.borrow().attachments.clone());
    if text.trim().is_empty() && attachments.is_empty() {
        return Err(String::from("Chat composer requires text or an attachment"));
    }
    let context = STATE.with(|slot| {
        let state = slot.borrow();
        let view = state.view.as_ref()?;
        let conversation_id = view.selected_conversation_id?;
        let participant = view
            .participants
            .iter()
            .find(|participant| participant.can_send)?;
        Some((
            conversation_id,
            participant.id,
            participant.linked_entity,
            view.selected_leaf,
        ))
    });
    let (conversation_id, participant_id, linked_entity, parent_message_id) =
        context.ok_or_else(|| {
            String::from("Chat has no controllable participant in the selected conversation")
        })?;
    let actor = linked_entity.map_or(
        rintawa::engine::world_commands::Actor::Principal,
        |entity_id| rintawa::engine::world_commands::Actor::Entity(entity_id.to_string()),
    );
    let mut blocks = Vec::with_capacity(usize::from(!text.trim().is_empty()) + attachments.len());
    if !text.trim().is_empty() {
        blocks.push(ContentBlock::Markdown { markdown: text });
    }
    blocks.extend(
        attachments
            .iter()
            .cloned()
            .map(|attachment| ContentBlock::ImageRef {
                asset: attachment.asset,
                alt: attachment.name,
            }),
    );
    submit_command(
        chat_send_message_command_schema_key()
            .map_err(|error| format!("Chat command schema construction failed: {error}"))?,
        actor,
        &SendMessageCommand {
            conversation_id,
            participant_id,
            parent_message_id,
            alternative_of: None,
            blocks,
        },
    )?;
    STATE.with(|slot| slot.borrow_mut().attachments.clear());
    mark_refresh("Sending message…");
    Ok(())
}

fn submit_command<T: Serialize>(
    schema: SchemaKey,
    actor: rintawa::engine::world_commands::Actor,
    payload: &T,
) -> Result<rintawa::engine::world_commands::Accepted, String> {
    let world_id = STATE
        .with(|slot| slot.borrow().world_id.clone())
        .ok_or_else(|| String::from("Chat has no bound World"))?;
    let payload_json = serde_json::to_vec(payload)
        .map_err(|error| format!("Chat command serialization failed: {error}"))?;
    rintawa::engine::world_commands::submit(&rintawa::engine::world_commands::Request {
        world_id,
        schema: schema.to_string(),
        actor,
        expected_position: None,
        payload_json,
    })
    .map_err(|error| format!("Chat command submission failed: {error:?}"))
}

fn require_no_payload(event: &UiActionEvent) -> Result<(), String> {
    if event.payload == UiActionPayload::None {
        Ok(())
    } else {
        Err(String::from("Chat action carried an unexpected payload"))
    }
}

fn mark_refresh(status: &str) {
    STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.refresh_needed = true;
        state.status = Some(status.to_string());
    });
}

fn handle_service(contract: &str, version: u32, payload: &[u8]) -> Vec<u8> {
    let command_schemas = match command_schema_keys() {
        Ok(schemas) => schemas,
        Err(error) => {
            log_error(&error);
            return Vec::new();
        }
    };
    for schema in command_schemas {
        let expected = world_system_service_contract_key(&schema);
        if contract == expected.id.to_string() && version == expected.version.major() {
            return handle_world_system_service(&schema, payload);
        }
    }

    let projection_schema = match chat_conversation_projection_schema_key() {
        Ok(schema) => schema,
        Err(error) => {
            log_error(&format!(
                "Chat projection schema construction failed: {error}"
            ));
            return Vec::new();
        }
    };
    let expected = world_projection_service_contract_key(&projection_schema);
    if contract == expected.id.to_string() && version == expected.version.major() {
        return handle_projection_service(&projection_schema, payload);
    }

    log_error("Chat runtime received an unexpected service contract");
    Vec::new()
}

fn handle_world_system_service(schema: &SchemaKey, payload: &[u8]) -> Vec<u8> {
    let request = match serde_json::from_slice::<WorldSystemServiceRequest>(payload) {
        Ok(request) => request,
        Err(error) => {
            log_error(&format!("Chat System request decoding failed: {error}"));
            return encode_system_response(WorldSystemServiceResponse::Failed {
                reason: String::from("invalid Chat System request"),
            });
        }
    };
    if &request.command.schema != schema {
        return encode_system_response(WorldSystemServiceResponse::Rejected {
            reason: String::from("Chat command schema does not match routed service"),
        });
    }
    encode_system_response(evaluate_chat_world_system(&request))
}

fn handle_projection_service(schema: &SchemaKey, payload: &[u8]) -> Vec<u8> {
    let request = match serde_json::from_slice::<WorldProjectionServiceRequest>(payload) {
        Ok(request) => request,
        Err(error) => {
            log_error(&format!("Chat projection request decoding failed: {error}"));
            return encode_projection_response(WorldProjectionServiceResponse::Failed {
                reason: String::from("invalid Chat projection request"),
            });
        }
    };
    if &request.projection_schema != schema {
        return encode_projection_response(WorldProjectionServiceResponse::Rejected {
            reason: String::from("Chat projection schema does not match routed service"),
        });
    }
    encode_projection_response(evaluate_chat_projection(&request))
}

fn encode_system_response(response: WorldSystemServiceResponse) -> Vec<u8> {
    match serde_json::to_vec(&response) {
        Ok(payload) => payload,
        Err(error) => {
            log_error(&format!(
                "Chat System response serialization failed: {error}"
            ));
            Vec::new()
        }
    }
}

fn encode_projection_response(response: WorldProjectionServiceResponse) -> Vec<u8> {
    match serde_json::to_vec(&response) {
        Ok(payload) => payload,
        Err(error) => {
            log_error(&format!(
                "Chat projection response serialization failed: {error}"
            ));
            Vec::new()
        }
    }
}

fn command_schema_keys() -> Result<Vec<SchemaKey>, String> {
    [
        chat_create_conversation_command_schema_key(),
        chat_add_participant_command_schema_key(),
        chat_send_message_command_schema_key(),
        chat_edit_message_command_schema_key(),
        chat_delete_message_command_schema_key(),
        chat_select_branch_command_schema_key(),
        chat_request_alternative_command_schema_key(),
    ]
    .into_iter()
    .map(|schema| {
        schema.map_err(|error| format!("Chat command schema construction failed: {error}"))
    })
    .collect()
}

fn schema_kind(kind: SchemaKind) -> rintawa::engine::world_registration::SchemaKind {
    match kind {
        SchemaKind::Entity => rintawa::engine::world_registration::SchemaKind::Entity,
        SchemaKind::Relation => rintawa::engine::world_registration::SchemaKind::Relation,
        SchemaKind::Facet => rintawa::engine::world_registration::SchemaKind::Facet,
        SchemaKind::Command => rintawa::engine::world_registration::SchemaKind::Command,
        SchemaKind::Event => rintawa::engine::world_registration::SchemaKind::Event,
        SchemaKind::Effect => rintawa::engine::world_registration::SchemaKind::Effect,
        SchemaKind::Projection => rintawa::engine::world_registration::SchemaKind::Projection,
    }
}

fn render_surface() -> Result<(), String> {
    let (revision, world_id, view, status, editing_message_id, attachments) =
        STATE.with(|slot| {
            let mut state = slot.borrow_mut();
            state.ui_revision = state
                .ui_revision
                .checked_add(1)
                .ok_or_else(|| String::from("Chat UI revision overflow"))?;
            Ok::<_, String>((
                state.ui_revision,
                state.world_id.clone(),
                state.view.clone(),
                state.status.clone(),
                state.editing_message_id,
                state.attachments.clone(),
            ))
        })?;
    let snapshot = build_chat_snapshot(
        revision,
        world_id.as_deref(),
        view.as_ref(),
        status.as_deref(),
        editing_message_id,
        &attachments,
    );
    let node_ids = snapshot
        .nodes
        .iter()
        .map(|node| node.id.as_str().to_string())
        .collect::<BTreeSet<_>>();
    let previous = PRESENTATION.with(|slot| slot.borrow().clone());

    let result = if previous.mounted {
        patch_snapshot(&previous, &snapshot, &node_ids)
    } else {
        mount_snapshot(&snapshot)
    };
    if let Err(error) = result {
        if !previous.mounted {
            return Err(error);
        }
        let _ = rintawa::engine::portable_ui::unmount_surface(CHAT_SURFACE_ID);
        mount_snapshot(&snapshot)
            .map_err(|mount_error| format!("{error}; Chat recovery mount failed: {mount_error}"))?;
    }

    PRESENTATION.with(|slot| {
        *slot.borrow_mut() = PresentationState {
            mounted: true,
            revision: snapshot.revision,
            node_ids,
        };
    });
    Ok(())
}

fn patch_snapshot(
    previous: &PresentationState,
    snapshot: &rintawa_sdk::ui::UiSurfaceSnapshot,
    node_ids: &BTreeSet<String>,
) -> Result<(), String> {
    if previous.revision.checked_add(1) != Some(snapshot.revision) {
        return Err(format!(
            "Chat presentation revision jumped from {} to {}",
            previous.revision, snapshot.revision
        ));
    }
    let mut patches = snapshot
        .nodes
        .iter()
        .cloned()
        .map(|node| UiPatch::UpsertNode { node })
        .collect::<Vec<_>>();
    patches.extend(
        previous
            .node_ids
            .difference(node_ids)
            .map(|node_id| UiPatch::RemoveNode {
                node_id: UiNodeId::new(node_id.clone()),
            }),
    );
    let batch = UiPatchBatch {
        surface_id: snapshot.surface_id.clone(),
        base_revision: previous.revision,
        next_revision: snapshot.revision,
        patches,
    };
    let payload = serde_json::to_vec(&batch)
        .map_err(|error| format!("Chat patch serialization failed: {error}"))?;
    rintawa::engine::portable_ui::patch_surface(&payload)
        .map_err(|error| format!("Chat surface patch failed: {error:?}"))
}

fn mount_snapshot(snapshot: &rintawa_sdk::ui::UiSurfaceSnapshot) -> Result<(), String> {
    let payload = serde_json::to_vec(snapshot)
        .map_err(|error| format!("Chat snapshot serialization failed: {error}"))?;
    rintawa::engine::portable_ui::mount_surface(&payload)
        .map_err(|error| format!("Chat surface mount failed: {error:?}"))
}

fn record_runtime_error(error: &str) {
    log_error(error);
    STATE.with(|slot| {
        slot.borrow_mut().status = Some(error.to_string());
    });
    if let Err(render_error) = render_surface() {
        log_error(&render_error);
    }
}

fn log_error(message: &str) {
    rintawa::engine::host::log(rintawa::engine::host::LogLevel::Error, message);
}

export!(ChatRuntime);
