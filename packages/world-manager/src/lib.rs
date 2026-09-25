//! Standard World Manager domain and Portable UI package for Rintawa.
//!
//! This crate intentionally lives outside the Core workspace. Core owns durable
//! world identity/storage/runtime authority; this package owns user-facing catalog
//! policy for the generic `world-sessions` capability.

#![forbid(unsafe_code)]
#![warn(missing_docs, rustdoc::broken_intra_doc_links)]

mod controller;
mod model;
mod ui;

pub use controller::{WorldManagerController, WorldManagerError, WorldManagerResult};
pub use model::{
    MAX_WORLD_CATALOG_ENTRIES, MAX_WORLD_SESSION_DIAGNOSTIC_BYTES, WorldCatalogEntry,
    WorldManagerState, WorldSessionGateway, WorldSessionGatewayError, WorldSessionRecord,
};
pub use ui::{
    WORLD_MANAGER_ACTION_CREATE, WORLD_MANAGER_ACTION_REFRESH, WORLD_MANAGER_ACTION_TOGGLE_ACTIVE,
    WORLD_MANAGER_ACTIVITY_ID, WORLD_MANAGER_SURFACE_ID, build_world_manager_snapshot,
    world_manager_surface_contribution, world_toggle_node_id,
};
