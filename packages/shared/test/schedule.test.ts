import { describe, it, expect } from 'vitest';
import { parseSchedule } from '../src/schedule.ts';

const now = new Date('2026-10-29T10:00:00Z'); // a Thursday
const tz = 'Europe/Warsaw';

describe('parseSchedule (R-43)', () => {
  it('tomorrow at 9 → once, next day 09:00 local', () => {
    const r = parseSchedule('tomorrow at 9', { tz, now });
    expect(r.ok).toBe(true);
    expect(r.kind).toBe('once');
    expect(r.cron).toBe('0 9 30 10 *');
    expect(new Date(r.next!).toISOString()).toBe('2026-10-30T08:00:00.000Z');
    expect(r.matched).toContain('tomorrow at 9');
  });
  it('every 2 hours 9–18 weekdays → 0 9-18/2 * * 1-5', () => {
    const r = parseSchedule('every 2 hours 9–18 weekdays', { tz, now });
    expect(r.ok).toBe(true);
    expect(r.cron).toBe('0 9-18/2 * * 1-5');
    expect(new Date(r.next!).toISOString()).toBe('2026-10-29T12:00:00.000Z'); // 13:00 Warsaw, the next slot after 11:00
    expect(r.human).toMatch(/Every 2 hours from 9:00 to 18:00 on weekdays/);
  });
  it('first day of each month at noon → 0 12 1 * *', () => {
    const r = parseSchedule('first day of each month at noon', { tz, now });
    expect(r.cron).toBe('0 12 1 * *');
    expect(new Date(r.next!).toISOString()).toBe('2026-11-01T11:00:00.000Z');
  });
  it('every weekday at 9:30', () => { expect(parseSchedule('every weekday at 9:30', { tz, now }).cron).toBe('30 9 * * 1-5'); });
  it('mondays and thursdays at 10', () => { expect(parseSchedule('mondays and thursdays at 10', { tz, now }).cron).toBe('0 10 * * 1,4'); });
  it('weekends at 8am', () => { expect(parseSchedule('every weekend at 8am', { tz, now }).cron).toBe('0 8 * * 0,6'); });
  it('every 15 minutes', () => { expect(parseSchedule('every 15 minutes', { tz, now }).cron).toBe('*/15 * * * *'); });
  it('next Monday at 10 → once', () => { const r = parseSchedule('next Monday at 10', { tz, now }); expect(r.kind).toBe('once'); expect(new Date(r.next!).toISOString()).toBe('2026-11-02T09:00:00.000Z'); });
  it('in 3 hours → once', () => { const r = parseSchedule('in 3 hours', { tz, now }); expect(r.kind).toBe('once'); expect(r.next).toBe(now.getTime() + 3 * 3600e3); });
  it('events', () => {
    expect(parseSchedule('on push to main', { tz, now }).event).toEqual({ event: 'git.push', detail: 'main' });
    expect(parseSchedule('when an email arrives', { tz, now }).event?.event).toBe('email.received');
  });
  it('rejects nonsense', () => { expect(parseSchedule('whenever you feel like it', { tz, now }).ok).toBe(false); });
});
