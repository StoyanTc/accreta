//! Background sweep: periodically rolls up and prunes the tenant's engine, if one exists yet.
//!
//! Not triggered inline by request handlers (see design summary) — ingest only ever touches
//! `BucketLevel::Second`; this task is what propagates that up through the hierarchy and (once a
//! retention policy is configured, see `schema_api.rs`'s `retention` field) prunes old buckets.
//!
//! `accreta::Engine::rollup()` is incremental: each sweep only recomputes the coarser buckets
//! whose children actually changed since the last sweep, not every bucket at every level. That
//! also means pruning is safe on any level here, not just leaf levels (`Week`/`Year`) — a bucket's
//! contribution is durably folded into its parent by the time it's pruned, so a later sweep never
//! needs to recompute a coarser level from "whichever children happen to still be around". Keep
//! calling `rollup()` before `prune()` on each sweep regardless (as below): `accreta::Engine`
//! still refuses to discard a bucket that hasn't been rolled up yet as a backstop, but that's not
//! a substitute for the right order.

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
