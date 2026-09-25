import { goto } from '$app/navigation';
import type { LoginRequest, LoginResponse } from '$lib/api/schema';
import { API_BASE_URL } from '$lib/api/config';

// Re-login this many ms before the token's actual expiry, so a request mid-flight never races
// the token going stale. Comfortably inside the server's 30-minute TTL.
const REFRESH_MARGIN_MS = 2 * 60 * 1000;

class AuthStore {
	token = $state<string | null>(sessionStorage.getItem('accreta_token'));
	expiresAt = $state<string | null>(sessionStorage.getItem('accreta_expires_at'));

	// Kept in memory only (never persisted) so a silent proactive re-login survives the normal
	// 30-minute session without re-prompting, but a page refresh always requires typing the
	// (seeded demo) credentials again rather than stashing them in sessionStorage indefinitely.
	#credentials: LoginRequest | null = null;
	#refreshTimer: ReturnType<typeof setTimeout> | null = null;

	get isAuthenticated() {
		return this.token !== null;
	}

	async login(credentials: LoginRequest) {
		const res = await fetch(`${API_BASE_URL}/login`, {
			method: 'POST',
			headers: { 'content-type': 'application/json' },
			body: JSON.stringify(credentials)
		});

		if (!res.ok) {
			const body = await res.json().catch(() => null);
			throw new Error(body?.detail ?? 'Login failed');
		}

		const data: LoginResponse = await res.json();
		this.#credentials = credentials;
		this.#setSession(data);
		await goto('/dashboard');
	}

	logout() {
		this.#credentials = null;
		this.#clearSession();
		goto('/login');
	}

	/** Called by the fetch wrapper on any 401 — the credentials it was silently refreshing with
	 *  either never existed (page was refreshed) or the server rejected them outright. Either
	 *  way, the only correct move is to send the person back to a real login screen. */
	forceReauth() {
		this.#clearSession();
		goto('/login');
	}

	#setSession(data: LoginResponse) {
		this.token = data.token;
		this.expiresAt = data.expires_at;
		sessionStorage.setItem('accreta_token', data.token);
		sessionStorage.setItem('accreta_expires_at', data.expires_at);
		this.#scheduleRefresh(data.expires_at);
	}

	#clearSession() {
		this.token = null;
		this.expiresAt = null;
		sessionStorage.removeItem('accreta_token');
		sessionStorage.removeItem('accreta_expires_at');
		if (this.#refreshTimer) clearTimeout(this.#refreshTimer);
	}

	#scheduleRefresh(expiresAt: string) {
		if (this.#refreshTimer) clearTimeout(this.#refreshTimer);
		if (!this.#credentials) return; // nothing to silently refresh with (e.g. after a page reload)

		const msUntilRefresh = new Date(expiresAt).getTime() - Date.now() - REFRESH_MARGIN_MS;
		this.#refreshTimer = setTimeout(
			() => {
				if (this.#credentials) this.login(this.#credentials).catch(() => this.forceReauth());
			},
			Math.max(msUntilRefresh, 0)
		);
	}
}

export const auth = new AuthStore();
