//! Deferred Tavern JSON import orchestration for the Character Library runtime.

use std::cell::RefCell;

use rintawa_character_library::{encode_character_template_rtw, import_tavern_v2};

use crate::rintawa::engine::{runtime_tasks, user_content};

const IMPORT_STATUS_POLL_INTERVAL_MS: u32 = 100;
const MAX_IMPORT_STATUS_POLLS: u16 = 100;

thread_local! {
    static PENDING_IMPORT: RefCell<Option<PendingImport>> = const { RefCell::new(None) };
}

struct PendingImport {
    operation_id: String,
    task_handle: u64,
    polls: u16,
}

/// Terminal/non-terminal result of one cooperative import status callback.
pub(crate) enum ImportPoll {
    /// Callback does not belong to the current import operation.
    Ignored,
    /// Host publication is still pending.
    Pending,
    /// Host published the exact content revision under this logical item id.
    Succeeded(String),
    /// Import terminated without publication/reconciliation.
    Failed(String),
}

/// Parses, packs, and queues one Tavern V2 JSON import without Host re-entry.
///
/// A polling task is allocated before write submission. Task callbacks cannot run
/// until the current guest callback unwinds, so accepted writes are observed only
/// after the Host has had an opportunity to process its deferred queue.
///
/// # Errors
///
/// Returns a bounded package/runtime diagnostic for malformed Tavern JSON, RTW
/// encoding failure, task scheduling failure, duplicate local import state, or
/// deferred Host write rejection.
pub(crate) fn begin(source: &str) -> Result<(), String> {
    if PENDING_IMPORT.with(|slot| slot.borrow().is_some()) {
        return Err(String::from("Character import is already pending"));
    }

    let template = import_tavern_v2(source.as_bytes())
        .map_err(|error| format!("Tavern V2 import failed: {error}"))?;
    let rtw = encode_character_template_rtw(&template)
        .map_err(|error| format!("CharacterTemplate RTW encoding failed: {error}"))?;
    let task_handle = runtime_tasks::spawn_periodic(IMPORT_STATUS_POLL_INTERVAL_MS)
        .map_err(|error| format!("Character import status task failed: {error:?}"))?;
    let accepted = match user_content::request_import(&rtw) {
        Ok(accepted) => accepted,
        Err(error) => {
            let _ = runtime_tasks::cancel(task_handle);
            return Err(format!("Character import submission failed: {error:?}"));
        }
    };
    PENDING_IMPORT.with(|slot| {
        *slot.borrow_mut() = Some(PendingImport {
            operation_id: accepted.operation_id,
            task_handle,
            polls: 0,
        });
    });
    Ok(())
}

/// Cancels any live import status task while preserving Host-owned write semantics.
pub(crate) fn stop() {
    if let Some(pending) = PENDING_IMPORT.with(|slot| slot.borrow_mut().take()) {
        let _ = runtime_tasks::cancel(pending.task_handle);
    }
}

/// Polls one owner-scoped deferred write status from a cooperative task callback.
pub(crate) fn poll(task_handle: u64) -> ImportPoll {
    let operation_id = PENDING_IMPORT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let pending = slot.as_mut()?;
        if pending.task_handle != task_handle {
            return None;
        }
        pending.polls = pending.polls.saturating_add(1);
        if pending.polls > MAX_IMPORT_STATUS_POLLS {
            return Some(Err(String::from(
                "Character import status timed out before Host completion",
            )));
        }
        Some(Ok(pending.operation_id.clone()))
    });

    let Some(operation_id) = operation_id else {
        return ImportPoll::Ignored;
    };
    let operation_id = match operation_id {
        Ok(operation_id) => operation_id,
        Err(error) => return finish_failed(task_handle, error),
    };

    match user_content::write_status(&operation_id) {
        Ok(user_content::WriteState::Pending) => ImportPoll::Pending,
        Ok(user_content::WriteState::Succeeded(entry)) => {
            finish_terminal(task_handle);
            ImportPoll::Succeeded(entry.id)
        }
        Ok(user_content::WriteState::Failed(diagnostic)) => finish_failed(
            task_handle,
            format!("Character import failed: {diagnostic}"),
        ),
        Err(error) => finish_failed(
            task_handle,
            format!("Character import status check failed: {error:?}"),
        ),
    }
}

fn finish_failed(task_handle: u64, diagnostic: String) -> ImportPoll {
    finish_terminal(task_handle);
    ImportPoll::Failed(diagnostic)
}

fn finish_terminal(task_handle: u64) {
    let pending = PENDING_IMPORT.with(|slot| slot.borrow_mut().take());
    if let Some(pending) = pending
        && pending.task_handle == task_handle
    {
        let _ = runtime_tasks::cancel(task_handle);
    }
}
