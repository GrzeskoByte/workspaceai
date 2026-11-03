// Entry point. Prints one ready line (`HARNESS_READY {"port":…,"token":…}`) that the Tauri shell reads.
// Stops every workspace sidecar on SIGINT/SIGTERM or when the parent closes stdin (R-42).
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Supervisor } from './supervisor.ts';
import { startServer } from './server.ts';
import { buildRouter } from './routes.ts';
import { installExtensions } from './extensions.ts';
import { harnessHome } from './paths.ts';
import { writeJson } from './jsonfile.ts';
import { recoverGraphRuns, stopGraphRuns } from './graphs.ts';
import { Scheduler } from './scheduler.ts';

const here = dirname(fileURLToPath(import.meta.url));
const staticDir = process.env.HARNESS_UI_DIR || join(here, '../../../apps/ui/dist');

const sup = new Supervisor();
const router = buildRouter();
const port = Number(process.env.HARNESS_PORT || 0);
const server = await startServer(sup, router, { port, token: process.env.HARNESS_TOKEN, staticDir });
installExtensions(sup, router, server);
await recoverGraphRuns(sup);
const scheduler = new Scheduler(sup);
if (!process.env.HARNESS_NO_SCHEDULER) scheduler.start();

if (process.env.HARNESS_DEV) writeJson(join(harnessHome(), 'supervisor.json'), { port: server.port, token: server.token, pid: process.pid });
process.stdout.write(`HARNESS_READY ${JSON.stringify({ port: server.port, token: server.token, home: harnessHome() })}\n`);

let stopping = false;
async function shutdown(code = 0) {
  if (stopping) return;
  stopping = true;
  scheduler.stop();
  await stopGraphRuns();
  await sup.shutdown();
  await server.close();
  process.exit(code);
}
process.on('SIGINT', () => shutdown(0));
process.on('SIGTERM', () => shutdown(0));
if (process.env.HARNESS_PARENT_STDIN) { process.stdin.resume(); process.stdin.on('end', () => shutdown(0)); process.stdin.on('close', () => shutdown(0)); }
