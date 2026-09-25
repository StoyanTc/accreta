<script lang="ts">
	import { onMount } from 'svelte';
	import { getSchema, query, ApiError } from '$lib/api/client';
	import type { SchemaResponse, BucketLevel, SelectItem, TimeRange } from '$lib/api/schema';
	import {
		TIME_RANGE_PRESETS,
		RESOLUTIONS,
		presetToTimeRange,
		estimatedBucketCountForRange,
		MAX_REASONABLE_BUCKETS,
		toSeries,
		type TimeRangePreset,
		type Series
	} from '$lib/api/query-helpers';
	import LineChart from '$lib/charts/LineChart.svelte';
	import CreateSchemaForm from '$lib/components/CreateSchemaForm.svelte';
	import FilterPicker from '$lib/components/FilterPicker.svelte';

	type RangeMode = TimeRangePreset | 'custom';

	let schema = $state<SchemaResponse | null>(null);
	let needsSchemaCreation = $state(false);
	let schemaError = $state<string | null>(null);

	// Controls
	let rangeMode = $state<RangeMode>('24h');
	let customStart = $state('');
	let customEnd = $state('');
	let level = $state<BucketLevel>('hour');
	let groupBy = $state<string[]>([]);
	let filters = $state<Record<string, string[]>>({});
	let selects = $state<SelectItem[]>([]);

	// Results
	let results = $state<{ select: SelectItem; series: Series[] }[]>([]);
	let bucketCount = $state<number | null>(null);
	let groupCount = $state<number | null>(null);
	let lastRequestJson = $state<string | null>(null);
	let loading = $state(false);
	let runError = $state<string | null>(null);

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

	function aggregatesFor(measureName: string): string[] {
		return schema?.measures.find((m) => m.name === measureName)?.aggregates ?? [];
	}

	function addSelect() {
		const first = schema?.measures[0];
		if (!first) return;
		selects = [...selects, { measure: first.name, aggregate: first.aggregates[0] }];
	}
	function removeSelect(i: number) {
		selects = selects.filter((_, idx) => idx !== i);
	}
	function onMeasureChange(i: number) {
		const s = selects[i];
		const aggs = aggregatesFor(s.measure);
		if (!aggs.includes(s.aggregate)) s.aggregate = aggs[0] ?? '';
		if (s.aggregate !== 'tdigest') delete s.quantile;
		selects = [...selects];
	}
	function onAggregateChange(i: number) {
		const s = selects[i];
		if (s.aggregate === 'tdigest' && s.quantile === undefined) s.quantile = 0.5;
		if (s.aggregate !== 'tdigest') delete s.quantile;
		selects = [...selects];
	}

	function toggleGroupBy(dim: string) {
		groupBy = groupBy.includes(dim) ? groupBy.filter((d) => d !== dim) : [...groupBy, dim];
	}

	async function loadSchema() {
		try {
			schema = await getSchema();
			needsSchemaCreation = false;
			filters = Object.fromEntries(schema.dimensions.map((d) => [d, []]));
			if (selects.length === 0 && schema.measures.length > 0) {
				const first = schema.measures[0];
				selects = [{ measure: first.name, aggregate: first.aggregates[0] }];
			}
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
		selects = [];
		await loadSchema();
	}

	async function runQuery() {
		if (!schema || selects.length === 0 || resolutionTooFine) return;

		const activeFilters = Object.fromEntries(
			Object.entries(filters).filter(([, values]) => values.length > 0)
		);

		const request = {
			level,
			time_range: currentTimeRange(),
			filter: activeFilters,
			group_by: groupBy,
			select: selects
		};
		lastRequestJson = JSON.stringify(request, null, 2);

		loading = true;
		runError = null;
		try {
			const response = await query(request);
			bucketCount = response.buckets.length;
			groupCount = response.buckets.reduce((n, b) => n + b.groups.length, 0);
			results = selects.map((select, i) => ({
				select,
				series: toSeries(request, response, i)
			}));
		} catch (e) {
			runError = e instanceof Error ? e.message : 'Query failed';
			results = [];
			bucketCount = null;
			groupCount = null;
		} finally {
			loading = false;
		}
	}

	function selectLabel(s: SelectItem): string {
		return s.aggregate === 'tdigest' ? `${s.measure} (p${(s.quantile ?? 0.5) * 100})` : `${s.measure} (${s.aggregate})`;
	}

	onMount(loadSchema);
</script>

<div class="explorer">
	{#if needsSchemaCreation}
		<CreateSchemaForm onCreated={onSchemaCreated} />
	{:else if schemaError}
		<div class="card banner">{schemaError}</div>
	{:else}
		<div class="card controls">
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
				Resolution
				<select bind:value={level}>
					{#each RESOLUTIONS as opt (opt.value)}
						<option value={opt.value}>{opt.label}</option>
					{/each}
				</select>
			</label>
		</div>

		{#if schema}
			<div class="card">
				<h2>Metrics</h2>
				{#each selects as select, i}
					<div class="select-row">
						<select bind:value={select.measure} onchange={() => onMeasureChange(i)}>
							{#each schema.measures as m (m.name)}
								<option value={m.name}>{m.name}</option>
							{/each}
						</select>
						<select bind:value={select.aggregate} onchange={() => onAggregateChange(i)}>
							{#each aggregatesFor(select.measure) as agg (agg)}
								<option value={agg}>{agg}</option>
							{/each}
						</select>
						{#if select.aggregate === 'tdigest'}
							<input
								type="number"
								min="0"
								max="1"
								step="0.01"
								value={select.quantile ?? 0.5}
								oninput={(e) => (select.quantile = parseFloat((e.target as HTMLInputElement).value))}
							/>
						{/if}
						<button type="button" class="icon" onclick={() => removeSelect(i)}>&times;</button>
					</div>
				{/each}
				<button type="button" class="secondary" onclick={addSelect}>+ Add metric</button>
			</div>

			{#if schema.dimensions.length > 0}
				<div class="card">
					<h2>Group by</h2>
					<div class="dimension-toggles">
						{#each schema.dimensions as dim (dim)}
							<label class="checkbox">
								<input
									type="checkbox"
									checked={groupBy.includes(dim)}
									onchange={() => toggleGroupBy(dim)}
								/>
								{dim}
							</label>
						{/each}
					</div>
				</div>

				<div class="card">
					<h2>Filters</h2>
					<p class="muted">
						Values are discovered from the currently-selected time range/resolution above — change
						those and re-discover if a value you expect is missing.
					</p>
					<div class="filters">
						{#each schema.dimensions as dim (dim)}
							<FilterPicker {schema} dimension={dim} {level} timeRange={currentTimeRange()} bind:selected={filters[dim]} />
						{/each}
					</div>
				</div>
			{/if}

			<div class="card">
				{#if resolutionTooFine}
					<p class="banner-inline">
						That range at {level} resolution would be ~{bucketEstimate.toLocaleString()} buckets —
						pick a coarser resolution or a shorter range.
					</p>
				{:else}
					<button onclick={runQuery} disabled={loading || selects.length === 0}>
						{loading ? 'Running…' : 'Run query'}
					</button>
				{/if}
				{#if runError}<p class="banner-inline">{runError}</p>{/if}
				{#if bucketCount !== null}
					<p class="muted">{bucketCount} bucket(s), {groupCount} group(s) returned.</p>
				{/if}
			</div>

			{#each results as result (selectLabel(result.select))}
				<div class="card">
					<h2>{selectLabel(result.select)}</h2>
					<LineChart series={result.series} />
				</div>
			{/each}

			{#if lastRequestJson}
				<details class="card">
					<summary>Last request</summary>
					<pre>{lastRequestJson}</pre>
				</details>
			{/if}
		{/if}
	{/if}
</div>

<style>
	.explorer {
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
	.controls label,
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
	h2 {
		margin: 0 0 0.5rem;
		font-size: 1rem;
	}
	.select-row {
		display: flex;
		gap: 0.5rem;
		align-items: center;
		margin-bottom: 0.5rem;
	}
	.dimension-toggles,
	.filters {
		display: flex;
		gap: 1rem;
		flex-wrap: wrap;
	}
	.checkbox {
		flex-direction: row !important;
		align-items: center;
		gap: 0.4rem !important;
	}
	button {
		padding: 0.5rem 0.9rem;
		border-radius: 6px;
		border: none;
		background: var(--accent);
		color: white;
		cursor: pointer;
		width: fit-content;
	}
	button:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}
	button.secondary {
		background: none;
		border: 1px solid var(--border);
		color: var(--text);
	}
	button.icon {
		background: none;
		border: none;
		color: var(--text-dim);
		font-size: 1.1rem;
		padding: 0.2rem 0.5rem;
		width: auto;
	}
	.muted {
		color: var(--text-dim);
		font-size: 0.8rem;
		margin: 0.25rem 0;
	}
	.banner-inline {
		color: #e15b5b;
		font-size: 0.85rem;
	}
	pre {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: 6px;
		padding: 0.5rem;
		overflow-x: auto;
		font-size: 0.75rem;
	}
</style>
