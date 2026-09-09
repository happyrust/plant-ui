# 更新任务详情（终态）：增删改三数与房间变更 · plant-ui 侧计划

- 日期：2026-09-09
- 提出：用户「在 plant-ui 里增加更新任务详情的查看功能。每个 dbnum 的增量更新任务结束后，
  我想看到增加、删除、修改的数量，以及房间变更的信息。帮我设计完善具体的 UI 展示（用 pencil），
  并拟定开发计划」
- 范围：`plant-ui`（`task_queue.rs`）、`plant-ui-app`（`model_update_api.rs` / `sim.rs`）、
  `design/QUEUE-FIELD-MAP.md` §1.5。**gen-model 侧只登记「待新增」字段清单（§3），本计划不反向
  要求它加任何东西**——与 09-08 计划同一条纪律。
- 依据：`design/QUEUE-FIELD-MAP.md` §1.5（终态行内明细是验收基准）、
  `docs/plans/2026-09-08-manual-update-is-a-dbnum-task-run-early.md`（一行四段 D3、失败留痕 D4、
  两代并存 D7）、gen-model `docs/plans/2026-09-08-external-increment-refactor-plan.md`
  （U5 房间内联、G2 (i) 房间失败 = 牵涉根不安定、N-C 失败只留痕不排队、N-D 房间随 Task 收尾）、
  本仓 ADR-0005（进度走 WS + 轮询）、ADR-0011（执行进度在任务队列）、ADR-0019（按保存与写入时刻说话）。
- 界面稿：`ui/手动增量更新.pen` 新增两张画板——**「更新任务详情 · 终态行内明细」**（画板
  `BgdMa`：已完成行展开后的四段明细，含增删改三数、分布条、房间变更与搬间明细）与
  **「更新任务详情 · 段形态表」**（画板 `y1o7hP`：数据段分解缺席 / 房间段无工作 / 房间段失败 G2 /
  房间段字段缺席 / 搬间超长收敛，五种形态）。导出稿
  `docs/plans/assets/2026-09-09-task-finished-detail-mockups.html`（离线可看）。
- 状态：V0（分析 + 两张画板 + 导出 HTML + `QUEUE-FIELD-MAP.md` §1.5 追记）成稿；
  **V1 已落地**（2026-09-09）：`model_update.rs` 补 `added / modified / deleted_elements`
  三字段与纯函数 `BatchResult::change_breakdown`（分解缺席判据），`task_queue.rs` 终态数据段
  画增删改三数 + 分布条（复用预览页 `ratio_bar`）、缺席退回一句总数，`sim.rs` 夹具随 `net` 带出
  三数。`cargo test -p plant-ui -p plant-ui-app` 全绿（147 + 138），新增三例：三数与总数一致 /
  老回执字段缺席只留总数 / 真零整区不画。
  **V2 已落地**（2026-09-09）：`UnitResult` 补 `kind`（`UnitKind::Regen` / `Transform`，
  `#[serde(default)]`，镜像 gen-model ADR-066）与纯函数 `Outcome::transform_roots`（只数成功
  前移的根），`task_queue.rs` 模型段在计数 > 0 时画「刚体前移 N 根（只动方位，网格未重算）」，
  旧回执缺 `kind` 恒 0 则整句不画；**元数据门那半句按计划等 §3 结构化字段，未画**。`sim.rs`
  给 7001 一根 EQUI 标 `transform` 作现成例。`cargo test -p plant-ui -p plant-ui-app` 全绿
  （149 + 138），新增两例：有 transform 根计数 / 老回执无 `kind` 计 0。V3 起未动（押 gen-model S8/T4，
  2026-09-09 10:50 复核：`panels_recalculated` / `metadata_only_roots` 等 §3 字段在 gen-model `src/`
  仍无一处，房间段契约未定形，V3 / V4 继续等；12:45 再核仍无——gen-model 侧刚落的是**规划器**的
  元数据门（0afe581be），回执结构化字段没跟着来）。
  **sim 目检已过**（2026-09-09 12:40）：`PLANT_UI_SIM=1` 实窗展开 db7001 终态行，三数 / 分布条 /
  总数句 / 刚体前移句逐项对上，db7006 走分解缺席降级句；证据（含截图与复跑要点）
  `docs/evidence/2026-09-09-task-finished-detail-sim-inspection.md`。§六「验收（实机）」四条仍欠，
  等真 gen-model 在场。
  **V1 收尾**（2026-09-09，接手会话）：分解缺席降级句去掉「共」字、回到 §1.5「分解缺席降级」
  的原句（`85 项变化（预览时 82）`），三数后的总数句照 §1.5「增删改三数」行改成
  `· 共 N 项变化 · 预览时 M`；两种形态提成纯函数 `change_total`（task_queue.rs）并加一例
  四断言的措辞回归测试。V-D2 配色勘误 accent → warn（代码与 S2 本来就一致，这次改的是文档）。

---

## 一、一句话

任务结束后人最想知道三件事——**这一窗改了什么（增 / 删 / 改）、模型跟上没有、房间有没有跟着搬**——
今天的行内明细只说得出第二件；本计划把第一件从**契约里已有但界面从没消费过的字段**里捡回来
（V1，今天就能做），把第三件登记成 gen-model 的待新增段（V3，等 S8/T4 房间内联落地），
两代服务端并存期间**缺哪段就不画哪段**。

---

## 二、契约地图（2026-09-09 对 gen-model 工作树逐条核过）

| 要显示的 | 契约出处 | 状态 |
|---|---|---|
| 新增 / 修改 / 删除三数 | `DataBatchResult.added_elements / modified_elements / deleted_elements`（gen-model `manual_update.rs:2255-2263`；`set_change_counts` 从冻结后净窗口的操作集里算，与 `changed_elements` 同源，`#[serde(default)]`） | **契约已有、界面未消费**——plant-ui 的批次解码只取 `changed_elements`（`model_update.rs:1108`、`task_queue.rs:2907-2912`） |
| 总变化数与预览差值 | `changed_elements` + 本地缓存的预览值 | 已画（QUEUE-FIELD-MAP §1.5「变化项数」） |
| 保存窗口 / 水位落点 / 并入保存 | `start/end_sesno_time`、`merged_sesnos` + `merged_sesno_times` | 已画 |
| 模型段计数与失败根 | `units_done / total_units`、`ModelUnitResult`（noun / root_refno / attempts / message） | 已画 |
| 刚体前移根数 | `ModelUnitResult.kind`（`UnitKind::Transform`，ADR-066，旧回执缺省 `Regen`） | **V2 已消费**（`Outcome::transform_roots` → 终态模型段） |
| 元数据门省下的根数 | 今天只活在回执 `warnings[]` 的一行字里（`IncrementReport.metadata_only`） | **待新增**结构化字段；ADR-0019 先例：不解析 warning 文本反拼数字 |
| 房间段（重算面板 / 构件数、受影响房间、搬间明细、失败留痕） | **无契约** | **待新增**（§3）；失败后果已由 G2 (i) 定死：牵涉根凭证不前移、模型水位卡住、库不判一致 |

**老服务端判据（数据段分解缺席）**：`added + modified + deleted == 0` 且 `changed_elements > 0`
→ 分解字段缺席，三数与分布条**整组不画**、只留总数一句；不许把缺省的 0 / 0 / 0 画成「无变化」。
（三个字段是 `#[serde(default)]`，缺席与真零靠这条判据分开：真零时 `changed_elements` 也是 0，
整个变化区本来就不画。）

---

## 三、待新增字段（给 gen-model S8 / S9 的消费者侧清单）

房间段随 dbnum Task 收尾产出（U5 / N-D），建议挂在 Task 终态 `result` 里与批次结果平级：

| 建议字段 | 语义 | 界面消费点 |
|---|---|---|
| `rooms.panels_recalculated` / `elements_recalculated` | 本 Task 就地重算了几块面板、几个构件（`RoomRecalcPanel / Element` 口径） | 房间段段头「重算 2 块面板 + 27 个构件」 |
| `rooms.touched[] = {room, moved_in, moved_out, recalculated}` | 受影响房间与迁入 / 迁出计数 | 房间行组（超 10 间收成计数 + 展开） |
| `rooms.moves[] = {refno, noun, name, from_room?, to_room?}` | 搬间明细；`from` 空 = 入屋、`to` 空 = 出屋 | 搬间明细行（列前 10 件 + 展开） |
| `rooms.failed[] = {target, message, involved_roots}` | 失败留痕；`involved_roots` 是凭证被卡住的根 | 失败框（G2 后果句 + 两个命令面按钮） |
| `metadata_only_roots: u32` | 元数据门省下的根数 | 模型段「省力注」那半句 |

界面纪律（与 D7 / 欠账段三态同一条）：**这些字段缺席 → 对应段整段不画**；`rooms` 整个缺席 →
行上四段退回三段、明细里房间段不出现——不猜、不冒充「无变更」。

---

## 四、决定

| # | 题 | 结论 |
|---|---|---|
| V-D1 | 详情放哪 | **队列行内明细（S12-C 的延伸）**，不另开新窗——ADR-0011 定了执行进度在任务队列；终态行点开展开，运行中默认展开的口径不变 |
| V-D2 | 三数怎么画 | 三个大数（`+18` / `~64` / `−3`，success / warn / danger——与预览页 S2 代码一致；本行原写 accent，2026-09-09 勘误：S2 的「修改」用的就是 `warn`，「同款视觉」以 S2 代码为准。画板示意稿通篇用的是通用示意色（绿 `#16A34A` / 蓝 `#2F6FED` / 红 `#DC2626`，均非 DESIGN-SYSTEM token），其中的蓝不构成配色依据）+ 分布条三段（与预览页 S2「三个大数 + 分布条」同款视觉，两处认得出是同一组数）+ 总数与预览差一句 |
| V-D3 | 房间段形态 | 四种（有变更 / 无工作 / 失败 G2 / 字段缺席），见段形态表画板。失败不给队列重试（N-C），给两个命令面入口：「重算这几根」走 `POST /model/ensure`、「再执行一次这个库」= 再排一次 Task——与 09-08 计划 D4 同款 |
| V-D4 | 列表上限 | 搬间明细列前 10 件 + 「展开全部 N 件」；受影响房间超 10 间收成「N 间 · 展开」。行内明细是常驻视图的展开区，不许无上限铺开 |
| V-D5 | 两代并存 | 段级缺席降级（§2 / §3 的判据），先例是欠账段三态（`pending_endpoint_retired`，09-08 U1 已落）；不问服务端是哪一代，只问这一格给了没有 |

---

## 五、里程碑

| 步 | 内容 | 依赖 | 验收 |
|---|---|---|---|
| **V0** | 本文 + 两张画板 + 导出 HTML；`QUEUE-FIELD-MAP.md` §1.5 补三行（增删改三数 / 分解缺席降级 / 房间段待新增注记） | — | 文档互引：本文 ↔ QUEUE-FIELD-MAP ↔ 09-08 计划 |
| **V1 ✅ 已落地** | **今天就能做**：批次解码补 `added_elements / modified_elements / deleted_elements` 三字段（`model_update.rs` 的 `Batch` 与 `task_queue.rs` 消费点、`model_update_api.rs` 若有独立解码一并），行内明细数据段画三数 + 分布条 + 总数句；分解缺席判据做成纯函数；`sim.rs` 夹具补字段各态一例 | V0 | `cargo test -p plant-ui -p plant-ui-app` 不降；新增测试：三数与总数一致例、老回执（字段缺席）只留总数例、真零例整区不画 |
| **V2 ✅ 已落地** | 模型段补「刚体前移 N 根」（数 `units[].kind == transform`，契约已有）；元数据门那半句**等 §3 的结构化字段**，落地前整格不画 | V1 | 渲染测试：有 / 无 transform 根两例 |
| **V3** | 房间段渲染（四形态 + 段头计数 + 房间行组 + 搬间明细 + 失败框两按钮的路由） | gen-model S8/T4 落地、§3 字段定形 | 四形态各一条渲染测试；字段缺席 → 整段不画（含行上四段退三段）；失败框按钮打到 `model/ensure` 与 execute 单库 `dbnums[]` |
| **V4** | 列表上限与「展开全部」（V-D4） | V3 | 上限测试：11 件搬间只列 10 + 尾行 |

顺序：V0 → V1 立刻可做；V2 随手；V3 / V4 跟 gen-model S8 / S9 走。

---

## 六、验收（实机）

1. 对一个有增删改的库跑完 Task：行内明细数据段读出 `+a ~m −d`，且 `a + m + d == changed_elements`；
   分布条三段比例与三数一致。
2. 连一台**没有分解字段**的老服务端：数据段只画「N 项变化」一句，画面上不出现 0 / 0 / 0。
3. gen-model S8 之后：房间段四形态各验一例——搬间明细与 `room_relate` 边的实际变化逐条一致；
   失败例中该库不判一致、行上水位段不亮、两个按钮各自打对端点。
4. 契约不动时界面不崩：`rooms` / 分解字段缺席一律整段（整组）不画，无 panic、无假数。

---

## 七、风险与不做

- **房间段今天没有契约**：V3 全部押在 gen-model S8/T4 的 Task 回执定形上；在那之前界面只有 V1 / V2
  可交付。§3 是消费者侧清单，不构成对 gen-model 的要求。
- **搬间明细可能巨大**（初始化 / 整库重建 `intent = reinitialize`）：V4 的上限规则先兜住；
  整库重建的任务房间段建议只给计数不给明细，随 §3 定形时一并裁。
- **不做**：房间独立面板（房间随 Task 收尾，不再是一条流水）；队列重试按钮（N-C 明文禁止）；
  解析 `warnings[]` 文本反拼数字（ADR-0019 先例）；预览侧任何改动（本计划只动终态明细）。
