import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},

			// This is a pure client-side SPA against accreta-metrics's HTTP API — no server-side
			// rendering or Node runtime needed, so adapter-static builds it to plain static files
			// (npm run build -> ./build) that can be served from anywhere (a CDN, a static
			// file server, or `vite preview`).
			adapter: adapter({
				fallback: 'index.html' // SPA fallback: every route resolves client-side
			})
		})
	]
});
