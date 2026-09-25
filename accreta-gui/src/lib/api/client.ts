import { auth } from '$lib/stores/auth.svelte';
import { API_BASE_URL } from '$lib/api/config';
import type {
	ErrorBody,
	HealthResponse,
	SchemaRequest,
	SchemaResponse,
	IngestRequest,
	IngestResponse,
	QueryRequest,
	QueryResponse
} from '$lib/api/schema';

export class ApiError extends Error {
	constructor(
		public status: number,
		public body: ErrorBody
	) {
		super(body.detail);
	}
}

async function request<T>(path: string, init: RequestInit = {}, authenticated = true): Promise<T> {
	const headers = new Headers(init.headers);
	headers.set('content-type', 'application/json');
	if (authenticated) {
		if (!auth.token) {
			auth.forceReauth();
			throw new Error('Not authenticated');
		}
		headers.set('authorization', `Bearer ${auth.token}`);
	}

	const res = await fetch(`${API_BASE_URL}${path}`, { ...init, headers });

	if (res.status === 401 && authenticated) {
		// Every kind of auth failure collapses to this one status server-side (see auth.rs) — the
		// only client-side response that makes sense is the same for all of them: back to login.
		auth.forceReauth();
		throw new Error('Session expired');
	}

	if (!res.ok) {
		const body: ErrorBody = await res.json().catch(() => ({
			error: 'unknown',
			detail: `Request failed with status ${res.status}`
		}));
		throw new ApiError(res.status, body);
	}

	// 200s with no body (none exist today, but cheap to guard) fall through to `undefined as T`.
	return res.status === 204 ? (undefined as T) : ((await res.json()) as T);
}

// health.ts is intentionally unauthenticated (see health.rs's own doc comment) — reachable even
// pre-login, which is what lets the login screen show "backend unreachable" before anyone types
// credentials.
export const health = () => request<HealthResponse>('/health', {}, false);

export const getSchema = () => request<SchemaResponse>('/schema');

export const createSchema = (body: SchemaRequest) =>
	request<SchemaResponse>('/schema', { method: 'POST', body: JSON.stringify(body) });

export const ingest = (body: IngestRequest) =>
	request<IngestResponse>('/schema/ingest', { method: 'POST', body: JSON.stringify(body) });

export const query = (body: QueryRequest) =>
	request<QueryResponse>('/schema/query', { method: 'POST', body: JSON.stringify(body) });
