# 构件的房间归属是读面上的一条读

状态：**提议**（2026-11-20 起草，同日按方案复审改口径，见 D5；按实现审核改 D4 里库供数的强度；按复审第 3 条加 D6；按复审第 6 条补 D3 末段；房间详情遇老边补 D4 末段；spec：gen-model `specs/045-element-room-membership/`，
服务端契约见 gen-model `docs/specs/web-service-api.md` §4.13.8）。
修订：`docs/plans/2026-09-07-read-face-dual-mode.md` D6（房间不动、只有库一条、服务供数下经 `mirror` 门控）——
「构件的房间归属」这一条从 D6 里拿出来；房间的其余功能（面板所属房间、房间详情、房间浏览器、X-Ray 拓扑）不变；房间详情只按 D4 末段补成员边的老边读法。
`docs/plans/room-info-feature.md` 决定 1（容器聚合直接子层）由 D5 取代。

## 背景

复制粘贴管道、增量更新之后，后端 `e3d.room.lookup` 答得出 R512，右键「查看所属房间」却是「无房间归属」。成因有四层：

- **客户端自己拼图查询。** 最早这条读直连库拼 `room_relate`，要求构件在库里有 `pe` 记录、子层有 `pe_owner` 边；
  平表化库里这两张表空或不全时恒返回空。
- **修补挂错了地方。** 之前的修补把它改成问 gen-model `/api/v1/element/rooms`，却仍挂在镜像门后面（两面都先问
  `/health.mirror` 再连库）：服务供数下库不通，就答不了一个根本不靠库的问题。
- **答复说不清「算完没有」。** 没算完的空表被画成「无房间归属」；老后端没有这条路由，404 被当成查询失败；
  解不出的行被悄悄丢掉。三样都是静默失败。
- **口径只到直接成员。** 两种供数都只聚合「自身 + 直接子层」。BRAN 自己没有 `inst_relate` 行，房间边挂在更下一层的
  管件上，选 PIPE 时一个成员都找不到。AMS 落盘库上 4 个 PIPE（含两个复制出来的）按旧口径全是空，算上全部后代
  各有 2–9 间房。

## 决定

### D1　构件的房间归属进读面

`ReadFace::element_rooms(refno, scope) -> RoomsOutcome`：

- **服务供数**问 `/api/v1/element/rooms`，归属口径（自身加全部后代、强度排序、面板在册与否）由服务端一处定义。
- **库供数**查 `room_relate`（`plant_ui_data::room::element_rooms`），口径与服务端相同（D5）；先等库连接，**不过镜像门**：
  那道门要问模型服务的 `/health`，而库供数不碰模型服务（`store_face_never_touches_the_service`），启动时也已经连上库。
- `data.rs` 的 `Req::ElementRooms` 那一臂只调读面，不过镜像门、不自己问接口（`element_rooms_go_through_the_read_face_only`）。

### D2　结论分四种，由读面给出

`RoomsOutcome::{Ready, Pending, Unavailable, Failed}`。服务供数按结构分型（`data::classify_element_rooms`，spec FR-103），
不看 message 里的字串：

| 服务端怎么答 | 结论 |
|---|---|
| 404 / 405 且包体不是错误包封（`route_missing`，老后端） | 查不了：请升级，或在设置里把供数切到库 |
| 200 但没有 `caliber`（只在测试站点上跑过的草稿构建） | 同上，并说明它的列表会漏房间，不画 |
| 200，`caliber` 是 `self_and_direct_members`（只到直接成员的旧构建） | 同上，并说明它只算到直接成员、选管道时会漏房间，不画 |
| 200，`caliber` 是别的值 | 查不了：口径不认识，请把界面与 gen-model 升到同一版本，不画 |
| `status` 不是 `ready` / `degraded` | 查不了，带服务端的原因 |
| `settled` 不是 `true` | 还没有结论：`false` 还没算完，`null` 暂时判断不了；答复里已有的行一并带出（D6） |
| 带包封的 `not_found` | 查不了：服务端还没有这个构件（可能尚未同步） |
| 带包封的 `container`（422，层级容器，D5） | 查不了，照搬服务端的原话 |
| 其它失败 | 失败，带原文 |
| 成功 | 有结论；空表才是「无房间归属」 |

有一行解不出（缺 `panel` / `room_num`，或 refno 解不出）就整次判失败并点名 `memberships[i]`，不丢行再画（还没有结论时也一样）。

库供数没有「还没有结论」（`store::store_rooms_outcome`）：读到边是「有结论」；一条边都没读到是「查不了」（`store::NO_EDGES`：
库里查不到这份归属算完没有，不说成「无房间归属」，spec US4）；层级容器是「查不了」，与服务端 422 `container` 同一句；
查询出错是「失败」。所以库供数下「无房间归属」不会出现。

### D3　宿主只采纳最后一次问的那一份

`Req::ElementRooms` / `Evt::ElementRooms` 带 `seq`，宿主每问一次加一；`accept_rooms_reply` 只认「仍是主选中、且是最后一次
问的」（FR-106）。只看 refno 不够：重试前发出的旧问题晚到，会把新答案盖回去。

「还没有结论」「查不了」都带构件号（`RoomVm::Pending { refno, reason, partial }` / `RoomVm::Unavailable { refno, reason }`），
「房间」页签与右键子菜单都给「重试」；子菜单只在问的那一行上显示它们——重试只对主选中有效，右键落在多选集里
非主选中的行上时不摆一个点了没反应的钮。「查不了」写一行警告、「失败」写一行错误，同一原因只写一次。

「重试」与已选中时再点「查看所属房间」只重问归属（`ask_rooms`），不走 `refetch_rooms`：后者连面板所属房间一起重问，
回包会重开房间 X-Ray、关掉用户拨开的虚拟孔洞 X-Ray（spec 045 FR-111）。`refetch_rooms` 只留给主选中变化与
取回工作完成，这两处都先撤了房间 X-Ray。面板所属房间查失败时，要重新选中那块面板才再查。

### D4　强度缺席照实显示

`RoomRelation.inside_count` / `center_dist`（以及绘制层的 `RoomRelationVm`）改成 `Option`：服务端老边没有强度时是 `null`，
界面显示「—」，不补 0（spec US5-3）。库供数同样按可空读：老边缺这两格时留空、归并时算最弱（与服务端同一规则），
不让一条老边把整次查询变成失败（spec FR-109）。房间为空（面板不在册，或服务供数下共用墙按房号挑不出
唯一一间）时，归属行说「房间不明（面板不在册或共用墙）」。

房间详情的成员列表同一条规则（spec 045 FR-112）：`RoomMemberPreview.inside_count`（与绘制层的 `RoomMemberVm`）改成
`Option`，成员边的两格强度按可空读，同一构件跨多面板归并、排序时缺的算最弱，成员行显示「—」；房里一条老边不再让
整个详情失败。

### D5　口径：自身 + 全部后代，两种供数一致

构件的归属聚合它自己和**全部后代**的 `room_relate` 边，每间房留最强一条（spec 045 FR-002 / FR-101）。服务端答复的
`caliber` 是 `self_and_descendants`，客户端只认这一个（`data::ELEMENT_ROOMS_CALIBER`）。

- **后代怎么取。** `inst_relate.anc` 是「自身 → 顶层」整条祖先链，`anc CONTAINS <构件>` 就是全部后代，走
  `idx_inst_relate_anc`。不再写 `anc[1] = <构件>`（服务端旧口径），也不再沿 `pe_owner` 找（库供数旧口径）。叶子件的 `anc`
  也含它自己，库供数那句要 `array::distinct`。边与服务端同一写法取：`array::flatten(($targets)<-room_relate)`。
  旧写法 `SELECT VALUE <-room_relate FROM $targets` 只遍历库里有 `pe` 记录的目标，平表化库里缺记录的构件一条边都
  出不来；`pe` 齐全的库上两种写法在 AMS 上对拍 500 个目标、3,762 行，0 差异（spec 045 背景第 4 条）。
- **层级容器不答。** WORL / WORLD / SITE / ZONE 的后代是整库级：服务端回 422 `container`；库供数先按 `pe` 上的类型
  拒答，不去查边（`pe` 没有这一行时类型无从知道，照常查）。两边同一句：「{类型} 是层级容器，范围太大，
  不答房间归属：请按房间查看，或选它下面的管道 / 设备」。
- **库供数的空表不当结论。** 库里没有生成凭证和房间欠账可查，空表分不清「算完了、不在任何房间」和「还没算」，
  所以说「判断不了」（D2）。对拍探针拿它当空集和服务供数比，要看出「服务供数有房间、库里一条边都没有」的元素。
- **归并键不动。** 服务端按房间 refno 归并；库供数照旧按房号（`merge_edges`），两间房号相同的房间在库供数下并成一行。
  对拍本来比的就是房号集合，这次不扩范围。

### D6　还没有结论时，已有的行照画、标明可能不全

`RoomsOutcome::Pending { reason, partial }`：服务端 `settled` 不是 `true` 时，答复里读到的行一并带出（spec 045 FR-110）。
`partial` 不空时，「房间」页签与右键子菜单照画这些行，顶上写明「下面的房间可能不全或已过时」，连同原因与「重试」；
页签的分组头写「已查到的归属」而不是「全部归属」。它不是结论：

- 首行不标「主归属」，右键子菜单不给「显示房间模型」（那一项只给主归属那间）；
- 属性面板不出房号芯片（`room_chip` 只认有结论）：芯片只有一格，指错房间比少一枚糟；
- 对拍仍归「没给出结论」、不比，说明里带上已读到几间；
- 宿主重问期间不拿它当防闪烁的占位（`RoomVm::begin_query` 只留有结论的列表）：占位是按完整列表画的。

`partial` 为空时同原来：只给原因与「重试」。只有服务供数的落盘形态会答出带行的「还没有结论」：读透形态这时是
空表，库供数没有「还没有结论」（D2）。典型是 PIPE 下一条 BRAN 已生成、新粘贴的一条还没生成；写「或已过时」，
是因为待重新生成的根上挂的还是上一次生成的边。

## 为什么不是别的做法

- **在 `data.rs` 里按面分支。** 两面的判断散在数据线程里，ADR-0026 的「`data.rs` 只经读面」就破了。
- **服务供数答不了时退回库查。** 违反 ADR-0026（数据线程内不许据错误自行换面），而且库那条路正是会漏的那条。
- **只改文案、不要 `settled`。** 没算完的空表仍会画成「无房间归属」。
- **库供数也先过镜像门（与房间其余功能一致）。** 镜像门要问模型服务的 `/health`：模型服务不在场时，库里的归属
  也查不了，而库供数本来就是为了不靠模型服务。
- **口径只算自身 + 直接成员（原口径）。** 选 PIPE 恒为空。界面不会说「只算了一层」，只会写「无房间归属」——
  把「漏了」画成「没有」，正是这条 ADR 要消掉的静默失败。
- **库供数照旧，记为已知限制。** 服务端改口径以后两面答案就不一致了，库供数的空表也仍画成「无房间归属」。
- **SITE / ZONE 也照算。** 后代是整库级，一次要扫的行数没有上限；要看一片区域里有什么，按房间看才对得上问题。
- **还没有结论就一行不画（D6 以前）。** 服务端判得保守时（例如所在库欠一次整库房间重算），已经算好的那部分也
  一直看不到，用户只能等或反复重试。
- **还没有结论也照常画、不加说明。** 可能不全或过时的列表被当成结论，与把「漏了」画成「没有」是同一类错。

## 后果

1. 老 gen-model（正式版 ≤ 0.1.49）没有这条路由：服务供数下「房间」页签与右键子菜单说「请升级；或切到库供数」，可重试，
   不再报「查询失败」。
2. 「没有路由」按结构认（`model_update_api::ROUTE_MISSING`）：文件侧属性接口「服务端不支持」的判法一并从
   「message 里含 405」改为认这个 code；`endpoint_missing` 的结果不变。
3. 对拍探针多一项「房间归属」：属性那批抽样元素两面各问一次，比房号集合；库供数一条边都没读到按空集比（服务供数
   也空算一致，有房间算不同），其余某一面没给出结论的单列。
4. 房间其余功能仍只有库一条路，服务供数下照旧经镜像门（D6 其余部分不变）。
5. 只算到直接成员的服务端构建（只在测试站点上跑过）配这一版界面：提示升级、不画列表。界面与 gen-model 要一起升，
   站点先升后端再发界面。
6. 选 WORL / SITE / ZONE 时两种供数都拒答，给出按房间查看的出路。
7. 库供数不再说一个构件「无房间归属」：一条边都没读到时说「判断不了」，可重试。
8. 库供数遇到没有强度两格的老边不再整次失败，那两格显示「—」。房间详情的成员列表同样不再因老边整次失败，缺的顶点数
   显示「—」、排在有强度的成员后面（D4 末段）。
9. 服务供数下「还没有结论」而答复里已有房间时，页签与右键子菜单先列出这些房间并标明可能不全或已过时；主归属、
   房号芯片与「显示房间模型」等算完再出。
10. 归属的「重试」不再把房间 X-Ray 重新打开，也不再关掉虚拟孔洞 X-Ray；面板所属房间查失败时，要重新选中
    那块面板才再查。

## 实施约束

1. 回归测试：`element_room_replies_are_classified_by_structure`（各种结论，加旧口径、不认识的口径、层级容器、坏行，
   以及还没有结论时带出的行）、
   `element_rooms_go_through_the_read_face_only`、`data_rs_reads_only_through_read_face` 的禁单、
   `store_face_never_touches_the_service` 的禁单与期望单、`only_the_latest_room_reply_for_the_primary_is_accepted`、
   `a_missing_route_is_told_apart_from_a_missing_element`、`route_detection_never_matches_message_substrings`、
   `missing_strength_reads_as_blank_not_zero`、`old_edges_without_strength_merge_as_weakest_and_stay_blank`（库供数老边）、
   `room_members_with_old_edges_rank_last_and_stay_blank`（房间详情老边，D4 末段）、
   `rooms_compare_room_sets_and_set_aside_inconclusive_answers`（含库供数没读到边、还没有结论时带出的行不比）、
   `sql_renderers_pin_tables_and_predicates`（库供数按 `anc CONTAINS` 取后代，不许出现 `pe_owner` / `anc[1]` / `FROM $targets`）、
   `hierarchy_containers_are_refused_like_the_service`、`store_rooms_never_turn_no_edges_into_no_rooms`、
   `a_partial_room_answer_is_not_kept_as_the_loading_placeholder`、
   `retrying_rooms_only_reasks_membership`（D3 末段）；D6 的画法由无头渲染测试钉住：
   `a_partial_answer_is_drawn_under_its_caveat_without_a_primary`、`an_empty_pending_answer_is_only_a_reason_and_a_retry`、
   `the_room_menu_lists_a_partial_answer_without_a_primary`。
2. 客户端只依赖契约里的 `caliber` / `status` / `reason` / `settled` / `settled_reason` / `memberships`；每一格都
   `#[serde(default)]`，认旧后端靠分型，不靠反序列化报错。
3. 库供数的层级容器类型表与 gen-model `generation_root::is_coarse_hierarchy_noun` 是同一组（WORL / WORLD / SITE / ZONE），
   改一边要同步另一边。
