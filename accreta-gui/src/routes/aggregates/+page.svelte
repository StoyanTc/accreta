<script lang="ts">
	import { onMount } from 'svelte';
	import { getSchema, query, ApiError } from '$lib/api/client';
	import type { SchemaResponse, BucketLevel, TimeRange, SelectItem } from '$lib/api/schema';
	import {
		TIME_RANGE_PRESETS,
		RESOLUTIONS,
		presetToTimeRange,
		estimatedBucketCountForRange,
		MAX_REASONABLE_BUCKETS,
		type TimeRangePreset
	} from '$lib/api/query-helpers';
	import CreateSchemaForm from '$lib/components/CreateSchemaForm.svelte';
	import FilterPicker from '$lib/components/FilterPicker.svelte';
	import RollupDiagram from '$lib/components/RollupDiagram.svelte';

	type RangeMode = TimeRangePreset | 'custom';

	let schema = $state<SchemaResponse | null>(null);
	let needsSchemaCreation = $state(false);
	let schemaError = $state<string | null>(null);

	let level = $state<BucketLevel>('second');
	let rangeMode = $state<RangeMode>('1h');
	let customStart = $state('');
	let customEnd = $state('');
	let measureName = $state('');
	let filters = $state<Record<string, string[]>>({});

	interface InspectedRow {
		bucketStart: string;
		dimensions: Record<string, string>;
		values: { label: string; value: number | string | null }[];
	}
	let rows = $state<InspectedRow[]>([]);
	let loading = $state(false);
	let inspectError = $state<string | null>(null);
	let hasRun = $state(false);

	function currentTimeRange(): TimeRange {
		if (rangeMode === 'custom') {
			return {
				start: customStart ? new Date(customStart).toISOString() : new Date().toISOString(),
				end: customEnd ? new Date(customEnd).toISOString() : new Date().toISOString()
			};
		}
		return presetToTimeRange(rangeMode);
	}

	let rangeMs = $derived.by(() => {
		const { start, end } = currentTimeRange();
		return new Date(end).getTime() - new Date(start).getTime();
	});
	let bucketEstimate = $derived(estimatedBucketCountForRange(rangeMs, level));
	let resolutionTooFine = $derived(bucketEstimate > MAX_REASONABLE_BUCKETS && bucketEstimate > 0);

	async function loadSchema() {
		try {
			schema = await getSchema();
			needsSchemaCreation = false;
			filters = Object.fromEntries(schema.dimensions.map((d) => [d, []]));
			if (!measureName && schema.measures.length > 0) measureName = schema.measures[0].name;
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
		measureName = '';
		await loadSchema();
	}

	const STANDARD_QUANTILES = [0.5, 0.9, 0.95, 0.99];

	function buildSelect(measure: SchemaResponse['measures'][number]): { item: SelectItem; label: string }[] {
		const out: { item: SelectItem; label: string }[] = [];
		for (const agg of measure.aggregates) {
			if (agg === 'tdigest') {
				for (const q of STANDARD_QUANTILES) {
					out.push({ item: { measure: measure.name, aggregate: 'tdigest', quantile: q }, label: `p${q * 100}` });
				}
			} else {
				out.push({ item: { measure: measure.name, aggregate: agg }, label: agg });
			}
		}
		return out;
	}

	async function inspect() {
		if (!schema || !measureName || resolutionTooFine) return;
		const measure = schema.measures.find((m) => m.name === measureName);
		if (!measure) return;

		const selectSpec = buildSelect(measure);
		const activeFilters = Object.fromEntries(
			Object.entries(filters).filter(([, values]) => values.length > 0)
		);

		loading = true;
		inspectError = null;
		hasRun = true;
		try {
			const response = await query({
				level,
				time_range: currentTimeRange(),
				filter: activeFilters,
				// Every dimension, so each returned group fully specifies one concrete combination
				// to inspect — a partial group_by would blur several real groups' states together.
				group_by: schema.dimensions,
				select: selectSpec.map((s) => s.item)
			});

			rows = response.buckets.flatMap((bucket) =>
				bucket.groups.map((group) => {
					const values = selectSpec.map((s, i) => ({ label: s.label, value: group.values[i] }));

					// accreta v0.2.0 removed the Average aggregate as redundant with sum/count — derive
					// it here from the two values already fetched, rather than an extra select item.
					const sumIdx = selectSpec.findIndex((s) => s.label === 'sum');
					const countIdx = selectSpec.findIndex((s) => s.label === 'count');
					if (sumIdx !== -1 && countIdx !== -1) {
						const sumVal = group.values[sumIdx];
						const countVal = group.values[countIdx];
						const mean =
							typeof sumVal === 'number' && typeof countVal === 'number' && countVal !== 0
								? sumVal / countVal
								: null;
						values.splice(countIdx + 1, 0, { label: 'mean', value: mean });
					}

					return {
						bucketStart: bucket.bucket_start,
						dimensions: Object.fromEntries(schema!.dimensions.map((d, i) => [d, group.dimensions[i]])),
						values
					};
				})
			);
		} catch (e) {
			inspectError = e instanceof Error ? e.message : 'Query failed';
			rows = [];
		} finally {
			loading = false;
		}
	}

	onMount(loadSchema);
</script>

<div class="aggregates">
	<div class="card">
		<h2>Rollup hierarchy</h2>
		<p class="muted">
			accreta's actual rollup DAG — not a straight chain. <code>Day</code> feeds both
			<code>Week</code> and <code>Month</code> independently; <code>Week</code> is terminal.
			Changing this GUI's resolution control queries a separately maintained aggregate at
			that level, not a re-sampling of the same underlying data.
		</p>
		<RollupDiagram />
	</div>

	{#if needsSchemaCreation}
		<CreateSchemaForm onCreated={onSchemaCreated} />
	{:else if schemaError}
		<div class="card banner">{schemaError}</div>
	{:else if schema}
		<div class="card">
			<h2>Aggregate inspector</h2>
			<p class="muted">
				Pick a level, time range, measure, and optionally narrow by dimension — every registered
				aggregate for that measure (including all four standard tdigest quantiles, if
				registered) is queried and shown per matching bucket/dimension combination below.
			</p>

			<div class="controls">
				<label>
					Level
					<select bind:value={level}>
						{#each RESOLUTIONS as opt (opt.value)}
							<option value={opt.value}>{opt.label}</option>
						{/each}
					</select>
				</label>
				<label>
					Time range
					<select bind:value={rangeMode}>
						{#each TIME_RANGE_PRESETS as opt (opt.value)}
							<option value={opt.value}>{opt.label}</option>
						{/each}
						<option value="custom">Custom</option>
					</select>
				</label>
				{#if rangeMode === 'custom'}
					<label>
						Start
						<input type="datetime-local" bind:value={customStart} />
					</label>
					<label>
						End
						<input type="datetime-local" bind:value={customEnd} />
					</label>
				{/if}
				<label>
					Measure
					<select bind:value={measureName}>
						{#each schema.measures as m (m.name)}
							<option value={m.name}>{m.name}</option>
						{/each}
					</select>
				</label>
			</div>

			{#if schema.dimensions.length > 0}
				<div class="filters">
					{#each schema.dimensions as dim (dim)}
						<FilterPicker {schema} dimension={dim} {level} timeRange={currentTimeRange()} bind:selected={filters[dim]} />
					{/each}
				</div>
			{/if}

			{#if resolutionTooFine}
				<p class="banner-inline">
					That range at {level} resolution would be ~{bucketEstimate.toLocaleString()} buckets —
					pick a coarser resolution, a shorter range, or narrow the filters above.
				</p>
			{:else}
				<button onclick={inspect} disabled={loading}>{loading ? 'Inspecting…' : 'Inspect'}</button>
			{/if}
			{#if inspectError}<p class="banner-inline">{inspectError}</p>{/if}
		</div>

		{#if hasRun && !loading}
			{#if rows.length === 0}
				<div class="card muted">No matching buckets/groups for this selection.</div>
			{:else}
				{#each rows as row, i (row.bucketStart + JSON.stringify(row.dimensions))}
					<div class="card aggregate-row">
						<div class="aggregate-header">
							<span class="bucket-time">{new Date(row.bucketStart).toLocaleString()}</span>
							<span class="dims">
								{#each Object.entries(row.dimensions) as [dim, val] (dim)}
									<span class="dim-pill">{dim} = {val}</span>
								{/each}
							</span>
						</div>
						<div class="state-grid">
							{#each row.values as v (v.label)}
								<div class="state-item">
									<div class="state-label">{v.label}</div>
									<div class="state-value">{v.value === null ? '—' : v.value}</div>
								</div>
							{/each}
						</div>
					</div>
				{/each}
			{/if}
		{/if}
	{/if}
</div>

<style>
	.aggregates {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}
	h2 {
		margin: 0 0 0.5rem;
		font-size: 1rem;
	}
	.muted {
		color: var(--text-dim);
		font-size: 0.85rem;
		margin: 0 0 0.75rem;
	}
	code {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: 4px;
		padding: 0.1em 0.35em;
		font-size: 0.9em;
	}
	.controls {
		display: flex;
		gap: 1.5rem;
		flex-wrap: wrap;
		align-items: flex-end;
		margin-bottom: 0.75rem;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		font-size: 0.8rem;
		color: var(--text-dim);
	}
	select,
	input {
		padding: 0.4rem;
		border-radius: 6px;
		border: 1px solid var(--border);
		background: var(--bg);
		color: var(--text);
	}
	.filters {
		display: flex;
		gap: 1rem;
		flex-wrap: wrap;
		margin-bottom: 0.75rem;
	}
	button {
		padding: 0.5rem 0.9rem;
		border-radius: 6px;
		border: none;
		background: var(--accent);
		color: white;
		cursor: pointer;
	}
	button:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}
	.banner-inline {
		color: #e15b5b;
		font-size: 0.85rem;
	}
	.aggregate-header {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-bottom: 0.75rem;
	}
	.bucket-time {
		font-weight: 600;
		font-size: 0.9rem;
	}
	.dims {
		display: flex;
		gap: 0.4rem;
		flex-wrap: wrap;
	}
	.dim-pill {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: 999px;
		padding: 0.15rem 0.6rem;
		font-size: 0.75rem;
		color: var(--text-dim);
	}
	.state-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(90px, 1fr));
		gap: 0.75rem;
	}
	.state-label {
		font-size: 0.7rem;
		color: var(--text-dim);
		text-transform: uppercase;
		letter-spacing: 0.03em;
	}
	.state-value {
		font-size: 1rem;
		font-weight: 600;
	}
</style>
