# 取回工作 · 清场重装已加载集 · 开发计划

- 日期：2026-08-19
- 范围：`plant-ui-app`（宿主，`src/main.rs`）与 `plant-ui-view3d`（仅相机取景一处）。
  `plant-ui`（绘制层）与 `plant-ui-data`（数据层）**零改动**；模型树的取回行为**不动**。
- 依据：`CONTEXT.md`「取回工作」定义（把库里此刻的样子取到界面上来，**模型树与已加载的
  三维模型都算在内**，不应用任何数据）；ADR-0016（eye 只读 View3d 实际渲染回执）。
- 状态：M1–M4 已完工（含 ADR-0021），单元测试绿；M5 真库实测待跑。

---

## 一、要改什么（一句话）

点「取回工作」的那一刻**立即清空三维场景**，然后只把**取回前场景里存在的那批模型**
（显示中 + 已隐藏）按库里此刻的样子逐个重查重建，显示 / 隐藏按快照原样回放，相机不动；
模型树照旧就地刷新（展开、选中、滚动全保留）。

## 二、现状与勘探事实

| 事实 | 位置 | 对本计划的影响 |
|---|---|---|
| 取回工作已是「就地刷树保展开 + 三维整场替换」，但替换是**回包时**原子交换：旧几何在冷查询期间一直挂在屏上冒充新数据 | `main.rs` `Evt::GetWork` 1927-2028、`Evt::Models` 1588-1634 | 改成点击即清场，替换语义保留 |
| 整场重载范围 = 全部 SITE 根，实测 59464 元素 / 213090 网格实例、十几秒冷查询；落地后**全部可见**，可见性台账作废 | `main.rs:1938-1949`、`.codex-artifacts/get-work-stale-model-fix/live-test-summary.json` | 范围改为快照里的已加载集，快慢与视野都回到「取回前的样子」 |
| `Req::Models` 全仓只有取回工作一个发送点 | `main.rs:1949`、`data.rs:205-217` | 改根集合不伤任何别的调用方 |
| `model_instances` 根类型无关：SITE / ZONE / BRAN / 叶子一律同一条 anc 索引查询，叶子 refno 直接可查；根间 8 路并发 | `plant-ui-data/src/lib.rs:73-140` | B2（按已加载集逐个重查）**不需要数据层改动** |
| `view3d.load()` 压入 `ModelBatch::Replace`：despawn 全部 `SceneRoot`、清 `desired_visibility` / `render_states` 等整场状态 | `view3d/src/lib.rs:234-238、1086-1147` | 「清空」可直接用 `load(vec![])` 表达，不新增 ModelAction |
| `should_frame_batch(replace, scene_empty) = replace \|\| scene_empty`：今天每次整场重载都会把相机拉去看全场 | `view3d/src/lib.rs:525-527` | 快照回放要求相机不动，去掉 `replace` 臂 |
| 帧循环里 `pending_models.take() → view3d.load()` 先于 `view3d_commands` 排空；`model_action(SetVisible)` 同步写 `desired_visibility`，Replace 批在 Bevy 系统里稍后落地 | `main.rs:506-516`、`view3d/src/lib.rs:276` | 同一帧「load + SetVisible(隐藏集)」时，隐藏方向在批次落地前就位，回放不闪 |
| 丢弃守卫：重载在途、后台又出现更新任务时整包丢弃、挂欠账等队列空闲 | `main.rs:1576-1587` | 点击即清场后此守卫会造成「空场景一直挂到整条队列跑完」，拆除 |
| 欠账机制（`model_reload_owed` / `auto_refresh` / `restore_model_reload`）在查询失败后队列空闲时自动补一次 FullReload | `main.rs:1072-1084、4203-4266` | 保留，只服务「查询失败」这一种情形 |
| 快照素材都在宿主手上：`loaded_models`（场景里存在的模型 refno 集）、`tree.pending_direction`（最后一次指令方向）、`tree.visibility`（实际渲染回执） | `main.rs:563-592、920-929` | 快照不需要问库，点击当帧就能拍 |
| 重入闸门 `begin_get_work` + 延后槽 `get_work_again`（忙时合并、带模型的优先） | `main.rs:962-969` | 不动；清场对第二次点击天然幂等（场景已空） |

## 三、已拍板的决定（逐题对齐记录）

| # | 问题 | 决定 |
|---|---|---|
| 1 | 改哪个工程 | plant-ui 本身（独立壳） |
| 2 | 清场动机 | 交互诚实：点下去立刻清空、看着重新加载，不让旧几何冒充新数据 |
| 3 | 模型树 | 维持现状（就地刷、保展开），本计划只动三维 |
| 4 | 在途更新任务 | 拆掉丢弃守卫，查回来一律上屏；欠账只留给查询失败 |
| 5 | 重载后可见性 | 恢复取回前的显示 / 隐藏（快照回放），不再全亮 |
| 6 | 重查范围 | 只重查取回前场景里存在的那批（显示 + 隐藏）；新增元素只进树、三维保持未加载；库里删掉的自然消失 |

决定 5、6 合起来正是 CONTEXT.md 给「取回工作」下的定义原文——刷「已加载的三维模型」，
从来没说要把全厂拉进来。现状的整场全拉属于实现偏离文档，本计划顺带把它拉回去。

## 四、改动设计

### 4.1 新状态：一份「只记第一次」的重载快照

```rust
/// 取回工作清场前的场景快照：(模型 refno, 取回前是否可见)。
/// 只记第一次（参照 view3d isolate_restore 的先例）：清场后重试、欠账补载
/// 都要按同一份快照回放；重载成功落地才消费掉。
model_reload_restore: Option<Vec<(RefU64, bool)>>,
```

- 每个 refno 的方向：`tree.pending_direction`（用户最后一次指令）优先，其次
  `tree.visibility` 的实际结果，都没有（刚加载还没回执）按可见记。
- 键集合就是 `loaded_models`——它记的正是「场景里存在实体的模型」。
- `reconnect` 时随其余重载状态一并清掉（`main.rs:3287-3303` 那一段加一行）。

### 4.2 点击即清场（`start_get_work_for_tasks`，reload_models=true 且请求发出成功后）

1. 拍快照（若 `model_reload_restore` 已有则不覆盖）。
2. 清场：`pending_models = Some(Vec::new())`——下一帧 `view3d.load(空)` despawn 全部
   场景根；同时复位宿主台账（今天挂在 `Evt::Models Ok` 里的那批复位**提前到此刻**）：
   `loaded_models`、`model_scopes`、`model_scope_pending`（并把 `model_scope_epoch` +1，
   在途范围查询回包作废）、`pending_incremental_models`、`model_show_waiting`、
   `latest_mesh_progress`、`tree.visibility`、`tree.pending_direction`、
   `tree.pending_visibility`、`tree.visibility_unavailable`。eye 随之全部回「未加载」，
   与 ADR-0016 一致：加载期间不抢说「已显示」。
3. 状态栏：`vm.model_load = Resolving("已清空三维，正在重查模型…")`。
4. 日志：`取回工作：已清空三维场景，将重查 N 个模型（其中 M 个保持隐藏）`；
   快照为空时改说 `三维本来就空着，只刷新树`。

### 4.3 重查范围（`Evt::GetWork` Ok 分支）

- `Req::Models(roots, true)` 的 `roots` 从「全部 SITE 根」改为**快照里的模型 refno 列表**。
- 快照为空：不发模型请求，`model_reload_in_flight` 落回 false，busy 按 reload=false
  的既有路径收尾（`set_get_work_busy(false)` + `run_deferred_get_work()`）。
- 树侧一个字不改：roots 替换、分支就地换、`prune_unreachable`、消失节点发
  `ModelAction::Unload`（场景此刻已空，Unload 落空无害）。

### 4.4 守卫拆除与快照回放（`Evt::Models`）

- **删掉** `Evt::Models(true, _) if model_reload_in_flight && !model_reload_ready` 整臂
  （main.rs:1576-1587）：查回来一律上屏。上屏那一刻若又有新保存落库，后续走今天
  已有的「自动刷新只换树 + 日志提示再点取回工作」，行为闭环不变。
- Ok 分支：装账照旧（`loaded_models` / `model_scopes` 按回包重建、
  `pending_models = Some(models)`），随后**回放**：取快照（消费掉），对
  `快照隐藏集 ∩ 回包模型` 调 `set_model_visible(hidden, false)`——走既有机器记
  pending 方向、发 `SetVisible`；帧循环里 load 先行、SetVisible 后至但先于批次落地，
  隐藏的模型直接以 Hidden 出生，不闪一下再消失。
- Ok 日志追加回放口径：`三维模型已就绪：N 个元素，M 个网格实例（回放隐藏 X 个）`。
- Err 分支：欠账恢复照旧（队列空闲时 `auto_refresh` 自动补，快照仍在、补载后照样回放）；
  日志明说现状与出路：`三维模型查询失败：场景已清空，队列空闲时自动重试，也可再点「取回工作」`。

### 4.5 相机不动（view3d）

- `should_frame_batch` 去掉 `replace` 臂，只在**场景原本为空**时取景：

```rust
fn should_frame_batch(scene_empty: bool) -> bool {
    scene_empty
}
```

- 清场 Replace 落地后会留下一个空 `SceneRoot`，随后的重载 Replace 见到的
  `roots.is_empty() == false`，于是不取景——相机停在用户原来的位置，与「恢复取回前
  的样子」一致。首次通过 eye 加载（Append 到空场景）的自动取景行为不变。
- 既有测试 `first_incremental_batch_frames_an_empty_scene` 只断言 Append 两种情形，
  按新签名微调后语义不变；补一条「Replace 不再取景」的断言。

### 4.6 文档

- `CHANGELOG.md` 未发布节新增「取回工作」小节，写界面上会不一样的四件事：
  点下去立刻清空三维、只重查取回前场景里已有的模型（快了一个量级）、显示隐藏取回后
  原样恢复且相机不动、后台有更新任务时结果不再被丢弃。
- `CONTEXT.md` 不动——新行为恰好落回「取回工作」词条原文。
- ADR-0021（取回工作清空三维并重装已加载集），风格照 ADR-0016。

## 五、施工顺序与测试

| 步 | 内容 | 测试 |
|---|---|---|
| M1 | view3d：`should_frame_batch` 收窄 | 改 `first_incremental_batch_frames_an_empty_scene`，补 Replace 不取景断言 |
| M2 | 宿主：快照 + 清场 + 重查范围改快照集 + 守卫拆除 + 回放 | 新纯函数测试：快照方向取值（pending 优先于实际）、只记第一次、空快照跳过模型请求、隐藏集 = 快照隐藏 ∩ 回包、失败保留快照；既有 `auto_refresh` / `restore_model_reload` / `begin_get_work` / `get_work_reports_removed_subtree_and_clears_its_ui_state` 全部保持绿 |
| M3 | 日志与状态栏文案（§4.2 / §4.4 措辞） | 随 M2 用例断言 |
| M4 | CHANGELOG（+ 可选 ADR-0021） | — |
| M5 | 实测验收（照 `.codex-artifacts/get-work-stale-model-fix` 的手法，AvevaMarineSample 真库） | 见下 |

M5 验收清单：

1. 显示两个 ZONE、单独隐藏其中一根管，记相机位置。
2. E3D 里改一处几何并保存 → 模型更新应用 → 点「取回工作」。
3. 断言：点击当帧场景清空、eye 全回未加载；数秒内（而不是十几秒）模型回来；
   改过的几何是新的；被隐藏的管回来仍是隐藏；相机没动；新建元素只出现在树上、
   三维保持未加载；E3D 删掉的元素树上和场景里都没了。
4. 更新任务运行中点「取回工作」：结果照样上屏，不再空等队列。
5. 断网 / 停库重试一次：场景空 + 红字 + 队列空闲后自动补载并回放隐藏。

## 六、风险与边界

- **重查在途用户点 eye**：`view3d.load()` 会清掉还没落地的 Append 批——与今天的整场
  重载同款竞态，不因本计划扩大；重载期间的 eye 点击排进 `model_scopes` 新纪元后自然重查。
- **容器行 eye**：取回后 `model_scopes` 只剩回包模型的自条目，容器行要等下次范围查询
  才重新聚合——与今天行为一致，不新增回归，不在本计划内治。
- **快照可能上千 refno**：anc 查询逐根两条、8 路并发、索引命中；对照全场 59k 根仍是
  数量级缩减。M5 记录实测秒数。
- **清场后失败**：空场景是诚实态——库里的样子此刻未知，旧几何本来就不可信；红字 +
  自动重试兜底，绝不留「点了没反应」。
