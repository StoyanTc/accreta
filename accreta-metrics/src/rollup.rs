//! Background sweep: periodically rolls up and prunes the tenant's engine, if one exists yet.
//!
//! Not triggered inline by request handlers (see design summary) — ingest only ever touches
//! `BucketLevel::Second`; this task is what propagates that up through the hierarchy and (once a
//! retention policy is configured) prunes old buckets.
//!
//! `rollup()` rebuilds every level above `Second` from the level below on each sweep, and the
//! sweep prunes *after* rolling up. That order is what keeps coarser levels complete: if a
//! retention policy pruning `Second` (or `Minute`, ...) is ever configured, the next sweep's
//! rollup would recompute the levels above it from only the surviving buckets. Make rollup
//! incremental before adding one.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration as StdDuration;

use crate::state::AppState;

const SWEEP_INTERVAL: StdDuration = StdDuration::from_secs(30);

pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SWEEP_INTERVAL);
        loop {
            interval.tick().await;
            sweep(&state).await;
        }
    });
}

async fn sweep(state: &Arc<AppState>) {
    for entry in state.credentials.iter() {
        let guard = entry.value().schema.read().await;
        let Some(engine_state) = guard.as_ref().cloned() else {
            continue;
        };
        drop(guard);

        let mut engine_state = engine_state.write().await;
        engine_state.engine.rollup();
        engine_state.engine.prune();
        tracing::debug!(tenant = %entry.key(), "rollup + prune swept");
    }

    state
        .last_rollup_sweep
        .store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
}
