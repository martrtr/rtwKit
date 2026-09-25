//! Character Library controller over generic user-content reads.

use std::collections::BTreeSet;

use rintawa_sdk::ui::{UiActionEvent, UiActionPayload};

use crate::{
    CharacterLibraryError, CharacterLibraryGateway, CharacterLibraryState,
    MAX_CHARACTER_LIBRARY_ENTRIES, catalog::decode_document, ui::REFRESH_NODE,
};

/// Semantic action used by the explicit catalog refresh control.
pub const CHARACTER_LIBRARY_ACTION_REFRESH: &str = "rintawa.character-library.refresh";
/// Semantic action used by per-row selection controls.
pub const CHARACTER_LIBRARY_ACTION_SELECT: &str = "rintawa.character-library.select";

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
    /// The previous selection is preserved when that logical entry still exists.
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
            entries.push(decode_document(&record, document)?);
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

    /// Applies one already UI-runtime-validated semantic action to package state.
    ///
    /// Surface, revision, payload, and node ownership are checked again so alternate
    /// adapters cannot bypass the package's own stale/spoofed-action defenses.
    ///
    /// # Errors
    ///
    /// Returns action-validation, gateway, catalog-validation, or revision errors.
    pub fn handle_action(&mut self, event: &UiActionEvent) -> Result<(), CharacterLibraryError> {
        if event.surface_id.as_str() != crate::ui::CHARACTER_LIBRARY_SURFACE_ID {
            return Err(CharacterLibraryError::WrongSurface);
        }
        if event.surface_revision != self.state.revision {
            return Err(CharacterLibraryError::StaleAction);
        }
        if event.payload != UiActionPayload::None {
            return Err(CharacterLibraryError::InvalidActionPayload);
        }

        match event.action_id.as_str() {
            CHARACTER_LIBRARY_ACTION_REFRESH => {
                if event.node_id.as_str() != REFRESH_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                self.refresh()
            }
            CHARACTER_LIBRARY_ACTION_SELECT => {
                let index = self
                    .state
                    .entries
                    .iter()
                    .enumerate()
                    .find_map(|(index, _)| {
                        (crate::ui::entry_select_node_id(index) == event.node_id).then_some(index)
                    })
                    .ok_or(CharacterLibraryError::UnknownEntryAction)?;
                self.select_index(index)
            }
            _ => Err(CharacterLibraryError::UnknownAction),
        }
    }
}

fn next_revision(revision: u64) -> Result<u64, CharacterLibraryError> {
    revision
        .checked_add(1)
        .ok_or(CharacterLibraryError::RevisionOverflow)
}
