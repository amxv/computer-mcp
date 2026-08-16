use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;

use super::{SessionRuntime, reap_exit_code};

pub(super) async fn wait_for_session_exits_until(
    runtimes: &[Arc<SessionRuntime>],
    poll_interval: Duration,
    deadline: Instant,
) -> Result<usize> {
    let deadline = tokio::time::Instant::from_std(deadline);
    let first_tick = tokio::time::Instant::now() + poll_interval;
    let mut ticker = tokio::time::interval_at(first_tick, poll_interval);
    // Relative sleep-per-poll loops accumulate scheduler delay and can turn one
    // runtime-wide grace window into several effective windows under load.
    // Keep polling cadence anchored to absolute time and give the shared
    // deadline its own timer so missed ticks never extend shutdown grace.
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        let mut survivors = 0;
        for runtime in runtimes {
            let mut inner = runtime.inner.lock().await;
            if reap_exit_code(&mut inner)?.is_some() {
                runtime.release_process_ownership();
            } else {
                survivors += 1;
            }
        }
        if survivors == 0 || tokio::time::Instant::now() >= deadline {
            return Ok(survivors);
        }

        tokio::select! {
            _ = ticker.tick() => {}
            _ = tokio::time::sleep_until(deadline) => return Ok(survivors),
        }
    }
}
