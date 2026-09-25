<script lang="ts">
	import { onMount } from 'svelte';
	import { getSchema, query, ApiError } from '$lib/api/client';
	import type { SchemaResponse, SelectItem } from '$lib/api/schema';
	import {
		TIME_RANGE_PRESETS,
		presetToTimeRange,
		presetToPriorTimeRange,
		defaultLevelForPreset,
		toSeries,
		divideSeries,
		sumSeriesTotal,
		SERIES_PALETTE,
		type TimeRangePreset,
		type Series,
		type SeriesPoint
	} from '$lib/api/query-helpers';
	import { deriveSignals, type DerivedSignal, type DerivedSignals } from '$lib/api/signals';
	import LineChart from '$lib/charts/LineChart.svelte';
	import KpiCard from '$lib/components/KpiCard.svelte';
	import BreakdownPanel from '$lib/components/BreakdownPanel.svelte';
	import CreateSchemaForm from '$lib/components/CreateSchemaForm.svelte';

	let schema = $state<SchemaResponse | null>(null);
	let schemaError = $state<string | null>(null);
	let needsSchemaCreation = $state(false);

	// Just two controls now — no manual metric/resolution/group-by picking. This is deliberately
	// the difference from Explorer: Dashboard shows a derived, opinionated overview; Explorer is
	// where you build an arbitrary query by hand.
	let preset = $state<TimeRangePreset>('1h');
	let autoRefresh = $state(true);
	let level = $derived(defaultLevelForPreset(preset));

	// Drill-down: clicking a breakdown row pins that dimension=value as a filter. Global to the
	// whole page, same reasoning as before — accreta's dimensions are flat, not a declared
	// hierarchy, so this is "narrow to exactly this slice", not a multi-level nav tree.
	let filters = $state<Record<string, string[]>>({});
	let breadcrumb = $state<string[]>([]);
	let pinnedDimensions = $derived(Object.keys(filters).filter((d) => filters[d].length > 0));

	let signals = $derived<DerivedSignals | null>(schema ? deriveSignals(schema) : null);

	const fmtCount = (v: number) =>
		new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(v);
	const fmtPct = (v: number) => `${v.toFixed(2)}%`;
	const fmtMs = (v: number) => `${v.toFixed(0)} ms`;

	// --- Volume + Errors + Error Rate ---
	let volumeSeries = $state<Series[]>([]);
	let volumeCurrent = $state<number | null>(null);
	let volumePrevious = $state<number | null>(null);
	let errorRateSeries = $state<Series[]>([]);
	let errorRateCurrent = $state<number | null>(null);
	let errorRatePrevious = $state<number | null>(null);
	let volumeError = $state<string | null>(null);

	async function runVolumeAndErrors() {
		const s = signals;
		if (!s?.volume) {
			volumeSeries = [];
			volumeCurrent = null;
			volumePrevious = null;
			errorRateSeries = [];
			errorRateCurrent = null;
			errorRatePrevious = null;
			return;
		}

		const select: SelectItem[] = [{ measure: s.volume.measure.name, aggregate: 'sum' }];
		if (s.errors) select.push({ measure: s.errors.measure.name, aggregate: 'sum' });

		const request = {
			level,
			time_range: presetToTimeRange(preset),
			filter: filters,
			group_by: [] as string[],
			select
		};

		try {
			const response = await query(request);
			volumeSeries = toSeries(request, response, 0);
			volumeCurrent = sumSeriesTotal(volumeSeries);
			volumeError = null;

			if (s.errors) {
				const errorsSeries = toSeries(request, response, 1);
				const errorsTotal = sumSeriesTotal(errorsSeries);
				errorRateCurrent = volumeCurrent && errorsTotal !== null ? (errorsTotal / volumeCurrent) * 100 : null;
				errorRateSeries = divideSeries(errorsSeries, volumeSeries).map((ser) => ({
					label: ser.label,
					points: ser.points.map((p) => ({ time: p.time, value: p.value !== null ? p.value * 100 : null }))
				}));
			}

			// Prior-window totals for the KPI deltas — best-effort, same reasoning as everywhere else.
			try {
				const priorRequest = { ...request, time_range: presetToPriorTimeRange(preset) };
				const priorResponse = await query(priorRequest);
				volumePrevious = sumSeriesTotal(toSeries(priorRequest, priorResponse, 0));
				if (s.errors) {
					const priorErrorsTotal = sumSeriesTotal(toSeries(priorRequest, priorResponse, 1));
					errorRatePrevious =
						volumePrevious && priorErrorsTotal !== null ? (priorErrorsTotal / volumePrevious) * 100 : null;
				}
			} catch {
				volumePrevious = null;
				errorRatePrevious = null;
			}
		} catch (e) {
			volumeError = e instanceof Error ? e.message : 'Query failed';
			volumeSeries = [];
			volumeCurrent = null;
			errorRateSeries = [];
			errorRateCurrent = null;
		}
	}

	// --- Distributions (one block per tdigest-registered measure) ---
	interface DistributionResult {
		key: string;
		label: string;
		avgCurrent: number | null;
		avgPrevious: number | null;
		avgPoints: SeriesPoint[];
		p95Current: number | null;
		p95Previous: number | null;
		chartSeries: Series[];
		error: string | null;
	}
	let distributionResults = $state<DistributionResult[]>([]);

	function latestValue(series: Series[]): number | null {
		return [...(series[0]?.points ?? [])].reverse().find((p) => p.value !== null)?.value ?? null;
	}

	async function runDistribution(dist: DerivedSignal): Promise<DistributionResult> {
		const m = dist.measure;
		const hasAvg = m.aggregates.includes('sum') && m.aggregates.includes('count');
		const selects: SelectItem[] = [];
		if (hasAvg) selects.push({ measure: m.name, aggregate: 'sum' }, { measure: m.name, aggregate: 'count' });
		const p50Idx = selects.length;
		selects.push(
			{ measure: m.name, aggregate: 'tdigest', quantile: 0.5 },
			{ measure: m.name, aggregate: 'tdigest', quantile: 0.95 },
			{ measure: m.name, aggregate: 'tdigest', quantile: 0.99 }
		);
		const [p95Idx, p99Idx] = [p50Idx + 1, p50Idx + 2];

		const request = {
			level,
			time_range: presetToTimeRange(preset),
			filter: filters,
			group_by: [] as string[],
			select: selects
		};
		const empty = (error: string): DistributionResult => ({
			key: m.name,
			label: dist.label,
			avgCurrent: null,
			avgPrevious: null,
			avgPoints: [],
			p95Current: null,
			p95Previous: null,
			chartSeries: [],
			error
		});

		try {
			const response = await query(request);

			let avgCurrent: number | null = null;
			let avgPoints: SeriesPoint[] = [];
			if (hasAvg) {
				const sumSeries = toSeries(request, response, 0);
				const countSeries = toSeries(request, response, 1);
				const sumTotal = sumSeriesTotal(sumSeries);
				const countTotal = sumSeriesTotal(countSeries);
				avgCurrent = sumTotal !== null && countTotal ? sumTotal / countTotal : null;
				avgPoints = divideSeries(sumSeries, countSeries)[0]?.points ?? [];
			}

			const p50Series = toSeries(request, response, p50Idx);
			const p95Series = toSeries(request, response, p95Idx);
			const p99Series = toSeries(request, response, p99Idx);
			const p95Current = latestValue(p95Series);
			const chartSeries: Series[] = [
				{ label: 'P50', points: p50Series[0]?.points ?? [] },
				{ label: 'P95', points: p95Series[0]?.points ?? [] },
				{ label: 'P99', points: p99Series[0]?.points ?? [] }
			];

			let avgPrevious: number | null = null;
			let p95Previous: number | null = null;
			try {
				const priorRequest = { ...request, time_range: presetToPriorTimeRange(preset) };
				const priorResponse = await query(priorRequest);
				if (hasAvg) {
					const sumTotal = sumSeriesTotal(toSeries(priorRequest, priorResponse, 0));
					const countTotal = sumSeriesTotal(toSeries(priorRequest, priorResponse, 1));
					avgPrevious = sumTotal !== null && countTotal ? sumTotal / countTotal : null;
				}
				p95Previous = latestValue(toSeries(priorRequest, priorResponse, p95Idx));
			} catch {
				// no prior-window data — deltas just don't show for this distribution.
			}

			return {
				key: m.name,
				label: dist.label,
				avgCurrent,
				avgPrevious,
				avgPoints,
				p95Current,
				p95Previous,
				chartSeries,
				error: null
			};
		} catch (e) {
			return empty(e instanceof Error ? e.message : 'Query failed');
		}
	}

	// --- Breakdowns (one panel per non-pinned dimension) ---
	interface BreakdownResult {
		dimension: string;
		rows: { label: string; value: number; pct: number }[];
		error: string | null;
	}
	let breakdownResults = $state<BreakdownResult[]>([]);

	// Prefer the derived volume measure for "how is this dimension's traffic distributed"; fall
	// back to any sum-having measure, then any count-having one, so breakdowns still work even on
	// a schema with no obviously volume-shaped measure.
	let breakdownMeasure = $derived.by((): { name: string; aggregate: 'sum' | 'count' } | null => {
		if (!schema) return null;
		if (signals?.volume) return { name: signals.volume.measure.name, aggregate: 'sum' };
		const anySum = schema.measures.find((m) => m.aggregates.includes('sum'));
		if (anySum) return { name: anySum.name, aggregate: 'sum' };
		const anyCount = schema.measures.find((m) => m.aggregates.includes('count'));
		if (anyCount) return { name: anyCount.name, aggregate: 'count' };
		return null;
	});

	async function runBreakdown(dimension: string): Promise<BreakdownResult> {
		if (!breakdownMeasure) return { dimension, rows: [], error: null };
		const request = {
			level,
			time_range: presetToTimeRange(preset),
			filter: filters,
			group_by: [dimension],
			select: [{ measure: breakdownMeasure.name, aggregate: breakdownMeasure.aggregate }]
		};
		try {
			const response = await query(request);
			const series = toSeries(request, response, 0);
			const totals = series.map((s) => ({ label: s.label, value: sumSeriesTotal([s]) ?? 0 }));
			const grandTotal = totals.reduce((n, t) => n + t.value, 0);
			const rows = totals
				.filter((t) => t.value > 0)
				.map((t) => ({ label: t.label, value: t.value, pct: grandTotal > 0 ? (t.value / grandTotal) * 100 : 0 }))
				.sort((a, b) => b.value - a.value)
				.slice(0, 6);
			return { dimension, rows, error: null };
		} catch (e) {
			return { dimension, rows: [], error: e instanceof Error ? e.message : 'Query failed' };
		}
	}

	async function loadSchema() {
		try {
			schema = await getSchema();
			needsSchemaCreation = false;
		} catch (e) {
			if (e instanceof ApiError && e.body.error === 'no_schema') {
				needsSchemaCreation = true;
			} else {
				schemaError = e instanceof Error ? e.message : 'Could not load schema.';
			}
		}
	}

	async function onSchemaCreated(newSchema: SchemaResponse) {
		schema = newSchema;
		needsSchemaCreation = false;
		schemaError = null;
		filters = {};
		breadcrumb = [];
		await refreshAll();
	}

	async function refreshAll() {
		await runVolumeAndErrors();
		distributionResults = signals ? await Promise.all(signals.distributions.map(runDistribution)) : [];
		if (schema && breakdownMeasure) {
			const dims = schema.dimensions.filter((d) => !pinnedDimensions.includes(d));
			breakdownResults = await Promise.all(dims.map(runBreakdown));
		} else {
			breakdownResults = [];
		}
	}

	function onControlsChanged() {
		refreshAll();
	}

	function onBreakdownClick(dimension: string, value: string) {
		filters = { ...filters, [dimension]: [value] };
		breadcrumb = [...breadcrumb, `${dimension}=${value}`];
		refreshAll();
	}

	function resetDrilldown() {
		filters = {};
		breadcrumb = [];
		refreshAll();
	}

	onMount(async () => {
		await loadSchema();
		await refreshAll();
	});

	$effect(() => {
		if (!autoRefresh) return;
		const id = setInterval(refreshAll, 30_000);
		return () => clearInterval(id);
	});
</script>

<div class="dashboard">
	{#if needsSchemaCreation}
		<CreateSchemaForm onCreated={onSchemaCreated} />
	{:else if schemaError}
		<div class="card banner">{schemaError}</div>
	{:else if schema && signals}
		<div class="card controls">
			<label>
				Time range
				<select bind:value={preset} onchange={onControlsChanged}>
					{#each TIME_RANGE_PRESETS as opt (opt.value)}
						<option value={opt.value}>{opt.label}</option>
					{/each}
				</select>
			</label>
			<label class="checkbox">
				<input type="checkbox" bind:checked={autoRefresh} />
				Auto-refresh (30s)
			</label>
		</div>

		{#if breadcrumb.length > 0}
			<div class="card breadcrumb">
				<button type="button" class="crumb" onclick={resetDrilldown}>All</button>
				{#each breadcrumb as crumb, i (crumb)}
					<span class="crumb-sep">›</span>
					<span class="crumb" class:current={i === breadcrumb.length - 1}>{crumb}</span>
				{/each}
			</div>
		{/if}

		{#if !signals.volume && signals.distributions.length === 0}
			<div class="card banner">
				Couldn't derive any monitoring signals from this schema — no measure looks like a volume
				counter (a name containing "count"/"requests"/etc with <code>sum</code> registered) or a
				distribution (<code>tdigest</code> registered). Use the Explorer to query this schema directly.
			</div>
		{/if}

		{#if signals.volume || signals.distributions.length > 0}
			<div class="card kpi-row">
				{#if signals.volume}
					<KpiCard
						label={signals.volume.label}
						current={volumeCurrent}
						previous={volumePrevious}
						points={volumeSeries[0]?.points ?? []}
						valueFormat={fmtCount}
						color={SERIES_PALETTE[0]}
					/>
				{/if}
				{#if signals.errors && signals.volume}
					<KpiCard
						label="Error Rate"
						current={errorRateCurrent}
						previous={errorRatePrevious}
						points={errorRateSeries[0]?.points ?? []}
						valueFormat={fmtPct}
						color={SERIES_PALETTE[1]}
					/>
				{/if}
				{#each distributionResults as d, i (d.key)}
					{#if d.avgCurrent !== null || d.avgPrevious !== null}
						<KpiCard
							label="Avg {d.label}"
							current={d.avgCurrent}
							previous={d.avgPrevious}
							points={d.avgPoints}
							valueFormat={fmtMs}
							color={SERIES_PALETTE[(2 + i * 2) % SERIES_PALETTE.length]}
						/>
					{/if}
					<KpiCard
						label="P95 {d.label}"
						current={d.p95Current}
						previous={d.p95Previous}
						points={d.chartSeries.find((s) => s.label === 'P95')?.points ?? []}
						valueFormat={fmtMs}
						color={SERIES_PALETTE[(3 + i * 2) % SERIES_PALETTE.length]}
					/>
				{/each}
			</div>
		{/if}

		{#if signals.volume}
			<div class="card">
				<h2>{signals.volume.label} rate</h2>
				{#if volumeError}<p class="banner-inline">{volumeError}</p>{/if}
				<LineChart series={volumeSeries} />
			</div>
		{/if}

		{#each distributionResults as d (d.key)}
			<div class="card">
				<h2>{d.label}</h2>
				{#if d.error}<p class="banner-inline">{d.error}</p>{/if}
				<LineChart series={d.chartSeries} />
			</div>
		{/each}
		{#if distributionResults.length > 0}
			<p class="muted">
				Percentile lines come from accreta's <code>tdigest</code> aggregate — a sketch structure
				that merges correctly across rollup levels, unlike an average of averages.
			</p>
		{/if}

		{#if breakdownResults.some((b) => b.rows.length > 0)}
			<div class="card breakdown-grid">
				{#each breakdownResults as b (b.dimension)}
					{#if b.rows.length > 0}
						<BreakdownPanel
							title="By {b.dimension}"
							rows={b.rows}
							onRowClick={(v) => onBreakdownClick(b.dimension, v)}
						/>
					{/if}
				{/each}
			</div>
		{/if}
	{/if}
</div>

<style>
	.dashboard {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}
	.controls {
		display: flex;
		gap: 1.5rem;
		flex-wrap: wrap;
		align-items: flex-end;
	}
	.controls label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		font-size: 0.8rem;
		color: var(--text-dim);
	}
	.controls select {
		padding: 0.4rem;
		border-radius: 6px;
		border: 1px solid var(--border);
		background: var(--bg);
		color: var(--text);
	}
	.checkbox {
		flex-direction: row !important;
		align-items: center;
		gap: 0.4rem !important;
	}
	.kpi-row {
		display: flex;
		gap: 2rem;
		flex-wrap: wrap;
	}
	.breadcrumb {
		display: flex;
		align-items: center;
		gap: 0.35rem;
		font-size: 0.8rem;
	}
	.crumb {
		background: none;
		border: none;
		color: var(--accent);
		cursor: pointer;
		padding: 0;
		font: inherit;
	}
	.crumb.current {
		color: var(--text-dim);
		cursor: default;
	}
	.crumb-sep {
		color: var(--text-dim);
	}
	h2 {
		margin: 0 0 0.5rem;
		font-size: 1rem;
	}
	.muted {
		color: var(--text-dim);
		font-size: 0.8rem;
		margin: -0.5rem 0 0;
	}
	.banner {
		color: var(--text-dim);
	}
	.banner-inline {
		color: #e15b5b;
		font-size: 0.85rem;
	}
	.breakdown-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
		gap: 1.5rem;
	}
	code {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: 4px;
		padding: 0.1em 0.35em;
		font-size: 0.9em;
	}
</style>
