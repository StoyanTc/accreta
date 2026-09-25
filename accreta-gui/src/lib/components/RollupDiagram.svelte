<script lang="ts">
	// A hand-laid-out diagram of accreta's real rollup hierarchy — deliberately not a generic
	// tree-rendering component, since there's exactly one hierarchy to show and it has one
	// structural quirk worth making visually obvious: Day feeds both Week and Month
	// independently (Week is terminal; Month feeds Year). Second is the current finest level —
	// raw samples fold into Second buckets, not Minute.
	const nodes = [
		{ id: 'second', label: 'Second', x: 60, y: 20 },
		{ id: 'minute', label: 'Minute', x: 60, y: 100 },
		{ id: 'hour', label: 'Hour', x: 60, y: 180 },
		{ id: 'day', label: 'Day', x: 60, y: 260 },
		{ id: 'week', label: 'Week', x: 0, y: 340 },
		{ id: 'month', label: 'Month', x: 130, y: 340 },
		{ id: 'year', label: 'Year', x: 130, y: 420 }
	];
	const edges: [string, string][] = [
		['second', 'minute'],
		['minute', 'hour'],
		['hour', 'day'],
		['day', 'week'],
		['day', 'month'],
		['month', 'year']
	];

	function node(id: string) {
		return nodes.find((n) => n.id === id)!;
	}
</script>

<svg viewBox="-20 0 260 460" role="img" aria-label="accreta rollup hierarchy diagram">
	{#each edges as [from, to] (from + to)}
		{@const a = node(from)}
		{@const b = node(to)}
		<line x1={a.x + 40} y1={a.y + 20} x2={b.x + 40} y2={b.y} class="edge" marker-end="url(#arrow)" />
	{/each}

	<defs>
		<marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
			<path d="M0,0 L10,5 L0,10 z" class="arrowhead" />
		</marker>
	</defs>

	{#each nodes as n (n.id)}
		<g transform="translate({n.x},{n.y})">
			<rect width="80" height="40" rx="8" class="node" />
			<text x="40" y="24" text-anchor="middle" class="node-label">{n.label}</text>
		</g>
	{/each}
</svg>

<style>
	svg {
		width: 100%;
		max-width: 320px;
		height: auto;
	}
	.node {
		fill: var(--surface);
		stroke: var(--accent);
		stroke-width: 1.5;
	}
	.node-label {
		fill: var(--text);
		font-size: 13px;
	}
	.edge {
		stroke: var(--text-dim);
		stroke-width: 1.5;
	}
	.arrowhead {
		fill: var(--text-dim);
	}
</style>
