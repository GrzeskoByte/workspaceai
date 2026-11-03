// Computers (R-28): pairing with a short code, Wake-on-LAN, and a live view of work running on a computer
// that can be stopped or taken over.
import { createSocket } from 'node:dgram';
import { randomInt } from 'node:crypto';
import type { Computer, LiveSession } from '../../shared/src/types.ts';
import { uid } from '../../shared/src/util.ts';
import type { Supervisor } from './supervisor.ts';
import { pauseGraphRun, resumeGraphRun } from './graphs.ts';

const codes = new Map<string, { expires: number }>();
const ALPHABET = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789';

/** A short code the other computer types into its desktop app. Valid for 10 minutes, single use. */
export function newPairingCode(now = Date.now()): { code: string; expires: number } {
  for (const [c, v] of codes) if (v.expires < now) codes.delete(c);
  const code = Array.from({ length: 6 }, () => ALPHABET[randomInt(ALPHABET.length)]).join('');
  const expires = now + 10 * 60000;
  codes.set(code, { expires });
  return { code: code.slice(0, 3) + '-' + code.slice(3), expires };
}

/** The other computer redeems the code. It joins with no workspace access until the user allows one. */
export function redeemPairingCode(sup: Supervisor, code: string, info: { name: string; os?: string; mac?: string }): Computer {
  const key = code.replace(/[^A-Z0-9]/gi, '').toUpperCase();
  const entry = codes.get(key);
  if (!entry || entry.expires < Date.now()) throw Object.assign(new Error('That pairing code is wrong or has expired'), { status: 403 });
  codes.delete(key);
  const c: Computer = {
    id: uid('pc'), name: (info.name || 'Computer').trim().slice(0, 80), os: info.os || 'unknown', status: 'online', lastSeen: Date.now(),
    capabilities: { files: true, commands: false, browser: false, computerUse: false, wake: !!info.mac },
    allowedApps: [], askFirst: { deleteFiles: true, sendMessages: true, purchases: true, installSoftware: true, systemSettings: true, openApps: true },
    access: {}, activity: [{ at: Date.now(), text: 'Paired with a code; no workspace may use it yet' }], mac: info.mac, live: null,
  };
  sup.computers.push(c);
  sup.saveComputers();
  return c;
}

/** Wake-on-LAN magic packet: 6×0xFF then the MAC 16 times, broadcast to UDP port 9. */
export function magicPacket(mac: string): Buffer {
  const hex = mac.replace(/[^0-9a-f]/gi, '');
  if (hex.length !== 12) throw new Error('A MAC address looks like 00:11:22:33:44:55');
  const m = Buffer.from(hex, 'hex');
  return Buffer.concat([Buffer.alloc(6, 0xff), ...Array.from({ length: 16 }, () => m)]);
}

export async function sendWake(mac: string, address = '255.255.255.255'): Promise<void> {
  const pkt = magicPacket(mac);
  const sock = createSocket('udp4');
  await new Promise<void>((resolve, reject) => sock.bind(() => { sock.setBroadcast(true); sock.send(pkt, 9, address, (e) => { sock.close(); e ? reject(e) : resolve(); }); }));
}

function computerOf(sup: Supervisor, id: string): Computer {
  const c = sup.computers.find((x) => x.id === id);
  if (!c) throw Object.assign(new Error('Computer not found'), { status: 404 });
  return c;
}

function log(sup: Supervisor, c: Computer, text: string): void {
  c.activity = [{ at: Date.now(), text }, ...c.activity].slice(0, 60);
  if (c.live) c.live.steps = [{ at: Date.now(), text }, ...c.live.steps].slice(0, 30);
  sup.saveComputers();
}

/** Work started on a computer appears in its live view. */
export function startLive(sup: Supervisor, computerId: string, s: Omit<LiveSession, 'controlledBy' | 'state' | 'startedAt' | 'steps'>): void {
  const c = computerOf(sup, computerId);
  c.live = { ...s, controlledBy: 'agent', state: 'running', startedAt: Date.now(), steps: [] };
  log(sup, c, `Started: ${s.title} — ${s.action}`);
}

export function updateLive(sup: Supervisor, computerId: string, action: string): void {
  const c = sup.computers.find((x) => x.id === computerId);
  if (!c?.live || c.live.state === 'stopped') return;
  c.live.action = action;
  log(sup, c, action);
}

/** Stop ends the work; Take over pauses it and hands the screen to you; Hand back resumes it. */
export async function controlLive(sup: Supervisor, computerId: string, op: 'stop' | 'takeover' | 'handback'): Promise<Computer> {
  const c = computerOf(sup, computerId);
  const live = c.live;
  if (!live || live.state === 'stopped') throw new Error('Nothing is running on this computer');
  const rt = sup.rt(live.workspace);
  if (op === 'stop') {
    if (live.ref.kind === 'graph') await pauseGraphRun(rt, live.ref.id).catch(() => {});
    else await rt.stop(live.ref.id).catch(() => {});
    live.state = 'stopped';
    log(sup, c, 'Stopped by you');
  } else if (op === 'takeover') {
    if (live.ref.kind === 'graph') await pauseGraphRun(rt, live.ref.id).catch(() => {});
    else await rt.stop(live.ref.id).catch(() => {});
    live.state = 'paused'; live.controlledBy = 'you';
    log(sup, c, 'You took over; the agent is paused');
  } else {
    if (live.ref.kind === 'graph') await resumeGraphRun(rt, live.ref.id).catch(() => {});
    live.state = 'running'; live.controlledBy = 'agent';
    log(sup, c, 'Handed back to the agent');
  }
  rt.audit('computer.' + op, `${c.name} · ${live.title}`);
  return c;
}
