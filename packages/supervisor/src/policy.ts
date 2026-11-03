// Organization policy: signed by the org, verified before use, last good copy kept (R-36).
// Ed25519 over the canonical JSON of the policy object.
import { generateKeyPairSync, sign, verify, createPublicKey, createPrivateKey, type KeyObject } from 'node:crypto';
import { join } from 'node:path';
import type { OrgPolicy, SignedPolicy } from '../../shared/src/types.ts';
import { harnessHome } from './paths.ts';
import { readJson, writeJson } from './jsonfile.ts';

export function canonical(v: unknown): string {
  if (Array.isArray(v)) return '[' + v.map(canonical).join(',') + ']';
  if (v && typeof v === 'object') return '{' + Object.keys(v as object).sort().map((k) => JSON.stringify(k) + ':' + canonical((v as Record<string, unknown>)[k])).join(',') + '}';
  return JSON.stringify(v);
}

export function signPolicy(policy: OrgPolicy, privateKeyPem: string, keyId: string): SignedPolicy {
  const sig = sign(null, Buffer.from(canonical(policy)), createPrivateKey(privateKeyPem)).toString('base64');
  return { policy, signature: sig, keyId };
}

export function verifyPolicy(sp: SignedPolicy, publicKeyPem: string): boolean {
  try { return verify(null, Buffer.from(canonical(sp.policy)), createPublicKey(publicKeyPem), Buffer.from(sp.signature, 'base64')); } catch { return false; }
}

/**
 * The demo organization ("BSC AI Factory") is simulated locally: its issuer key lives under ~/.harness/demo-org.
 * A real org publishes its policy and public key through its identity provider; only the fetch differs.
 */
export function demoIssuer(): { publicKey: string; privateKey: string; keyId: string } {
  const file = join(harnessHome(), 'demo-org', 'issuer.json');
  const existing = readJson<{ publicKey: string; privateKey: string; keyId: string } | null>(file, null);
  if (existing) return existing;
  const { publicKey, privateKey } = generateKeyPairSync('ed25519');
  const out = { publicKey: (publicKey as KeyObject).export({ type: 'spki', format: 'pem' }).toString(), privateKey: (privateKey as KeyObject).export({ type: 'pkcs8', format: 'pem' }).toString(), keyId: 'demo-org-' + Date.now().toString(36) };
  writeJson(file, out);
  return out;
}

export interface PolicyCheck { ok: boolean; policy: OrgPolicy | null; reason?: string }

/** Verify a freshly fetched policy; if it fails, keep the last good one and block sending. */
export function acceptPolicy(fetched: SignedPolicy | null, pinnedPublicKey: string, lastGood: OrgPolicy | null): PolicyCheck {
  if (fetched && verifyPolicy(fetched, pinnedPublicKey)) return { ok: true, policy: fetched.policy };
  return { ok: false, policy: lastGood, reason: fetched ? 'Policy signature did not verify; keeping the last good policy and blocking sends.' : 'Could not fetch policy; keeping the last good one.' };
}
