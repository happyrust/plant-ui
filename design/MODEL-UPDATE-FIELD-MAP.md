# 模型更新画板 · 元素与契约字段对照

这张表是**实现的验收基准**：照画板做的时候，每一个数字、每一条状态文案，都必须在这里找到出处。
找不到出处的东西不许画上去，也不许实现出来（见 `DESIGN-SYSTEM.md` 校验清单）。

> **2026-08-07 SITE 视图重构（ADR-0017）**：预览与确认两步的验收基准移至新画板
> **S2-G（预览 · SITE 视图）** 与 **S2-H（确认执行 · SITE 视图）**，元素对照见第 2-G 节与
> 第 3 节的追记。旧 S2 / S2-B 画板保留作历史参照；本文其余各节的字段结论仍然有效。

> **2026-08-10 界面语言改说「保存」与时刻（ADR-0019）**：对人露出的一律是**保存**与**保存的
> 写入时刻**（`8 次保存待应用`、`08-01 09:12 → 08-07 14:10`），「会话 / sesno」退回代码、
> 契约与日志。**契约字段名一个都没改**——本表「出处」列照旧写 `sessions` / `applied_sesno` /
> `file_latest_sesno`，改的只是「示例」列那句给人看的话。时刻的尺子仍是 ADR-020 的
> **E3D 会话写入时刻**，与挂钟 `applied_at` 不得混用。
>
> 历史参照节（§2 S2、§3 的 S2-B 基准行、§4 S4 / S4-B、§5 S4-C）**示例文案一律保持
> ADR-0019 之前的原样**，与画板上那几张历史板对齐，照抄前先看清是哪一节。

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

## 0. 各画板共用

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 项目名 | `AMS-8009` | 契约 | `ManualUpdatePreview.project` / `ManualUpdateResult.project` |
| MDB 名 | `/ALL` | 契约 | `ManualUpdatePreview.mdb`——服务端回显它照哪个 MDB 解的范围。**请求要带 `mdb`**：范围由 MDB 定，两侧各有一份 `mdb_name` 配置，不回显的话错开是静默的 |
| 执行任务号 | `mu-3c81` | 契约 | execute 返回体的 `task_id` |
| 预览任务号 | `pv-7f31` | **待新增** | ADR-0006：预览改 202 + `task_id` |
| 步骤条两步 | 更新预览 / 确认执行 | 界面自有 | ADR-0011：第三步退役，确认后入队，执行进度看任务队列视图（S12）。三张 S4 系画板保留作历史参照（2026-08-05 步骤条已改） |

---

## 1. S2-A 正在预览

> **排期注（2026-07-27 拷问定案）**：本节的逐库行与分母全部依赖 ADR-0006（预览改
> 202 + `task_id`），而 ADR-0006 押后到任务队列迁移（`docs/plans/task-queue-rollout.md`）
> 完成之后。在那之前 S2-A 保持现状的诚实等待态：转圈 + 已用时 + 取消等待 + 上次预览摘要，
> 本节其余各行**先不实现**。

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 已用时 | `已用 01:12` | 本地 | 任务提交时刻起算 |
| 库计数与进度条 | `1 / 2 个 DESI 库` | **待新增** | ADR-0006 + ADR-0007：分子分母都取预览任务详情里的计数。**分母不取「已登记 DESI dbnum 总数」**——已登记里含文件缺失的库，它们不进扫描循环 |
| 行 · 已扫描 | `db8000 · DESI` | **待新增** | `PreviewDbFinished { dbnum, db_type }` |
| 行 · 扫描结果 | `8 次保存待应用 · 487 项变化` | **待新增** | `PreviewDbFinished { pending_sessions, changed_elements }`；终态可用 `DbnumPreview.sessions.len()` 与 `net_*` 之和复核。字段名照旧叫 `pending_sessions`，界面上说「保存」（ADR-0019）|
| 行 · 扫描中 | `db8003 · DESI` + 文件路径 | **待新增** | `PreviewDbStarted { dbnum, db_type, file_path }` |
| 行 · 单库已用时 | `扫描中 · 已用 22s` | 本地 | 从该库的 `PreviewDbStarted` 起算 |
| 行 · 非 DESI | `不需扫描` / `不在本期范围` | 契约 | 非 DESI 压根不进扫描循环，**不占分母** |
| 行序 | 按 dbnum 升序 | **待新增** | ADR-0007：现在扫描循环走 `IndexMap` 的 WalkDir 序、最终结果却按 dbnum 排过序，照事件顺序堆出来的列表会在预览完成那一瞬重排。后端在循环前排好，前端照事件 append 即可 |
| 上次预览摘要 | `1 个数据批次 · 23 个交付单元 · 1 个待重试单元` | 本地 | 缓存的上一份 `ManualUpdatePreview` |
| ~~600 秒说明~~ | | | ADR-0006 之后预览是 202 + `task_id`，没有一条挂十分钟的 HTTP 请求，也就没有这个上限要交代 |

> 「取消预览」只放弃客户端等待。服务端没有 cancel 接口，**不许**在文案上暗示它会停。

---

## 2. S2 预览结果

> **注（2026-08-07）**：本节对应的 S2 画板已随 SITE 视图重构退为历史参照，验收基准是
> 第 2-G 节 + S2-G 画板。下表的字段结论（三个大数、范围行、阻断卡、待重试卡等）仍然有效，
> 树行形态以 2-G 节为准。
>
> **注（2026-08-10）**：下表「示例」列里的 `sesno 1 024 → 1 031`、`812 < 已应用 1 005`
> 是 ADR-0019 之前的用语，与历史板 S2 一致，**保持原样不改**。现行说法见第 2-G 节。

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 三个大数 | `128 / 342 / 17` | 契约 | `Σ DbnumPreview.net_added / net_modified / net_deleted` |
| 分布条三段 | 85 / 228 / 11 px | 契约 | 上面三数的比例 |
| 受影响交付单元 | `23` | 契约 | `Σ DbnumPreview.units.len()`，`DeliveryUnitSummary` 已按 refno 去重 |
| 其中影响模型的变化 | `361` | 契约 | `Σ DbnumPreview.model_affecting` |
| 并发标注 | `1 个库正在应用，以上数字可能偏大` | 本地 | 队列快照里本项目 `state = running` 的行数（`task_queue::Vm::applying`）。合流删掉单飞预检后预览与数据批次可以并发，正在被应用的保存会被算进「待应用」（ADR-0011；gen-model ADR-011 §12）。为 0 时整行不画 |
| 范围行 | `3 个 DESI 设计库在范围内 · 1 个非 DESI 已排除` | 契约 | 三个理由分三个数，不许合成一个：`desi` 是当前 MDB 声明且项目里有文件的，`not_in_project` 是声明了够不着的，`excluded` 是非 DESI。计数为 0 的理由整格不画（画板场景本期无「声明但无文件」）；「够不着」的行形态见 S2-F。**MDB 之外的设计库连行都没有**——那正是「287 行」收成「29 行」的地方 |
| 树行库标识 | `db8000 · DESI` | 契约 | `dbnum` + `db_type` |
| 树行会话区间 | `sesno 1 024 → 1 031` | 契约 | `applied_sesno + 1` → `file_latest_sesno` |
| 树行 · 需初始化 | `ams-8009.stru · db8021 · DESI` + `需初始化 · 首次导入，整库生成` | 契约 | `initialization_required = true`：契约不为它解保存区间、`net_*` 全 0，但它确实会跑，绝不能显示成「无变化」（行形态见 S2-E，2026-08-05 已接进 S2 树） |
| 树行 · 阻断 | `ams-8009.desi2 · db8003 · DESI` + `文件回退 · 已阻断` | 契约 | `blocked = true` 的库在树里有行（`$danger-bg` 底），与右栏阻断卡一一对应；其余三种阻断行形态见 S2-E |
| ~~ZONE 分组行~~ | | | **已删**。ZONE 分桶全局退役，树从 dbnum → ZONE → 交付单元三层改为 dbnum → 交付单元两层；分组一律按 dbnum（`docs/adr/0011`）|
| 阻断卡 | `db8003 文件回退 812 < 已应用 1 005` | 契约 | `anomaly = Rollback { file_latest_sesno, applied_sesno }` 且 `blocked = true`；S2 只摆场景里真实出现的卡，四种阻断卡形态齐列于 S2-F（2026-08-05） |
| no_generation 卡 | `14 项变化找不到可生成的交付单元` | 契约 | `Σ DbnumPreview.no_generation` |
| 待重试卡 | `待重试单元：1 个将合并，1 个已放弃` | 契约 | `pending_model_retries[]` 按 `dead` 分两行（2026-08-05 已摘出）：死信不并入本次更新，唯一复活入口是队列面板的「立刻重试」 |
| 待重试单元标识 | `EQUI /P-1201B` | 契约 | `PendingModelUnit.noun` + `root_refno` |
| 已尝试次数 | `已尝试 2 次` | 契约 | `PendingModelUnit.attempts` |
| 主按钮批次数 | `开始更新 · 2 个批次` | 契约 | 可执行 dbnum 计数：非 `blocked`、非排除；需初始化的库计入 |

---

## 2-G. S2-G 预览 · SITE 视图（2026-08-07 起的验收基准）

树是 **SITE 平铺顶层 → 展开见详情行与交付单元** + 三段式（可执行 / 首次导入 / 本期不执行）。
「库分组头 → SITE 子行」中间方案在画板验收时被推翻（2026-08-07 二次定案）：**用户不需要
看文件，SITE 是树的唯一顶层语言**；库级事实（两个时间、待应用保存次数、同库联动）迁进 SITE
展开的详情行，同库多 SITE 时按 SITE 重复画——这是 SITE 平铺的代价，验收时明确接受。
执行边界仍然是（dbnum, 会话号区间）——SITE 只是选取入口，勾选集折算成 execute 请求的
`dbnums` 参数（gen-model ADR-020）；dbnum 不画在预览树上，只在确认页批次行的括号里
露面（与任务队列 S12 对账用）。

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| SITE 行 · 勾选框 | ☑ / ☐ | 本地 | 勾选集，默认全勾。**同库联动**（Q1 定案）：勾任何一个 SITE = 同库 SITE 全亮，只有勾/不勾两态、没有半选；折算成 execute 的 `dbnums[]`（ADR-020，2026-08-07 两侧已实现） |
| SITE 行 · 标识 | `SITE /1WCC-PIPE` | 契约 | ADR-020 `DbnumPreview.sites[]`（`SiteSummary.site_refno` / `name`，按**最近 SITE 祖先**分桶，ZoneSummary 同款做法） |
| SITE 行 · 增删改计数 | `+8 ~21 −2` | 契约 | `SiteSummary.added / modified / deleted`（净变化，配色 success / accent-strong / danger，等宽） |
| SITE 详情行（展开首行） | `上次应用 07-30 18:24 → 文件最新 08-07 14:10 · 8 次保存待应用 · 与 /1WCC-EQUI 同库联动` | 契约 + 本地 | 时间对 = ADR-020 `applied_sesno_time` / `file_latest_sesno_time`（**E3D 会话写入时刻**，Q3 定案同一把尺子，相减 = 文件里没被吸收的时间跨度；从未应用为空 → 板上作 `从未应用 → 文件最新 08-05 11:02`）；保存次数 = `sessions.len()`。**sesno 区间那一段已删**（ADR-0019）：区间 `applied_sesno + 1 → file_latest_sesno` 仍是执行边界，但只活在契约与日志里，详情行上不再露面。联动名单 = 同库其余 SITE 的名字（本地折算），同库只有 1 个 SITE 时整句不画。收起的 SITE 看不到这一行——时间是展开后的信息（原始诉求原文如此） |
| SITE 行 · 展开 → 交付单元行 | `BRAN /100-B1 几何修改` | 契约 | `SiteSummary.units[]`——`DeliveryUnitSummary` 原形态照旧（§2 表），只是挂在 SITE 桶下 |
| 归属未知桶 | `SITE 归属未知` | 契约 | `SiteSummary.site_refno` 为空的那一桶（解析不出 SITE 祖先的变化，ZoneSummary 同款）。本期画板场景没有，出现时行形态同 SITE 行、无勾选框、排在同库 SITE 行之后 |
| 首次导入库行 | `ams-8009.stru · db8021 · DESI` + `需初始化 · 整库生成` | 契约 | `initialization_required = true`：有勾选框（会执行、可排除），无 SITE 行可给（从未解析，PE 里没有它的数据）——**「不看文件」的唯一例外**，文件名是它仅有的身份，行上必须同时把「首次导入 · 整库生成」说清。`applied_sesno_time` 为空 → 「从未应用」。绝不能显示成「无变化」（S2-E 铁律不变） |
| 分段「本期不执行」 | 分隔行 | 界面自有 | 阻断行与够不着行归位到这一段，无勾选框；行形态照 S2-E / S2-F 不变（这两类也是文件级身份——没有 SITE 可说时才许露文件名），右栏阻断卡对应关系照旧 |
| ~~库分组头~~ | | | **已删**（2026-08-07 画板验收）：库级事实迁 SITE 详情行，文件名不再出现在可执行段 |
| ~~排除行~~ | | | **已删**（2026-08-07）：非 DESI 不占树行（与 S2-E 注记「排除不出行」终于一致） |
| 工具行 · 范围行 | `4 个 DESI 设计库在范围内` | 契约 | `desi` 计数照旧；**非 DESI（CATA 等）的数据完全不展示，连排除计数也不再交代**（2026-08-07 验收定案，收窄 §2 范围行的三理由规则）；「够不着」计数照旧，非 0 时才画 |
| 工具行 · 已勾选计数 | `已勾选 2 / 3 个 SITE · 折算 2 个数据批次` | 本地 | SITE 勾选数 / 有变化的 SITE 总数 + 折算出的 dbnum 批次数（含勾中的首次导入库）。两个数都给：人对着 SITE 做选择，机器按批次执行，桥在这里搭 |
| 主按钮批次数 | `开始更新 · 2 个批次` | 本地 | = 勾选集折算的批次数（不再是全部可执行库数） |

> 预览请求**不变**：预览永远全范围扫描（Q5 定案），勾选只影响 execute 的 `dbnums`。
> 跨库级联单元（`cascaded` 来源在别的库）仍挂在**触发批次**的分组下，实现时在行上标注
> 来源——SITE 桶是报告口径，不改变「选择的是批次」这件事。

---

## 3. S2-B 确认执行

> **注（2026-08-10）**：S2-B 是历史参照板，下表示例（`11 个会话`、`新会话自动并入`）
> 保持 ADR-0019 之前的原样。现行验收基准是下面的 **S2-H 追记** + S2-H 画板。

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 会执行 · 汇总 | `2 个数据批次 · 23 个交付单元` | 契约 | 同上批次计数 + `Σ units.len()`；需初始化的库产生批次但预览给不出它的单元数，23 不含它 |
| 会执行 · 库行 | `11 个会话 · 487 项变化` | 契约 | `sessions.len()` + `net_*` 之和 |
| 会执行 · 需初始化行 | `db8021 · DESI` `首次导入，还没有权威水位` `需初始化 · 整库生成` | 契约 | `initialization_required = true`（2026-08-05 已接进本卡）；整库生成的单元不计入 23，卡上单元汇总行要把这句说出来 |
| 不会执行 · 阻断 | `已阻断 · 水位不变` | 契约 | `blocked = true` + `anomaly` |
| 不会执行 · 排除 | `非 DESI，本期不处理` | 契约 | 排除发生在扫描之前，**与阻断不是一回事，界面上不许合成一行** |
| 不会执行 · 够不着 | `MDB 声明了它，当前项目目录里没有这个文件` | 契约 | `DbnumPreview.not_in_project`。与「文件缺失」隔开：缺失是登记过的文件不见了（阻断），这个从没登记过、多半在别的项目目录里 |
| 不会执行 · no_generation | `计入 no_generation 并告警` | 契约 | `Σ no_generation` |
| 会一并处理 | `1 个将合并 · 1 个已放弃` | 契约 | `pending_model_retries[]` 按 `dead` 分行（2026-08-05）：死信行不并入、不占 24 的分母，行内提示去队列面板手动重试 |
| 重扫提示 | 开始时重新扫描，新会话自动并入 | 契约 | 结果里的 `merged_sesnos` 就是这句话的兑现 |

### S2-H 追记（2026-08-07，SITE 视图重构后的验收基准）

S2-H 在 S2-B 形态上改五处（含删一行），其余逐格照旧。
**「不会执行 · 排除」行已删**——非 DESI（CATA 等）的数据在两张 SITE 视图画板上完全不展示
（验收定案），「N 项」计数不含它。2026-08-10 起本表按 ADR-0019 说保存与时刻，
并补记原先漏列的两格（**批次行保存窗口**、**不可取消警告**）——这两格画板上一直有：

| 元素 | 示例 | 来源 | 出处 |
|---|---|---|---|
| 会执行 · 批次行标题 | `SITE /1WCC-PIPE + /1WCC-EQUI` | 契约 | 批次行以**同库 SITE 名单**为题（`sites[].name`，ADR-020）——确认页也说 SITE 的语言，不说文件名 |
| 会执行 · 批次行摘要 | `同库一批（db8000）· 8 次保存 · 487 项变化` | 契约 | 「同库一批」交代联动折算；**dbnum 只在这个括号里露面**——任务队列（S12）按 dbnum 键，去那儿对账要认得出这一批是谁。保存次数 / 变化数照旧 `sessions.len()` + `net_*` 之和 |
| 会执行 · 批次行保存窗口 | `08-01 09:12 → 08-07 14:10` | 契约 | 窗口自身两端（ADR-0019 Q3）：左端 = `DbnumPreview.first_pending_sesno_time`（**2026-08-10 已落 gen-model**，取解析器定下的窗口左端而非 `applied + 1`），右端复用 ADR-020 的 `file_latest_sesno_time`。**注意左端不是 `applied_sesno_time`**——那个是「上次应用的是哪一条」，这个是「这批的第一条」。老服务端给不出左端时**整格不画**，只留「N 次保存」 |
| 不会执行 · 阻断 | `文件回退 · 最新保存 07-01 10:00 早于已应用 08-05 18:24` + `已阻断 · 水位不变` | 契约 + **待新增** | `blocked = true` + `anomaly`。两端时刻**已落 gen-model**（2026-08-10，ADR-0019 Q6）：挂在 `FileAnomaly::Rollback` 上，`file_latest_sesno_time`（文件端现读）+ `applied_sesno_time`（已应用端取水位表存量）。**这一格只读 `anomaly`**——阻断行的 `DbnumPreview.file_latest_sesno_time` / `applied_sesno_time` 服务端刻意留空，同一个值不摆两处。无存量时降级成 `早于已应用水位（应用时刻无记录）`，**不许拿挂钟 `applied_at` 兜底** |
| 不会执行 · **未勾选行** | `SITE /1WCC-HVAC` `08-06 15:02 → 08-07 09:44 · 3 次保存待应用` `未勾选 · 本次跳过，水位不变` | 本地 + 契约 | 勾选集之外的可执行 SITE（标题同样用 SITE 名），窗口时间对与批次行同一个来源。**与阻断、够不着不是一回事，三类不许合成一行**：这一类是人自己选的，出路就是回上一步勾上。计入「N 项」分子 |
| 重扫提示 · 范围限定 | `预览完成于 2 分钟前；开始时对勾选范围重新扫描，期间新产生的保存自动并入本批次，结果摘要里会逐条列出；未勾选的不扫描、不入队` | 契约 + 本地 | execute 的 `dbnums` 过滤（ADR-020）作用于重扫循环——未勾选的库连扫描都不进，预览后新存的保存不会被偷偷并入。「逐条列出」是 `merged_sesnos` 的兑现，逐条带时刻（ADR-0019 Q5）走 `merged_sesno_times`（**2026-08-10 已落 gen-model**，与 `merged_sesnos` 等长的平行数组，读不到的那条是 `None`）|
| 不可取消警告 | `数据批次一旦成功，水位立即推进且不回滚；模型生成失败的单元转入待重试，不影响已写入的数据。关闭窗口不会停任务。` | 契约 | 两阶段设计：批次 `Applied` 即推进水位，阶段二失败只落 `model_update_pending`。**「不回滚」这句不许省**——它是人按下确认前唯一能看到的不可逆声明 |

> 首次导入行（db8021）与阻断行（db8003）保持文件级身份——没有 SITE 可说的行才许露
> 文件名 / dbnum，口径与 §2-G 一致。

---

## 4. S4 执行进度 / S4-B 断线降级

> **注（2026-08-05）：S4 / S4-B / S4-C 已随 ADR-0011 退役**——向导只留预览与确认两步，
> 执行进度整体迁任务队列视图（验收基准 `QUEUE-FIELD-MAP.md`）。本节与第 5 节保留作
> 字段出处参照；三张画板上的 ZONE 列与行内重试按钮均已过时，勿再照抄。
>
> **注（2026-08-10）**：本节与第 5 节的示例（`sesno 1 024 → 1 034`、`并入的会话 …`）
> 同样是 ADR-0019 之前的用语，字段出处仍然有效，**给人看的话一律以 `QUEUE-FIELD-MAP.md`
> §1.5（S12-C）为准**。

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

- **偏小**：执行开始时会重新扫描，预览之后新产生的保存自动并入同一个数据批次（就是
  `merged_sesnos`），并入的保存带来新单元。
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
| 「重试」按钮 | | 契约 | **已实现**（2026-08-05），但落在队列面板的行内明细而非这张板上——第三步随 ADR-0011 退役之后，欠账单元的唯一去处就是那里。走的也不是 `model/ensure` 而是 `POST /api/v1/update/pending-units/retry`：那个端点只复活已存在的行（`attempts` 清零、`revision + 1`、清 `last_error`）再叫醒调度器，不排新批次；`model/ensure` 会绕开队列另起一条生成通道，而 `model_update_pending` 上还没有 claim/lease |
| 「模型树就地刷新」 | | 契约 | **已实现**：`ModelUnitResult.old_owner` / `new_owner` 经 `Outcome::refresh_units` 交给宿主的 `refresh_for_units`，只重查真正会变的分支 |
| no_generation 行 | `14 处` | 契约 | `ManualUpdateResult.warnings[]`，原文是「N 个变更无法解析合法生成根，跳过模型生成（样例: …）」 |

---

## 6. S2-D 不可用与失败态

服务端错误包封是 `{ code, message, detail }`（`web_service/mod.rs` 的 `ApiError`）。
**按 `code` 分型，不要解析 message 字符串，更不要把 anyhow 错误链摊给人看。**

| 形态 | 状态码 · code | 来源 | 出处 |
|---|---|---|---|
| 数据源未就绪 | 不发请求 | 本地 | `vm.data_source_ok = false`；`workbench/chrome.rs:106` 据此禁用入口按钮 |
| 范围不一致 | 422 · `identity_mismatch` | 契约 + 本地 | 服务端 `ServiceIdentity::validate`（显式 project / mdb / namespace 与服务不符，message 点名是哪一项）；本地闸门 `task_queue::Vm::can_mutate` 拦下时同码合成、不发请求。两处一个画面 |
| 已是最新 | 200 | 契约 | `ManualUpdatePreview.up_to_date = true` |
| 连不上 / 超时 | 504 · `timeout` | 契约 | `ApiError::timeout`，或客户端 600 秒上限到点 |
| 其余 | 500 · `internal` | 契约 | **只有这一类**才需要把原始 message 摊出来，且收进「详情」默认折起 |

> ~~「自动同步接管 · 422 · precondition」与「已有任务在跑 · 409 · conflict」~~ 随合流退役
> （gen-model ADR-011 §12：单飞预检、`sync_live` 拒绝与 `ProjectExecGuard` 四处守卫全部删除，
> 执行请求一律 202 入队；plant-ui ADR-0011 把 409 那一格改成入队回执「已入队，排在第 N 位」，
> 回执现在进日志并跳转任务队列视图）。这两个 code 若意外出现，没有专属画面，归 `internal`。

---

## 7. S2-E 设计库行形态表

`blocked` 不是随 `anomaly` 一起来的，是**算出来的**：`preview_dbnum` 里
`matches!(anomaly, Rollback | TypeChanged)`，项目扫描器再把 `Duplicate` / `Missing` 直接置
`blocked = true`。所以五种异常里 `PathMigrated` 独自落在「会执行」那一组。

| 行形态 | 关键字段 | 会否执行 | 界面必须说清的事 |
|---|---|---|---|
| 正常待更新 | `anomaly = None`、`applied_sesno < file_latest_sesno` | 会 | 保存窗口与成功后的水位落点，两者都说时刻：板上作 `08-01 09:12 → 08-07 14:10，8 次保存 · 487 项变化。成功后水位推进至 08-07 14:10。`（ADR-0019）|
| 需初始化 | `initialization_required = true` | **会** | 契约不为它解保存区间，`net_*` 全是 0；**绝不能显示成「无变化」** |
| 路径迁移 | `anomaly.kind = path_migrated`、`blocked = false` | 会 | 五种异常里唯一不阻断的；登记路径可自动跟新 |
| 文件回退 | `anomaly.kind = rollback` | 阻断 | 判据仍是 `file_latest_sesno < applied_sesno`，**说给人听的是两个时刻**：`文件里最新的保存（07-01 10:00）早于已应用的保存（08-05 18:24）`（ADR-0019 Q6，两端来源见 §3 S2-H「不会执行 · 阻断」）；水位只前进不后退 |
| 类型变化 | `anomaly.kind = type_changed` | 阻断 | 身份永不自动覆盖，必须人工确认 |
| 同号重复 | `anomaly.kind = duplicate` | 阻断 | 契约明写 block, do not pick —— 要把 `paths[]` 都列出来交给人挑 |
| 文件缺失 | `anomaly.kind = missing` | 阻断 | 出路二选一：补回文件，或注销这条登记 |

> 「排除」不在这张表里 —— 它压根不进扫描，与「阻断」来自契约的不同位置。排除现在有两个理由：
> 非 DESI，以及不在当前 MDB 声明的 DESI 名单里（后者连行都不出）。「够不着」（`not_in_project`）
> 是第三种不执行：MDB 声明了、当前项目目录里没有这个文件，有行但不阻断。

---

## 8. 契约里有、界面还没用的字段

反过来查的一遍。这些不是缺陷清单，是**下一轮画板的候选**，也是评审时该问的问题。

| 字段 | 情况 |
|---|---|
| ~~`DbnumPreview.initialization_required`~~ | **已接进**（2026-08-05）：S2 树行与 S2-B「本次会执行」卡都有 db8021 行，形态照 S2-E |
| `DbnumPreview.sessions[]`（`SessionPreview` 逐会话 added/modified/deleted） | 只用了 `len()`（界面上的「N 次保存」）。想做「按保存展开」得从这里取。**ADR-0019 明确不给它加逐条时刻**：Q5 的逐条时刻挂在 `DataBatchResult` 上，预览侧暂无消费者 |
| ~~`DbnumPreview` 第一条待应用保存的时刻~~ | **已进契约**（2026-08-10，`first_pending_sesno_time`）：窗口时间对的左端（ADR-0019 Q3）。此前契约只给得出 `applied_sesno_time`（上次应用）与 `file_latest_sesno_time`（文件最新），差的就是「这批的第一条」。界面侧待接 |
| ~~`DataBatchResult` 的窗口两端时刻与并入逐条时刻~~ | **已进契约**（2026-08-10，`start_sesno_time` / `end_sesno_time` / `merged_sesno_times`）：终态行内明细的窗口时间对（Q3）与并入逐条列出（Q5）。`merged_sesno_times` 与 `merged_sesnos` **等长**，读不到的那条是 `None`——按下标取，别按长度对齐。界面侧待接 |
| `DbnumState.applied_sesno_time`（水位表新增列） | **已进存储**（2026-08-10）：`applied_sesno` 那条保存的写入时刻，随水位同一条单调条件写入，永远指同一条保存。回退阻断卡的「已应用」端从它来（Q6）——文件被换回旧版本后那一页现读读不到了。界面不直接读这一列，读 `FileAnomaly::Rollback` 上转出来的那一份 |
| ~~`FileAnomaly::Rollback` 的两端时刻~~ | **已进契约**（2026-08-10，`file_latest_sesno_time` / `applied_sesno_time`）：Q6 那张阻断卡的材料齐了。服务端在裁决处一次补齐（预览 / 入队 / 执行三条路径同源），文件端现读一页、已应用端取水位表存量。`reason` 原文保持不变，**界面自己组句**。界面侧待接 |
| `DbnumPreview.file_name` / `file_path` | 只在 S2-A 的扫描行露过脸，预览结果页与终态页都没有 |
| `DeliveryUnitSummary.moved_in` / `moved_out` | 元素跨交付单元迁移的计数，没画。这恰恰是最容易让人困惑的一类变化 |
| `SitePreview.moved_in` / `moved_out`（ADR-020，2026-08-07 新增） | SITE 桶级的迁入迁出计数，没画——挪动的单元 pre/post 两侧各入一桶，SITE 行上现在只画 `+a ~m −d`。想标「挪进来的」得从这里取 |
| `DeliveryUnitSummary.name` | 只用了 `noun + root_refno`，人可读的名字没显示 |
| `DbnumPreview.zones[]`（含 `ZoneSummary.model_affecting`） | 曾经画过，`docs/adr/0011` 之后整组退役。契约字段保留，反序列化仍在守（`model_update_api` 的解码用例），但界面不再用它分组 |
| ~~`FileAnomaly` 五种只画了 `Rollback`~~ | **画齐且已接进**（2026-08-05）：行形态在 S2-E，阻断行已进 S2 树，四种阻断卡形态齐列于 **S2-F**（含 `duplicate` 的 `paths[]` 逐条列出）；S2 场景只摆真实出现的卡 |
| ~~`PendingModelUnit.last_error`~~ | **已画**（2026-08-05）：队列面板行内明细每条欠账单元下面摆一行原因。它是「为什么失败」唯一跨得过重启的来源——WS 明细活在进程内存里、重连即失，`Outcome.units[].message` 只活在 `/tasks` 的 200 条窗口内 |
| `PendingModelUnit.source_end_sesno` | 来源会话号只在 S2-B（历史板）「会一并处理」卡里露过脸，队列面板的欠账行没有。**ADR-0019 Q7 之后它不再直接露面**：改露 `source_end_sesno_time`（**2026-08-10 已落 gen-model**），界面说 `来源保存 08-05 18:24`。旧行、以及**不认领会话号的行**（房间任务、反向级联派生根，`source_end_sesno == 0`）都是 `None`，此时**来源段整个不摆**——不许回落成会话号 |
| ~~`PendingModelUnit.dead`（**新增字段**，2026-08-05）~~ | 服务端按 `attempts >= MAX_ATTEMPTS` 算出来随行带出。上限是服务端常量、不在契约里，客户端拿 `attempts` 判不出死没死。三处消费**已全部落位**：队列面板行内文案与「立刻重试」按钮、状态栏「N 个单元已放弃重试」（此前已画），S2 待重试卡与 S2-B「会一并处理」卡按 `dead` 分「将合并 / 已放弃」两行（2026-08-05 摘出） |
| `ManualUpdatePreview.warnings[]` | 预览期的非致命告警（读不了库头、遍历目录失败等）在 S2 上没有落脚点 |
| ~~`ManualUpdateStatus::Success` / `Failed`~~ | **已画**（2026-08-05）：三种终态的队列行内明细齐列于 **S12-C**（succeeded / failed 补齐，partial 对照参展）；`partial` 那张就是退役 S4-C 的行内化，重试与死信入口都在里面 |
