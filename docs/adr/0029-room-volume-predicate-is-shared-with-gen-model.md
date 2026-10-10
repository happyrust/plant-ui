# 房间体的内外判定与 gen-model 共用一份纯库

直段截断要问「这一点在不在房间体里」，gen-model 算房间归属问的是同一个问题。那份判定是踩过坑才定下来的：
2026-09-18 之前走 parry `contains_point`，盒外一米的点也能被判成成员；之后改成焊接 + 剔退化三角 + 三射线奇偶
三取二（gen-model `src/fast_model/room_predicate.rs`）。**决定：把 `room_predicate.rs` 的纯几何部分（面板三角网
构建、点包含、八角点计数）原样抽成纯库 `plant-room`（照 plant-mbd 的先例：只依赖 glam + parry3d，不读库、
不走 HTTP、wasm 编得动），新写的「线段在房间体内的区间」也放在这里。gen-model 的归属计算与 plant-ui 视口的
截断都只调它。**

## Considered Options

- **客户端照抄一份**：最快，但两份一旦漂移，同一个点在归属里算内、在截断里算外，而且什么都不会报错
  （`regenerate.rs` 那句「两份实现漂移的起点」是同一类教训）。被否。
- **服务端算好每段区间下发**：要新表或新端点，还要随每次房间重算一起重算；直段在客户端没有身份（refno 是
  BRAN），对不上号；房间今天只走库（ADR-0026）。被否。

## Consequences

- **现状（2026-09-30）**：库暂住 plant-ui 工作区 `crates/plant-room`（工作区成员）。原定放在两仓旁边的
  `old/vendor/plant-room`，但那个位置在两个工作区之外，写入没有放行，先落在这里。plant-ui 这一侧 parry3d 走
  path 依赖，指向与 rs-core 同一份 fork（`vendor/forks/parry`），依赖图里只有一份 parry，不用打补丁；
  库里 `pub use parry3d`，宿主经它取 `TriMesh` / `Aabb`，版本不会错开。
- **还没做（计划 M0 后一半）**：gen-model 改用本库。届时库挪到两仓都够得着的位置（`old/vendor/plant-room`，
  或 gen-model 直接 path 引用这里），gen-model 用 `[patch.crates-io]` 把 parry3d 钉到它那份 gitee fork，
  否则依赖图里会有两份 parry，编译期就对不上类型。抽库要走 gen-model 自己的评审与 ADR，而且必须零行为变化，
  以它现有的房间测试逐条为准（真网格那条 R320 用例留在 gen-model，因为 `.mesh` 反序列化要 aios_core）。
  在那之前两边各有一份判定：本库是按 gen-model 2026-09-18 版逐字搬的，只把入参从 `&PlantMesh` 换成了切片。
- 共用的是「点在不在房间体内」，不是归属口径。归属仍按 AABB 八角点 + 顶点兜底，截断按轴线区间，两者会有
  合理的出入：擦墙的直段是成员，画面上却没有。
- 线段求穿越点不走 BVH 的射线遍历：管轴多半沿坐标轴，打在墙面三角零厚度包围盒的边界上是浮点里 `0 × ∞`
  的边角。改为逐三角 f64 包围盒粗筛再求交；点包含那三条射线是歪斜的，照旧走 BVH。
- 计划：`docs/plans/2026-09-29-room-view-tube-truncation.md`（M0）。
