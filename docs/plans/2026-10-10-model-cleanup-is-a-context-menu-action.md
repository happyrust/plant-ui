# 「清理模型」：范围清理在右键、整库清理在库行 · 只删产物、不重算 · 开发计划

> 日期：2026-10-10（第 2 稿：plannotator 第 1 稿 approved 后，用户补「整库清理也要」，加 §1C / E 组决策 / G3 / C5）
> 状态：**计划，未实施。** 本文与 gen-model `docs/plans/2026-10-10-regeneration-leaves-no-stale-artifacts-dev-plan.md`（S0 已提交 `a63fa2f57`，S1 在工作树）同一条线：那份管「重新生成 = 先删后生成」，这份管「只删不生成」的两个手动入口。
> 起因：用户要「一个模型清理的功能，在重新生成模型下方，方便特殊情况下使用」；随后补「整库清理也要」。
> 术语（拟进 `CONTEXT.md`「模型显示与生成」）：**清理模型 (Model Cleanup)** = 把**已生成的产物行**删掉、三维里对应的模型卸载，不重算、不删共享单位网格。两个口径：**范围清理**（右键，一个节点子树，§4.5.1）与**整库清理**（库行，一个 dbnum 的全部模型面 + 模型水位归零）。_Avoid_：删除模型（会被理解成删设计元素）、深度删除（d-8 已否决）、清库（那是数据面的 `DELETE /dbnums/{n}/data`）。
> 涉及：plant-ui `crates/plant-ui/src/{lib,workbench/tree,model_regenerate,task_queue}.rs`、`crates/plant-ui-app/src/{main,data,model_update_api}.rs`、`CONTEXT.md`、`docs/adr/0030-…`、`CHANGELOG.md`；gen-model `src/data_interface/{on_demand_model,fast_delete}.rs`、`src/web_service/{handlers,mod}.rs`、`docs/specs/web-service-api.md`、`changelog.md`。
> 约束：工作树多会话共用，只 `git add <明确路径>`；gen-model 工作树里 S1 的 5 个未提交文件（`e3d_mesh_store` / `e3d_model_service` / `historical_model` / `direct_model_persist` / `CONTEXT.md`）一字不碰，本计划的 gen-model 改动与它们不重叠（`changelog.md` 除外，各加各的条目）；起后端由用户来，实施方只跑有尽头的检查。

## 0. 一句话

**范围清理**：右键菜单「重新生成模型」正下方加「清理模型」：确认框 → 对选中的每个目标各一发 `DELETE /api/v1/model/subtree?refno=X&confirm=X` → 成功的目标把三维里它名下已加载的模型卸掉、树行回「未加载」、日志一句「需要时再右键重新生成」。服务端把这条 DELETE 接到与 `force=true` 清理**同一个函数**上，两种形态的锁与路由行为一致。

**整库清理**：任务队列库行「操作」列在「重算模型（整库）」旁加「清理模型（整库）」：确认框 → 一发新端点 `DELETE /api/v1/dbnums/{dbnum}/model?confirm={dbnum}` → 服务端删该库全部模型面行、清进程内投影与账本、**模型水位归零**（与清库 / 回退重建的模型半边同一句）→ 客户端卸载该库已加载的全部模型、日志提示「点「重算模型（整库）」重新生成」。

## 1. 事实（2026-10-10 核过源码）

### plant-ui（`release-20261010\plant-ui` master @ `a1af3c30e`，工作树干净）

| 事实 | 位置 | 对本计划的影响 |
|---|---|---|
| 右键菜单 `element_menu` 由模型树与三维视口共用；「重新生成模型」自成一组（`live` 门禁、`regen_busy` 禁用、禁用悬停「已经有一趟重新生成在跑了」），按下推 `Cmd::RegenerateModels { targets }` | `crates/plant-ui/src/workbench/tree.rs:370–388`；`workbench/view3d.rs:203` | 新项放同一组、同一门禁；两处入口自动都有 |
| 确认框是独立模块 `model_regenerate`：`Vm::{Confirm, Running, Failed}`，`Running` 无叉无按钮（服务端停不下来），文案钉子 `the_confirm_prompt_names_the_replacement_and_carries_no_count` 等 3 条 | `crates/plant-ui/src/model_regenerate.rs` | D2：泛化成一个「模型库变更」确认框，两种动作各一份文案 |
| 宿主流程：`begin_regenerate`（开框、`regenerate_run` 记目标、帧号 +1）→ `settle_regenerate_confirm`（发 `Req::GenerateModels`，`regen_busy = true`）→ `settle_models_generated`（成功的目标 `model_scopes.remove` + `regenerate_refresh.insert` 再 `set_model_visible(true)`） | `crates/plant-ui-app/src/main.rs:3977`、`:3992`、`:4042` | 清理照同一骨架，收尾那一步换成卸载 |
| 数据线程对一批目标**顺序**各一发 `generate_model(force = true)`，整批回 `Evt::ModelsGenerated { epoch, outcomes }` | `crates/plant-ui-app/src/data.rs:152`、`:344`、`:700–725` | 清理同形：`Req::CleanupModels` / `Evt::ModelsCleaned` |
| `model_update_api` 只有 `get` / `post` 两个请求助手（ehttp），没有 DELETE；`urlencode` 在；错误分型 `failure_of` / `endpoint_missing` 在 | `model_update_api.rs:902`、`:913`、`:890`、`:563` | 加一个 `delete` 助手（`ehttp::Request` 改 `method = "DELETE"`，无 body） |
| 已加载的模型按「范围 → 成员」记在 `model_scopes: HashMap<RefU64, Vec<RefU64>>`，叶子集合在 `loaded_models`；`ModelAction::Unload { refnos }` 卸载指定叶子并释放渲染状态（取回工作里对消失的节点就这么用） | `main.rs:1257`、`:1263`、`:3019`；`crates/plant-ui/src/lib.rs:36`；`plant-ui-view3d/src/lib.rs:785` | D3：清理成功后用它卸载目标名下已加载的叶子 |
| 守卫 `regeneration_is_not_a_scene_action`：生成不是 `ModelAction` | `crates/plant-ui/src/lib.rs:382` | 清理同样走 `Cmd`，守卫扩一条 |
| `regen_busy` 还挡「取回工作」按钮 | `workbench/chrome.rs:137–145` | 共用同一个忙碌位时清理期间取回工作也灰，可接受（几秒） |

### gen-model（`release-20261010\gen-model` `gen-model-refactor` @ `a63fa2f57`）

| 事实 | 位置 | 对本计划的影响 |
|---|---|---|
| `DELETE /api/v1/model/subtree?refno&confirm` 已有：`confirm` 必须等于 `refno`，否则 400；删「传入 refno 自身及全部 PE 后代」的模型关系、无剩余引用的实例信息、直管边、空间树条目、`room_relate`；**不删** PE、`inst_geo`、`.mesh`；幂等；回 `{requested_refno, scope:"exact_subtree", status:"deleted"}` | `docs/specs/web-service-api.md:390–402`；`handlers.rs:2403–2441` | 客户端直接用，不新增接口 |
| handler 直接调 `mgr.delete_model_subtree(refno)`，**没有** `model_generate` 那套 `dbnum_for_roots → route_for → scope_memory_only` 路由，也没有 120 s 预算包装 | `handlers.rs:2412–2441` 对比 `:2514–2531`、`:2545`、`:2566–2571` | D4：Memory 路由的库（初始化未落 / 读透）走不进「只写内存」作用域 |
| `delete_model_subtree`：先过初始化门 `require_model_generation()`；读透形态直接 `delete_projected_subtree(.., true)`，**不先对范围内生成根 try-lock**（S0 只给 `clear_scope_for_regeneration` 补了这一步）；摄入形态在 SurrealDB 解根、逐根 `try_generation_root`（被占 409）再删 | `on_demand_model.rs:217–276`；对照 `:80–106` | D4：spec 说的「任一覆盖根正在生成回 409」在读透形态不成立，与 d-6 当时查出的出入同款 |
| `clear_scope_for_regeneration(requested_refno, roots)`（私有）：只写内存 / 读透 → 对 roots try-lock 后 `delete_projected_subtree(requested, read_through)`；摄入 → `delete_model_subtree`。三路同一个窗口、同一套锁 | `on_demand_model.rs:80–106` | D4：把它变成两条入口共用的那一份实现 |
| handler 源码钉 `model_subtree_delete_requires_exact_confirmation_and_preserves_requested_scope` 钉着 `.delete_model_subtree(refno)` | `handlers.rs:4436–4451` | D4 改 handler 时同步改钉 |
| `/model/generate` 对容器（ZONE 等）是「范围」语义：解出子树里全部生成根逐根生成；DELETE 本来就是 exact-subtree 语义 | `handlers.rs:2557–2563`；spec §4.5.1 | 清理对 ZONE / SITE 同样可用，范围口径与右键重新生成的「先删」一致 |
| 共享决策 d-8 / d-10：重新生成不删共享单位网格（`inst_geo` / `.mesh`），否决「深度删除」 | gen-model `CONTEXT.md`「单位网格身份」；计划 Q2 / Q7 | 清理口径 = 产物行；本计划不开「深度」选项 |

### 1C. 整库清理的材料（2026-10-10 补）

| 事实 | 位置 | 对本计划的影响 |
|---|---|---|
| **没有**「只清一个库的模型面」的端点。同族现有三条：`DELETE /dbnums/{n}/data?confirm=n`（整库快删，含数据面，要求队列先暂停、该库无排队 / 运行批次：`ensure_dbnum_mutation_idle`）、`POST /dbnums/{n}/data/reinitialize`（重新解析，worker 冻结点 `wipe_dbnum_for_reinit`）、`POST /dbnums/{n}/model/rebuild`（整库重算，不删旧模型） | `web_service/mod.rs:592–663`；`handlers.rs:3413–3463`、`:2765–2800`、`:2899–2941` | G3 新端点，形状照 `DELETE /dbnums/{n}/data`（`confirm` 必须等于路径 dbnum） |
| 模型面在库里的表：按 `model_sesno` 谓词删的三张 `MODEL_FACE_TABLES = inst_relate / inst_info / tubi_relate`（回退用）；按 Ref0 区间删的 `RANGE_TABLES` 里模型相关的是 `inst_relate / tubi_relate / room_relate / room_panel_relate / geo_relate / inst_info / aabb / trans`（`aabb:<mesh id>` 共享本地盒是十六进制、落不进 `<ref0>_` 区间，天然不删；`pe` / `ref_rev` / noun 表是数据面） | `fast_delete.rs:13–43`、`:266`、`:600–655` | G3 删的是 `RANGE_TABLES` 里的模型子集（按 Ref0 区间，一条都不漏、不碰数据面）；房间边按 `in.dbnum` 谓词删（边上没有 dbnum 列，`:342–349`） |
| 清库 / 回退重建的「模型半边」已有一句定稿（ADR-060 D8）：`model_sesno = 0, model_sesno_time = NONE, model_settled_at = NONE, model_lazy_roots = NONE, model_dead_roots = NONE, model_roots_observed_at = NONE`，并递增 spatial epoch；进程内 `ModelMemoryStore::clear_dbnum`、`model_root_ledger::forget_dbnum`、`mark_aabb_tree_dirty`、`DbnumState::invalidate_mirrored` | `fast_delete.rs:633–652`、`:724–765`、`:494–499`；`model_memory_store.rs:1337` | G3 整库清理 = 这一句的模型半边 + 模型表区间删，**一份渲染两处用**（抽成函数给 `render_delete_phases` 与新端点共用） |
| 路由判据：`model_sesno < applied_sesno` → `Memory`（`model_lagging`）：`/model/records` 改读进程内投影，按需生成改写投影（`ProjectionOnly`，不落库），直到一条整库 ModelOnly 任务把模型水位推回 | `model_read_route.rs:172–206`、`:129–161` | **水位归零的后果**：清理之后该库翻面到内存，右键重新生成的结果只在进程里，要「重算模型（整库）」才落库。这是 E2 的核心取舍 |
| 整库重算 = 排一条起点 0 的 `ModelOnly` 任务（「从没追平过」那一档整库枚举根）；**启动时**模型水位落后会自动排同样的恢复任务（`startup_reconcile`）；运行期扫描不自动排 | `model_rebuild.rs:69–138`；`batch_worker.rs:2777–2782`；`startup_reconcile.rs:408–449` | 清理 + 水位归零之后：手动点重算就回来；不点，下次服务重启也会自动整库重算 |
| 并发闸门：`DELETE /dbnums/{n}/data` 要求队列暂停 + 该库无批次；整库重算在飞时同库按需生成回 409（`model_rebuild::active_for_dbnum`）；Memory 路由同库有整库任务时单根 generate 回 409（`TaskRegistry::active_dbnum_model_ensure`） | `handlers.rs:3413–3431`、`:2532–2540`；`model_rebuild.rs:36–66` | E4：整库清理的闸门 = 该库无排队 / 运行批次、无在飞整库任务、拿到该库生成锁；**不要求队列暂停**（它不碰数据面） |
| 读透形态的持久面是 mirror spool；现有墓碑只有按根的 `render_spooled_scope_deletion(root, affected, tube_branches)`，没有按库的 | `model_db_adapter.rs:2472`；`on_demand_model.rs:147–197` | G3 读透分支：对投影里该库的每个根各渲一条墓碑（实施时核量；根数以千计时再考虑按库墓碑） |
| plant-ui 库行「操作」列已有两枚：「立即执行」与「重算模型（整库）」（`Btn` 空标签 + `ARROWS_CLOCKWISE`，判据 `task_queue::model_rebuild` 五档，按下**不开确认框**直接 `Cmd::RebuildDbnumModel`） | `task_queue.rs:588–662`、`:3082–3134`；`lib.rs:293`；`main.rs:3685`；`data.rs:776`；`model_update_api.rs:144` | C5 第三枚按钮同一列、同一套判据材料；破坏性，所以**开确认框**（复用 D2 的那一个） |
| `/dbnums` 行有 `model_source` / 判决（`InSync` / `Lagging`）的字形，模型落后用提醒色 | `task_queue.rs:3136–3154` | 清理后库行自然显示「落后 + 内存」，不用新状态 |

## 2. 完成定义

1. 模型树与三维视口右键菜单里，「重新生成模型」正下方有「清理模型{ N 项}」，同一组、同一门禁；确认框说清后果；按下后目标名下已生成的产物在服务端被删、三维里对应模型卸载、树行回「未加载」、日志一句收尾。
2. 失败按目标点名：409（范围内有根正在生成）、超时（服务端可能已删一半，提示点眼睛重查）、其他错误原文；成功与失败混在一批里各归各。
3. 服务端 `DELETE /model/subtree` 与 `force=true` 的清理走同一份实现：Memory 路由的库进「只写内存」作用域清投影，读透形态先对范围内生成根 try-lock、被占回 409（spec §4.5.1 的承诺在三种形态都成立）。
4. 不删共享单位网格、不碰 PE；取回工作 / 显示路径一个请求都不多发。
5. **整库清理**：库行按钮 → 确认框 → `DELETE /dbnums/{n}/model?confirm=n` → 该库模型面行一行不剩（八张模型表按 Ref0 区间 + 房间两边）、进程内投影与账本清空、模型水位归零、spatial epoch 递增；客户端卸载该库全部已加载模型、库行显示「落后」；该库有批次 / 整库任务 / 生成在飞时 409、一行不删。清理**不自动重算**；日志指路「重算模型（整库）」。

## 3. 决策（★ = 推荐，批注时请重点看）

| # | 题 | 选项 | 推荐 |
|---|---|---|---|
| D1 | 菜单项文案 / 图标 / 门禁 | (a) ★「清理模型{suffix}」，图标 `ph::BROOM`（版本里没有就 `ph::ERASER`），与「重新生成模型」同组紧挨其下，同 `live` 门禁，共用 `regen_busy`（禁用悬停改成「已经有一趟重新生成 / 清理在跑了」）；(b) 另起 `cleanup_busy` 两项互不相挡 | (a)。两件事都改模型库，服务端对重叠的根本来就回 409，没必要让人先点出一个 409 来 |
| D2 | 确认框 | (a) ★ 把 `model_regenerate` 泛化成 `model_mutation`（或原名不动，`Vm` 加 `kind: Kind::{Regenerate, Cleanup}`），文案按 kind 查表，`Running` 无叉无按钮的纪律两种共用，现有 3 条钉子保留并各加清理那一份；(b) 复制一份 `model_cleanup.rs` | (a)。宪法 II：一个「不可回头的模型库变更」确认框只有一份实现 |
| D3 | 清理成功后三维怎么办 | (a) ★ 本地卸载：对每个成功目标，取 `model_scopes` 中键等于它或在它子树下的全部成员 + `loaded_models` 里祖先链含它的叶子，合并后发 `ModelAction::Unload`，移除这些 `model_scopes` 键、`loaded_models` 条目、`regenerate_refresh` 条目；树行 eye 回「未加载」（`pending_visibility` / `visibility_unavailable` 同步清）；**不**发 records 重查；(b) 清理后以目标为祖先重查 records（结果必空）再按空处理 | (a)。显示只查产物（ADR-0030 D1），清理后产物为空是已知结论，不值一次往返；祖先链用 `tree.parent` 缓存，缺链的叶子按「在范围内」保守卸载 |
| D4 | 服务端 DELETE 要不要接到同一份清理实现 | (a) ★ handler 照 `model_generate` 做路由（`dbnum_for_roots` → `route_for` → 非 Database 则 `scope_memory_only`），用 direct tree 解范围内生成根，调 `clear_scope_for_regeneration` 改名后的 `pub async fn clear_model_scope(requested, roots)`；`delete_model_subtree` 保留为摄入形态那一支（它的读透分支随之删掉，读透只从 `clear_model_scope` 进）；(b) 最小改：只给 `delete_model_subtree` 读透分支补 try-lock，不做路由；(c) 服务端不动 | (a)。否则 Memory 路由（初始化未落的 durable 库）上点「清理」会撞初始化门或删到错的地方，而 `force=true` 在同一库上却能清——同一个范围两条入口两种行为。(b) 修了 409 但修不了路由 |
| D5 | DELETE 要不要 120 s 预算包装 | (a) ★ 不加：客户端超时 125 s（与 generate 同），超时按「服务端可能已删一半，点眼睛重查」提示；(b) 加 `await_background_without_cancelling` + 504 契约 | (a)。删除比生成快得多，先不为它建一套 504 语义；V4 记一次 ZONE 清理用时，慢得多再议 |
| D6 | 批量 | ★ 与生成同形：顺序各一发，整批回执；一个失败不影响其余 | — |
| D7 | 文档 | (a) ★ ADR-0030 末尾加「2026-10-10 修订 D4：清理也是右键动作，调 §4.5.1」；不立新 ADR；`CONTEXT.md` 加「清理模型」词条；gen-model spec §4.5.1 改写（路由、409 三形态、与 `force=true` 同一实现）；两边 changelog 各一条；gen-model 拟立的 ADR-087 D5 顺带写上「DELETE /model/subtree 同一实现」 | (a)。决定本身小、依附于 ADR-0030 的框架（显示查、生成右键、清理右键），不够一篇 |

### 3E. 整库清理（2026-10-10 补；★ = 推荐）

| # | 题 | 选项 | 推荐 |
|---|---|---|---|
| E1 | 入口 | (a) ★ 任务队列库行「操作」列第三枚按钮「清理模型（整库）」（`Btn` 空标签 + `ph::BROOM`，紧挨「重算模型（整库）」右侧），判据复用 `model_rebuild` 的材料另出一份 `model_cleanup(db, active_rebuild, active_data_task)`：排除 / 够不着 → 不画；阻断 → 灰原文；未初始化 → 灰「没有模型可清」；同库在飞重建或排队 / 运行中的数据任务 → 灰「等它跑完」；其余可点；(b) 塞进「重新计算…」两步窗当第四个勾选项 | (a)。清理是单库、一次性、破坏性的动作，要一键 + 确认框，不是批量计划书里的一项；(b) 的勾选项与「重新解析」挤在一起容易点错 |
| E2 | 模型水位怎么处置 | (a) ★ **归零**（与清库 / 回退重建的模型半边同一句，ADR-060 D8）：库行随即显示「落后 + 内存」，右键重新生成的结果只进投影，手动「重算模型（整库）」或下次服务重启的自动恢复把整库算回来并落库；(b) 水位不动：库行仍「追平」、按需生成逐根落库，但「水位说追平过、模型面一行都没有」正是 ADR-060 D8 点名的幽灵形态，而且增量窗口只算改过的根，没改的根永远空着 | (a)。清理的目的就是「清掉再整库重算」，水位归零让这件事在 `/dbnums` 上看得见、重启也不会忘；(b) 是静默失效（宪法 III） |
| E3 | 清理后要不要顺手排整库重算 | (a) ★ 不排：两枚按钮各管各的，清理回执日志末尾一句「点「重算模型（整库）」重新生成（服务重启也会自动补）」；(b) 确认框加勾选「清理后立即排整库重算」 | (a)。少一个耦合点；重算按钮就在旁边，一次点击的事；要「先清后算」成一键再议 |
| E4 | 服务端闸门 | (a) ★ 该库无排队 / 运行中的批次（队列快照同库行）、无在飞整库重建 / 整库即时任务、`try` 该库生成锁（拿不到 = 有按需生成在飞）→ 任一不满足 409 带原文，一行不删；**不要求队列暂停**（不碰数据面，与 `DELETE /dbnums/{n}/data` 不同）；(b) 照搬 `ensure_dbnum_mutation_idle`（要求先暂停队列） | (a)。暂停队列是数据面快删的运维契约，模型面清理是用户面动作，让人先去暂停再清再恢复是三步替一步 |
| E5 | 删哪些表 | ★ `RANGE_TABLES` 的模型子集按 Ref0 区间：`inst_relate / tubi_relate / geo_relate / inst_info / aabb / trans`（`aabb:<mesh id>` 共享盒天然不在区间内）+ `room_relate / room_panel_relate WHERE in.dbnum`；不碰 `pe / ref_rev` / noun 表 / `dbnum_info_table` / `dbnum_stats_state` / 交付台账。表清单与 `render_delete_phases` 同一个常量切出来，不抄第二份 | — |
| E6 | Memory 路由（初始化未落 / 读透） | ★ 库里没有权威模型面可删：只 `clear_dbnum` + `forget_dbnum` + `mark_aabb_tree_dirty`；读透另对投影里该库每个根渲一条 spool 墓碑；水位行在读透下不存在就不写 | — |
| E7 | 客户端清理后三维 | ★ 卸载 `loaded_models` 里属于该库的叶子：`/dbnums` 每行已带 `ref0s`（该库全部 Ref0，`handlers.rs:3372–3390`），`DbnumStatus` 补 `#[serde(default)] ref0s: Vec<u32>`，叶子 `RefU64` 的高位 ref0 落在其中即属于该库；相关 `model_scopes` 键（成员里有该库叶子的）整条移除、让下次点眼睛重查；树行回未加载；日志「已清理 db{n} 全部模型产物（N 条语句 / M ms）；三维卸载 K 个模型；点「重算模型（整库）」重新生成（服务重启也会自动补）」 | — |

## 4. 步骤

依赖：C1 → C2 → C3（plant-ui，可对现有服务端联调）；G1 独立（gen-model）；**G3 → C5**（整库：服务端先有端点，客户端再接）；C4 / G2 文档随各步落。每步单独可提交、单独可回退。

### C1 · plant-ui 命令与接口（`lib.rs`、`model_update_api.rs`、`data.rs`）

- `Cmd::CleanupModels { targets }`、`Cmd::CleanupConfirm { accepted }`（或与 Regenerate 共用一个 `ModelMutationConfirm { accepted }`，实施时按 D2 的形状定）。守卫 `regeneration_is_not_a_scene_action` 扩成「生成与清理都不是 `ModelAction`」。
- `model_update_api::delete_model_subtree(base, refno, project, mdb, namespace) -> Result<CleanupReply { requested_refno, scope, status }>`：新增 `delete` 助手（`ehttp::Request::get` 后 `method = "DELETE".into()`，Accept 头，无 body，超时 125 s）；query 里 `refno` 与 `confirm` 都 `urlencode`（斜杠形），身份三件套同 generate。
- `Req::CleanupModels { epoch, refnos, base, project, mdb, namespace }` → 顺序各一发 → `Evt::ModelsCleaned { epoch, outcomes: Vec<(RefU64, Result<CleanupReply>)> }`。
- 用例：请求行形状（路径、`confirm == refno`、编码）、回包解析、`CleanupModels` 顺序发且整批回。

### C2 · 确认框（`model_regenerate.rs` → 按 D2 泛化）

- `Vm` 加 `kind`；文案表：清理 = 「对 {label} 清理模型？范围内已生成的产物会被删除，三维里对应的模型会卸载；需要时再右键「重新生成模型」。」小字：「只删产物，不删设计元素；服务端按子树删，范围内有根正在生成会被拒绝（稍后再试）。」`Running` = 「正在清理 {label} 的模型…」；窗标题按 kind。
- 钉子：清理文案点名「删除」与「不删设计元素」、不带数字；`Running` 两种 kind 都无叉无按钮；`tone` 两档 Warn / 失败 Error 不变。

### C3 · 宿主流程与菜单（`main.rs`、`workbench/tree.rs`）

- `tree.rs::element_menu`：「重新生成模型」按钮之后、同组内加「清理模型{suffix}」，`add_enabled(!regen_busy)`，悬停文案改（D1）。
- `main.rs`：`begin_cleanup(targets)` / `settle_cleanup_confirm(accepted)` / `settle_models_cleaned(outcomes)`，骨架照 regenerate 三件；`settle_models_cleaned` 收尾按 D3 卸载；日志：成功「{label}：已清理模型产物（服务端 exact_subtree）；三维卸载 N 个模型；需要时右键「重新生成模型」」，409 → ERROR「{label}：范围内有根正在生成，稍后再试」，timeout → WARN「{label}：清理请求超时，服务端可能已删一部分；点眼睛重查确认」，其余 ERROR 原文。
- 用例（照 `regenerate_generates_once_then_displays_the_targets` 写法）：确认 → 恰好 N 发 `Req::CleanupModels`；回执成功 → 发出 `ModelAction::Unload` 覆盖目标名下全部已加载叶子（含别的范围键下、祖先链含目标的叶子）、`model_scopes` 不再含目标及其子树键、eye 回未加载、`regen_busy` 复位；取消 / 关窗零请求；409 不卸载。

### G1 · gen-model：DELETE 与 `force=true` 共用一份清理（`on_demand_model.rs`、`handlers.rs`）

- `clear_scope_for_regeneration` → `pub async fn clear_model_scope(&self, requested, roots)`，文档改成「右键重新生成的先删一步与 `DELETE /model/subtree` 的唯一实现」；`delete_model_subtree` 去掉读透分支（只剩摄入形态），或保留但只被 `clear_model_scope` 调——实施时核 `lib.rs:2009`、`batch_worker.rs:3111` 两个旧调用点该走哪一支（增量窗口的 `DeleteCleanup` 是摄入形态语义，照旧直调 `delete_model_subtree` 即可）。
- `model_delete_subtree` handler：照 `model_generate` 的路由段（`dbnum_for_roots` → `route_for` → 非 Database 进 `scope_memory_only`；解不出库不路由），用 `state.direct_tree` 解范围内生成根，调 `clear_model_scope`；Memory 路由同样 `require_room_bootstrap_ready()`、同库有整库任务回 409。
- 钉子：handler 源码钉改成钉 `.clear_model_scope(`；`on_demand_model` 加「`clear_model_scope` 只有这一份、`model_generate` 与 `model_delete_subtree` 都调它」；读透形态「先 try-lock 再删」的断言从 S0 那条钉子扩到这里。
- 验证：`cargo check --lib`；`cargo test --lib -- on_demand_model web_service::handlers::tests model_write_behind`；全量 `cargo test --lib` 记数（实施前以 S1 当时现跑为准）。
- 提交：只 `git add src/data_interface/on_demand_model.rs src/web_service/handlers.rs docs/specs/web-service-api.md changelog.md`。

### G3 · gen-model：`DELETE /api/v1/dbnums/{dbnum}/model` 整库模型清理（`fast_delete.rs`、`handlers.rs`、`web_service/mod.rs`）

- `fast_delete.rs`：把 `render_delete_phases` 里「模型表区间删 + 房间两边 + 模型水位半边 + spatial epoch」抽成 `render_model_face_wipe(dbnum, ref0s) -> (ranges, metadata)`，`ResetForReinit` 那一支改调它（渲染一份两处用；现有 `fast_delete` 用例按语句比对，先跑一遍确认抽完逐字相同）。新增 `pub async fn wipe_dbnum_model_face(dbnum) -> ModelFaceWipeResult { dbnum, ref0s, range_statements, elapsed_ms }`：Ref0 圈定沿用 `wipe_dbnum_rows_on` 开头那三句（抽成 `collect_dbnum_ref0s_on`）；拿 `DATA_COMMIT_SERIAL` + `DBNUM_STATE_WRITE_GATE` 写锁 + `lock_db_generation(dbnum)` 的 try 形态（拿不到 = 有生成在飞 → 409 类错误）；执行区间删 → 事务里写元数据 → 核对八张表该库区间为零；进程内 `clear_dbnum` / `forget_dbnum` / `mark_aabb_tree_dirty` / `invalidate_mirrored` / `announce_source_changed(Memory)`（与 `wipe_dbnum_for_reinit` 同一串，抽成 `forget_dbnum_model_in_process(dbnum)` 两处共用）。**不**调 `forget_persisted_catalogue_rows`（目录行是数据面）。
- Memory 路由（E6）：`route_for(dbnum)` 非 Database 时跳过库里那半边，只做进程内那一串；读透另对投影里该库每个根 `render_spooled_scope_deletion` 进 spool。
- handler `dbnum_model_delete`：`confirm` 必须等于路径 dbnum（400）；`watch_scope::admits`（400，与 rebuild 同）；E4 闸门：队列快照同库有行 → 409；`model_rebuild::active_for_dbnum` / `active_dbnum_model_ensure` → 409；读透下照 E6。回 200 `{dbnum, scope:"model_face", status:"deleted", ref0s, range_statements, elapsed_ms, model_source_after:"memory"}`。路由 `.route("/api/v1/dbnums/{dbnum}/model", delete(handlers::dbnum_model_delete))`。
- 钉子：`render_model_face_wipe` 的表清单 = `RANGE_TABLES` 去掉 `pe / ref_rev`（源码钉，防两份清单漂移）；「整库清理从不碰 `pe` / noun 表 / 交付台账」（照 `the_wipe_never_touches_the_delivery_ledgers` 写）；「水位半边与 `ResetForReinit` 逐字相同」；handler 顺序钉（confirm → scope → 闸门 → 删）；`mem://` 上一条行为用例：写两根几何 + 房间边 → 清 → 八张表为零、`pe` 不动、`model_sesno = 0`、`state_revision + 1`。
- 验证：`cargo check --lib`；`cargo test --lib -- fast_delete web_service::handlers::tests model_read_route`；全量 `cargo test --lib` 记数。
- 提交：只 `git add src/data_interface/fast_delete.rs src/web_service/handlers.rs src/web_service/mod.rs docs/specs/web-service-api.md changelog.md`（与 G1 分两笔）。

### C5 · plant-ui：库行「清理模型（整库）」（`task_queue.rs`、`lib.rs`、`model_update_api.rs`、`data.rs`、`main.rs`）

- `task_queue.rs`：`DbnumStatus` 补 `#[serde(default)] ref0s: Vec<u32>`；判据 `model_cleanup(db, active_rebuild, active_data_task) -> ModelCleanup::{Hidden, Blocked(..), NeedsInit, Busy{task_id}, Ready(dbnum)}`（E1，纯函数、材料只吃 `/dbnums` 一行 + `Vm` 里同库活动任务）；操作列再扩一枚（74 + 4 + 44 + 4 + 44 点），第三枚 `Btn` 空标签 + `ph::BROOM`，悬停「清理模型（整库）db{n}：删掉该库全部已生成的模型产物并把模型水位归零；不自动重算」；按下 `Cmd::CleanupDbnumModel { dbnum }`。
- `lib.rs`：`Cmd::CleanupDbnumModel { dbnum }`；确认走 D2 那一个框（`kind: Cleanup`、`scope: Dbnum(n)`，文案「清理 db{n} 的全部模型？该库已生成的模型产物会被删除、模型水位归零，三维里该库的模型会卸载；之后请点「重算模型（整库）」重新生成。」）。
- `model_update_api.rs`：`delete_dbnum_model(base, project, mdb, namespace, dbnum) -> Result<DbnumModelCleanupReceipt>`（`DELETE /api/v1/dbnums/{n}/model?confirm={n}&project..`，复用 C1 的 `delete` 助手，超时 125 s）；`endpoint_missing` → 「这一档服务端没有整库清理接口」（与 rebuild 的 D9 同一判法）。
- `data.rs`：`Req::CleanupDbnumModel { dbnum, base, project, mdb, namespace }` → `Evt::DbnumModelCleaned { dbnum, result }`。
- `main.rs`：`settle_dbnum_model_cleaned`：成功 → E7 卸载 + 日志；409 → ERROR 原文（「该库有任务在跑 / 有生成在飞」）；`endpoint_missing` → INFO 降级；其余 ERROR；催一拍队列轮询（库行翻成「落后 + 内存」）。
- 用例：判据五档（照 `model_rebuild` 的那组写）；按钮位置钉（第三枚、在 rebuild 右侧）；请求行形状；`ref0s` 缺席（老服务端）时清理成功只记日志、不卸载并说明「老服务端不给 ref0s，请点眼睛重查」；卸载集合 = `loaded_models ∩ ref0 ∈ ref0s`。

### C4 / G2 · 文档

- plant-ui：`CONTEXT.md`「模型显示与生成」加「清理模型」词条（定义、两个口径、_Avoid_）；ADR-0030 末尾加修订段（D7，含整库那条：清理在库行、与「重算模型（整库）」并排）；`CHANGELOG.md` 一条。
- gen-model：spec §4.5.1 改写；新增 §4.5.1D `DELETE /dbnums/{dbnum}/model`（闸门、删什么、水位归零与路由翻面、与 `DELETE /dbnums/{n}/data` 的分界）；`changelog.md` 一条；ADR-087 起草时 D5 带上这两条。

## 5. 验收

| # | 判据 | 怎么判 |
|---|---|---|
| V1 | 单元用例全绿 | plant-ui `cargo test -p plant-ui -p plant-ui-app`；gen-model 定向 + 全量 `cargo test --lib` 不少于实施前 |
| V2 | 真机单根 | 用户起后端（`release-20261010\verification\model-display\VERIFY-20261010.md`）；对已显示的 BRAN 右键「清理模型」→ 确认 → 三维里管身消失、树行未加载、日志一句；`GET /model/records?refno=<BRAN>` 为空；再右键「重新生成模型」→ 回来 |
| V3 | 多选 + 容器 | 选 2 条 BRAN + 1 个 ZONE 清理 → 三发 DELETE、各自回执；ZONE 下所有已加载模型都卸掉、别的 ZONE 不动 |
| V4 | 并发 409 | 读透形态：对一个 ZONE 发 `generate force=true`，生成中立刻右键清理其中一条 BRAN → 日志「范围内有根正在生成，稍后再试」，三维不卸载；记一次 ZONE 清理用时 |
| V5 | Memory 路由 | 初始化未落的库（或读透）上清理 → 走投影清理，不撞初始化门，`/model/records` 空 |
| V6 | 不多发 | 整个流程网络日志里只有 DELETE，没有 records / generate |
| V7 | 整库清理（摄入形态） | 已初始化、已追平的库：显示几根 → 库行点「清理模型（整库）」→ 确认 → 回 200；三维该库模型全卸、别的库不动；库行翻成「落后 + 内存」；库里 `SELECT count() FROM inst_relate WHERE dbnum = n` 等八张表为 0、`pe` 行数不变、`dbnum_watermark:n.model_sesno = 0`；再点「重算模型（整库）」→ 任务跑完库行回「追平 + 数据库」、三维点眼睛回来 |
| V8 | 整库清理闸门 | 该库有排队 / 运行批次时点 → 409 原文、库里一行不少；整库重算在飞时点 → 409；右键重新生成在飞时点 → 409 |
| V9 | 整库清理（读透 / Memory 路由） | 读透配置下清一个库 → `/model/records` 该库为空、别的库不动；spool 里有该库每根的墓碑 |
| V10 | 重启自动补 | V7 清理后不点重算、重启服务 → 启动恢复排一条 ModelOnly 整库任务（`[model-completion] dbnum=n applied=T model=0`） |

## 6. 风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| 清理与生成共用 `regen_busy`，大 ZONE 清理期间取回工作灰几秒 | 轻微 | 日志有在途提示；V4 记用时 |
| 服务端 DELETE 无预算包装，超大范围超过 125 s | 客户端超时、服务端继续删 | 日志按「可能已删一部分，点眼睛重查」说清；D5 留口 |
| 本地卸载漏掉「别的范围键下、祖先链不在 `tree.parent` 缓存里」的叶子 | 三维残留已删产物 | 缺链按在范围内保守卸载；点眼睛重查即纠正（records 为空 → 卸载） |
| G1 改 handler 路由，老客户端（python `aios_client.py`、plant3d-web）调 DELETE 的行为变化 | Memory 路由上从「撞门 / 删错处」变成「清投影」，摄入形态不变 | spec 写明；属修正不是回归 |
| 读透形态 try-lock 改到 `clear_model_scope`，`delete_model_subtree` 旧调用点语义漂移 | 增量窗口清理走错支 | G1 实施时逐个核调用点、加钉 |
| 整库清理把模型水位归零 → 路由翻到内存，期间右键重新生成的结果不落库（E2 的代价） | 用户以为「重新生成了」却没持久化；重启后由启动恢复整库重算 | 确认框与回执日志都点名「之后请点重算模型（整库）」；库行「落后 + 内存」字形本来就在；spec 写明 |
| 整库清理与 `ResetForReinit` 共用一份水位 / 表清单渲染，抽函数时改坏回退重建 | 回退重建少删一张表或水位句漂移 | 先用现有 `fast_delete` 语句比对用例做基线，抽完逐字相同才算；加「两处渲染相同」的钉 |
| 大库（几万 inst_relate 行）整库清理在 SurrealDB 上的区间删耗时 | 超过客户端 125 s | 区间删本来就是 `DELETE /dbnums/{n}/data` 在用的写法（整库快删含 pe 都能过）；V7 记用时；慢再分块 |
| 读透形态按根渲 spool 墓碑，根数以千计 | spool 文件大、落库慢 | V9 记量；超过阈值再做按库墓碑 |
| 老服务端没有 `DELETE /dbnums/{n}/model` | 新客户端按钮点了 404 | `endpoint_missing` 降级为一句 INFO「这一档服务端没有整库清理接口」，按钮照画（与 rebuild D9 同一纪律） |

## 7. 不做

- 「深度删除」（连 `inst_geo` / `.mesh` 一起删）：d-8 / d-10 否决；共享行重写由重新生成计划 S2 负责。
- 整库清理**不自动排重算**（E3）；不做「多库批量清理」（要批量进「重新计算…」两步窗再议）。
- 整库清理不碰数据面（`pe` / noun / `ref_rev` / 统计 / 交付台账）：那是 `DELETE /dbnums/{n}/data` 与「重新解析」的事。
- 服务端 DELETE 的 504 / 后台继续契约（D5）。
- 清理后自动重新生成：那就是「重新生成模型」，已有。
- 不动 `ModelAction`；显示路径不变（ADR-0030 D1 / D3）。

## 8. 宪法核对

| 条 | 本计划 |
|---|---|
| II 一条规则只有一份实现 | 范围清理只有 `clear_model_scope` 一份（`force=true` 与 DELETE 共用）；整库清理的表清单 / 水位半边 / 进程内作废与清库、回退重建共用同一份渲染；确认框只有一份（D2）；批量与回执形状照 generate |
| III 静默失效是最高级别缺陷 | 409 / 超时 / 其他失败逐目标点名；超时明说「可能删了一部分」；服务端读透形态不再悄悄排队等锁；整库清理把水位归零而不是留一个「追平但空」的幽灵（E2） |
| VI 不变量由可执行的守护看住 | 「生成与清理不是 `ModelAction`」守卫；菜单顺序钉；文案钉；handler / on_demand / fast_delete 源码钉；卸载范围用例；`mem://` 上整库清理的八表为零 / `pe` 不动用例 |

## 9. 回退

C1–C3 一笔或三笔提交，`git revert` 即回，服务端不依赖它。G1 单独一笔；回退后 DELETE 回到今天的行为（读透不 try-lock、不路由），客户端照常可用。G3 单独一笔；回退后端点消失，C5 的按钮按 `endpoint_missing` 降级为一句 INFO，不报错。C5 单独一笔。
