// Starts the supervisor, then the Vite UI once the supervisor prints HARNESS_READY.
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';

const sup = spawn(process.execPath, ['packages/supervisor/src/main.ts'], {
  env: { ...process.env, HARNESS_DEV: '1' },
  stdio: ['ignore', 'pipe', 'inherit'],
});

let ui;
const lines = createInterface({ input: sup.stdout });
lines.on('line', (line) => {
  process.stdout.write(line + '\n');
  if (!ui && line.startsWith('HARNESS_READY')) {
    ui = spawn('npm', ['--workspace', 'apps/ui', 'run', 'dev'], { stdio: 'inherit' });
  }
});

function stop() {
  ui?.kill('SIGINT');
  sup.kill('SIGINT');
}
process.on('SIGINT', stop);
process.on('SIGTERM', stop);
sup.on('exit', (code) => { ui?.kill('SIGINT'); process.exit(code ?? 0); });
