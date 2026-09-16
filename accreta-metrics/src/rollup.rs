//! Background sweep: periodically rolls up and prunes the tenant's engine, if one exists yet.
//!
//! Not triggered inline by request handlers (see design summary) — ingest only ever touches
//! `BucketLevel::Minute`; this task is what propagates that up through the hierarchy and (once a
//! retention policy is configured) prunes old buckets.

use std::sync::Arc;
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
}
