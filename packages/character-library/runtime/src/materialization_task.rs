//! Cooperative execution for UI-triggered Character materialization intents.

use std::{cell::RefCell, mem};

use rintawa_character_library::CharacterLibraryIntent;

use crate::{materialization, rintawa::engine::runtime_tasks};

const MATERIALIZATION_TASK_INTERVAL_MS: u32 = 50;

thread_local! {
    static PENDING: RefCell<Option<PendingMaterialization>> = const { RefCell::new(None) };
}

struct PendingMaterialization {
    task_handle: u64,
    state: MaterializationState,
}

enum MaterializationState {
    Queued(CharacterLibraryIntent),
    Prepared(Box<materialization::PreparedExistingMaterialization>),
    Encoded(materialization::EncodedExistingMaterialization),
    Processing,
}

/// Result of one cooperative materialization callback.
pub(crate) enum MaterializationPoll {
    /// Callback does not belong to the pending materialization task.
    Ignored,
    /// A bounded stage completed and another callback is required.
    Pending,
    /// The queued materialization completed successfully.
    Succeeded,
    /// The queued materialization failed with a bounded package diagnostic.
    Failed(String),
}

/// Queues one already validated Character materialization intent.
///
/// # Errors
///
/// Returns an error when another materialization is pending or the Host refuses
/// the bounded background task allocation.
pub(crate) fn begin(intent: CharacterLibraryIntent) -> Result<(), String> {
    if PENDING.with(|slot| slot.borrow().is_some()) {
        return Err(String::from("Character materialization is already pending"));
    }
    let task_handle = runtime_tasks::spawn_periodic(MATERIALIZATION_TASK_INTERVAL_MS)
        .map_err(|error| format!("Character materialization task failed: {error:?}"))?;
    PENDING.with(|slot| {
        *slot.borrow_mut() = Some(PendingMaterialization {
            task_handle,
            state: MaterializationState::Queued(intent),
        });
    });
    Ok(())
}

/// Cancels a pending materialization callback during runtime shutdown.
pub(crate) fn stop() {
    if let Some(pending) = PENDING.with(|slot| slot.borrow_mut().take()) {
        let _ = runtime_tasks::cancel(pending.task_handle);
    }
}

/// Advances one bounded materialization stage.
pub(crate) fn poll(task_handle: u64) -> MaterializationPoll {
    let state = PENDING.with(|slot| {
        let mut slot = slot.borrow_mut();
        let pending = slot.as_mut()?;
        if pending.task_handle != task_handle {
            return None;
        }
        Some(mem::replace(
            &mut pending.state,
            MaterializationState::Processing,
        ))
    });
    let Some(state) = state else {
        return MaterializationPoll::Ignored;
    };
    advance(task_handle, state)
}

fn advance(task_handle: u64, state: MaterializationState) -> MaterializationPoll {
    let result = match state {
        MaterializationState::Queued(CharacterLibraryIntent::InstantiateInWorld {
            world_id,
            template,
        }) => materialization::prepare_existing_materialization(world_id, template)
            .map(|prepared| MaterializationState::Prepared(Box::new(prepared)))
            .map(Some),
        MaterializationState::Prepared(prepared) => {
            materialization::encode_existing_materialization(*prepared)
                .map(MaterializationState::Encoded)
                .map(Some)
        }
        MaterializationState::Encoded(encoded) => {
            materialization::submit_existing_materialization(encoded).map(|()| None)
        }
        MaterializationState::Queued(intent) => {
            materialization::execute_intent(intent).map(|()| None)
        }
        MaterializationState::Processing => Err(String::from(
            "Character materialization was left in an in-flight state",
        )),
    };
    finish_stage(task_handle, result)
}

fn finish_stage(
    task_handle: u64,
    result: Result<Option<MaterializationState>, String>,
) -> MaterializationPoll {
    match result {
        Ok(Some(next)) => {
            PENDING.with(|slot| {
                if let Some(pending) = slot.borrow_mut().as_mut()
                    && pending.task_handle == task_handle
                {
                    pending.state = next;
                }
            });
            MaterializationPoll::Pending
        }
        Ok(None) => {
            finish_terminal(task_handle);
            MaterializationPoll::Succeeded
        }
        Err(error) => {
            finish_terminal(task_handle);
            MaterializationPoll::Failed(error)
        }
    }
}

fn finish_terminal(task_handle: u64) {
    let pending = PENDING.with(|slot| slot.borrow_mut().take());
    if pending.is_some_and(|pending| pending.task_handle == task_handle) {
        let _ = runtime_tasks::cancel(task_handle);
    }
}
