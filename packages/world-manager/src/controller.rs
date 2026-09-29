//! World Manager controller over the generic world-session gateway.

use std::{cmp::Ordering, collections::BTreeSet};

use rintawa_sdk::ui::{UiActionEvent, UiActionPayload};
use rintawa_sdk::world::WorldId;
use thiserror::Error;

use crate::{
    MAX_WORLD_CATALOG_ENTRIES, MAX_WORLD_DESCRIPTION_BYTES, MAX_WORLD_SESSION_DIAGNOSTIC_BYTES,
    MAX_WORLD_TITLE_BYTES, WORLD_MANAGER_ACTION_CREATE, WORLD_MANAGER_ACTION_DELETE,
    WORLD_MANAGER_ACTION_OPEN, WORLD_MANAGER_ACTION_REFRESH, WORLD_MANAGER_ACTION_RENAME,
    WORLD_MANAGER_ACTION_RENAME_DRAFT, WORLD_MANAGER_ACTION_SELECT, WORLD_MANAGER_ACTION_SORT,
    WORLD_MANAGER_ACTION_TOGGLE_ACTIVE, WORLD_MANAGER_SURFACE_ID, WorldCatalogEntry,
    WorldManagerState, WorldSessionGateway, WorldSessionGatewayError, WorldSessionRecord,
    WorldSortColumn, WorldSortDirection,
    ui::{CATALOG_GRID_NODE, CREATE_NODE, REFRESH_NODE, RENAME_INPUT_NODE, RENAME_SAVE_NODE},
    world_delete_node_id, world_open_node_id, world_toggle_node_id,
};

/// World Manager domain/controller failure.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorldManagerError {
    /// Generic host world-session capability failed.
    #[error(transparent)]
    Gateway(#[from] WorldSessionGatewayError),
    /// Host adapter supplied a non-canonical WorldId.
    #[error("world-session gateway returned invalid WorldId `{0}`")]
    InvalidWorldId(String),
    /// Host adapter supplied the same WorldId more than once.
    #[error("world-session gateway returned duplicate world `{0}`")]
    DuplicateWorld(WorldId),
    /// Catalog exceeds the package's bounded Portable UI capacity.
    #[error("world catalog exceeds the {MAX_WORLD_CATALOG_ENTRIES}-entry package bound")]
    CatalogTooLarge,
    /// Lifecycle diagnostic exceeds the package boundary.
    #[error("world-session diagnostic exceeds package bounds")]
    DiagnosticTooLarge,
    /// Human-facing World title is blank or above the package/protocol bound.
    #[error("world title must contain 1..={MAX_WORLD_TITLE_BYTES} UTF-8 bytes")]
    InvalidTitle,
    /// Human-facing World description is above the package/protocol bound.
    #[error("world description must contain at most {MAX_WORLD_DESCRIPTION_BYTES} UTF-8 bytes")]
    InvalidDescription,
    /// Local presentation revision cannot advance safely.
    #[error("world-manager presentation revision overflow")]
    RevisionOverflow,
    /// UI action targets another portable surface.
    #[error("UI action does not target the World Manager surface")]
    WrongSurface,
    /// UI action was emitted from a stale rendered revision.
    #[error("stale World Manager action: expected revision {expected}, got {actual}")]
    StaleAction {
        /// Current controller revision.
        expected: u64,
        /// Revision observed by the action event.
        actual: u64,
    },
    /// UI action carries an unsupported payload shape.
    #[error("World Manager action payload is invalid")]
    InvalidActionPayload,
    /// A known semantic action was emitted from a node that does not own it.
    #[error("World Manager action was emitted from the wrong node")]
    WrongActionNode,
    /// The host accepted Create World but the returned summary violated package invariants.
    #[error("world was created but the returned world summary is invalid")]
    CreateAcceptedButInvalidSummary,
    /// The host accepted metadata update but returned a malformed or mismatched summary.
    #[error("world metadata was updated but the returned world summary is invalid")]
    MetadataAcceptedButInvalidSummary,
    /// Action identity is unknown to this package version.
    #[error("unknown World Manager action `{0}`")]
    UnknownAction(String),
    /// Toggle node does not correspond to a current catalog entry.
    #[error("World Manager action references an unknown world row")]
    UnknownWorldAction,
    /// A lifecycle action was requested while an earlier transition is pending.
    #[error("world lifecycle transition is already pending")]
    LifecyclePending,
    /// Destructive deletion was requested while the World is active or transitioning.
    #[error("world must be stopped before it can be deleted")]
    DeleteRequiresStopped,
}

/// Result type used by World Manager controller operations.
pub type WorldManagerResult<T> = Result<T, WorldManagerError>;

/// Semantic side effect requested by one validated World Manager action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldManagerActionOutcome {
    /// No shell-level navigation is required.
    None,
    /// Bring this World into the caller's foreground presentation session.
    OpenWorld(WorldId),
}

/// Stateful World Manager controller parameterized by a runtime adapter.
pub struct WorldManagerController<G> {
    gateway: G,
    state: WorldManagerState,
}

impl<G> WorldManagerController<G>
where
    G: WorldSessionGateway,
{
    /// Creates an empty controller. Call [`Self::refresh`] before first presentation.
    pub fn new(gateway: G) -> Self {
        Self {
            gateway,
            state: WorldManagerState::default(),
        }
    }

    /// Returns current validated package state.
    pub const fn state(&self) -> &WorldManagerState {
        &self.state
    }

    /// Returns the gateway for diagnostics/tests or adapter-owned state inspection.
    pub const fn gateway(&self) -> &G {
        &self.gateway
    }

    /// Reloads the authoritative catalog without mutating host lifecycle state.
    ///
    /// # Errors
    ///
    /// Returns a gateway error or fail-closed package validation error. Existing
    /// state is preserved when the replacement catalog is invalid.
    pub fn refresh(&mut self) -> WorldManagerResult<()> {
        let records = self.gateway.list_worlds()?;
        let mut worlds = validate_catalog(records)?;
        sort_worlds(
            &mut worlds,
            self.state.sort_column,
            self.state.sort_direction,
        );
        let revision = next_revision(self.state.revision)?;
        let selected_world_id = self
            .state
            .selected_world_id
            .filter(|selected| worlds.iter().any(|world| world.world_id == *selected))
            .or_else(|| worlds.first().map(|world| world.world_id));
        let rename_draft = selected_world_id
            .and_then(|selected| worlds.iter().find(|world| world.world_id == selected))
            .map(|world| world.title.clone())
            .unwrap_or_default();
        self.state = WorldManagerState {
            worlds,
            selected_world_id,
            rename_draft,
            sort_column: self.state.sort_column,
            sort_direction: self.state.sort_direction,
            revision,
        };
        Ok(())
    }

    /// Creates one persistent world and inserts the returned authoritative summary.
    ///
    /// This does not perform a second catalog read, so a successful host mutation
    /// cannot be misreported as failed merely because a later refresh failed.
    ///
    /// # Errors
    ///
    /// Returns a gateway failure, malformed/duplicate returned record, catalog bound,
    /// or revision overflow. Existing state is preserved after validation failures.
    pub fn create_world(&mut self) -> WorldManagerResult<WorldId> {
        if self.state.worlds.len() >= MAX_WORLD_CATALOG_ENTRIES {
            return Err(WorldManagerError::CatalogTooLarge);
        }
        // Finish every fallible local precondition before asking the host to mutate.
        let revision = next_revision(self.state.revision)?;
        let record = self.gateway.create_world()?;
        let entry = validate_record(record)
            .map_err(|_| WorldManagerError::CreateAcceptedButInvalidSummary)?;
        if self
            .state
            .worlds
            .iter()
            .any(|world| world.world_id == entry.world_id)
        {
            return Err(WorldManagerError::CreateAcceptedButInvalidSummary);
        }
        let world_id = entry.world_id;
        let mut worlds = self.state.worlds.clone();
        worlds.push(entry);
        sort_worlds(
            &mut worlds,
            self.state.sort_column,
            self.state.sort_direction,
        );
        let rename_draft = worlds
            .iter()
            .find(|world| world.world_id == world_id)
            .map(|world| world.title.clone())
            .unwrap_or_default();
        self.state = WorldManagerState {
            worlds,
            selected_world_id: Some(world_id),
            rename_draft,
            sort_column: self.state.sort_column,
            sort_direction: self.state.sort_direction,
            revision,
        };
        Ok(world_id)
    }

    /// Requests the desired active state and updates local state to the accepted pending state.
    ///
    /// # Errors
    ///
    /// Returns [`WorldManagerError::UnknownWorldAction`] for a world absent from the
    /// current catalog, [`WorldManagerError::LifecyclePending`] while another request
    /// is pending, a gateway failure, or revision overflow.
    pub fn request_active(&mut self, world_id: WorldId, active: bool) -> WorldManagerResult<()> {
        let index = self
            .state
            .worlds
            .iter()
            .position(|world| world.world_id == world_id)
            .ok_or(WorldManagerError::UnknownWorldAction)?;
        if self.state.worlds[index].pending_active.is_some() {
            return Err(WorldManagerError::LifecyclePending);
        }
        // Do not report a local revision failure after the host already accepted the request.
        let revision = next_revision(self.state.revision)?;
        self.gateway.set_active(&world_id.to_string(), active)?;
        let mut worlds = self.state.worlds.clone();
        worlds[index].pending_active = Some(active);
        worlds[index].last_error = None;
        sort_worlds(
            &mut worlds,
            self.state.sort_column,
            self.state.sort_direction,
        );
        self.state = WorldManagerState {
            worlds,
            selected_world_id: self.state.selected_world_id,
            rename_draft: self.state.rename_draft.clone(),
            sort_column: self.state.sort_column,
            sort_direction: self.state.sort_direction,
            revision,
        };
        Ok(())
    }

    /// Selects one catalog row for contextual launcher actions.
    ///
    /// # Errors
    ///
    /// Returns [`WorldManagerError::UnknownWorldAction`] when the World is no longer
    /// present or [`WorldManagerError::RevisionOverflow`] when presentation cannot advance.
    pub fn select_world(&mut self, world_id: WorldId) -> WorldManagerResult<()> {
        if !self
            .state
            .worlds
            .iter()
            .any(|world| world.world_id == world_id)
        {
            return Err(WorldManagerError::UnknownWorldAction);
        }
        let title = self
            .state
            .worlds
            .iter()
            .find(|world| world.world_id == world_id)
            .map(|world| world.title.clone())
            .ok_or(WorldManagerError::UnknownWorldAction)?;
        self.state.selected_world_id = Some(world_id);
        self.state.rename_draft = title;
        self.state.revision = next_revision(self.state.revision)?;
        Ok(())
    }

    /// Changes catalog ordering from one validated data-grid column key.
    ///
    /// Selecting the active column toggles direction; selecting another column resets to ascending.
    pub fn sort_by(&mut self, column: WorldSortColumn) -> WorldManagerResult<()> {
        let revision = next_revision(self.state.revision)?;
        let direction = if self.state.sort_column == column {
            self.state.sort_direction.toggled()
        } else {
            WorldSortDirection::Ascending
        };
        self.state.sort_column = column;
        self.state.sort_direction = direction;
        sort_worlds(&mut self.state.worlds, column, direction);
        self.state.revision = revision;
        Ok(())
    }

    /// Updates the launcher-local title draft without mutating host metadata.
    pub fn update_rename_draft(&mut self, title: String) -> WorldManagerResult<()> {
        if title.len() > MAX_WORLD_TITLE_BYTES {
            return Err(WorldManagerError::InvalidTitle);
        }
        self.state.rename_draft = title;
        self.state.revision = next_revision(self.state.revision)?;
        Ok(())
    }

    /// Persists a new title for the selected World and applies the authoritative summary.
    pub fn rename_selected_world(&mut self, title: String) -> WorldManagerResult<()> {
        let title = validate_title(&title)?.to_string();
        let world_id = self
            .state
            .selected_world_id
            .ok_or(WorldManagerError::UnknownWorldAction)?;
        let current = self
            .state
            .worlds
            .iter()
            .find(|world| world.world_id == world_id)
            .ok_or(WorldManagerError::UnknownWorldAction)?;
        let revision = next_revision(self.state.revision)?;
        let returned = self.gateway.set_metadata(
            &world_id.to_string(),
            &title,
            current.description.as_deref(),
            current.cover.clone(),
        )?;
        let entry = validate_record(returned)
            .map_err(|_| WorldManagerError::MetadataAcceptedButInvalidSummary)?;
        if entry.world_id != world_id {
            return Err(WorldManagerError::MetadataAcceptedButInvalidSummary);
        }
        let mut worlds = self.state.worlds.clone();
        let index = worlds
            .iter()
            .position(|world| world.world_id == world_id)
            .ok_or(WorldManagerError::UnknownWorldAction)?;
        worlds[index] = entry;
        sort_worlds(
            &mut worlds,
            self.state.sort_column,
            self.state.sort_direction,
        );
        self.state = WorldManagerState {
            worlds,
            selected_world_id: Some(world_id),
            rename_draft: title,
            sort_column: self.state.sort_column,
            sort_direction: self.state.sort_direction,
            revision,
        };
        Ok(())
    }

    /// Deletes the selected stopped World and moves selection to a deterministic fallback.
    pub fn delete_selected_world(&mut self) -> WorldManagerResult<()> {
        let world_id = self
            .state
            .selected_world_id
            .ok_or(WorldManagerError::UnknownWorldAction)?;
        let index = self
            .state
            .worlds
            .iter()
            .position(|world| world.world_id == world_id)
            .ok_or(WorldManagerError::UnknownWorldAction)?;
        let world = &self.state.worlds[index];
        if world.active || world.pending_active.is_some() {
            return Err(WorldManagerError::DeleteRequiresStopped);
        }
        let revision = next_revision(self.state.revision)?;
        self.gateway.delete_world(&world_id.to_string())?;
        let mut worlds = self.state.worlds.clone();
        worlds.remove(index);
        let selected_world_id = if worlds.is_empty() {
            None
        } else {
            worlds
                .get(index.min(worlds.len() - 1))
                .map(|world| world.world_id)
        };
        let rename_draft = selected_world_id
            .and_then(|selected| worlds.iter().find(|world| world.world_id == selected))
            .map(|world| world.title.clone())
            .unwrap_or_default();
        self.state = WorldManagerState {
            worlds,
            selected_world_id,
            rename_draft,
            sort_column: self.state.sort_column,
            sort_direction: self.state.sort_direction,
            revision,
        };
        Ok(())
    }

    fn advance_presentation_revision(&mut self) -> WorldManagerResult<()> {
        self.state.revision = next_revision(self.state.revision)?;
        Ok(())
    }

    /// Applies one already UI-runtime-validated semantic action to package state.
    ///
    /// The controller still checks surface, revision, payload, and row ownership so
    /// alternate adapters cannot bypass Portable UI's normal dispatch validation.
    ///
    /// # Errors
    ///
    /// Returns action-validation, gateway, catalog-validation, or lifecycle errors.
    pub fn handle_action(
        &mut self,
        event: &UiActionEvent,
    ) -> WorldManagerResult<WorldManagerActionOutcome> {
        if event.surface_id.as_str() != WORLD_MANAGER_SURFACE_ID {
            return Err(WorldManagerError::WrongSurface);
        }
        if event.surface_revision != self.state.revision {
            return Err(WorldManagerError::StaleAction {
                expected: self.state.revision,
                actual: event.surface_revision,
            });
        }
        match event.action_id.as_str() {
            WORLD_MANAGER_ACTION_REFRESH => {
                require_no_payload(event)?;
                if event.node_id.as_str() != REFRESH_NODE {
                    return Err(WorldManagerError::WrongActionNode);
                }
                self.refresh()?;
                Ok(WorldManagerActionOutcome::None)
            }
            WORLD_MANAGER_ACTION_CREATE => {
                require_no_payload(event)?;
                if event.node_id.as_str() != CREATE_NODE {
                    return Err(WorldManagerError::WrongActionNode);
                }
                self.create_world()?;
                Ok(WorldManagerActionOutcome::None)
            }
            WORLD_MANAGER_ACTION_SELECT => {
                if event.node_id.as_str() != CATALOG_GRID_NODE {
                    return Err(WorldManagerError::WrongActionNode);
                }
                let UiActionPayload::Text(world_id_text) = &event.payload else {
                    return Err(WorldManagerError::InvalidActionPayload);
                };
                let world_id = self
                    .state
                    .worlds
                    .iter()
                    .find(|world| world.world_id.to_string() == *world_id_text)
                    .map(|world| world.world_id)
                    .ok_or(WorldManagerError::UnknownWorldAction)?;
                self.select_world(world_id)?;
                Ok(WorldManagerActionOutcome::None)
            }
            WORLD_MANAGER_ACTION_SORT => {
                if event.node_id.as_str() != CATALOG_GRID_NODE {
                    return Err(WorldManagerError::WrongActionNode);
                }
                let UiActionPayload::Text(column_key) = &event.payload else {
                    return Err(WorldManagerError::InvalidActionPayload);
                };
                let column = WorldSortColumn::from_key(column_key)
                    .ok_or(WorldManagerError::InvalidActionPayload)?;
                self.sort_by(column)?;
                Ok(WorldManagerActionOutcome::None)
            }
            WORLD_MANAGER_ACTION_RENAME_DRAFT => {
                if event.node_id.as_str() != RENAME_INPUT_NODE {
                    return Err(WorldManagerError::WrongActionNode);
                }
                let UiActionPayload::Text(title) = &event.payload else {
                    return Err(WorldManagerError::InvalidActionPayload);
                };
                self.update_rename_draft(title.clone())?;
                Ok(WorldManagerActionOutcome::None)
            }
            WORLD_MANAGER_ACTION_RENAME => {
                let title = match event.node_id.as_str() {
                    RENAME_INPUT_NODE => {
                        let UiActionPayload::Text(title) = &event.payload else {
                            return Err(WorldManagerError::InvalidActionPayload);
                        };
                        title.clone()
                    }
                    RENAME_SAVE_NODE => {
                        require_no_payload(event)?;
                        self.state.rename_draft.clone()
                    }
                    _ => return Err(WorldManagerError::WrongActionNode),
                };
                self.rename_selected_world(title)?;
                Ok(WorldManagerActionOutcome::None)
            }
            WORLD_MANAGER_ACTION_DELETE => {
                require_no_payload(event)?;
                let world = self
                    .state
                    .selected_world()
                    .ok_or(WorldManagerError::UnknownWorldAction)?;
                if world_delete_node_id(world.world_id) != event.node_id {
                    return Err(WorldManagerError::WrongActionNode);
                }
                self.delete_selected_world()?;
                Ok(WorldManagerActionOutcome::None)
            }
            WORLD_MANAGER_ACTION_OPEN => {
                require_no_payload(event)?;
                let world = self
                    .state
                    .worlds
                    .iter()
                    .find(|world| {
                        self.state.selected_world_id == Some(world.world_id)
                            && world_open_node_id(world.world_id) == event.node_id
                    })
                    .ok_or(WorldManagerError::UnknownWorldAction)?;
                if world.pending_active == Some(false) {
                    return Err(WorldManagerError::LifecyclePending);
                }
                let world_id = world.world_id;
                let should_start = !world.active && world.pending_active.is_none();
                if should_start {
                    self.request_active(world_id, true)?;
                } else {
                    self.advance_presentation_revision()?;
                }
                Ok(WorldManagerActionOutcome::OpenWorld(world_id))
            }
            WORLD_MANAGER_ACTION_TOGGLE_ACTIVE => {
                require_no_payload(event)?;
                let world = self
                    .state
                    .worlds
                    .iter()
                    .find(|world| {
                        self.state.selected_world_id == Some(world.world_id)
                            && world_toggle_node_id(world.world_id) == event.node_id
                    })
                    .ok_or(WorldManagerError::UnknownWorldAction)?;
                if world.pending_active.is_some() {
                    return Err(WorldManagerError::LifecyclePending);
                }
                let world_id = world.world_id;
                let desired_active = !world.active;
                self.request_active(world_id, desired_active)?;
                Ok(WorldManagerActionOutcome::None)
            }
            action => Err(WorldManagerError::UnknownAction(action.to_string())),
        }
    }
}

fn sort_worlds(
    worlds: &mut [WorldCatalogEntry],
    column: WorldSortColumn,
    direction: WorldSortDirection,
) {
    worlds.sort_by(|left, right| {
        let ordering = match column {
            WorldSortColumn::Title => left
                .title
                .to_lowercase()
                .cmp(&right.title.to_lowercase())
                .then_with(|| left.world_id.cmp(&right.world_id)),
            WorldSortColumn::Status => world_status_rank(left)
                .cmp(&world_status_rank(right))
                .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
                .then_with(|| left.world_id.cmp(&right.world_id)),
            WorldSortColumn::Diagnostic => left
                .last_error
                .as_deref()
                .unwrap_or("")
                .to_lowercase()
                .cmp(&right.last_error.as_deref().unwrap_or("").to_lowercase())
                .then_with(|| left.title.to_lowercase().cmp(&right.title.to_lowercase()))
                .then_with(|| left.world_id.cmp(&right.world_id)),
        };
        match direction {
            WorldSortDirection::Ascending => ordering,
            WorldSortDirection::Descending => reverse_ordering(ordering),
        }
    });
}

fn world_status_rank(world: &WorldCatalogEntry) -> u8 {
    match world.pending_active {
        Some(false) => 0,
        None if !world.active => 1,
        Some(true) => 2,
        None => 3,
    }
}

fn reverse_ordering(ordering: Ordering) -> Ordering {
    ordering.reverse()
}

fn validate_catalog(
    records: Vec<WorldSessionRecord>,
) -> WorldManagerResult<Vec<WorldCatalogEntry>> {
    if records.len() > MAX_WORLD_CATALOG_ENTRIES {
        return Err(WorldManagerError::CatalogTooLarge);
    }
    let mut seen = BTreeSet::new();
    let mut worlds = Vec::with_capacity(records.len());
    for record in records {
        let entry = validate_record(record)?;
        if !seen.insert(entry.world_id) {
            return Err(WorldManagerError::DuplicateWorld(entry.world_id));
        }
        worlds.push(entry);
    }
    worlds.sort_by_key(|world| world.world_id);
    Ok(worlds)
}

fn validate_record(record: WorldSessionRecord) -> WorldManagerResult<WorldCatalogEntry> {
    let world_id_text = record.world_id;
    let world_id = world_id_text
        .parse::<WorldId>()
        .map_err(|_| WorldManagerError::InvalidWorldId(world_id_text.clone()))?;
    if world_id.to_string() != world_id_text {
        return Err(WorldManagerError::InvalidWorldId(world_id_text));
    }
    let title = validate_title(&record.title)?.to_string();
    let description = validate_description(record.description)?;
    if record
        .last_error
        .as_ref()
        .is_some_and(|diagnostic| diagnostic.len() > MAX_WORLD_SESSION_DIAGNOSTIC_BYTES)
    {
        return Err(WorldManagerError::DiagnosticTooLarge);
    }
    Ok(WorldCatalogEntry {
        world_id,
        title,
        description,
        cover: record.cover,
        commit_position: record.commit_position,
        active: record.active,
        pending_active: record.pending_active,
        last_error: record.last_error,
    })
}

fn validate_description(description: Option<String>) -> WorldManagerResult<Option<String>> {
    let Some(description) = description else {
        return Ok(None);
    };
    let description = description.trim().to_string();
    if description.is_empty() {
        return Ok(None);
    }
    if description.len() > MAX_WORLD_DESCRIPTION_BYTES {
        return Err(WorldManagerError::InvalidDescription);
    }
    Ok(Some(description))
}

fn validate_title(title: &str) -> WorldManagerResult<&str> {
    let title = title.trim();
    if title.is_empty() || title.len() > MAX_WORLD_TITLE_BYTES {
        return Err(WorldManagerError::InvalidTitle);
    }
    Ok(title)
}

fn require_no_payload(event: &UiActionEvent) -> WorldManagerResult<()> {
    if event.payload == UiActionPayload::None {
        Ok(())
    } else {
        Err(WorldManagerError::InvalidActionPayload)
    }
}

fn next_revision(revision: u64) -> WorldManagerResult<u64> {
    revision
        .checked_add(1)
        .ok_or(WorldManagerError::RevisionOverflow)
}
