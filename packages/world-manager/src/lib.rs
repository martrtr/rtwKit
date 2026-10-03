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

pub use controller::{
    WorldManagerActionOutcome, WorldManagerController, WorldManagerError, WorldManagerResult,
};
pub use model::{
    MAX_WORLD_CATALOG_ENTRIES, MAX_WORLD_COVER_BYTES, MAX_WORLD_DESCRIPTION_BYTES,
    MAX_WORLD_SESSION_DIAGNOSTIC_BYTES, MAX_WORLD_TITLE_BYTES, WorldCatalogAssetRef,
    WorldCatalogEntry, WorldCreatorOption, WorldImportResourceRef, WorldManagerState,
    WorldSessionGateway, WorldSessionGatewayError, WorldSessionRecord, WorldSortColumn,
    WorldSortDirection,
};
pub use ui::{
    WORLD_MANAGER_ACTION_CREATE, WORLD_MANAGER_ACTION_DELETE, WORLD_MANAGER_ACTION_EDIT,
    WORLD_MANAGER_ACTION_IMPORT_RESOURCE, WORLD_MANAGER_ACTION_OPEN, WORLD_MANAGER_ACTION_REFRESH,
    WORLD_MANAGER_ACTION_RENAME, WORLD_MANAGER_ACTION_SELECT, WORLD_MANAGER_ACTION_SORT,
    WORLD_MANAGER_ACTION_TOGGLE_ACTIVE, WORLD_MANAGER_ACTION_TOGGLE_CREATE_MENU,
    WORLD_MANAGER_ACTION_UPDATE_COVER, WORLD_MANAGER_ACTION_UPDATE_DESCRIPTION,
    WORLD_MANAGER_ACTIVITY_ID, WORLD_MANAGER_SURFACE_ID, build_world_manager_snapshot,
    creator_import_node_id, world_delete_node_id, world_manager_surface_contribution,
    world_open_node_id, world_toggle_node_id,
};
