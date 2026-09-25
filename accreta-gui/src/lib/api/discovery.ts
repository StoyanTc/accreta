import type { SchemaResponse, SelectItem, BucketLevel, TimeRange } from '$lib/api/schema';
import { query } from '$lib/api/client';

/**
 * Pick any (measure, aggregate) pair that's cheap and safe to use purely as a probe for
 * `group_by` discovery — the actual value returned is never used, only which `dimensions`
 * combinations come back. Avoids `tdigest` when anything else is available, since tdigest needs
 * a quantile and is more expensive for no benefit here.
 */
function pickProbeSelect(schema: SchemaResponse): SelectItem {
	for (const m of schema.measures) {
		const nonTdigest = m.aggregates.find((a) => a !== 'tdigest');
		if (nonTdigest) return { measure: m.name, aggregate: nonTdigest };
	}
	return { measure: schema.measures[0].name, aggregate: 'tdigest', quantile: 0.5 };
}

/**
 * Discover the distinct values a dimension has actually taken, *within a given level/time_range*
 * — deliberately the currently-selected view, not some separately invented "all time" range.
 *
 * Querying at a much coarser level or wider range than what's on screen risks landing on rollup
 * levels that haven't finished propagating yet (accreta-metrics' background sweep cascades one
 * hop at a time — see accreta-metrics.md notes on this being unresolved/unverified against
 * accreta's own source), which would make "discover values" silently miss things that are
 * visible in the chart right in front of the user. Reusing the same level/time_range the user
 * already has selected sidesteps that risk entirely, at the cost of only surfacing values that
 * appear in the current view (a reasonable trade-off — there's a "re-discover" affordance for
 * when the user changes the range).
 */
export async function discoverDimensionValues(
	schema: SchemaResponse,
	dimension: string,
	level: BucketLevel,
	timeRange: TimeRange
): Promise<string[]> {
	const response = await query({
		level,
		time_range: timeRange,
		group_by: [dimension],
		select: [pickProbeSelect(schema)]
	});

	const values = new Set<string>();
	for (const bucket of response.buckets) {
		for (const group of bucket.groups) {
			if (group.dimensions[0]) values.add(group.dimensions[0]);
		}
	}
	return [...values].sort();
}
