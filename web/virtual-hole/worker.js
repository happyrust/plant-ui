// worker.js — 虚拟孔洞布尔预览的 Web Worker（ADR-0031）。
//
// 加载官方 manifold-3d（vendor/manifold.js，内核与原生端 C++ v3.5.2 一致），
// 按 meshKey 缓存焊接实体（LRU 64）。主线程只管界面和渲染，布尔运算不占帧线程。
//
// 协议见 crates/plant-csg/src/web.rs 的模块文档。

import Module from './vendor/manifold.js';
import { runJob, cacheGet, cacheSet, cacheDelete } from './boolean.js';

const mod = await Module();
mod.setup();

const cache = new Map();

self.onmessage = (event) => {
  const msg = event.data;
  if (!msg) return;
  if (msg.type === 'forget') {
    cacheDelete(cache, Number(msg.meshKey));
    return;
  }
  if (msg.type !== 'job') return;
  const job = msg.job;
  const meshKey = Number(job.meshKey);
  const cutters = job.cutters.map((c) => ({
    shape: c.shape,
    size: Array.from(c.size),
    affine: Array.from(c.affine),
    segments: c.segments | 0,
  }));
  const target = cacheGet(cache, meshKey);
  if (target) {
    postOutcome(job, runJob(mod, { target, cutters }), []);
    return;
  }
  if (!msg.positions) {
    // 缓存未命中且没带网格：让主线程带网格重提一次。
    self.postMessage({ type: 'outcome', jobId: job.jobId, epoch: job.epoch, status: 'NeedMesh' });
    return;
  }
  const result = runJob(mod, {
    mesh: { positions: msg.positions, indices: msg.indices },
    cutters,
    cache,
    meshKey,
  });
  const transfer = [];
  if (result.positions) transfer.push(result.positions.buffer);
  if (result.indices) transfer.push(result.indices.buffer);
  postOutcome(job, result, transfer);
};

function postOutcome(job, result, transfer) {
  const msg = {
    type: 'outcome',
    jobId: job.jobId,
    epoch: job.epoch,
    status: result.status,
    volumeBefore: result.volumeBefore || 0,
    volumeAfter: result.volumeAfter || 0,
    triCount: result.triCount || 0,
    elapsedMs: result.elapsedMs || 0,
  };
  if (result.error) msg.error = result.error;
  if (result.positions) msg.positions = result.positions;
  if (result.indices) msg.indices = result.indices;
  self.postMessage(msg, transfer);
}
