# 手动更新 = 把这个库的 Task 提前排上 · plant-ui 侧适配计划

- 日期：2026-09-08
- 提出：用户「先分析当前 plant-ui 的手动更新面板，按照我们现在的新方案
  `2026-09-08-external-increment-refactor-plan.md`，应该如何显示和交互；比如我想手动增量更新某个 dbnum，
  那就是提前执行」
- 范围：`plant-ui`（`model_update.rs` / `task_queue.rs` / `vm.rs`）、`plant-ui-app`（`model_update_api.rs` /
  `main.rs` / `sim.rs`）、`CONTEXT.md` 三条词条。**gen-model 零改动**——本计划全部吃它已定的契约。
- 依据：gen-model `docs/plans/2026-09-08-external-increment-refactor-plan.md`（改判 U1–U5、不变量 N-A～N-E、
  裁决 Q30 / Q31 / Q32 / Q33、审核补入 G2 / G3 / G4）与 `docs/adr/ADR-065-*`；本仓 ADR-0005（进度只走 WS）、
  ADR-0011（执行进度在任务队列不在向导）、ADR-0019（按写入时刻说话）、ADR-020（SITE 分桶与勾选子集）、
  ADR-0025（一次装载一个源）；`CONTEXT.md`「水位 / 任务队列 / 待重试单元」。
- 状态：**D1–D8 全部已拍**（D1 2026-09-08；D2–D8 2026-09-09 用户照表拍板，共识 d-69——含一条判据勘误：
  U2 六态里「阻断」那档的 `anomaly` / `blocked` 字段 `/dbnums` **已经给了**，QUEUE-FIELD-MAP §3 的
  「待新增」是旧账）。**U2 已落地**（2026-09-09，见 §六），**当日 sim 实机过目**（§七.1–.3 的
  sim 版，见 §七 注）；U3–U5 仍押 gen-model S6–S9。
- 界面稿：五张画在 pen.dev 画布 `pencil-new.pen` 上，导出 `docs/plans/assets/2026-09-08-manual-update-mockups.html`
  ——任务队列（§5.3 / §5.4 / §5.5 / §5.6）、提前执行悬停气泡（§5.1 A）、向导执行计划书（§5.2）、
  本期执行范围「立即执行」列（§5.1 A 的第二处落点 + 六种按钮态）、启动那一幕（§5.5 / §5.6 的 G4 两种意图）。
  稿子是按本文的口径画的；两边不一致时以本文为准，改稿。

---

## 一、一句话

用户那句「手动增量更新某个 dbnum = 提前执行」正是新方案的中心：一个 dbnum 就是一个 **Task**
（数据 → 模型 → 房间 → 凭证前移 → 推模型水位），手动不是第二条流水，只是**把本来要等 sweep / watcher
发现才排的那个 Task 现在排上**。今天的界面画的是另一个世界——三条平级流水（数据批次 / 模型 drain /
房间轮）加一张持久欠账表——新方案把这三样一起拆了，所以要改的不是几句文案，是界面背后的名词表。

---

## 二、现状勘探（2026-09-08 工作树实位，逐条核过）

### 2.1 两块面怎么分工

| 面 | 文件 | 管什么 |
|---|---|---|
| 「模型增量更新」向导窗 | `crates/plant-ui/src/model_update.rs`（203 KB） | 两步：预览（`POST /api/v1/update/preview`）→ 确认（`POST /api/v1/update/execute`）。按下确认就关窗、焦点跳队列（ADR-0011），向导不跟任何一次运行 |
| 「任务队列」面板 | `crates/plant-ui/src/task_queue.rs`（189 KB） | 排队 / 运行 / 终态行、两枚水位、库一致性判决、欠账与重试 |

向导的预览树是 **库(dbnum) → SITE → 交付单元** 三层（`DbPreview` `:137` / `SitePreview` `:227` /
`UnitPreview` `:264`），勾选按库记、同库联动（`State::unchecked` `:1397`），执行请求带勾选子集
（`selected_dbnums` `:1452`，ADR-020）。七种设计库行状态（阻断 / 需初始化 / 够不着 / 五种文件异常…）在 S2-E 收拢。

### 2.2 界面背后的名词表 = 三条流水 + 一张欠账表

- **三个 task kind**（`task_queue.rs:36-40`）：`data_batch`、`model_drain`（注释原话「数据阶段之后的持久模型工作
  消费者。它会因新数据让位，`yielded` 是终态而不是失败」）、`room_recalc`（「与 dbnum 列表平级，不挂在任何
  dbnum 行下」）。
- **一张持久欠账表**：`GET /api/v1/update/pending-units`（`model_update_api.rs:393`）→ `PendingUnits` `:304`
  → `Vm.pending` / `pending_known` `:566-568` → 行内欠账行 `pending_line` `:3067`，带 `attempts` / `dead` /
  `last_error`，以及重试入口 `POST /api/v1/update/pending-units/retry`（`model_update_api.rs:455`）。
  同一份 `PendingModelUnit` 也进向导：`Preview.pending_model_retries` `:46`。
- **手动执行 = 扫描 + 入队数据批次**：回执 `Enqueued` `:756` 的五桶（`enqueued` / `merged` /
  `already_covered` / `blocked` / `up_to_date`，外加勾选子集的 `unselected`）。

水位那一半已经是新口径：`DbnumStatus` `:315` 吃 `/api/v1/dbnums`，`verdict_label()` `:413` 按两枚水位说话，
`CONTEXT.md`「水位」词条 2026-09-08 已经写死「界面上一律以两枚水位为基准说话，不说待应用 / pending」。
**只有账的那一半是新的，活的那一半还是旧的。**

### 2.3 新方案拆掉了哪三样

`external-increment-refactor-plan` §3.0 U1–U5 与 §6：

1. **模型工作只在 dbnum Task 里算**（N-B）：`model_drain` 与 `idle_round` 模型段退役，`DataQueued` 让位退役。
2. **房间随 Task 收尾**（N-D / U5）：`room_recalc` 轮、`room_published_inst`、文件轮次屏障退役；
   且 G2 (i) 定了顺序 数据 → 模型 → **房间** → 凭证前移 → 推 `model_sesno`。
3. **没有第三份「欠什么」的表**（N-A / U2 / U4）：`model_update_pending` 整体退役；失败根 = `gen_root` 非安定
   + 失败记录 + Task 回执 + 日志，**不自动重试**。

另外两条会直接长进界面：**`ModelOnly` Task**（Q31：没有数据窗口也要算模型的三条路）与
**初始化 FIFO 排他串行、稳态跨库并行同库串行**（Q30）。

### 2.4 会变成死码或说谎的地方（S9 一动就同时发生）

| 今天的东西 | 出处 | 新方案下 |
|---|---|---|
| 向导「待重试」段 | `Preview.pending_model_retries` `model_update.rs:46` / `PendingModelUnit:308` | 无源 |
| 欠账行与重试按钮 | `PendingUnits` / `Vm.pending` / `pending_line` / `pending-units/retry` | 无源 |
| `rooms_pending()` | `task_queue.rs:963` | 无源（房间不再是独立轮次） |
| `KIND_MODEL_DRAIN` / `KIND_ROOM_RECALC` 行形态 | `task_queue.rs:38-40` | 不再产生新行 |
| 「已放弃：不再自动重试，也不并入手动更新；可在任务队列逐个重试」 | `model_update.rs:3117`、`task_queue.rs:3140` | 后半句变假 |
| `verdict_note()` 里「N 根已放弃（死信，队列面板可立刻重试）」 | `task_queue.rs:442` | 同上 |
| `Enqueued` 五桶的数据批次口径 | `model_update.rs:756` | 换成 Task 口径 |

**今天还缺一格**：房间。U5 + G2 (i) 之后「房间重算失败 → 那几根凭证不前移 → `model_sesno` 卡住 →
库不 `in_sync`」是一条真实因果链，而预览里没有房间这一段，`verdict_label()` 也说不出「卡在房间上」。

---

## 三、术语（随本计划进 `CONTEXT.md`）

**dbnum 任务 (Dbnum Task)**：一个设计库的一次完整推进——数据窗口、模型重算、房间重算、凭证前移、
推模型水位，五件事一个事务性顺序、一行进度。手动与自动同一种任务，差别只在谁触发。
_Avoid_：数据批次、批、作业、更新轮

**提前执行**：人对某个 dbnum 主动排一次它的 Task，不等 sweep 周期或 watcher 事件。它不插队、不改范围、
不是另一条流水。
_Avoid_：手动更新、强制更新、立即同步

**仅模型任务 (ModelOnly)**：没有数据窗口、只算模型的 Task（启动退化窗口 / `model/rebuild` / 读透文件事件）。
同一队列、同一行形态，数据那一段画成「无数据窗口」。
_Avoid_：模型任务、补算、rebuild

**退役词条**：「待重试单元」（`CONTEXT.md:123`）随 pending 表一起退役；「任务队列」词条要从
「所有等着被应用的**数据批次**排成的一条线」改成「所有等着跑的 **dbnum 任务**」。

---

## 四、决定

| # | 题 | 结论 |
|---|---|---|
| **D1** | 提前执行的入口放哪 | **已拍（2026-09-08 用户）：两处都要。** 库行上「立即执行」+ 悬停气泡管**单库提前执行**；向导继续管**全范围扫一遍**。理由：用户要的那件事（对一个库提前执行）今天要开向导 → 全库重查询 → 取消其余 19 个库的勾 → 确认，四步换一件小事；而全范围扫一遍确实需要向导那棵树 |
| D2 | 预览是几段 | **三段**（数据 / 模型 / 房间），一份「执行计划书」。今天只有数据一段 |
| D3 | 进度怎么画 | **一个 Task 一行四段**（数据 ─ 模型 ─ 房间 ─ 水位），走到哪亮到哪；末段亮 = 这一库追平。三种 kind 三行退役 |
| D4 | 失败给不给「重试」 | **不给队列重试**（N-C）。给两个真实入口：「重算这几根」走 `POST /model/ensure`（本端已有这条路，眼睛那条）、「再执行一次这个库」= 再排一次 Task。死信那一格改成陈述句 |
| D5 | `ModelOnly` 怎么显示 | 同队列同行形态 + 「仅模型」徽标，数据段画「无数据窗口（凭证 < 文件最新）」。按 G4 分两种意图：启动退化窗口那种计入「初始化中」并阻 ready，`model/rebuild` 与读透文件事件那种不计不阻 |
| D6 | 两种排队语义 | 初始化 = FIFO 排他串行，说「初始化：第 3/12 库，其余 9 个等这一个跑完」；稳态 = 跨库并行同库串行，说「排队第 2 位」。**两句话不许合并** |
| D7 | 两代服务端并存 | **必须**：exe 与 gen-model 各自发布，S9 前后都要能连。沿用本仓一贯纪律——**服务端不给这一格就整格不画**，不猜、不硬切；`pending-units` 404 / 缺字段一律降级成「这一档服务端没有」，不得表现为「欠账清零」 |
| D8 | 时机 | 本计划挂在 gen-model **S9（G3 对外字段兼容表）之前**当消费者侧清单：那一步一动，plant-ui 同时缺字段与说谎。S9 之前只做 D7 的降级与不说谎那几格，D1–D6 等 S9 的字段定形 |

**范围提醒**：gen-model 那份计划 Q20 把 plant-ui **明文排除在外**，所以上面这些今天没有主人。本计划就是补
那个主人，不反过来要求 gen-model 加任何东西。

---

## 五、设计

### 5.1 入口（D1）

**A. 库行上的「立即执行」**（新，主路）。落在队列面板的库行与 `/dbnums` 那张表的行尾。判据与文案在**按下之前**
就说完——今天这三句都要等回执才知道：

| 库此刻的样子 | 按钮 | 旁边那句 |
|---|---|---|
| `applied_sesno == file_latest_sesno` | 灰 | 「文件没有新保存，无可执行」 |
| 该库已有 Task `queued` | 灰 | 「已在队列第 N 位，提前执行不会插队」 |
| 该库已有 Task `running` | 灰 | 「正在执行中」 |
| `blocked` | 灰 | 阻断原因原文（五种文件异常之一） |
| `not_in_project` / `excluded` | 不画这一格 | 它本来就不在本期范围 |
| 其余 | 可点 | 「提前执行：数据 N 次保存 · 模型 M 根 · 房间 J 间」（悬停气泡展开，见 5.2） |

悬停气泡 = 一次**轻量**预览。它不能走今天那条全范围 `POST /update/preview`（重查询，跨全部库）——
要么 gen-model 给单库预览、要么本端只用 `/dbnums` 已有的两枚水位与 `model_chasing_roots` 画一个粗版气泡
（数据段准、模型段写「约 M 根」、房间段不画）。**先用粗版**，等 S9 定字段再谈精确版：粗版说得出的每一格
都有出处，说不出的整格不画，符合本仓纪律。

**B. 向导继续管全范围**（保留）。两步不变，预览树不变，勾选子集不变；只把回执口径从「数据批次五桶」换成
Task 五桶，并在标题下加一句「共 N 个库的任务将被提前排上」。

### 5.2 预览三段（D2）

一份执行计划书，三段各自可空、空了要说为什么空：

```
数据   7997  applied 412 → 文件最新 415（3 次保存，最早一条 09-08 14:02）
       净变化 +18 ~64 −3
模型   重算 12 根 · 派生容器 2 个 · 元数据门省下 4 根（只改了 NAME / DESC）
       CACHID 那类字典外属性照旧整根重算
房间   受影响 3 间（PANE /R-101 /R-102 /R-205）
       房间重算失败会让这几根的凭证不前移、模型水位卡住
```

第三段是新的，也是 G2 (i) 唯一能在界面上兑现的地方。第二段里「元数据门省下几根」正是 2026-09-08 落地的
`metadata_only` 计数（gen-model `IncrementReport.metadata_only`，回执 `warnings` 已经带一行）——它是这次
重构里唯一**减少**工作量的量，值得单独显示一格。

### 5.3 进度一行四段（D3）

```
7997  ●━━━━━━━●━━━━━━━○────────○     数据 ✓ 3 次保存 · 模型 8/12 根 · 房间 — · 水位 —
      数据     模型     房间     水位
```

- 事件源不变：WS `tasks` 主题（ADR-0005）。G3 已定 `task_started / task_finished` 改由 dbnum Task 发，
  所以本端的 `ProgressEvent` / `RowState` 形状能原样吃。
- 末段（水位）亮起 = `advance_model_watermark` 成功 = 这一库这一轮追平。**只有它亮才算完**，
  中间三段亮完不算——今天「数据批次 completed」就当完事，那是旧口径。
- 「模型让位了」这类话从界面上整体消失。

### 5.4 失败（D4）

行内留痕，不给队列重试：

```
7997  模型 3 根未算成：/BRAN-24384-26338 …（展开看错误）
      模型水位卡在 412，不会越过这几根 · 不会自动重试
      [重算这几根]（POST /model/ensure）   [再执行一次这个库]
```

两个按钮都是**命令面**（ADR-0026：命令面永远走模型服务），与供数模式无关。死信那一格从
「N 根已放弃（死信，队列面板可立刻重试）」改成「N 根上次没算成，模型水位卡在它们前面」。

### 5.5 ModelOnly（D5）与 5.6 初始化串行（D6）

同一队列里三种行的措辞必须互相认得出：

| 行 | 数据段 | 排队语义那句 |
|---|---|---|
| 稳态 Task | 窗口 S→T | 「排队第 2 位」 |
| `ModelOnly`（启动退化窗口） | 「无数据窗口（凭证 < 文件最新）」+「仅模型」徽标 | 「初始化：第 3/12 库」，计入初始化进度 |
| `ModelOnly`（rebuild / 读透文件事件） | 同上 | 「排队第 5 位」，不计入初始化 |

### 5.7 两代服务端并存（D7）

每一格都问「服务端给了吗」，不问「服务端是哪一代」：

- `GET /update/pending-units` 404 或缺失 → `pending_known = false` 那条路已经有了（`task_queue.rs:568`
  注释原话「失败不能冒充欠账清零」），把它从「暂时取不到」扩成「这一档服务端没有」，欠账段整段不画；
- `task.kind` 认不出的字面 → 按普通 Task 行画，不猜；
- 四段进度里服务端没发过事件的段 → 画 `Unknown` 那一档（ADR-0005 已有：「没收到事件就不知道它开没开始，
  不许说成排队中」）。

### 5.8 消费者侧字段表（给 gen-model S9 / G3 用）

| 对外字段 | 本端消费点 | S9 后 |
|---|---|---|
| `/dbnums.model_verdict` / `model_chasing_roots` / `model_dead_roots` / `model_sesno` | `DbnumStatus:366-380`、`verdict_label` `:413`、`verdict_note` `:437` | 换源不换名，本端不改 |
| `/update/pending-units` | `model_update_api.rs:393`、`Vm.pending` | 退役 → 本端整段降级 |
| `/update/pending-units/retry` | `model_update_api.rs:455` | 退役 → 换 `/model/ensure` |
| `/health` 的 pending 计数格 | 队列面板摘要 | 退役 → 改说在飞 Task 数 |
| WS `tasks`：`task_started` / `task_finished` | `ProgressEvent:1166` | 改由 dbnum Task 发，形状不变 |
| `task.kind` = `data_batch` / `model_drain` / `room_recalc` | `task_queue.rs:36-40` | 后两个不再产生；建议新增 `dbnum_task` / `model_only` 两个字面 |

---

## 六、里程碑

| 步 | 内容 | 依赖 | 验收 |
|---|---|---|---|
| **U0** | 本文；`CONTEXT.md` 三条词条（新增 dbnum 任务 / 提前执行 / 仅模型任务，改「任务队列」，标「待重试单元」待退役） | — | 文档互引：本文 ↔ gen-model 那份 ↔ `CONTEXT.md` |
| ✅ **U1** | **不说谎先行**（S9 之前就能做）：`pending-units` 缺席时整段降级；死信那两句不再指路到队列重试 | U0 | **已过**（2026-09-08）。落地形状：欠账那一格改成**三态**——取到了 / 端点不在（404 / 405 / `not_found`，纯函数 `pending_endpoint_retired` 判，答案是**可信的空**：`Poll.pending_unsupported = true`、`Vm.pending` 清空、派生计数跟着归零）/ 真取不到（超时 / 500，沿用上一份，不冒充「欠账清零」）。两句改口都只删了「指路」那半句、留下事实：`verdict_note` 的「N 根已放弃（死信，队列面板可立刻重试）」→「N 根已放弃，模型水位不会越过它们」，向导里「…；可在任务队列逐个重试。」→ 句号收尾。测试 `only_a_missing_endpoint_counts_as_the_owed_table_being_gone`（三态六例）、`a_retired_pending_endpoint_clears_the_owed_section_instead_of_freezing_it`（adopt 三拍）；`cargo test -p plant-ui -p plant-ui-app -p plant-ui-data` = **144+2 / 136 / 19** 全绿，wasm check 过，四份改动文件 `rustfmt --check` 干净 |
| ✅ **U2** | 库行「立即执行」+ 粗版气泡（D1 A、5.1） | U1 | **已过**（2026-09-09）。落地形状：判据纯函数 `task_queue::early_run`（六档——排除 / 够不着不画这一格、阻断灰 + 原因原文、已排队灰 + 第 N 位、运行中灰、文件没有新保存灰、其余可点），材料只吃 `/dbnums` 现有字段；队列行尾新增「操作」列（`/dbnums` 空表**整列不画**，D7），「本期不执行」的阻断行同格永灰；粗版气泡 = 数据段（`已应用 412 → 文件最新 415（3 次保存）`）+ 模型段（`落后约 M 根（服务端判）`，`model_chasing_roots` 没给整行不画）+ 「不插队、不改本期执行范围」那句，房间段整格不画；需初始化的库照样可点，气泡说「首次导入，整库建立基线」、不说「落后 N 次保存」（CONTEXT.md「需初始化」）；断线的 `Unknown` 行按「占着队列」判，不放行重复入队。按下推 `Cmd::RunDbnumNow` → 既有 execute + 单库 `dbnums[]`（`main.rs` 与「立刻扫一遍」同一守卫同一接口）。测试七例（六态判据 + 首次导入措辞）；`cargo test -p plant-ui -p plant-ui-app` = 157 + 138 全绿，三份改动文件 rustfmt 干净。**已 sim 实机**（2026-09-09，`PLANT_UI_SIM=1` + `EGUI_INSPECTION` 探针，截图 `.sim-verify/`）：六态、粗版气泡两种形状（db7001 数据 + 模型两行、db7002 只数据行——`model_chasing_roots` 缺席整行不画）、点击 → 入队 → 行出现 → 跑完追平回灰的整条链、灰按钮（排队中 / 已追平）点了不入队不重复排队，全部过目；为演 Ready 态给剧本加了一幕（`sim.rs` `late_saves`：第一轮房间收敛后 db7001 / db7002 又各存一次盘，db7001 且判 12 根模型落后，任务跑完即清）。**观察一则**：禁用态按钮的点击会穿到行身、把明细翻开（egui 禁用控件不吃输入）——不触发执行、不违反 §七.2「按不下去」，暂记为可接受；首次导入那档的气泡措辞 sim 里够不着（Ready 只长在有行的库上，需初始化库跑完即追平），仍靠单测背书。真服务 live（§七与 gen-model live #2 同趟）仍欠 |
| **U3** | 一行四段进度 + `ModelOnly` 徽标 + 两种排队语义（D3 / D5 / D6） | gen-model S6 / S7 落地、`task.kind` 字面定形 | WS 事件驱动的行状态机测试；`Unknown` 档不许说成排队中 |
| **U4** | 预览三段（D2）与失败留痕（D4） | gen-model S5 / S8 落地 | 三段各自可空的渲染测试；两个命令面按钮的路由测试 |
| **U5** | 退役：欠账段、两个 kind、向导「待重试」段、`rooms_pending` | gen-model S9 | 死码清零；`cargo test -p plant-ui -p plant-ui-app` 不降 |

**顺序**：U0 → U1 立刻可做；U2 可与 gen-model 并行；U3–U5 跟着 S6 / S7 / S8 / S9 走。

---

## 七、验收（实机，与 gen-model live #2 同一趟）

> 2026-09-09：第 1–3 条已在 **sim** 上过目（U2 范围内的版本——四段进度是 U3 的，第 1 条只验到
> 「当场多一行、跑完判追平、按钮回灰」；剧本为此加了跑空后的晚到保存一幕，见 §六 U2 行）。
> 连真服务的那一趟仍欠，第 4–6 条不动。

1. 对一个 `applied < file_latest` 的库按「立即执行」→ 队列里**当场**多一行该库的 Task，四段依次亮，
   末段亮起后 `/dbnums` 该行判「模型水位已追平数据水位」。
   全程不开向导、不动其它库的水位。
2. 对一个已是最新的库：按钮是灰的，旁边写「文件没有新保存」——**按不下去**，而不是按下去回一句「已是最新」。
3. 对一个队列里已有行的库：按钮灰 + 「已在队列第 N 位，提前执行不会插队」，队列行数不变。
4. 造一个房间重算失败：那几根凭证不前移、库不 `in_sync`、行内说得出「卡在房间上」（G2 (i) 的界面兑现）。
5. 冷启动一次：初始化那几库按 FIFO 一库一库亮，界面说「第 k/N 库」，不是一堆库同时转圈。
6. 连一台 **S9 之前**的 gen-model：欠账段照旧画、四段进度退回今天的三行形态，界面不崩、不说反话（D7）。

---

## 八、风险与不做

- **单库预览没有契约**：5.1 的气泡想要精确就得有单库预览端点，本计划**不要求** gen-model 加。先用
  `/dbnums` 现有字段画粗版；粗版说不出的整格不画。
- **一行四段的事件粒度**：G3 只说 `task_started / task_finished` 改由 dbnum Task 发，中间三段的分段事件
  今天没有契约。U3 落地时若只有首尾两个事件，四段退化成「进行中」一段，**不许拿本端计时假装分段**。
- **两代服务端**：D7 是硬要求，不是尽力而为。plant-ui 的发布节奏与 gen-model 不同步，任何一格「新服务端
  才有」的字段都要能缺席。
- **不做**：房间的独立面板（房间随 Task 收尾之后它不再是一条流水）；把 `model/ensure` 做成批量重试队列
  （那是 N-C 明文禁止的形状，换个位置排队还是排队）；供数模式相关的任何改动（与本计划正交，ADR-0026）；
  gen-model 侧任何新端点。
