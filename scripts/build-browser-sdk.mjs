import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { stripTypeScriptTypes } from 'node:module';
const source = await readFile(new URL('../web/bridge/src/index.ts', import.meta.url), 'utf8');
await mkdir(new URL('../web/bridge/dist/', import.meta.url), { recursive: true });
await writeFile(new URL('../web/bridge/dist/index.js', import.meta.url), stripTypeScriptTypes(source, { mode: 'strip' }));
