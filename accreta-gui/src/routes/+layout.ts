// This is a pure client-side SPA talking to accreta-metrics over HTTP — there's no server to
// render on, and no reason to prerender pages whose whole content is a live query result.
// adapter-static's `fallback: 'index.html'` mode requires ssr disabled for exactly this reason.
export const ssr = false;
export const prerender = false;
