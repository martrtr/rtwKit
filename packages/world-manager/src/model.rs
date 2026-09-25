//! World Manager catalog model and runtime-adapter boundary.

use rintawa_sdk::world::WorldId;
use thiserror::Error;

/// Maximum persistent worlds rendered by one World Manager catalog snapshot.
///
/// The limit is chosen so the worst-case row shape remains below Portable UI's
/// 4096-node validation bound with room for package chrome.
pub const MAX_WORLD_CATALOG_ENTRIES: usize = 512;
/// Maximum lifecycle diagnostic retained in one package catalog row.
pub const MAX_WORLD_SESSION_DIAGNOSTIC_BYTES: usize = 2 * 1024;

/// Transport-neutral mirror of one generic `world-sessions` catalog record.
///
/// A future WASM adapter maps the generated WIT record into this type without
/// exposing generated bindings to package domain/UI code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldSessionRecord {
    /// Canonical textual WorldId returned by the host capability.
    pub world_id: String,
    /// Last committed authoritative position.
    pub commit_position: u64,
    /// Whether an authoritative runtime is currently active.
    pub active: bool,
    /// Desired active state accepted but not yet applied by the host pump.
    pub pending_active: Option<bool>,
    /// Bounded diagnostic from the last failed lifecycle transition.
    pub last_error: Option<String>,
}

/// Stable, validated catalog entry used by World Manager domain/UI code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorldCatalogEntry {
    /// Stable authoritative world identity.
    pub world_id: WorldId,
    /// Last committed authoritative position.
    pub commit_position: u64,
    /// Whether the world currently owns a running runtime.
    pub active: bool,
    /// Desired active state waiting for the host pump, when any.
    pub pending_active: Option<bool>,
    /// Bounded failure from the previous lifecycle transition, when any.
    pub last_error: Option<String>,
}

/// Current World Manager presentation/domain state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorldManagerState {
    pub(crate) worlds: Vec<WorldCatalogEntry>,
    pub(crate) revision: u64,
}

impl WorldManagerState {
    /// Returns worlds in deterministic WorldId order.
    pub fn worlds(&self) -> &[WorldCatalogEntry] {
        &self.worlds
    }

    /// Returns the monotonic package presentation revision.
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

/// Typed failure surfaced by a runtime adapter for the generic `world-sessions` capability.
#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum WorldSessionGatewayError {
    /// Guest host access is outside an active callback scope.
    #[error("world-session host access is not active")]
    AccessNotActive,
    /// Exact component principal lacks the required runtime permission.
    #[error("world-session runtime permission denied")]
    PermissionDenied,
    /// Host rejected a malformed WorldId.
    #[error("invalid world id")]
    InvalidWorldId,
    /// Requested persistent world does not exist.
    #[error("world not found")]
    NotFound,
    /// Host lifecycle queue has no remaining distinct-world capacity.
    #[error("world-session lifecycle queue is full")]
    QueueFull,
    /// Current callback exceeded the host mutation budget.
    #[error("world-session mutation budget exceeded")]
    LimitExceeded,
    /// Host message bounds were exceeded.
    #[error("world-session message exceeds host bounds")]
    MessageTooLarge,
    /// Host policy/state rejected the operation.
    #[error("world-session operation rejected")]
    Rejected,
    /// Host capability is unavailable in the current runtime.
    #[error("world-session capability unavailable")]
    Unavailable,
}

/// Minimal package-facing gateway implemented by the future WASM runtime adapter.
pub trait WorldSessionGateway {
    /// Reads the persistent world catalog and runtime lifecycle status.
    fn list_worlds(&mut self) -> Result<Vec<WorldSessionRecord>, WorldSessionGatewayError>;

    /// Creates one empty persistent world and returns its host summary.
    fn create_world(&mut self) -> Result<WorldSessionRecord, WorldSessionGatewayError>;

    /// Requests active/inactive state; application is deferred to the host pump.
    fn set_active(&mut self, world_id: &str, active: bool) -> Result<(), WorldSessionGatewayError>;
}
