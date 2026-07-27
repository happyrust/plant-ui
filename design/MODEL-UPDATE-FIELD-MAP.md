# 模型更新八张画板 · 元素与契约字段对照

这张表是**实现的验收基准**：照画板做的时候，每一个数字、每一条状态文案，都必须在这里找到出处。
找不到出处的东西不许画上去，也不许实现出来（见 `DESIGN-SYSTEM.md` 校验清单）。

来源只有三类：

| 记号 | 含义 |
|---|---|
| **契约** | gen-model 现有接口已经给得出来的字段 |
| **本地** | plant-ui 自己能算的量（计时、计数差、缓存的上一份数据） |
| **待新增** | 要 gen-model 补事件或补字段才有，必须注明是哪条 ADR |

契约字段全部来自 `old/gen-model/src/data_interface/manual_update.rs`（预览与结果）、
`dbnum_state.rs`（`FileAnomaly`）与 `web_service/mod.rs`（`ApiError`）。

**这份文档存在的意义，就是让真实界面不必自己扛这些字段名。** 用户界面只放人要做决定时用得上的
信息；字段出处查这里，形态表 S2-D / S2-E 里可以标出来给实现者对照。

---

## 0. 八张画板共用

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 项目名 | `AMS-8009` | 契约 | `ManualUpdatePreview.project` / `ManualUpdateResult.project` |
| 执行任务号 | `mu-3c81` | 契约 | execute 返回体的 `task_id` |
| 预览任务号 | `pv-7f31` | **待新增** | ADR-0006：预览改 202 + `task_id` |
| 步骤条三步 | 更新预览 / 确认执行 / 执行进度 | 界面自有 | 无契约来源，是本仓库定义的流程 |

---

## 1. S2-A 正在预览

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 已用时 | `已用 01:12` | 本地 | 任务提交时刻起算 |
| 库计数与进度条 | `1 / 2 个 DESI 库` | **待新增** | ADR-0006 + ADR-0007：分子分母都取预览任务详情里的计数。**分母不取「已登记 DESI dbnum 总数」**——已登记里含文件缺失的库，它们不进扫描循环 |
| 行 · 已扫描 | `db8000 · DESI` | **待新增** | `PreviewDbFinished { dbnum, db_type }` |
| 行 · 扫描结果 | `8 个待应用会话 · 487 项变化` | **待新增** | `PreviewDbFinished { pending_sessions, changed_elements }`；终态可用 `DbnumPreview.sessions.len()` 与 `net_*` 之和复核 |
| 行 · 扫描中 | `db8003 · DESI` + 文件路径 | **待新增** | `PreviewDbStarted { dbnum, db_type, file_path }` |
| 行 · 单库已用时 | `扫描中 · 已用 22s` | 本地 | 从该库的 `PreviewDbStarted` 起算 |
| 行 · 非 DESI | `不需扫描` / `不在本期范围` | 契约 | 非 DESI 压根不进扫描循环，**不占分母** |
| 行序 | 按 dbnum 升序 | **待新增** | ADR-0007：现在扫描循环走 `IndexMap` 的 WalkDir 序、最终结果却按 dbnum 排过序，照事件顺序堆出来的列表会在预览完成那一瞬重排。后端在循环前排好，前端照事件 append 即可 |
| 上次预览摘要 | `1 个数据批次 · 23 个交付单元 · 1 个待重试单元` | 本地 | 缓存的上一份 `ManualUpdatePreview` |
| ~~600 秒说明~~ | | | ADR-0006 之后预览是 202 + `task_id`，没有一条挂十分钟的 HTTP 请求，也就没有这个上限要交代 |

> 「取消预览」只放弃客户端等待。服务端没有 cancel 接口，**不许**在文案上暗示它会停。

---

## 2. S2 预览结果

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 三个大数 | `128 / 342 / 17` | 契约 | `Σ DbnumPreview.net_added / net_modified / net_deleted` |
| 分布条三段 | 85 / 228 / 11 px | 契约 | 上面三数的比例 |
| 受影响交付单元 | `23` | 契约 | `Σ DbnumPreview.units.len()`，`DeliveryUnitSummary` 已按 refno 去重 |
| 其中影响模型的变化 | `361` | 契约 | `Σ DbnumPreview.model_affecting` |
| 树行库标识 | `db8000 · DESI` | 契约 | `dbnum` + `db_type` |
| 树行会话区间 | `sesno 1 024 → 1 031` | 契约 | `applied_sesno + 1` → `file_latest_sesno` |
| ZONE 分组行 | `ZONE /RXC0-Z1 · N 个生成单元` | 契约 | `DbnumPreview.zones[]`（`ZoneSummary`）；`zone_refno` 为空的那桶就是「归属未知」 |
| 阻断卡 | `db8003 文件回退 812 < 已应用 1 005` | 契约 | `anomaly = Rollback { file_latest_sesno, applied_sesno }` 且 `blocked = true` |
| no_generation 卡 | `14 项变化找不到可生成的交付单元` | 契约 | `Σ DbnumPreview.no_generation` |
| 待重试卡 | `1 个待重试单元将合并` | 契约 | `pending_model_retries[]` |
| 待重试单元标识 | `EQUI /P-1201B` | 契约 | `PendingModelUnit.noun` + `root_refno` |
| 已尝试次数 | `已尝试 2 次` | 契约 | `PendingModelUnit.attempts` |
| 主按钮批次数 | `开始更新 · 1 个批次` | 契约 | 可执行 dbnum 计数：非 `blocked`、非排除 |

---

## 3. S2-B 确认执行

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 会执行 · 汇总 | `1 个数据批次 · 23 个交付单元` | 契约 | 同上批次计数 + `Σ units.len()` |
| 会执行 · 库行 | `11 个会话 · 487 项变化` | 契约 | `sessions.len()` + `net_*` 之和 |
| 不会执行 · 阻断 | `已阻断 · 水位不变` | 契约 | `blocked = true` + `anomaly` |
| 不会执行 · 排除 | `非 DESI，本期不处理` | 契约 | 排除发生在扫描之前，**与阻断不是一回事，界面上不许合成一行** |
| 不会执行 · no_generation | `计入 no_generation 并告警` | 契约 | `Σ no_generation` |
| 会一并处理 | `1 个待重试单元` | 契约 | `pending_model_retries[]` |
| 重扫提示 | 开始时重新扫描，新会话自动并入 | 契约 | 结果里的 `merged_sesnos` 就是这句话的兑现 |

---

## 4. S4 执行进度 / S4-B 断线降级

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 已用时 | `已用 02:14` | 本地 | 执行提交时刻起算 |
| 阶段一逐行 | `db8000 · DESI` `sesno 1 024 → 1 034` | 契约 | `DataBatchStarted / Finished` 事件（WS，ADR-0005）。**现在有四条早退路径压根不发这两条事件**（同 dbnum 多文件、文件回退、首次按需初始化、无事可做），ADR-0007 要求把发事件包到 `execute_one_dbnum` 的函数边界上，否则阶段一的条永远走不满 |
| 阶段一计数 | `2 / 3 个数据批次` | **待新增** | ADR-0007：任务详情的 `batches_done` / `total_batches` |
| 阶段一用时 | `已完成 · 用时 48 秒` | 本地 | 首尾两条阶段一事件的时间差 |
| 并入提示 | `预览后新增的会话 1 032 – 1 034 已并入` | 契约 | `DataBatchResult.merged_sesnos` |
| 阶段二计数 | `14 / 24 个交付单元` | **待新增** | ADR-0007：分子分母都取任务详情里的 `units_done` / `total_units`。**不许用预览值相加**——见下方「分母为什么不能自己算」 |
| 单元行标识 | `EQUI /P-1201A` | 契约 | `ModelUnitStarted { noun, root_refno }` |
| 单元行「生成中 · 已用 14s」 | | 本地 | 从该单元的 `ModelUnitStarted` 起算，跑满 10 秒才显示 |
| no_generation 提示 | `预览时另有 14 处…` | 契约 | 预览的 `Σ no_generation`（执行期不发这类事件） |
| **S4-B** 落后条数 | `服务端已发 31 条，本端落后 17 条` | 契约 + 本地 | `GET /tasks/{id}` 的 `events_seen` 减本端已收条数。**只用来解释单元行明细缺了多少，不许拿它反推进度** |
| **S4-B** 进度条 | 照常准确推进 | **待新增** | ADR-0007：计数走轮询通道，断线不影响。断线要降级的是**列表区**，不是那条条 |
| **S4-B** 「状态未知」 | | 本地 | 没收到事件就不知道有没有开始，不许写「排队中」 |

> 有百分比，没有 ETA。百分比的分子分母都来自 ADR-0007 那四个计数，是权威值；
> ETA 要靠已完成单元的平均耗时外推，而一个 BRAN 和一个 EQUI 差一个数量级，算出来是猜。
> 界面上要说清这是**单元数**进度而非时间进度——条走到一半不代表时间过半。

### 分母为什么不能自己算

本文件早先写的是「分母 24 = 预览的 23 个新单元 + 1 个待重试单元」。这个加法两个方向都会偏：

- **偏小**：执行开始时会重新扫描，预览之后新产生的会话自动并入同一个数据批次（就是
  `merged_sesnos`），并入的会话带来新单元。
- **偏大**：`merge_unit_worklist` 按 `(dbnum, root_refno)` 去重。待重试的那个单元若本窗口
  又被改到，它和新单元会塌成一条。

真实工作量 `worklist` 在生成循环开始前就已经算好了，权威值在后端手里，取它即可。

---

## 5. S4-C 部分完成（`partial` 终态）

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 状态标 | `status = partial` | 契约 | `ManualUpdateResult.status`；`aggregate_manual_status` 按「有成功也有失败」判定 |
| 耗时 | `耗时 04:38` | 本地 | 提交到终态的时间差 |
| 阶段一计数 | `1 已应用 · 1 阻断 · 0 失败` | 契约 | `batches[].status`（`Applied` / `Failed` / `Skipped`） |
| 批次会话窗 | `sesno 1 024 → 1 034` | 契约 | `DataBatchResult.start_sesno` / `end_sesno`（跳过的批次两者都是 0） |
| 并入会话 | `1 032 / 1 033 / 1 034` | 契约 | `DataBatchResult.merged_sesnos` —— **契约明写结果摘要必须逐条列出** |
| 变化项数 | `512 项（预览时 487）` | 契约 + 本地 | `DataBatchResult.changed_elements`，与缓存的预览值相减得到差额 |
| 阶段二计数 | `22 成功 · 2 失败 · 共 24` | 契约 | `units[].status`（`Generated` / `Failed`） |
| 失败单元标识 | `EQUI /P-1201B` | 契约 | `ModelUnitResult.noun` + `root_refno` |
| 尝试次数 | `第 3 次尝试` | 契约 | `ModelUnitResult.attempts`（跨重试累计） |
| 失败原因 | 一句话 | 契约 | `ModelUnitResult.message` |
| 「重试」按钮 | | 契约 | `POST /api/v1/model/ensure { refno }`，幂等，**无需后端改动** |
| 「模型树就地刷新」 | | 契约 | `ModelUnitResult.old_owner` / `new_owner`（现在的实现完全忽略了这两个字段） |
| no_generation 行 | `14 处` | 契约 | `ManualUpdateResult.warnings[]`，原文是「N 个变更无法解析合法生成根，跳过模型生成（样例: …）」 |

---

## 6. S2-D 不可用与失败态

服务端错误包封是 `{ code, message, detail }`（`web_service/mod.rs` 的 `ApiError`）。
**按 `code` 分型，不要解析 message 字符串，更不要把 anyhow 错误链摊给人看。**

| 形态 | 状态码 · code | 来源 | 出处 |
|---|---|---|---|
| 数据源未就绪 | 不发请求 | 本地 | `vm.data_source_ok = false`；`workbench/chrome.rs:106` 据此禁用入口按钮 |
| 自动同步接管 | 422 · `precondition` | 契约 | `ApiError::from_domain` 里 message 含 `sync_live` 的分支 |
| 已有任务在跑 | 409 · `conflict` | 契约 | `tasks.running_for_project` 命中；message 带着那个 `task_id` |
| 已是最新 | 200 | 契约 | `ManualUpdatePreview.up_to_date = true` |
| 连不上 / 超时 | 504 · `timeout` | 契约 | `ApiError::timeout`，或客户端 600 秒上限到点 |
| 其余 | 500 · `internal` | 契约 | **只有这一类**才需要把原始 message 摊出来，且收进「详情」默认折起 |

---

## 7. S2-E 设计库行形态表

`blocked` 不是随 `anomaly` 一起来的，是**算出来的**：`preview_dbnum` 里
`matches!(anomaly, Rollback | TypeChanged)`，项目扫描器再把 `Duplicate` / `Missing` 直接置
`blocked = true`。所以五种异常里 `PathMigrated` 独自落在「会执行」那一组。

| 行形态 | 关键字段 | 会否执行 | 界面必须说清的事 |
|---|---|---|---|
| 正常待更新 | `anomaly = None`、`applied_sesno < file_latest_sesno` | 会 | 会话区间与成功后的水位落点 |
| 需初始化 | `initialization_required = true` | **会** | 契约不为它解会话区间，`net_*` 全是 0；**绝不能显示成「无变化」** |
| 路径迁移 | `anomaly.kind = path_migrated`、`blocked = false` | 会 | 五种异常里唯一不阻断的；登记路径可自动跟新 |
| 文件回退 | `anomaly.kind = rollback` | 阻断 | `file_latest_sesno < applied_sesno`；水位只前进不后退 |
| 类型变化 | `anomaly.kind = type_changed` | 阻断 | 身份永不自动覆盖，必须人工确认 |
| 同号重复 | `anomaly.kind = duplicate` | 阻断 | 契约明写 block, do not pick —— 要把 `paths[]` 都列出来交给人挑 |
| 文件缺失 | `anomaly.kind = missing` | 阻断 | 出路二选一：补回文件，或注销这条登记 |

> 「排除」（非 DESI）不在这张表里 —— 它压根不进扫描，与「阻断」来自契约的不同位置。

---

## 8. 契约里有、界面还没用的字段

反过来查的一遍。这些不是缺陷清单，是**下一轮画板的候选**，也是评审时该问的问题。

| 字段 | 情况 |
|---|---|
| ~~`DbnumPreview.initialization_required`~~ | **已在 S2-E 给出形态**；还欠的是把它接进 S2 的树行与 S2-B 的「本次会执行」卡 |
| `DbnumPreview.sessions[]`（`SessionPreview` 逐会话 added/modified/deleted） | 完全没用。想做「按会话展开」得从这里取 |
| `DbnumPreview.file_name` / `file_path` | 只在 S2-A 的扫描行露过脸，预览结果页与终态页都没有 |
| `DeliveryUnitSummary.moved_in` / `moved_out` | 元素跨交付单元迁移的计数，没画。这恰恰是最容易让人困惑的一类变化 |
| `DeliveryUnitSummary.name` | 只用了 `noun + root_refno`，人可读的名字没显示 |
| `ZoneSummary.model_affecting` | ZONE 分桶里影响模型的计数，没画 |
| ~~`FileAnomaly` 五种只画了 `Rollback`~~ | **五种已在 S2-E 画齐**；还欠的是把它们接进 S2 的树行与右栏阻断卡（现在只有 `Rollback` 一种有卡片形态） |
| `PendingModelUnit.last_error` / `source_end_sesno` | 待重试单元的上次错误与来源会话号，没画 |
| `ManualUpdatePreview.warnings[]` | 预览期的非致命告警（读不了库头、遍历目录失败等）在 S2 上没有落脚点 |
| `ManualUpdateStatus::Success` / `Failed` | 三种终态只画了 `partial`，另外两种没有画板 |
