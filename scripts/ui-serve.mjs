// Serves the UI as a production build for the desktop shell: React's production build is several times faster
// than the dev server's (see docs/PROGRESS.md, performance). Builds once, then rebuilds on change and serves dist/
// on the dev URL. Reload the window to pick up a rebuild; `npm run desktop:hot` keeps the hot-reloading dev server.
import { spawn, spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ui = join(dirname(fileURLToPath(import.meta.url)), '..', 'apps', 'ui');
const port = process.env.HARNESS_UI_PORT || '5173';
const vite = (args, opts = {}) => spawn('npx', ['vite', ...args], { cwd: ui, stdio: 'inherit', ...opts });

const first = spawnSync('npx', ['vite', 'build', '--logLevel', 'warn'], { cwd: ui, stdio: 'inherit' });
if (first.status !== 0) process.exit(first.status || 1);
const children = [
  vite(['build', '--watch', '--logLevel', 'warn']),
  vite(['preview', '--host', '127.0.0.1', '--port', port, '--strictPort']),
];
const stop = () => { for (const c of children) c.kill('SIGTERM'); process.exit(0); };
process.on('SIGINT', stop); process.on('SIGTERM', stop);
for (const c of children) c.on('exit', (code) => { if (code) stop(); });
