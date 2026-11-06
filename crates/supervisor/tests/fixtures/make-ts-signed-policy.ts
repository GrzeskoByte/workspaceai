// Regenerates the cross-language policy fixture. Run from the repo root:
//   node crates/supervisor/tests/fixtures/make-ts-signed-policy.ts
// The committed output (ts-signed-policy.json, public.pem, private.pem) proves that a policy signed by
// `signPolicy` in packages/supervisor/src/policy.ts verifies in Rust.
import { writeFileSync } from 'node:fs';
import { generateKeyPairSync } from 'node:crypto';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { signPolicy } from '../../../../packages/supervisor/src/policy.ts';

const here = dirname(fileURLToPath(import.meta.url));
const { publicKey, privateKey } = generateKeyPairSync('ed25519');
const publicPem = publicKey.export({ type: 'spki', format: 'pem' }).toString();
const privatePem = privateKey.export({ type: 'pkcs8', format: 'pem' }).toString();

// Numbers that JSON.stringify formats in every branch of ECMAScript Number::toString, plus unicode keys.
const policy = {
  version: '2026.10.1',
  org: 'BSC AI Factory · Ünïcode',
  'ключ': 'значение',
  'emoji🙂': 'ok',
  nested: {
    'é': [0, -0, 1, 1.5, 1e21, 1e-7, 1e-6, 1e300, 123456789012345678901, 2.5e-10],
    control: 'tab\tnewline\nquote"backslash\\',
    'Ω': { '中': true, '🙂': null },
  },
  allowedProviders: [],
  allowedModels: [],
  logContent: false,
  issuedAt: 1790812800000,
};

const signed = signPolicy(policy as never, privatePem, 'ts-fixture-key');
writeFileSync(join(here, 'ts-signed-policy.json'), JSON.stringify(signed, null, 2) + '\n');
writeFileSync(join(here, 'public.pem'), publicPem);
writeFileSync(join(here, 'private.pem'), privatePem);
console.log(`wrote ${join(here, 'ts-signed-policy.json')} (key ${signed.keyId})`);
