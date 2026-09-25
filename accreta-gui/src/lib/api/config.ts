// Set VITE_API_BASE_URL in a .env file (or your hosting platform's env config) to point this at
// a running accreta-metrics instance. Falls back to localhost for local dev against `cargo run`.
export const API_BASE_URL = import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:8080';
