use std::sync::atomic::Ordering;

use tracing::warn;

use crate::protocol::TerminationReason;

use super::{OwnedProcessEnd, SessionInner, SessionRuntime};

impl SessionRuntime {
    pub(super) fn release_process_ownership(&self, end: OwnedProcessEnd) {
        if self.ownership_released.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(process) = self.owned_process.as_ref() else {
            return;
        };
        if let Err(error) = self.process_observer.process_ended(process, &end) {
            warn!(
                event = "session_process_observer_remove_failed",
                internal_session_id = self.internal_session_id,
                session_handle_prefix = self.handle_prefix(),
                error = %error,
            );
        }
    }
}

pub(super) fn process_termination_reason(inner: &SessionInner) -> TerminationReason {
    if inner.timed_out {
        TerminationReason::Timeout
    } else if inner.kill_requested || inner.force_killed {
        TerminationReason::Killed
    } else {
        TerminationReason::Exit
    }
}

pub(super) fn owned_process_end(inner: &SessionInner) -> Option<OwnedProcessEnd> {
    if !inner.owned_group_members.is_empty() {
        return None;
    }
    inner.reaped_exit_code.map(|exit_code| {
        OwnedProcessEnd::exited(
            exit_code,
            process_termination_reason(inner),
            inner.last_known_cwd.clone(),
        )
    })
}
