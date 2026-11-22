//! Bounded browser protocol and WASM execution target for installed web bundles.

pub mod http;
pub mod layout;
pub mod session;
#[cfg(target_arch = "wasm32")]
mod wasm;
pub mod wire;
