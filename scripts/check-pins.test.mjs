// `node --test scripts/check-pins.test.mjs`: the pins check's pure parts.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { pinsIn, isVersion, compareVersions, behind } from './check-pins.mjs';

test('finds the pinned releases in the code, the newest tag of each, and where', () => {
  const files = [
    ['src-tauri/src/a.rs', 'url: "https://github.com/example-org/engine/releases/download/v0.2.0/engine-0.2.0-win.zip",\n'],
    ['src-tauri/src/b.rs', 'x\n  "https://github.com/example-org/engine/releases/download/v0.1.9/old.zip"\n"https://github.com/example-org/models/releases/download/speaker-models/a.onnx"'],
  ];
  assert.deepEqual(pinsIn(files), [{ repo: 'example-org/engine', tag: 'v0.2.0', where: 'src-tauri/src/a.rs:1' }]);
});

test('versions are told from asset folders, and ordered by their numbers', () => {
  assert.ok(isVersion('v1.13.8') && isVersion('1.52.0') && isVersion('b11960') && isVersion('0.10.6'));
  assert.ok(!isVersion('speaker-segmentation-models') && !isVersion('latest'));
  assert.ok(compareVersions('v1.13.8', '1.13.10') < 0);
  assert.ok(compareVersions('b10441', 'b11960') < 0);
  assert.equal(compareVersions('v0.2.0', '0.2.0'), 0);
});

test("a pin is behind when upstream is newer; a fast project's floor only after its grace", () => {
  assert.equal(behind({ repo: 'example-org/engine', tag: 'v0.2.0' }, 'v0.3.0'), true);
  assert.equal(behind({ repo: 'example-org/engine', tag: 'v0.3.0' }, 'v0.3.0'), false);
  const floor = { repo: 'ggml-org/llama.cpp', tag: 'b11900' };
  assert.equal(behind(floor, 'b11960', '2026-10-20T00:00:00Z', '2026-10-28T00:00:00Z'), false);
  assert.equal(behind(floor, 'b11960', '2026-09-01T00:00:00Z', '2026-10-28T00:00:00Z'), true);
});
