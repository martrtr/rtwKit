//! Character Library controller over generic user-content reads.

use std::collections::BTreeSet;

use rintawa_sdk::ui::{UiActionEvent, UiActionPayload};

use crate::{
    CharacterLibraryError, CharacterLibraryGateway, CharacterLibraryState, CharacterWorldSummary,
    MAX_CHARACTER_CAST_ENTRIES, MAX_CHARACTER_IMPORT_TEXT_BYTES, MAX_CHARACTER_LIBRARY_ENTRIES,
    MAX_CHARACTER_SEARCH_BYTES, decode_character_content_document,
    ui::{
        ADD_TO_WORLD_NODE, CAST_CREATE_NODE, IMPORT_SOURCE_NODE, IMPORT_TOGGLE_NODE, REFRESH_NODE,
        SEARCH_NODE, TARGET_WORLD_NODE, cast_member_toggle_node_id, entry_cast_toggle_node_id,
    },
};

/// Semantic action used by the explicit catalog refresh control.
pub const CHARACTER_LIBRARY_ACTION_REFRESH: &str = "rintawa.character-library.refresh";
/// Semantic action used by per-row selection controls.
pub const CHARACTER_LIBRARY_ACTION_SELECT: &str = "rintawa.character-library.select";
/// Semantic action used to instantiate the exact selected template into a new World.
pub const CHARACTER_LIBRARY_ACTION_INSTANTIATE: &str = "rintawa.character-library.instantiate";
/// Semantic action adding/removing one reusable template from the new-World cast.
pub const CHARACTER_LIBRARY_ACTION_TOGGLE_CAST: &str = "rintawa.character-library.toggle-cast";
/// Semantic action used to submit Tavern V2 JSON from the portable import editor.
pub const CHARACTER_LIBRARY_ACTION_IMPORT: &str = "rintawa.character-library.import-tavern-v2";
/// Semantic action updating the bounded Character Gateway search query.
pub const CHARACTER_LIBRARY_ACTION_SEARCH: &str = "rintawa.character-library.search";
/// Semantic action opening or closing the advanced Tavern import panel.
pub const CHARACTER_LIBRARY_ACTION_TOGGLE_IMPORT: &str = "rintawa.character-library.toggle-import";
/// Semantic action selecting the persistent World edited by this management section.
pub const CHARACTER_LIBRARY_ACTION_SELECT_WORLD: &str = "rintawa.character-library.select-world";
/// Semantic action materializing the selected template into one existing World.
pub const CHARACTER_LIBRARY_ACTION_ADD_TO_WORLD: &str = "rintawa.character-library.add-to-world";

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
    /// Create one new World and instantiate the exact selected immutable cast.
    InstantiateCast {
        /// Exact immutable templates selected by the user, in deterministic cast order.
        templates: Vec<CharacterCastSelection>,
    },
    /// Materialize one reusable CharacterTemplate into an existing persistent World.
    InstantiateInWorld {
        /// Canonical target World identity selected from the current Host catalog.
        world_id: String,
        /// Exact immutable template revision selected by the user.
        template: CharacterCastSelection,
    },
}

/// One exact reusable template revision selected for World materialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterCastSelection {
    /// Stable logical user-content identity.
    pub template_id: String,
    /// Exact immutable artifact revision selected by the user.
    pub template_revision: String,
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

        let cast_ids = self
            .state
            .cast_ids
            .iter()
            .filter(|id| entries.iter().any(|entry| &entry.id == *id))
            .cloned()
            .collect::<Vec<_>>();
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
            cast_ids,
            revision,
            import_source: self.state.import_source.clone(),
            import_status: self.state.import_status.clone(),
            import_pending: self.state.import_pending,
            search_query: self.state.search_query.clone(),
            import_open: self.state.import_open,
            worlds: self.state.worlds.clone(),
            target_world_id: self.state.target_world_id.clone(),
        };
        Ok(())
    }

    /// Reconciles the Host-owned persistent World catalog used by management controls.
    ///
    /// # Errors
    ///
    /// Returns [`CharacterLibraryError::RevisionOverflow`] before mutating state.
    pub fn replace_worlds(
        &mut self,
        mut worlds: Vec<CharacterWorldSummary>,
    ) -> Result<(), CharacterLibraryError> {
        worlds.sort_by(|left, right| {
            left.title
                .to_lowercase()
                .cmp(&right.title.to_lowercase())
                .then_with(|| left.world_id.cmp(&right.world_id))
        });
        worlds.dedup_by(|left, right| left.world_id == right.world_id);
        let target_world_id = self
            .state
            .target_world_id
            .as_ref()
            .filter(|target| worlds.iter().any(|world| &world.world_id == *target))
            .cloned();
        if self.state.worlds == worlds && self.state.target_world_id == target_world_id {
            return Ok(());
        }
        let revision = next_revision(self.state.revision)?;
        self.state.worlds = worlds;
        self.state.target_world_id = target_world_id;
        self.state.revision = revision;
        Ok(())
    }

    /// Selects one existing persistent World as the Character management target.
    ///
    /// # Errors
    ///
    /// Returns [`CharacterLibraryError::UnknownWorldAction`] for a foreign identity.
    pub fn select_world(&mut self, scope_id: &str) -> Result<(), CharacterLibraryError> {
        let target_world_id = if scope_id == "host" {
            None
        } else {
            let world_id = scope_id
                .strip_prefix("world:")
                .ok_or(CharacterLibraryError::UnknownWorldAction)?;
            if !self
                .state
                .worlds
                .iter()
                .any(|world| world.world_id == world_id)
            {
                return Err(CharacterLibraryError::UnknownWorldAction);
            }
            Some(world_id.to_string())
        };
        if self.state.target_world_id == target_world_id {
            return Ok(());
        }
        let revision = next_revision(self.state.revision)?;
        self.state.target_world_id = target_world_id;
        self.state.revision = revision;
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

    /// Adds or removes one current CharacterTemplate from the pending new-World cast.
    pub fn toggle_cast_index(&mut self, index: usize) -> Result<(), CharacterLibraryError> {
        let id = self
            .state
            .entries
            .get(index)
            .map(|entry| entry.id.clone())
            .ok_or(CharacterLibraryError::UnknownEntryAction)?;
        let revision = next_revision(self.state.revision)?;
        if let Some(position) = self
            .state
            .cast_ids
            .iter()
            .position(|candidate| candidate == &id)
        {
            self.state.cast_ids.remove(position);
        } else {
            if self.state.cast_ids.len() >= MAX_CHARACTER_CAST_ENTRIES {
                return Err(CharacterLibraryError::CastTooLarge);
            }
            self.state.cast_ids.push(id);
        }
        self.state.revision = revision;
        Ok(())
    }

    /// Updates the package-owned Character Gateway search query.
    pub fn update_search_query(&mut self, query: String) -> Result<(), CharacterLibraryError> {
        if query.len() > MAX_CHARACTER_SEARCH_BYTES {
            return Err(CharacterLibraryError::SearchQueryTooLarge);
        }
        let revision = next_revision(self.state.revision)?;
        let normalized = query.trim().to_lowercase();
        let current_matches = self.state.selected_id.as_ref().is_some_and(|selected| {
            self.state
                .entries
                .iter()
                .find(|entry| &entry.id == selected)
                .is_some_and(|entry| search_matches_entry(entry, &normalized))
        });
        if !current_matches {
            self.state.selected_id = self
                .state
                .entries
                .iter()
                .find(|entry| search_matches_entry(entry, &normalized))
                .map(|entry| entry.id.clone());
        }
        self.state.search_query = query;
        self.state.revision = revision;
        Ok(())
    }

    /// Toggles the advanced Tavern import panel without affecting catalog state.
    pub fn toggle_import_panel(&mut self) -> Result<(), CharacterLibraryError> {
        let revision = next_revision(self.state.revision)?;
        self.state.import_open = !self.state.import_open;
        self.state.revision = revision;
        Ok(())
    }

    /// Marks the current deferred import as failed while retaining the source internally.
    ///
    /// Portable UI intentionally does not mirror the potentially large source back into
    /// presentation state; a renderer may accept a fresh retry payload instead.
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
        self.state.import_open = false;
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
            CHARACTER_LIBRARY_ACTION_SEARCH => {
                if event.node_id.as_str() != SEARCH_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                let UiActionPayload::Text(query) = &event.payload else {
                    return Err(CharacterLibraryError::InvalidActionPayload);
                };
                self.update_search_query(query.clone())?;
                Ok(CharacterLibraryIntent::None)
            }
            CHARACTER_LIBRARY_ACTION_SELECT_WORLD => {
                if event.node_id.as_str() != TARGET_WORLD_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                let UiActionPayload::Text(world_id) = &event.payload else {
                    return Err(CharacterLibraryError::InvalidActionPayload);
                };
                self.select_world(world_id)?;
                Ok(CharacterLibraryIntent::None)
            }
            CHARACTER_LIBRARY_ACTION_ADD_TO_WORLD => {
                require_none_payload(event)?;
                if event.node_id.as_str() != ADD_TO_WORLD_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                let world_id = self
                    .state
                    .target_world_id
                    .clone()
                    .ok_or(CharacterLibraryError::MissingTargetWorld)?;
                let target_world = self
                    .state
                    .worlds
                    .iter()
                    .find(|world| world.world_id == world_id)
                    .ok_or(CharacterLibraryError::UnknownWorldAction)?;
                if !target_world.active {
                    return Err(CharacterLibraryError::TargetWorldInactive);
                }
                let entry = self
                    .state
                    .selected_entry()
                    .ok_or(CharacterLibraryError::UnknownEntryAction)?;
                Ok(CharacterLibraryIntent::InstantiateInWorld {
                    world_id,
                    template: CharacterCastSelection {
                        template_id: entry.id.clone(),
                        template_revision: entry.revision.to_string(),
                    },
                })
            }
            CHARACTER_LIBRARY_ACTION_TOGGLE_IMPORT => {
                require_none_payload(event)?;
                if event.node_id.as_str() != IMPORT_TOGGLE_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                self.toggle_import_panel()?;
                Ok(CharacterLibraryIntent::None)
            }
            CHARACTER_LIBRARY_ACTION_IMPORT => {
                if event.node_id.as_str() != IMPORT_SOURCE_NODE || !self.state.import_open {
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
            CHARACTER_LIBRARY_ACTION_TOGGLE_CAST => {
                require_none_payload(event)?;
                let catalog_index = self
                    .state
                    .entries
                    .iter()
                    .enumerate()
                    .find_map(|(index, _)| {
                        (entry_cast_toggle_node_id(index) == event.node_id).then_some(index)
                    })
                    .or_else(|| {
                        self.state
                            .cast_ids
                            .iter()
                            .enumerate()
                            .find_map(|(cast_index, id)| {
                                (cast_member_toggle_node_id(cast_index) == event.node_id)
                                    .then_some(id)
                            })
                            .and_then(|id| {
                                self.state.entries.iter().position(|entry| &entry.id == id)
                            })
                    })
                    .ok_or(CharacterLibraryError::UnknownEntryAction)?;
                self.toggle_cast_index(catalog_index)?;
                Ok(CharacterLibraryIntent::None)
            }
            CHARACTER_LIBRARY_ACTION_INSTANTIATE => {
                require_none_payload(event)?;
                if event.node_id.as_str() != CAST_CREATE_NODE {
                    return Err(CharacterLibraryError::WrongActionNode);
                }
                if self.state.cast_ids.is_empty() {
                    return Err(CharacterLibraryError::EmptyCast);
                }
                let templates = self
                    .state
                    .cast_ids
                    .iter()
                    .map(|id| {
                        let entry = self
                            .state
                            .entries
                            .iter()
                            .find(|entry| &entry.id == id)
                            .ok_or(CharacterLibraryError::UnknownEntryAction)?;
                        Ok(CharacterCastSelection {
                            template_id: entry.id.clone(),
                            template_revision: entry.revision.to_string(),
                        })
                    })
                    .collect::<Result<Vec<_>, CharacterLibraryError>>()?;
                Ok(CharacterLibraryIntent::InstantiateCast { templates })
            }
            _ => Err(CharacterLibraryError::UnknownAction),
        }
    }
}

fn search_matches_entry(entry: &crate::CharacterLibraryEntry, normalized: &str) -> bool {
    normalized.is_empty()
        || entry.template.name.to_lowercase().contains(normalized)
        || entry
            .template
            .description
            .to_lowercase()
            .contains(normalized)
        || entry
            .template
            .metadata
            .creator
            .to_lowercase()
            .contains(normalized)
        || entry
            .template
            .metadata
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(normalized))
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
