# 任务队列 · 施工顺序

- 对象：`D:\work\plant-code\old\gen-model`（分支 `codex/manual-incremental-verify-checkpoint`）与
  `D:\work\plant-code\old\plant-ui`（`master`）
- 日期：2026-07-27
- 依据：`gen-model/docs/adr/ADR-011-one-data-batch-queue-for-manual-and-auto.md`（12 条服务端决策）、
  `docs/adr/0011-execution-progress-moves-to-the-task-queue.md`（客户端划界）、
  `design/QUEUE-FIELD-MAP.md`（验收基准）、`CONTEXT.md`（术语）
- 性质：设计已定稿，本文件只排施工顺序。已落地的改动见第六节；服务端只新增了一个
  **不依赖 `web_service`** 的纯逻辑模块，接口层一行未改。

**一句话现状：两个卡口都已解（2026-07-27 晚）——gen-model 最小基线
`4184a5b1` 已入库；S12 画板确认存活于 19:52 的旧名文件并已 rename 回权威名
`plant-ui.pen`。第五节第 2 步（服务端 1–4 项 + 心脏）与第 4 步（服务端 5–9 项：
暂停端点+持久化、房间轮 detail 分项计数、/dbnums anomaly/blocked/excluded、
/health started_at+gen_spatial_tree+queue_paused、新增 GET /queue 快照）均已落地
（分别见六之三、六之四与 gen-model ADR-011 状态段；238 单测全绿，实机 curl 验收因 8021
旧服务在跑欠一次）。**客户端第 2 项（dock 队列视图 + 状态栏计数）也已落地**，见六之五
——对着替身服务逐行核过 S12 / S12-B，真服务验收同样欠一次。下一步：客户端第 3 项
（砍向导第三步，那段代码现在已经跑不到了）与第 4 项（房间泳道、「本期不执行」）。
三轮拷问定案见第八、九、十节。**

**第十节补的一句（深夜）：** 队列层第三轮审核查出三条高危——task_id 熵不够导致
启动重扫约一半概率静默覆盖任务行、worker panic 后永不重启且 `/health` 照报 ok、
锁中毒把单点 panic 放大成整个增量子系统连坐。连同四条中等已一并修复入树，
`cargo test --lib` 238 → **242**，**未提交**。`/health` 多了 `worker_alive` 与
`worker_idle_secs` 两个字段（客户端 `Health` 全字段 `#[serde(default)]`、
无 `deny_unknown_fields`，加字段向后兼容）。那笔实机 curl 验收仍然欠着，
而且第十节论证了：按当前 `manual_db_nums = [7997]` + `sync_live = false` 去跑，
六个队列形态里有三个压根碰不到。

---

## 一、两个卡口

| 卡口 | 事实 | 为什么必须先解 |
|---|---|---|
| gen-model 未提基线 | 215 个脏文件，且 `src/web_service/` 整个目录是 `??`（未跟踪） | 第二节九项里的**前八项全落在那个目录里**。未入库意味着改坏了没有 `git diff` 可看、没有 `git checkout` 可回 |
| 画板存盘待验证 | `plant-ui.pen` 仍是 17:24 那份；但旧名 `rs-plant3d-ui.pen` 在 19:52 被写过（727KB > 654KB），`S12` / `S12-B` / `C/QueueRow` **可能已存进旧名文件**——.pen 加密，只能在 Pencil 里打开验证 | 验证之前不知道画板是不是丢了。HTML 快照在 `output/queue-boards-backup.html`，但那是只读的，回写不了 .pen |

卡口一已解（第九节第 2 条，gen-model `4184a5b1`）：最小基线单独一个 commit——`src/web_service/`
（5 文件）+ `src/data_interface/batch_queue.rs` + `data_interface/mod.rs` 那两行挂载，
提交前跑 `cargo check`；其余 200+ 脏文件是别的工作流的中间态，不动。
`increment_manager.rs` 与 `lib.rs`（心脏手术对象）本来就是干净的，已有基线。

卡口二已解（第九节第 3 条，S12 确认存活、已 rename）：权威定 `plant-ui.pen`（rollout 与
`QUEUE-FIELD-MAP.md` 引用的都是它，文档不用改）。先在 Pencil 打开
`rs-plant3d-ui.pen` 验证 S12 画板在不在：在，就把那份存成 `plant-ui.pen`、
git 里 rename 掉旧名；两份都没有，按 `QUEUE-FIELD-MAP.md` 重画——那张表
本来就是为「画板可重建」准备的验收基准。

另有一条与卡口同源的观察：当时 Pencil 编辑器绑的是 `design/rs-plant3d-ui.pen`，
而那个路径一度不存在（文件被改名成 `plant-ui.pen`）——19:52 的那次写盘让旧路径
又有了实体，两个 .pen 从此并存，这正是权威归属必须定下来的原因。彼时同一会话里
新建的每个节点在画布上都被多加了 50px 竖向偏移、截图全白——用一个 20×20 的
探针方块插进**本来渲染正常**的旧框里复现过：旧邻居在 y=12–27，新方块落在 y=68。
HTML 导出没有这个偏移（172 个 flex 容器、2 处绝对定位），所以是画布这一层的事，
不是数据。重开编辑器绑定权威文件后要先复验这条还在不在。

---

## 二、服务端九项

全部落在 `gen-model`。

| # | 项 | 改哪里 | 依据 |
|---|---|---|---|
| 1 | `TaskState` 加 `queued` | `web_service/tasks.rs`，含 `as_str()` | ADR-011 §3 |
| 2 | `TaskEntry` 加 `dbnum` / `db_type` / 会话区间 / **`started_at`** | 同上 | ADR-011 §3。`created_at` 入队后语义变成入队时刻，「已排」与「已用」是两个起点 |
| 3 | `units_done` / `total_units` 改按数据批次算 | 同上 + `handlers.rs` 的 `ManualUpdateProgress` 回调 | ADR-0007 的口径迁移；`total_batches` / `batches_done` 随「运行」退役 |
| 4 | 两个新 kind：`data_batch` / `room_recalc` | 同上 | ADR-011 §3 / §10 |
| 5 | `room_recalc` 详情：面板数 / 构件数 / 死信数 / done / total | 同上 + `model_update_pending` 计数查询 | ADR-011 §10。`PendingModelWork` 没有时间戳，等待时长改由客户端按上一轮 `finished_at` 算 |
| 6 | `POST /queue/pause` 与 `/resume`，快照带 `paused` | `handlers.rs` | ADR-011 §9 |
| 7 | `GET /health` 带进程启动时刻与 `gen_spatial_tree` | `handlers.rs` | ADR-011 §4（界面要说得出「队列是重建的」）与 §8（房间增量没开时不许显示空泳道）|
| 8 | `GET /dbnums` 补 `anomaly` / `blocked` | `handlers.rs` + 从预览里提出判定逻辑 | 队列面板的「本期不执行」一格。`blocked` 是算出来的：`preview_dbnum` 只判 `Rollback \| TypeChanged`，扫描器另置 `Duplicate` / `Missing` |
| 9 | 暂停标志持久化（与水位同库，不进队列表） | `handlers.rs` + 水位所在的持久层 | ADR-011 §9：暂停最常见的后继动作就是重启服务；不活过重启等于把暂停的用意抹掉 |

注：表里写的 `web_service/tasks.rs` 指注册表**搬家之后**的位置——`TaskRegistry`
先搬出 `web_service`（第三节第一条、第九节第 4 条），九项再往上落。

**这九项一条都不含生成根口径。** 队列排的是数据批次，而批次里阶段二按什么颗粒生成是
`baseline_model_plan` 的事——它今天按 SITE 排，与规格矛盾且在大库上跑不动
（六之六 G1）。第五节新增的第 6 步就是它，本节不改口径、只把这处空白点出来。

还要删的其实是**四处**（第二轮对齐逐行核实；首稿那句「不在领域层」与代码不符，
领域层有第二道）：

- `handlers.rs:66–70` 的 `sync_live` 422 —— 合流之后手动与自动不再互斥（ADR-011 §2）。
- `handlers.rs:72–76` 的 409 与 `tasks.rs:69` 的 `running_for_project` —— 互斥是调度器的性质，
  在 HTTP 层再写一遍只会产生假冲突（ADR-011 §12）。
- `manual_update.rs:2687` 的领域层 `sync_live` 检查——与 422 同职责，一并退役。
- `manual_update.rs:2702` 的 `ProjectExecGuard` —— 单 worker 就是互斥，守卫职责被
  调度器取代；留着就是一段永远拦不到东西的死代码。

与之配套，`execute_manual_update` 拆成两半：「扫描 + 入队」（HTTP 入口与探针共用）
与「worker 执行体」。执行体直接复用 `execute_one_dbnum`（`manual_update.rs:2876`），
它本来就是按单库执行的函数，不必先拆循环。探针 `bin/manual_exec_probe.rs` 改走
入队后等队空。`POST /update/execute` 的 202 从单 task_id 改成入队回执数组
`{scanned, enqueued:[{task_id, dbnum, position}], merged, already_covered}`
——S2-D 的「已入队，排在第 N 位」直接有数据，前端不用拼（第九节第 7 条）。

`insert_running`（`tasks.rs:77`）的剔除逻辑要从一句 `find(|t| t.state != Running)` 变成三条规则：
queued / running 免除 → 每个 dbnum 保留最近一条终态 → 其余最老先剔（ADR-011 §11）。
**现在这句会把新增的 `queued` 当历史清掉，那不是取舍，是硬伤。**

---

## 三、心脏：`async_watch` 改入队 + 单 worker

不在接口层，是本轮工作量最大的一块，落在 `data_interface/increment_manager.rs`。

- **任务注册表搬家先行**（第九节第 4 条）：`TaskRegistry` 从 `web_service/tasks.rs`
  搬到 feature 无关层，`web_service` 只留 HTTP 映射。`pub mod web_service` 整个在
  `#[cfg(feature = "http_api")]` 门后（`lib.rs:82`），队列真身若住在门里，http_api
  关闭时合流路径就没有队列可用——两种编译形态两种执行行为。WS 广播照现成先例
  （`execute_incr_update` 里的 `notify_incr_applied`）用 cfg 包一行。数据批次仍是
  注册表的一种新 kind，ADR-011 §3 不变，只是注册表搬家。
- `async_watch` 的 while 循环（`:1036` 起）：`resolve_with_header` 拿到 plan 之后不再直接
  apply，改为 `enqueue`。
- 新增单 worker 调度循环——**无条件 spawn，不放进 `lib.rs:303` 那个 `tokio::join!`**
  （第九节第 5 条）：那个 join! 在 `if sync_live` 分支里，而合流后手动模式
  （sync_live=false）的执行也走队列，worker 若只活在自动分支，手动模式的队列就没有
  消费者——首稿这处写错了。进程起来就有且只有一个 worker：
  取队首 → `mark_started` → 数据应用 + 模型生成 → `finish`；队空且未暂停 → 跑一轮
  `drain_rooms`，包成 `room_recalc` 任务。
- **合并与冻结的判定已经写好了**：`src/data_interface/batch_queue.rs`，纯逻辑、不连库、
  10 条单测全绿（首稿写 8，以文件为准）。
  `enqueue()` 三条规则按序判（排队中合并 → 运行中另起一条接在右端之后 → 新排），
  `freeze_next(queue, paused)` 是 FIFO 出队并冻结，`paused` **只挡出队、不碰正在跑的那条**
  ——界面上那句「不再出队，这一批会跑完为止」就是这条实现的兑现。
  放在 `data_interface` 而不是 `web_service`，
  是因为两条触发路径合流之后共用同一份判定，而 `web_service` 按它自己的模块注释「仅作 UI 视图」。
  worker 与 `async_watch` 直接调它即可，**不要再在别处写第二份**。

验收不依赖界面：`GET /api/v1/tasks?kind=data_batch` 用 curl 就能看出排队、合并与冻结。
`one_dbnum_occupies_at_most_two_rows` 那条用例钉的正是界面上那个不变式——
再密集的保存也只塌成「运行中 + 排队中」两行。

---

## 四、客户端四项

| # | 项 | 依赖 |
|---|---|---|
| 1 | ~~ZONE 分桶拆除~~ | **已完成**，见第六节 |
| 2 | dock 加「任务队列」视图，状态栏加计数 | 服务端 1–4 项（否则是空壳）|
| 3 | 向导砍掉第三步，确认后跳队列面板 | 第 2 项（否则「确认执行」按下去无处可去）|
| 4 | 「本期不执行」一格、房间泳道 | 服务端第 5、7、8 项 |

第 3 项**故意押后**：先拆第三步而面板还不存在，中间会有一段人点完确认无处可去的空窗期。

---

## 五、顺序

1. 解两个卡口（第一节）。
2. 服务端 1–4 项 + 第三节的心脏 —— 到这里 curl 能看见队列，客户端才有得连。
3. 客户端第 2 项。
4. 服务端 5–9 项。
5. 客户端第 3、4 项。
6. **生成根口径改细颗粒**（六之六 G1）——不挡界面落地，所以排在第 5 步之后；但它是
   队列在大库上**跑得完**的前提。第十节第 3 条的验收要拿一个大库压冻结，那压的是阶段一
   （数据应用）；阶段二（模型生成）在 SITE 口径下会在同一个批次上再挂几小时、行内那个
   「x / y 个交付单元」的分母还是按 SITE 算的——验收看得到冻结，看不到一个批次善终。

---

## 六、已经做完的

- **ZONE 分桶全局退役**（`crates/plant-ui/src/model_update.rs`）：变化树从
  dbnum → ZONE → 交付单元三层改为两层，单元行 depth 2 降 1；`expandable` 判据从
  `db.zones` 换成 `db.units`；「展开全部」那枚 chip 与 `State.zones_open` 一并删除
  —— 两层树里 db 行的展开本身就是单元列表，那枚 chip 没有第二层可展；两处文案与模块 doc 改掉。
  契约侧一个字段没动，`DbPreview.zones` 与 `ZonePreview` 保留，反序列化用例继续守着它。
- **合并与冻结的领域规则**（`gen-model/src/data_interface/batch_queue.rs`，新文件）：
  `DataBatch` / `BatchState` / `enqueue()` / `freeze_next()`，10 条不连库单测全绿，clippy 干净。
  它不依赖 `web_service`，因此不受第一节那个卡口影响。
- 文档：两条 ADR、`QUEUE-FIELD-MAP.md`、`CONTEXT.md`（删 3 条术语、新增 2 条、改写 5 条）、
  `MODEL-UPDATE-FIELD-MAP.md`（S2 的 ZONE 分组行标为已删，§8 补退役说明）。
- 画板：`S12`（常态）与 `S12-B`（暂停 / 重建 / 断线降级）+ 组件 `C/QueueRow`，
  **存没存下来待验证**——旧名 `rs-plant3d-ui.pen` 在 19:52 被写过，可能就是那次
  会话的存盘（见第一节卡口二）。

验证：`cargo check` 干净；`cargo test -p plant-ui -p plant-ui-app` 34 + 4 全绿；
clippy 在 `model_update.rs` 上只剩两条早就在的「too many arguments」。

---

## 六之二、房间泳道现在打开也是空的（2026-07-27 实测）

对 8009 的只读查询。**查的是 `load_room_panel_groups` 真正读的那几张表**——
房间代码走的是独立的 `FRMW` / `PANE` 表（`select ... from FRMW where '<关键字>' in NAME`），
不是 `pe WHERE noun = 'FRMW'`；两者这次恰好都是 274，但**别拿 `pe` 去替代**，
`pe` 上连 `NAME` 字段都没有（抽样全是 null，名字在 UDA 那边），拿它筛房间名只会得到假的空集。

| 查询 | 现在 | `room_model.rs:259` 注释记的本项目值 |
|---|---|---|
| `FRMW` | **274** | 2 889 |
| `FRMW WHERE '-RM' IN NAME` | **0** | 124（即 124 间合规房间）|
| `PANE` | **0** | 有 |
| `CWALL` / `CFLOOR` | **0 / 0** | 有 |
| `inst_relate WHERE in.noun = 'PANE'` | **0** | 0 |
| `inst_relate WHERE aabb != none` | 1 575 | 906（房间审计同日早些时候）|
| `room_relate` / `room_panel_relate` | **0 / 0** | 0 |

**结论比房间审计那条更靠前一步。** 审计说的卡点是「面板有元素但没几何」；
现在**连 PANE / CWALL / CFLOOR 三张表都是空的，`-RM` 一间房都匹配不上**——
房间结构整个不在当前这份数据里。佐证：库里所有元素的 refno 前缀都是 `24384`
（FRMW / BEND / SITE 抽样一致），与 `included_db_files = ["ams8000_0001"]` 对得上，
**当前解析进来的就这一个库**；而代码注释记的 2 889 个 FRMW / 124 间房是另一份解析范围下的数字。

所以此刻把 `gen_spatial_tree` 打开，`load_room_panel_groups` 会筛出 0 间房，
房间队列永远空转，而且**不报错**。ADR-011 §8 的
「`gen_spatial_tree` 关着时界面要说『房间增量没开』」因此不是可选项，
是当前唯一诚实的形态。

要让房间泳道有东西可报，前置是把带房间结构的那个 DESI 库重新纳入解析范围并生成到
PANE 这一层。是哪个库，需要对着 E3D 项目的 MDB 结构查，本轮没查到底——那要动实库。
第二轮对齐已定（第九节第 10 条）：**这件事不进本轮**，房间泳道按
「房间增量没开 / 无数据」的诚实形态验收，数据准备单列跟踪项。

顺带一条与队列直接相关的实测，**读的时候要连着配置一起看**：

`GET /api/v1/dbnums` 返回 **299 个已登记库**，其中 DESI 约 290 个，
**只有 db8000 的水位非零**（34 / 34，已是最新），其余全部停在 0、各自压着几十到上千个
待应用会话（db7324 一家就压着 1 363 个）。

**但「已登记」不等于「在本期范围内」。** `should_process_database_with` 的最后一条是
「非系统库再看 `manual_db_nums` 里有没有它」，而当前配置是 `manual_db_nums = [7997, 7999, 8000]`
——真正会被处理的只有三个库，那 287 个 0 水位是**配置决定的，不是被重置过**。

这条对队列有两层意思：

- **今天打开 `sync_live` 不会有风暴**，只会有三个库。
- 但风暴离得只有一行配置那么远：`manual_db_nums` 一旦放宽或去掉，第一轮就是几百个库
  同时进队。所以第五节第 2 步（心脏）跑通之后，**压测要拿放宽后的规模压**，
  不能拿这三个库的手感去判断队列够不够用。压测环境已定（第九节第 9 条）：
  **可丢弃副本库**——复制数据目录另起端口、放宽 `manual_db_nums` 压
  287 库 / 13 323 会话的全量，不动 8009 实库。

放宽之后的规模已经算出来了（同一次 `/dbnums` 快照）：

| | |
|---|---|
| DESI 库 | 288 |
| 有待应用会话的 | **287** |
| 待应用会话合计 | **13 323** |
| 单库最多 | **1 724**（db7333；其后是 7327 的 1 470、7324 的 1 363）|
| 超过 100 个会话的库 | 10 |

这三个数直接改写两处设计约束：

1. **`MAX_TASKS = 200` 差了一个量级。** 第二节第 11 项定的分层保留是
   「queued / running 永不剔 + 每个 dbnum 保留最近一条终态」，那么首轮放宽之后光是
   287 条排队 + 287 条终态就要 **≥ 574** 的容量。策略本身护住了排队行不被误删，
   但常量必须跟着抬，否则终态会被当场冲光、「上次跑成什么样」永远查不到。
   数值已定（第九节第 8 条）：**1000**——574 打底，余量留给全局最近终态；
   分层规则先行，常量只是兜底上限。
2. **队列面板扛不住 287 行。** 画板 `S12` 上是 8 行的形态。287 行是**积压态**
   （首轮放宽范围、或服务停了很久），稳态下队列只有刚被保存过的那几个库。
   收敛手段已定：**默认只看有变化的库**（运行中 + 排队中 + 欠单元的部分完成），
   另给 dbnum 搜索框与状态筛选芯片，「运行中」那一行置顶常驻
   ——见 `design/QUEUE-FIELD-MAP.md` 第 8 节。

---

## 六之三、服务端 1–4 项与心脏已落地（2026-07-27 深夜，ZhiMoAll-20 会话）

全部在 gen-model 侧，`cargo test --lib` **238 passed / 0 failed**（较上一轮 +10 条队列层
单测），lib / bins × default / http_api 四种编译形态全过。ADR-011 的状态段有逐条对照。

| 项 | 落点 |
|---|---|
| `TaskRegistry` 搬家 + `queued` + 新字段（1/2 项）| `src/data_interface/task_registry.rs`（新文件；`web_service/tasks.rs` 删除）。`created_at`=入队时刻、`started_at`=开跑时刻；`units_done`/`total_units` 按数据批次算（3 项）|
| 两个新 kind（4 项）| `data_batch` / `room_recalc`；`manual_update` kind 退役不再产生新行 |
| 剔除三规则 + `MAX_TASKS=1000` | queued/running 永不剔 → 每 dbnum 留最近终态 → 全局最老终态先走；5 条单测 |
| 心脏：发现即入队 | `increment_manager.rs`：`init_watcher`/`async_watch` 只发现（`discover_batch`）+ 入队（`enqueue_discovered`）；基线与增量在发现层不分家，worker 执行体 `needs_initial_load` 接管；watcher 里的启动/周期 drain 全部移走 |
| 心脏：单 worker | `batch_scheduler.rs`（队列真身，行 ←→ 任务一一对应）+ `batch_worker.rs`（无条件 spawn，`ensure_batch_worker` 幂等守卫；出队冻结 → `execute_one_dbnum` → 每批交付单元生成；队列跑空先消化积压再收房间轮，包成 `room_recalc` 任务）|
| 四处守卫退役 + 202 回执（§8.6/8.7）| HTTP 409/422、领域层 sync_live 检查、`ProjectExecGuard` 全删；`POST /update/execute` 返回 `{scanned, enqueued:[{task_id,dbnum,position,…}], merged, already_covered, blocked, up_to_date, warnings}`；探针改「入队后等队空」（`drain_queue_until_empty`）|

**两笔注记**：① 同文件上此前未提交的 T903（watcher unwrap→上传播）与 SYST 按文件记账
两组修复随本片一并入库——worker 的批次执行依赖它们。② 客户端对接时注意 task_id 前缀
从 `mu-` 变为 `db-`（数据批次）/ `room-`（房间轮）；终态 result 形状是
`{project, status, batch, units, warnings}`（单数 batch，「一次运行」退役）。

**评审收尾（`8a0211e5`，ZhiMoAll-16 按第九节逐条核对后的跟进）**：
退役三处死代码——`execute_incr_update`（合流后无调用方，不留第二条执行路径）、
`notify_incr_applied` 与 `increments` WS 主题（从未有过消费者）、
`complete_syst_jobs` / `fail_syst_jobs` / `IncrResult::has_db_type`（随旧编排失联）；
MySQL 可选同步（feature=sql）从旧编排搬进 `execute_one_dbnum` 成功分支
（注意：sql feature 本身在本仓早已编译不过，`team_data` / `versioned_db` 的
AiosDBMgr 池 API 变更所致，与本轮无关）；`discover_batch` 统一 file_name 口径；
11 处注释引用从「第八节」改「第九节」；spec 顶部加修订注记，全文修订随 5–9 项。
`cargo test --lib` 238 通过，clippy 触及模块零告警。

---

## 六之四、服务端 5–9 项已落地（2026-07-27 夜，gen-model `c28c0f07`）

第五节第 4 步整片收口，接 `796b50b4` / `8a0211e5`：8 文件 342 行，`cargo test --lib`
仍是 **238 passed / 0 failed**，lib × default / http_api 编译干净。**客户端第 2、4 项要的
契约到这里全齐**——下表与 gen-model `docs/specs/web-service-api.md` §4.1 / §4.7 / §4.8
就是照着写界面的依据，不必再翻 ADR。

| 项 | 落点 |
|---|---|
| 5 房间轮详情 | `count_room_targets` 分项统计，随 `room_recalc` 任务的 `TaskEntry.detail` 带出 `{panels, elements, dead_letters}`；done / total 仍走 `units_done` / `total_units` |
| 6 + 9 暂停端点与持久化 | 新增 `GET /api/v1/queue` 快照 + `POST /api/v1/queue/pause`\|`/resume`；标志持久化在 `queue_control:main`（与水位同库，不进队列表），worker 起跑前恢复——**活过重启**，队列重建完也不开吃，直到 `resume`。暂停同时挡出队与空闲轮 |
| 7 `GET /health` | 补 `started_at`（进程启动时刻，「队列是重建的」靠它）、`gen_spatial_tree`、`queue_paused` |
| 8 `GET /dbnums` | 改走 `dbnum_statuses(project)`（登记表 ∪ 项目扫描，只读头部与最新会话号），逐行带 `anomaly` / `blocked` / `excluded`；判定与预览共用 `FileAnomaly::blocks`。旧字段是新形状的子集，既有消费者不受影响 |

**给客户端的三条硬约束**（都是服务端形状直接决定的，不是口味）：

1. **`blocked` 与 `excluded` 不许合成一行。** 阻断的库压根不入队、队列面板里没有它们的行，
   `/dbnums` 的 `blocked` 因此是「这个库的水位为什么一直不动」的**唯一出处**；`excluded`
   只是不在本期范围（`manual_db_nums` / 类型门控）。混进同一格「本期不执行」会把「出事了」
   讲成「本来就不跑」。五种异常 `rollback` / `path_migrated` / `type_changed` /
   `duplicate`（带 `paths[]` 交给人挑）/ `missing` 里**只有 `path_migrated` 不阻断**。
2. **暂停的文案只能说「不再出队」。** 服务端没有中止接口，正在跑的那条跑完为止；也**没有
   单条取消**——队列是派生态，从队里移掉一行不推水位，下一轮轮询照样把它发现回来，
   那是个会自己撤销的按钮（ADR-011 §9）。
3. **预览的 422 没了。** `POST /update/preview` 在 `sync_live=true` 下同样可用，代价是
   「待应用」可能偏大（正在被应用的会话也算进去）；界面按 `/queue` 快照里的运行中批次数
   标注「N 个库正在应用，数字可能偏大」。

**一条到期的前提**：第八节第 6 条把 `FailForm::SyncLive / Conflict` 的退役押到「客户端队列
视图 + 砍第三步」，理由是「今天的服务端还真会回 422 / 409，提前删就是对它说谎」——**那个
理由到本片为止已经过期**：409 随 §12 在 `796b50b4` 退役，预览的 422 随本片退役，服务端两种
都不会再回了。退役时机不改（仍随队列视图一起走，避免半截形态），但留着它们从此是「守着
一段拦不到东西的死分支」，不再是「对服务端诚实」。

**仍欠一次实机 curl 验收**（排队 / 合并 / 冻结实况）：验证当晚 8021 有在跑的旧服务 + 活动
客户端，未做本机实跑，留待下次服务重启窗口——ADR-011 状态段同记。

---

## 六之五、客户端第 2 项已落地（2026-07-27 深夜，ZhiMoAll-16 会话）

dock 加「任务队列」视图 + 状态栏计数，全部在 plant-ui 侧。
`cargo test -p plant-ui -p plant-ui-app` **48 + 5 通过**（较上一轮 +14 +1），
clippy 在新增模块上零告警（`model_update.rs` 那两条「参数过多」是早就在的）。

| 项 | 落点 |
|---|---|
| 视图本体 | `crates/plant-ui/src/task_queue.rs`（新文件）：契约 + Vm + 行推导 + 绘制。放顶层而不是 `workbench/` 下，与 `model_update` 同源——那一层按模块注释是纯绘制 |
| dock 接线 | `Pane::TaskQueue` 与日志同一排页签（命令行仍并排，两者要同时看得见）；`WorkbenchState::focus()` 供「确认执行」之后自动跳过去 |
| 状态栏 | `vm::QueueStatusVm`：`队列 N` / `队列 N · 已暂停`，另加「已过滤 N 条其它项目条目」微字。**没读到快照就整格不画**——「队列 0」与「还没拿到第一份快照」是两句话 |
| 数据通道 | 轮询 `/queue` + `/tasks?limit=200` + `/health` + `/update/pending-units` 四份打包成一次 `Evt`，有活一秒一拍、全空五秒；WS 推广成 `Scope::Run` / `Scope::Queue` 两种订阅，队列那条订全部任务并按 task_id 分桶铺明细 |
| 两个来源的分工 | **排队与运行中的行以 `/queue` 为准**（不封顶，287 行也全在），`/tasks` 只补计时、单元计数与终态历史——服务端把 `limit` 钳到 200，拿它当队列来源会在积压态下截断 |

**契约错位一并修掉（这两处解错了界面不会报错，只会静默空掉）**：
① `Outcome.batches: Vec<_>` → `batch: Option<_>`（服务端是单数 `DataBatchTaskResult`，
旧形状带 `serde(default)` 会静默解成空数组，终态摘要整块消失）；
② `Accepted{task_id,kind,state}` → `Enqueued` 入队回执（三个必填字段会让
`POST /update/execute` 的响应**整个反序列化失败**，向导那枚「确认并开始」按下去
只会得到一个看不懂的 internal 错误页）。第八节第 2 条把空窗期记成「确认后无处可看」，
实际形态比那更糟，已由本片修正。

**顺带的后果**：向导确认之后改为「回执进日志 + 关窗 + 跳队列页签」，
`Vm::Running` / `Run` / `RunVm` 与第三步的整段绘制**已经没有构造点**，
成了编译得到、跑不到的死代码——它们随客户端第 3 项「砍第三步」一并退役
（第八节第 6 条的时机到了）。`Run` 的 `total_batches` / `batches_done` 同批走。

**验收**：`output/queue-mock.ps1` 是按 QUEUE-FIELD-MAP 样例数据搭的替身服务，
`output/queue-s12-normal.png` / `queue-s12b-paused.png` 是对着它拍的实机截图，
逐行与 S12 / S12-B 对得上（生成中 14 / 24、排队第 N 位、已排 / 已用两个起点、
部分完成 · 欠 2 个单元、暂停后转「已暂停 · 第 N 位」、重建横幅后半句完整）。
**仍欠一次真服务验收**：8021 上跑的还是旧版，客户端没有和新服务端真正对连过。

**一处按窄面板改掉的画板口径**：S12 是按 1024 宽画的，而 dock 里这块面板默认只有
六百点上下，照画板写死列位会把「进度 / 说明」整列挤没——`上一批已冻结，这是之后
新存的会话` 一丢，同一个 dbnum 那两行看着就是重复项。改成左四列定宽、状态与说明
分剩余宽度，装不下的说明在行下面另起一行，不截断。

**本轮不做、界面上也一个字不提**：房间泳道（`gen_spatial_tree` 关着，
顶栏那格房间计数一并不画）、「本期不执行」一格——客户端第 4 项。

---

## 六之六、7997 全量生成暴露的四条队列级阻碍，与今晚的实机探针（2026-07-27 夜，ZhiMoAll-6 会话）

来源是 gen-model 侧另一条并行工作流留下的未入库报告
`gen-model/docs/2026-07-27_ams7997-full-parse-and-generation-report.md`（21:23 建，`??`）。
它本来与队列无关，但它把「队列那条路在大库上跑不跑得动」实测了一遍，结论直接落在本文件头上。
**四条里有三条不在服务端九项的覆盖范围内。**

### G1 · 基线生成根按 SITE 排，与规格矛盾，大库上跑不动（High）

`baseline_model_plan`（`manual_update.rs:2163-2179`）按 `query_type_refnos_by_dbnum(&["SITE"], …)`
排生成根；而规格 `docs/specs/manual-model-update.md` §最小交付单元 明写 SITE / ZONE / WORL
只作层级容器、**不允许成为生成根**。

7997 的 SITE `24381/101405`（/1PTU-INST23）子树 **75,967 元素、占全库 48%**。实测拿它喂
`generate_unit_model`：**20 分钟零写入**，两端 CPU <7%、磁盘 0 IOPS——瓶颈是小查询循环的
往返延迟，不是算力；且根内无检查点，失败即整根重来。报告的执行路径里那一行写得很直白：
`manual_exec_probe.exe（消费队列）← 在 76k 元素的 SITE 根上不可行，放弃`。

**这对队列视图是釜底的一条**：面板上「x / y 个交付单元」的分母，在服务端目前仍是 SITE 口径。
G1 不修，7997 这种库上队列跑出来的就是一行几小时不动、还说不出自己为什么不动的批次
——正是 ADR-0011 当初判「S4 站不住」的那个形态，只是换了个地方摆。

改道已验证：客户端算细颗粒根（`Get-7997RootCover.ps1`）= 交付单元根 1,918（BRAN 814 /
EQUI 992 / HANG 112）+ 残差根 972 = **2,890 根**，8 并发 ensure 约 100 分钟全量收敛、
根粒度断点续跑。修法就是照这个口径改 `baseline_model_plan`。

### G9 · 绕过队列生成后 regen_root 行永久残留（Medium·一致性）

`ensure_model_generated` 成功后不清 `model_update_pending` 里对应的 regen_root 行。8000 上
实测有 4 条 SITE regen_root 躺了 4 小时以上，而**任何后续 `execute_manual_update` 都会把整个库
重新生成一遍**（对 7997 是 20 小时级）。队列面板直接吃这个亏：QUEUE-FIELD-MAP §1 的
「欠 N 个单元」读的就是 `/update/pending-units`，残留行会让它长期显示一个不真实的欠账。

### G5 / G6 · `model_update_pending` 没有 claim/lease，跨进程踩踏（High·数据安全）

`ProjectExecGuard` 是**进程内**静态锁。两个 probe 进程同时消费同一队列、同根被并发
delete-then-write，本轮实测发生过；被硬杀的一方留下「行已清、数据半途」的孤儿根
（24381/100675），靠人工重排队列行修复。

与第二节直接对话：四处守卫退役的理由是「单 worker 就是互斥」——那在**一个进程内**成立。
probe 类二进制与常驻服务并存时不成立，而合流之后探针仍然存在（只是改成了「入队后等队空」）。
修法是给行加 `claimed_by` / `claimed_at` + CAS 抢占 + 超时回收，把互斥从君子协定变成存储层事实。

### G4 · 空交付单元 ensure 回 500（Medium·契约）

无子件的 BRAN 生成后 0 实例，`/model/ensure` 回 `500 {"code":"internal", …}`，而规格
`web-service-api.md` §4.5 定义此形态应是 200 + `NoRenderableGeometry`。QUEUE-FIELD-MAP §1
那枚「重试」按钮打的正是这个接口——**界面会把「这个单元本来就是空的」当成服务故障，
让人反复重试**。修法：`written==0 && renderable==0` 且流程本身成功 → 归
`NoRenderableGeometry`，并给 `OnDemandModelResult` 加 `empty_unit: bool`，让界面分得清
「画不出」与「本来就没东西」。

### 实机探针：那次验收欠着，欠的原因很具体——进程没换

对 8021 的只读 curl：

| 探针 | 结果 |
|---|---|
| `GET /api/v1/queue` | **404** |
| `GET /api/v1/health` | 只回 `{status, project, sync_live, version}`，无 `started_at` / `gen_spatial_tree` / `queue_paused` |
| `GET /api/v1/dbnums` | 299 行，每行无 `blocked` / `excluded` |
| `GET /api/v1/tasks` | **0 条**——而进程此刻正满载跑 ensure，旧注册表压根不记它 |

进程 `aios-database`（PID 51180）起于 **21:05:18**，用的是 **19:42 构建**的 debug 二进制；
而服务端 5–9 项是 **21:38:30** 才提交的（`c28c0f07`）。三处缺字段同一个原因。

水位现状（`manual_db_nums` 因 7997 全量生成窗口临时收窄到 `[7997]`）：7997 = 84 / 84、
8000 = 34 / 34、**7999 = 0 / 41**。

**换服务的时机还有一条物理约束**：8021 正被那轮 2,890 根的 ensure sweep 占着
（`scripts/Invoke-EnsureSweep.ps1`，8 并发），而 Windows 下运行中的 exe 不能被覆盖
——不先停它，`cargo build` 连链接都过不去。

验收环境本身按第十节第 3 条走（副本库 + `sync_live = true` + 放宽到十几个中等库含一个大库），
顺序按第十节第 1 条走（先修 H1 再验收）。这里只补一条本机事实：8021 这台上
7997 与 8000 都已追平、`execute` 只会回 `up_to_date`，**7999 那 0 / 41 是眼下唯一
还排得出队来的库**——想在本机随手看一眼队列有没有反应，只有它。

---

## 七、三条收尾决定（本轮代决）

写在 `design/QUEUE-FIELD-MAP.md` 第 7 节，要推翻只需一句话：

1. **暂停活过重启**（服务端第 9 项）；重启后界面要同时说「队列是重建的」与「仍处于暂停」。
2. **行内明细默认折叠**，只有「运行中」那一行默认展开——与变化树已有的口径一致。
3. **只画当前项目**；快照里 `project` 不一致的条目直接不显示，**不许静默混排**。

三条都不阻塞第五节的前三步。

---

## 八、本轮拷问定案（2026-07-27，ZhiMoAll-4）

对「手动模型生成的实现」整体过了一遍拷问，逐题定案如下。与第七节同一待遇：
**要推翻只需一句话。**

1. **北极星不变**：按第五节的五步序推进，不改序。
2. **空窗期照单接受**：心脏上线到客户端队列视图落地之间，向导确认执行后无处可看
   （`manual_update` 这个 kind 随心脏消失），窗口内靠 curl 观察；**窗口期不拿新服务端
   给真实使用场景用**。不为过渡写一次性回执 UI。
3. **plant-ui 侧基线已解**：vendor（连带改动记录表补登）、三线代码、文档设计配置共三笔
   入库，另加摘按钮一笔。原议「可见性台账与模型更新分笔」让位于每笔可编译——三条线在
   `lib.rs` / `vm.rs` / `chrome.rs` / `data.rs` / `main.rs` 里共享接线，按域拆开各自编译不过。
   **gen-model 那个卡口仍未解**，在另一仓另一分支，须人工定夺。
4. **顶栏「生成模型」占位按钮已摘**：渲染出来却不发任何 Cmd，是顶栏唯一按不动的控件。
   它背后的功能（对选中元素手动补齐，ADR-0009 客户端侧）落地时按画板重新加入口。
5. **ADR-0006 押后**到本迁移完成之后；`MODEL-UPDATE-FIELD-MAP.md` S2-A 节已加排期注，
   期间 S2-A 保持诚实等待态。
6. **退役品统一时机**：`FailForm::SyncLive / Conflict`（409/422 分型）、`Run` / `RunVm`
   口径、词表「运行」——随「客户端队列视图 + 砍第三步」一起退，之前一行不动。今天的
   服务端还真会回 422/409，提前删就是对它说谎；CONTEXT.md 先行改口是术语表的正常形态。
7. **三条收尾代决照单全收**（第七节），另加一条实现细则：跨项目过滤不许无声——状态栏
   计数处带一句「已过滤 N 条其它项目条目」的微字，免得有人对着空面板怀疑服务没连上。
8. **显示补齐（ADR-0009）与队列维持分离**：队列只画数据批次与房间泳道。ensure 在服务端
   没有任务记录，硬塞进队列就得造假行；将来真做批量 ensure 接口、有了 task_id，再以
   正式 kind 进队列——那是契约驱动的自然结果，不是现在的例外。

---

## 九、第二轮对齐（2026-07-27 晚 · ZhiMoAll-16 会话代决）

对着代码逐行核实后的十一条决定。与第七节同一性质：**要推翻只需一句话**。
（对齐会上人不在场，均按推荐项超时自决——更要盯着这一节复核。
本节与上一节是同日两个会话各自的拷问记录，先编号撞成两个「八」，已改九。）

1. 施工照第五节顺序，不重排；本轮只细化。
2. 卡口一最小基线：`src/web_service/`（5 文件）+ `batch_queue.rs` +
   `data_interface/mod.rs` 两行挂载，单独一个 commit，提交前 `cargo check`。
   其余 200+ 脏文件是别的工作流的中间态，不动。
   **已执行**：gen-model `4184a5b1`（7 文件 1006 行，`cargo check --features
   http_api` 通过）——上一节第 3 条「gen-model 卡口仍未解」自此过时。
3. 画板权威定 `plant-ui.pen`：先在 Pencil 打开 `rs-plant3d-ui.pen` 验证
   S12 / S12-B / C/QueueRow 是否已存盘；在，就把那份存成 `plant-ui.pen`、git 里
   rename 掉旧名；两份都没有，按 `QUEUE-FIELD-MAP.md` 重画。
   **已执行**：三者都在 19:52 的旧名文件里（S12 `IoAZx` / S12-B `c58Zc0` /
   C/QueueRow `smhp5`），其余 17 个顶层节点两文件逐 id 一致、旧 `plant-ui.pen`
   是纯子集；已 `git mv` 旧名为 `plant-ui.pen`，17:24 那份留了
   `plant-ui.pen.stale-20260727-1724.bak`（git 历史 `504d4c1` 里也有），可删。
   **去重收尾（ZhiMoAll-4，同晚）**：rename 落地前，ZhiMoAll-4 会话按「画板已丢」的
   误判从 `QUEUE-FIELD-MAP.md` 并行重建过一套 S12 / S12-B / C/QueueRow，两套一度同
   处一个文件。已按原版为准删掉重建版三节点，原版三件套逐板截图完好；顺带留下一条
   画板标注：S1 命令栏「生成模型」按钮代码侧已摘（第八节第 4 条），链路落地时按板上
   形态重接。`.bak` 已删。**教训**：两个会话共用一个 Pencil 编辑器时，后动手的一方
   看到的顶层清单可能是对方会话的合并态，「在不在」要对着磁盘文件与 git 各验一遍。
4. `TaskRegistry` 搬出 `web_service` 到 feature 无关层；队列真身单一，
   不随编译形态分叉（详见第三节第一条）。
5. worker 无条件 spawn：一个进程一个消费者，不分 sync_live（详见第三节）。
6. 领域层两道守卫（`manual_update.rs:2687` 的 sync_live 检查、`:2702` 的
   `ProjectExecGuard`）随 HTTP 层的 422 / 409 一并退役；`execute_manual_update`
   拆成「扫描 + 入队」与「worker 执行体」（复用 `execute_one_dbnum`），
   探针二进制改走入队后等队空（详见第二节）。
7. `POST /update/execute` 的 202 返回入队回执数组
   `{scanned, enqueued:[{task_id, dbnum, position}], merged, already_covered}`。
8. `MAX_TASKS` = 1000；分层保留规则先行，常量只是兜底上限。
9. 压测在可丢弃副本库上做：复制数据目录另起端口、放宽 `manual_db_nums` 压全量，
   不动 8009 实库。
10. 房间结构数据准备不进本轮；泳道按「房间增量没开 / 无数据」诚实形态验收，
    数据准备单列跟踪项。
11. 对齐结果直接写进本文件，不另开文档；ADR-011 的「尚未实现」状态等实现落地再改。

---

## 十、第三轮拷问定案（2026-07-27 深夜 · ZhiMoAll-1 会话）

前两轮拷问的是施工顺序与设计口径。本轮换了个入口：**先对合流上来的队列层做一遍
独立缺陷审核，再拿审出来的东西倒逼决策**。findings 全文在
`gen-model/docs/2026-07-27_queue-layer-audit-round3.md`（3 高危 / 4 中等 / 3 低），
本节只记定案。与第七、八、九节同一性质：**要推翻只需一句话。**

（本轮人在场，六题里五题手选、第 1 题超时自决按推荐项。）

1. **先修 H1，再做实机 curl 验收**——不是并列两件事。验收手段就是
   `GET /api/v1/tasks?kind=data_batch` 看排队/合并/冻结（第三节），而 H1 的
   task_id 碰撞恰好会让任务行静默消失；先验收的话，看到异常分不清是队列逻辑错了
   还是注册表覆盖了。
2. **task_id = `db-{时间戳}-{单调序号}`**（`AtomicU64`，`{:06}`）。保留时间戳是为了
   日志里一眼看得出何时入队，序号单调则让字典序即入队序。查过 plant-ui：task_id
   只当不透明标识用（`find` 比较、HashMap 键、`contains` 去重），无前缀解析、
   测试夹具用的是 `db-7997-1` 这种随手编的形状——**格式不是契约**。
   同时 `insert_entry` 撞键不再静默覆盖，打 `log::error!` 列出新旧两行。
3. **实机验收在副本库 + `sync_live = true` + 放宽到十几个中等库（含一个大库）上做。**
   三条理由：① `lib.rs:301` 是 `if sync_live { ... async_watch() }`，而当前
   `sync_live = false`——自动路径压根不跑，ADR-011 §2「两条路径合流」这个核心命题
   验不到；② 当前 `manual_db_nums = [7997]` 是单库（比六之二记的三个库又窄了），
   FIFO 多库排队无从观察；③ 含一个大库（如 db7333 的 1 724 个会话）才能让批次跑得
   够久、稳定复现冻结，不必掐着秒抢时间。287 全量留给 H1 修完后的压测（第九节第 9 条）。
4. **验收前只做「止血」两件：H3 锁中毒恢复 + worker 存活可观测，不做自动重启。**
   H2 与 H3 是同一个故事的两半——panic 之后会不会连坐（H3）、你看不看得见（H2）；
   「自愈」是第三件事。验收阶段需要的是**分得清**：worker 死了、大库在慢慢跑、
   暂停旗误置、队列真空了，这四种在外部观察上今天长得一模一样。自动重启反而会把
   一个可复现的 panic 变成反复重启的噪音，掩盖根因。
5. **冻结区间按 ADR 收口。** ADR-011 §5 把冻结点定在「执行真正开始之前」的那次重扫，
   而 `batch_queue.rs` 的注释与单测把它定在入队时的 `end_sesno`——两个定义打架，
   差落在 `BehindRunning` 的 `running_end + 1` 上。定案：`end_sesno` 正名为
   **预期上界**，执行开始后由 `record_frozen_end` 把真实值回写队列行与任务行。
   不动 `execute_one_dbnum`（它自带回退阻断 / 基线补全 / 崩溃重放三套语义，风险最高）。
   依据是你们自己定的尺子：ADR-011 §4「不许装成一直在那儿」、六之四「混进同一格会把
   『出事了』讲成『本来就不跑』」——显示一个已知偏小的区间也是说谎。
6. **F2 用启动期自检堵，不自动加载 `common.surql`。** 试调一次
   `fn::find_ancestor_types`，缺了就用指名道姓的错误顶在最前面。不自动加载的理由：
   那份脚本是 `REMOVE FUNCTION` + `DEFINE FUNCTION` 形态，启动时无条件重建会把库里
   手工调过的函数静默盖掉，而它自己还带着未提交改动、权威版本本身没定。
   本轮新查到一条：`increment_pipeline.rs:1561` 有单测在钉这个调用，但断言的是
   **渲染出的 SQL 字符串**而非函数在库里存不存在——「238 全绿」里有一条专门盖住了
   F2 的失败模式。

### 顺带澄清一个被反复引用的数字

文档里多处写「gen-model 215 / 217 / 206 项未提交，是最大工程风险」。实测拆开
（`git status --short` 共 221 项）：**未跟踪 `??` 123 项**、已跟踪内容真改动 60 项、
删除 33 项。大头是未跟踪的日志 / 探针 / teach 笔记 / output 产物。

另有一批是 **CRLF 幻影脏**：`core.autocrlf = true` 且无 `.gitattributes`，
`batch_scheduler.rs` / `batch_worker.rs` / `task_registry.rs` 三个文件 `git status`
标着 M、`git diff` 却零差异。本轮改的 8 个文件动手前**全部不在真改动清单里**，
所以这批改动是 `c28c0f07` 之上的纯增量，没卷进任何别人的中间态。

### 仍然欠着

- **实机 curl 验收**（第 3 条的方案已定，环境未搭）。
- **H2 的自愈半边**（有意押后，见第 4 条）。
- L2、L3、`sync_publisher` 的 SQL 拼接转义，以及上一轮的 F1 / F5 / F6 / F7。
  （**L1 已排除**：`owner_change` 覆盖旧属主那条追到上游后不成立——`pdms-io/src/io.rs:761-812`
  的差分保证 modified / added / deleted 三桶互斥，覆盖分支走不到。别去"修"它。）
- `model_impact.rs`（67 KB）、`manual_update.rs`（200 KB）、`model_update_pending.rs`
  （74 KB）、`cata_closure.rs`（62 KB）本轮只做了定点抽查，**没有逐行过**。
