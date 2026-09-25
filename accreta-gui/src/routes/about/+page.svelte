<div class="about">
	<div class="card">
		<h1>About accreta</h1>
		<p>
			<strong>accreta</strong> is a Rust crate implementing a mergeable-state aggregation
			engine for time-series data. Rather than storing raw events and re-scanning them on
			every query, it keeps <em>aggregate states</em> — running counts, sums, min/max,
			averages, and t-digest sketches for percentiles — that can be merged with each other
			commutatively and associatively. That property is what makes hierarchical rollups
			possible: a day's aggregate is just the merge of that day's hourly aggregates, a
			week's is the merge of its days, and so on, with no need to revisit the original
			samples at any level above where they were first ingested.
		</p>
	</div>

	<div class="card">
		<h2>Why rollups, and why they branch</h2>
		<p>
			Every ingested sample lands at the finest level (<code>Second</code>). A background
			sweep in accreta-metrics periodically merges each level up into the next, following
			accreta's own rollup hierarchy — which is a small DAG, not a straight line:
			<code>Second → Minute → Hour → Day</code>, and then <code>Day</code> feeds
			<strong>both</strong> <code>Week</code> and <code>Month</code> independently (with
			<code>Month</code> feeding <code>Year</code>). <code>Week</code> is a terminal level —
			nothing rolls up from it. See the <a href="/aggregates">Aggregates</a> page for a
			diagram and the raw aggregate states themselves.
		</p>
		<p>
			The practical upshot: switching this GUI's resolution control from "1 hour" to "1 day"
			isn't just re-sampling the same chart — it's querying an entirely different, separately
			maintained aggregate, computed once and reused, rather than a live re-aggregation of
			raw data every time.
		</p>
	</div>

	<div class="card">
		<h2>Why percentiles need a sketch, not just an average</h2>
		<p>
			Count, sum, min, max, and average all merge trivially — add two counts, add two
			sums. Percentiles don't: you cannot compute "the P95 of two buckets" from each
			bucket's own P95 alone. accreta's <code>TDigest</code> aggregate is a compact sketch
			structure that <em>does</em> merge correctly, which is what lets P50/P90/P95/P99
			latency lines exist at every rollup level, not just at raw-sample resolution — see the
			percentile chart on the <a href="/dashboard">Dashboard</a>.
		</p>
	</div>

	<div class="card">
		<h2>This GUI</h2>
		<p>
			A Svelte SPA talking to <code>accreta-metrics</code>, a reference HTTP service built on
			axum that exposes accreta over a small JSON API (schema definition, ingestion, and
			rollup queries). Every control in this GUI — measures, dimensions, which aggregates are
			available — is read from that service's live schema at runtime; nothing about your
			specific metrics or dimension names is hardcoded here.
		</p>
	</div>
</div>

<style>
	.about {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		max-width: 720px;
	}
	h1 {
		margin: 0 0 0.5rem;
		font-size: 1.3rem;
	}
	h2 {
		margin: 0 0 0.5rem;
		font-size: 1rem;
	}
	p {
		line-height: 1.6;
		color: var(--text-dim);
	}
	code {
		background: var(--bg);
		border: 1px solid var(--border);
		border-radius: 4px;
		padding: 0.1em 0.35em;
		font-size: 0.9em;
	}
	a {
		color: var(--accent);
	}
</style>
