// virtual-hole-parity.mjs — 桌面 native 与浏览器 WASM 的布尔对拍（ADR-0031）。
//
// 拿 crates/plant-csg/tests/fixtures/parity.json 里的固定输入，用官方 manifold-3d
//（web/public/virtual-hole/vendor，与 worker.js 同款）跑同一套 boolean.js 算法，
// 跟原生端黄金结果比对。口径（见 web/public/virtual-hole/VENDOR.md）：
//   状态一致；三角数完全一致；体积相对误差 < 1e-6；包围盒误差 < 0.02mm。
//
// 用法：node scripts/virtual-hole-parity.mjs
// 退出码：0 全部通过，1 有失败。夹具由 cargo test -p plant-csg --features manifold
// --test parity 录制（缺失时自动写一份）。

import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import Module from '../web/public/virtual-hole/vendor/manifold.js';
import { runJob } from '../web/public/virtual-hole/boolean.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const fixturePath = path.join(here, '../crates/plant-csg/tests/fixtures/parity.json');
const fixture = JSON.parse(readFileSync(fixturePath, 'utf8'));

const mod = await Module();
mod.setup();

const VOLUME_REL_EPS = 1e-6;
const BBOX_EPS_MM = 0.02;

function relDiff(a, b) {
  return Math.abs(a - b) / Math.max(1.0, Math.abs(a), Math.abs(b));
}

function bboxOf(positions) {
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (let i = 0; i < positions.length; i += 3) {
    for (let axis = 0; axis < 3; axis++) {
      const v = positions[i + axis];
      if (v < min[axis]) min[axis] = v;
      if (v > max[axis]) max[axis] = v;
    }
  }
  return { min, max };
}

let failures = 0;
for (const testCase of fixture.cases) {
  const positions = new Float32Array(testCase.mesh.positions.flat());
  const indices = new Uint32Array(testCase.mesh.indices);
  const cutters = testCase.cutters.map((c) => ({
    shape: c.shape,
    size: c.size,
    affine: c.affine,
    segments: c.segments,
  }));
  const result = runJob(mod, {
    mesh: { positions, indices },
    cutters,
    cache: new Map(),
    meshKey: 1,
  });
  const expected = testCase.expected;
  const problems = [];
  if (result.status !== expected.status) {
    problems.push(`status: wasm=${result.status} native=${expected.status}`);
  }
  if (result.triCount !== expected.triCount) {
    problems.push(`triCount: wasm=${result.triCount} native=${expected.triCount}`);
  }
  if (relDiff(result.volumeBefore, expected.volumeBefore) > VOLUME_REL_EPS) {
    problems.push(`volumeBefore: wasm=${result.volumeBefore} native=${expected.volumeBefore}`);
  }
  if (relDiff(result.volumeAfter, expected.volumeAfter) > VOLUME_REL_EPS) {
    problems.push(`volumeAfter: wasm=${result.volumeAfter} native=${expected.volumeAfter}`);
  }
  if (result.status === 'Changed' && result.positions) {
    const bbox = bboxOf(result.positions);
    for (let axis = 0; axis < 3; axis++) {
      const dMin = Math.abs(bbox.min[axis] - expected.bboxMin[axis]);
      const dMax = Math.abs(bbox.max[axis] - expected.bboxMax[axis]);
      if (dMin > BBOX_EPS_MM || dMax > BBOX_EPS_MM) {
        problems.push(
          `bbox axis ${axis}: wasm=[${bbox.min[axis]}, ${bbox.max[axis]}] ` +
            `native=[${expected.bboxMin[axis]}, ${expected.bboxMax[axis]}]`
        );
      }
    }
  }
  if (problems.length === 0) {
    console.log(
      `PASS ${testCase.name} (${result.status}, ${result.triCount} tris, ` +
        `volume ${result.volumeAfter.toFixed(3)} mm^3, ${result.elapsedMs.toFixed(1)} ms)`
    );
  } else {
    failures++;
    console.error(`FAIL ${testCase.name}`);
    for (const problem of problems) console.error(`  - ${problem}`);
  }
}

if (failures > 0) {
  console.error(`\n${failures} case(s) failed`);
  process.exit(1);
}
console.log(`\nall ${fixture.cases.length} case(s) match the native golden`);
