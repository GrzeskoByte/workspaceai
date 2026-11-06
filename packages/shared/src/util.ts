import type { Project, Chat, Artifact } from './types.ts';

let seq = 0;
export function uid(prefix: string): string {
  seq = (seq + 1) % 1e6;
  return prefix + '_' + Date.now().toString(36) + seq.toString(36) + Math.random().toString(36).slice(2, 6);
}

/** Whether a chat keeps following new text after a scroll: any move up lets go, and coming back near the bottom follows again. */
export function followsBottom(following: boolean, lastTop: number, top: number, gapToBottom: number): boolean {
  if (top < lastTop) return false;
  return following || gapToBottom < 80;
}

export function slugify(name: string, fallback = 'workspace'): string {
  return name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || fallback;
}

export function initialsOf(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (!parts.length) return '?';
  if (parts.length === 1) return parts[0].charAt(0).toUpperCase();
  return parts.map((p) => p.charAt(0)).join('').slice(0, 2).toUpperCase();
}

/** Root-first chain of projects from the top level down to `pid`. */
export function projectPath(projects: Project[], pid: string | null | undefined): Project[] {
  const out: Project[] = [];
  let p = projects.find((x) => x.id === pid);
  let guard = 0;
  while (p && guard++ < 64) {
    out.unshift(p);
    const parent = p.parent;
    p = parent ? projects.find((x) => x.id === parent) : undefined;
  }
  return out;
}

export function projectPathName(projects: Project[], pid: string | null | undefined): string {
  return projectPath(projects, pid).map((p) => p.name).join(' › ');
}

export function childProjects(projects: Project[], pid: string | null): Project[] {
  return projects.filter((p) => p.parent === pid && !p.archived);
}

export function descendants(projects: Project[], pid: string): string[] {
  return childProjects(projects, pid).flatMap((p) => [p.id, ...descendants(projects, p.id)]);
}

/** True when `pid` is `ancestor` or nested anywhere beneath it. */
export function isWithin(projects: Project[], pid: string, ancestor: string): boolean {
  return projectPath(projects, pid).some((p) => p.id === ancestor);
}

/** Depth-first flat list, used by pickers. */
export function flatProjects(projects: Project[]): { project: Project; depth: number }[] {
  const out: { project: Project; depth: number }[] = [];
  const walk = (pid: string | null, depth: number) => {
    const kids = childProjects(projects, pid).sort((a, b) => Number(!!b.isGeneral) - Number(!!a.isGeneral) || a.createdAt - b.createdAt);
    for (const p of kids) { out.push({ project: p, depth }); walk(p.id, depth + 1); }
  };
  walk(null, 0);
  return out;
}

/** Artifacts that roll up into a project's Artifacts tab: its own plus every sub-project's (R-13). */
export function rolledUpArtifacts(projects: Project[], artifacts: Artifact[], pid: string): Artifact[] {
  return artifacts.filter((a) => isWithin(projects, a.project, pid));
}

export function chatsWithin(projects: Project[], chats: Chat[], pid: string): Chat[] {
  return chats.filter((c) => !c.archived && isWithin(projects, c.project, pid));
}

/** Folder → parent folder mapping used when imported session folders become nested projects (R-18). */
export function nestFolders(dirs: string[]): { dir: string; parent: string | null }[] {
  const sorted = Array.from(new Set(dirs)).sort((a, b) => a.length - b.length);
  return sorted.map((d) => {
    const parent = sorted.filter((o) => o !== d && d.startsWith(o.replace(/\/$/, '') + '/')).sort((a, b) => b.length - a.length)[0] ?? null;
    return { dir: d, parent };
  });
}

export function titleCaseSegment(dir: string): string {
  const seg = dir.replace(/\/$/, '').split('/').pop() || dir;
  return seg.charAt(0).toUpperCase() + seg.slice(1);
}

export function timeAgo(ms: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  if (s < 60) return 'just now';
  const m = Math.round(s / 60); if (m < 60) return m + ' min ago';
  const h = Math.round(m / 60); if (h < 24) return h + ' h ago';
  const d = Math.round(h / 24); if (d === 1) return 'yesterday';
  if (d < 7) return d + ' days ago';
  if (d < 14) return 'last week';
  if (d < 31) return Math.round(d / 7) + ' weeks ago';
  return d < 62 ? 'last month' : Math.round(d / 30) + ' months ago';
}

/** Fuzzy score used by the ⌘K palette and tree search: 3 prefix, 2 substring, 1 subsequence, 0 none. */
export function fuzzyScore(query: string, label: string, extra = ''): number {
  const q = query.toLowerCase().trim();
  if (!q) return 1;
  const l = label.toLowerCase();
  if (l.startsWith(q)) return 3;
  const hay = (label + ' ' + extra).toLowerCase();
  if (hay.includes(q)) return 2;
  let i = 0;
  for (const ch of hay) { if (ch === q[i]) i++; if (i === q.length) return 1; }
  return 0;
}
