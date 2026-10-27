// boolean.js — 虚拟孔洞布尔预览的纯算法（ADR-0031）。
//
// 与原生端 crates/plant-csg/src/{lib,convert,native}.rs 逐行对齐——改一处要改另一处。
// 两个消费者：worker.js（浏览器端预览）和 scripts/virtual-hole-parity.mjs（Node 对拍）。
//
// 入口 runJob(mod, input) 里的 mod 是 manifold-3d 的 Emscripten 模块（已 setup）。

export const WELD_EPS = 1e-4;
const NO_INTERSECTION_VOLUME_EPS = 1e-9;
const CACHE_CAP = 64;

// 焊接：按精确坐标（f64 位模式，-0 归一成 0）合并重复顶点。
// JS 里 String(-0) === "0"，f32 → f64 精确，所以字符串键与原生端 to_bits 口径一致。
export function weldMesh(positions, indices) {
  const vertProps = [];
  const vertexIds = new Uint32Array(positions.length / 3);
  const welded = new Map();
  for (let i = 0; i < positions.length; i += 3) {
    const x = positions[i];
    const y = positions[i + 1];
    const z = positions[i + 2];
    const key = `${x},${y},${z}`;
    let id = welded.get(key);
    if (id === undefined) {
      id = vertProps.length / 3;
      welded.set(key, id);
      vertProps.push(x, y, z);
    }
    vertexIds[i / 3] = id;
  }
  const tri = new Uint32Array(indices.length);
  for (let i = 0; i < indices.length; i++) tri[i] = vertexIds[indices[i]];
  return { vertProps, tri };
}

// 退化三角形判据（mm）：任一边不超过 WELD_EPS（f64 上算），或 f64 零面积。
export function isDegenerateTriangle(a, b, c) {
  const sub = (p, q) => [p[0] - q[0], p[1] - q[1], p[2] - q[2]];
  const d2 = (p, q) => {
    const d = sub(p, q);
    return d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
  };
  const eps2 = WELD_EPS * WELD_EPS;
  if (d2(a, b) <= eps2 || d2(b, c) <= eps2 || d2(c, a) <= eps2) return true;
  const ab = sub(b, a);
  const ac = sub(c, a);
  const cross = [
    ab[1] * ac[2] - ab[2] * ac[1],
    ab[2] * ac[0] - ab[0] * ac[2],
    ab[0] * ac[1] - ab[1] * ac[0],
  ];
  const len = Math.sqrt(cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]);
  return !(len > 0.0);
}

// 丢掉退化三角形后的索引（顶点表不变，允许残留孤立顶点）。
export function dropDegenerateTris(vertProperties, triVerts) {
  const at = (i) => [
    vertProperties[i * 3],
    vertProperties[i * 3 + 1],
    vertProperties[i * 3 + 2],
  ];
  const kept = [];
  for (let t = 0; t < triVerts.length; t += 3) {
    const i = triVerts[t];
    const j = triVerts[t + 1];
    const k = triVerts[t + 2];
    if (i * 3 >= vertProperties.length || j * 3 >= vertProperties.length || k * 3 >= vertProperties.length) {
      continue;
    }
    if (isDegenerateTriangle(at(i), at(j), at(k))) continue;
    kept.push(i, j, k);
  }
  return new Uint32Array(kept);
}

// 焊接实体缓存（LRU，与原生端 CACHE_CAP 同值）。Map 保持插入序。
export function cacheGet(cache, key) {
  const solid = cache.get(key);
  if (solid) {
    cache.delete(key);
    cache.set(key, solid);
  }
  return solid;
}

// 换掉 / 淘汰的实体要 delete()：manifold-3d 是 Emscripten 绑定，不删就留在 WASM 堆里。
export function cacheSet(cache, key, solid) {
  const old = cache.get(key);
  if (old) {
    cache.delete(key);
    if (old !== solid && old.delete) old.delete();
  } else if (cache.size >= CACHE_CAP) {
    const oldest = cache.keys().next().value;
    const evicted = cache.get(oldest);
    cache.delete(oldest);
    if (evicted && evicted.delete) evicted.delete();
  }
  cache.set(key, solid);
}

export function cacheDelete(cache, key) {
  const solid = cache.get(key);
  if (solid && solid.delete) solid.delete();
  cache.delete(key);
}

// 跑一个作业：一个目标实体减去它的全部切削体（合并为一次 difference，
// 上游给的是列主序 4×3 仿射（12 元素）；manifold-3d 的 `transform` 要列主序 4×4
// （16 元素）——补一行 [0,0,0,1]。两端矩阵口径必须一致，见 parity 脚本。
function affine4x3ToMat4(affine) {
  return [
    affine[0], affine[1], affine[2], 0,
    affine[3], affine[4], affine[5], 0,
    affine[6], affine[7], affine[8], 0,
    affine[9], affine[10], affine[11], 1,
  ];
}

// 与孔洞顺序无关）。input.target 命中缓存时直接用；否则 input.mesh 必须在。
export function runJob(mod, input) {
  const started = performance.now();
  const done = (result) => ({ elapsedMs: performance.now() - started, ...result });
  // manifold-3d 是 Emscripten 绑定：Manifold 实体要显式 delete() 才释放 WASM 堆。作业里临时造的基元、
  // 摆好位的切削体、差集结果都记在这儿，出口统一释放；只有进了缓存的目标实体留着（由 cacheSet / cacheDelete 管）。
  // getMesh() 拷出来的 typed array 不随 result 一起没。
  const transient = [];
  const release = () => {
    for (const solid of transient) {
      if (solid && solid.delete) solid.delete();
    }
    transient.length = 0;
  };
  try {
    let target = input.target;
    if (!target) {
      if (!input.mesh) return done({ status: 'NeedMesh' });
      const { vertProps, tri } = weldMesh(input.mesh.positions, input.mesh.indices);
      const mesh = new mod.Mesh({
        numProp: 3,
        vertProperties: new Float32Array(vertProps),
        triVerts: tri,
      });
      // 状态非 NoError 会抛 ManifoldError——与原生端 ingest 失败同口径。
      target = mod.Manifold.ofMesh(mesh);
      if (input.cache) cacheSet(input.cache, input.meshKey, target);
      else transient.push(target);
    }
    const volumeBefore = target.volume();
    const triBefore = target.numTri();
    const group = [target];
    for (const c of input.cutters) {
      // 单位基元：圆柱半径 0.5、高 1，或 1×1×1 箱，以原点为中心、轴沿 Z。
      // 膨胀（每边让 0.051mm）在上游 VirtualHole::cutter_mat4 里已折进 affine。
      const prim =
        c.shape === 'Circle'
          ? mod.Manifold.cylinder(1.0, 0.5, 0.5, Math.max(3, c.segments | 0), true)
          : mod.Manifold.cube(1.0, 1.0, 1.0, true);
      const placed = prim.transform(affine4x3ToMat4(c.affine));
      transient.push(prim, placed);
      group.push(placed);
    }
    const result = mod.Manifold.difference(group); // DifferenceN：头减其余
    transient.push(result);
    const status = result.status();
    if (status !== 'NoError') throw new mod.ManifoldError(status);
    if (result.isEmpty()) {
      // ADR-029：空结果不覆盖原网格。
      return done({ status: 'Failed', error: 'difference result is empty', volumeBefore });
    }
    const volumeAfter = result.volume();
    const triCount = result.numTri();
    const unchanged =
      Math.abs(volumeAfter - volumeBefore) <=
        NO_INTERSECTION_VOLUME_EPS * Math.max(1.0, Math.abs(volumeBefore)) &&
      triCount === triBefore;
    if (unchanged) {
      return done({ status: 'NoIntersection', volumeBefore, volumeAfter, triCount });
    }
    const mesh = result.getMesh();
    const indices = dropDegenerateTris(mesh.vertProperties, mesh.triVerts);
    return done({
      status: 'Changed',
      positions: mesh.vertProperties,
      indices,
      volumeBefore,
      volumeAfter,
      triCount,
    });
  } catch (err) {
    return done({ status: 'Failed', error: String((err && err.message) || err) });
  } finally {
    release();
  }
}
