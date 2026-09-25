<script lang="ts">
	interface Row {
		label: string;
		value: number;
		pct: number;
	}
	interface Props {
		title: string;
		rows: Row[];
		onRowClick?: (label: string) => void;
	}
	let { title, rows, onRowClick }: Props = $props();
</script>

<div class="breakdown">
	<h3>{title}</h3>
	{#if rows.length === 0}
		<p class="empty">No data yet.</p>
	{:else}
		{#each rows as row (row.label)}
			{#if onRowClick}
				<button type="button" class="row clickable" onclick={() => onRowClick?.(row.label)}>
					<span class="row-label">{row.label}</span>
					<span class="row-bar"><span class="row-fill" style:width="{row.pct}%"></span></span>
					<span class="row-pct">{row.pct.toFixed(0)}%</span>
				</button>
			{:else}
				<div class="row">
					<span class="row-label">{row.label}</span>
					<span class="row-bar"><span class="row-fill" style:width="{row.pct}%"></span></span>
					<span class="row-pct">{row.pct.toFixed(0)}%</span>
				</div>
			{/if}
		{/each}
	{/if}
</div>

<style>
	.breakdown {
		display: flex;
		flex-direction: column;
		gap: 0.4rem;
	}
	h3 {
		margin: 0 0 0.25rem;
		font-size: 0.85rem;
		color: var(--text-dim);
	}
	.empty {
		font-size: 0.8rem;
		color: var(--text-dim);
		margin: 0;
	}
	.row {
		display: grid;
		grid-template-columns: 90px 1fr 40px;
		align-items: center;
		gap: 0.6rem;
		background: none;
		border: none;
		padding: 0.15rem 0;
		font: inherit;
		color: inherit;
		width: 100%;
		text-align: left;
	}
	.row.clickable {
		cursor: pointer;
		border-radius: 6px;
		transition: background 0.15s ease;
	}
	.row.clickable:hover {
		background: var(--accent-soft);
	}
	.row-label {
		font-size: 0.82rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.row-bar {
		height: 6px;
		background: var(--bg);
		border-radius: 999px;
		overflow: hidden;
	}
	.row-fill {
		display: block;
		height: 100%;
		background: var(--accent);
		border-radius: 999px;
	}
	.row-pct {
		font-size: 0.78rem;
		color: var(--text-dim);
		text-align: right;
		font-family: var(--font-mono);
	}
</style>
