# 虚拟孔洞预览只换三维画面，不写回正式模型（内存-only）

状态：**已接受**（2026-10-14。与用户两轮确认定案：浏览器端用官方 manifold-3d + Web Worker；
桌面端 manifold-csg 做默认开启的可选 feature；五个阶段一次做完；测试夹具在代码内生成；
`RelyItemREF` 按 `库号_参考号` 解析参考号。与计划书的偏差逐条记在本文末尾与计划书附录。）
计划：`docs/plans/2026-10-14-ams-virtual-hole-plannotator.md`。

## 背景

AMS 现场（AvevaMarineSample）的孔洞表（XLSX/CSV，一行一条记录：CIR 圆孔 / RECT 矩形孔，
带 Position 中心、Ori 姿态、SizeWidth / SizeHeight、SizeThrowWall 深度与 RelyItemREF）需要
在三维里**实时预览**开孔效果——设计师想先看一眼「这块板开了孔是什么样」，再决定要不要真正改模型。

风险在于「预览」很容易滑成「修改」：一旦布尔结果写回模型服务或落进模型库，一份拿来试看的
孔洞表就污染了正式模型，撤不干净。ADR-0028（直段截断）已经踩过一回类似的边界：临时几何
必须有明确的生命周期与恢复路径。本 ADR 把这条边界定死：**预览只存在内存，只换三维画面的
网格，正式模型一根头发都不动。**

## 决定

### D1　预览 = 临时替换实例的渲染网格，清预览恢复原网格

布尔结果网格只替换目标实例实体上的 `Mesh3d` 渲染句柄，并给实体挂 `HolePreviewed` 标记
（材质由 `apply_hole_materials` 接管）。恢复路径有三条，哪条先到先恢复：

- 用户点「清除」：整层撤掉，所有被换过的网格换回原始句柄，后端按 `mesh_key` 释放缓存。
- 换库（`reset_for_reconnect`）与清空场景（`clear_scene_for_reload`）：实体本身没了，
  预览层撤掉、会话作废或保留列表（换库整份作废，清场保留列表与映射）。
- 目标实体重装 / 卸载：视口发现被替换的实体一个都没了，预览失效并回执 `Failed(dropped)`，
  宿主把行状态落账。

不写回模型服务、不落库、不进版本管理、不改可见性台账、不碰正式模型文件。ADR-0030 定的
「显示只查、生成只在右键」在孔洞这里同样成立：预览触发不了任何模型生成，目标没加载时
宿主只是**显示**模型（走 ADR-0030 的只查显示），绝不生成。

### D2　布尔内核两端一套口径，对拍有黄金夹具

- 原生端：`manifold-csg`（gen-model ADR-029 同款转换逻辑的移植，C++ 内核 v3.5.2），
  专用线程跑，UI 帧只 submit / poll。
- 浏览器端：官方 `manifold-3d@3.5.4`（npm，内核与原生端一致）+ module Web Worker
  （`web/public/virtual-hole/worker.js`，算法在同目录 `boolean.js`）。
- 焊接（f64 位模式键、-0 归一）、退化三角形判据（WELD_EPS 1e-4）、圆柱段数口径两端逐行
  一致；对拍靠 `scripts/virtual-hole-parity.mjs` 拿同一份输入比对——三角数完全一致、
  体积相对误差 < 1e-6、包围盒误差 < 0.02mm。黄金夹具 `crates/plant-csg/tests/fixtures/parity.json`
  由 `cargo test -p plant-csg --features manifold --test parity` 录制 / 比对。

### D3　后端接口按作业提交 + 轮询，按内容键缓存焊接实体

计划书原定的 `SolidHandle` 句柄接口跨不过线程与 Worker 边界（句柄是裸指针，搬不动），
降为 `submit(BooleanJob) / poll() -> Vec<BooleanOutcome> / forget(mesh_key)`
（见 `crates/plant-csg/src/lib.rs`）。同一目标的全部切削体合并为一次 `difference`，
与孔洞顺序无关；焊接实体按网格内容哈希（`mesh_key`）做 LRU 64 缓存，`NeedMesh`
回包触发带网格重提一次。

### D4　相交筛选与结果落账在视口，宿主只管编排

- 只切 `RelyItemREF` 映射的实体；不切无关联但空间相交的管道和设备（计划阶段四第 7 条）。
- 粗筛：孔洞 OBB 做 SAT 判断 vs 实例网格的世界 AABB，不相交直接 `NoIntersection`，
  不提交作业。
- 精确：通过粗筛的实例等网格落地（10s 超时按失败），提交布尔作业；结果分四档——
  `Changed`（换网格）、`NoIntersection`（体积相对变化 < 1e-9 且三角数不变）、
  `Failed`（保留原网格，含空结果按 ADR-029 不覆盖）、`NeedMesh`（重提一次）。
- 回执攒在 `View3d` 上按帧号（`epoch`）回宿主落账；帧号对不上（清预览 / 重导入）作废。
- 目标一个实例都没有：回执 `no_instances`，宿主显示模型、等网格落地（`mesh_loaded`）
  自动重发一次预览，每目标只重试一次。

### D5　分工：plant-ui 管类型与页签，宿主管会话，View3d 管切削体层与布尔推进

- `plant-ui`：共享类型（`VirtualHole` / `VirtualHoleOp` / `VirtualHoleReceipt`）、
  `Cmd` 变体、`VirtualHolesVm` 投影、「虚拟孔洞」页签（右侧停靠）。
- 宿主（`plant-ui-app`）：`VirtualHoleSession`（孔洞列表、映射、状态机）、文件选择
  （原生 rfd 对话框线程 / 浏览器隐藏 input）、映射确认（`Req::VirtualHoleTargets`，
  服务端按 `face.ancestors` 链确认存在）、重试与回执落账、日志。
- `plant-ui-view3d`：切削体层（与 `SceneRoot` 平级，隔离 / 显示隐藏碰不到它）、
  切削体选中高亮与标签投影、布尔推进、材质接管。指令走独立的 `pending_hole_ops`
  通道交给视口，不走 `Cmd` / `view3d_commands`。

## 与计划书的偏差（5 条，均经用户确认）

1. **浏览器端**原定 `manifold-rs-wasm` 的 JS/WASM 绑定，改为官方 `manifold-3d@3.5.4`
   （npm）+ Web Worker——内核版本与原生端 C++ v3.5.2 对齐，对拍有抓手
   （`web/public/virtual-hole/VENDOR.md` 记了版本取舍）。
2. **桌面端** `manifold-csg` 不做硬依赖，挂默认开启的可选 feature `virtual-hole-csg`；
   关掉时预览不可用（`UnavailableBackend` 兜底，行状态如实报失败），导入与列表仍可用。
3. **范围**：五个阶段一次做完，不做分期交付。
4. **夹具**：测试夹具在代码内生成（XLSX 用 `zip` 现场造），不签入附件文件。
5. **`RelyItemREF`** 按 `库号_参考号`（如 `24381_145018`）解析参考号。

另外一条实现层面的调整（不改变用户可见行为）：阶段三「统一接口」的 `SolidHandle`
句柄降为作业提交 + 轮询（D3），原因见上。

## 构建前置

原生端打开 `virtual-hole-csg` 时，`manifold-csg-sys` 首次构建会 git clone manifold3d
（v3.5.2）并用 cmake 编译——需要 git、cmake 与 C++ 编译器（本仓 `vendor/registry/cmake`
已 vendored，离线可用）；之后构建走缓存。完全离线的构建器可以用 `MANIFOLD_CSG_LIB_DIR`
直接链接预编译的 manifold 库（跳过 clone 与 cmake，见 manifold-csg 仓 README）。

## 后果

- **内存**：预览网格只活在 `Assets<Mesh>` 与后端 LRU 里；清预览 / 卸载当场释放，
  不留后手。大模型多目标同时预览时峰值内存会涨——夹具级实测单孔 13 ms，批量异步排队。
- **X-Ray 互斥**：孔洞 X-Ray 与房间 X-Ray 抢同一份半透明材质，后拨的赢
  （`begin_room_xray` 会关掉孔洞 X-Ray，反之亦然）。
- **会话脆弱性**：预览绑定网格内容（`mesh_key`），模型重生成出新网格后旧预览的
  `mesh_key` 不再命中——实体重装的「预览失效」回执就是干这个的，宿主如实落账，
  不假装预览还在。
- **两个源码形态测试**：`plant-ui` 里有两个源码文本测试钉住 `pub enum Cmd {` 与
  `fn handle_cmds` 的形状（防止命令枚举被意外拆散）；本次给 `Cmd` 加了 8 个变体，
  这两个测试是预期内的钉子，改 `Cmd` 结构时要一并看它们。

## 为什么不是别的做法

- **把结果写回模型服务 / 模型库**：一份试看用的孔洞表会污染正式模型，撤回成本远高于
  预览本身；ADR-0030 已经把「显示不生成」定成铁律，预览触发写回直接违反。
- **浏览器端用 manifold-rs-wasm（Rust manifold 编 wasm）**：与原生端 C++ v3.5.2 不是
  同一份内核版本，三角数 / 体积对拍没有抓手；官方 manifold-3d 与原生端同版，
  黄金夹具可以钉到「三角数完全一致」。
- **主线程跑布尔**：单孔单实体目标虽只要十几毫秒，但大模型多目标会拖垮 UI 帧；
  原生端专用线程、浏览器端 Worker，UI 只轮询回执。
