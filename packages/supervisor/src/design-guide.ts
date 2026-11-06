// What an agent reads before it makes or revises an artifact: Open Design's reference for that kind of artifact,
// layered the way Open Design composes its own designer prompt — the workspace's design system (DESIGN.md, the
// authoritative tokens), the skill for the job (its SKILL.md, example page and seed template), for decks Open
// Design's fixed deck framework, and its craft rules. The agent then saves the result with the harness tools.
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { openDesign } from './opendesign.ts';
import { designMd, designSystems, DEFAULT_SYSTEM } from './pipeline.ts';
import { odSettings } from '../../shared/src/defaults.ts';
import { DESIGN_MD_LIMIT } from '../../shared/src/claude-design-system.ts';
import type { WorkspaceRuntime } from './runtime.ts';

export type GuideType = 'doc' | 'deck' | 'prototype' | 'sheet' | 'design-system';

/** Open Design craft sections per artifact kind (its `craft/<id>.md`). */
const CRAFT: Record<GuideType, string[]> = {
  doc: ['anti-ai-slop', 'typography-hierarchy-editorial'],
  deck: ['anti-ai-slop', 'typography', 'color'],
  prototype: ['anti-ai-slop', 'typography', 'color', 'accessibility-baseline'],
  sheet: [],
  'design-system': ['anti-ai-slop', 'typography', 'color', 'accessibility-baseline'],
};

const FORMAT: Record<GuideType, string> = {
  doc: 'Pass `format`: "markdown" for a plain document (one H1, sections, lists, tables), or "html" for a designed page (one complete self-contained HTML file built with the design system and, if one fits, an Open Design skill below). Open Design stores the page either way.',
  deck: 'Pass `format`: "html" (preferred) for a deck built on Open Design’s deck framework below — copy its skeleton, bind the design system’s tokens in :root, fill one <section class="slide"> per slide; or "markdown" for quick slides separated by a line with only `---`, which WorkspaceAI lays onto the same framework.',
  prototype: 'Pass `format: "html"`: one complete self-contained HTML file — inline CSS (the design system’s tokens as CSS custom properties on :root) and inline JS, no external network requests or CDN fonts.',
  sheet: 'CSV with a header row (`format: "csv"`), or `format: "html"` for a designed table page.',
  'design-system': 'Pass `format: "markdown"`: a DESIGN.md with one H1 (the system’s name), a one-line summary under it, then principles, colour tokens (a table with 6-digit hex values, light and dark), type, spacing and component rules. Every value must be concrete enough to build from.',
};

/** Chats that read the guide for a kind of artifact this run (`chat:type`); artifact_create checks it. */
const guided = new Set<string>();
export const markGuided = (chat: string | null, type: string) => { if (chat) guided.add(chat + ':' + type); };
export const isGuided = (chat: string | null, type: string) => !chat || type === 'sheet' || guided.has(chat + ':' + type);

async function od<T>(url: string, path: string, as: 'json' | 'text' = 'json'): Promise<T | null> {
  try { const r = await fetch(url + path, { signal: AbortSignal.timeout(8000) }); return r.ok ? (as === 'json' ? await r.json() : await r.text()) as T : null; } catch { return null; }
}

interface OdSkill { id: string; name: string; description: string; mode: string; category?: string | null; featured: number | null; craftRequires?: string[]; body?: string }

const DOC_SKILL = /doc|article|report|resume|one-pager|editorial|magazine|faq|release|research|pdf|writing|letter|notes|principles|review/i;

/** Open Design skills that fit a kind of artifact, most featured first. */
function skillsFor(type: GuideType, all: OdSkill[]): OdSkill[] {
  const fits = (s: OdSkill) => type === 'deck' ? s.mode === 'deck'
    : type === 'prototype' ? s.mode === 'prototype' || s.mode === 'template'
    : type === 'doc' ? (s.mode === 'prototype' || s.mode === 'template') && DOC_SKILL.test(s.id + ' ' + (s.category || ''))
    : false;
  return all.filter(fits).sort((a, b) => (a.featured ?? 99) - (b.featured ?? 99)).slice(0, 30);
}

/** A skill as Open Design hands it to its own agents: SKILL.md, then the files it tells the agent to read first. */
async function skillBlock(url: string, id: string): Promise<{ text: string; skill: OdSkill | null }> {
  const skill = await od<OdSkill>(url, '/api/skills/' + encodeURIComponent(id));
  if (!skill) return { text: `## Skill\nOpen Design has no skill “${id}”.`, skill: null };
  const parts = [`## Open Design skill: ${skill.name}\nUse its workflow, layout vocabulary and visual signature. Write everything into the one file you save (its assets cannot be linked); the design system above wins on tokens unless the skill is a fixed template.\n\n${(skill.body || '').slice(0, 16000)}`];
  const files = (await od<{ files: { path: string; kind: string }[] }>(url, `/api/skills/${encodeURIComponent(id)}/files`))?.files || [];
  const seed = files.find((f) => f.path === 'assets/template.html');
  if (seed) {
    const t = await od<string>(url, `/api/skills/${encodeURIComponent(id)}/assets/template.html`, 'text');
    if (t) parts.push(`### Seed template (assets/template.html)\nStart from this file and replace its content; keep its structure and styles.\n\n\`\`\`html\n${t.slice(0, 48000)}\n\`\`\``);
  }
  if (files.some((f) => f.path === 'example.html') || !seed) {
    const ex = await od<string>(url, `/api/skills/${encodeURIComponent(id)}/example`, 'text');
    if (ex && /<html|<!doctype/i.test(ex)) parts.push(`### Example output\nWhat this skill’s result looks like — match its quality, not its content.\n\n\`\`\`html\n${ex.slice(0, 24000)}\n\`\`\``);
  }
  // References ship beside built-in skills; Open Design's agents read them before writing.
  const refDir = join(openDesign.installDir(), 'skills', id, 'references');
  if (existsSync(refDir)) {
    for (const f of readdirSync(refDir).filter((x) => x.endsWith('.md')).slice(0, 4)) parts.push(`### Reference: ${f}\n\n${readFileSync(join(refDir, f), 'utf8').slice(0, 8000)}`);
  }
  return { text: parts.join('\n\n'), skill };
}

export async function artifactGuide(rt: WorkspaceRuntime, type: GuideType, skillId?: string, designSystemId?: string): Promise<string> {
  const viaOd = odSettings(rt.ws).engine === 'open-design';
  const url = viaOd ? await openDesign.require(rt) : null;
  const systems = await designSystems(rt);
  const sys = systems.find((d) => (designSystemId ? d.id === designSystemId : d.title === rt.ws.designSystem)) || systems[0];
  const out: string[] = [`# Open Design reference for this ${type}`,
    `Follow it for the artifact you save with artifact_create / artifact_update. ${type === 'design-system' ? 'You are writing a new design system, not following one.' : 'The design system is authoritative: use its palette, type scale, spacing and component rules exactly; do not invent other colours or fonts.'} Pass the complete document as \`content\` — never a summary or a file path.`,
    `## Format\n${FORMAT[type]}`];
  if (type !== 'sheet' && type !== 'design-system') {
    const md = await designMd(rt, sys?.id || DEFAULT_SYSTEM).catch(() => '');
    out.push(`## Design system: ${sys?.title || 'Claude Warm Editorial'}${sys?.source === 'open-design' ? ' (Open Design)' : ''}\n\n${md.slice(0, sys?.source === 'workspace' ? DESIGN_MD_LIMIT : 14000) || '(not available)'}`);
  }
  let skill: OdSkill | null = null;
  if (url && type !== 'sheet') {
    if (skillId) {
      const b = await skillBlock(url, skillId);
      skill = b.skill;
      out.push(b.text);
    } else {
      const picks = skillsFor(type, (await od<{ skills: OdSkill[] }>(url, '/api/skills'))?.skills || []);
      if (picks.length) out.push(`## Open Design skills for ${type === 'doc' ? 'designed documents' : type + 's'}\nIf one fits the request, call artifact_guide again with \`skill\` set to its id: it returns that skill’s workflow, seed template and example. Otherwise build from the design system.\n${picks.map((s) => `- ${s.id} — ${s.description.replace(/\s+/g, ' ').slice(0, 140)}`).join('\n')}`);
    }
  }
  if (type === 'deck') {
    const fw = await openDesign.deckFramework();
    if (fw) out.push(`## Open Design deck framework (format "html")\n${skill && /assets\/template\.html/.test(skill.body || '') ? 'The skill above ships its own seed: use the seed instead of this skeleton, but keep its navigation and print rules.\n\n' : ''}${fw.directive}\n\n## Handoff\nWhen the deck is complete and self-checked, save the whole \`<!doctype html>\` document with artifact_create (\`type: "deck", format: "html"\`), or artifact_update for a new version. Do not paste it into the chat.`);
  }
  if (url) {
    const ids = [...new Set([...CRAFT[type], ...(skill?.craftRequires || [])])];
    for (const id of ids) {
      const c = await od<{ body: string }>(url, '/api/craft/' + id);
      if (c?.body) out.push(`## Open Design craft: ${id}\n\n${c.body.slice(0, 7000)}`);
    }
  }
  return out.join('\n\n');
}
