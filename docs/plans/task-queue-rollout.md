# 任务队列 · 施工顺序

- 对象：`D:\work\plant-code\old\gen-model`（分支 `codex/manual-incremental-verify-checkpoint`）与
  `D:\work\plant-code\old\plant-ui`（`master`）
- 日期：2026-07-27
- 依据：`gen-model/docs/adr/ADR-011-one-data-batch-queue-for-manual-and-auto.md`（12 条服务端决策）、
  `docs/adr/0011-execution-progress-moves-to-the-task-queue.md`（客户端划界）、
  `design/QUEUE-FIELD-MAP.md`（验收基准）、`CONTEXT.md`（术语）
- 性质：设计已定稿，本文件只排施工顺序。已落地的改动见第六节；服务端只新增了一个
  **不依赖 `web_service`** 的纯逻辑模块，接口层一行未改。

**一句话现状：设计与字段出处都齐了，两个卡口卡在人手上——gen-model 的
`src/web_service/` 还没入库；画板存没存下来要在 Pencil 里验证（19:52 旧名文件被
写过，见第一节）。第二轮对齐的十一条决定见第八节。**

---

## 一、两个卡口

| 卡口 | 事实 | 为什么必须先解 |
|---|---|---|
| gen-model 未提基线 | 215 个脏文件，且 `src/web_service/` 整个目录是 `??`（未跟踪） | 第二节九项里的**前八项全落在那个目录里**。未入库意味着改坏了没有 `git diff` 可看、没有 `git checkout` 可回 |
| 画板存盘待验证 | `plant-ui.pen` 仍是 17:24 那份；但旧名 `rs-plant3d-ui.pen` 在 19:52 被写过（727KB > 654KB），`S12` / `S12-B` / `C/QueueRow` **可能已存进旧名文件**——.pen 加密，只能在 Pencil 里打开验证 | 验证之前不知道画板是不是丢了。HTML 快照在 `output/queue-boards-backup.html`，但那是只读的，回写不了 .pen |

卡口一的解法已定（第八节第 2 条）：最小基线单独一个 commit——`src/web_service/`
（5 文件）+ `src/data_interface/batch_queue.rs` + `data_interface/mod.rs` 那两行挂载，
提交前跑 `cargo check`；其余 200+ 脏文件是别的工作流的中间态，不动。
`increment_manager.rs` 与 `lib.rs`（心脏手术对象）本来就是干净的，已有基线。

卡口二的解法已定（第八节第 3 条）：权威定 `plant-ui.pen`（rollout 与
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
先搬出 `web_service`（第三节第一条、第八节第 4 条），九项再往上落。

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
——S2-D 的「已入队，排在第 N 位」直接有数据，前端不用拼（第八节第 7 条）。

`insert_running`（`tasks.rs:77`）的剔除逻辑要从一句 `find(|t| t.state != Running)` 变成三条规则：
queued / running 免除 → 每个 dbnum 保留最近一条终态 → 其余最老先剔（ADR-011 §11）。
**现在这句会把新增的 `queued` 当历史清掉，那不是取舍，是硬伤。**

---

## 三、心脏：`async_watch` 改入队 + 单 worker

不在接口层，是本轮工作量最大的一块，落在 `data_interface/increment_manager.rs`。

- **任务注册表搬家先行**（第八节第 4 条）：`TaskRegistry` 从 `web_service/tasks.rs`
  搬到 feature 无关层，`web_service` 只留 HTTP 映射。`pub mod web_service` 整个在
  `#[cfg(feature = "http_api")]` 门后（`lib.rs:82`），队列真身若住在门里，http_api
  关闭时合流路径就没有队列可用——两种编译形态两种执行行为。WS 广播照现成先例
  （`execute_incr_update` 里的 `notify_incr_applied`）用 cfg 包一行。数据批次仍是
  注册表的一种新 kind，ADR-011 §3 不变，只是注册表搬家。
- `async_watch` 的 while 循环（`:1036` 起）：`resolve_with_header` 拿到 plan 之后不再直接
  apply，改为 `enqueue`。
- 新增单 worker 调度循环——**无条件 spawn，不放进 `lib.rs:303` 那个 `tokio::join!`**
  （第八节第 5 条）：那个 join! 在 `if sync_live` 分支里，而合流后手动模式
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
第二轮对齐已定（第八节第 10 条）：**这件事不进本轮**，房间泳道按
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
  不能拿这三个库的手感去判断队列够不够用。压测环境已定（第八节第 9 条）：
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
   数值已定（第八节第 8 条）：**1000**——574 打底，余量留给全局最近终态；
   分层规则先行，常量只是兜底上限。
2. **队列面板扛不住 287 行。** 画板 `S12` 上是 8 行的形态。287 行是**积压态**
   （首轮放宽范围、或服务停了很久），稳态下队列只有刚被保存过的那几个库。
   收敛手段已定：**默认只看有变化的库**（运行中 + 排队中 + 欠单元的部分完成），
   另给 dbnum 搜索框与状态筛选芯片，「运行中」那一行置顶常驻
   ——见 `design/QUEUE-FIELD-MAP.md` 第 8 节。

---

## 七、三条收尾决定（本轮代决）

写在 `design/QUEUE-FIELD-MAP.md` 第 7 节，要推翻只需一句话：

1. **暂停活过重启**（服务端第 9 项）；重启后界面要同时说「队列是重建的」与「仍处于暂停」。
2. **行内明细默认折叠**，只有「运行中」那一行默认展开——与变化树已有的口径一致。
3. **只画当前项目**；快照里 `project` 不一致的条目直接不显示，**不许静默混排**。

三条都不阻塞第五节的前三步。

---

## 八、第二轮对齐（2026-07-27 晚 · ZhiMoAll-16 会话代决）

对着代码逐行核实后的十一条决定。与第七节同一性质：**要推翻只需一句话**。
（对齐会上人不在场，均按推荐项超时自决——更要盯着这一节复核。）

1. 施工照第五节顺序，不重排；本轮只细化。
2. 卡口一最小基线：`src/web_service/`（5 文件）+ `batch_queue.rs` +
   `data_interface/mod.rs` 两行挂载，单独一个 commit，提交前 `cargo check`。
   其余 200+ 脏文件是别的工作流的中间态，不动。
3. 画板权威定 `plant-ui.pen`：先在 Pencil 打开 `rs-plant3d-ui.pen` 验证
   S12 / S12-B / C/QueueRow 是否已存盘；在，就把那份存成 `plant-ui.pen`、git 里
   rename 掉旧名；两份都没有，按 `QUEUE-FIELD-MAP.md` 重画。
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
