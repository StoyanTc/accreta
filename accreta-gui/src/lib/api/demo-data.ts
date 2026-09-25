import type { SchemaRequest, SchemaResponse, SampleRequest, ValueType } from '$lib/api/schema';

/** A ready-made schema matching the original mockup (service/region dimensions, a request-count
 *  measure, a latency measure with tdigest) — a one-click starting point for anyone who just
 *  wants to see the Dashboard/Explorer/Aggregates screens working before designing a real schema. */
export const DEMO_SCHEMA: SchemaRequest = {
	name: 'demo',
	dimensions: ['service', 'region'],
	measures: [
		{ name: 'requests', value_type: 'u64', aggregates: ['sum', 'count'] },
		{ name: 'latency_ms', value_type: 'f64', aggregates: ['sum', 'count', 'tdigest'] }
	]
};

const DEMO_DIMENSION_VALUES: Record<string, string[]> = {
	service: ['api', 'payments', 'auth', 'search'],
	region: ['eu', 'us']
};

function randomFor(valueType: ValueType, measureName: string): number {
	// Only special-cased for the demo schema's own measure names, so the numbers look plausible
	// in a screenshot — falls back to a generic range for any other schema (see below).
	if (measureName === 'latency_ms') return Math.round(40 + Math.random() * 300);
	if (measureName === 'requests') return Math.round(1 + Math.random() * 50);
	return valueType === 'f64' ? Math.random() * 1000 : Math.round(Math.random() * 1000);
}

/**
 * Generate synthetic samples for any schema (not just DEMO_SCHEMA), spread evenly across
 * `hoursBack` hours ending now, one sample every `intervalMinutes` per dimension combination.
 *
 * For a schema whose dimension names aren't in DEMO_DIMENSION_VALUES (i.e. a real user-authored
 * schema, not the demo one), each dimension gets 2 generic synthetic values ("service-a",
 * "service-b") — enough to exercise group-by/filter, without pretending to know what the values
 * should mean.
 */
export function generateSyntheticSamples(
	schema: SchemaResponse,
	opts: { hoursBack?: number; intervalMinutes?: number } = {}
): SampleRequest[] {
	const hoursBack = opts.hoursBack ?? 24;
	const intervalMinutes = opts.intervalMinutes ?? 5;

	const valuesPerDim = schema.dimensions.map(
		(dim) => DEMO_DIMENSION_VALUES[dim] ?? [`${dim}-a`, `${dim}-b`]
	);

	// Cartesian product of dimension values, so every combination gets its own consistent series
	// (otherwise group-by would show mostly-empty, randomly-assigned groups).
	let combinations: string[][] = [[]];
	for (const values of valuesPerDim) {
		combinations = combinations.flatMap((combo) => values.map((v) => [...combo, v]));
	}

	const samples: SampleRequest[] = [];
	const now = Date.now();
	const totalTicks = Math.floor((hoursBack * 60) / intervalMinutes);

	for (let tick = 0; tick <= totalTicks; tick++) {
		const ts = new Date(now - (totalTicks - tick) * intervalMinutes * 60_000).toISOString();
		for (const dimensions of combinations) {
			samples.push({
				ts,
				dimensions,
				measures: schema.measures.map((m) => randomFor(m.value_type, m.name))
			});
		}
	}

	return samples;
}

/** accreta-metrics validates and applies a whole ingest batch atomically, but there's no
 *  documented upper bound on batch size in v1 — chunking client-side keeps any single request
 *  reasonably sized regardless of how many dimension combinations/ticks a given schema produces. */
export function chunk<T>(items: T[], size: number): T[][] {
	const out: T[][] = [];
	for (let i = 0; i < items.length; i += size) out.push(items.slice(i, i + size));
	return out;
}
