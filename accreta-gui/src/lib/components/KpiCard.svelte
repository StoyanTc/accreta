<script lang="ts">
	import Sparkline from '$lib/charts/Sparkline.svelte';
	import type { SeriesPoint } from '$lib/api/query-helpers';

	interface Props {
		label: string;
		current: number | null;
		previous: number | null;
		points: SeriesPoint[];
		valueFormat: (v: number) => string;
		color: string;
	}
	let { label, current, previous, points, valueFormat, color }: Props = $props();

	// Undefined (not just null) when there's nothing to compare against — e.g. this label didn't
	// exist in the prior window at all, which is a meaningfully different case from "0% change"
	// and shouldn't be presented as one.
	let deltaPct = $derived(
		current !== null && previous !== null && previous !== 0
			? ((current - previous) / previous) * 100
			: null
	);
</script>

<div class="kpi-card">
	<div class="kpi-label">{label}</div>
	<div class="kpi-value">{current === null ? '—' : valueFormat(current)}</div>
	{#if deltaPct !== null}
		<div class="kpi-delta" class:up={deltaPct >= 0} class:down={deltaPct < 0}>
			{deltaPct >= 0 ? '+' : ''}{deltaPct.toFixed(1)}%
			<span class="kpi-delta-caption">vs prior period</span>
		</div>
	{/if}
	<div class="kpi-spark">
		<Sparkline {points} {color} />
	</div>
</div>

<style>
	.kpi-card {
		display: flex;
		flex-direction: column;
		gap: 0.2rem;
		min-width: 140px;
	}
	.kpi-label {
		font-size: 0.75rem;
		color: var(--text-dim);
	}
	.kpi-value {
		font-size: 1.4rem;
		font-weight: 600;
	}
	.kpi-delta {
		font-size: 0.75rem;
	}
	.kpi-delta.up {
		color: #5be18f;
	}
	.kpi-delta.down {
		color: #e15b5b;
	}
	.kpi-delta-caption {
		color: var(--text-dim);
		margin-left: 0.25rem;
	}
	.kpi-spark {
		margin-top: 0.25rem;
	}
</style>
