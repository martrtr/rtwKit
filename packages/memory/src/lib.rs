pub mod affect;
pub mod automatic;
#[cfg(target_arch = "wasm32")]
mod cycle_inference;
pub mod cycles;
pub mod extraction;
#[cfg(any(target_arch = "wasm32", test))]
mod extraction_schemas;
pub mod model;
#[cfg(target_arch = "wasm32")]
mod wasm;
