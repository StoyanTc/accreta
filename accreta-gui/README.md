# accreta-gui

Svelte SPA for [accreta-metrics](../accreta-metrics) — a Rust/axum service
exposing the `accreta` mergeable-state aggregation engine over HTTP.

## Prerequisites

- An accreta-metrics instance running and reachable (default `http://localhost:8080`,
  started with `cargo run` from that project). This GUI is a pure client — it
  has nothing to show without a running backend to query.
- Node 18+.

## 1. Install dependencies

```sh
npm install
```

## 2. Point the GUI at your backend (optional)

Only needed if accreta-metrics isn't running on the default
`http://localhost:8080`. Create a `.env` file in this directory:

```
VITE_API_BASE_URL=http://your-host:port
```

## 3. Run it

```sh
npm run dev
```

Open the printed URL. You'll land on `/login`:

- If accreta-metrics isn't reachable, the login screen tells you so via a
  `GET /health` check — before you even type anything.
- Log in with accreta-metrics's seeded demo credentials (`demo` / `demo123`
  unless you set `ACCRETA_METRICS_USERNAME` / `ACCRETA_METRICS_PASSWORD` when
  starting the backend).
- If `/health` reports no schema exists yet on the backend, you'll see a
  warning on the login screen — accreta-metrics needs a `POST /schema` (and
  some ingested data) before there's anything to query. There's no seed
  script in this GUI yet; that's a planned "Seed demo data" action.

Session note: the JWT is kept in `sessionStorage` (survives a page refresh),
but the credentials used for the automatic silent re-login before expiry are
kept in memory only — so a page refresh always requires logging in again,
even mid-session.

## Building for deployment

```sh
npm run build
```

This project is already configured with `@sveltejs/adapter-static`
(see `vite.config.ts`) — no adapter installation needed. `npm run build`
outputs plain static files to `build/`, deployable to any static host.
Preview the build locally with `npm run preview`.

## Project layout

```
src/
├── lib/
│   ├── api/
│   │   ├── config.ts   # API_BASE_URL — reads VITE_API_BASE_URL, defaults to localhost:8080
│   │   ├── schema.ts   # TS types mirroring accreta-metrics's OpenAPI schema (hand-written for
│   │   │                 now — see the TODO at the top of the file)
│   │   └── client.ts   # fetch wrapper: auth header injection, 401 handling, one function per endpoint
│   └── stores/
│       └── auth.svelte.ts   # session state, proactive silent re-login, logout
└── routes/
    ├── login/            # health check + credentials form
    ├── dashboard/        # main monitoring view (placeholder)
    ├── explorer/         # free-form query builder (placeholder)
    ├── aggregates/       # rollup hierarchy + aggregate inspector (placeholder)
    └── about/            # placeholder
```
