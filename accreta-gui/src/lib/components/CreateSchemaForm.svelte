<script lang="ts">
	import { createSchema, ingest, ApiError } from '$lib/api/client';
	import { DEMO_SCHEMA, generateSyntheticSamples, chunk } from '$lib/api/demo-data';
	import type { SchemaRequest, SchemaResponse, MeasureRequest, ValueType } from '$lib/api/schema';

	interface Props {
		onCreated: (schema: SchemaResponse) => void;
	}
	let { onCreated }: Props = $props();

	const ALL_AGGREGATES = ['sum', 'count', 'min', 'max', 'tdigest'] as const;
	const VALUE_TYPES: ValueType[] = ['f64', 'i64', 'u64'];

	let name = $state('demo');
	let dimensions = $state<string[]>(['']);
	let measures = $state<MeasureRequest[]>([{ name: '', value_type: 'f64', aggregates: [] }]);

	let submitting = $state(false);
	let error = $state<string | null>(null);
	let created = $state<SchemaResponse | null>(null);

	let seeding = $state(false);
	let seedProgress = $state<{ done: number; total: number } | null>(null);
	let seedError = $state<string | null>(null);
	let seeded = $state(false);

	function loadDemoTemplate() {
		name = DEMO_SCHEMA.name;
		dimensions = [...DEMO_SCHEMA.dimensions];
		measures = DEMO_SCHEMA.measures.map((m) => ({ ...m, aggregates: [...m.aggregates] }));
	}

	function addDimension() {
		dimensions = [...dimensions, ''];
	}
	function removeDimension(i: number) {
		dimensions = dimensions.filter((_, idx) => idx !== i);
	}

	function addMeasure() {
		measures = [...measures, { name: '', value_type: 'f64', aggregates: [] }];
	}
	function removeMeasure(i: number) {
		measures = measures.filter((_, idx) => idx !== i);
	}
	function toggleAggregate(measureIndex: number, agg: string) {
		const m = measures[measureIndex];
		m.aggregates = m.aggregates.includes(agg)
			? m.aggregates.filter((a) => a !== agg)
			: [...m.aggregates, agg];
		// tdigest only compiles for f64 server-side (dispatch.rs) — drop it immediately if the
		// value type doesn't (or no longer) supports it, rather than letting the server reject it.
		if (agg === 'tdigest' && m.value_type !== 'f64') {
			m.aggregates = m.aggregates.filter((a) => a !== 'tdigest');
		}
		measures = [...measures];
	}
	function onValueTypeChange(measureIndex: number) {
		const m = measures[measureIndex];
		if (m.value_type !== 'f64') m.aggregates = m.aggregates.filter((a) => a !== 'tdigest');
		measures = [...measures];
	}

	// Mirrors the checks in schema_api.rs/dispatch.rs closely enough to catch obvious mistakes
	// before a round trip — the server remains the source of truth and re-validates regardless.
	function clientSideError(): string | null {
		const dims = dimensions.map((d) => d.trim()).filter(Boolean);
		if (dims.length === 0) return 'At least one dimension is required.';
		if (new Set(dims).size !== dims.length) return 'Dimension names must be unique.';

		const measureNames = measures.map((m) => m.name.trim()).filter(Boolean);
		if (measureNames.length === 0) return 'At least one measure is required.';
		if (new Set(measureNames).size !== measureNames.length) return 'Measure names must be unique.';
		if (new Set([...dims, ...measureNames]).size !== dims.length + measureNames.length) {
			return 'Dimension and measure names must all be distinct from each other.';
		}
		for (const m of measures) {
			if (m.aggregates.length === 0) return `Measure "${m.name}" needs at least one aggregate.`;
		}
		return null;
	}

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		error = clientSideError();
		if (error) return;

		const request: SchemaRequest = {
			name,
			dimensions: dimensions.map((d) => d.trim()).filter(Boolean),
			measures: measures.map((m) => ({ ...m, name: m.name.trim() }))
		};

		submitting = true;
		try {
			created = await createSchema(request);
			onCreated(created);
		} catch (e) {
			error =
				e instanceof ApiError
					? `${e.body.detail}${e.body.field ? ` (${e.body.field})` : ''}`
					: 'Failed to create schema.';
		} finally {
			submitting = false;
		}
	}

	async function seedSampleData() {
		if (!created) return;
		seeding = true;
		seedError = null;
		try {
			const samples = generateSyntheticSamples(created);
			const batches = chunk(samples, 500);
			seedProgress = { done: 0, total: batches.length };
			for (const batch of batches) {
				await ingest({ samples: batch });
				seedProgress = { done: seedProgress.done + 1, total: batches.length };
			}
			seeded = true;
		} catch (e) {
			seedError = e instanceof ApiError ? e.body.detail : 'Failed to seed sample data.';
		} finally {
			seeding = false;
		}
	}
</script>

{#if created}
	<div class="card">
		<h2>Schema "{created.name}" created</h2>
		<p class="muted">
			{created.dimensions.length} dimension(s), {created.measures.length} measure(s) registered.
			The dashboard has nothing to chart yet since nothing's been ingested.
		</p>

		{#if !seeded}
			<button onclick={seedSampleData} disabled={seeding}>
				{seeding
					? seedProgress
						? `Seeding batch ${seedProgress.done}/${seedProgress.total}…`
						: 'Seeding…'
					: 'Seed synthetic sample data (last 24h)'}
			</button>
			{#if seedError}<p class="error">{seedError}</p>{/if}
			<p class="muted small">
				Or skip this and ingest your own data via <code>POST /schema/ingest</code>.
			</p>
		{:else}
			<p class="success">
				Sample data ingested. Rollups run on a background sweep every ~30s, so give it a moment
				before hourly/daily resolutions show everything.
			</p>
			<button onclick={() => onCreated(created!)}>Continue to dashboard</button>
		{/if}
	</div>
{:else}
	<form class="card" onsubmit={submit}>
		<div class="form-header">
			<h2>Create a schema</h2>
			<button type="button" class="secondary" onclick={loadDemoTemplate}>Load demo template</button>
		</div>

		<label>
			Schema name
			<input bind:value={name} />
		</label>

		<fieldset>
			<legend>Dimensions</legend>
			{#each dimensions as _, i}
				<div class="row">
					<input placeholder="e.g. service" bind:value={dimensions[i]} />
					<button type="button" class="icon" onclick={() => removeDimension(i)}>&times;</button>
				</div>
			{/each}
			<button type="button" class="secondary" onclick={addDimension}>+ Add dimension</button>
		</fieldset>

		<fieldset>
			<legend>Measures</legend>
			{#each measures as measure, i}
				<div class="measure-row">
					<input placeholder="e.g. latency_ms" bind:value={measure.name} />
					<select bind:value={measure.value_type} onchange={() => onValueTypeChange(i)}>
						{#each VALUE_TYPES as vt (vt)}
							<option value={vt}>{vt}</option>
						{/each}
					</select>
					<div class="aggregates">
						{#each ALL_AGGREGATES as agg (agg)}
							<label class="checkbox small" class:disabled={agg === 'tdigest' && measure.value_type !== 'f64'}>
								<input
									type="checkbox"
									checked={measure.aggregates.includes(agg)}
									disabled={agg === 'tdigest' && measure.value_type !== 'f64'}
									onchange={() => toggleAggregate(i, agg)}
								/>
								{agg}
							</label>
						{/each}
					</div>
					<button type="button" class="icon" onclick={() => removeMeasure(i)}>&times;</button>
				</div>
			{/each}
			<button type="button" class="secondary" onclick={addMeasure}>+ Add measure</button>
		</fieldset>

		{#if error}<p class="error">{error}</p>{/if}

		<button type="submit" disabled={submitting}>
			{submitting ? 'Creating…' : 'Create schema'}
		</button>
	</form>
{/if}

<style>
	form,
	.card {
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}
	.form-header {
		display: flex;
		justify-content: space-between;
		align-items: baseline;
	}
	.form-header h2,
	.card h2 {
		margin: 0;
		font-size: 1rem;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		font-size: 0.8rem;
		color: var(--text-dim);
	}
	fieldset {
		border: 1px solid var(--border);
		border-radius: 8px;
		padding: 0.75rem;
		display: flex;
		flex-direction: column;
		gap: 0.5rem;
	}
	legend {
		font-size: 0.8rem;
		color: var(--text-dim);
		padding: 0 0.3rem;
	}
	.row,
	.measure-row {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}
	.measure-row {
		flex-wrap: wrap;
	}
	input,
	select {
		padding: 0.4rem;
		border-radius: 6px;
		border: 1px solid var(--border);
		background: var(--bg);
		color: var(--text);
	}
	.aggregates {
		display: flex;
		gap: 0.5rem;
		flex-wrap: wrap;
		flex: 1;
	}
	.checkbox {
		display: flex;
		align-items: center;
		gap: 0.25rem;
		font-size: 0.75rem;
		color: var(--text-dim);
	}
	.checkbox.disabled {
		opacity: 0.4;
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
	}
	.muted {
		color: var(--text-dim);
		font-size: 0.85rem;
		margin: 0;
	}
	.muted.small {
		font-size: 0.75rem;
	}
	.error {
		color: #e15b5b;
		font-size: 0.85rem;
		margin: 0;
	}
	.success {
		color: #5be18f;
		font-size: 0.85rem;
		margin: 0;
	}
	code {
		font-size: 0.8em;
	}
</style>
