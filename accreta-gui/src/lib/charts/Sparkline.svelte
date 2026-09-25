<script lang="ts">
	import { scaleLinear, scaleTime } from 'd3-scale';
	import { line as d3line, curveMonotoneX } from 'd3-shape';
	import { extent, max } from 'd3-array';
	import type { SeriesPoint } from '$lib/api/query-helpers';

	interface Props {
		points: SeriesPoint[];
		color?: string;
		width?: number;
		height?: number;
	}
	let { points, color = '#5b6ee1', width = 120, height = 32 }: Props = $props();

	let timeExtent = $derived(extent(points, (p) => p.time) as [Date, Date] | [undefined, undefined]);
	let maxValue = $derived(max(points, (p) => p.value ?? 0) ?? 0);

	let xScale = $derived(
		timeExtent[0] && timeExtent[1] ? scaleTime().domain(timeExtent).range([2, width - 2]) : null
	);
	let yScale = $derived(scaleLinear().domain([0, maxValue || 1]).range([height - 2, 2]));

	let path = $derived(
		xScale
			? (d3line<SeriesPoint>()
					.defined((d) => d.value !== null)
					.x((d) => xScale!(d.time))
					.y((d) => yScale(d.value ?? 0))
					.curve(curveMonotoneX)(points) ?? '')
			: ''
	);

	// A point with no defined neighbor on either side produces no visible line segment at all
	// (d3's line generator only draws *between* consecutive defined points) — mark those
	// specifically, rather than every point, so a dense sparkline doesn't get cluttered with dots.
	let isolatedPoints = $derived(
		points.filter((p, i) => {
			if (p.value === null) return false;
			const prevDefined = i > 0 && points[i - 1].value !== null;
			const nextDefined = i < points.length - 1 && points[i + 1].value !== null;
			return !prevDefined && !nextDefined;
		})
	);
</script>

<svg {width} {height} role="presentation">
	<path d={path} fill="none" stroke={color} stroke-width="1.5" />
	{#if xScale}
		{#each isolatedPoints as p (p.time.getTime())}
			<circle cx={xScale(p.time)} cy={yScale(p.value ?? 0)} r="2" fill={color} />
		{/each}
	{/if}
</svg>

<style>
	svg {
		display: block;
	}
</style>
