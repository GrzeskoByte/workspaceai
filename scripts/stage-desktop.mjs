// Stages what the packaged desktop app runs (R-42): the supervisor, orchestrator, shared and MCP packages with their
// production dependencies under src-tauri/resources/app, and this machine's Node as the `node` sidecar.
// Run by `tauri build` through beforeBuildCommand; safe to run again (it starts from a clean folder).
import { cpSync, mkdirSync, rmSync, writeFileSync, copyFileSync, chmodSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const tauri = join(root, 'apps/desktop/src-tauri');
const app = join(tauri, 'resources/app');

rmSync(app, { recursive: true, force: true });
mkdirSync(app, { recursive: true });
for (const pkg of ['shared', 'supervisor', 'orchestrator', 'memory-mcp']) {
  cpSync(join(root, 'packages', pkg, 'src'), join(app, 'packages', pkg, 'src'), { recursive: true });
  copyFileSync(join(root, 'packages', pkg, 'package.json'), join(app, 'packages', pkg, 'package.json'));
}
cpSync(join(root, 'design-systems'), join(app, 'design-systems'), { recursive: true });
const rootPkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
writeFileSync(join(app, 'package.json'), JSON.stringify({ name: 'harness-runtime', version: rootPkg.version, private: true, type: 'module', workspaces: ['packages/*'], engines: rootPkg.engines }, null, 2));

// Production dependencies only; workspace links become real copies so the bundle has no symlinks out of itself.
execFileSync('npm', ['install', '--omit=dev', '--no-audit', '--no-fund', '--install-links', '--prefer-offline'], { cwd: app, stdio: 'inherit' });

// The sources import each other by relative path; drop npm's workspace links, which point outside the bundle.
rmSync(join(app, 'node_modules/@harness'), { recursive: true, force: true });

// Node with type stripping runs the .ts sources as they are; the sidecar name carries the target triple.
const triple = execFileSync('rustc', ['-vV']).toString().match(/host: (\S+)/)[1];
const bin = join(tauri, 'binaries', `node-${triple}`);
mkdirSync(dirname(bin), { recursive: true });
copyFileSync(process.execPath, bin);
chmodSync(bin, 0o755);
console.log(`staged ${app} and ${bin}`);
