<script lang="ts">
	import { auth } from '$lib/stores/auth.svelte';
	import { health } from '$lib/api/client';
	import { onMount } from 'svelte';

	let username = $state('demo');
	let password = $state('');
	let error = $state<string | null>(null);
	let submitting = $state(false);

	// Backend reachability, checked via the unauthenticated /health endpoint before anyone even
	// types credentials — this is the whole reason /health exists unauthenticated.
	let backendStatus = $state<'checking' | 'up' | 'down'>('checking');
	let schemaPresent = $state(false);

	onMount(async () => {
		try {
			const res = await health();
			backendStatus = 'up';
			schemaPresent = res.schema_present;
		} catch {
			backendStatus = 'down';
		}
	});

	async function submit(e: SubmitEvent) {
		e.preventDefault();
		error = null;
		submitting = true;
		try {
			await auth.login({ username, password });
		} catch (e) {
			error = e instanceof Error ? e.message : 'Login failed';
		} finally {
			submitting = false;
		}
	}
</script>

<div class="login-page">
	<form class="card" onsubmit={submit}>
		<h1>ACCRETA</h1>

		{#if backendStatus === 'down'}
			<p class="status status-down">Backend unreachable — check the API is running.</p>
		{:else if backendStatus === 'up' && !schemaPresent}
			<p class="status status-warn">
				Backend is up, but no schema exists yet. You'll need to seed one after logging in.
			</p>
		{/if}

		<label>
			Username
			<input bind:value={username} autocomplete="username" />
		</label>
		<label>
			Password
			<input type="password" bind:value={password} autocomplete="current-password" />
		</label>

		{#if error}
			<p class="status status-down">{error}</p>
		{/if}

		<button type="submit" disabled={submitting || backendStatus === 'down'}>
			{submitting ? 'Logging in…' : 'Log in'}
		</button>
	</form>
</div>

<style>
	.login-page {
		min-height: 100vh;
		display: flex;
		align-items: center;
		justify-content: center;
	}
	form {
		width: 320px;
		display: flex;
		flex-direction: column;
		gap: 0.75rem;
	}
	h1 {
		margin: 0 0 0.5rem;
		letter-spacing: 0.08em;
		font-size: 1.1rem;
	}
	label {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
		font-size: 0.85rem;
		color: var(--text-dim);
	}
	input {
		padding: 0.5rem;
		border-radius: 6px;
		border: 1px solid var(--border);
		background: var(--bg);
		color: var(--text);
	}
	button {
		margin-top: 0.5rem;
		padding: 0.6rem;
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
	.status {
		font-size: 0.8rem;
		margin: 0;
	}
	.status-down {
		color: #e15b5b;
	}
	.status-warn {
		color: #e1a05b;
	}
</style>
