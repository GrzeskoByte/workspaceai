// Verifies a policy signed by Rust with the TS `verifyPolicy`. Usage: node verify-ts-policy.ts <signed.json> <public.pem>
import { readFileSync } from 'node:fs';
import { verifyPolicy } from '../../../../packages/supervisor/src/policy.ts';

const [, , signedPath, publicPath] = process.argv;
if (!signedPath || !publicPath) {
  console.error('usage: verify-ts-policy.ts <signed.json> <public.pem>');
  process.exit(2);
}
const signed = JSON.parse(readFileSync(signedPath, 'utf8'));
const publicPem = readFileSync(publicPath, 'utf8');
if (!verifyPolicy(signed, publicPem)) {
  console.error('TS verifyPolicy rejected the Rust signature');
  process.exit(1);
}
console.log('ok');
