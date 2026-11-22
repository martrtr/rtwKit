import assert from 'node:assert/strict';
import test from 'node:test';
import { readLayout } from '../src/layout.ts';

const layout = () => ({
  version: 1,
  panels: [{ panelId: 'panel', restoreKey: { worldId: null, instanceId: 'instance', componentId: 'dice' } }],
  dockview: {
    grid: { root: { type: 'branch', data: [{ type: 'leaf', data: { id: 'group', views: ['panel'], activeView: 'panel' }, size: 640 }] }, height: 480, width: 640, orientation: 'HORIZONTAL' },
    panels: { panel: { id: 'panel', contentComponent: 'extension', title: 'Dice' } }, activeGroup: 'group',
  },
});

test('layout preserves stable identity and accepts the supported Dockview profile', () => {
  const saved = layout(); assert.deepEqual(readLayout(saved), saved);
});
test('layout rejects executable panel parameters and popout URLs before restoration', () => {
  const params = layout(); params.dockview.panels.panel.params = { url: 'https://example.invalid' };
  assert.equal(readLayout(params), null);
  const popout = layout(); popout.dockview.popoutGroups = [{ url: 'https://example.invalid' }];
  assert.equal(readLayout(popout), null);
});
test('layout rejects duplicate and dangling panel references and malformed World identity', () => {
  const duplicate = layout(); duplicate.dockview.grid.root.data[0].data.views.push('panel');
  assert.equal(readLayout(duplicate), null);
  const dangling = layout(); dangling.dockview.grid.root.data[0].data.views = ['missing'];
  assert.equal(readLayout(dangling), null);
  const world = layout(); world.panels[0].restoreKey.worldId = 'other-world';
  assert.equal(readLayout(world), null);
});

test('Dockview live serializer optional undefined fields do not invalidate saved JSON', () => {
  const saved = layout(); saved.dockview.panels.panel.tabComponent = undefined;
  assert.deepEqual(readLayout(saved), layout());
  saved.dockview.panels.panel.tabComponent = 'unapproved-renderer';
  assert.equal(readLayout(saved), null);
});
