# 显示只查已生成的产物、生成只在右键 · 开发计划

依据：plant-ui ADR-0030、gen-model-refactor ADR-086（2026-10-09 grill-with-docs 定案）。术语见 `CONTEXT.md`
「模型显示与生成」。本文只记要干的活、顺序、验证与提交；决定本身与理由在两份 ADR 里，这里不重复论证。

工作树与分支（主工作树里都有别的会话未提交的改动，一字不碰）：

| 仓 | 工作树 | 分支 | 基线 |
|---|---|---|---|
| gen-model-refactor | `D:\work\plant-code\old\.scratch\dqo\gen-model-refactor-model-roots`（旁边 `plant-ui` 是指向 `plant-ui-release` 的目录联接，供 `include_bytes!` 内嵌 wasm 包用） | `codex/model-roots-and-record-status` | `c7ad0c196`（ci/linux-build-refactor-20260928） |
| plant-ui | `D:\work\plant-code\old\plant-ui-display-query-only` | `codex/display-query-only` | `ed09e945f`（codex/increment-ui-closure） |

## 一、定下来的东西（只列结论）

| # | 问题 | 结论 | 出处 |
|---|---|---|---|
| 1 | 显示怎么查 | 以点下去的节点为祖先，`/model/records` 回子树里已生成的全部实例；所有显示入口都不调生成接口 | ADR-0030 D1 |
| 2 | 查空怎么办 | 树行停「未加载」，INFO 一句「范围内没有已生成的模型；右键「重新生成模型」可生成」，不记 ERROR；老服务端 409 `not_generated` 同样处理 | ADR-0030 D1 / 后果 6 |
| 3 | 生成接口 | `/model/ensure` 改名 `/model/generate`，请求 / 回执 / `force` 不变；旧路径留一版别名并打 deprecated | ADR-086 D4 |
| 4 | 右键怎么调 | 确认框（不带数字）→ 一发 `generate { refnos, force: true }` → 成功回执后以同一批目标为祖先查 records 显示；`timeout` 不显示、日志说明后台继续 | ADR-0030 D2 |
| 5 | 旧逐根流水 | 清点 / 数字确认 / 整片删 / 逐根调 / 停在这里 全部退役 | ADR-0030 D2 |
| 6 | 取回工作 | 只重装，不再先生成 | ADR-0030 D3 |
| 7 | 服务端 records | 内存投影路由也按祖先回答；没生成过 200 空；`not_generated` 退役；回执形状不变 | ADR-086 D1–D3 |
| 8 | 新旧兼容 | 新客户端对老服务端：`/model/generate` 404 → 退回 `/model/ensure`；409 当查空。老客户端对新服务端：别名在，字段不变 | ADR-0030 后果 7、ADR-086 后果 3 |

## 二、不做

- 不加 `/model/roots`，不在客户端解根、不在客户端删产物。
- 树行不加「未生成 / 已过期」态，records 不带凭证。
- 不改服务端 120 s 生成预算，不做异步作业。
- 不动 `ModelAction`，`regeneration_is_not_a_scene_action` 守卫照旧。
- 不动库行「重算模型（整库）」那条路。

## 三、分段

### S1　gen-model-refactor：`/model/generate` 路由与别名

- `src/web_service/mod.rs`：新增 `.route("/api/v1/model/generate", post(model_generate))`；`/api/v1/model/ensure` 指向
  同一 handler 的薄包装，命中时 `tracing::warn!("deprecated: use /api/v1/model/generate")` 一行。
- `src/web_service/handlers.rs`：`model_ensure` 改名 `model_generate`（函数名、文档、日志措辞 ensure → generate；
  `[model.worker.*]` 的字段名不动，避免破坏现有日志消费者——改名只在人读的措辞）。
- 测试：既有 ensure 路由测试改打 `/model/generate`；新增 `the_ensure_path_is_an_alias_of_generate`（同一请求两条路径回执相等）。
- 提交：`feat(web): /model/generate 取代 /model/ensure，旧路径留别名打 deprecated（ADR-086 D4）`。

### S2　gen-model-refactor：records 按祖先回答、没生成过是空

- `src/web_service/handlers.rs` `model_records`（内存路由那一段，今天 `store.receipt(root)` 缺失即 409
  `not_generated`）：改为对每个请求根做祖先过滤——投影里 `anc` 含该根（或精确等于）的实例全部返回；
  一个都没有就空数组，不再 409。`conflict` 分支不动。
- 投影 overlay（`overlay_records_of_root` 所在模块）：补一个按祖先取的读法；若投影记录没有 `anc`，写回执时带上
  （`GeomInstQuery` 本身有 `anc`，核实后决定是否需要补）。
- `rg not_generated` 核 plant3d-web 与 gen-model-refactor 自己的消费者，有把 409 当分支的同一次改掉。
- 测试：`memory_route_records_answer_by_ancestor`、`a_scope_without_products_is_empty_not_409`、
  `durable_and_memory_routes_agree_on_an_ancestor`；既有 records 反序列化测试不动。
- 提交：`feat(web): /model/records 内存路由按祖先回答，没生成过回 200 空（ADR-086 D1–D3）`。

### S3　gen-model-refactor：验证与收尾

- `cargo test --lib` 相关模块 + `cargo test --test <records / ensure 相关集成测试>`；`cargo clippy -- -D warnings`。
- `docs/release-notes/`：下一版说明里点名两条行为变化（generate 改名 + 别名；未生成根 409 → 200 空）。
- 提交 ADR-086 与 release note：`docs(adr): ADR-086 …`。

### S4　plant-ui：显示路径只查

- `crates/plant-ui-app/src/data.rs`：
  - `ModelLoad::Scopes` 臂：删掉 `ensure_model` 调用与 `Evt::ModelScopeEnsured`；`generation_roots` 直接用 `[target]`
    （服务供数由服务端按祖先回答；库供数本就 `anc CONTAINS`）。
  - `ModelLoad::Replace` 臂（取回工作重装）：删掉先 ensure 那段与 `Evt::ReloadEnsured`，只重查。
  - `scope_targets` 的 ensure 用途与相关 `Req` / `Evt` 变体退役（集合本身留作服务供数重装的重查范围：`/model/records`
    一根一次往返，按叶子问整个 ZONE 是几千次——实施时定，已写进 ADR-0030 D1 / D3）；测试
    `a_failed_ensure_still_queries_the_models` 换成源码钉 `display_never_generates`（`Scopes` / `Replace` 臂文本里不含
    `generate_model` / `ensure_model`）。
- `crates/plant-ui-app/src/main.rs`：
  - 删 `Evt::ModelScopeEnsured` / `Evt::ReloadEnsured` 的处理臂（含「范围核对失败」WARN、「核对 N 个范围」那句尾巴）。
  - `Evt::ModelScope(_, _, Ok(records))` 查空：INFO「范围内没有已生成的模型；右键「重新生成模型」可生成」。
  - `Evt::ModelScope(_, _, Err(e))`：`failure_of(e).code == "conflict"` 且消息含 `not_generated`（老服务端）→ 同上一句 INFO；
    其余照旧 ERROR「已有模型查询失败」。测试 `an_empty_scope_is_a_hint_not_an_error`。
  - `FocusTreeScope` / `ShowRoomModel`：核对它们走的是同一条 `ModelLoad::Scopes`，注释里「ensuring missing geometry once」改掉。
- 「正在补齐 N 个模型」那条进度（`Evt::ModelScopeProgress` 里 ensure 那一半）去掉，只留 records 的分页进度。
- 提交：`feat(display): 显示只按祖先查已生成的产物，不再先生成（ADR-0030 D1 / D3）`。

### S5　plant-ui：接口改名与右键流水

- `crates/plant-ui-app/src/model_update_api.rs`：`ensure_model` → `generate_model`，路径 `/api/v1/model/generate`；
  `EnsureReply / EnsureStatus` → `GenerateReply / GenerateStatus`；404 时退回 `/api/v1/model/ensure` 同一请求体，
  并通过回执带回「旧服务端」标记供日志点名一次。`delete_models`（整片删）失去调用方后删除。
- `crates/plant-ui/src/lib.rs`：`Cmd::RegenerateModels { targets }` 与 `RegenerateConfirm { accepted }` 保留；
  `RegenerateStop` 删除（没有逐根派发就没有「停在这里」）。注释按 ADR-0030 D2 改。
- `crates/plant-ui/src/model_regenerate.rs`：`Vm` 收成 `Confirm { label } / Running { label } / Failed { label, reason }`，
  确认框文案「对 {label} 重新生成模型？范围内已有的产物会被重算替换」；`STOP_LABEL` 删除。
- `crates/plant-ui-app/src/main.rs`：
  - `begin_regenerate_count` → `begin_regenerate`：不再发 `Req::RegenerateScope`，直接进 `Confirm`。
  - `settle_regenerate_confirm(true)`：发一条 `Req::GenerateModels { epoch, refnos, base, project, mdb, namespace }`
    （`force = true`），`regen_busy = true`。
  - 新 `Evt::ModelsGenerated { epoch, result }`：`Ok` 且 `status != timeout` → 日志一句摘要（生成根数 / 命中数）→
    对同一批目标发 `ModelLoad::Scopes`（显示）；`timeout` → WARN「服务端后台继续生成，完成后再点眼睛」，不显示；
    `Err` → ERROR。`regen_busy = false`，epoch 对不上的回执丢弃。
  - `dispatch_next_regenerate_unit` / `settle_regenerate_unit` / `finish_regenerate` / `regenerate_summary` /
    `regenerate_label` / `regenerate_show` / `regenerate::UnitOutcome` 五档 全部删除。
- `crates/plant-ui-app/src/data.rs`：`Req::RegenerateScope / RegenerateDeleted / RegenerateUnit` 与对应 `Evt` 删除，
  新增 `Req::GenerateModels` → `generate_model` → `Evt::ModelsGenerated`。
- 全局：`rg -n 'ensure' crates/` 清到只剩 404 回退分支与注释里对 ADR-0009 的历史引用；日志措辞「核对 / 补齐」→「生成」。
- 测试：`regenerate_generates_once_then_displays_the_targets`、`a_timed_out_generate_does_not_display`、
  `get_work_reloads_without_generating`；`regeneration_is_not_a_scene_action` 不动；源码钉
  `no_ensure_left_outside_the_fallback`。
- 提交：`feat(regenerate): 右键一发 generate 后显示，旧逐根流水退役；ensure 改名 generate（ADR-0030 D2）`。

### S6　plant-ui：验证与收尾

- `cargo test -p plant-ui -p plant-ui-app`（相关测试二进制），`cargo clippy -p plant-ui -p plant-ui-app -- -D warnings`，
  `cargo fmt --check`；wasm 目标 `cargo check -p plant-ui-app --target wasm32-unknown-unknown`（显示链路两端共用）。
- 手工：对着本机 gen-model（S1–S3 那版）点 `ZONE /1WCC-PIPE-RX` 眼睛——只有一次 records 查询、无 WARN / ERROR；
  右键重新生成 → 一发 generate → 跑完三维出现；`timeout` 场景日志符合 D2。记进 `docs/evidence/2026-10-09-display-query-only.md`。
- 提交 ADR-0030、CONTEXT.md、ADR-0009 / 0024 的取代标记、本计划、evidence：`docs: ADR-0030 显示只查、生成只在右键 …`。

## 四、顺序与依赖

S1 → S2 → S3（服务端先齐，plant-ui 手工验证要用它）→ S4 → S5 → S6。S4 与 S5 可以合成一个提交，但分开更好回看。
每段做完经 zhimo 交一次进度；每段只 `git add` 自己碰过的路径。

## 五、风险

1. **plant3d-web 可能依赖 409 `not_generated`**：S2 先 `rg`，有则同一次改；没有也在 release note 点名。
2. **投影记录没有 `anc`**：S2 要补写回执带 `anc`；已有投影（服务不重启）里的旧记录没有 `anc` 就查不到——实施时若遇到，
   启动时重建投影或按根回退一次，写进 evidence。
3. **大范围一发 generate 撞 120 s**：ADR-0030 后果 3 已接受；日志必须把「后台继续、完成后再点眼睛」说清。
4. **wasm 端**：`generate_model` 的 404 回退要在 `ehttp` 两端同形，S6 的 wasm check 守着。

## 六、验收

- ADR-0030 实施约束 1–7、ADR-086 实施约束 1–4 对应的测试全绿。
- 手工场景（§三 S6）通过并留 evidence。
- 两仓 clippy / fmt 干净；提交只含本计划点名的路径。
