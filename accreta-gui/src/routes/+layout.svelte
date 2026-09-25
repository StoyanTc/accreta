<script lang="ts">
	import favicon from '$lib/assets/favicon.svg';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { auth } from '$lib/stores/auth.svelte';
	import '../app.css';

	let { children } = $props();

	const navItems = [
		{ href: '/dashboard', label: 'Dashboard' },
		{ href: '/explorer', label: 'Explorer' },
		{ href: '/aggregates', label: 'Aggregates' },
		{ href: '/about', label: 'About' }
	];

	// Route guard: every page except /login requires a session. This runs on every navigation
	// (page.url.pathname changing) rather than once at mount, since SvelteKit reuses the layout
	// across client-side navigations.
	$effect(() => {
		if (!auth.isAuthenticated && page.url.pathname !== '/login') {
			goto('/login');
		}
	});
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{#if page.url.pathname !== '/login'}
	<div class="shell">
		<header>
			<span class="brand">ACCRETA</span>
			<nav>
				{#each navItems as item (item.href)}
					<a href={item.href} class:active={page.url.pathname.startsWith(item.href)}>
						{item.label}
					</a>
				{/each}
			</nav>
			<button class="logout" onclick={() => auth.logout()}>Log out</button>
		</header>
		<main>
			{@render children()}
		</main>
	</div>
{:else}
	{@render children()}
{/if}
