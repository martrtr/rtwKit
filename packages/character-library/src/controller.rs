//! Character Library controller over generic user-content reads.

use std::collections::BTreeSet;

use rintawa_sdk::ui::{UiActionEvent, UiActionPayload};

use crate::{
    CharacterLibraryError, CharacterLibraryGateway, CharacterLibraryState,
    MAX_CHARACTER_IMPORT_TEXT_BYTES, MAX_CHARACTER_LIBRARY_ENTRIES,
    decode_character_content_document,
    ui::{IMPORT_SOURCE_NODE, INSTANTIATE_NODE, REFRESH_NODE},
};

/// Semantic action used by the explicit catalog refresh control.
pub const CHARACTER_LIBRARY_ACTION_REFRESH: &str = "rintawa.character-library.refresh";
/// Semantic action used by per-row selection controls.
pub const CHARACTER_LIBRARY_ACTION_SELECT: &str = "rintawa.character-library.select";
/// Semantic action used to instantiate the exact selected template into a new World.
pub const CHARACTER_LIBRARY_ACTION_INSTANTIATE: &str = "rintawa.character-library.instantiate";
/// Semantic action used to submit Tavern V2 JSON from the portable import editor.
pub const CHARACTER_LIBRARY_ACTION_IMPORT: &str = "rintawa.character-library.import-tavern-v2";

const MAX_IMPORT_STATUS_BYTES: usize = 2 * 1024;

/// Side effect requested by one already validated Character Library action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CharacterLibraryIntent {
    /// No Host-side operation is required after applying the action.
    None,
    /// Parse, encode, and defer publication of one Tavern V2 JSON document.
    ImportTavernJson {
        /// Exact bounded source submitted by the Portable UI field.
        source: String,
    },
    /// Create/open a new World and instantiate one exact immutable template revision.
    InstantiateSelected {
        /// Stable logical user-content identity.
        template_id: String,
        /// Exact immutable artifact revision selected by the user.
        template_revision: String,
    },
}

/// Stateful Character Library controller parameterized by a runtime adapter.
pub struct CharacterLibraryController<G> {
    gateway: G,
    state: CharacterLibraryState,
}

impl<G> CharacterLibraryController<G>
where
    G: CharacterLibraryGateway,
{
    /// Creates an empty controller. Call [`Self::refresh`] before first presentation.
    pub fn new(gateway: G) -> Self {
        Self {
            gateway,
            state: CharacterLibraryState::default(),
        }
    }

    /// Returns current validated package state.
    pub const fn state(&self) -> &CharacterLibraryState {
        &self.state
    }

    /// Returns the gateway for diagnostics, tests, or adapter-owned state inspection.
    pub const fn gateway(&self) -> &G {
        &self.gateway
    }

    /// Reloads and validates the exact CharacterTemplate catalog.
    ///
    /// Existing state is preserved if any replacement record/document is malformed.
    /// The previous selection and import presentation state are preserved when valid.
    ///
    /// # Errors
    ///
    /// Returns a gateway failure, package validation failure, catalog bound, or
    /// presentation revision overflow.
    pub fn refresh(&mut self) -> Result<(), CharacterLibraryError> {
        let revision = next_revision(self.state.revision)?;
        let records = self.gateway.list_templates()?;
        if records.len() > MAX_CHARACTER_LIBRARY_ENTRIES {
            return Err(CharacterLibraryError::CatalogTooLarge);
        }

        let mut seen = BTreeSet::new();
        let mut entries = Vec::with_capacity(records.len());
        for record in records {
            crate::catalog::validate_record(&record)?;
            if !seen.insert(record.id.clone()) {
                return Err(CharacterLibraryError::DuplicateEntryId);
            }
            let document = self.gateway.read_template(&record.id)?;
            entries.push(decode_character_content_document(&record, document)?);
        }
        entries.sort_by(|left, right| {
            left.template
                .name
                .cmp(&right.template.name)
                .then_with(|| left.id.cmp(&right.id))
        });

        let selected_id = self
            .state
            .selected_id
            .as_ref()
            .filter(|selected| entries.iter().any(|entry| &entry.id == *selected))
            .cloned()
            .or_else(|| entries.first().map(|entry| entry.id.clone()));
        self.state = CharacterLibraryState {
            entries,
            selected_id,
            revision,
            import_source: self.state.import_source.clone(),
            import_status: self.state.import_status.clone(),
            import_pending: self.state.import_pending,
        };
        Ok(())
    }

    /// Selects one current catalog entry by row index.
    ///
    /// # Errors
    ///
    /// Returns [`CharacterLibraryError::UnknownEntryAction`] for an out-of-range row
    /// or [`CharacterLibraryError::RevisionOverflow`] before mutating local state.
    pub fn select_index(&mut self, index: usize) -> Result<(), CharacterLibraryError> {
        let id = self
            .state
            .entries
            .get(index)
            .map(|entry| entry.id.clone())
            .ok_or(CharacterLibraryError::UnknownEntryAction)?;
        let revision = next_revision(self.state.revision)?;
        self.state.selected_id = Some(id);
        self.state.revision = revision;
        Ok(())
    }

    /// Marks the current deferred import as failed and keeps its source editable.
    ///
    /// # Errors
    ///
    /// Returns [`CharacterLibraryError::RevisionOverflow`] before changing state.
    pub fn fail_import(&mut self, diagnostic: &str) -> Result<(), CharacterLibraryError> {
        let revision = next_revision(self.state.revision)?;
        self.state.import_pending = false;
        self.state.import_status = Some(bounded_status(diagnostic));
        self.state.revision = revision;
        Ok(())
    }

    /// Refreshes the catalog after Host publication and selects the imported item.
    ///
    /// The retained source is cleared only after the imported logical item is visible
    /// in the validated catalog, so failed refreshes never discard user input.
    ///
    /// # Errors
    ///
    /// Returns refresh/validation errors, revision overflow, or
    /// [`CharacterLibraryError::ImportedEntryMissing`] when Host success cannot be
    /// reconciled with the subsequent exact catalog read.
    pub fn complete_import(&mut self, imported_id: &str) -> Result<(), CharacterLibraryError> {
        self.refresh()?;
        let imported = self
            .state
            .entries
            .iter()
            .find(|entry| entry.id == imported_id)
            .ok_or(CharacterLibraryError::ImportedEntryMissing)?;
        let name = imported.template.name.clone();
        self.state.selected_id = Some(imported.id.clone());
        self.state.import_source.clear();
        self.state.import_pending = false;
        self.state.import_status = Some(bounded_status(&format!("Imported {name}.")));
        Ok(())
    }

    /// Applies one already UI-runtime-validated semantic action to package state.
    ///
    /// Surface, revision, payload, and node ownership are checked again so alternate
    /// adapters cannot bypass the package's own stale/spoofed-action defenses.
    ///
    /// # Errors
    ///
    /// Returns action-validation, gateway, catalog-validation, import-bound, or
    /// revision errors.
    pub fn handle_action(
        &mut self,
        event: &UiActionEvent,
    ) -> Result<CharacterLibraryIntent, CharacterLibraryError> {
        if event.surface_id.as_str() != crate::ui::CHARACTER_LIBRARY_SURFACE_ID {
            return Err(CharacterLibraryError::WrongSurface);
        }
        if event.surface_revision != self.state.revision {
            return Err(CharacterLibraryError::StaleAction);
        }

        match event.action_id.as_str() {
            CHARACTER_LIBRARY_ACTION_REFRESH => {
                require_none_payload(event)?;
                if event.node_id.as_str() != REFRESH_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                self.refresh()?;
                Ok(CharacterLibraryIntent::None)
            }
            CHARACTER_LIBRARY_ACTION_SELECT => {
                require_none_payload(event)?;
                let index = self
                    .state
                    .entries
                    .iter()
                    .enumerate()
                    .find_map(|(index, _)| {
                        (crate::ui::entry_select_node_id(index) == event.node_id).then_some(index)
                    })
                    .ok_or(CharacterLibraryError::UnknownEntryAction)?;
                self.select_index(index)?;
                Ok(CharacterLibraryIntent::None)
            }
            CHARACTER_LIBRARY_ACTION_IMPORT => {
                if event.node_id.as_str() != IMPORT_SOURCE_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                if self.state.import_pending {
                    return Err(CharacterLibraryError::ImportAlreadyPending);
                }
                let UiActionPayload::Text(source) = &event.payload else {
                    return Err(CharacterLibraryError::InvalidActionPayload);
                };
                if source.trim().is_empty() {
                    return Err(CharacterLibraryError::EmptyImportSource);
                }
                if source.len() > MAX_CHARACTER_IMPORT_TEXT_BYTES {
                    return Err(CharacterLibraryError::ImportSourceTooLarge);
                }
                let revision = next_revision(self.state.revision)?;
                self.state.import_source = source.clone();
                self.state.import_status = Some(String::from("Importing Tavern V2 JSON…"));
                self.state.import_pending = true;
                self.state.revision = revision;
                Ok(CharacterLibraryIntent::ImportTavernJson {
                    source: source.clone(),
                })
            }
            CHARACTER_LIBRARY_ACTION_INSTANTIATE => {
                require_none_payload(event)?;
                if event.node_id.as_str() != INSTANTIATE_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                let entry = self
                    .state
                    .selected_entry()
                    .ok_or(CharacterLibraryError::UnknownEntryAction)?;
                Ok(CharacterLibraryIntent::InstantiateSelected {
                    template_id: entry.id.clone(),
                    template_revision: entry.revision.to_string(),
                })
            }
            _ => Err(CharacterLibraryError::UnknownAction),
        }
    }
}

fn require_none_payload(event: &UiActionEvent) -> Result<(), CharacterLibraryError> {
    if event.payload == UiActionPayload::None {
        Ok(())
    } else {
        Err(CharacterLibraryError::InvalidActionPayload)
    }
}

fn bounded_status(value: &str) -> String {
    if value.len() <= MAX_IMPORT_STATUS_BYTES {
        return value.to_string();
    }
    let mut end = MAX_IMPORT_STATUS_BYTES;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].to_string()
}

fn next_revision(revision: u64) -> Result<u64, CharacterLibraryError> {
    revision
        .checked_add(1)
        .ok_or(CharacterLibraryError::RevisionOverflow)
}
