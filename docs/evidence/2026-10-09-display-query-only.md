# 显示只查、生成只在右键 · 验证记录（ADR-0030 / gen-model ADR-086）

计划：`docs/plans/2026-10-09-display-reads-products-generation-is-a-context-menu-action.md`。
工作树与提交：gen-model-refactor `codex/model-roots-and-record-status`（`82e1257b4` S1、`fd5d3ccd9` S2、`392a731f8` S3、
`4771cbcf1` 别名点名改走控制台）；plant-ui `codex/display-query-only`（`3b895956c` S4、`0dd254ea2` S5、本文随 S6 提交）。

## 1. 自动化

### gen-model-refactor（`c7ad0c196` 之上）

- `cargo test --lib`：2462 passed / 1 failed / 128 ignored。唯一失败 `plant_ui_web_bundle_contains_bootstrap_and_runtime_assets`
  是 0.1.34 release note 已点名的内嵌 `index.html` 字符串问题（这个工作树旁边的 `plant-ui` 目录联接指向 `plant-ui-release`），
  与本改动无关。
- `cargo test --test refno_native_api_contract`：4/4（`model_generate_base_shape_and_route_enrichment_are_frozen` 改钉 `model_generate` 段）。
- `cargo clippy --lib -- -D warnings` 干净；`--tests` 下 `src/bin/e3d_tty_cata_members_compare.rs:190` 一处 `iter().cloned().collect()`
  是基线既有，不在本计划路径，未动。
- 新增用例：`receipt_roots_under_an_ancestor_lists_the_committed_roots_below_it`（存储层：祖先展成名下盖过章的根、根以下的件找到头上那根、
  一根都没有是空）、`model_records_answer_by_ancestor_and_never_say_not_generated`（源码钉：三条路都在、`not_generated` 不在）、
  `the_ensure_handler_is_a_deprecated_alias_of_generate`、`the_ensure_path_is_an_alias_of_generate`。
- `rg not_generated`（ADR-086 后果 2 / 计划风险 1）：gen-model 自己的消费者没有；plant3d-web `modelRecords.ts` 的 `isNotGeneratedConflict`
  一次重试与 `scripts/verify-gen-model-v1.ps1` 的整批退回逐根把 409 当分支——它们主路径先 ensure 再 records，不受影响，分支只是走不到；
  plant3d-web 不在本计划的工作树表里，**没改**，在 0.1.35 release note 点名。

### plant-ui（`ed09e945f` 之上）

- `cargo test -p rs-plant -p plant-ui`：167 + 170 passed / 0 failed（+2 doc test）。
- 实施约束对应的测试：1 `display_never_generates`、2 `an_empty_scope_is_a_hint_not_an_error`、3 `regenerate_generates_once_then_displays_the_targets`
  与 `a_timed_out_generate_does_not_display`、4 `get_work_reloads_without_generating`、5 `regeneration_is_not_a_scene_action`（不动）、
  6 `no_ensure_left_outside_the_fallback`（源码里 `ensure_model / EnsureReply / EnsureStatus` 一个不剩，`/api/v1/model/ensure` 只在
  `LEGACY_GENERATE_PATH` 一处）、7 CONTEXT.md 新增「显示」「生成」「重新生成」、改写「取回工作」。
- `cargo check -p rs-plant --target wasm32-unknown-unknown`：通过（`ring` 的 build script 要 `clang`，本机在 `D:\LLVM\bin`，加进 PATH 才编得过；
  这是环境，不是代码）。
- `cargo clippy -p plant-ui -p rs-plant --tests`：本次碰过的文件没有新告警。`-D warnings` 整体是红的，全是基线既有（`model_update.rs` 6 处、
  `plant-ui-data/lib.rs` 文档缩进 3 处、`workbench/search.rs` 3 处、`settings.rs` 2 处等），没动。
- `cargo fmt --check`：基线即干净的文件（`main.rs` / `model_update_api.rs` / `model_regenerate.rs` / `lib.rs` / `focus_bounds.rs`）过了 rustfmt；
  `data.rs` / `sim.rs` 连同基线既有的几处排版一起过了（`data.rs` 保持 CRLF）；`manual_data_publish.rs` / `workbench/mod.rs` 基线就不干净，没碰。
- 编译说明：这个工作树的 `vendor/registry` 缺 tantivy 等未跟踪的包，编译时用
  `--config 'source.vendored-sources.directory="D:/work/plant-code/old/plant-ui/vendor/registry"'` 指向主工作树，仓里配置没动。

## 2. 服务端 API 级对拍（本机真库，2026-10-10 07:49–07:52）

起法：把 S1–S3 这版 `aios-database`（debug）连同 `DbOption.toml` 复制到 `%TEMP%\dqo-gen-run`，只改 `http_api_addr = 127.0.0.1:18922`
（本机 `:8022` 另有一个会话的服务在跑，没碰），`AIOS_STORE_MODE=mem`（read-through）、`PLANT_ASSET_ROOT` 指到临时目录；
等 `/health.initialization` 到 `model_phase_open && !startup_rooms_pending && !startup_reconcile_pending`（本机约 20 s）再打接口，
打完 `Stop-Process`。脚本不入库。

| 步 | 请求 | 结果 | 说明 |
|---|---|---|---|
| A | `records{generation_root: ZONE /1WCC-PIPE-RX = 24383/66457}`，什么都没生成 | **200，items=0**，`source=model-memory` | ADR-086 D2：没生成过是空，不是 409 |
| B0 | `generate{refno: BRAN /CODEX-ISSUE18-RETEST2 = 24383/101287, force}` | 200 `NoRenderableGeometry`，0 实例 | 这根 BRAN 是空的（测试遗留），跳过 |
| B | `generate{refno: BRAN /1WCC1135/B1 = 24383/66459, force}` | 200 `Generated`，1 根 / 2 实例，`model_source=memory` | 一发，服务端自己解根 |
| C | `records{ZONE}` | **200，items=2**（CAP 24383_66460 + 隐式管身，`owner` 都是 BRAN） | 节点在根之上：`receipt_roots_under` 展开 |
| C2 | `records{PIPE /1WCC1135 = 24383/66458}` | 200，items=2 | 同上，中间层级 |
| C3 | `records{BRAN}` | 200，items=2 | 精确根（原路） |
| D | `records{CAP 24383/66460}`（BRAN 的唯一成员） | 200，items=2 | 节点在根之下：`receipt_root_above` + `overlay_records_under_node`；管身的祖先链含这个 CAP，所以一起回 |
| E | `records{旁边没生成的 PIPE 24383/66461}` | **200，items=0** | 展开为空就是空 |
| F | `ensure{refno: BRAN, force=false}` | 200 `AlreadyAvailable`，回执与 B 同形 | 别名；控制台一行 `*** deprecated: POST /api/v1/model/ensure is an alias of /api/v1/model/generate … (refno=24383/66459) ***` |

两条路由同口径（ADR-086 D3）这一条没对：`mem` 形态只有内存路由，rocksdb 路由的 `durable_scope_reads` 本来就是 `anc CONTAINS`，没有改动。

第一轮曾把 `generate` 打在服务端还在「初始化房间（常驻房间模型）」时，回 503（`initialization_not_ready`）——这是既有的启动门，
不是本改动；等门开再打即 200。

## 3. 没做的手工场景（**未验证**）

plant-ui GUI 那一半——对着本机 gen-model 点 `ZONE /1WCC-PIPE-RX` 眼睛看「只有一次 records 查询、无 WARN / ERROR」，右键重新生成看
「一发 generate → 三维出现」，以及 `timeout` 场景——没有在本轮做：要起 GUI 并点击，本会话没有做。建议步骤：

1. 起 S1–S3 这版 gen-model（上面那套起法即可，端口随意，设置窗「模型服务」指过去）。
2. 点 `ZONE /1WCC-PIPE-RX` 眼睛：期望日志只有「开始查询并显示模型：1 个目标」→「查询到 N 个元素…」或
   INFO「范围内没有已生成的模型；右键「重新生成模型」可生成」；**没有** WARN「范围核对失败」、**没有** ERROR「已有模型查询失败」。
3. 右键 ZONE →「重新生成模型」→ 确认框只有一句「对 ZONE /1WCC-PIPE-RX 重新生成模型？范围内已有的产物会被重算替换。」→「重新生成」：
   期望日志「…正在生成模型（1 个目标…）」→「…生成完成，N 个生成根（重算 N、命中 0）」→ 三维出现；118 根 PIPE 很可能撞 120 s：
   期望 WARN「…服务端 120 秒内没做完，后台继续生成；完成后再点眼睛查看（这次不显示）」，确认框关闭，过一会儿再点眼睛能看到。
4. 右键一根已经显示的 BRAN 重新生成：期望旧几何被卸掉、新几何装回（`regenerate_refresh` 那条路）。
5. 取回工作：期望日志「已清空三维场景：按 N 个范围（点过眼睛的目标）重查已生成的模型…；不重新生成…」→「三维模型已就绪…」，
   没有「核对 N 个范围」那半句。
