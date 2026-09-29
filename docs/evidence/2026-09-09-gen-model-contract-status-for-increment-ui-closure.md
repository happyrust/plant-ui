# gen-model S5–S9 契约现状 · plant-ui 增量 UI 收口的三个卡点（U3 / U4 / V3）

> 起因：核对 `ui/手动增量更新.pen` 那几张监控画板是否都已实现。答案是**没有全部实现**——
> 未实现的部分几乎全在等 gen-model 的 external 增量重构（S5–S9）给出**消费侧契约字段**。
> 本文把「gen-model 那边到哪一步、plant-ui 这三个里程碑各等哪一格」钉在一处备查。
> 依据两份计划：`docs/plans/2026-09-08-manual-update-is-a-dbnum-task-run-early.md`（U0–U5）、
> `docs/plans/2026-09-09-task-finished-detail-plan.md`（V0–V4）。

- 日期：2026-09-09（UTC+8）
- 性质：跨仓现状核对，只记事实与出处，不改设计、不动代码。
- 范围：读 gen-model 工作树（分支 `e3d-direct`）与 plant-ui 两份计划；grep 两仓 `src/`。
- 关键实位：gen-model 顶到 `0afe581be`（2026-09-08，规划器元数据门）；`git log --all` 无
  `dbnum_task` / S8 / 回执房间段提交。plant-ui 分支 `codex/increment-ui-closure`，源码无未提交改动。

## 一、一句话

plant-ui 侧**不依赖新契约**的都做完了（U1 / U2、V1 / V2 已落地并 sim 目检）；剩下的
**U3（一行四段进度 + ModelOnly 徽标 + 两种排队语义）、U4（三段执行计划书 + 失败留痕）、
V3（终态房间段）** 全部**卡在 gen-model 尚未产出的消费侧字段上**，现在都不能解锁。

## 二、`手动增量更新.pen` 七张画板的实现状态

| 画板 | 里程碑 | 状态 |
|---|---|---|
| 本期执行范围 · 立即执行列 (`dFvJ0`) | U2 | ✅ 完整 |
| 任务队列 · dbnum 任务形态 (`zS7Mo`) | U2 + **U3** | ◑ 行尾「立即执行」列已落；**核心的一行四段进度行未做** |
| 提前执行 · 悬停气泡 (`DkpHD`) | U2（粗版） | ◑ 粗版已落；**房间段 + 精确模型数未做** |
| 更新任务详情 · 终态行内明细 (`BgdMa`) | V1 + V2（房间 **V3**） | ◑ 数据段三数 + 模型段刚体前移已落；**房间段未做** |
| 更新任务详情 · 段形态表 (`y1o7hP`) | V1（房间 **V3**） | ◑ 数据分解缺席一态已落；**四种房间段形态未做** |
| 向导 · 执行计划书 (`J3OXRE`) | **U4** | ✗ 仅旧的单段预览；三段计划书未做 |
| 启动那一幕 · 初始化串行与仅模型任务 (`mPvbM`) | **U3** | ✗ 未做 |

（`bi8Au` 是空占位帧，不计。）

## 三、gen-model S4b–S9 现状（refactor-plan §6 状态列 + T4 工单）

| 里程碑 | 是什么 | 状态 |
|---|---|---|
| S4b / ADR-066 | 「只动方位」Transform 档 | ✅ 已落——**plant-ui V2「刚体前移 N 根」的源，已消费** |
| S5（T1） | 稳态 Task 内联模型 | ◑ 部分：Task 内生成 / 逐根结果 / 发布后推严格水位已落；窗口计划仍经 pending 交接 |
| S5a | 模块搬家（G1） | ✗ 未做 |
| S6（T2） | ModelOnly Task（三条路改排） | ✗ 未做（`batch_queue.rs` 有 `model_only` 入队名，但三条路未改排、对外任务未换） |
| S7（T3） | 初始化内联 + FIFO 串行 | ◑ 部分：门控已落，init run 退役未完 |
| S8（T4） | 房间内联 + **回执房间段定形** | ✗ 未做（只 T4.0 GroupCache 落地；产出字段的 T4.4 未开工） |
| S9（T5） | pending 表 + drain 退役 | ✗ 未做 |

## 四、plant-ui 消费侧字段逐格核对

- `task.kind` 仍是 `data_batch` / `model_drain` / `room_recalc`（gen-model `src/data_interface/task_registry.rs:19/21/27`），
  **无 `dbnum_task`**，四段分段事件也无。
- 预览房间段**无源**（S8 未做）。
- Task 回执房间字段 `rooms.{panels_recalculated, elements_recalculated, touched[], moves[], failed[]}`
  与结构化 `metadata_only_roots` 在 gen-model `src/` **零命中**；元数据门只在 `warnings[]` 里留一行文本，
  plant-ui 按 ADR-0019 纪律**不解析文本反拼数字**。
- 别混：gen-model `manual_update.rs` 的 `moved_in / moved_out` 是**交付单元**级迁入迁出（模型段），不是房间搬间。

## 五、三个卡点各等什么

| plant-ui | 等 gen-model | 现状 |
|---|---|---|
| U3 一行四段 + ModelOnly + 排队语义 | S6 / S7 + `task.kind` 定形 | S6 未做、S7 部分；`task.kind` 仍旧三种 → 无源 |
| U4 三段执行计划书 + 失败留痕 | S5 / S8 | 预览房间段等 S8；S8 未做 |
| V3 终态房间段（四形态 + 段头计数 + 搬间明细 + 失败框） | S8 / T4.4 | T4.4 未开工 |

## 六、T4（S8）现在也动不了

T4 工单（gen-model `docs/plans/2026-09-09-t4-room-into-task-work-order.md`）§3 协调前提：
`batch_worker.rs` / `model_update_pending.rs` / `room_resident.rs` / `room_startup.rs` 有别的会话
**约 1.9 万行在途未提交**（Task 收尾重排、completion S1 生命周期）。T4.1 起直接重排这些区域，
**须先确认在途会话已收笔或由用户裁移交**，否则按 d-91 先例单独拉 worktree 做。
即：要解锁 plant-ui 的 U3 / U4 / V3，先得在 gen-model 那边清掉这个前提、再走 T4.1 → T4.4。

## 七、核对证据

- gen-model：`git rev-parse --abbrev-ref HEAD` = `e3d-direct`；顶 `0afe581be`（2026-09-08）；
  `git log --all` 搜 `dbnum_task` / `S8` / `receipt` / `回执` 无命中。
- grep gen-model `src/`：`panels_recalculated` / `elements_recalculated` / `metadata_only_roots` 零命中；
  `task_registry.rs` 只有旧三 kind；`batch_queue.rs:31` `Self::ModelOnly => "model_only"`。
- 文档：refactor-plan §6 状态列、`2026-09-09-t4-room-into-task-work-order.md`（T4.0 已落 / T4.1–T4.6 未做）、
  `2026-09-09-s8-task-receipt-room-fields-consumer-input.md`（房间字段消费者对照，待 S8 定形）。
- plant-ui：分支 `codex/increment-ui-closure`，源码无未提交改动；`task_queue.rs` 命中
  `early_run`（U2）、`change_breakdown` / `ratio_bar` / `change_total`（V1）、`transform_roots`（V2）；
  房间 / 四段 / 初始化串行相关文案（`无数据窗口` / `初始化：第` / `排他串行` / `仅模型` / 房间`搬间`）零命中。
