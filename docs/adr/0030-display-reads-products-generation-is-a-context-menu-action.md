# 显示只按祖先索引查已生成的产物，生成只由右键「重新生成模型」直接调 generate 接口

状态：**已接受**（2026-10-09 grill-with-docs。用户定的五句：「在模型显示层级的节点时，是走的查询，不需要 ensure model」；
「显示节点的模型，默认都是使用查询 inst_relate 的 anc 索引」；「模型生成就是直接调用模型生成的接口，不需要额外的接口」；
「现有 /model/ensure 不要使用 ensure，而是改成模型生成的接口，generate 即可」；「生成完成之后，再调用显示接口」。
G1（右键直接调 generate、旧逐根流水退役）与 G2（`/model/ensure` 留别名）按推荐定案，批注时重点看 D2 与后果 2 / 3。）
取代：ADR-0009「显示时补齐模型走服务」整条；ADR-0024「取回工作先经模型服务补齐再重装」整条。
服务端半边：gen-model-refactor ADR-086（`/model/records` 在每条路由上都按祖先回答；没生成是空结果，不是 409）。

## 背景

ADR-0009 把「显示一个没生成过模型的元素」定成「先 `POST /model/ensure`，再查 `/model/records`」。现场
（2026-10-09 AvevaMarineSample，`ZONE /1WCC-PIPE-RX`，118 根 PIPE）的样子是：点眼睛 → 对 ZONE ensure → 服务端串行生成
118 根 → 120 s 到点回 `timeout`、后台继续、客户端拿不到句柄 → 客户端按「失败不中止」接着查 records，可回执里没有生成根，
只能拿 ZONE 自己去问（`data.rs:1180`）→ 内存路由只认精确生成根回执 → 409 `not_generated` → 日志「已有模型查询失败」。
一条 WARN、一条 ERROR、一片空白，分不清「没生成」「正在后台生成」「出错了」。

根因是**显示动作会改模型库**：一次点击可能持锁、生成、跑两分钟。而显示本来就有一条现成的只读路：`inst_relate` 的 `anc`
索引——给任何一个祖先，把子树里已经生成的实例全查出来（库供数直连 SurrealDB 就是这么查的，服务端 rocksdb 路由的
`durable_scope_reads` 也是）。只有内存投影路由例外地要求精确生成根，那是服务端的事（ADR-086）。

## 决定

### D1　显示 = 按祖先索引查已生成的产物，所有显示入口都不调 ensure

点下去的树节点就是查询范围：`/model/records` 以它为祖先回答子树里已生成的全部实例（库供数直接 `anc CONTAINS`）。
眼睛（`SetVisible`）、视口工具栏、房间模型（`ShowRoomModel`）、树范围聚焦（`FocusTreeScope`）、取回工作的重装——
凡语义是「让三维里出现它」的入口一律只查不生，不持锁、不写模型库。查空就是空：树行停在「未加载」，日志一句
「范围内没有已生成的模型；右键「重新生成模型」可生成」，**不记 ERROR**。`data.rs` 的 `ModelLoad::Scopes` 臂里不再有
`ensure_model`；`Evt::ModelScopeEnsured`、「正在补齐 N 个模型」进度与 `scope_targets` 的 ensure 用途（ADR-0024）退役
——那个集合本身留下，改作 D3 重查的范围（实施时定：`/model/records` 一根一次往返，按叶子问整个 ZONE 是几千次）。

### D2　生成只由右键「重新生成模型」发起，直接调 `POST /model/generate`，生成完成之后再显示

接口改名：`/model/ensure` → **`/model/generate`**（gen-model ADR-086 D4），客户端里 ensure 这个词随之消失
（`ensure_model` → `generate_model`，`EnsureReply / EnsureStatus` → `GenerateReply / GenerateStatus`，日志里的
「核对 / 补齐」一律改成「生成」）。

菜单还是现有那一条（模型树行与视口右键共用），作用于当前选中，一次一趟（`regen_busy`）。按下去：

1. 一个不带数字的确认框：「对 /ZONE-A 等 3 项重新生成模型？范围内已有的产物会被重算替换」。
2. 对选中的目标发 `POST /model/generate { refno, force: true }`——请求体与 ensure 时一字不变（ADR-086 D4），一个
   refno 一发，选中几项就几发、顺序跑、整批回执；服务端自己解根、删旧、重算（根级原子替换），客户端不解根。
3. **生成完成之后，再调用显示接口**：按 D1 以这几个目标为祖先查 records 并装进三维（等于替用户点一次眼睛；
   回包到了先卸掉场景里同名的旧几何再装，重算过的产物不能被当成「已经有了」）。
   `timeout` 回执（服务端后台继续）不显示，日志写明「服务端后台继续生成，完成后再点眼睛」。

客户端原有的「清点 → 数字确认框 → 整片删产物 → 逐根调 → 停在这里」整条流水退役（`Req::RegenerateScope /
RegenerateDeleted / RegenerateUnit`、`regenerate::UnitOutcome` 五档、`model_regenerate::Vm::Counting / Ready(Plan)`）。
不新增接口，不在客户端解根，不在客户端删产物。

### D3　取回工作只重装

取回工作的三维半边改为「清场 → 按快照重查 records → 重装」，不再先 ensure。重查范围：服务供数以快照里**点过眼睛
的范围目标**为祖先问（一个 ZONE 一次往返；老快照没有范围记录才退到按模型 refno 逐个问），库供数照旧按模型 refno
`anc CONTAINS`。SAVEWORK 之后三维**不会自己追新**，追新只有右键「重新生成模型」——这正是要的区分。

## 为什么不是别的做法

- **保留 ADR-0009，只修那条退回 ZONE 的 fallback**：409 没了，但眼睛照样会生成、照样 120 s。修的是症状。
- **加一个只解根不生成的接口，客户端拿根清单逐根 ensure**（本文第一稿）：用户否了——生成接口已经会解根，再加接口是多余的；
  显示那边有 `anc` 索引，也不需要根清单。
- **树行加「未生成 / 已过期」两档数据态**（本文第一稿）：要靠服务端在回执里报每根状态与凭证，随上一条一起撤。
  显示查空就是未加载，日志提示右键；过期与否由人判断、人右键。
- **取回工作保留 ensure**：「生成只在右键」就不是真的，整 SITE 的取回工作也仍会撞 120 s。

## 后果

1. 眼睛从此只是一次查询；「WARN 超时 + ERROR 查询失败 + 空白三维」那组现场不再出现。
2. SAVEWORK 之后三维不再自动追到文件最新；要人右键。界面上没有「已过期」提示，这是有意的取舍（见上）。
3. 右键对大范围（整 ZONE）一发 generate 仍受服务端 120 s 预算约束：超时后服务端后台继续，客户端等它跑完再点眼睛。
   逐根进度与「停在这里」随旧流水一起没了——这是 G1 选 A 的代价；要改只能改服务端的生成预算或作业化，不在本文范围。
4. `FocusTreeScope`（原注释 ensuring missing geometry once）与 `ShowRoomModel` 在范围没产物时显示不出东西，不再悄悄生成。
5. ADR-0026 的测试 `a_failed_ensure_still_queries_the_models` 失去对象，换成「显示路径不含 generate」的源码钉。
6. 内存投影路由的服务端在 ADR-086 落地前，对没生成过的范围仍回 409：客户端把它当查空处理（同一句日志），不记 ERROR。
7. 新客户端对老服务端：`/model/generate` 404 时退回 `/model/ensure` 同一个请求体，日志点名一次「模型服务是旧版」。

## 实施约束

1. 源码钉：`data.rs` 的 `ModelLoad::Scopes` 与 `Replace` 臂里不出现 `generate_model` / `ensure_model`
   （测试 `display_never_generates`）。
2. 显示查空（含老服务端 409 `not_generated`）走 INFO 提示，不走 ERROR（测试 `an_empty_scope_is_a_hint_not_an_error`）。
3. 右键：确认之后恰好一发 `generate(force = true)`，成功回执之后对同一批目标发显示（测试
   `regenerate_generates_once_then_displays_the_targets`）；`timeout` 回执不发显示（测试 `a_timed_out_generate_does_not_display`）。
4. 取回工作重装不含 generate（测试 `get_work_reloads_without_generating`）。
5. `regeneration_is_not_a_scene_action` 那条守卫不动：生成仍不进 `ModelAction`。
6. 源码里不再出现 `ensure_model` / `EnsureReply` / `EnsureStatus` / `/model/ensure`（只允许出现在后果 7 的旧服务端回退分支与
   ADR 文本里）。
7. CONTEXT.md 新增「显示」「生成」「重新生成」，改写「取回工作」。

## 2026-10-10 修订

### D4　清理也是右键动作，只删产物、不重算

显示仍只查（D1），生成仍只在右键「重新生成模型」（D2）。清理是第三条，与重新生成共用确认框和 `regen_busy`，不进 `ModelAction`。

**范围清理。** 模型树与视口右键，「重新生成模型」正下方「清理模型」。确认之后对每个选中目标一发 `DELETE /model/subtree`（gen-model spec §4.5.1，`confirm` 等于该 refno）。成功：本端按缓存把这个范围里已加载的模型卸掉，树行回「未加载」，不重查 records。卸载范围 = 目标自己，加上半链碰到目标的已知范围键的全部成员，加上半链碰到目标的已加载叶子。半链先走 `tree.parent`，缺的一跳用几何行装进来时的 owner 补；碰到目标即算在范围内，不必走到 SITE。链在碰到目标之前断了的叶子不动——分不清它在哪个 ZONE 下，猜成在范围内会把别处正看着的模型一起卸掉。409（范围内有根正在生成，或同库有整库任务）不卸载。超时不等于没删：摘掉这个目标的范围缓存、标成待重查，日志让人点眼睛；eye 仍只读渲染回执，不提前写成「未加载」。

**整库清理。** 任务队列库行，「重算模型（整库）」右侧「清理模型（整库）」。确认之后一发 `DELETE /dbnums/{n}/model`（`confirm` 等于路径 dbnum）。成功：按回执里的 `ref0s` 卸该库已加载的模型；回执缺这格则只记日志，不靠猜卸载。不自动排「重算模型（整库）」，日志把这条路指出来。本机已经有一趟重新生成或清理时这一枚是灰的。开确认框和真正发出 DELETE 之前都过执行闸门（与「重算模型（整库）」同一道）。回执带帧号，对不上当前这一趟就丢掉，不关掉此时新开的确认框。

不删设计元素，不删共享单位网格。清库（`DELETE /dbnums/{n}/data`）是数据面，不是这一条。
