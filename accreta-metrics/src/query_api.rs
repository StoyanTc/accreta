//! `POST /schema/query`.
//!
//! `accreta::Engine` doesn't offer quite what the design summary's query shape needs directly:
//!
//! - `Engine::query_range_grouped` groups by a `DimensionMask` but has **no equality-filter
//!   parameter at all** — there's nothing to hand it a `filter: {"region": ["us-east"]}` clause.
//! - It also operates on **one `MeasureId` at a time**, but a query's `select` list can name
//!   several different measures.
//! - `DimensionKey::project`'s output order follows `DimensionMask::iter()` (ascending
//!   `DimensionId`), which won't generally match the order the caller wrote `group_by` in — but
//!   the design summary's response contract is "`dimensions` matches the request's `group_by`
//!   order".
//!
//! So this handler doesn't call `query_range_grouped` at all. It scans `Engine::buckets(level)`
//! and each bucket's `Bucket::groups()` directly (both public), which hand back **full**
//! dimension keys — exactly what's needed to filter by value, and to project into the caller's
//! requested `group_by` order by hand, all measures at once per group.

use std::collections::HashMap;
use std::sync::Arc;

use accreta::bucket::BucketLevel;
use accreta::measures::MeasureId;
use accreta::AggregateSet;
use axum::extract::State;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::auth::TenantId;
use crate::dispatch;
use crate::error::ApiError;
use crate::state::{AppState, EngineState, SchemaMeta};

#[derive(Debug, Deserialize, ToSchema)]
pub struct TimeRange {
    pub start: String,
    pub end: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct SelectItem {
    pub measure: String,
    pub aggregate: String,
    #[serde(default)]
    pub quantile: Option<f64>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct QueryRequest {
    pub level: String,
    pub time_range: TimeRange,
    #[serde(default)]
    pub filter: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub group_by: Vec<String>,
    pub select: Vec<SelectItem>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct GroupResult {
    pub dimensions: Vec<String>,
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BucketResult {
    pub bucket_start: DateTime<Utc>,
    pub groups: Vec<GroupResult>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct QueryResponse {
    pub buckets: Vec<BucketResult>,
}

fn parse_level(s: &str) -> Result<BucketLevel, ApiError> {
    match s {
        "minute" => Ok(BucketLevel::Minute),
        "hour" => Ok(BucketLevel::Hour),
        "day" => Ok(BucketLevel::Day),
        "week" => Ok(BucketLevel::Week),
        "month" => Ok(BucketLevel::Month),
        "year" => Ok(BucketLevel::Year),
        other => Err(ApiError::validation(
            "invalid_level",
            format!("unknown level '{other}'"),
            "level",
        )),
    }
}

fn parse_ts(s: &str, field: &str) -> Result<DateTime<Utc>, ApiError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| {
            ApiError::validation(
                "invalid_timestamp",
                format!("could not parse '{s}' as an RFC3339 timestamp"),
                field,
            )
        })
}

/// Resolved, validated query — everything below this point is infallible index math.
struct Plan {
    level: BucketLevel,
    range_start: DateTime<Utc>,
    range_end: DateTime<Utc>,
    /// `(dimension_index, allowed_value_ids)`, one entry per filter clause.
    filters: Vec<(usize, Vec<u32>)>,
    /// Dimension indices in the exact order the caller wrote `group_by`.
    group_by_indices: Vec<usize>,
    /// `(MeasureId, value_type, aggregate name, quantile)`, one per `select` entry, in order.
    selects: Vec<(MeasureId, dispatch::ValueType, String, Option<f64>)>,
}

fn plan_query(
    req: &QueryRequest,
    meta: &SchemaMeta,
    dim_dicts: &[crate::state::DimDict],
) -> Result<Plan, ApiError> {
    let level = parse_level(&req.level)?;
    let range_start = parse_ts(&req.time_range.start, "time_range.start")?;
    let range_end = parse_ts(&req.time_range.end, "time_range.end")?;
    if range_start > range_end {
        return Err(ApiError::validation(
            "invalid_range",
            "time_range.start must not be after time_range.end",
            "time_range",
        ));
    }

    if req.select.is_empty() {
        return Err(ApiError::validation(
            "empty_select",
            "select must contain at least one entry",
            "select",
        ));
    }

    let dim_index = |name: &str| meta.dimensions.iter().position(|d| d == name);

    let mut filters = Vec::with_capacity(req.filter.len());
    for (dim_name, values) in &req.filter {
        let idx = dim_index(dim_name).ok_or_else(|| {
            ApiError::validation(
                "unknown_dimension",
                format!("'{dim_name}' is not a dimension on this schema"),
                "filter",
            )
        })?;
        // A value that was never ingested simply resolves to nothing — not an error, it just
        // means that literal can never match any stored group.
        let ids: Vec<u32> = values
            .iter()
            .filter_map(|v| dim_dicts[idx].get(v))
            .collect();
        filters.push((idx, ids));
    }

    let mut group_by_indices = Vec::with_capacity(req.group_by.len());
    for dim_name in &req.group_by {
        let idx = dim_index(dim_name).ok_or_else(|| {
            ApiError::validation(
                "unknown_dimension",
                format!("'{dim_name}' is not a dimension on this schema"),
                "group_by",
            )
        })?;
        group_by_indices.push(idx);
    }

    let mut selects = Vec::with_capacity(req.select.len());
    for (i, item) in req.select.iter().enumerate() {
        let (measure_idx, measure_meta) = meta
            .measures
            .iter()
            .enumerate()
            .find(|(_, m)| m.name == item.measure)
            .ok_or_else(|| {
                ApiError::validation(
                    "unknown_measure",
                    format!("'{}' is not a measure on this schema", item.measure),
                    format!("select[{i}].measure"),
                )
            })?;

        if !measure_meta.aggregates.iter().any(|a| a == &item.aggregate) {
            return Err(ApiError::validation(
                "unknown_aggregate",
                format!(
                    "'{}' is not registered for measure '{}'",
                    item.aggregate, item.measure
                ),
                format!("select[{i}].aggregate"),
            ));
        }

        if item.aggregate == "tdigest" {
            match item.quantile {
                Some(q) if (0.0..=1.0).contains(&q) => {}
                _ => {
                    return Err(ApiError::validation(
                        "invalid_quantile",
                        "tdigest requires a quantile in [0.0, 1.0]",
                        format!("select[{i}].quantile"),
                    ));
                }
            }
        }

        selects.push((
            MeasureId(measure_idx as u8),
            measure_meta.value_type,
            item.aggregate.clone(),
            item.quantile,
        ));
    }

    Ok(Plan {
        level,
        range_start,
        range_end,
        filters,
        group_by_indices,
        selects,
    })
}

fn run_query(engine_state: &EngineState, plan: &Plan) -> QueryResponse {
    let mut buckets_out = Vec::new();

    for bucket in engine_state.engine.buckets(plan.level) {
        // Same overlap test accreta's own Engine::query_range uses.
        if !(bucket.start() < plan.range_end && bucket.end() > plan.range_start) {
            continue;
        }

        // Keyed by the group_by values, in the caller's requested order — not accreta's
        // DimensionKey::project order, which follows ascending DimensionId instead (see module
        // docs for why that distinction matters here).
        let mut local_groups: HashMap<Vec<u32>, Vec<AggregateSet>> = HashMap::new();

        for (full_key, sets) in bucket.groups() {
            let values = full_key.values();

            let passes_filter = plan
                .filters
                .iter()
                .all(|(idx, allowed)| allowed.contains(&values[*idx]));
            if !passes_filter {
                continue;
            }

            let projected: Vec<u32> = plan
                .group_by_indices
                .iter()
                .map(|&idx| values[idx])
                .collect();

            local_groups
                .entry(projected)
                .and_modify(|acc| {
                    for (mine, theirs) in acc.iter_mut().zip(sets.iter()) {
                        mine.merge(theirs);
                    }
                })
                .or_insert_with(|| sets.clone());
        }

        if local_groups.is_empty() {
            // Sparse: a bucket with nothing matching the filter for this range is simply
            // omitted, per the design summary.
            continue;
        }

        let mut groups_out = Vec::with_capacity(local_groups.len());
        for (projected_ids, sets) in local_groups {
            let dimensions: Vec<String> = plan
                .group_by_indices
                .iter()
                .zip(projected_ids.iter())
                .map(|(&dim_idx, &value_id)| {
                    engine_state.dim_dicts[dim_idx]
                        .resolve(value_id)
                        .unwrap_or("<unknown>")
                        .to_owned()
                })
                .collect();

            let values: Vec<serde_json::Value> = plan
                .selects
                .iter()
                .map(|(measure_id, value_type, aggregate, quantile)| {
                    let set = &sets[measure_id.index()];
                    dispatch::extract_value(set, *value_type, aggregate, *quantile)
                        .unwrap_or(serde_json::Value::Null)
                })
                .collect();

            groups_out.push(GroupResult { dimensions, values });
        }

        buckets_out.push(BucketResult {
            bucket_start: bucket.start(),
            groups: groups_out,
        });
    }

    buckets_out.sort_by_key(|b| b.bucket_start);

    QueryResponse {
        buckets: buckets_out,
    }
}

#[utoipa::path(
    post,
    path = "/schema/query",
    request_body = QueryRequest,
    responses(
        (status = 200, description = "Query result", body = QueryResponse),
        (status = 400, description = "Validation error", body = crate::error::ErrorBody),
        (status = 401, description = "Unauthorized", body = crate::error::ErrorBody),
        (status = 404, description = "No schema yet", body = crate::error::ErrorBody),
    ),
    security(("bearer_auth" = [])),
    tag = "query"
)]
pub async fn query(
    State(state): State<Arc<AppState>>,
    tenant: TenantId,
    Json(req): Json<QueryRequest>,
) -> Result<Json<QueryResponse>, ApiError> {
    let record = state
        .credentials
        .get(&tenant.0)
        .ok_or_else(ApiError::unauthorized)?;

    let slot = record.schema.read().await;
    let engine_state_lock = slot.as_ref().ok_or_else(ApiError::no_schema)?.clone();
    drop(slot);

    let engine_state = engine_state_lock.read().await;
    let plan = plan_query(&req, &engine_state.meta, &engine_state.dim_dicts)?;
    let response = run_query(&engine_state, &plan);

    Ok(Json(response))
}
