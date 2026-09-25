<script lang="ts">
	import { scaleLinear, scaleTime } from 'd3-scale';
	import { line as d3line, curveMonotoneX } from 'd3-shape';
	import { extent, max } from 'd3-array';
	import { timeFormat } from 'd3-time-format';
	import { SERIES_PALETTE } from '$lib/api/query-helpers';
	import type { Series } from '$lib/api/query-helpers';

	interface Props {
		series: Series[];
		height?: number;
		valueFormat?: (v: number) => string;
		/** Vertical unit label, e.g. "ms" or "req/s" — shown in the tooltip only. */
		unit?: string;
		/** When provided, legend entries become clickable — used for drill-down: clicking a
		 *  series pins its exact dimension-value combination as a filter. Omit to leave the
		 *  legend as plain, non-interactive labels (e.g. for the percentile chart, where a
		 *  "series" is a quantile, not a dimension combination to drill into). */
		onSeriesClick?: (label: string) => void;
	}

	let {
		series,
		height = 260,
		valueFormat = (v) => v.toLocaleString(),
		unit = '',
		onSeriesClick
	}: Props = $props();

	const PALETTE = SERIES_PALETTE;

	let containerEl: HTMLDivElement | undefined = $state();
	let width = $state(600);
	let hovered = $state<{ x: number; time: Date; entries: { label: string; value: number | null; color: string }[] } | null>(null);

	// Re-measure on resize so the chart stays responsive without a charting-library dependency
	// doing it for us.
	$effect(() => {
		if (!containerEl) return;
		const ro = new ResizeObserver(([entry]) => {
			width = entry.contentRect.width;
		});
		ro.observe(containerEl);
		return () => ro.disconnect();
	});

	const margin = { top: 10, right: 16, bottom: 24, left: 48 };
	let innerWidth = $derived(Math.max(width - margin.left - margin.right, 0));
	let innerHeight = $derived(height - margin.top - margin.bottom);

	let allPoints = $derived(series.flatMap((s) => s.points));
	let timeExtent = $derived(
		(extent(allPoints, (p) => p.time) as [Date, Date] | [undefined, undefined])
	);
	let maxValue = $derived(max(allPoints, (p) => p.value ?? 0) ?? 0);

	let xScale = $derived(
		timeExtent[0] && timeExtent[1]
			? scaleTime().domain(timeExtent).range([0, innerWidth])
			: null
	);
	let yScale = $derived(scaleLinear().domain([0, maxValue * 1.1 || 1]).range([innerHeight, 0]).nice());

	let lineGen = $derived(
		xScale
			? d3line<{ time: Date; value: number | null }>()
					.defined((d) => d.value !== null)
					.x((d) => xScale!(d.time))
					.y((d) => yScale(d.value ?? 0))
					.curve(curveMonotoneX)
			: null
	);

	let yTicks = $derived(yScale.ticks(5));
	let xTicks = $derived(xScale ? xScale.ticks(6) : []);
	const formatTick = timeFormat('%H:%M');
	const formatDay = timeFormat('%b %d');

	function tickLabel(d: Date, spanMs: number) {
		// Sub-day ranges show a clock; anything wider shows a date, so the axis stays legible
		// whether you're looking at "last 15 minutes" or "last 30 days".
		return spanMs <= 26 * 60 * 60_000 ? formatTick(d) : formatDay(d);
	}

	function onMouseMove(evt: MouseEvent) {
		if (!xScale) return;
		const rect = (evt.currentTarget as SVGElement).getBoundingClientRect();
		const mouseX = evt.clientX - rect.left - margin.left;
		const time = xScale.invert(mouseX);

		// Snap to the nearest actual point per series rather than interpolating a fake value.
		const entries = series.map((s, i) => {
			let closest = s.points[0];
			let bestDelta = Infinity;
			for (const p of s.points) {
				const delta = Math.abs(p.time.getTime() - time.getTime());
				if (delta < bestDelta) {
					bestDelta = delta;
					closest = p;
				}
			}
			return { label: s.label, value: closest?.value ?? null, color: PALETTE[i % PALETTE.length] };
		});

		hovered = { x: mouseX, time, entries };
	}
</script>

<div class="chart" bind:this={containerEl}>
	{#if series.length === 0 || allPoints.every((p) => p.value === null)}
		<div class="empty" style:height="{height}px">No data for this selection yet.</div>
	{:else}
		<svg
			{width}
			{height}
			role="img"
			aria-label="Time series chart"
			onmousemove={onMouseMove}
			onmouseleave={() => (hovered = null)}
		>
			<g transform="translate({margin.left},{margin.top})">
				<!-- gridlines -->
				{#each yTicks as tick (tick)}
					<line x1="0" x2={innerWidth} y1={yScale(tick)} y2={yScale(tick)} class="grid" />
					<text x="-8" y={yScale(tick)} dy="0.32em" text-anchor="end" class="axis-label">
						{valueFormat(tick)}
					</text>
				{/each}

				<!-- x axis -->
				{#if xScale}
					{#each xTicks as tick (tick.getTime())}
						<text
							x={xScale(tick)}
							y={innerHeight + 16}
							text-anchor="middle"
							class="axis-label"
						>
							{tickLabel(tick, timeExtent[1]!.getTime() - timeExtent[0]!.getTime())}
						</text>
					{/each}
				{/if}

				<!-- series lines -->
				{#if lineGen && xScale}
					{#each series as s, i (s.label)}
						<path d={lineGen(s.points) ?? ''} fill="none" stroke={PALETTE[i % PALETTE.length]} stroke-width="2" />
						{#each s.points.filter((p) => p.value !== null) as p (p.time.getTime())}
							<circle cx={xScale(p.time)} cy={yScale(p.value ?? 0)} r="3" fill={PALETTE[i % PALETTE.length]} />
						{/each}
					{/each}
				{/if}

				<!-- hover crosshair -->
				{#if hovered}
					<line x1={hovered.x} x2={hovered.x} y1="0" y2={innerHeight} class="crosshair" />
				{/if}
			</g>
		</svg>

		{#if hovered}
			<div
				class="tooltip"
				style:left="{Math.min(hovered.x + margin.left + 12, width - 180)}px"
			>
				<div class="tooltip-time">{hovered.time.toLocaleString()}</div>
				{#each hovered.entries as entry (entry.label)}
					<div class="tooltip-row">
						<span class="dot" style:background={entry.color}></span>
						<span class="tooltip-label">{entry.label}</span>
						<span class="tooltip-value">
							{entry.value === null ? '—' : `${valueFormat(entry.value)}${unit}`}
						</span>
					</div>
				{/each}
			</div>
		{/if}

		<div class="legend">
			{#each series as s, i (s.label)}
				{#if onSeriesClick}
					<button type="button" class="legend-item clickable" onclick={() => onSeriesClick?.(s.label)}>
						<span class="dot" style:background={PALETTE[i % PALETTE.length]}></span>
						{s.label}
					</button>
				{:else}
					<span class="legend-item">
						<span class="dot" style:background={PALETTE[i % PALETTE.length]}></span>
						{s.label}
					</span>
				{/if}
			{/each}
		</div>
	{/if}
</div>

<style>
	.chart {
		position: relative;
		width: 100%;
	}
	.empty {
		display: flex;
		align-items: center;
		justify-content: center;
		color: var(--text-dim);
		font-size: 0.9rem;
	}
	svg {
		display: block;
		width: 100%;
	}
	.grid {
		stroke: var(--border);
		stroke-dasharray: 2 2;
	}
	.axis-label {
		font-size: 0.7rem;
		fill: var(--text-dim);
	}
	.crosshair {
		stroke: var(--text-dim);
		stroke-dasharray: 3 3;
	}
	.tooltip {
		position: absolute;
		top: 8px;
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: 8px;
		padding: 0.5rem 0.65rem;
		font-size: 0.78rem;
		pointer-events: none;
		box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
		min-width: 150px;
	}
	.tooltip-time {
		color: var(--text-dim);
		margin-bottom: 0.25rem;
	}
	.tooltip-row {
		display: flex;
		align-items: center;
		gap: 0.4rem;
	}
	.tooltip-label {
		flex: 1;
	}
	.legend {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		margin-top: 0.5rem;
		font-size: 0.78rem;
		color: var(--text-dim);
	}
	.legend-item {
		display: flex;
		align-items: center;
		gap: 0.35rem;
	}
	.legend-item.clickable {
		background: none;
		border: none;
		color: var(--text-dim);
		font: inherit;
		cursor: pointer;
		padding: 0;
	}
	.legend-item.clickable:hover {
		color: var(--text);
		text-decoration: underline;
	}
	.dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		display: inline-block;
	}
</style>
