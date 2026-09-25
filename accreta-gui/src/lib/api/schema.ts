// Hand-written now, mirroring the `#[derive(ToSchema)]` structs in accreta-metrics's Rust
// source (auth.rs, schema_api.rs, ingest_api.rs, query_api.rs, error.rs, health.rs).
//
// TODO once accreta-metrics is actually running somewhere reachable: replace this file with
// generated output —
//
//   npx openapi-typescript http://<host>/api-docs/openapi.json -o src/lib/api/schema.ts
//
// — so these types can never drift from the server the way a hand-maintained copy eventually
// will. Everything downstream (client.ts, the query builder, every page) imports only from this
// file, specifically so that swap is a one-file change.

export interface ErrorBody {
	error: string;
	detail: string;
	field?: string;
}

export interface HealthResponse {
	status: 'ok';
	schema_present: boolean;
	last_rollup_sweep: string | null; // RFC3339, matches chrono::DateTime<Utc>'s serde format
}

export interface LoginRequest {
	username: string;
	password: string;
}

export interface LoginResponse {
	token: string;
	expires_at: string; // RFC3339
}

export type ValueType = 'f64' | 'i64' | 'u64';

export interface MeasureRequest {
	name: string;
	value_type: ValueType;
	aggregates: string[];
}

export interface SchemaRequest {
	name: string;
	dimensions: string[];
	measures: MeasureRequest[];
}

export interface MeasureResponse {
	name: string;
	value_type: ValueType;
	aggregates: string[];
}

export interface SchemaResponse {
	name: string;
	dimensions: string[];
	measures: MeasureResponse[];
}

export interface SampleRequest {
	ts: string; // RFC3339
	measures: (number | string)[]; // positional against SchemaResponse.measures
	dimensions: string[]; // positional against SchemaResponse.dimensions
}

export interface IngestRequest {
	samples: SampleRequest[];
}

export interface IngestResponse {
	ingested: number;
}

export type BucketLevel = 'second' | 'minute' | 'hour' | 'day' | 'week' | 'month' | 'year';

export interface TimeRange {
	start: string; // RFC3339
	end: string; // RFC3339
}

export interface SelectItem {
	measure: string;
	aggregate: string;
	quantile?: number; // required by the server iff aggregate === 'tdigest'
}

export interface QueryRequest {
	level: BucketLevel;
	time_range: TimeRange;
	filter?: Record<string, string[]>; // equality/IN only, per the v1 filter vocabulary
	group_by?: string[];
	select: SelectItem[];
}

export interface GroupResult {
	dimensions: string[]; // positional against QueryRequest.group_by
	values: (number | string | null)[]; // positional against QueryRequest.select
}

export interface BucketResult {
	bucket_start: string; // RFC3339 — buckets with no matching group are omitted, not zero-filled
	groups: GroupResult[];
}

export interface QueryResponse {
	buckets: BucketResult[];
}
