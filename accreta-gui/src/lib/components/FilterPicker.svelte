<script lang="ts">
	import { discoverDimensionValues } from '$lib/api/discovery';
	import type { SchemaResponse, BucketLevel, TimeRange } from '$lib/api/schema';

	interface Props {
		schema: SchemaResponse;
		dimension: string;
		level: BucketLevel;
		timeRange: TimeRange;
		selected: string[];
	}
	let { schema, dimension, level, timeRange, selected = $bindable() }: Props = $props();

	let values = $state<string[] | null>(null);
	let loading = $state(false);
	let error = $state<string | null>(null);

	async function discover() {
		loading = true;
		error = null;
		try {
			values = await discoverDimensionValues(schema, dimension, level, timeRange);
		} catch {
			error = 'Could not discover values for this dimension.';
		} finally {
			loading = false;
		}
	}

	function toggle(value: string) {
		selected = selected.includes(value) ? selected.filter((v) => v !== value) : [...selected, value];
	}
</script>

<div class="filter-picker">
	<div class="filter-header">
		<span class="filter-name">{dimension}</span>
		<button type="button" class="link" onclick={discover} disabled={loading}>
			{loading ? 'discovering…' : values === null ? 'discover values' : 're-discover'}
		</button>
	</div>

	{#if error}
		<p class="error">{error}</p>
	{/if}

	{#if values !== null}
		{#if values.length === 0}
			<p class="muted">No values seen for "{dimension}" in the current time range/resolution.</p>
		{:else}
			<div class="values">
				{#each values as value (value)}
					<label class="checkbox">
						<input
							type="checkbox"
							checked={selected.includes(value)}
							onchange={() => toggle(value)}
						/>
						{value}
					</label>
				{/each}
			</div>
		{/if}
	{/if}
</div>

<style>
	.filter-picker {
		display: flex;
		flex-direction: column;
		gap: 0.35rem;
		padding: 0.5rem;
		border: 1px solid var(--border);
		border-radius: 8px;
	}
	.filter-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
	}
	.filter-name {
		font-size: 0.8rem;
		font-weight: 600;
	}
	.link {
		background: none;
		border: none;
		color: var(--accent);
		font-size: 0.75rem;
		cursor: pointer;
		padding: 0;
	}
	.link:disabled {
		opacity: 0.6;
		cursor: not-allowed;
	}
	.values {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
	.checkbox {
		display: flex;
		align-items: center;
		gap: 0.25rem;
		font-size: 0.75rem;
		color: var(--text-dim);
	}
	.muted {
		font-size: 0.75rem;
		color: var(--text-dim);
		margin: 0;
	}
	.error {
		font-size: 0.75rem;
		color: #e15b5b;
		margin: 0;
	}
</style>
