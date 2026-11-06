// Talks to the supervisor. Token sources: Tauri (invoke), injected by the supervisor's static server, or the Vite dev proxy.
import type { UiEvent } from '../../../packages/shared/src/types.ts';

declare global { interface Window { __HARNESS__?: { token: string; base?: string }; __TAURI_INTERNALS__?: unknown } }

let base = '';
let token = '';

export async function initApi(): Promise<void> {
  if (window.__TAURI_INTERNALS__) {
    const { invoke } = await import('@tauri-apps/api/core');
    // The shell starts the supervisor sidecar and hands us its port and token.
    for (let i = 0; i < 100; i++) {
      const info = await invoke<{ port: number; token: string } | null>('supervisor_info').catch(() => null);
      if (info) { base = `http://127.0.0.1:${info.port}`; token = info.token; return; }
      await new Promise((r) => setTimeout(r, 150));
    }
    throw new Error('The supervisor did not start');
  }
  if (window.__HARNESS__) { token = window.__HARNESS__.token; base = window.__HARNESS__.base || ''; return; }
  token = 'dev-proxy'; // the Vite proxy adds the real token
}

export class ApiError extends Error { status: number; constructor(m: string, s: number) { super(m); this.status = s; } }

export async function api<T = any>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(base + path, {
    method,
    headers: { 'x-harness-token': token, ...(body !== undefined ? { 'content-type': 'application/json' } : {}) },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  let data: any = {};
  try { data = text ? JSON.parse(text) : {}; } catch { data = { error: text }; }
  if (!res.ok) throw new ApiError(data.error || res.statusText, res.status);
  return data as T;
}

export const get = <T = any>(p: string) => api<T>('GET', p);
export const post = <T = any>(p: string, b: unknown = {}) => api<T>('POST', p, b);
export const patch = <T = any>(p: string, b: unknown) => api<T>('PATCH', p, b);
export const put = <T = any>(p: string, b: unknown) => api<T>('PUT', p, b);
export const del = <T = any>(p: string) => api<T>('DELETE', p);

/** Download a file the supervisor streams (artifact export). The dev proxy supplies the token. */
export async function download(path: string): Promise<void> {
  const res = await fetch(base + path, { headers: { 'x-harness-token': token } });
  if (!res.ok) throw new ApiError(res.statusText || 'Download failed', res.status);
  const blob = await res.blob();
  const name = /filename="([^"]+)"/.exec(res.headers.get('content-disposition') || '')?.[1] || 'export';
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url; a.download = name; a.click();
  URL.revokeObjectURL(url);
}

export function events(onEvent: (e: UiEvent) => void, onReconnect: () => void): () => void {
  let es: EventSource | null = null;
  let closed = false;
  let lost = false;
  const open = () => {
    es = new EventSource(`${base}/api/events?token=${encodeURIComponent(token)}`);
    es.onmessage = (m) => { try { onEvent(JSON.parse(m.data)); } catch { /* ignore */ } };
    es.onopen = () => { if (lost) onReconnect(); lost = false; };
    es.onerror = () => { lost = true; es?.close(); if (!closed) setTimeout(open, 1500); };
  };
  open();
  return () => { closed = true; es?.close(); };
}
