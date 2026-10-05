//! Cooperative scheduling for the baseline Character catalog refresh.

use std::cell::RefCell;

use crate::rintawa::engine::runtime_tasks;

const CATALOG_REFRESH_INTERVAL_MS: u32 = 50;

thread_local! {
    static TASK_HANDLE: RefCell<Option<u64>> = const { RefCell::new(None) };
}

/// Schedules one deferred catalog refresh after lifecycle start.
///
/// # Errors
///
/// Returns an error when the Host refuses the bounded task allocation.
pub(crate) fn schedule() -> Result<(), String> {
    stop();
    let handle = runtime_tasks::spawn_periodic(CATALOG_REFRESH_INTERVAL_MS)
        .map_err(|error| format!("Character catalog refresh task failed: {error:?}"))?;
    TASK_HANDLE.with(|slot| *slot.borrow_mut() = Some(handle));
    Ok(())
}

/// Cancels the deferred refresh task if it is still pending.
pub(crate) fn stop() {
    if let Some(handle) = TASK_HANDLE.with(|slot| slot.borrow_mut().take()) {
        let _ = runtime_tasks::cancel(handle);
    }
}

/// Returns whether this callback owns the one-shot refresh task.
///
/// Matching consumes and cancels the periodic handle before the caller performs
/// the potentially expensive catalog read, so refresh cannot overlap itself.
pub(crate) fn take(task_handle: u64) -> bool {
    let matches = TASK_HANDLE.with(|slot| {
        let mut slot = slot.borrow_mut();
        if *slot != Some(task_handle) {
            return false;
        }
        *slot = None;
        true
    });
    if matches {
        let _ = runtime_tasks::cancel(task_handle);
    }
    matches
}
