// Dev: runs the supervisor and restarts it when its sources change. Replaces `node --watch`, which on Linux watches
// each file's inode: a save that writes a temp file and renames it over (most editors and agents) triggers one restart
// and then nothing more, leaving the running code behind the sources. Watching the folders sees every save.
import { spawn } from 'node:child_process';
import { watch } from 'node:fs';

const entry = process.argv[2];
const FOLDERS = ['packages/supervisor/src', 'packages/shared/src'];
let child = null;
let stopping = false;
let timer;

function start() {
  // Stdin is shared, so the child also sees the app's pipe close.
  child = spawn(process.execPath, [entry], { stdio: 'inherit' });
  child.once('exit', () => {
    child = null;
    if (stopping) process.exit(0);
    if (restart) { restart = false; start(); }
  });
}

let restart = false;
function changed(_event, file) {
  if (!file || !/\.ts$/.test(file)) return;
  clearTimeout(timer);
  // A save can be several writes, and an edit several saves.
  timer = setTimeout(() => {
    if (!child) { start(); return; }
    restart = true;
    child.kill('SIGTERM');
    const dying = child;
    setTimeout(() => { if (dying.exitCode === null && dying.signalCode === null) dying.kill('SIGKILL'); }, 8000).unref();
  }, 300);
}

function stop() {
  if (stopping) return;
  stopping = true;
  if (!child) process.exit(0);
  child.kill('SIGTERM');
}

// ponytail: not recursive, since node's recursive mode on Linux goes deaf the same way; both folders are flat. A subfolder needs its own entry above.
for (const f of FOLDERS) watch(f, changed);
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
// The app going away, however it went, closes this pipe.
process.stdin.resume();
process.stdin.on('end', stop);
process.stdin.on('close', stop);
start();
