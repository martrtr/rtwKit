//! Persistent Chat domain schemas and ordinary World System evaluation.
//!
//! Chat is an ordinary Rintawa package. It owns conversation/message semantics but
//! relies exclusively on public World contracts for persistence and authority.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

mod ids;
mod model;
mod projection;
mod schemas;
mod system;
mod ui;

pub use model::{
    AddParticipantCommand, AlternativeRequestedEvent, BranchSelectedEvent, ChatModelError,
    ChatWorldIndex, ContentBlock, ConversationCreatedEvent, ConversationState,
    CreateConversationCommand, EditMessageCommand, MAX_CHAT_CONVERSATIONS, MAX_CHAT_MESSAGES,
    MAX_CHAT_PARTICIPANTS, MessageAddedEvent, MessageContent, MessageEditedEvent, MessageState,
    ParticipantAddedEvent, ParticipantBinding, ParticipantBindingRequest, ParticipantIdentity,
    RequestAlternativeCommand, SelectBranchCommand, SendMessageCommand,
};
pub use projection::{
    ChatConversationSummary, ChatConversationView, ChatMessageView, ChatParticipantView,
    ChatProjectionInput, evaluate_chat_projection,
};
pub use schemas::{
    ChatSchemaError, chat_add_participant_command_schema_key, chat_all_world_schemas,
    chat_conversation_projection_schema_key, chat_create_conversation_command_schema_key,
    chat_edit_message_command_schema_key, chat_request_alternative_command_schema_key,
    chat_select_branch_command_schema_key, chat_send_message_command_schema_key,
};
pub use system::evaluate_chat_world_system;
pub use ui::{
    CHAT_ACTION_CREATE_CONVERSATION, CHAT_ACTION_REFRESH, CHAT_ACTION_SELECT_BRANCH,
    CHAT_ACTION_SELECT_CONVERSATION, CHAT_ACTION_SEND, CHAT_ACTIVITY_ID, CHAT_SURFACE_ID,
    build_chat_snapshot, chat_surface_contribution, conversation_select_node_id,
    message_select_branch_node_id,
};
