//! Chat-owned command, facet, content-block, and event models.

use rintawa_artifacts::AssetRef;
use rintawa_sdk::world::{EntityId, PrincipalId, UnixTimeMillis};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum conversations indexed by Chat in one World.
pub const MAX_CHAT_CONVERSATIONS: usize = 32;
/// Maximum conversation-local participants indexed by one Conversation.
pub const MAX_CHAT_PARTICIPANTS: usize = 32;
/// Maximum logical Messages indexed by one Conversation.
pub const MAX_CHAT_MESSAGES: usize = 72;
/// Maximum UTF-8 bytes accepted for a conversation title.
pub const MAX_CHAT_TITLE_BYTES: usize = 512;
/// Maximum UTF-8 bytes accepted for a participant display name.
pub const MAX_PARTICIPANT_NAME_BYTES: usize = 512;
/// Maximum semantic blocks in one immutable message revision.
pub const MAX_MESSAGE_BLOCKS: usize = 64;
/// Maximum UTF-8 bytes in one text-like block.
pub const MAX_MESSAGE_BLOCK_TEXT_BYTES: usize = 64 * 1024;
/// Maximum aggregate UTF-8 bytes across one revision's text-like blocks.
pub const MAX_MESSAGE_TEXT_BYTES: usize = 256 * 1024;
/// Maximum UTF-8 bytes accepted for one file/display label.
pub const MAX_ATTACHMENT_LABEL_BYTES: usize = 512;

/// Chat-domain validation failure before an authoritative transaction is proposed.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ChatModelError {
    /// A bounded non-empty text field was empty or oversized.
    #[error("chat text field `{field}` must contain 1..={maximum} UTF-8 bytes")]
    InvalidText {
        /// Stable field name suitable for diagnostics.
        field: &'static str,
        /// Maximum accepted byte count.
        maximum: usize,
    },
    /// Too many semantic blocks were supplied for one revision.
    #[error("message revision must contain 1..={MAX_MESSAGE_BLOCKS} content blocks")]
    TooManyBlocks,
    /// Aggregate text content exceeds the revision budget.
    #[error("message revision text exceeds the aggregate byte budget")]
    MessageTextTooLarge,
    /// Message parent and alternative references contradict the requested structure.
    #[error("message relationship references are contradictory")]
    ContradictoryMessageReferences,
    /// The World already contains the maximum number of Chat conversations.
    #[error("world Chat index exceeds the conversation bound")]
    TooManyConversations,
    /// The Conversation already contains the maximum number of participants.
    #[error("conversation participant index exceeds the participant bound")]
    TooManyParticipants,
    /// The Conversation already contains the maximum number of logical Messages.
    #[error("conversation message index exceeds the message bound")]
    TooManyMessages,
}

/// Conversation-local controller binding for one participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ParticipantBinding {
    /// Directly controlled by one authenticated outside principal.
    Principal {
        /// Principal allowed to author through this participant.
        principal_id: PrincipalId,
    },
    /// Controlled through one world entity and Core `ControlGrant` authority.
    Entity {
        /// World entity represented by this participant.
        entity_id: EntityId,
    },
    /// No direct controller; a separate System/Narrator may act for it later.
    Uncontrolled,
}

/// Binding requested when a new conversation-local participant is created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ParticipantBindingRequest {
    /// Bind to the Host-authenticated principal submitting the command.
    CurrentPrincipal,
    /// Bind to one controlled world entity.
    Entity {
        /// World entity to expose in this conversation.
        entity_id: EntityId,
    },
    /// Create a participant without a direct controller.
    Uncontrolled,
}

/// Bounded Chat-owned index attached to the World target.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ChatWorldIndex {
    /// Persistent Conversation identities in deterministic creation order.
    pub conversations: Vec<EntityId>,
}

impl ChatWorldIndex {
    /// Validates the bounded conversation index and rejects duplicate identities.
    ///
    /// # Errors
    ///
    /// Returns [`ChatModelError::TooManyConversations`] for an oversized index or
    /// [`ChatModelError::ContradictoryMessageReferences`] for duplicate identities.
    pub fn validate(&self) -> Result<(), ChatModelError> {
        validate_unique_ids(&self.conversations, MAX_CHAT_CONVERSATIONS).map_err(
            |error| match error {
                IndexValidationError::TooLarge => ChatModelError::TooManyConversations,
                IndexValidationError::Duplicate => ChatModelError::ContradictoryMessageReferences,
            },
        )
    }
}

/// Mutable current state attached to one Conversation entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ConversationState {
    /// Optional bounded human-facing title.
    pub title: Option<String>,
    /// Currently selected leaf in the visible branch, when one exists.
    pub selected_leaf: Option<EntityId>,
    /// Conversation-local participants in deterministic creation order.
    pub participant_ids: Vec<EntityId>,
    /// Logical Messages in deterministic creation order, including hidden alternatives.
    pub message_ids: Vec<EntityId>,
}

impl ConversationState {
    /// Validates bounded persistent indexes and selected-leaf membership.
    ///
    /// # Errors
    ///
    /// Returns [`ChatModelError`] for an oversized/duplicate index or an invalid selected leaf.
    pub fn validate(&self) -> Result<(), ChatModelError> {
        validate_title(&self.title)?;
        validate_unique_ids(&self.participant_ids, MAX_CHAT_PARTICIPANTS).map_err(|error| {
            match error {
                IndexValidationError::TooLarge => ChatModelError::TooManyParticipants,
                IndexValidationError::Duplicate => ChatModelError::ContradictoryMessageReferences,
            }
        })?;
        validate_unique_ids(&self.message_ids, MAX_CHAT_MESSAGES).map_err(|error| match error {
            IndexValidationError::TooLarge => ChatModelError::TooManyMessages,
            IndexValidationError::Duplicate => ChatModelError::ContradictoryMessageReferences,
        })?;
        if self
            .selected_leaf
            .is_some_and(|selected| !self.message_ids.contains(&selected))
        {
            return Err(ChatModelError::ContradictoryMessageReferences);
        }
        Ok(())
    }
}

/// Conversation-local identity and controller metadata for one Participant entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ParticipantIdentity {
    /// Human-facing name rendered by Chat UI.
    pub display_name: String,
    /// Authority/controller binding for authored commands.
    pub binding: ParticipantBinding,
}

/// Mutable head pointer attached to one logical Message entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MessageState {
    /// Conversation that owns this logical message.
    pub conversation_id: EntityId,
    /// Conversation-local participant that authored this message.
    pub author_participant_id: EntityId,
    /// Optional parent message in the branch graph.
    pub parent_message_id: Option<EntityId>,
    /// Optional earlier message for which this Message is an alternative variant.
    pub alternative_of: Option<EntityId>,
    /// Current immutable MessageRevision entity.
    pub current_revision: EntityId,
}

/// One renderer-neutral semantic content block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ContentBlock {
    /// Plain text content.
    Text {
        /// Exact UTF-8 text.
        text: String,
    },
    /// Markdown content that remains data rather than executable UI.
    Markdown {
        /// Exact UTF-8 markdown source.
        markdown: String,
    },
    /// Immutable image attachment.
    ImageRef {
        /// Content-addressed immutable asset.
        asset: AssetRef,
        /// Optional bounded accessibility/display text.
        alt: Option<String>,
    },
    /// Immutable generic file attachment.
    FileRef {
        /// Content-addressed immutable asset.
        asset: AssetRef,
        /// Bounded display filename/label.
        name: String,
    },
}

impl ContentBlock {
    fn validate(&self) -> Result<usize, ChatModelError> {
        match self {
            Self::Text { text } => {
                validate_required_text("text", text, MAX_MESSAGE_BLOCK_TEXT_BYTES)?;
                Ok(text.len())
            }
            Self::Markdown { markdown } => {
                validate_required_text("markdown", markdown, MAX_MESSAGE_BLOCK_TEXT_BYTES)?;
                Ok(markdown.len())
            }
            Self::ImageRef { alt, .. } => {
                if let Some(alt) = alt {
                    validate_optional_text("image-alt", alt, MAX_ATTACHMENT_LABEL_BYTES)?;
                }
                Ok(alt.as_ref().map_or(0, String::len))
            }
            Self::FileRef { name, .. } => {
                validate_required_text("file-name", name, MAX_ATTACHMENT_LABEL_BYTES)?;
                Ok(name.len())
            }
        }
    }
}

/// Immutable payload attached to one MessageRevision entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MessageContent {
    /// Ordered semantic blocks.
    pub blocks: Vec<ContentBlock>,
    /// Optional authoritative semantic timestamp copied from the command envelope.
    pub effective_at: Option<UnixTimeMillis>,
}

impl MessageContent {
    /// Validates block count and bounded text budgets.
    ///
    /// # Errors
    ///
    /// Returns [`ChatModelError`] for an empty/oversized block or aggregate budget overflow.
    pub fn validate(&self) -> Result<(), ChatModelError> {
        if self.blocks.is_empty() || self.blocks.len() > MAX_MESSAGE_BLOCKS {
            return Err(ChatModelError::TooManyBlocks);
        }
        let total = self.blocks.iter().try_fold(0_usize, |total, block| {
            let block_bytes = block.validate()?;
            total
                .checked_add(block_bytes)
                .ok_or(ChatModelError::MessageTextTooLarge)
        })?;
        if total > MAX_MESSAGE_TEXT_BYTES {
            return Err(ChatModelError::MessageTextTooLarge);
        }
        Ok(())
    }
}

/// Creates one persistent Conversation entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CreateConversationCommand {
    /// Optional human-facing title.
    pub title: Option<String>,
}

/// Adds one conversation-local participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct AddParticipantCommand {
    /// Existing Conversation entity.
    pub conversation_id: EntityId,
    /// Human-facing participant name.
    pub display_name: String,
    /// Requested controller binding.
    pub binding: ParticipantBindingRequest,
}

/// Creates one logical Message and its initial immutable revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SendMessageCommand {
    /// Existing Conversation entity.
    pub conversation_id: EntityId,
    /// Existing conversation-local Participant entity authoring the message.
    pub participant_id: EntityId,
    /// Optional parent message in the selected/derived branch.
    pub parent_message_id: Option<EntityId>,
    /// Optional earlier message for which this message is an alternative variant.
    pub alternative_of: Option<EntityId>,
    /// Renderer-neutral semantic content blocks.
    pub blocks: Vec<ContentBlock>,
}

/// Appends a new immutable revision and moves one logical Message head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct EditMessageCommand {
    /// Existing logical Message entity.
    pub message_id: EntityId,
    /// Replacement semantic content stored as a new immutable revision.
    pub blocks: Vec<ContentBlock>,
}

/// Chooses one existing message as the currently visible conversation branch leaf.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct SelectBranchCommand {
    /// Existing Conversation entity.
    pub conversation_id: EntityId,
    /// Existing Message entity that belongs to the conversation.
    pub message_id: EntityId,
}

/// Requests another response variant without naming any AI/provider implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RequestAlternativeCommand {
    /// Existing Conversation entity.
    pub conversation_id: EntityId,
    /// Existing message for which another branch/variant is requested.
    pub message_id: EntityId,
    /// Participant expected to author the future alternative.
    pub participant_id: EntityId,
}

/// Durable event emitted after a Conversation is created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ConversationCreatedEvent {
    /// Created Conversation entity.
    pub conversation_id: EntityId,
}

/// Durable event emitted after a Participant is added.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ParticipantAddedEvent {
    /// Owning Conversation entity.
    pub conversation_id: EntityId,
    /// Created Participant entity.
    pub participant_id: EntityId,
}

/// Durable event emitted after a logical Message and initial revision are created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MessageAddedEvent {
    /// Owning Conversation entity.
    pub conversation_id: EntityId,
    /// Created logical Message entity.
    pub message_id: EntityId,
    /// Initial immutable MessageRevision entity.
    pub revision_id: EntityId,
    /// Authoring Participant entity.
    pub participant_id: EntityId,
    /// Optional parent message.
    pub parent_message_id: Option<EntityId>,
    /// Optional message for which this one is an alternative.
    pub alternative_of: Option<EntityId>,
}

/// Durable event emitted when a new immutable revision becomes a Message head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MessageEditedEvent {
    /// Edited logical Message entity.
    pub message_id: EntityId,
    /// Previous immutable head revision.
    pub previous_revision_id: EntityId,
    /// New immutable head revision.
    pub revision_id: EntityId,
}

/// Durable event emitted after the selected visible branch changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct BranchSelectedEvent {
    /// Conversation whose selected leaf changed.
    pub conversation_id: EntityId,
    /// Newly selected leaf Message entity.
    pub message_id: EntityId,
}

/// Durable domain intent asking controllers/Narrator for another reply variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct AlternativeRequestedEvent {
    /// Conversation containing the source message.
    pub conversation_id: EntityId,
    /// Message for which another alternative was requested.
    pub message_id: EntityId,
    /// Participant expected to author the future variant.
    pub participant_id: EntityId,
}

pub(crate) fn validate_title(title: &Option<String>) -> Result<(), ChatModelError> {
    if let Some(title) = title {
        validate_optional_text("title", title, MAX_CHAT_TITLE_BYTES)?;
    }
    Ok(())
}

pub(crate) fn validate_participant_name(name: &str) -> Result<(), ChatModelError> {
    validate_required_text("display-name", name, MAX_PARTICIPANT_NAME_BYTES)
}

pub(crate) fn validate_blocks(blocks: &[ContentBlock]) -> Result<(), ChatModelError> {
    MessageContent {
        blocks: blocks.to_vec(),
        effective_at: None,
    }
    .validate()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IndexValidationError {
    TooLarge,
    Duplicate,
}

fn validate_unique_ids(ids: &[EntityId], maximum: usize) -> Result<(), IndexValidationError> {
    if ids.len() > maximum {
        return Err(IndexValidationError::TooLarge);
    }
    let unique = ids
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if unique.len() != ids.len() {
        return Err(IndexValidationError::Duplicate);
    }
    Ok(())
}

fn validate_required_text(
    field: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), ChatModelError> {
    if value.trim().is_empty() || value.len() > maximum {
        return Err(ChatModelError::InvalidText { field, maximum });
    }
    Ok(())
}

fn validate_optional_text(
    field: &'static str,
    value: &str,
    maximum: usize,
) -> Result<(), ChatModelError> {
    if value.len() > maximum {
        return Err(ChatModelError::InvalidText { field, maximum });
    }
    Ok(())
}
