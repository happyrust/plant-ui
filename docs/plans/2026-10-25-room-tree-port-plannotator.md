# 房间层级树（plant3d-web「房间」页签）移植 plant-ui 开发计划

日期：2026-10-25；状态：**阶段一至三已完成**（用户 2026-10-24 23:5x「将 ../plant3d-web 里的房间树的显示功能，也移植到 ../plant-ui」，
2026-10-25「开始实现」；按 D1–D9 推荐项实施，ADR-0033，证据 `docs/evidence/2026-10-25-room-tree-port.md`）。与计划的出入见文末「进度」。

## 当前基线

仓库：`D:/work/plant-code/old/plant3d-web`（来源，一字不改）+ `D:/work/plant-code/old/plant-ui`（落点）。服务端是 gen-model（路由已在线，不动）。

### 来源侧：plant3d-web 已经做成、这次照搬口径的东西

「房间树」是模型树面板里的「房间」页签（ADR 0068；计划 `docs/plans/2026-09-20-spatial-room-hierarchy-tree-plan.md`、`2026-09-21-model-tree-room-tab-pr-d-closeout-plan.md`、`2026-09-21-room-tree-tube-segments-plan.md`）：**在册房间平铺一层 + 顶部搜索框，展开一间房是服务端一次聚合的 房间 → 专业 → 最小交付单元类型 → 单元 → 构件（+ BRAN 单元下的直段）**，不属任何单元的构件归专业下的「其他构件」按 noun 分。

| 文件 | 内容 | 要搬的核心 |
|---|---|---|
| `src/api/genModelV1Api.ts`（§「房间层级树」）、`src/api/genModelSpatialApi.ts` | 两条只读路由：`GET /api/v1/spatial/rooms`（在册房间清单：`refno a_b` / `room_num` / `name` / `dbnum` / `panel_count` / `panel_refnos?`，外加 `status` ready / degraded / initializing / disabled / unsupported / failed 与 `reason`）；`GET /api/v1/spatial/rooms/{a_b}/tree?tubes=1`（一间房的树：`rooms[0].specs[].unit_types[].units[]{refno, noun, name, count, tube_count, min_distance, elements?[], tubes?[]}` + `specs[].others.by_noun[]{noun, count, min_distance, elements?}`；叶子 `{refno, noun, distance, shared_rooms?}`；直段 `{ordinal, from, to, from_noun, to_noun, distance, length, aabb{min,max}, invalid, shared_rooms?}`；`leaves_inline` / `leaf_cap` 5000 / `leaf_count`，超上限时 `elements` 缺、按 `unit=a_b` 或 `other_noun=NOUN` 再取一组（响应 `inlined` 说是哪组）；`total_count` / `total_tube_count` / `tubes_truncated` / `delivery_unit_types` / `room_status` / `warnings`）。房间没生成过面板模型 → 404；老服务端不认 `tubes=1` 就没有 `total_tube_count`（前端按有没有这些键画） | 全部类型与两条路由照搬成 serde 结构；`tubes=1` 恒带 |
| `src/composables/roomTreeNodes.ts` | 节点模型与标签：节点 id `room:<a_b>` / `spec:<room>:<spec>` / `type:<room>:<spec>:<noun>` / `unit:<room>:<a_b>` / `others:<room>:<spec>` / `othernoun:<room>:<spec>:<noun>` / `elem:<room>:<a_b>` / `tube:<room>:<unit>#<from>-<to>#<ordinal>`；房间行「`room_num` · 名 · N 项 · M 段直管」、单元行「名 · N 项 · M 段直管」、构件行「noun · refno · 距离」、直段行「直管 · from_noun → to_noun · 长度（· 无效）（· 跨 N 房）」；`refnosUnder` / `branUnitRefnosUnder` / `pendingLeafNodesUnder`（直段行不进任何 refno 集） | 层次、标签、三个收集器照搬；id 改成 Rust 枚举 |
| `src/composables/useRoomTree.ts` | 状态机：根清单按 `room_num` 排、搜索按 `room_num` / `name` 本地过滤；展开一间房才取树（同房并发去重），取过的缓存到「刷新」；单元 / 其他 noun 的叶子缺时展开到它再按组补取；点选 / Ctrl / Shift 选行并写全局选中；勾选态（眼睛）按节点记、父节点按子节点重算（全勾 / 半勾 / 全不勾）；`setVisible` 按节点下的 refno 整批显隐，**单元级及以上连带所属 BRAN 整条**（方案 A，ADR 0068 追记）；直段行的眼睛是逐段对象级（T4，对象没装进场景就提示先加载所属 BRAN）；`flyTo`（构件按场景里已装几何的并集盒，直段按它自己的 `aabb`）；`isolateXray`（节点下构件 + BRAN 整条隔离，房间面板 X-Ray）/ `clearXray`；`revealRefno`（外部选中联动：只在已取过树的房下展开并滚到可见，不为它去拉没展开的房）、`revealRoom`（关联房间浮窗「在房间页签查看」） | 状态语义照搬；显隐 / 聚焦 / 隔离落到 plant-ui 已有的 `ModelAction` |
| `src/components/model-tree/RoomTreePanel.vue` | 页签壳：搜索框 + 刷新钮 + 取树失败的节点逐条列在树上方；行沿用 `ModelTreeRow`（箭头 / 图标 / 标签 / 眼睛 / 勾选）；右键：聚焦 · 隔离 · 取消隔离 · 显示 / 隐藏 · 加载模型（> 200 个先确认）· 查看属性 · 查看关联房间；直段行只有聚焦 / 查看属性（看所属 BRAN）/ 显示隐藏；页签不在前台不拉数据、不动场景 | 菜单项与门槛照搬 |

### 落点侧：plant-ui 现状

- 窗体是 egui + egui_dock（`crates/plant-ui/src/workbench/panes.rs::Pane`：模型树 / 三维视图 / 命令行 / 日志 / 属性 / 房间 / 虚拟孔洞 / 任务队列 / 增量记录），每个面板本身就是一个 dock 页签；模型树（`workbench/tree.rs`）是虚拟滚动的 `tree_row_ui`（深度 / 图标 / 标签 / meta / 选中 / 主选 / 可展开 / 眼睛 / 标签片），行 id 按 refno 定死；点箭头展开、点眼睛只切可见性、点行选中（Ctrl / Shift）、右键落在选择集外才重设选中。
- 契约层 `crates/plant-ui`（只画、只发 `Cmd`）↔ 宿主 `crates/plant-ui-app`（数据线程、三维、命令处理）。已有可直接复用的命令：`Cmd::SelectElement` / `SetSelection` / `LocateElement`（树定位）/ `FocusRoom`（隔离 + 取景 + 房间页签详情）/ `ShowRoomModel` / `ShowRooms(refno)`（关联房间）/ `OpenRoomBrowser`；`Cmd::Model(ModelAction::{SetVisible{refnos, visible}, Unload, SetXRay, HideAll, ShowAll, Focus, FocusBounds{min_mm,max_mm}, FocusGroup{refnos}, Isolate{..}, ExitIsolate, FitAll})`。
- 供数模式（ADR-0026）：服务供数走 gen-model HTTP（`plant-ui-app/src/model_update_api.rs` 的 `ehttp` `get` / `post` / `delete`，两端同一套）；库供数直连 SurrealDB（`plant-ui-data`）。房间数据现在走 `plant-ui-data::room`（`rooms_overview` 全表、`room_detail` 单间详情、`tube_carriers_of` = 成员里的管件连带所属 BRAN，即方案 A 的同一个概念），是 SurrealDB 查询，不是层级树。
- 已有的房间功能：「房间」面板（`workbench/room.rs`：一间房的横幅 / 摘要 / 关联 / 组成 / 成员预览）、房间浏览器浮窗（`room_browser.rs`：全表 + 检索 + 四态筛选，行操作只有「聚焦」）、房间视图（管身截断，ADR-0028）、模型树右键的房间菜单段。**没有**房间 → 专业 → 单元 → 构件的层级浏览，也没有任何 `/api/v1/spatial/*` 调用。
- 三维可见性是 refno 级（`SetVisible{refnos}`，实际可见性由 View3d 回执挂在行上）；隐式管身 TUBI 挂在 BRAN 的 refno 下，没有「按（BRAN, from, to, ordinal）只显 / 隐一段」的对象级开关。
- 独立壳 / 模拟引擎（`plant-ui-app/src/sim.rs`，`PLANT_UI_SIM=1`）供设计验收截图，不要求 gen-model 在场。

## 决策摘要（推荐项在前）

| 编号 | 决策 | 推荐与理由 |
|---|---|---|
| D1 落点 | (a) **新增 dock 面板「房间树」**（`Pane::RoomTree`），缺省与「模型树」同一侧、同一组页签；(b) 在「模型树」面板里自画 PDMS / 房间两个子页签；(c) 并进现有「房间」面板 | **(a)**：plant3d-web 把它做成模型树里的子页签是因为那边的 dock 面板不是页签；plant-ui 的面板本身就是 dock 页签，再套一层子页签是两套页签样式。「房间」面板是**一间房的详情**，和浏览全部房间的树是两件事，并进去会把详情挤掉 |
| D2 供数 | (a) **只走服务供数**（gen-model `/api/v1/spatial/rooms` + `/rooms/{a_b}/tree?tubes=1`），库供数下面板显「房间树只在服务供数下有」的空态注释；(b) 库供数用 SurrealDB 自己聚合一棵 | **(a)**：ADR 0068 的树是服务端聚合的（单元根派生、跨房去重、直段补全），前端再拼一遍是 ADR 0068「Considered Options」里否掉的那条路；ADR-0026 要求库供数缺的格要标出来、不许安静空着，所以是一条明白的注释而不是灰页 |
| D3 树形与文案 | 层次、节点标签、计数尾巴（「N 项」「M 段直管」「跨 N 房」「无效」）、距离格式、「其他构件」分组、按 `room_num` 排序、搜索按 `room_num` / `name` **全部照搬**；行沿用 `tree_row_ui`（计数尾巴走 `meta`，「跨 N 房」「无效」走 `tags`） | 这是「显示功能」本身，不重新设计；两端看到的是同一棵树 |
| D4 显隐（眼睛） | (a) 构件 / 分组行的眼睛 = `ModelAction::SetVisible{refnos}`，单元级及以上连带树里的 BRAN 单元 refno（方案 A；树里 BRAN 单元是显式一层，不用再查属主）；行上的眼睛谁来画：照模型树的规矩**读 View3d 的实际可见性回执**（叶子按 refno，父节点按子节点聚合成 全 / 半 / 无），不另记一份勾选表；**直段行第一版不画眼睛**（只读 + 聚焦，即 plant3d-web 的 D5 (i)）；(b) 照 plant3d-web 自维一份勾选表；(c) 第一版就做逐段眼睛 | **(a)**：plant-ui 的房规是「eye 只读实际可见性」（CONTEXT.md「实际可见性」），另记勾选表会和模型树的眼睛说两套话；逐段眼睛要 View3d 长出「按 (BRAN, from, to, ordinal) 只显 / 隐一段」的对象级开关 + `model/records` 的 `tube{ordinal, from, to}` 元数据对上实例，是另一块能力，单立阶段四 |
| D5 右键菜单 | 照搬：聚焦（构件 / 分组 → `FocusGroup{refnos}`，直段 → `FocusBounds` 用它的 `aabb`；没有已装几何时提示先加载）· 隔离（`Isolate{refnos}` 含 BRAN 连带）· 取消隔离（`ExitIsolate`）· 显示 / 隐藏 · 加载模型（= 显示，> 200 个先弹确认，沿用清理模型那种确认框）· 查看属性（`SelectElement` + 聚焦属性面板）· 查看关联房间（`ShowRooms(refno)`）；房间行多「聚焦房间」（`FocusRoom`）与「显示房间模型」（`ShowRoomModel`）；直段行只有聚焦 / 查看属性（看所属 BRAN） | plant3d-web 的隔离是「隔离构件 + 房间面板 X-Ray」，plant-ui 已有 `FocusRoom` 做同一件事；节点级隔离用 `Isolate` 即可，不再另造 |
| D6 选中联动 | 树内点选写全局 `Selection`（单点 / Ctrl / Shift 与模型树同手势，直段行选中 = 选中所属 BRAN）；外部选中变了、面板在前台时，若 refno 在**已取过树的房**下就展开到它并滚动到可见（不为它拉没展开的房）；「关联房间」浮窗加一条「在房间树查看」（`Cmd::RevealRoomInTree(refno)`：取树、展开、滚到） | 与 plant3d-web 收口计划 P3-b 同口径 |
| D7 取数与缓存 | 面板第一次成为活动页签才拉根清单；展开一间房才取那间房的树，取过的缓存到「刷新」或切项目；单元 / 其他 noun 的 `elements` 缺时展开到它再按 `unit=` / `other_noun=` 补取；取树失败的节点原因列在树上方；`status` 非 ready / degraded 整面板显 `reason`（含老服务端 `unsupported`） | 照搬；全部在宿主数据线程做，契约层只拿展平好的行 |
| D8 两端 | PC 先跑通并验收；HTTP 走 `ehttp`（与 `model_update_api.rs` 同一套），从第一行起按两端共用写；Web 端只做浏览器验收 | 与 E3D 外观移植同一条纪律 |
| D9 非目标 | 空间查询抽屉（`nearby/tree`）、逐段眼睛（阶段四单立）、版本对比入口、模型树本身的改动、gen-model / plant3d-web 一字不改 | — |

## 实施阶段

### 阶段一：契约与数据（不动 UI）

1. `plant-ui-app/src/spatial_api.rs`（新）：两条路由的 serde 结构（照来源表格，一字不增减）与 `rooms()` / `room_tree(a_b, only: Option<LeafSelector>)`（恒带 `tubes=1`）；404 → 「这间房还没生成面板模型」；`success:false` + `unsupported` → 整面板注释。
2. `plant-ui/src/room_tree.rs`（新，纯逻辑）：`RoomNodeId` 枚举（Room / Spec / UnitType / Unit / Others / OtherNoun / Element / Tube）、`RoomTreeNode`（kind / label / meta / tags / refno / count / tube_count / children / leaves_pending）、`flatten_room_tree()`（照 `flattenRoomTree`）、`refnos_under` / `bran_units_under` / `pending_leaf_nodes_under`、根排序与过滤、展平成行；单测照 plant3d-web `roomTreeNodes.test.ts` / `useRoomTree.test.ts` 的用例改写（标签、层次、跨房标、直段不进 refno 集、叶子缺时的待补标记）。
3. 契约：`vm::RoomTreeVm`（Idle / Unsupported(reason) / Loading / Ready{rows, roots_total, filter, errors} / Failed）、`RoomTreeRowVm`（id / depth / label / meta / tags / expandable / visibility / selected / loading）；`Cmd::RoomTree(RoomTreeAction::{Load, Refresh, SetFilter, ToggleExpand(id), Select(id, modifiers), SetVisible(id, bool), Focus(id), Isolate(id), LoadModels(id), ViewProps(id), ViewRooms(id)})` 与 `Cmd::RevealRoomInTree(refno)`。

### 阶段二：面板

4. `Pane::RoomTree`（图标 `ph::TREE_VIEW`，标题「房间树」），缺省布局跟在「模型树」后面同一组；`workbench/room_tree.rs`：顶部搜索框 + 刷新 + 在册数，失败条，虚拟滚动的 `tree_row_ui` 行（行 id 按 `RoomNodeId` 定死），点箭头 / 眼睛 / 行 / 右键与模型树同手势；库供数 / unsupported / 非 ready 的注释态。
5. 宿主：数据线程按 `RoomTreeAction` 取数与缓存（按房间 refno 的树缓存、在途去重、叶子补取合并），每次结构变化重新展平行；眼睛读 View3d 实际可见性聚合；显隐 / 聚焦 / 隔离 / 加载转成现有 `ModelAction`（含 BRAN 连带）；> 200 确认框；选中联动与 `RevealRoomInTree`。
6. 「关联房间」浮窗加「在房间树查看」；`sim.rs` 加一份小剧本（两间房、两个专业、BRAN 单元带直段、一组叶子超上限）供独立壳截图。

### 阶段三：验收（PC）

7. 单测全绿；独立壳（`PLANT_UI_SIM=1`）截图：根清单 / 展开一间房的五层 / 其他构件 / 直段行 / 搜索 / 右键菜单 / 库供数注释态；连本机 gen-model（ams7997 或 ams8000 那套有房间定义的库）对着 plant3d-web 同一间房逐行核：节点数、计数尾巴、直段条数与 `verification/spatial-room-hierarchy-tree-2026-09-20/http/*.json` 一致；显隐 / 聚焦 / 隔离在三维里的结果截图。证据进 `docs/evidence/2026-10-25-room-tree-port.md`，ADR 一条（「房间树是服务端聚合的、只在服务供数下有」），`CONTEXT.md` 补词条「房间树」「最小交付单元」「其他构件」。

### 阶段四（单立；2026-10-25 已做，见「进度」末两条）：直段逐段眼睛与 Web 端

- View3d 按 `model/records` 的 `tube{ordinal, from, to}` 登记 `(BRAN, 段) → 实例`，加 `set_tube_segments_visible(keys, visible)`；对象没装进来提示先加载所属 BRAN；直段行长出眼睛。
- wasm 构建、浏览器验收。

## 风险

- 树路由依赖房间体制就绪（房间面板模型已生成、空间索引建好）；`status` 非 ready 时只能显原因，这与 plant3d-web 一致。
- 「孤直段」（管件全在范围外、只有直段穿过）列不出——服务端已知漏洞，照样带过来。
- 眼睛读实际可见性：没加载的构件在树里是「未加载」态（与模型树同），不会像 plant3d-web 那样缺省打勾；这是 D4 (a) 的有意之为。

## 进度（2026-10-25）

阶段一至三按计划落地，与计划的出入：

- **契约类型的位置**：两条路由的 serde 结构放在 `crates/plant-ui/src/room_tree.rs`（而不是计划写的 `spatial_api.rs`）——绘制层的
  `flatten_room_tree` 要拿响应展平，依赖方向只能是 app → plant-ui。`spatial_api.rs` 只剩取数、路径拼接与错误分型（复用 `model_update_api` 的
  `get` / `urlencode` / `failure_of` / `endpoint_missing`，两个改成 `pub(crate)`）。
- **文案唯一的偏差**：来源写「M 段直管」「TUBI REDU → BEND」，这边按 `CONTEXT.md`（界面上的词是「直段」，TUBI / 直管段 / 管身不上界面）
  写「M 条直段」「直段 · 141 mm」；计数、层次、距离格式不变。
- **不可用态带「重试」**：D2 写的是一句注释；实施时发现房间体制会从「正在就绪」走到就绪，注释态没有出路就只能切页签，所以注释下照样摆一枚「重试」。
- **「加载模型」的取景**：计划没说清什么时候取景；实施按 `pending_room_frame` 的同一理由等全部目标的范围查询回来再 `FocusGroup`（范围没回来前视口没有它们的包围盒）。
- **隔离在房间视图里**：照来源 `exitRoomView({ keepIsolation })`，先摘徽章 / 撤房间体再隔离这一支。
- **树内选中写全局选中的口径**：只把能落到 refno 上的行（构件 / 单元 / 直段 → 所属 BRAN）写进 `Selection`，主选中 = 刚点的那一行；只选了分组行全局选中不动
  （清掉属性面板只会让人以为点空了）。来源是「只有单选一行才写全局」，这边 `Selection` 本来就是多选集，不必缩成单选。
- **外部选中联动的「在前台」**：`WorkbenchState::is_pane_active(Pane)`（egui_dock `find_tab` + 所在叶的 `active`），不是 `active_pane()`（那个是拿焦点的那一格）。
- **模拟剧本**：`sim.rs` 两间有树的房 + 一间空房壳；第二条 BRAN 与 PANE 组叶子不内联（走 `unit=` / `other_noun=`），PANE 230 个让房间根的「加载模型」过 200 阈值。
- **验收里没有做的**：探针 `inspect click` 注不进修饰键状态（模型树同样），Ctrl / Shift 多选只有单测证据。
- **阶段四前半（Web）已做**（2026-10-25，用户「按推荐继续」）：release wasm + headless Chrome 连本机 gen-model，两条路由在浏览器里同样发出、同样上屏
  （证据 §3b）；仓里的 `web/public/pkg` 没重发。
- **阶段四后半（直段逐段眼睛）已做**（2026-10-25，ADR-0033 D8），与计划这一条的出入：
  - 身份不是 View3d 另记一张 `(BRAN, 段) → 实体` 的表，而是让 `/model/records` 的 `tube` 一路进场景：`plant_ui::ModelRecord`（`GeomInstQuery`
    flatten + `tube: Option<TubeSegment>`，与 gen-model 读面同形）换掉装场景这条链上的 `GeomInstQuery`（`Deref` 到它，读字段的代码不动），带身份的
    直段根挂 `TubeSegmentRoot` 组件。动作名是 `ModelAction::SetTubeSegmentsVisible { bran, segments, visible }`（走 `Cmd::Model`，不另开方法）。
  - 逐段的实际账靠 Bevy 变更检测（`track_tube_segments` 盯直段根的 `Visibility`），不在每条指令里逐一记账；回执按 BRAN 整份、等网格 settled 才发。
  - 模拟剧本补了唯一一组三维记录：B1（9001_1010）的三条直段按 records 形状给、标成无效直段（零网格文件就能画虚线），三个构件回零实例记录；
    本机没有一台带非空房间树的 gen-model，逐段眼睛的端到端证据在独立壳里（证据 §3c）。
  - 没做：逐段结果不进取回工作的隐藏回放、隔离快照仍按 refno（ADR-0033 D8「不做」）。
