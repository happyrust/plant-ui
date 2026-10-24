# AMS 虚拟孔洞导入与实时布尔预览 · 验证记录（ADR-0031）

计划：`docs/plans/2026-10-14-ams-virtual-hole-plannotator.md`（含实施偏差附录）。
决策：`docs/adr/0031-virtual-hole-preview-is-memory-only.md`。
工作树：plant-ui `master`，本次改动未提交（本地工作树）。

## 1. 自动化

- `cargo test -p plant-ui --lib`：**189 passed / 0 failed**。含 `virtual_hole` 解析 10 例
  （CIR/RECT 规格、姿态矩阵、AABB 相交、膨胀口径）与 `workbench::virtual_holes` 页签 2 例。
- `cargo test -p rs-plant --bin rs-plant`：**196 passed / 0 failed**。含
  `virtual_hole_session` 8 例（导入替换与唯一 ID、帧号作废、映射确认、回执落账四档、
  清预览、重置）与 `virtual_hole` 导入器 8 例（CSV/XLSX、列边界兼容、行级错误不阻断）。
- `cargo test -p plant-csg --features manifold`：**13 passed / 0 failed**（lib，真实内核）
  + `cargo test -p plant-csg --features manifold --test parity`：**1 passed / 0 failed**
  （黄金夹具比对：状态一致、三角数完全一致、体积相对误差 < 1e-9、包围盒误差 < 1e-3mm）。
- `cargo test -p plant-ui-view3d`：**75 passed / 0 failed**。含 `virtual_hole` 10 例：
  切削体层生成与选中高亮、无实例回执 `no_instances`、远距离孔洞不过提交直接
  `NoIntersection`、`Changed` 换网格 + 回执、显示原始切换、飞行中卸载作废、预览后卸载
  失效回执、孔洞 X-Ray 材质、清除恢复原始网格并撤层、圆切削体网格与布尔段数同源。
- `node scripts/virtual-hole-parity.mjs`：**3/3 通过**——浏览器 WASM（官方 manifold-3d 3.5.4，
  与 worker.js 同款）对原生端黄金：
  - `through_circle`：Changed，三角数 400 = 400，体积 206819796.322 mm³ 双端一致，13.3 ms
  - `circle_and_rect`：Changed，三角数 420 = 420，体积 158791232.161 mm³ 双端一致，4.8 ms
  - `far_away`：NoIntersection，体积不变，0.7 ms
  - 单孔单实体门槛（< 200ms）：实测 13.3 ms（浏览器 Worker 内），原生端专用线程同量级。
- `cargo check -p rs-plant --bin rs-plant`（默认 features）与
  `cargo check -p rs-plant --bin rs-plant --no-default-features`（`UnavailableBackend` 路径）：
  均通过，本仓无新增告警。
- `scripts/build-wasm.ps1`：**通过**。wasm32-unknown-unknown release 构建 +
  wasm-bindgen（--target web）产出 `web/public/pkg/rs-plant.js` 与 `rs-plant_bg.wasm`；
  `web/public/virtual-hole/vendor/manifold-3d@3.5.4` 四件套在位。

### 对拍暴露并修复的缺陷

- `boolean.js` 原把列主序 **4×3**（12 元素）仿射直接喂给 manifold-3d 的 `transform`
  （要 16 元素列主序 4×4）——矩阵读到垃圾，切削体退化成零体积，差集白跑
  （三角数乱变、体积不变）。对拍脚本第一跑即抓住；修成 4×3→4×4 补 `[0,0,0,1]` 行后
  三端（native / WASM / 黄金）完全一致。这是「对拍脚本先行」的直接回报。
- `plant-csg` 后端要住进 Bevy `Resource`（要求 Send + Sync）：原生端 `Receiver` 套了
  `Mutex`，浏览器端 `Worker`/`Rc` 包了 `SendSync`（wasm32 单线程，安全见注释）。
- `HoleFilePick`（浏览器端）持有 `Rc`/`Closure` 不满足 `Send`，而 `App` 要进
  `Local<Option<App>>`——同样包 `SendSync`。

### 已知的钉子（预期内，不是失败）

- `plant-ui` 有两个源码文本测试钉住 `pub enum Cmd {` 与 `fn handle_cmds` 的形状；
  本次给 `Cmd` 加了 8 个变体（导入 / 选中 / 映射 / 预览 / 清预览 / 定位 ×2 / 原始 / X-Ray），
  测试仍绿——改 `Cmd` 结构时要一并看它们。
- vendor 依赖（parry / rstar / gpu-allocator / bevy_egui35）的既有告警未动，不在本次路径。

## 2. 手工验收（计划阶段五「应用测试」，待用户真界面跑）

环境：本机服务 + AvevaMarineSample 库，桌面端 `cargo run -p rs-plant` 与浏览器端
`web/public` 静态页（wasm 产物已构建）。

- [ ] 原生端：项目菜单「导入虚拟孔洞…」选 XLSX，3 条记录进列表，坏行进「解析错误」
- [ ] 浏览器端：同一份表走隐藏 input 导入，记录一致
- [ ] 自动映射（RelyItemREF 命中库里元素）与手动映射（库里没有 → 手动指定）
- [ ] 「全部预览」：计算中计数 → 行状态「已切割」；切削体琥珀半透明；单孔响应 < 200ms
- [ ] 无相交孔洞行状态「无相交」，失败行保留原网格且状态「失败」
- [ ] 「显示原始」切回原网格；「X-Ray」下被切目标半透明，与房间 X-Ray 互斥后拨的赢
- [ ] 「定位孔洞」（相机取景到孔洞包围盒）与「定位目标」（树展开 + 取景到目标模型）
- [ ] 「清除」恢复所有原始网格；换库 / 清场后预览撤掉、列表按 ADR-0031 规则保留或作废
- [ ] Worker 运行时相机旋转与拾取保持响应（布尔不占主线程）
- [ ] 同一输入桌面 native 与浏览器 WASM 的包围盒、体积、三角数一致（脚本已钉，
      `node scripts/virtual-hole-parity.mjs` 3/3）

手工跑完后在本节补勾并记录时间。
