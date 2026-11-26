import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import vm from 'node:vm';
// Execute the real component with controlled transport failures. This checks the
// recovery UI and allocation/admission calls, without replacing Story's logic.
if (!vm.SourceTextModule) {
  const result = spawnSync(process.execPath, ['--experimental-vm-modules', fileURLToPath(import.meta.url)], { stdio: 'inherit' });
  if (result.error) throw result.error;
  process.exit(result.status ?? 1);
}
const repository = fileURLToPath(new URL('../', import.meta.url));
const output = await mkdtemp(join(tmpdir(), 'rtwkit-story-commit-'));
try {
  const config = join(output, 'tsconfig.json');
  await writeFile(config, JSON.stringify({
    extends: join(repository, 'tsconfig.json'),
    include: [join(repository, 'packages/character/src/Story.tsx')],
    compilerOptions: {
      types: [join(repository, 'node_modules/@types/node'), join(repository, 'node_modules/vite/client')],
      noEmit: false, outDir: join(output, 'emitted'), rootDir: repository,
      rewriteRelativeImportExtensions: true,
    },
  }));
  const compiled = spawnSync(join(repository, 'node_modules/.bin/tsc'), ['--project', config], { cwd: repository, stdio: 'inherit' });
  if (compiled.error) throw compiled.error;
  assert.equal(compiled.status, 0, 'scoped compilation of the actual Story component');
  const source = await readFile(join(output, 'emitted/packages/character/src/Story.js'), 'utf8');
  const id = '00000000-0000-7000-8000-000000000001';
  const card = { id: '00000000-0000-7000-8000-000000000002', card: { name: 'Alice' }, greeting: { text: null, error: null } };
  const label = node => node == null || typeof node === 'boolean' ? '' : typeof node !== 'object' ? String(node) : Array.isArray(node) ? node.map(label).join('') : label(node.props?.children);
  const find = (node, predicate) => {
    if (!node || typeof node !== 'object') return undefined;
    if (Array.isArray(node)) return node.map(child => find(child, predicate)).find(Boolean);
    return predicate(node) ? node : find(node.props?.children, predicate);
  };
  for (const scenario of ['recent', 'describe', 'navigation', 'save', 'restart', 'unknown', 'failed']) {
    const slots = [], effects = []; let cursor = 0, injected = false, allocations = 0, begins = 0, navigations = 0;
    let saved = scenario === 'restart' ? { conversationId: id, phase: 'committed' } : {};
    const react = {
      useRef(value) { const n = cursor++; return slots[n] ??= { current: value }; },
      useState(value) { const n = cursor++; if (!(n in slots)) slots[n] = value; return [slots[n], next => { slots[n] = typeof next === 'function' ? next(slots[n]) : next; }]; },
      useEffect(effect) { const n = cursor++; if (!(n in slots)) { slots[n] = true; effects.push(effect); } },
    };
    class DomainError extends Error { constructor(code) { super(code); this.code = code; this.outcome = 'not-started'; } }
    class BridgeError extends DomainError {}
    const fail = stage => {
      if (!injected && (scenario === stage || scenario === 'restart' && stage === 'recent')) {
        injected = true; throw new DomainError(`injected-${stage}`);
      }
    };
    class DomainClient {
      constructor(client, point) { this.point = point; }
      async allocate() { allocations++; return [id]; }
      async begin() { begins++; return { operation: 'fixture', itemIds: [id] }; }
      async wait() { return { status: scenario === 'failed' ? 'failed' : scenario === 'unknown' ? 'unknown' : 'committed', reason: 'fixture rejected' }; }
      async release() {}
      async request(method) {
        if (method === 'list') return { items: this.point.includes('character') ? [card] : [], next: null };
        if (method === 'get') return card;
        if (method === 'list-greetings') return { sourceDigest: 'sha256:' + '0'.repeat(64), items: [{id:'primary',status:'empty',preview:null}], next:null };
        if (method === 'recent' || method === 'describe') fail(method);
        if (method === 'recent') return { items: [] };
        if (method === 'describe') return { instanceId: 'chat-owner', componentId: 'ui' };
        throw Error(`Unexpected request ${method}`);
      }
    }
    const client = {
      async getState() { return saved; },
      async setState(state) { if (state.phase === 'committed') fail('save'); saved = structuredClone(state); },
      async navigate(instance, component, state) { assert.equal(state.conversationId, id); fail('navigation'); navigations++; },
    };
    const jsx = (type, props) => ({ type, props });
    const imports = {
      react,
      'react/jsx-runtime': { jsx, jsxs: jsx, Fragment: 'fragment' },
      '@rintawa/bridge': { BridgeError },
      '@rintawa/domain': { DomainClient, DomainError, observeResource: options => ({ refresh: async () => { try { options.apply(await options.read(new AbortController().signal)); } catch (error) { options.failed(error); throw error; } }, close() {} }) },
      '@rintawa/ui': { Button: 'button', Icon: 'icon', Status: 'status' },
      './PersonaEditor': { PersonaEditor: 'persona-editor' },
    };
    const context = vm.createContext({ AbortController, console });
    const module = new vm.SourceTextModule(source, { context });
    await module.link(async specifier => {
      assert.ok(imports[specifier], `Unexpected import ${specifier}`);
      const values = imports[specifier];
      return new vm.SyntheticModule(Object.keys(values), function () { for (const [key, value] of Object.entries(values)) this.setExport(key, value); }, { context });
    });
    await module.evaluate();
    const render = () => { cursor = 0; return module.namespace.Story({ client }); };
    const button = (tree, name) => find(tree, node => node.type === 'button' && label(node) === name);
    const flush = async () => { for (let i = 0; i < 5; i++) await new Promise(setImmediate); };
    render(); effects.forEach(effect => effect()); await flush();
    if (scenario !== 'restart') {
      const start = button(render(), 'Continue with Alice'); assert.ok(start); start.props.onClick(); await flush();
      assert.equal(begins, 0, 'draft preview must not create a conversation'); assert.equal(allocations, 0);
      const confirm = button(render(), 'Start without greeting'); assert.ok(confirm); confirm.props.onClick(); await flush();
      assert.equal(begins, 1); assert.equal(allocations, 1);
    }
    if (scenario === 'failed') {
      assert.equal(saved.phase, 'imported'); assert.equal(saved.conversationId, undefined);
      assert.ok(button(render(), 'Start without greeting')); assert.equal(button(render(), 'Retry opening chat'), undefined);
    } else if (scenario === 'unknown') {
      assert.equal(saved.phase, 'creating'); assert.equal(saved.conversationId, id);
      const read = button(render(), 'Read World'); assert.ok(read); read.props.onClick(); await flush();
      assert.equal(saved.phase, 'committed'); assert.equal(saved.conversationId, id); assert.equal(begins, 1); assert.equal(allocations, 1); assert.equal(navigations, 1);
    } else {
      assert.equal(saved.conversationId, id, `${scenario}: retain committed ID`);
      const tree = render(), retry = button(tree, 'Retry opening chat'); assert.ok(retry, `${scenario}: explicit same-ID retry`);
      assert.equal(button(tree, 'Start without greeting'), undefined);
      assert.equal(button(tree, 'Continue with Alice'), undefined);
      assert.equal(find(tree, node => node.type === 'input' && node.props.type === 'file'), undefined);
      const before = { allocations, begins }; retry.props.onClick(); await flush();
      assert.deepEqual({ allocations, begins }, before, `${scenario}: opening recovery must never create another Chat`);
      assert.equal(saved.phase, 'committed'); assert.equal(saved.conversationId, id); assert.equal(navigations, 1);
    }
    console.log(`PASS ${scenario}: actual Story component preserves commit identity and never replays creation`);
  }

} finally {
  await rm(output, { recursive: true, force: true });
}
