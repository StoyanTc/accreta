import type { SchemaResponse, MeasureResponse } from '$lib/api/schema';

/**
 * Derives a small set of monitoring-dashboard "signals" from a schema — the same way a human
 * skimming a schema for the first time would guess what's worth watching, using only two kinds
 * of evidence:
 *   1. Which aggregates a measure registered (a `tdigest` measure is a distribution worth
 *      surfacing as percentiles, no matter what it's called).
 *   2. Naming conventions (a measure called anything error/fail-like is probably a failure
 *      counter; anything count/request/event-like is probably a volume counter).
 *
 * Nothing here references a specific measure name like "request_count" or "latency_ms" — those
 * only match because they satisfy the same generic patterns any similarly-named schema would.
 * A schema with none of these patterns simply produces fewer signals (see DerivedSignals'
 * optional fields) rather than guessing wrong.
 */

export interface DerivedSignal {
	measure: MeasureResponse;
	/** Human-readable label derived from the measure name, e.g. "request_count" -> "Requests". */
	label: string;
}

export interface DerivedSignals {
	/** The measure most likely to represent "how much traffic/activity happened". */
	volume?: DerivedSignal;
	/** The measure most likely to represent "how much of it failed". */
	errors?: DerivedSignal;
	/** Every measure that registered `tdigest` — each is its own percentile-worthy distribution,
	 *  however many there are (zero, one, or several). */
	distributions: DerivedSignal[];
}

const VOLUME_NAME = /count|requests?|events?|hits?|calls?|ops\b/i;
const ERROR_NAME = /error|fail|fault|exception/i;

function humanize(name: string): string {
	return name
		.replace(/_/g, ' ')
		.replace(/\b\w/g, (c) => c.toUpperCase())
		.trim();
}

export function deriveSignals(schema: SchemaResponse): DerivedSignals {
	const distributions = schema.measures
		.filter((m) => m.aggregates.includes('tdigest'))
		.map((m) => ({ measure: m, label: humanize(m.name) }));
	const distributionNames = new Set(distributions.map((d) => d.measure.name));

	// Volume/errors candidates are sum-having measures that aren't themselves a distribution —
	// a tdigest measure (e.g. latency) isn't "how much happened", it's "how long things took".
	const sumCandidates = schema.measures.filter(
		(m) => m.aggregates.includes('sum') && !distributionNames.has(m.name)
	);

	const errorsMeasure = sumCandidates.find((m) => ERROR_NAME.test(m.name));
	const volumeMeasure =
		sumCandidates.find((m) => VOLUME_NAME.test(m.name) && m !== errorsMeasure) ??
		sumCandidates.find((m) => m !== errorsMeasure);

	return {
		volume: volumeMeasure ? { measure: volumeMeasure, label: humanize(volumeMeasure.name) } : undefined,
		errors: errorsMeasure ? { measure: errorsMeasure, label: humanize(errorsMeasure.name) } : undefined,
		distributions
	};
}
