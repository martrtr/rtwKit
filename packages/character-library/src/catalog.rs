//! Character Library catalog model over the generic user-content capability.

use rintawa_artifacts::ArtifactDigest;
use rintawa_sdk::content::MAX_CONTENT_HANDLER_ENTRY_BYTES;
use thiserror::Error;

use crate::{CHARACTER_TEMPLATE_CONTENT_V1, CharacterTemplate};

/// Maximum CharacterTemplate items loaded into one bounded library snapshot.
pub const MAX_CHARACTER_LIBRARY_ENTRIES: usize = 128;
/// Maximum byte length accepted for one opaque host-local user-content identity.
pub const MAX_CHARACTER_LIBRARY_ID_BYTES: usize = 128;
/// Maximum Tavern V2 JSON text accepted from one Portable UI submit action.
pub const MAX_CHARACTER_IMPORT_TEXT_BYTES: usize = 256 * 1024;

/// Transport-neutral mirror of one generic user-content list record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterContentRecord {
    /// Opaque stable logical user-content identity.
    pub id: String,
    /// Exact versioned RTW content type.
    pub content: String,
    /// Exact immutable artifact revision digest.
    pub revision: String,
}

/// Transport-neutral user-content document returned by a runtime adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterContentDocument {
    /// Metadata that must exactly match the list record selected by the package.
    pub metadata: CharacterContentRecord,
    /// Exact bounded CharacterTemplate root descriptor bytes.
    pub descriptor: Vec<u8>,
}

/// One validated CharacterTemplate entry in current library presentation state.
#[derive(Debug, Clone, PartialEq)]
pub struct CharacterLibraryEntry {
    /// Opaque stable logical user-content identity.
    pub id: String,
    /// Exact immutable revision used to decode this entry.
    pub revision: ArtifactDigest,
    /// Validated reusable CharacterTemplate content.
    pub template: CharacterTemplate,
}

/// Current validated Character Library state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CharacterLibraryState {
    pub(crate) entries: Vec<CharacterLibraryEntry>,
    pub(crate) selected_id: Option<String>,
    pub(crate) revision: u64,
    pub(crate) import_source: String,
    pub(crate) import_status: Option<String>,
    pub(crate) import_pending: bool,
}

impl CharacterLibraryState {
    /// Returns entries in deterministic display-name then logical-ID order.
    pub fn entries(&self) -> &[CharacterLibraryEntry] {
        &self.entries
    }

    /// Returns the currently selected entry when it still exists.
    pub fn selected_entry(&self) -> Option<&CharacterLibraryEntry> {
        let selected_id = self.selected_id.as_deref()?;
        self.entries.iter().find(|entry| entry.id == selected_id)
    }

    /// Returns the current selected opaque user-content identity.
    pub fn selected_id(&self) -> Option<&str> {
        self.selected_id.as_deref()
    }

    /// Returns the monotonic Portable UI revision owned by the package.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the Tavern V2 JSON retained for the current/past import attempt.
    pub fn import_source(&self) -> &str {
        &self.import_source
    }

    /// Returns the bounded user-facing status for the latest import attempt.
    pub fn import_status(&self) -> Option<&str> {
        self.import_status.as_deref()
    }

    /// Returns whether a deferred Host user-content write is awaiting completion.
    pub const fn import_pending(&self) -> bool {
        self.import_pending
    }
}

/// Typed failure surfaced by a runtime adapter for generic user-content reads.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum CharacterLibraryGatewayError {
    /// Guest host access is outside an active callback scope.
    #[error("user-content host access is not active")]
    AccessNotActive,
    /// Exact component principal lacks `user-content-read`.
    #[error("user-content runtime permission denied")]
    PermissionDenied,
    /// Host rejected a malformed logical user-content identity.
    #[error("invalid user-content id")]
    InvalidId,
    /// Host rejected the exact content type filter.
    #[error("invalid user-content type")]
    InvalidContent,
    /// Requested logical user-content entry no longer exists.
    #[error("user-content entry not found")]
    NotFound,
    /// Host deferred user-content write queue is full.
    #[error("user-content write queue is full")]
    QueueFull,
    /// Per-callback user-content write budget was exhausted.
    #[error("user-content write callback budget is exhausted")]
    LimitExceeded,
    /// Host message bounds were exceeded.
    #[error("user-content message exceeds host bounds")]
    MessageTooLarge,
    /// Host policy or persistent state rejected the read.
    #[error("user-content read rejected")]
    Rejected,
    /// Generic user-content access is unavailable in this runtime.
    #[error("user-content capability unavailable")]
    Unavailable,
}

/// Package-facing gateway implemented by the Character Library WASM adapter.
pub trait CharacterLibraryGateway {
    /// Lists exact CharacterTemplate content records from the generic host library.
    fn list_templates(
        &mut self,
    ) -> Result<Vec<CharacterContentRecord>, CharacterLibraryGatewayError>;

    /// Reads one exact CharacterTemplate root descriptor by opaque logical identity.
    fn read_template(
        &mut self,
        id: &str,
    ) -> Result<CharacterContentDocument, CharacterLibraryGatewayError>;
}

/// Character Library catalog/controller validation failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CharacterLibraryError {
    /// Generic host user-content access failed.
    #[error(transparent)]
    Gateway(#[from] CharacterLibraryGatewayError),
    /// Catalog exceeds the package's bounded presentation capacity.
    #[error("character library exceeds the package catalog bound")]
    CatalogTooLarge,
    /// Host returned an empty or oversized opaque logical user-content identity.
    #[error("invalid character library entry id")]
    InvalidEntryId,
    /// Host returned a non-CharacterTemplate item despite the exact type filter.
    #[error("character library received an unexpected content type")]
    UnexpectedContentType,
    /// Host returned the same logical identity more than once.
    #[error("character library returned a duplicate logical content id")]
    DuplicateEntryId,
    /// Host returned a malformed immutable artifact digest.
    #[error("character library returned an invalid revision digest")]
    InvalidRevision,
    /// Document metadata did not match the list record selected for reading.
    #[error("character library document metadata does not match its catalog record")]
    MetadataMismatch,
    /// Root descriptor exceeds the public content-handler protocol bound.
    #[error("character template descriptor exceeds the package bound")]
    DescriptorTooLarge,
    /// Root descriptor is not valid CharacterTemplate JSON.
    #[error("character template descriptor is malformed")]
    InvalidDescriptor,
    /// Decoded CharacterTemplate violates package-owned semantic invariants.
    #[error("character template descriptor violates package invariants")]
    InvalidTemplate,
    /// Local presentation revision cannot advance safely.
    #[error("character library presentation revision overflow")]
    RevisionOverflow,
    /// UI action targets another Portable UI surface.
    #[error("UI action does not target the Character Library surface")]
    WrongSurface,
    /// UI action was emitted from a stale rendered revision.
    #[error("stale Character Library action")]
    StaleAction,
    /// UI action carries an unsupported payload shape.
    #[error("Character Library action payload is invalid")]
    InvalidActionPayload,
    /// Tavern JSON submitted from Portable UI is empty.
    #[error("Character import source is empty")]
    EmptyImportSource,
    /// Tavern JSON submitted from Portable UI exceeds the package UI bound.
    #[error("Character import source exceeds the package UI bound")]
    ImportSourceTooLarge,
    /// A second import was submitted before the previous deferred write completed.
    #[error("Character import is already pending")]
    ImportAlreadyPending,
    /// Host reported import success but the refreshed catalog omitted that logical item.
    #[error("imported CharacterTemplate is missing from the refreshed catalog")]
    ImportedEntryMissing,
    /// A known semantic action was emitted from a node that does not own it.
    #[error("Character Library action was emitted from the wrong node")]
    WrongActionNode,
    /// Row selection does not correspond to the current catalog.
    #[error("Character Library action references an unknown catalog row")]
    UnknownEntryAction,
    /// Action identity is unknown to this package version.
    #[error("unknown Character Library action")]
    UnknownAction,
}

pub(crate) fn validate_record(
    record: &CharacterContentRecord,
) -> Result<ArtifactDigest, CharacterLibraryError> {
    if record.id.trim().is_empty() || record.id.len() > MAX_CHARACTER_LIBRARY_ID_BYTES {
        return Err(CharacterLibraryError::InvalidEntryId);
    }
    if record.content != CHARACTER_TEMPLATE_CONTENT_V1 {
        return Err(CharacterLibraryError::UnexpectedContentType);
    }
    record
        .revision
        .parse::<ArtifactDigest>()
        .map_err(|_| CharacterLibraryError::InvalidRevision)
}

/// Decodes one exact CharacterTemplate document after validating host metadata.
///
/// This is shared by the catalog controller and the World System runtime so both
/// paths enforce the same content type, immutable revision, descriptor bound, and
/// CharacterTemplate invariants.
///
/// # Errors
///
/// Returns [`CharacterLibraryError`] when metadata is inconsistent, the descriptor
/// is oversized/malformed, or the decoded CharacterTemplate is invalid.
pub fn decode_character_content_document(
    expected: &CharacterContentRecord,
    document: CharacterContentDocument,
) -> Result<CharacterLibraryEntry, CharacterLibraryError> {
    if document.metadata != *expected {
        return Err(CharacterLibraryError::MetadataMismatch);
    }
    if document.descriptor.len() > MAX_CONTENT_HANDLER_ENTRY_BYTES {
        return Err(CharacterLibraryError::DescriptorTooLarge);
    }
    let revision = validate_record(expected)?;
    let template = serde_json::from_slice::<CharacterTemplate>(&document.descriptor)
        .map_err(|_| CharacterLibraryError::InvalidDescriptor)?;
    template
        .validate()
        .map_err(|_| CharacterLibraryError::InvalidTemplate)?;
    Ok(CharacterLibraryEntry {
        id: expected.id.clone(),
        revision,
        template,
    })
}
