import { query } from '$lib/api/client';
import type { BucketLevel, QueryRequest, QueryResponse, SchemaResponse } from '$lib/api/schema';

/** One point in a renderable series. `value` is `null` for a bucket/group combination the
 *  server didn't return at all (see below) — charts must treat that as a gap, not a zero. */
export interface SeriesPoint {
	time: Date;
	value: number | null;
}

export interface Series {
	/** e.g. "payments / Europe" for group_by ["service","region"], or "(all)" for no group_by. */
	label: string;
	points: SeriesPoint[];
}

// Shared between LineChart, Sparkline, and KpiCard so a given series's color stays consistent
// wherever it's rendered — an at-most-a-handful-of-series palette rather than a computed scale.
export const SERIES_PALETTE = ['#5b6ee1', '#e15b8f', '#5be1c2', '#e1a05b', '#8f5be1', '#5be15b'];

// accreta only ever produces buckets aligned to these durations — this is what lets us generate
// the *expected* tick grid ourselves instead of trusting the (sparse) response to define it.
const LEVEL_DURATION_MS: Record<BucketLevel, number> = {
	second: 1_000,
	minute: 60_000,
	hour: 60 * 60_000,
	day: 24 * 60 * 60_000,
	week: 7 * 24 * 60 * 60_000,
	// Approximate on purpose: real month/year buckets aren't fixed-width, but this is only used
	// to generate a *display* tick grid, never to compute actual bucket boundaries server-side.
	month: 30 * 24 * 60 * 60_000,
	year: 365 * 24 * 60 * 60_000
};

function expectedTicks(request: QueryRequest, response: QueryResponse): Date[] {
	const start = new Date(request.time_range.start).getTime();
	const end = new Date(request.time_range.end).getTime();
	const step = LEVEL_DURATION_MS[request.level];

	// accreta aligns bucket_start to real minute/hour/day/etc boundaries server-side (e.g. always
	// :00 seconds for "minute"), but request.time_range.start is whatever arbitrary instant the
	// client asked for (e.g. Date.now() - 24h, down to the millisecond) — the two essentially
	// never coincide. Rather than guessing accreta's alignment rule from the client, anchor the
	// grid to one real bucket_start from the response and step outward from there by the level's
	// duration; every generated tick then lands exactly where a real bucket could be.
	const anchor = response.buckets[0] ? new Date(response.buckets[0].bucket_start).getTime() : start;

	let gridStart = anchor;
	while (gridStart > start) gridStart -= step;
	while (gridStart + step <= start) gridStart += step;

	const ticks: Date[] = [];
	for (let t = gridStart; t < end; t += step) {
		if (t >= start) ticks.push(new Date(t));
	}
	return ticks;
}

/**
 * Convert a QueryResponse into one Series per distinct group_by combination, with every expected
 * time bucket present (as a gap when the server omitted it) rather than only the buckets that
 * happened to have matching data.
 *
 * `selectIndex` picks which entry of each GroupResult.values array to plot — callers with a
 * single select item pass 0.
 */
export function toSeries(
	request: QueryRequest,
	response: QueryResponse,
	selectIndex: number
): Series[] {
	const ticks = expectedTicks(request, response);

	// bucket_start (ISO string) -> label -> value, so we can look up "what did group X have at
	// tick T" in O(1) while walking the tick grid below.
	const byBucketAndLabel = new Map<string, Map<string, number | null>>();
	const labelsSeen = new Set<string>();

	for (const bucket of response.buckets) {
		const perLabel = new Map<string, number | null>();
		for (const group of bucket.groups) {
			const label = group.dimensions.length > 0 ? group.dimensions.join(' / ') : '(all)';
			labelsSeen.add(label);
			const raw = group.values[selectIndex];
			perLabel.set(label, typeof raw === 'number' ? raw : null);
		}
		byBucketAndLabel.set(bucket.bucket_start, perLabel);
	}

	// No data at all yet (e.g. brand-new schema, nothing ingested): still return one empty-ish
	// series so the chart can render its "no data" state rather than nothing at all.
	const labels = labelsSeen.size > 0 ? [...labelsSeen] : ['(all)'];

	return labels.map((label) => ({
		label,
		points: ticks.map((time) => {
			// bucket_start values from the server won't line up character-for-character with our
			// generated tick's toISOString() in general (different but equal instants can format
			// differently) — compare by timestamp, not by string.
			const targetMs = time.getTime();
			let value: number | null = null;
			for (const [bucketStartIso, perLabel] of byBucketAndLabel) {
				if (new Date(bucketStartIso).getTime() === targetMs) {
					value = perLabel.get(label) ?? null;
					break;
				}
			}
			return { time, value };
		})
	}));
}

/**
 * Combine a numerator series set (e.g. sum) with a denominator series set (e.g. count) into a
 * derived per-point ratio — used for a client-side "mean", since accreta v0.2.0 removed the
 * Average aggregate as redundant with sum/count. Matches series by label and points by index;
 * both come from toSeries() calls against the *same* request/response pair (just different
 * select indices), so labels and tick grids are guaranteed to line up.
 */
export function divideSeries(numerator: Series[], denominator: Series[]): Series[] {
	const denomByLabel = new Map(denominator.map((s) => [s.label, s]));
	return numerator.map((numSeries) => {
		const denomSeries = denomByLabel.get(numSeries.label);
		return {
			label: numSeries.label,
			points: numSeries.points.map((p, i) => {
				const d = denomSeries?.points[i]?.value ?? null;
				const value = p.value !== null && d !== null && d !== 0 ? p.value / d : null;
				return { time: p.time, value };
			})
		};
	});
}

export type TimeRangePreset =
	| '15m'
	| '1h'
	| '6h'
	| '24h'
	| '7d'
	| '30d';

const PRESET_MS: Record<TimeRangePreset, number> = {
	'15m': 15 * 60_000,
	'1h': 60 * 60_000,
	'6h': 6 * 60 * 60_000,
	'24h': 24 * 60 * 60_000,
	'7d': 7 * 24 * 60 * 60_000,
	'30d': 30 * 24 * 60 * 60_000
};

export const TIME_RANGE_PRESETS: { value: TimeRangePreset; label: string }[] = [
	{ value: '15m', label: 'Last 15 minutes' },
	{ value: '1h', label: 'Last hour' },
	{ value: '6h', label: 'Last 6 hours' },
	{ value: '24h', label: 'Last 24 hours' },
	{ value: '7d', label: 'Last 7 days' },
	{ value: '30d', label: 'Last 30 days' }
];

// Only real BucketLevels are ever offered — there is no synthetic "5 minutes" option, since
// that isn't a level accreta actually rolls up to (see accreta-metrics.md notes on this).
export const RESOLUTIONS: { value: BucketLevel; label: string }[] = [
	{ value: 'second', label: '1 second' },
	{ value: 'minute', label: '1 minute' },
	{ value: 'hour', label: '1 hour' },
	{ value: 'day', label: '1 day' },
	{ value: 'week', label: '1 week' },
	{ value: 'month', label: '1 month' }
];

export function presetToTimeRange(preset: TimeRangePreset): { start: string; end: string } {
	const end = new Date();
	const start = new Date(end.getTime() - PRESET_MS[preset]);
	return { start: start.toISOString(), end: end.toISOString() };
}

/** The equal-length window immediately preceding the current preset's window — e.g. for "last
 *  24 hours" this is the 24 hours before that. Used for KPI card period-over-period deltas: two
 *  separate queries, diffed client-side, since accreta-metrics has no `compare_to` param. */
export function presetToPriorTimeRange(preset: TimeRangePreset): { start: string; end: string } {
	const currentStart = new Date(Date.now() - PRESET_MS[preset]);
	const priorStart = new Date(currentStart.getTime() - PRESET_MS[preset]);
	return { start: priorStart.toISOString(), end: currentStart.toISOString() };
}

/** Rough guard against firing a query whose bucket count would be unreasonable for a browser to
 *  render or accreta-metrics to compute in one pass (no server-side resource cap exists in v1 —
 *  see accreta-metrics.md notes). */
export function estimatedBucketCountForRange(rangeMs: number, level: BucketLevel): number {
	return Math.ceil(rangeMs / LEVEL_DURATION_MS[level]);
}

export function estimatedBucketCount(preset: TimeRangePreset, level: BucketLevel): number {
	return estimatedBucketCountForRange(PRESET_MS[preset], level);
}

export const MAX_REASONABLE_BUCKETS = 2000;

/**
 * A sensible default bucket level for a given time-range preset, so the Dashboard (unlike the
 * Explorer) never needs a manual resolution picker — it picks a resolution fine enough to show
 * real variation but coarse enough to stay well under MAX_REASONABLE_BUCKETS.
 */
export function defaultLevelForPreset(preset: TimeRangePreset): BucketLevel {
	switch (preset) {
		case '15m':
			return 'second';
		case '1h':
			return 'minute';
		case '6h':
			return 'minute';
		case '24h':
			return 'hour';
		case '7d':
			return 'hour';
		case '30d':
			return 'day';
	}
}

/**
 * Sum every non-null point across every series — valid for a `sum`-type aggregate (sum-of-sums
 * over a window equals the sum over the whole window), NOT valid for percentiles or averages,
 * which don't merge by simple addition. Returns null only when there's no data at all, so a
 * genuine zero-total window is distinguishable from "nothing came back".
 */
export function sumSeriesTotal(series: Series[]): number | null {
	let total = 0;
	let any = false;
	for (const s of series) {
		for (const p of s.points) {
			if (p.value !== null) {
				total += p.value;
				any = true;
			}
		}
	}
	return any ? total : null;
}
