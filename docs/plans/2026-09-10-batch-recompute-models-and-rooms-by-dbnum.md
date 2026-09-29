# 重新计算（按库批量）：重新生成模型 / 重新房间计算 · plant-ui 侧计划

- 日期：2026-09-10
- 提出：用户「添加重新生成模型和重新房间计算的功能，可以选 dbnum 来批量执行。在 plant-ui 里先使用 pencil
  设计 UI，使用 plannotator 制定开发计划」；**第一轮 Plannotator 批注（2026-09-10）：「重新解析这个功能也加上」**
  ——第三个动作「重新解析」随之并入（清库 + 按首次导入重新解析 + 整库生成 + 房间，一个 dbnum Task 四段全跑）。
- 范围：`plant-ui`（`task_queue.rs` / 新文件 `recompute.rs` / `lib.rs::Cmd`）、`plant-ui-app`（`model_update_api.rs` /
  `main.rs` / `sim.rs`）、`CONTEXT.md` 两条词条。**gen-model 侧**：模型那一半**零改动**（吃它已有的
  `POST /api/v1/dbnums/{dbnum}/model/rebuild`）；房间那一半与重新解析那一半**各要一条新契约**（§九），服务端实施稿在
  gen-model `docs/plans/2026-09-10-recompute-contracts-rooms-rebuild-and-data-reinitialize.md`（C1–C7；那边的 C2 / C5 就是这边的 D10 / D4）。
- 依据：本仓 ADR-0005（进度只走 WS）、ADR-0011（执行进度在任务队列不在向导）、ADR-0019（按写入时刻说话）、
  ADR-0026（命令面永远走模型服务）、ADR-0027（按段降级不按服务端代际）；`CONTEXT.md`「dbnum 任务 / 提前执行 /
  仅模型任务 / 任务队列 / 水位」；姊妹计划 `2026-09-08-manual-update-is-a-dbnum-task-run-early.md`（下称 **09-08 计划**）
  的 D1 / D4 / D5 / D7 与 §5.1 六态判据；gen-model `docs/specs/web-service-api.md` §4.5.1（model/rebuild）、
  `src/data_interface/model_rebuild.rs`、ADR-060 / ADR-065 / ADR-067。
- 状态：**第 2 版 2026-09-10 经 Plannotator 批准**（第 1 版批注「重新解析这个功能也加上」已并入）；R0 / R1 已落地（§六）。
  决策点 D1–D11 见 §四；**D4 / D5 / D10 三条仍待用户逐条拍**（gen-model 侧计划的 C5 / C2 与之同题），其余按表内建议走。
- 界面稿：三张画在 `ui/手动增量更新.pen`（与 09-08 计划的八张同一画布、同一套变量；第 2 版已加上「重新解析」
  那张卡、计划书的红卡变体与队列里的四段行），导出：
  - `docs/plans/assets/2026-09-10-recompute-mockups/1-pick-libraries-and-actions.png` —— 第 1 步 选库与动作（三张动作卡）
  - `docs/plans/assets/2026-09-10-recompute-mockups/2-execution-plan.png` —— 第 2 步 执行计划书（下半是勾了重新解析的变体卡）
  - `docs/plans/assets/2026-09-10-recompute-mockups/3-in-the-task-queue.png` —— 队列里的样子（首行是重新解析的四段行）
  - `docs/plans/assets/2026-09-10-recompute-mockups.html` —— 三张合一
  稿子按本文口径画；两边不一致时以本文为准，改稿。

---

## 一、一句话

「重新计算」不是第三条流水，是三种**人工触发的 dbnum 任务**——**重新生成模型** = 仅模型任务的人工 rebuild
来源（`CONTEXT.md`「仅模型任务」三条来源之一，今天服务端已经有口，界面上没有）；**重新房间计算** = 对一个库的
房间归属整段重算（服务端今天只有启动那一条路，没有 HTTP 口）；**重新解析** = 整库清空、按首次导入重新解析当前
文件、再整库生成模型与重算房间（服务端内部就是回退重建那条 `intent=reinitialize` 的 worker 路，只是今天没有人工
触发口）。「批量」只是**客户端一次选多个库、逐库排一个 Task**，进的还是任务队列那一条线，不插队；前两种不碰数据
水位，第三种让数据水位归零再重建。界面上要做的是：一个两步窗（选库与动作 → 执行计划书），加任务队列里能认出这三种行。

---

## 二、现状勘探（2026-09-10 工作树实位）

### 2.1 服务端有什么、缺什么

| 动作 | 契约 | 现状 |
|---|---|---|
| 重新生成模型 | `POST /api/v1/dbnums/{dbnum}/model/rebuild`（gen-model spec §4.5.1；`handlers.rs::dbnum_model_rebuild` → `model_rebuild::start`） | **有**。单库、`202 {task_id, dbnum, expected_roots, discarded_pending, state:"queued"}`；同库已有在飞重建回同一 `task_id`；`watch_scope` 外 400；`model_full_rebuild_enabled=false` 整个拒；不预删旧模型、不动数据水位；重建期间同库 `model/ensure` 回 409；任务只在进程内，重启不恢复。Task 进 `TaskRegistry`（`insert_running_model_rebuild`，`detail` 带 `expected_roots / pending / completed / dead / stage`） |
| 重新房间计算 | —— | **没有 HTTP 口**。房间全量只有启动那一臂的六条显式入口（无参照 / 读参照失败 / 同键不等 / 参照不可读 / 持久 `rebuild_required` / 环境变量 `AIOS_ROOM_FULL_REBUILD`，gen-model `lib.rs::reconcile_rooms_with_resident_model`，共识 d-42）；按库的房间重算在服务端内部是 Task 收尾的 `room_recalc_element / room_recalc_panel` 动作（ADR-065 / ADR-067），没有一个「对这个库整段重算」的对外命令 |
| 重新解析 | —— | **没有人工触发口，但 worker 路是现成的**：`BatchIntent::Reinitialize`（gen-model `batch_queue.rs`）今天只由回退检测排出（`increment_manager::reinit_batch` / `manual_update.rs:5090`），worker 冻结点复核后 `wipe_dbnum_for_reinit` 整库清空 → `initialize_dbnum_baseline` 首次导入 → 整库模型 → 房间（ADR-021 §2/§3）；清库在 worker 里做，受队列暂停 / `startup_autorun` 约束。运维兜底只有 `DELETE /api/v1/dbnums/{dbnum}/data?confirm={dbnum}`（要求**先暂停整条队列**、该库无排队 / 运行行，删完队列仍停着）+ 之后 `/update/execute` 走首次导入——三步手拼，不适合做成批量按钮 |
| 取消 | —— | 三种都**没有取消口**。重建一旦开工就跑到底；重新解析清库之后中途杀进程 = 幽灵水位，下次启动按首次导入重跑（ADR-021） |
| 批量 | —— | 没有批量端点；单库口逐个调即可 |

`/api/v1/dbnums` 每行今天给：两枚水位与判决（`applied_sesno` / `model_sesno` / `model_verdict`）、四格计数
（`model_chasing_roots` / `model_dead_roots` / `model_lazy_roots`）、`room_sesno`（gen-model `dbnum_consistency.rs`
行结构里有，**是否每一版都序列化出来以实测为准**——没给就整列不画）、`anomaly` / `blocked` / `initialized` /
`awaiting_manual_initialization`。选库那一页的全部材料都从这里来，不加新查询。

### 2.2 plant-ui 有什么

- 单库「提前执行」已落地（09-08 计划 U2）：判据纯函数 `task_queue::early_run`（`task_queue.rs:535`，六态）、
  按钮 `early_run_button`（`:2833`）、命令 `Cmd::RunDbnumNow`（`lib.rs:225`）→ `main.rs:3621` 发
  `data::Req::ModelUpdateExecute { dbnums: Some(vec![dbnum]) }`。**它跑的是数据 Task（有数据窗口才有事做），
  不是 rebuild**——文件没有新保存时按钮是灰的，正是「重新生成模型」要补的那一格。
- 向导窗 `model_update.rs`（两步：预览 → 确认，按下关窗焦点跳队列，ADR-0011）：外壳可复用，内容不复用。
- `model_update_api.rs`：模型服务的 HTTP 客户端，有 `execute / preview / pending-units(-retry)`，**没有** `model/rebuild`。
- 任务队列 `task_queue.rs`：行按 `task.kind` 画（`data_batch / model_drain / room_recalc`），`kind` 认不出按普通行画（D7）。
  gen-model 的 rebuild Task **已从源码核出**（gen-model `task_registry.rs:23` / `model_rebuild.rs:86-134`，2026-09-10）：
  `kind = "model_rebuild"`，创建即 `running`（没有 `queued`），`dbnum` 有值，`units_done / total_units` = 已完成根 / 应重算根，
  `current_stage ∈ { coverage_scan, claim, settling }`，`detail = { dbnum, expected_roots, pending, completed, failed, dead, discarded_pending,
  stage, execution_group_size, effective_root_inflight, root_inflight_max }`，`result = { completed, model_ready: true } | { completed, dead }`；
  同库重复请求回同一 `task_id`。R0 剩下要实测的只有 `/dbnums` 是否每一版都给 `room_sesno`。
- sim（`sim.rs`）：有 `late_saves` 一幕演 Ready 态；没有 rebuild 的剧本。

---

## 三、术语（随本计划进 `CONTEXT.md`）

**重新计算 (Recompute)**：人对一个或多个设计库主动排的 dbnum 任务，三种：**重新生成模型**（该库全部权威生成根
重算，根级原子替换，模型水位随收尾前移；无数据窗口）、**重新房间计算**（该库全部构件与面板的房间归属整段重算，结果
以库为单位原子发布；无数据窗口）、**重新解析**（整库清空该库数据与模型，按首次导入重新解析当前文件，再整库生成、
重算房间；数据水位归零再重建——它包含前两种，选了它对该库就不再单排另两项）。它不插队、不改本期执行范围；批量只是
一次选多个库、逐库一个 Task。
_Avoid_：重建、rebuild / reinit（作界面词时）、全量、重算一下、刷新模型、重导

**重算批次 (Recompute Batch)**：一次「重新计算」窗按下去排出的那一组 Task 的**分组视角**——任务队列顶上一条
批次条收拢它们（几库 × 几动作、几个完成 / 运行 / 失败 / 排队）。它只是视角，不是可取消的整体：服务端没有取消口。
_Avoid_：批任务、作业组、重建任务

「仅模型任务」词条补一句：人工 rebuild 那一条来源在界面上叫「重新生成模型」，行上徽标「仅模型 · 重建」。
「重建批次」词条（今天指回退检测排出的整库重建）补一句：人工「重新解析」排出的是同一种批次（`intent=reinitialize`），
差别只在谁触发；行上徽标「重新解析」。

---

## 四、决定

| # | 题 | 结论 / 建议 |
|---|---|---|
| D1 | 入口放哪 | **建议两处**：① 任务队列面板工具条「重新计算…」开两步窗（批量主路）；② 库行「操作」列在「立即执行」旁加「重新生成模型」（单库快捷，按下直接排、不开窗、进日志）。理由同 09-08 D1：批量要一张能勾选的表，单库要一键。**「重新房间计算」只在窗里**，不给单库快捷——它有 D4 的契约前提，而且失败后果（牵涉根凭证不前移）该在计划书里看过再按 |
| D2 | 窗是几步 | **两步**：选库与动作 → 执行计划书。按下「开始执行」关窗、焦点跳任务队列（ADR-0011）；窗不跟任何一次运行 |
| D3 | 选库判据 | 复用 `early_run` 那套六态**扩一格**成七态：排除 / 够不着 → 不画这一行；阻断 → 灰 + 原因原文；需初始化 → 灰 +「先在手动更新向导首次导入」；**重建中（同库已有在飞 rebuild）→ 灰 +「回同一个 Task，不重复排」**；已在队列 / 运行中的**数据** Task → 可选（rebuild 与数据 Task 是两个 Task，服务端同库串行）；其余可选。**与「立即执行」列不同的一条**：文件没有新保存**不灰**——重新生成模型正是为「数据没变、模型要重算」而设。判据是纯函数、材料只吃 `/dbnums`。**勾了「重新解析」时判据换一份**：需初始化 → **可选**（重新解析对它就是首次导入）；已在队列 / 运行中的任何 Task → 灰 +「等它跑完再清库」（清库要求该库无在飞行，服务端冻结点也会复核）；阻断里只有「文件回退」→ 可选（那本来就是回退重建），`TypeChanged` / `Duplicate` / `Missing` 照旧灰（身份歧义，自动清库等于替人挑文件，ADR-021 §5） |
| **D4** | 房间重算的契约 | **用户定**。向 gen-model 要 `POST /api/v1/dbnums/{dbnum}/rooms/rebuild`（`202 {task_id, dbnum, state}`，形状照 model/rebuild），语义二选一：**(a) 按库**——重算该库全部构件与面板的归属，结果以库为单位原子发布（建议：与「按 dbnum 批量」的题面一致，跨库参照系不动）；(b) **全 MDB 全量**——等价启动那六条入口的人工第七条，选哪几个库都一样跑全量（简单，但「选库」在它身上是假的）。契约到位前这一卡整卡灰、写「这一档服务端没有」（D9） |
| **D5** | 批量怎么发 | **用户定**。**(a) 客户端逐库调单库口**（建议：不要求 gen-model 加端点，一库一回执，一库拒了不连累其余；顺序与并发由服务端说了算）；(b) 服务端加批量端点 `POST /api/v1/model/rebuild {dbnums:[…]}`（一次往返、一份回执，但要新契约，且「部分拒绝」的回执形状要另定） |
| D6 | 同库几个动作的顺序 | **先模型后房间**，房间等本库模型 Task **成功**；模型失败则房间那行判「跳过」并说明（房间吃的是模型产物，不拿旧产物算）；重算根成功后那一行自动重排。**勾了「重新解析」的库只排一个 Task**（它四段全跑，模型 / 房间不再单排）。跨库由服务端并行（稳态队列语义；重新解析 = 初始化形状，服务端按 FIFO 排他串行，界面照 09-08 D6 说「初始化：第 k/N 库」），界面不排序、不限流 |
| D7 | 进度怎么画 | 队列同一种行 + 「仅模型 · 重建」/「房间重算」徽标；进度只吃服务端 `detail`（`expected_roots / completed / dead / stage`），没给的段画「未知」，**不拿本端计时假装进度**（ADR-0005）。同一批用一条批次条收拢（§五.3） |
| D8 | 失败给什么 | 不给队列重试（N-C）。行内两个真入口：「重算这几根」→ `POST /model/ensure force=true`（眼睛那条路），「再执行一次这个库」→ 再调一次 rebuild。死信那一格是陈述句：「N 根上次没算成，模型水位卡在它们前面」 |
| D9 | 两代服务端并存 | 硬要求（09-08 D7 / ADR-0027）：`rooms/rebuild` / `data/reinitialize` 404 / 405 → 对应动作卡整卡灰、队列不产生那种行；`model/rebuild` 400 且消息含 `model_full_rebuild_enabled=false` → 整批不发、逐库说明；rebuild Task `detail` 缺字段 → 那一格不画；`task.kind` 认不出 → 普通行 |
| **D10** | 重新解析的契约 | **用户定**。**(a) 服务端加显式人工触发口** `POST /api/v1/dbnums/{dbnum}/data/reinitialize`（建议）：排一条 `intent=reinitialize` 的 dbnum Task，**复用回退重建那条 worker 路**（冻结点复核 → `wipe_dbnum_for_reinit` 清库 → 首次导入 → 整库模型 → 房间），`202 {task_id, dbnum, state:"queued"}`；不要求暂停队列（清库在 worker 里做，本来就受暂停 / `startup_autorun` 约束，ADR-021 §3）；同库已有在飞行 → 排后继行（`reinitialize` 到达运行中同库时留后继，spec §4.3 已有这条语义）。(b) **客户端拼三步**：`POST /queue/pause` → `DELETE /dbnums/{dbnum}/data?confirm=` → `POST /update/execute {dbnums}` → `POST /queue/resume`——不推荐：要停整条队列、连累其他库；DELETE 成功而 execute 失败时库空着没人管；20 个库就是 80 次往返 |
| D11 | 破坏性确认门 | 重新解析会清库，**多一道门**：动作卡默认不勾、丹格色徒标「清库重建 · 破坏性」；执行计划书里这些库的卡红边、数据段写清「清空 N 行 → 首次导入 1..=T」；底栏多一枚必勾的「我知道重建完成前这 N 个库在三维视图里为空、耗时按全量算」，不勾「开始执行」灰；按钮文案带数字「开始执行：含 N 库重新解析（先清库）」。不做二次输入 dbnum 的那种门——它挡的是手滑，不是不知情 |

---

## 五、设计（对应三张稿）

### 5.1 第 1 步 · 选库与动作（稿 1）

```
┌ 重新计算（按库批量） · 第 1 步 / 2 · 选库与动作 ─────────── 本期执行范围 = MDB /ALL 声明的 DESI ∩ watch_dbnum · 20 个在范围内 ┐
│ 要做什么（可多选，逐库一个 Task；重新解析包含后两项；只选后两项时同库先模型后房间）                                                │
│ [✓] 重新生成模型  POST /dbnums/{dbnum}/model/rebuild   [✓] 重新房间计算  待契约 rooms/rebuild   [ ] 重新解析  清库重建·破坏性 · 待契约 data/reinitialize │
│ 搜 dbnum / 文件名  [全选] [清空] [只选模型水位落后的] [只选有已放弃根的]                                     已选 3 / 20 · 灰掉的 3 个不计入 │
│ ☐ 设计库        数据水位   模型水位            生成根（已追平/还在赶/已放弃/没生成过）  房间      能不能选 · 为什么           │
│ ☑ 7997 DESI    415       415 [已追平]          2 720 / 0 / 0 / 12                       415 [已发布]  可选 · 重算 2 720 根        │
│ ☑ 7999 DESI    380       380 [已追平]          1 106 / 0 / 2 / 0                        380 [已发布]  可选 · 含 2 根已放弃        │
│ ▢ 8000 DESI    26        26 [模型重建中]       1 240 / 1 480 / 0 / 0                    26 [等模型]   已在重建，不重复排          │
│ ▢ 7998 DESI    402→398   402 [阻断]            —                                        —            阻断：文件回退              │
│ ☑ 8010 DESI    88        85 [模型水位落后…]    604 / 12 / 0 / 3                         85 [已发布]   可选 · 重算 619 根          │
│ ▢ 6003 DESI    0         — [需初始化]          —                                        —            尚未初始化                  │
│ 已选 3 个库 · 重新生成模型 ≈ 4 447 根（服务端判）· 重新房间计算 3 库                       [取消] [下一步：执行计划书 →]       │
└──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

- 三张动作卡的说明写清各自**会做什么 / 不会做什么**；房间卡与重新解析卡挂契约状态（有 → 正常；404 → 整卡灰 + 「这一档服务端没有」）。
  重新解析卡默认不勾、徒标「清库重建 · 破坏性」，说明里写「重建完成前该库属性与模型不可读、三维视图里为空 · 耗时 = 全量解析 + 全量生成
  （7350 实测约 2 小时）」；勾上它，表格的判据换成 D3 后半那份（需初始化变可选、在飞行的库变灰）。
- 「≈ N 根（服务端判）」= 该库 `model_chasing_roots + model_dead_roots + 已追平数`的粗和；**开工时以回执 `expected_roots` 为准**，
  计划书里写明。`/dbnums` 没给四格计数 → 整列不画、摘要里不写根数。
- 快选三个：全选 / 清空 / 只选「模型水位落后」（`model_verdict == lagging`）/ 只选「有已放弃根」（`model_dead_roots > 0`）。
- 灰行**按不下去**，理由写在行尾（D3 七态）；不等回执。

### 5.2 第 2 步 · 执行计划书（稿 2）

每库一张卡，三段（模型 / 房间 / 水位），各段可空、空了说为什么：

```
7997  DESI  ams7997_0001
  模型  重新生成模型 · 2 720 根（全部权威生成根，含 12 根从未生成过的）
        不预删旧模型，根级原子替换 · 期间 model/ensure 回 409 · 开工时作废该库排着的旧增量行（R6）
  房间  重新房间计算 · 本库全部构件与面板的归属
        排在模型之后 · 房间失败 → 牵涉根凭证不前移、模型水位卡住，行内说得出「卡在房间上」
  水位  数据水位 415 不动 · 模型水位随收尾前移到 415 · 房间水位随房间收尾
```

- 头部摘要条：几库 × 几动作 = 几个 Task、≈ 根数、「进任务队列同一条线，不插队」。
- 顺序条：按列表顺序逐库排；服务端跨库并行、同库串行；两项都选同库先模型后房间。
- 提醒条（三句，warn 色）：重建期间三维视图按需加载会等（409）；不改数据水位 / 不改范围 / 不删旧模型后空窗；
  `model_full_rebuild_enabled=false` 时整批被拒（400）、逐库说明、不部分执行。
- 按钮「开始执行：排 N 个 Task」；按下关窗、焦点跳队列；每库一条日志「已提交 db{dbnum} 重新生成模型 · task=…」。
- **勾了「重新解析」的变体**（稿 2 下半）：那些库的卡红边、卡头徒标「重新解析 · 先清空本库 N 万行」，多一段**数据**排最前
  （「整库清空后按首次导入重新解析当前文件 1..=T（N 行，服务端判）· 数据水位 T → 0 → T · 重建完成前该库属性与模型不可读」），
  模型 / 房间两段改写成「随首次导入整库做」，水位段写「三枚水位归零后随四段重建到 T」。底栏多一枚必勾的确认（D11），
  按钮文案变「开始执行：含 N 库重新解析（先清库）」。

### 5.3 队列里的样子（稿 3）

- **批次条**（accent 底）：「重新计算批次 · 08:12 排上 · 3 库 × 2 动作 = 6 个 Task · 2 完成 · 1 运行中 · 1 失败 · 2 排队」，
  右侧一句「批次只是分组视角，不是可取消的整体；服务端没有取消口」。批次由本端记（谁按的、按了哪些 `task_id`），
  重启界面即散——它不是持久对象。
- **行**：同库两行，第二行设计库列写「└ 同库 7997」；任务列两枚丸标（种类 + 状态）+ `task_id`；进度列
  主句 / 进度条 / 副句全部来自服务端 `detail`：
  - 运行中：「1 240 / 2 720 根 · 死信 0 · 阶段 claim」，副句「同库 model/ensure 此刻回 409 · 数据水位不动 · 模型水位收尾才前移」，操作列「不可取消」；
  - 成功：「1 108 / 1 108 根 · model_ready · 模型水位 380 ✓」；
  - 失败：「617 / 619 根 · 2 根上次没算成：/BRAN-… · /BRAN-…」，副句「模型水位卡在 85，不会越过这两根 · 不会自动重试」，操作列 [重算这几根] [再执行一次这个库]；
  - 房间行排队：「排队第 1 位 · 等本库模型 Task 收尾」；房间行跳过：「本库模型 Task 失败，房间重算不跑」；
  - **重新解析行**（丹格色徒标「重新解析」）：进度是 09-08 计划 D3 那**一行四段**（数据 ─ 模型 ─ 房间 ─ 水位），
    主句「数据：已清空 42.6 万行 → 首次导入 138 / 397 会话 · 模型 — · 房间 — · 水位 —」，副句「整库清空已在冻结点做完 ·
    重建完成前本库属性与模型不可读 · 末段亮起才算追平」；操作列「不可取消 · 清库已发生，只能跑到底；中途杀进程 = 幽灵水位，
    下次启动重跑首次导入」。四段里服务端没发过事件的段画「未知」（ADR-0005）。
- 脚注写 D9：404 → 房间卡 / 重新解析卡灰、不产生那种行；`detail` 缺字段 → 那一格不画。

### 5.4 单库快捷（D1 ②）

库行「操作」列在「立即执行」右侧加一枚次要按钮「重新生成模型」：可点条件 = D3 七态里的「可选」；按下 → `Cmd::RebuildDbnumModel { dbnum }` →
`model_update_api::model_rebuild(dbnum)` → 日志一行 → 队列出现该行。文件没有新保存时它**不灰**（与「立即执行」正好互补：
一个管「数据变了」，一个管「模型要重算」）。

---

## 六、里程碑

| 步 | 内容 | 依赖 | 验收 |
|---|---|---|---|
| ✅ **R0** | 本文；`CONTEXT.md` 词条「重新计算」「重算批次」+ 「仅模型任务」补一句；rebuild Task 的 `kind` / `detail` 形状**已从 gen-model 源码核出并写进 §2.2**（2026-09-10）；剩一项实测：`/dbnums` 是否每一版都给 `room_sesno` | — | **已过**（2026-09-10）。文档互引：本文 ↔ gen-model `docs/plans/2026-09-10-recompute-contracts-rooms-rebuild-and-data-reinitialize.md` ↔ `CONTEXT.md` |
| ✅ **R1** | 客户端：`model_update_api::model_rebuild(dbnum) -> Result<ModelRebuildReceipt>`；`Cmd::RebuildDbnumModel` + 库行「重算模型（整库）」按钮（D1 ②）；判据纯函数 `task_queue::model_rebuild`（D3 七态里模型那一份） | R0 | **已过**（2026-09-10）。落地形状：判据 `task_queue::model_rebuild(db, active_rebuild)` 五档（`Hidden` / `Blocked(原文)` / `NeedsInit` / `Rebuilding{task_id}` / `Ready(plan)`——「排着的数据任务」与「文件没有新保存」两档**不灰**，正是与 `early_run` 不同的两格），材料只吃 `/dbnums` 一行 + `Vm::active_model_rebuild(dbnum)`（`kind = model_rebuild` 且非终态的同库行）；操作列从 74 点扩成 74 + 4 + 44 点，第二枚是 `Btn` 空标签 + `ARROWS_CLOCKWISE` 图标；按下推 `Cmd::RebuildDbnumModel` → `Req::ModelRebuild` → `POST /api/v1/dbnums/{dbnum}/model/rebuild`（60 s），回执 `ModelRebuildReceipt{task_id, dbnum, expected_roots, discarded_pending, state}` 进日志并催一拍轮询；失败按 code 分型，`endpoint_missing`（404 / 405 / `not_found`，从 U1 的 `pending_endpoint_retired` 抽出来的通用判法）→ 「这一档服务端没有整库重建接口」（D9）；`model_full_rebuild_enabled=false` **不解析 message**（S2-D 纪律），原文进日志。按钮文案用「重算模型（整库）」而不是「重新生成模型」——后者在模型树右键里已经是客户端逐根 ensure 那条路的名字，两件事不许同名。sim：`Req::ModelRebuild` 如实回「演示模式没有模型服务」，不造假 task_id（R5 补剧本）。测试六例（隐藏 / 阻断同 `early_run`；无新保存不灰；需初始化灰；只对同库在飞 rebuild 让位；`active_model_rebuild` 忽略他种 / 他库 / 终态行；气泡只说服务端说过的）；`cargo test -p plant-ui -p plant-ui-app` = **168 / 159** 全绿；`cargo check -p plant-ui-app --target wasm32-unknown-unknown` 过（ring 需 `CC_wasm32_unknown_unknown` 指到带 wasm32 的 clang，本机 `D:\LLVM\bin`）；六份改动文件 `rustfmt --check` 干净。**未 sim / 实机过目**（R5 / R6） |
| **R2** | 两步窗（选库与动作 → 执行计划书）：复用向导外壳；表格材料只吃 `/dbnums`；快选三个；计划书三段渲染；「开始执行」逐库调 rebuild（D5 (a) 时）、每库一条日志；`model_full_rebuild_enabled=false` 整批不发 | R1；**D5 拍板** | 三段各自可空的渲染测试；逐库调用顺序 / 部分拒绝的路由测试；wasm check 过 |
| **R3** | 队列：「仅模型 · 重建」徽标、批次条（本端分组，`task_id` 归属）、进度只吃 `detail`、失败行两个入口（重算这几根 → 既有 ensure 路；再执行一次 → R1 的 rebuild） | R1 实测 `kind` / `detail` | 行状态机测试（运行 / 成功 / 失败 / 缺字段不画）；ADR-0005「没收到事件不说排队中」照旧 |
| **R4** | 房间重算：客户端 `rooms_rebuild(dbnum)`；动作卡 / 计划书房间段 / 队列房间行；404 降级整卡灰 | **D4 拍板 + gen-model 落契约** | 契约在：同库先模型后房间、模型失败房间跳过的路由测试；契约不在：整卡灰、零请求 |
| **R4b** | 重新解析：客户端 `data_reinitialize(dbnum)`；第三张动作卡 + D3 后半那份判据 + 计划书数据段 / 红卡 + D11 确认门；队列「重新解析」行吃 09-08 U3 的一行四段（U3 未落地前退成「进行中」一段 + 数据段主句，不本端计时）；404 降级整卡灰 | **D10 拍板 + gen-model 落契约**；四段进度押 09-08 U3 | 判据换份的单测（需初始化变可选 / 在飞行变灰 / 身份歧义仍灰）；确认门不勾按钮灰；契约不在：整卡灰、零请求 |
| **R5** | sim 剧本：一幕「重新计算 3 库」（运行 / 成功 / 失败 / 跳过四种行各一）+ 一幕「重新解析 1 库」（清库 → 首次导入 → 四段依次亮）+ 一幕「房间口 / 重新解析口 404」；`PLANT_UI_SIM=1` 过目截图入 `.sim-verify/` | R2 / R3 / R4b | §七第 1–5、8 条在 sim 上过目 |
| **R6** | 实机验收（真 gen-model；重新解析只在隔离项目上试，不碰生产目录） | R5 + gen-model live | §七全部 |

**顺序**：R0 → R1 → R2 / R3 可并行 → R4 / R4b 各等自己的契约 → R5 → R6。D5 选 (b) 时 R2 改成等 gen-model 批量端点，R1 的单库口照旧；
D10 选 (b) 时 R4b 改成客户端三步编排（暂停 / 清库 / 执行 / 恢复），并要补「DELETE 成功而 execute 失败」的补救提示。

---

## 七、验收

1. 选 3 个库、只勾「重新生成模型」→ 队列**当场**多 3 行「仅模型 · 重建」，批次条计数对；跑完三行绿、`/dbnums` 三库判 `in_sync`；
   全程数据水位一个字没动。
2. 同一个库正在重建时再选它：行灰、写「已在重建，不重复排」，**按不下去**；硬发也只回同一个 `task_id`（服务端幂等）。
3. 一个库 `model_full_rebuild_enabled=false`：按「开始执行」→ 整批不发，逐库写「服务端关闭了整库重建」，队列行数不变。
4. 造一个根失败：该行红、两根点名、「模型水位卡在 N」；「重算这几根」走 ensure 后行变绿；「再执行一次这个库」再排一行。
5. 两项都选：同库房间行排在模型行后、模型成功后房间行才动；模型失败 → 房间行「跳过」+ 理由。
6. 连一台没有 `rooms/rebuild` 的 gen-model：房间卡整卡灰、写「这一档服务端没有」；模型那一半照常可用；不崩、不说反话。
7. 文件没有新保存的库：「立即执行」灰、「重新生成模型」可点（两个按钮的互补关系）。
8. 勾「重新解析」选 1 个库：确认门不勾按钮灰；勾上按下 → 队列多一行「重新解析」，数据段先亮（清库 → 首次导入），四段依次亮，
   末段亮后 `/dbnums` 该行三枚水位相等、`in_sync`；期间该库在三维视图里为空、跑完自动回来；同库另两张卡对它不再单排。
9. 勾「重新解析」时：一个「需初始化」的库变可点（= 首次导入），一个队列里有在飞行的库变灰 +「等它跑完再清库」，
   一个 `Duplicate` 阻断的库仍灰。

---

## 八、风险与不做

- **房间与重新解析的契约都是空的**：D4 / D10 不拍、gen-model 不落，R4 / R4b 就是灰卡。计划把它们各单独一步，不让模型那一半等它们。
- **重新解析是破坏性的**：清库不可逆，重建期间库为空、耗时按全量算（7350 实测约 2 小时）。D11 的门挡「不知情」，不挡「手滑」；
  实机验收只在隔离项目上做。
- **rebuild Task 的 `kind` / `detail` 形状没核过**：R0 实测前不动 R3；核不出就按 D9 缺字段不画，不猜。
- **批量 = 客户端循环**（D5 (a)）：20 个库就是 20 次往返；每次 ~ms 级，可接受；一库失败不影响其余；不做客户端限流（服务端同库串行、跨库并行由它定）。
- **批次是本端视角**：重启界面就散，不持久、不同步到别的客户端——写在批次条上，不假装是服务端对象。
- **不做**：取消（服务端没有口，界面不画一个按不下去的取消）；把 `model/ensure` 做成批量重试队列（N-C）；房间的独立面板；
  改任何 gen-model 端点的形状（房间与重新解析两条新契约除外，且只提需求）；供数模式相关改动（ADR-0026 正交）。

---

## 九、给 gen-model 的契约请求（D4 / D10 拍板后发）

> 2026-09-10：服务端实施稿已写成 gen-model `docs/plans/2026-09-10-recompute-contracts-rooms-rebuild-and-data-reinitialize.md`
> （C1–C7、P0–P4）。下面这几条是需求原文，以那边为实施口径；两边不一致时先改那边、再回来改这里。

- `POST /api/v1/dbnums/{dbnum}/rooms/rebuild`，请求体 `ProjectReq` 身份字段，`202 {task_id, dbnum, state:"queued"}`；
  同库已有在飞房间重算回同一 `task_id`；`watch_scope` 外 400；语义按 D4 (a)：重算该库全部构件与面板的归属、结果以库为单位原子发布，
  排在同库在飞的模型 Task 之后；模型 Task 失败时它不跑并在 Task 终态写明。
- `POST /api/v1/dbnums/{dbnum}/data/reinitialize`（D10 (a)），请求体 `ProjectReq` 身份字段，`202 {task_id, dbnum, state:"queued",
  intent:"reinitialize"}`：排一条 `BatchIntent::Reinitialize` 的 dbnum Task，复用回退重建的 worker 路（冻结点复核 → `wipe_dbnum_for_reinit`
  → 首次导入 → 整库模型 → 房间），不要求调用方暂停队列；`watch_scope` 外 400；身份歧义异常（`TypeChanged` / `Duplicate` / `Missing`）
  400 并带原因原文；同库已有排队行 → `reinitialize` 占优合并，运行中 → 留后继行（spec §4.3 既有语义）；受 `auto_initialize_new_dbnums`
  **不**门控（它是手动路径，d-103）。回执与 `/update/execute` 的 Task 五桶同口径。
- `TaskRegistry` 给三种 Task 稳定的 `kind` 字面（建议 `model_rebuild` / `room_rebuild` / `reinitialize`——最后一个与 `intent` 同名，
  免得再造一个词）与 `detail` 字段清单（模型：`expected_roots / completed / dead / stage`；房间：`expected_panels / expected_elements /
  completed / stage`；重新解析：`wiped_rows / baseline_sessions / baseline_done / stage`），写进 `web-service-api.md`。
- 可选：批量端点（D5 (b) 时才要）。
