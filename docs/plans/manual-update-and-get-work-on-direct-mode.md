# 手动增量更新与取回工作 · 对接 gen-model direct 形态 · 开发计划

- 日期：2026-09-02
- 范围：`plant-ui-app`（`main.rs` / `data.rs` / `model_update_api.rs`）、`plant-ui`（`task_queue.rs` /
  `model_update.rs` / `vm.rs` / `workbench/chrome.rs`）、`plant-ui-data`（退役一个查询）。
  `plant-ui-view3d` **零改动**。gen-model 侧的三项前置（§四 G1–G3）单列，**不在本仓施工**。
- 依据：`CONTEXT.md`「手动增量更新 / 取回工作 / 水位 / 保存」；ADR-0009（显示时补齐模型走服务）、
  ADR-0016（eye 读实际渲染回执）、ADR-0019（界面说保存与时刻）、ADR-0021（取回工作清场重装已加载集）；
  gen-model ADR-053（direct 生成读）、ADR-054（生成时点 = 文件最新，凭证单调）、ADR-056（直写 RocksDB、
  e3d-model 选根、凭证前移、kv-mem 退役）、ADR-057（e3d-model 是纯函数层）、
  `docs/plans/2026-09-02-e3d-model-increment-direct-rocksdb-plan.md`、
  `docs/plans/2026-09-02-increment-update-audit-and-next-plan.md`（S8）、
  `docs/plans/2026-09-02-gen-model-increment-cleanup-inventory.md`（§6.3 预览 = 渲染计划）。
- 状态：**已定案**（2026-09-02 用户拍板：D1–D4 全按推荐项）。**M1、M2、M3a 已完工**（`plant-ui` 121 / `plant-ui-app` 87
  单测全绿，新增 12 条；ADR-0024、CHANGELOG、CONTEXT.md、两份 FIELD-MAP 已补）；M3b（等 G3）、M4（等 G4）、M5 未动。

---

## 一、要改什么（一句话）

让 plant-ui 在 gen-model 以 **direct 形态**（`AIOS_DATA_READ_MODE=direct`）运行时：
(1) 手动增量更新的入口**诚实**——服务不会消化批次时不让人白点、白等；
(2) 取回工作把**三维也拉到文件最新**（经 `ensure`，与 eye 同一条路），而不是重读上一次生成的产物；
(3) 界面能同时说出**两枚凭证**——数据水位（`applied_sesno`）与模型凭证（`gen_root.source_end_sesno`）。
预览 / 确认 / 入队 / 任务队列的形状**不动**。

## 二、现状与勘探事实

| 事实 | 位置 | 对本计划的影响 |
|---|---|---|
| gen-model direct 形态**不起 watcher、不起 worker**：`sync_live = configured && !direct`；`if !direct_read_mode { ensure_batch_worker }` | `gen-model/src/lib.rs:592,737` | `POST /update/execute` 入队后没人出队，队列行永远「排队中」 |
| `update_execute` 没有任何 direct 守卫：`enqueue_manual_update` + `arm_auto_work` 后直接 202 | `gen-model/src/web_service/handlers.rs:652–678` | 服务端不会替界面说「不行」 |
| `/health` 在 direct 下 `worker_alive: null`，另给 `data_read_mode: "direct"` | `handlers.rs:345,369` | plant-ui `Health` 只解 `worker_alive: Option<bool>`，**没有 `data_read_mode`**（`task_queue.rs:168–194`）；`None` 被当「老服务端不给」，不出横幅（`task_queue.rs:1200` 只对 `Some(false)` 画） |
| 预览仍由 gen-model 自家规划器算（old-pdms-io 净窗口 → SITE 汇总）；换成「渲染 e3d-model `UpdatePlan`」是清理清单 T409，**回执 JSON 形状不变** | `gen-model/src/data_interface/manual_update.rs:3495`；清理清单 §6.3 | plant-ui 向导零改；F8 已坐实老收集器幻删 / 漏增，今天的预览数字在两个真库窗口上是错的——这是 gen-model 的账 |
| P2-1 选根 / P2-2 凭证前移已落地在 `window_root_plan.rs`，但**未接进 `apply_one`**（影子模式） | gen-model spec 035 T201/T204、计划文档 P2-6 | 即便起了 worker，仍是旧规划器：全库根过期、无凭证前移、S8 照旧 |
| 取回工作的树走 direct（`tree_sites` / `tree_children` → `/api/v1/tree/*`） | `data.rs:34–57,392–409` | ✅ 已是文件最新 |
| 取回工作的三维：清场 → `Req::Models(快照 refno, true)` → `ModelLoad::Replace` → `plant_ui_data::model_instances`，**只读 `inst_relate`，不经 `ensure`** | `main.rs:2303–2329`；`data.rs:734–743` | direct 下没有数据批次替它先把模型算好；取到的是上一次生成的几何 |
| eye 那条路（`ModelLoad::Scopes`）是 `ensure_model(force=false)` → `model_instances_with_progress` | `data.rs:763–806` | 取回工作的三维该走的正是这条 |
| `ensure_model_scope_generated_from_roots`：凭证「当前」（`source_end_sesno >= 文件最新`，ADR-054）的根直接算命中，不重算 | `gen-model/src/data_interface/on_demand_model.rs:253–261` | P2-2 前移落地后，ensure 的代价 = 只重算被改到的根；落地前每次 SAVEWORK 后首次 ensure 等于整范围重生成（审核 S8） |
| `model_scopes: HashMap<RefU64, Vec<RefU64>>`「tree/container refno → 实际模型 refno」，eye 的范围回包时 `insert(target, refs)`，且每个模型也自映射一条 | `main.rs:956,1887–1900,1273` | 清场前「点过眼睛的范围目标」可以从这里拍下来，但它混着自映射条目——单独记一份更干净 |
| 清场快照 `model_reload_restore: Option<Vec<(RefU64, bool)>>` 只记第一次，重装成功才消费 | `main.rs:924–927,3829–3866,1765–1768` | 快照要多记一份范围目标；「只记第一次」规则不动 |
| 属性面板 `element_props` → `aios_core::get_ui_named_attmap` → SurrealDB `pe` / `ATT_*` | `plant-ui-data/src/lib.rs:553` | direct 下没有数据批次落库，属性停在上次解析；与树分叉 |
| 水位提示 `pending_sessions` 直读 SurrealDB `dbnum_watermark`，文案「N 个会话未应用」 | `plant-ui-data/src/lib.rs:393`；`chrome.rs:140` | 零解析库表为空；文案违反 ADR-0019（该说「保存」）；只说了数据那一枚凭证 |
| `/dbnums` 每行已带 `applied_sesno` / `file_latest_sesno` / `initialized`，plant-ui `DbnumStatus` 没解 | `gen-model manual_update.rs:2762–2770`；`task_queue.rs:210–231` | 数据那一枚凭证不需要 gen-model 改任何东西 |
| 队列轮询常驻：忙 1 s / 闲 5 s，`/dbnums` 在那四份里 | `main.rs:4120–4136`；`model_update_api.rs:206–234` | 提示行可以从轮询回包算，不必再让 UI 进程认识 gen-model 的表名 |
| `e3d.element.attributes`（`/api/v1/query`）只给 name / type / owner / position 四格 | `gen-model/src/query_service.rs:263–283` | 属性面板要直读得另开端点 |
| sim 剧本的 health 固定 `worker_alive: Some(true)` | `sim.rs:774` | M1 要补一档 direct 无 worker 的剧本 |

## 三、已拍板的决定（2026-09-02，全按推荐项）

| # | 题 | 定案 | 被否的备选 | 影响的里程碑 |
|---|---|---|---|---|
| D1 | direct 形态下手动执行怎么落地 | **gen-model 起 worker、不起 watcher**（G1）：`sync_live=false` 已保证不自动追新；worker 是清理清单里 gen-model 保留的「队列管理」 | `update_execute` 在 direct 下回 422 `code:"worker_disabled"`——不作主路，但 M1 仍给它留一格（G1 落地前的过渡期可能出现） | M1 两种都吃；G1 落地后 M1 的禁用态自动解除 |
| D2 | 取回工作的三维是否经 `ensure` | **是**：与 eye 同路（ADR-0009），时点口径同 ADR-054「点看即最新」。「不应用任何数据」精确成**不推进 `applied_sesno`**——ensure 只写模型面自己的状态（`gen_root` / `inst_relate`） | 维持重读 `inst_relate`：direct 下等于「取回工作对三维无效」直到手动更新跑完 | M2 |
| D3 | 属性面板数据源 | **gen-model 直读端点**（A）：取回工作的树 / 属性 / 三维同源；N5 的「新属性 + 旧模型」分叉只留给搜索 / 房间 / 材料表这些真正吃 `pe` 的消费者 | SurrealDB 不动，属性面板顶栏标「属性截至 08-05 18:24（数据水位）」（B） | M4 |
| D4 | M1 要不要等 G1 | **不等**：M1 不依赖 gen-model，先把「点了没反应」堵住 | — | 排期 |

## 四、gen-model 侧前置（不在本仓施工，只列 plant-ui 要什么）

| # | 事 | 锚点 | plant-ui 依赖它的里程碑 |
|---|---|---|---|
| G1 | direct 形态也启动数据批次 worker（不启动 watcher）；`/health.worker_alive` 在 direct 下回真值。备选：`update_execute` 回 422 `worker_disabled`——**✅ 2026-09-02 落地**（`run_cli` 无条件 `ensure_batch_worker`，direct 分支开门后 `scheduler.wake()`，`sync_live` 仍 `&& !direct`；两条源码钉；gen-model `changelog.md` 一条、ADR-053 追记；未 live 验证） | `lib.rs:737`、`handlers.rs:369,652` | M1 的禁用态解除；M5 验收 2 |
| G2 | P2-6 接线：`apply_one` 换 `build_model_update_plan_from_window`；首次出现的根带 `pe/dbnum/noun` 身份字段 | spec 035 T201/T204；`window_root_plan.rs` | 无硬依赖——接线前后界面行为一样，只差快慢与「凭证前移 K 根」那一行 |
| G3 | 回执补两件事实：`DbnumStatus.model_source_end_sesno: Option<i32>`（该库 `gen_root` 行的最小 `source_end_sesno`，无行为 `None`）；`DataBatchResult.credential_advance: { requested, advanced, degraded: Option<String> }` | `manual_update.rs::DbnumStatus`、`DataBatchResult`；`handlers.rs::dbnums` | M3 的模型那一枚凭证与队列明细那一行 |
| G4（可选，D3=A 时） | `GET /api/v1/element/attrs?refno=a/b&project&mdb&namespace` → `{ source:"direct", refno, noun, attrs:[{name, value}] }`，用 `direct_attmap::NamedAttrMap`（ADR-053 Q4 与写库侧同源）+ 字典默认值补齐，与 `get_ui_named_attmap` 同一套 UI 渲染规则 | 新 handler | M4 |

## 五、改动设计

### 5.1 M1 · 服务形态感知（D4：先做，不等 G1）

**契约**（`plant-ui/src/task_queue.rs`）

```rust
pub struct Health {
    // …既有字段不动…
    /// `"direct"` = gen-model 以 e3d-io 直读形态运行（ADR-053）：树与模型从库文件按需读，
    /// 旧增量 watcher/worker 默认不启动。老服务端不给这个键 → None。
    #[serde(default)]
    pub data_read_mode: Option<String>,
}
```

**判据**（`task_queue::Vm`）——`can_mutate` 管身份，新增一档管「服务会不会消化」：

```rust
/// 执行类写操作（开始更新 / 立刻扫一遍 / 立刻重试）此刻为什么不可用；None = 可用。
/// 暂停 / 恢复队列不受它管——那两个只改调度器旗标，不需要 worker。
pub fn execution_blocked_reason(&self) -> Option<&'static str> {
    let health = self.health.as_ref()?;
    match (health.data_read_mode.as_deref(), health.worker_alive) {
        (_, Some(false)) => Some("增量 worker 未存活：新批次不会被处理"),
        (Some("direct"), None) => Some("模型服务以 direct 形态运行，未启动数据批次 worker：预览可用，执行不可用"),
        _ => None,
    }
}
pub fn can_execute(&self) -> bool {
    self.can_mutate() && self.execution_blocked_reason().is_none()
}
```

**消费点**

- `task_queue.rs:1401`「立刻扫一遍」与行内「立刻重试」（`:2416`）改 `can_execute()`，`on_disabled_hover_text` 用 `execution_blocked_reason()`；`:1278 / :1384 / :1393` 暂停 / 恢复保持 `can_mutate()`。
- `task_queue.rs:1197` 的横幅扩成两档：`Some(false)` 照旧 `Status::Error`；`(direct, None)` 给 `Status::Warning`，文案就是上面那句 + 「入队的批次不会被处理」。
- `main.rs:4080 model_service_writable` 拆两个：`model_service_writable`（暂停 / 恢复用，判 `can_mutate`）与 `model_service_executable`（`Cmd::ExecuteModelUpdate` / `Cmd::ScanNow` / `Cmd::RetryPendingUnit` 用，判 `can_execute`，被拦时 warn 日志带 reason）。
- 向导 S2-H 主按钮「开始更新 · N 个批次」：`ModelUpdateVm::Ready` 且 `!can_execute()` 时禁用，按钮下方一行 micro 文案 = reason。预览按钮**不受影响**（预览不需要 worker）。
- `model_update::FailForm` 加 `WorkerDisabled`（`code == "worker_disabled"`），S2-D 加一格：文案同 reason，出路「等服务端切换形态或联系管理员」。给 G1 的备选路径预留；`code` 缺席时的状态码兜底规则不动。
- sim（`sim.rs:774`）：剧本加一档 `PLANT_UI_SIM_HEALTH=direct-no-worker`（或等价开关），`data_read_mode: Some("direct")`、`worker_alive: None`。

**测试**（`task_queue.rs` / `model_update.rs` 既有 `mod tests`）

- `direct_mode_without_a_worker_blocks_execution_but_not_pause`：`can_execute()==false`、`can_mutate()==true`、reason 命中 direct 那句。
- `old_servers_without_data_read_mode_stay_executable`：`data_read_mode: None, worker_alive: None` → 可执行（兼容）。
- `a_dead_worker_outranks_direct_mode_in_the_reason`：`Some(false)` 优先。
- `worker_disabled_is_its_own_fail_form`。

### 5.2 M2 · 取回工作的三维经 ensure 重装（D2）

**不变量**（ADR-0021 全部保留）：点击即清场；快照只记第一次、重装成功才消费；重查范围 = 快照里的模型 refno；
显示 / 隐藏按快照回放；相机不动；库里删掉的自然消失、新增元素只进树。**新增一条**：重查之前，对清场前
点过眼睛的范围目标逐个 `ensure_model(force=false)`，与 eye 同路；ensure 失败不阻断重查，但必须出声。

**状态**（`main.rs` App）

```rust
/// 清场前点过眼睛、且范围回包非空的树目标（`Evt::ModelScope Ok(非空)` 时登记）。
/// 它是取回工作重装前要 ensure 的名单——`model_scopes` 混着每个模型的自映射条目，不拿它当名单。
scope_targets: HashSet<RefU64>,

/// 清场快照：模型集 + 方向，外加范围目标名单。只记第一次（ADR-0021）。
struct ReloadSnapshot {
    models: Vec<(RefU64, bool)>,
    targets: Vec<RefU64>,
}
model_reload_restore: Option<ReloadSnapshot>,
```

- `scope_targets` 登记点：`main.rs:1900` `self.model_scopes.insert(target, refs)` 旁 `self.scope_targets.insert(target)`；
  清点：`clear_scene_for_reload`（拍完快照再清）、`reconnect`、`Evt::Models Ok`（重装落地后按回包重新登记：把快照里的
  targets 原样放回——重装后它们仍是「点过眼睛的范围」）。
- `reload_snapshot(loaded, pending_direction, visibility, scope_targets) -> ReloadSnapshot`：方向取值规则不变
  （`pending_direction` 优先于 `visibility`，都没有按可见）；`targets` 按树序（`tree` 的前序）排好，去重。
- `take_hidden_for_replay` 改吃 `ReloadSnapshot.models`，语义不变。

**请求**（`data.rs`）

```rust
Req::Models {
    roots: Vec<RefU64>,
    /// 重查之前要 ensure 的范围目标（清场快照里的 `targets`）。空表 = 跳过 ensure，行为与今天一致。
    ensure_targets: Vec<RefU64>,
    debt_reload: bool,
}
```

`ModelLoad::Replace { roots, ensure_targets, debt_reload }`（`data.rs:734–743`）改成三步：

1. 对 `ensure_targets` **顺序**调 `model_update_api::ensure_model(base, target, false, …)`（与 `ModelLoad::Scopes` 同一调用；
   sim 模式短路）。每个目标回一条 `Evt::ReloadEnsured { target, result }`：`Ok` 进日志汇总，`Err` 立刻
   `warn_of(target, "按需生成失败，该范围下的模型可能仍是旧几何：{err}")`；`code == "timeout"`（504）改说
   「服务端后台继续生成，完成后再点一次取回工作」。**不因 ensure 失败中止**——第 2 步照跑，空场景是比旧几何更坏的结局。
   `base / project / mdb / namespace` 随请求带下来（`Req::Models` 加这四个字段，与 `Req::ModelScopes` 同形）。
2. `plant_ui_data::model_instances_with_progress(&roots, …)`，进度经既有 `Evt::ModelScopeProgress` 同款事件回
   （新 `Evt::ReloadProgress { done, total }`，状态栏「正在重查模型 k/N」）。
3. `Evt::Models(debt_reload, result)` 原样。

**宿主**（`main.rs`）

- `Evt::GetWork Ok` 分支（`:2303–2329`）：`roots` 仍取快照 `models`；`ensure_targets` 取快照 `targets`；快照为空的短路不变。
- `clear_scene_for_reload`：状态栏 `Resolving("已清空三维，正在核对 {targets.len()} 个范围的模型是否最新…")`；
  日志加一句 `将先核对 {T} 个范围（点过眼睛的目标），再重查 {N} 个模型`。`targets` 为空而 `models` 非空时说
  `无范围目标记录，按上次生成的产物重装`（老快照 / 旧路径加载的模型）。
- `Evt::ReloadEnsured`：累计 `generated_root_count / cached_root_count / generation_root_count`，全部回来后一条汇总
  日志 `取回工作：核对 {T} 个范围 → 重算 {g} 个生成根、命中 {c} 个`（`Unknown` 状态计入「做过了」但点名）。
- `Evt::Models Ok`（`:1735–1797`）：回放、busy 收尾、相机规则**一个字不改**；只多一句把 `targets` 放回 `scope_targets`。
- `Evt::Models Err`：欠账恢复照旧；快照仍在（含 targets），补载那轮照样先 ensure 再查。

**为什么不用 `ModelLoad::Scopes` 直接重装**：那条路是 Append 语义 + 按目标方向统一显示，回放要按**模型**方向、
落地要按**整批**收尾（busy / 相机 / 快照消费）。把「先 ensure」加进 Replace 只改数据线程三步里的第一步，
宿主侧 Replace 的全部收尾逻辑照旧——改动面最小，ADR-0021 的测试全部保持绿。

**为什么不逐个模型 refno 打 ensure**：快照可上千 refno = 上千次 HTTP；范围目标是人真正点过的那几个树节点，
服务端在它下面一次归并全部生成根（`generation_roots_in_subtree`），凭证当前的直接算命中。

**边界**

- 范围目标是 SITE 且刚 SAVEWORK 过、P2-2 未落地：ensure 等于整 SITE 重生成，可能撞服务端 120 s 上限 → 504
  「后台继续跑」。日志按上面那句说；再点一次取回工作即拿到结果。这是 ADR-054 明写的切换代价，M5 记下数字。
- 目标已在 E3D 里删掉：ensure 报 refno 解不出 → `info_of` 「范围目标已不在库中」，不算失败。
- 与正在跑的数据批次撞 `db_generation_lock(dbnum)` → 服务端 409 `conflict` → 归 `Internal` 日志说明「该库正在被数据批次
  处理，稍后再点取回工作」。
- 回包里若出现快照集之外的模型（同根新增元素被一并生成）：`Req::Models(roots)` 按 refno 查，本来就查不到它们——
  「新增元素只进树、三维保持未加载」自然成立。

**测试**（`main.rs` 纯函数 `mod tests`）

- `reload_snapshot_records_scope_targets_in_tree_order_and_dedups`。
- `reload_snapshot_is_taken_once_including_targets`（第二次清场不覆盖 targets）。
- `an_empty_snapshot_still_skips_the_model_request`（既有用例扩到 targets）。
- `ensure_summary_counts_generated_cached_and_unknown_separately`（汇总日志纯函数）。
- `get_work_reports_removed_subtree_and_clears_its_ui_state`、`auto_refresh`、`restore_model_reload`、`begin_get_work`
  全部保持绿。
- `data.rs`：`model_load_uses_dedicated_lane` 按新 `Req::Models` 形状改；新增 `replace_without_targets_skips_ensure`
  （对 `ModelLoad::Replace` 的路由与字段断言）。

**文档**：ADR-0024「取回工作的三维经 ensure 重装」（风格照 ADR-0021）：一句话定义 + 「不应用任何数据 = 不推进
`applied_sesno`」的口径 + 与 ADR-0009 同路 + S8 代价声明。`CHANGELOG.md` 未发布节「取回工作」小节。
`CONTEXT.md`「取回工作」词条追加一句：「三维经模型服务按需补齐到文件最新，不推进数据水位」。

### 5.3 M3 · 两枚凭证进界面

**数据那一枚（不依赖 gen-model）**

- `task_queue::DbnumStatus` 加 `applied_sesno: i32`、`file_latest_sesno: i32`、`initialized: bool`（`serde(default)`），
  服务端早就给（`manual_update.rs:2762–2770`）。
- `task_queue::Vm` 加纯函数 `pending_saves(&self) -> PendingSaves { saves: u32, uninitialized: usize }`：
  本项目 DESI、`!excluded && !blocked && !not_in_project`；`applied_sesno > 0` 的库累计 `max(file_latest − applied, 0)`，
  `applied_sesno == 0` 的库计入 `uninitialized`（「需初始化」不许显示成「无变化」）。
- `vm.pending_sessions: Option<u32>` → `vm.pending_saves: Option<PendingSaves>`，在 `Evt::QueuePoll Ok` 之后由 `queue.pending_saves()`
  赋值；`Req::PendingSessions` / `Evt::PendingSessions` / `plant_ui_data::pending_sessions` / `WATERMARK_TABLE` **退役**
  （`main.rs:2287` 那次发送一并删）。
- `chrome.rs:140` 文案按 ADR-0019 改口：`设计库还有 {saves} 次保存未应用` + （`uninitialized > 0` 时）`· {n} 个库需初始化`
  + `· 去「模型更新」`。

**模型那一枚（依赖 G3）**

- `DbnumStatus.model_source_end_sesno: Option<i32>`（`serde(default)`）。
- `pending_saves` 加 `model_lagging: usize` = `model_source_end_sesno.is_some_and(|m| m < file_latest_sesno)` 的库数；
  提示行第二句 `· 模型：{n} 个库落后于文件`；`None`（服务端没给 / 无 `gen_root` 行）整句不画。
- 队列终态行内明细（S12-C）：`DataBatchResult.credential_advance: Option<CredentialAdvance>`（`serde(default)`），有则加一行
  `凭证前移 {advanced}/{requested} 根`，`degraded` 非空改说 `凭证前移退化：{reason}（全部根按过期处理）`。
- `MODEL-UPDATE-FIELD-MAP.md` §8 与 `QUEUE-FIELD-MAP.md` 各补一行出处；老服务端不给的字段整格不画（既有纪律）。

**测试**：`pending_saves_counts_only_in_scope_desi_and_splits_uninitialized`、`model_lag_is_omitted_when_the_server_is_silent`、
`credential_advance_row_decodes_and_defaults`。

### 5.4 M4 · 属性面板直读（D3=A；依赖 G4）

- `data.rs` 已有 direct 分流（`direct_tree_enabled()`，`data.rs:24–33`）；同一开关下 `Req::Props` 改调
  `model_update_api::element_attrs(base, refno)`（新函数，`require_direct(source)` 同款守卫），回包映射成既有 `plant_ui_data::Attr`。
- `Attr.kind`：属性编辑今天没有写回路径，直读版先一律 `Opaque`（只读）；`plant_ui_data::attr_kind` 的可编辑分类留给写回落地时搬到服务端。
- `invalidate_all()` 不变（direct 读无本地缓存）。
- D3=B 时改做：属性面板顶栏一行 `属性截至 {applied_sesno_time}（数据水位）`，`applied_sesno_time` 需 gen-model 在 `/dbnums` 露出
  （存储里已有，ADR-0019 Q6）。

## 六、施工顺序与测试

| 步 | 内容 | 依赖 | 测试 / 产物 |
|---|---|---|---|
| M1 | 服务形态感知（§5.1）——**✅ 2026-09-02 完工** | 无 | 6 条单测（`task_queue` 4 + `model_update` 2）；sim 新剧本 `PLANT_UI_SIM_SERVICE=direct-no-worker`；`main.rs` 拆出 `model_service_executable` |
| M2 | 取回工作三维经 ensure（§5.2）——**✅ 2026-09-02 完工** | D2 拍板 | 3 条新单测（`reload_snapshot_records_scope_targets_sorted_and_deduped` / `ensure_tally_counts_generated_cached_and_failed_separately` / `a_reload_carries_its_ensure_targets_and_identity_to_the_model_lane`）+ 两条既有随签名改；ADR-0024；CHANGELOG；CONTEXT.md 一句。落地形态与 §5.2 的差别：`ReloadEnsureTally` 做流水账（代替「累计三个数」的散字段）；目标名单按 refno 排序而非树序（幂等，工作量相同）；没做 `data.rs` 的源码钉测试（时序留 M5 实测） |
| M3a | 数据凭证提示 + ADR-0019 改口 + 退役 `pending_sessions`（§5.3 上半）——**✅ 2026-09-02 完工** | 无 | 3 条单测（`pending_saves_counts_only_in_scope_desi_and_splits_uninitialized` / `dbnum_status_decodes_the_watermark_pair` / `the_pending_saves_hint_speaks_saves_and_keeps_uninitialized_apart`）；`Req/Evt::PendingSessions`、`plant_ui_data::pending_sessions`、`WATERMARK_TABLE` 已删；QUEUE-FIELD-MAP §4 补行；CHANGELOG 一条 |
| M3b | 模型凭证提示 + 凭证前移明细（§5.3 下半） | G3 | 1 条单测 |
| M4 | 属性直读（§5.4） | D3、G4 | `element_attrs` 解码用例 |
| M5 | 实测验收（见下） | G1 至少备选落地 | `docs/2026-08-12_live-test-ledger.md` 同款记录 |

M5 验收清单（AvevaMarineSample，gen-model 以 direct 形态起）：

1. **G1 未落地**：向导预览可用；「开始更新」「立刻扫一遍」「立刻重试」灰，横幅与按钮下文案都说 direct 无 worker；
   暂停 / 恢复照常可点；`/health.data_read_mode == "direct"`。
2. **G1 落地后**：同一界面不重启自动变可用；执行一次 → 队列行走到终态 → 自动刷新只换树 + 日志提示；
   `AutoRefresh::FullReload` 那次的 ensure 全部命中（受影响根刚 eager 过、其余已前移或本来就当前）。
3. **取回工作**：显示两个 ZONE、隐藏其中一根管、记相机；E3D 改一个 BOX 并 SAVEWORK；**不跑手动更新**，点取回工作：
   点击当帧清场、eye 全回未加载；数秒到数十秒内三维回来且 BOX 是新的；隐藏的管仍隐藏；相机没动；
   新建元素只在树上；日志 `核对 2 个范围 → 重算 g 个生成根、命中 c 个`。G2 之前 `g` 会是该 ZONE 全部根（S8），把数字记成 before。
4. **ensure 超时**：对整个 SITE 点眼睛后 SAVEWORK 再取回 → 504 → 日志「后台继续生成」，场景以当前产物落地；
   稍后再点一次拿到新几何。
5. **提示行**：`设计库还有 N 次保存未应用 · M 个库需初始化 · 去「模型更新」`；G3 后追加 `模型：K 个库落后于文件`。
6. **老服务端**（无 `data_read_mode`）：M1 全部不出声，行为与今天一致。

## 七、风险与边界

- **S8 代价**（G2 落地前）：SAVEWORK 之后第一次取回工作 = 范围内全部根重算。缓解：范围目标顺序 ensure、日志逐条报数、
  504 时说清「后台继续」；这是切换成本不是回归，M5 记数字，G2 落地后同一场景应降到只重算被改的根。
- **`scope_targets` 的准确性**：人点过眼睛后又被 `Unload`（重新生成路径、树上消失的节点）的目标仍在名单里——ensure 多做一次
  无害（幂等、命中即返）；重查范围仍以快照模型集为准，不会把未加载的东西拉进来。
- **ensure 与数据批次并发**：G1 起 worker 后，取回工作可能撞上正在跑的 dbnum（409）。日志说明即可；ADR-056 R7 明写 409 面积只会缩小。
- **老服务端兼容**：M1 / M3 所有新字段 `serde(default)`；`data_read_mode: None` 视同 legacy，行为与今天逐字节相同。
- **「不应用任何数据」的口径**：ADR-0024 必须把「ensure 只写模型面状态、不推进 `applied_sesno`」写成一句话，否则 CONTEXT.md
  的定义与实现又会被人读成矛盾。
- **属性直读的分叉**（M4）：搜索 / 房间 / 材料表继续吃 SurrealDB `pe`（数据水位那一版），属性面板改吃文件最新——这是 ADR-056 N5
  承认的两枚凭证在界面上的体现，不是缺陷；提示行（M3）就是解释它的地方。
- **不做**：预览换源（gen-model T409）、CATA→DESI 反查、单元级执行——都是 gen-model 的账；plant-ui 只要契约形状不变就不动。
