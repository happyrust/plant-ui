# 房间树是服务端聚合的，只在服务供数下有

状态：**已接受**（2026-10-25；用户 2026-10-24「将 ../plant3d-web 里的房间树的显示功能，也移植到 ../plant-ui」，计划 D1–D9 按推荐项实施）。
计划：`docs/plans/2026-10-25-room-tree-port-plannotator.md`。来源：plant3d-web 模型树「房间」页签（其 ADR 0068；`src/composables/roomTreeNodes.ts` /
`useRoomTree.ts` / `src/components/model-tree/RoomTreePanel.vue`），一字不改。验证：`docs/evidence/2026-10-25-room-tree-port.md`。

## 背景

plant3d-web 有一棵按房间浏览模型的树：在册房间平铺一层，展开一间房是 房间 → 专业 → 最小交付单元类型 → 单元 → 构件
（BRAN 单元下还列直段），不属任何单元的构件归专业下的「其他构件」。那棵树是 gen-model 服务端两条只读路由聚合好的
（`GET /api/v1/spatial/rooms`、`GET /api/v1/spatial/rooms/{a_b}/tree?tubes=1`）：单元根派生、跨房去重、直段补全、叶子上限都在服务端。

plant-ui 已有的房间功能（「房间」页签、房间浏览器、房间视图）都是「一间房」或「全表」，没有层级浏览；也没有任何 `/api/v1/spatial/*` 调用。
要定的不是树长什么样（那是来源侧的事，照搬），而是这棵树在 plant-ui 里**落在哪、数据从哪来、显隐怎么说话**。

## 决定

### D1　单立一个 dock 面板「房间树」，跟在模型树后面同一组

`Pane::RoomTree`，缺省与「模型树」同一侧、同一格页签。plant3d-web 把它做成模型树里的子页签，是因为那边的 dock 面板不是页签；
plant-ui 的面板本身就是 dock 页签，再套一层是两套页签样式。「房间」面板是**一间房的详情**，和浏览全部房间的树是两件事，不并。

### D2　只走服务供数；库供数下是一句明白的注释

树只从 gen-model 的两条路由来（`plant-ui-app/src/spatial_api.rs`，走与 `model_update_api` 同一套 `ehttp`、同一台服务、同一套错误包封），
不进读面。前端用 SurrealDB 再拼一遍是来源 ADR 0068「Considered Options」里否掉的那条路。库供数下宿主不发请求，面板显
「房间树只在服务供数下有…」（ADR-0026：缺的格要标出来，不许安静空着）；老服务端没有路由、房间体制未就绪（`status` 非 ready / degraded）
同样是注释态而不是故障，带一枚「重试」。

### D3　层次、文案、计数、排序、过滤照搬；只换一个词

节点身份改成 Rust 枚举 `RoomNodeId`（同一构件在两间房下各有一个节点，与来源一致）；行沿用 `tree_row_ui`，计数尾巴走 `meta`，
「跨 N 房」「无效」走 `tags`。唯一的文案偏差：来源写「M 段直管」「TUBI …」，这边按 `CONTEXT.md` 写「M 条直段」「直段 · 长度」
（TUBI / 直管段 / 管身都不上界面），计数和层次不变。

### D4　眼睛读三维的实际可见性，不另记勾选表

构件 / 分组行的眼睛 = `ModelAction::SetVisible{refnos}`，单元级及以上连树里的 BRAN 单元 refno（来源方案 A；直段挂在 BRAN 自己名下）。
行上画的是 View3d 的实际可见性回执（叶子按 refno 走 `TreeModel::visibility_for`，父节点按子节点聚合成 全 / 半 / 无，BRAN 单元另算上它自己），
与模型树同一条房规（ADR-0016）；另记一份勾选表会和模型树的眼睛说两套话。没加载的构件在树里是「未加载」，不像 plant3d-web 那样缺省打勾——
这是有意为之。直段行的眼睛第一版没画（只读 + 聚焦），阶段四补上，见 D8。

### D5　右键落到既有动作上，不另造

聚焦（构件 / 分组 → `FocusGroup` 只算已加载的；直段 → `FocusBounds` 用它自己的 `aabb`）· 隔离（`Isolate{RefnoOrOwner}`，含 BRAN 连带）· 取消隔离 ·
显示 / 隐藏 · 加载模型（= 显示节点下全部构件，> 200 个先弹确认，落地后取景）· 查看属性 · 查看关联房间；房间行多「聚焦房间（房间视图）」
（`FocusRoom`）与「显示房间模型」（`ShowRoomModel`）；直段行只有聚焦 / 查看属性 / 查看关联房间（看所属 BRAN）。模型动作吃 `live` 门禁。

### D6　选中双向联动，不为它拉没展开的房

树内点选（单点 / Ctrl / Shift，与模型树同手势）把能落到 refno 上的那部分写全局 `Selection`（直段行 = 所属 BRAN；只选了分组行全局选中不动）；
外部选中变了、面板在前台时，若 refno 在**已取过树的房**下就展开到它并滚到可见。「房间」页签多一枚「在房间树查看」（`Cmd::RevealRoomInTree`）。

### D7　取数按需、缓存到刷新；全部在宿主做

面板第一次画出来才拉根清单；展开一间房才取那间房的树，取过的缓存到「刷新」或换接入点；单元 / 其他 noun 的 `elements` 缺时展开到它再按
`unit=` / `other_noun=` 补取；显隐 / 聚焦 / 隔离 / 加载先把没内联的组补齐（最多 64 组）再做；取树失败的节点原因列在树上方。
状态机（`plant_ui::room_tree::RoomTreeModel`）不做 IO，绘制层只拿展平好的行。

### D8　直段逐段眼睛：直段以身份进场景，按身份点名，整根指令盖过逐段（阶段四，2026-10-25）

直段没有自己的 refno：它以所属 BRAN 的 refno 进场景，按 refno 的显隐一动就是整根。gen-model `/model/records` 每条记录本来就带着
`tube{ordinal, from, to}`（与房间树路由的 `tubes[]` 同一套编码），plant-ui 从前把它丢了。这次让它一路进场景：

- **契约**：`plant_ui::ModelRecord { model: GeomInstQuery (flatten), tube: Option<TubeSegment> }` 与 gen-model 读面的 `ModelRecord` 同形，
  `/model/records` 的每一页按它解；库供数与老服务端没有 `tube`，记录就只按 refno 认，与从前一样。去重键（`model_record_union`）连身份一起算。
- **场景**：带身份的直段根挂 `TubeSegmentRoot`；新动作 `ModelAction::SetTubeSegmentsVisible { bran, segments, visible }` 只动点名的那几根；
  逐段的实际账由 `track_tube_segments` 盯着直段根的 `Visibility` 变更回写——不问是哪条指令改的，所以整根的 `SetVisible` / `HideAll` /
  `ShowAll` / 隔离与逐段的指令记的是同一张账；回执 `take_tube_render_states` 按 BRAN 整份、等这根的网格有了结果才发（与 refno 级回执同一条纪律）。
  逐段藏光了，BRAN 自己的 refno 级实际账也记成不可见（模型树里这根的眼睛跟着暗，再点就是整根显示回来）。
- **房间树**：直段行的眼睛按 (所属 BRAN, 身份) 读逐段账，点它只动这一段；已装进场景的直段计入所属 BRAN 单元的 全 / 半 / 无 聚合，没装的
  （老服务端没给身份）不计，免得整根已显示被说成半可见。段还没以身份进场景时点眼睛只是一句提示（先显示所属 BRAN / 这批记录没带身份），
  不记方向。右键菜单的「显示 / 隐藏」对直段行也开放，隔离与加载仍不摆。
- **不做**：逐段的显隐不进取回工作的隐藏回放快照，隔离一进一出也按 refno 整根恢复——两处快照都是按 refno 记的，逐段结果会被整根盖回去；
  这是「整根指令盖过逐段」的同一句话，不另开一套按段的快照。

## 后果

- gen-model / plant3d-web 一字不改；两端看到的是同一棵树。
- 「孤直段」（管件全在范围外、只有直段穿过）列不出——服务端已知漏洞，照样带过来。
- 房间体制未就绪时只能显原因，与 plant3d-web 一致。
- 阶段四（Web 端浏览器验收、直段逐段眼睛）已做；三维场景的模型记录从 `GeomInstQuery` 换成 `plant_ui::ModelRecord`（`Deref` 到前者），
  读字段的代码不变，只有装场景的入口换了类型。库供数路径的直段没有身份，房间树里它们的眼睛停「未加载」（房间树本来就只在服务供数下有）。
