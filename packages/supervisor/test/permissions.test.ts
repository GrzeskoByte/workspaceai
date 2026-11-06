// Permission routing: a request belongs to the session that asked for it, which can be a subagent
// child session rather than the chat's root session. Replying to the wrong session is a 404.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const home = mkdtempSync(join(tmpdir(), 'harness-perm-'));
process.env.HARNESS_HOME = home;
process.env.HARNESS_KEYCHAIN = 'memory';
process.env.HARNESS_OD_DIR = join(home, 'no-open-design');

let sup: any;
let rt: any;
const chatId = 'chat_permissions';

interface FakeOptions {
  pending?: Array<Array<Record<string, unknown>>>;
  sessions?: Record<string, { id: string; parentID?: string }>;
  failOnce?: boolean;
}

function fakeClient(o: FakeOptions) {
  const replies: Array<[string, string, string]> = [];
  const patched: Array<[string, unknown]> = [];
  const pending = [...(o.pending || [])];
  return {
    replies,
    patched,
    async pendingPermissions() { return pending.shift() || []; },
    async permissions() { return []; },
    async listSessions() { return []; },
    async getSession(id: string) { return o.sessions?.[id] || { id }; },
    async setSessionPermissions(id: string, permissions: unknown) { patched.push([id, permissions]); },
    async replyPermission(sid: string, rid: string, decision: string) {
      replies.push([sid, rid, decision]);
      if (o.failOnce) {
        o.failOnce = false;
        const { OpencodeError } = await import('../src/opencode.ts');
        throw new OpencodeError('POST permission reply → 404', 404);
      }
    },
  };
}

beforeAll(async () => {
  const { Supervisor } = await import('../src/supervisor.ts');
  sup = new Supervisor();
  const ws = sup.createWorkspace('Permissions', 'personal');
  rt = sup.rt(ws.id);
  const general = rt.state.projects.find((p: any) => p.isGeneral);
  rt.state.chats.push({
    id: chatId, project: general.id, title: 'permissions', sid: 'ses_root', agent: 'opencode',
    model: '', web: null, instructions: '', imported: false, archived: false, createdAt: 1, updatedAt: 1,
  });
  rt.save();
});

afterAll(async () => { await sup.shutdown(); });

describe('opencode permission requests', () => {
  it('replies to the request owner (a subagent child session), not the chat session', async () => {
    const fake = fakeClient({ pending: [[{ id: 'per_1', sessionID: 'ses_child', action: 'shell', resources: ['ls'] }]] });
    rt.client = async () => fake;
    expect(await rt.replyPermission(chatId, 'per_1', 'once')).toBe('ok');
    expect(fake.replies).toEqual([['ses_child', 'per_1', 'once']]);
  });

  it('retries with a fresh owner when the cached session is stale', async () => {
    const fake = fakeClient({
      pending: [
        [{ id: 'per_2', sessionID: 'ses_stale', action: 'shell' }],
        [{ id: 'per_2', sessionID: 'ses_other', action: 'shell' }],
      ],
      failOnce: true,
    });
    rt.client = async () => fake;
    expect(await rt.replyPermission(chatId, 'per_2', 'always')).toBe('ok');
    expect(fake.replies.map((r) => r[0])).toEqual(['ses_stale', 'ses_other']);
  });

  it('reports an expired request instead of throwing a raw 404', async () => {
    const fake = fakeClient({ pending: [[]] });
    rt.client = async () => fake;
    expect(await rt.replyPermission(chatId, 'per_gone', 'once')).toBe('expired');
    expect(fake.replies).toEqual([]);
  });

  it('syncs pending requests from the root and child sessions and keeps their owner session', async () => {
    const fake = fakeClient({
      pending: [[
        { id: 'per_root', sessionID: 'ses_root', action: 'edit', resources: ['a.ts'] },
        { id: 'per_child', sessionID: 'ses_child2', action: 'shell', resources: ['pwd'] },
      ]],
      sessions: { ses_child2: { id: 'ses_child2', parentID: 'ses_root' } },
    });
    rt.client = async () => fake;
    const pending = await rt.pendingPermissions(chatId);
    expect(pending.map((p: any) => [p.id, p.sid])).toEqual([['per_root', 'ses_root'], ['per_child', 'ses_child2']]);
  });

  it('emits cards with the owning session id', async () => {
    const seen: any[] = [];
    const hooks = (rt as any).hooks;
    const old = hooks.emit;
    hooks.emit = (e: any) => seen.push(e);
    try {
      (rt as any).emitPermission(rt.chat(chatId), { type: 'permission.asked', data: { id: 'per_card', sessionID: 'ses_child3', action: 'shell', resources: ['pwd'] } });
    } finally { hooks.emit = old; }
    expect(seen).toHaveLength(1);
    expect(seen[0].request).toMatchObject({ id: 'per_card', sid: 'ses_child3' });
  });
});

describe('chat activity', () => {
  it('the snapshot says which chats are working or waiting, and every change is announced', () => {
    rt.permSessions.clear();
    const activity = () => rt.snapshot().chatActivity[chatId];
    const emit = rt.hooks.emit;
    let snapshots = 0;
    rt.hooks.emit = (e: { type: string }) => { if (e.type === 'snapshot') snapshots++; };
    try {
      expect(activity()).toBeUndefined();
      rt.setLive(chatId, { chatId, messageId: 'm', text: '', tools: [], started: 1 });
      expect(activity()).toBe('working');
      expect(snapshots).toBe(1);
      rt.emitPermission(rt.chat(chatId), { type: 'permission', data: { id: 'per_a', sessionID: 'ses_root', action: 'shell' } });
      expect(activity()).toBe('waiting');
      expect(snapshots).toBe(2);
      rt.clearPermission(chatId, 'per_a');
      expect(activity()).toBe('working');
      expect(snapshots).toBe(3);
      rt.dropLive(chatId);
      expect(activity()).toBeUndefined();
      expect(snapshots).toBe(4);
    } finally { rt.hooks.emit = emit; rt.live.delete(chatId); rt.permSessions.delete('per_a'); }
  });
});

describe('full permissions (YOLO) mode', () => {
  it('applies an allow-all session ruleset and settles asks already waiting', async () => {
    const fake = fakeClient({ pending: [[{ id: 'per_y1', sessionID: 'ses_root', action: 'shell', resources: ['pwd'] }]] });
    rt.client = async () => fake;
    const chat = await rt.setChatYolo(chatId, true);
    expect(chat.yolo).toBe(true);
    expect(fake.patched).toEqual([['ses_root', [{ action: '*', resource: '*', effect: 'allow' }]]]);
    expect(fake.replies).toEqual([['ses_root', 'per_y1', 'once']]);

    await rt.setChatYolo(chatId, false);
    expect(rt.chat(chatId).yolo).toBe(false);
    expect(fake.patched[1]).toEqual(['ses_root', []]);
  });

  it('never shows a card for an ask that arrives while full access is on', async () => {
    rt.chat(chatId).yolo = true;
    const fake = fakeClient({});
    rt.client = async () => fake;
    const seen: any[] = [];
    const hooks = (rt as any).hooks;
    const old = hooks.emit;
    hooks.emit = (e: any) => seen.push(e);
    try {
      (rt as any).emitPermission(rt.chat(chatId), { type: 'permission.asked', data: { id: 'per_y2', sessionID: 'ses_root', action: 'shell', resources: ['pwd'] } });
      await new Promise((r) => setTimeout(r, 20));
    } finally { hooks.emit = old; rt.chat(chatId).yolo = false; }
    expect(seen.filter((e) => e.type === 'chat.permission')).toHaveLength(0);
    expect(fake.replies).toEqual([['ses_root', 'per_y2', 'once']]);
  });

  it('auto-approves ACP permission requests without a card', async () => {
    rt.chat(chatId).yolo = true;
    const key = 'claude::/tmp/folder';
    (rt as any).acpChatBySession.set(key + '\nacp_s1', chatId);
    try {
      const answer = await (rt as any).onAcpPermission(key, 'acp_s1', {
        id: 'acp_per1', title: 'Run pwd', detail: 'pwd',
        options: [{ optionId: 'o_allow', name: 'Allow once', kind: 'allow_once' }, { optionId: 'o_no', name: 'Reject', kind: 'reject_once' }],
      });
      expect(answer).toBe('o_allow');
    } finally {
      rt.chat(chatId).yolo = false;
      (rt as any).acpChatBySession.delete(key + '\nacp_s1');
    }
  });
});
