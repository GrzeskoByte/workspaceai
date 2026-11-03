import { defineConfig } from 'vitest/config';
export default defineConfig({ test: { include: ['packages/*/test/**/*.test.ts'], fileParallelism: false, testTimeout: 120000, hookTimeout: 120000 } });
