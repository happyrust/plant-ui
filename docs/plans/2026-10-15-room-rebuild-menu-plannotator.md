# 「重新生成房间」菜单与生成路径简化开发计划

## 当前基线

仓库：`D:/work/plant-code/old/gen-model`（refactor 分支）+ `D:/work/plant-code/old/plant-ui`

已经存在、本次直接复用的机制：

- 服务端启动即自动构建常驻房间参照模型（`room_resident.rs`：`initialize_file_backed_at_startup` / `initialize_latest_at_startup` → `warm_for_install` → `install` → `mark_ready`）。这是「没有房间数据时的第一次初始化」，后续房间变化全走增量换代（`ensure_current`）。
- 生命周期门 `ensure_ready()`：`Loading`/`Blocked` 拒绝，其余放行；错误含 `RESIDENT_MODEL_UPDATING` 标记。
- 持久化房间数据 = 两张边表 `room_relate`（面板→成员）与 `room_panel_relate`（房间→面板）；停放链在 `room_reverse`。
- `RoomOnly` 整库房间重算已存在（`model_db_adapter.rs` 按本库全部根的现有几何重写两张边表），并经 `write_state::WriteIntent::RoomOnly` → `BatchIntent::ApplyWindow` 走队列。
- 电平型欠账：`dbnum_watermark.model_room_generation` ≠ 常驻代号 ⇒ 该库欠一次整库重算，下个任务自动补。
- 上一轮已交付（本次部分保留、部分撤除）：
  - 保留：`resident_health` 的 `progress` 段、`409/updating` 错误归类、客户端 `ResidentStatus` 归一 + `Evt::ResidentWait` + `Vm::WaitingResident` 等待屏。
  - 撤除：`/model/generate` 里的 `wait_ready` 有界等待、`data.rs` 生成循环的发出前预检/重试/取消、`Req::GenerateModels.cancel`、`MutationRun.cancel`。

方向（用户拍板）：模型指定重新生成**不管**房间模型的生成（启动自动建；没建好就失败、按可重试报），但照常做房间计算。另加一枚「重新生成房间」菜单：删掉全部房间数据 → 重建常驻参照模型 → 重新计算房间关系。

## 决策摘要

| 决策 | 结论 |
|---|---|
| 等待链处置 | 生成路径回快败；进度/等待屏改给房间重建用 |
| 菜单位置 | 任务队列页库行面板头部，全局一枚「重新生成房间（全库）」 |
| 删除范围 | 两张边表全库清空 + 停放链清空 + resident 换代；**不删 mesh 缓存** |
| 重建并发 | 重建期间生成请求照旧 `UPDATING`→409，确认框里写明 |
| 触发形态 | `POST` 返 202，客户端轮询 `/health.room_build.resident` 画进度 |
| 重算方式 | 重建完成后给每个在册库立刻排一条 `RoomOnly` 写意图 |

## 阶段一：服务端 `POST /api/v1/rooms/rebuild`

### 目标

一个显式入口完成「删全部房间数据 → 重建常驻模型 → 排全库重算」，幂等可重发，并发安全。

### 实现

`src/web_service/handlers.rs` 新增 `rooms_rebuild`，`mod.rs` 注册 `post(handlers::rooms_rebuild)`：

1. 前置门：`room_membership()` 关 → `409 unsupported`；生命周期已是 `Loading` → `409 updating`（一次只许一趟重建）。
2. `mark_loading("source-snapshot")`（进度槽随之清零，客户端立刻看得到）。
3. 清持久层：`DELETE room_relate;` `DELETE room_panel_relate;`（全库，不限 dbnum——语义照抄 `fast_delete` 的姐妹边纪律）；清全部库的停放链（`room_reverse` 需要补一个全清入口，现状 `clear(dbnum)` 按库）。
4. 重建常驻：把 `initialize_file_backed_at_startup` 的「load → `mark_loading("mesh-warm")` → `warm_for_install` → `lock_spatial_serial` → `install` → `mark_ready`/`mark_failed`」一段提成可复用函数，startup 与本端点共用；`ResidentForm` 选择逻辑与 startup 一致。
5. `mark_ready` 之后：对每个在册库（`expected_dbnums` ∩ 监听集）排一条 `RoomOnly` 写意图（经 `write_state` → `batch_queue` 现有入队缝），回执列出排了几个库。即便排队失败，电平型欠账仍兜住——代号不等，下个任务自动整库重算，不会丢。
6. 响应 `202`：`{ status: "accepted", phase, queued_dbnums: [...] }`；失败按 `ApiError` 结构回。

### 验收

- `POST` 后 `/health` 立刻显示 `loading` + `source-snapshot` 进度；建完 `ready` 且 `startup`/`last_swap` 换代。
- 两张边表行数归零→`RoomOnly` 跑完后逐库回涨。
- 重建中 `POST /model/generate` 得 `409 updating`；重建完成后同请求正常。
- 重复 `POST`：第二次得 `409 updating`，不并发双建。

## 阶段二：生成路径回简

### 目标

`model_generate` 不再等房间模型——`ensure_ready` 在冻房间计算器时把关，撞上 `Loading` 就是 `409 updating`（上一轮已归好类）。

### 实现

- `handlers.rs`：删 `model_generate` 里的 `wait_ready(...)` 调用；`wait_ready` / `lifecycle_channel` / `set_lifecycle` 的 watch 广播一并删（无人用，不留死代码）；进度槽与 `resident_health.progress` 保留。
- `data.rs`：`Req::GenerateModels` 去掉 `cancel`；生成循环恢复顺序直发；删 `wait_resident_ready` 与 `RESIDENT_POLL_UPDATES` / `RESIDENT_WAIT_LIMIT`；`handle_read` 的 `task_ctx` 形参若只服务等待则一并回退。`resident_building` 识别保留——生成回执撞上它时，失败文案指向「稍后重试或先跑『重新生成房间』」。
- `main.rs`：`MutationRun.cancel` 删除；`settle_mutation_confirm` 的等待档取消分支删除；`Evt::ResidentWait` 分发保留但只挂房间重建那一趟（见阶段三）。

### 验收

- `cargo check` 两仓过；`git diff` 里生成路径回到改动前形状（除错误文案）。
- 老服务端（无 `room_build` 格）行为与今天完全一致。

## 阶段三：客户端「重新生成房间」菜单

### 目标

队列页可触发全库房间重建，重建期间实时进度，结果可见。

### 实现

- `plant-ui/src/task_queue.rs`：库行面板头部加按钮「重新生成房间（全库）」；`room_membership` 关 / resident `unsupported` 时禁用并 hover 说明；已有重建在跑（`status==loading`）时按钮灰。
- `plant-ui/src/lib.rs`：`Cmd` 加 `RebuildRooms`；`model_mutation::Kind` 加 `RoomRebuild`（标题「重新生成房间」、confirm 文案写清三句：删全库房间边、重建参照模型、期间生成请求会被拒）。
- `plant-ui-app/src/model_update_api.rs`：`rebuild_rooms(base)` → `POST /api/v1/rooms/rebuild`。
- `main.rs`：确认后 POST，得 202 进 `Vm::WaitingResident`（复用现有渲染：阶段人话 + 文件/网格两条进度条）；数据侧起一条轮询 `Evt::ResidentWait` 的轻任务（`resident_status` 每 ~2s），`Ready` → 关窗 + 日志「参照模型重建完成，已排 N 库整库房间重算」；`Blocked` → `Vm::Failed` 带 `problems` 首条。
- 等待档的「取消等待」换成「后台继续」：重建一旦发出服务端收不回，按钮只摘对话、不影响服务端；轮询随关窗停止。

### 验收

- 点击 → 确认框 → 202 → 进度屏（`source-snapshot` X/Y → `mesh-warm` X/Y）→ 就绪自动收尾。
- 老服务端无 `room_build` 格：按钮禁用（功能不可判就不放行），hover 写「服务端不支持」。
- 重建完成或失败都有日志一行 + 界面定格态。

## 阶段四：验证与交付

- `cargo check`（gen-model / rs-plant native）；`cargo test -p plant-ui`（含等待屏钉测试改名保留——语义换成「后台继续」）。
- 本机真服务端冒烟：启动后 `POST /api/v1/rooms/rebuild` → 轮询 `/health` 看两段进度推进 → 建完查边表行数 → `POST /model/generate` 验证重建中 409 / 完成后正常。
- 那台出过问题的机器上回归：服务刚启动即点「重新生成模型」→ 得可读的「房间参照模型初始化中」失败；点「重新生成房间」→ 进度屏走完后重发成功。

## 非目标

- 不做 resident 模型的磁盘持久化复用（每次启动仍全量重建，另议）。
- 不动增量换代链路（`ensure_current` / swap 账本 / 停放链语义照旧）。
- 不给重建加服务端取消——一旦发出跑到底，界面只摘对话。
