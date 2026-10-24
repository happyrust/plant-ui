# virtual-hole 第三方资产

## vendor/manifold-3d（官方 manifold WASM 绑定）

- 来源：npm 包 `manifold-3d@3.5.4`（`npm pack manifold-3d@3.5.4`）。
- 文件：`manifold.js`、`manifold.wasm`、`LICENSE`（Apache-2.0）、`package.json`、`README.md`。
- 版本取舍：原生端 `manifold-csg` 钉的是 C++ `v3.5.2`；npm 没有 3.5.2 这一版，
  3.5.3 / 3.5.4 相对 3.5.2 只有发布流水线与 CSP 修复（见 GitHub release notes），
  布尔内核没有变化，所以取 3.5.4。对拍靠 `scripts/virtual-hole-parity.mjs`
  （三角数完全一致、体积相对误差 < 1e-6、包围盒误差 < 0.02mm）。
- 用途：`worker.js`（module Worker）里跑布尔预览；算法在同目录 `boolean.js`，
  与原生端 `crates/plant-csg` 逐行对齐。
- 更新：换版本时重新 `npm pack`、覆盖 `vendor/` 四个文件，并跑一遍对拍脚本。
