// Routines in plain words (R-27, R-43): chrono-node for dates and times, plus a harness layer for repeats
// ("every", "weekdays", "weekends", "every 2 hours 9–18", "first day of each month") and events
// ("on push to main", "when an email arrives"). The result shows how it was understood before saving.
import * as chrono from 'chrono-node';
import { CronExpressionParser } from 'cron-parser';

export interface ParsedSchedule {
  ok: boolean;
  kind: 'repeat' | 'once' | 'event';
  cron: string;
  tz: string;
  once?: number;
  event?: { event: string; detail: string };
  next: number | null;
  human: string;
  matched: string[];
  error?: string;
}

const DAYS: Record<string, number> = { sunday: 0, sun: 0, monday: 1, mon: 1, tuesday: 2, tue: 2, tues: 2, wednesday: 3, wed: 3, thursday: 4, thu: 4, thur: 4, thurs: 4, friday: 5, fri: 5, saturday: 6, sat: 6 };
const DAY_NAMES = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

export function localTz(): string { try { return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC'; } catch { return 'UTC'; } }

/** Offset of `tz` from UTC in minutes at `at`. */
export function tzOffsetMinutes(tz: string, at: Date): number {
  const f = new Intl.DateTimeFormat('en-US', { timeZone: tz, hourCycle: 'h23', year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit', second: '2-digit' });
  const p = Object.fromEntries(f.formatToParts(at).map((x) => [x.type, x.value]));
  const asUtc = Date.UTC(Number(p.year), Number(p.month) - 1, Number(p.day), Number(p.hour), Number(p.minute), Number(p.second));
  return Math.round((asUtc - at.getTime()) / 60000);
}

export function nextRun(cron: string, tz: string, from = new Date()): number | null {
  try { return CronExpressionParser.parse(cron, { tz, currentDate: from }).next().toDate().getTime(); } catch { return null; }
}

export function nextRuns(cron: string, tz: string, count: number, from = new Date()): number[] {
  try { const it = CronExpressionParser.parse(cron, { tz, currentDate: from }); return Array.from({ length: count }, () => it.next().toDate().getTime()); } catch { return []; }
}

export function formatWhen(ms: number, tz: string, now = Date.now()): string {
  const d = new Date(ms);
  const day = new Intl.DateTimeFormat('en-GB', { timeZone: tz, weekday: 'short', day: 'numeric', month: 'short' }).format(d);
  const time = new Intl.DateTimeFormat('en-GB', { timeZone: tz, hour: '2-digit', minute: '2-digit', hourCycle: 'h23' }).format(d);
  const key = (x: number) => new Intl.DateTimeFormat('en-CA', { timeZone: tz }).format(new Date(x));
  if (key(ms) === key(now)) return 'Today ' + time;
  if (key(ms) === key(now + 864e5)) return 'Tomorrow ' + time;
  return day + ' ' + time;
}

export function describeNext(cron: string, tz: string): string {
  const n = nextRun(cron, tz);
  return n ? formatWhen(n, tz) : 'not scheduled';
}

function timeOf(text: string, ref: Date, tz: string): { hour: number; minute: number; text: string } | null {
  const named = /\b(noon|midday|midnight|morning|evening|night|afternoon)\b/i.exec(text);
  const results = chrono.parse(text, { instant: ref, timezone: tzOffsetMinutes(tz, ref) });
  for (const r of results) {
    if (r.start.isCertain('hour')) return { hour: r.start.get('hour') ?? 9, minute: r.start.get('minute') ?? 0, text: r.text };
  }
  if (named) {
    const m: Record<string, number> = { noon: 12, midday: 12, midnight: 0, morning: 8, afternoon: 14, evening: 18, night: 21 };
    return { hour: m[named[1].toLowerCase()], minute: 0, text: named[0] };
  }
  return null;
}

function eventOf(t: string): { event: string; detail: string; text: string } | null {
  let m = /\bon (?:a )?push(?:es)? to ([\w./-]+)/i.exec(t); if (m) return { event: 'git.push', detail: m[1], text: m[0] };
  m = /\bon (?:a )?(?:new )?(?:pull request|pr|merge request)\b(?: (?:to|on) ([\w./-]+))?/i.exec(t); if (m) return { event: 'git.pull_request', detail: m[1] || '*', text: m[0] };
  m = /\bon commit\b/i.exec(t); if (m) return { event: 'git.commit', detail: '*', text: m[0] };
  m = /\bwhen an? (?:new )?(e-?mail|message) (?:arrives|comes in)(?: from ([^\s,]+))?/i.exec(t); if (m) return { event: m[1].toLowerCase().startsWith('e') ? 'email.received' : 'message.received', detail: m[2] || '*', text: m[0] };
  m = /\bwhen (?:a )?files? (?:in |under )?([~\w./-]+)? ?changes?\b|\bwhen ([~\w./-]+) changes\b/i.exec(t); if (m) return { event: 'file.changed', detail: m[1] || m[2] || '.', text: m[0] };
  m = /\bon (?:a )?webhook\b(?: ([\w/-]+))?/i.exec(t); if (m) return { event: 'webhook', detail: m[1] || 'default', text: m[0] };
  return null;
}

export function parseSchedule(phrase: string, opts: { tz?: string; now?: Date } = {}): ParsedSchedule {
  const tz = opts.tz || localTz();
  const now = opts.now || new Date();
  const t = phrase.trim().replace(/[–—]/g, '-');
  const lower = t.toLowerCase();
  const matched: string[] = [];
  const fail = (error: string): ParsedSchedule => ({ ok: false, kind: 'repeat', cron: '', tz, next: null, human: '', matched, error });
  if (!t) return fail('Write when it should run, e.g. “every weekday at 9”.');

  const ev = eventOf(t);
  if (ev) {
    matched.push(ev.text);
    const human = { 'git.push': `On every push to ${ev.detail}`, 'git.pull_request': `On every pull request${ev.detail !== '*' ? ' to ' + ev.detail : ''}`, 'git.commit': 'On every commit', 'email.received': 'When an email arrives' + (ev.detail !== '*' ? ' from ' + ev.detail : ''), 'message.received': 'When a message arrives', 'file.changed': `When files in ${ev.detail} change`, webhook: 'When the webhook is called' }[ev.event] || ev.event;
    return { ok: true, kind: 'event', cron: '', tz, event: { event: ev.event, detail: ev.detail }, next: null, human, matched };
  }

  const tm = timeOf(t, now, tz);
  const hour = tm?.hour ?? 9;
  const minute = tm?.minute ?? 0;
  const hhmm = `${String(hour).padStart(2, '0')}:${String(minute).padStart(2, '0')}`;
  const isRepeat = /\b(every|each|daily|weekly|monthly|hourly|weekdays?|weekends?|mondays|tuesdays|wednesdays|thursdays|fridays|saturdays|sundays)\b/.test(lower);

  // Day-of-week set
  let dow = '*';
  let dowHuman = '';
  if (/\bweekdays?\b|\bmon(day)?\s*-\s*fri(day)?\b/.test(lower)) { dow = '1-5'; dowHuman = 'weekdays'; matched.push(/\bweekdays?\b/.exec(lower)?.[0] || 'mon-fri'); }
  else if (/\bweekends?\b/.test(lower)) { dow = '0,6'; dowHuman = 'weekends'; matched.push('weekends'); }
  else {
    const days = Array.from(lower.matchAll(/\b(sundays?|mondays?|tuesdays?|wednesdays?|thursdays?|fridays?|saturdays?|sun|mon|tue|tues|wed|thu|thur|thurs|fri|sat)\b/g)).map((m) => m[1].replace(/s$/, ''));
    const nums = Array.from(new Set(days.map((d) => DAYS[d] ?? DAYS[d + 's']).filter((n) => n !== undefined))).sort();
    if (nums.length && isRepeat) { dow = nums.join(','); dowHuman = nums.map((n) => DAY_NAMES[n]).join(', '); matched.push(...days); }
  }

  // Every N minutes / hours, optionally within an hour window
  const everyN = /\bevery\s+(\d+)\s*(minutes?|mins?|hours?|hrs?|h)\b/.exec(lower);
  const window = /\b(\d{1,2})(?::00)?\s*-\s*(\d{1,2})(?::00)?\b/.exec(lower);
  if (everyN || /\bevery hour\b|\bhourly\b|\bevery minute\b/.test(lower)) {
    const n = everyN ? Number(everyN[1]) : 1;
    const unit = everyN ? everyN[2] : /minute/.test(lower) ? 'minute' : 'hour';
    matched.push(everyN?.[0] || (/hourly/.test(lower) ? 'hourly' : 'every hour'));
    if (/^m/.test(unit)) {
      if (n < 1 || n > 59) return fail('Minutes must be between 1 and 59.');
      const hours = window ? `${Number(window[1])}-${Number(window[2])}` : '*';
      if (window) matched.push(window[0]);
      const cron = `*/${n} ${hours} * * ${dow}`;
      return done('repeat', cron, `Every ${n} minute${n === 1 ? '' : 's'}${window ? ` between ${window[1]}:00 and ${window[2]}:59` : ''}${dowHuman ? ' on ' + dowHuman : ''}`);
    }
    if (n < 1 || n > 23) return fail('Hours must be between 1 and 23.');
    const hours = window ? `${Number(window[1])}-${Number(window[2])}/${n}` : `*/${n}`;
    if (window) matched.push(window[0]);
    const cron = `${minute} ${hours} * * ${dow}`;
    return done('repeat', cron, `Every ${n === 1 ? '' : n + ' '}hour${n === 1 ? '' : 's'}${window ? ` from ${window[1]}:00 to ${window[2]}:00` : ''}${dowHuman ? ' on ' + dowHuman : ''}`);
  }

  // Monthly: "first day of each month", "on the 15th of every month", "last day of the month", "monthly"
  const firstDay = /\bfirst day of (?:each|every|the) month\b/.exec(lower);
  const nth = /\b(?:on )?the (\d{1,2})(?:st|nd|rd|th)?(?: day)? of (?:each|every|the) month\b|\bevery month on the (\d{1,2})(?:st|nd|rd|th)?\b/.exec(lower);
  if (firstDay || nth || /\bmonthly\b/.test(lower)) {
    const dom = firstDay ? 1 : nth ? Number(nth[1] || nth[2]) : 1;
    if (dom < 1 || dom > 31) return fail('Day of month must be between 1 and 31.');
    matched.push(firstDay?.[0] || nth?.[0] || 'monthly');
    if (tm) matched.push(tm.text);
    return done('repeat', `${minute} ${hour} ${dom} * *`, `On day ${dom} of every month at ${hhmm}`);
  }

  // Daily / weekly / day-of-week repeats
  if (isRepeat) {
    if (/\b(every day|each day|daily|every morning|every evening|every night|every afternoon)\b/.test(lower)) matched.push(/\b(every day|each day|daily|every morning|every evening|every night|every afternoon)\b/.exec(lower)![0]);
    if (/\b(every week|weekly)\b/.test(lower) && dow === '*') { dow = String(now.getDay()); dowHuman = DAY_NAMES[now.getDay()]; matched.push('weekly'); }
    if (tm) matched.push(tm.text);
    const cron = `${minute} ${hour} * * ${dow}`;
    return done('repeat', cron, `${dowHuman ? 'Every ' + dowHuman : 'Every day'} at ${hhmm}`);
  }

  // One-off: tomorrow at 9, next Monday at 10, in 3 hours
  const r = chrono.parse(t, { instant: now, timezone: tzOffsetMinutes(tz, now) }, { forwardDate: true })[0];
  if (r) {
    const when = r.start.date();
    if (!r.start.isCertain('hour') && !/\bin \d+/.test(lower)) when.setTime(when.getTime() - (when.getTime() % 60000));
    matched.push(r.text);
    if (when.getTime() <= now.getTime()) return fail('That time has already passed.');
    const parts = Object.fromEntries(new Intl.DateTimeFormat('en-US', { timeZone: tz, hourCycle: 'h23', month: 'numeric', day: 'numeric', hour: 'numeric', minute: 'numeric' }).formatToParts(when).map((p) => [p.type, p.value]));
    const cron = `${Number(parts.minute)} ${Number(parts.hour)} ${Number(parts.day)} ${Number(parts.month)} *`;
    return { ok: true, kind: 'once', cron, tz, once: when.getTime(), next: when.getTime(), human: 'Once, ' + formatWhen(when.getTime(), tz, now.getTime()), matched };
  }
  return fail('I couldn’t find a time in that. Try “every weekday at 9” or “tomorrow at 10”.');

  function done(kind: 'repeat', cron: string, human: string): ParsedSchedule {
    const next = nextRun(cron, tz, now);
    if (next == null) return fail('That schedule never runs.');
    return { ok: true, kind, cron, tz, next, human, matched: Array.from(new Set(matched)) };
  }
}
