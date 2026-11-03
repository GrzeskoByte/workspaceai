// Supervisor HTTP API: loopback only, per-launch token, JSON in/out, SSE for live events.
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { randomBytes } from 'node:crypto';
import { readFileSync, existsSync, statSync } from 'node:fs';
import { join, extname, normalize } from 'node:path';
import type { Supervisor } from './supervisor.ts';
import type { UiEvent } from '../../shared/src/types.ts';

export type Handler = (ctx: Ctx) => Promise<unknown> | unknown;
export interface Ctx { req: IncomingMessage; res: ServerResponse; params: Record<string, string>; query: URLSearchParams; body: any; sup: Supervisor }

interface Route { method: string; parts: string[]; handler: Handler }

export class Router {
  routes: Route[] = [];
  add(method: string, path: string, handler: Handler): this { this.routes.push({ method, parts: path.split('/').filter(Boolean), handler }); return this; }
  get(p: string, h: Handler) { return this.add('GET', p, h); }
  post(p: string, h: Handler) { return this.add('POST', p, h); }
  patch(p: string, h: Handler) { return this.add('PATCH', p, h); }
  put(p: string, h: Handler) { return this.add('PUT', p, h); }
  del(p: string, h: Handler) { return this.add('DELETE', p, h); }
  match(method: string, path: string): { handler: Handler; params: Record<string, string> } | null {
    const segs = path.split('/').filter(Boolean);
    for (const r of this.routes) {
      if (r.method !== method || r.parts.length !== segs.length) continue;
      const params: Record<string, string> = {};
      let ok = true;
      for (let i = 0; i < segs.length; i++) {
        if (r.parts[i].startsWith(':')) params[r.parts[i].slice(1)] = decodeURIComponent(segs[i]);
        else if (r.parts[i] !== segs[i]) { ok = false; break; }
      }
      if (ok) return { handler: r.handler, params };
    }
    return null;
  }
}

const MIME: Record<string, string> = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.json': 'application/json', '.ico': 'image/x-icon' };

const ALLOWED_ORIGINS = new Set(['http://127.0.0.1:5173', 'http://localhost:5173', 'tauri://localhost', 'http://tauri.localhost', 'https://tauri.localhost']);

async function readBody(req: IncomingMessage): Promise<any> {
  const chunks: Buffer[] = [];
  let size = 0;
  for await (const c of req) { size += (c as Buffer).length; if (size > 20e6) throw Object.assign(new Error('Body too large'), { status: 413 }); chunks.push(c as Buffer); }
  if (!chunks.length) return {};
  const text = Buffer.concat(chunks).toString('utf8');
  try { return JSON.parse(text); } catch { throw Object.assign(new Error('Invalid JSON'), { status: 400 }); }
}

export interface StartedServer { port: number; token: string; close: () => Promise<void> }

export async function startServer(sup: Supervisor, router: Router, opts: { port?: number; token?: string; staticDir?: string } = {}): Promise<StartedServer> {
  const token = opts.token || randomBytes(24).toString('base64url');
  const sseClients = new Set<ServerResponse>();
  sup.on((e: UiEvent) => { const line = `data: ${JSON.stringify(e)}\n\n`; for (const c of sseClients) c.write(line); });
  const heartbeat = setInterval(() => { for (const c of sseClients) c.write(': hb\n\n'); }, 15000);
  heartbeat.unref();

  const server = createServer(async (req, res) => {
    const origin = req.headers.origin;
    if (origin && ALLOWED_ORIGINS.has(origin)) {
      res.setHeader('access-control-allow-origin', origin);
      res.setHeader('access-control-allow-headers', 'content-type, x-harness-token');
      res.setHeader('access-control-allow-methods', 'GET, POST, PATCH, PUT, DELETE, OPTIONS');
      res.setHeader('vary', 'origin');
    }
    if (req.method === 'OPTIONS') { res.writeHead(204).end(); return; }
    const url = new URL(req.url || '/', 'http://127.0.0.1');
    const path = url.pathname;

    if (!path.startsWith('/api/')) return serveStatic(res, path, opts.staticDir, token);

    const given = req.headers['x-harness-token'] || url.searchParams.get('token');
    // Webhook triggers carry a per-routine secret instead of the launch token (checked by the handler).
    // Artifact previews carry a short-lived, read-only preview token in the path (sandboxed iframes can't send headers).
    if (given !== token && !path.startsWith('/api/hooks/') && !path.startsWith('/api/preview/') && !path.startsWith('/api/shared/')) { res.writeHead(401, { 'content-type': 'application/json' }).end('{"error":"unauthorized"}'); return; }

    if (path === '/api/events') {
      res.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache', connection: 'keep-alive' });
      res.write(': connected\n\n');
      sseClients.add(res);
      req.on('close', () => sseClients.delete(res));
      return;
    }

    const m = router.match(req.method || 'GET', path);
    if (!m) { res.writeHead(404, { 'content-type': 'application/json' }).end(JSON.stringify({ error: 'No route ' + req.method + ' ' + path })); return; }
    try {
      const body = req.method === 'GET' || req.method === 'DELETE' ? {} : await readBody(req);
      const out = await m.handler({ req, res, params: m.params, query: url.searchParams, body, sup });
      if (res.writableEnded) return;
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(out ?? { ok: true }));
    } catch (e) {
      const err = e as Error & { status?: number };
      if (!res.headersSent) res.writeHead(err.status || 400, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: err.message || String(e) }));
    }
  });

  await new Promise<void>((resolve, reject) => { server.once('error', reject); server.listen(opts.port ?? 0, '127.0.0.1', () => resolve()); });
  const addr = server.address();
  const port = typeof addr === 'object' && addr ? addr.port : 0;
  return {
    port, token,
    close: () => new Promise<void>((r) => { clearInterval(heartbeat); for (const c of sseClients) c.end(); server.close(() => r()); server.closeAllConnections?.(); }),
  };
}

function serveStatic(res: ServerResponse, path: string, dir: string | undefined, token: string): void {
  if (!dir || !existsSync(dir)) { res.writeHead(404).end('UI not built. Run npm run build, or use npm run dev.'); return; }
  const safe = normalize(path).replace(/^(\.\.[/\\])+/, '');
  let file = join(dir, safe);
  if (!file.startsWith(dir) || !existsSync(file) || statSync(file).isDirectory()) file = join(dir, 'index.html');
  let body: Buffer | string = readFileSync(file);
  if (file.endsWith('index.html')) body = body.toString('utf8').replace('<head>', `<head><script>window.__HARNESS__=${JSON.stringify({ token })}</script>`);
  res.writeHead(200, { 'content-type': MIME[extname(file)] || 'application/octet-stream', 'content-security-policy': "frame-src 'self' blob: data: http: https:; object-src 'none'" }).end(body);
}
