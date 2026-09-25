//! World Manager controller over the generic world-session gateway.

use std::collections::BTreeSet;

use rintawa_sdk::ui::{UiActionEvent, UiActionPayload};
use rintawa_sdk::world::WorldId;
use thiserror::Error;

use crate::{
    MAX_WORLD_CATALOG_ENTRIES, MAX_WORLD_SESSION_DIAGNOSTIC_BYTES, WORLD_MANAGER_ACTION_CREATE,
    WORLD_MANAGER_ACTION_REFRESH, WORLD_MANAGER_ACTION_TOGGLE_ACTIVE, WORLD_MANAGER_SURFACE_ID,
    WorldCatalogEntry, WorldManagerState, WorldSessionGateway, WorldSessionGatewayError,
    WorldSessionRecord,
    ui::{CREATE_NODE, REFRESH_NODE},
    world_toggle_node_id,
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
    /// Action identity is unknown to this package version.
    #[error("unknown World Manager action `{0}`")]
    UnknownAction(String),
    /// Toggle node does not correspond to a current catalog entry.
    #[error("World Manager action references an unknown world row")]
    UnknownWorldAction,
    /// A lifecycle action was requested while an earlier transition is pending.
    #[error("world lifecycle transition is already pending")]
    LifecyclePending,
}

/// Result type used by World Manager controller operations.
pub type WorldManagerResult<T> = Result<T, WorldManagerError>;

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
        let worlds = validate_catalog(records)?;
        let revision = next_revision(self.state.revision)?;
        self.state = WorldManagerState { worlds, revision };
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
        worlds.sort_by_key(|world| world.world_id);
        self.state = WorldManagerState { worlds, revision };
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
        self.state = WorldManagerState { worlds, revision };
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
    pub fn handle_action(&mut self, event: &UiActionEvent) -> WorldManagerResult<()> {
        if event.surface_id.as_str() != WORLD_MANAGER_SURFACE_ID {
            return Err(WorldManagerError::WrongSurface);
        }
        if event.surface_revision != self.state.revision {
            return Err(WorldManagerError::StaleAction {
                expected: self.state.revision,
                actual: event.surface_revision,
            });
        }
        if event.payload != UiActionPayload::None {
            return Err(WorldManagerError::InvalidActionPayload);
        }

        match event.action_id.as_str() {
            WORLD_MANAGER_ACTION_REFRESH => {
                if event.node_id.as_str() != REFRESH_NODE {
                    return Err(WorldManagerError::WrongActionNode);
                }
                self.refresh()
            }
            WORLD_MANAGER_ACTION_CREATE => {
                if event.node_id.as_str() != CREATE_NODE {
                    return Err(WorldManagerError::WrongActionNode);
                }
                self.create_world().map(|_| ())
            }
            WORLD_MANAGER_ACTION_TOGGLE_ACTIVE => {
                let world = self
                    .state
                    .worlds
                    .iter()
                    .find(|world| world_toggle_node_id(world.world_id) == event.node_id)
                    .ok_or(WorldManagerError::UnknownWorldAction)?;
                if world.pending_active.is_some() {
                    return Err(WorldManagerError::LifecyclePending);
                }
                let world_id = world.world_id;
                let desired_active = !world.active;
                self.request_active(world_id, desired_active)
            }
            action => Err(WorldManagerError::UnknownAction(action.to_string())),
        }
    }
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
    if record
        .last_error
        .as_ref()
        .is_some_and(|diagnostic| diagnostic.len() > MAX_WORLD_SESSION_DIAGNOSTIC_BYTES)
    {
        return Err(WorldManagerError::DiagnosticTooLarge);
    }
    Ok(WorldCatalogEntry {
        world_id,
        commit_position: record.commit_position,
        active: record.active,
        pending_active: record.pending_active,
        last_error: record.last_error,
    })
}

fn next_revision(revision: u64) -> WorldManagerResult<u64> {
    revision
        .checked_add(1)
        .ok_or(WorldManagerError::RevisionOverflow)
}
