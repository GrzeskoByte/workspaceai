import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { homedir } from 'node:os';

// Dev: proxy /api to the supervisor started by `npm run dev`, adding its per-launch token.
// scripts/dev.mjs starts Vite only after HARNESS_READY, so this file exists when the config loads.
// http-proxy 3 (Vite 8) ignores a `router` function, so the port is read here, not per request.
function supervisor(): { port: number; token: string } {
  const home = process.env.HARNESS_HOME || join(homedir(), '.harness');
  try { return JSON.parse(readFileSync(join(home, 'supervisor.json'), 'utf8')); } catch { return { port: 4319, token: '' }; }
}

const supervisorAtStart = supervisor();

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    proxy: {
      '/api': {
        target: `http://127.0.0.1:${supervisorAtStart.port}`,
        changeOrigin: false,
        configure: (proxy) => {
          proxy.on('proxyReq', (req) => { req.setHeader('x-harness-token', supervisor().token); });
        },
        rewrite: (p) => p.replace(/([?&])token=[^&]*/, `$1token=${encodeURIComponent(supervisor().token)}`),
      },
    },
  },
  build: { outDir: 'dist', emptyOutDir: true, chunkSizeWarningLimit: 4000 },
});
