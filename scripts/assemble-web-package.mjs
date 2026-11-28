import { cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
const slug = process.argv[2];
if (!['web-ui', 'dice', 'chat', 'character', 'ai-provider', 'settings-ui', 'package-manager', 'world-atlas', 'memory', 'scenes'].includes(slug)) throw new Error('Unknown web package');
const root = resolve(import.meta.dirname, '..');
const target = resolve(root, 'packages', slug, 'build/rtw');
await rm(target, { recursive: true, force: true });
await mkdir(target, { recursive: true });
if (['chat', 'character', 'ai-provider', 'package-manager', 'world-atlas', 'memory', 'scenes'].includes(slug)) {
  if (!process.argv[3]) throw new Error('Domain assembly requires its built WASM backend');
  await cp(resolve(process.argv[3]), resolve(target, 'backend.wasm'));
  if (process.argv[4]) await cp(resolve(process.argv[4]), resolve(target, 'world-portability.json'));
}
await cp(resolve(root, 'packages', slug, 'dist'), resolve(target, 'web'), { recursive: true, force: true });
for (const name of ['rtw.toml', 'manifest.toml', 'web-bundle.toml']) await cp(resolve(root, 'packages', slug, name), resolve(target, name));
const extraFiles = {
  character: ['story-bundle.toml'],
  'web-ui': ['home-bundle.toml'],
  'settings-ui': ['appearance-bundle.toml', 'world-portability.json'],
};
for (const name of extraFiles[slug] ?? []) await cp(resolve(root, 'packages', slug, name), resolve(target, name));
await cp(resolve(root, 'LICENSE'), resolve(target, 'LICENSE'));
const names = ['react', 'react-dom', 'scheduler', ...(slug === 'web-ui' ? ['dockview-react', 'dockview', 'dockview-core'] : [])];
let notices = '';
for (const name of names) {
  const directory = resolve(root, 'node_modules', name);
  const pkg = JSON.parse(await readFile(resolve(directory, 'package.json'), 'utf8'));
  let license;
  for (const file of ['LICENSE', 'LICENSE.md', 'LICENCE.md']) { try { license = await readFile(resolve(directory, file), 'utf8'); break; } catch {} }
  if (!license) throw new Error(`Missing bundled license for ${name}`);
  notices += `${name} ${pkg.version}\n${license}\n\n`;
}
await writeFile(resolve(target, 'THIRD-PARTY-NOTICES.txt'), notices);
