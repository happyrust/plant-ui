# 房间视图 · 直段截断 · 开发计划

- 日期：2026-09-29
- 提出：用户「房间显示时加一个显示选项，自动裁掉穿插过的 tubi，把 tubi 截断；分析怎样图形显示效果最佳」。
  按 grill-with-docs 问了一轮（Q1–Q6），用户回「全部按推荐方案生成开发计划」：第一轮六题与第二轮待问的七题
  **一律按推荐定案**（§三）。第二轮那七题没有单独问过，批注时请重点看。
- 范围：
  - `plant-ui`：HUD、`RoomViewVm`、`Settings`、`Cmd` / `ModelAction`。
  - `plant-ui-app`：房间视图编排、数据桥、设置落盘、sim。
  - `plant-ui-data`：房间详情多带一格。
  - `plant-ui-view3d`：房间体图层 + 直段截断。
  - 新纯库 `vendor/plant-room`：从 gen-model `room_predicate.rs` 抽出。gen-model 侧只做抽库、改引用，**零行为变化**。
    （实施时落在本仓 `crates/plant-room`，见 §十。）
- 依据：ADR-0016（eye 读实际渲染）、ADR-0021（取回工作清场重装）、ADR-0026（房间只走库）；本计划新立
  **ADR-0028**（截断改写实例变换）与 **ADR-0029**（房间体判定与 gen-model 共用纯库）；`CONTEXT.md`「房间视图」
  一组五条（本次新增）；前序计划 `room-info-feature.md`（房间视图一期：隔离 + 取景，压暗 + 亮框后置）；
  gen-model ADR-010 §3 与 `src/fast_model/room_predicate.rs`（唯一判定口径）。
- 设计依据：`design/plant-ui.pen` S13-B（房间页签 + 视口聚焦，HUD「房间视图 · R301」）。芯片四态与断口特写在 M4 补稿
  （还没补）。
- 状态：**M1–M4 已实现（2026-09-30），M5 实机验收未做。M0 只做了前一半：纯库暂住本仓 `crates/plant-room`，
  gen-model 还没改用它。** 与计划不同的地方和验证情况见 §十。

---

## 一、一句话与完成定义

房间视图的 HUD 上多一枚「截断直段」芯片，默认开，桌面端记住上次的选择。开着时，直段只画轴线落在房间体内的
那一截，截断处是一面剖切色断口，房间外的管件不显示。不管开关，房间体都画成半透明，相机对准房间体。关掉就是
今天的整根显示。

完成定义：

1. 进入房间视图：房间体以 X-Ray 半透明画出，相机框住房间体，不再是整根分支乃至整个已加载范围的并集。
2. 截断开：
   - 穿墙直段在墙面处截断，断口是剖切色；
   - 整段在房间外的直段不画；
   - 有归属边的跨墙法兰整件显示；
   - 墙外的阀门、弯头不显示。
3. 截断关：分支整根显示，与今天一致。半透明房间体与取景照旧。
4. 芯片说清状态（开 / 计算中 / 关 / 不可用 + 原因）。点一下即切，不用重进房间视图。
5. 拾取与旋转中心落不到墙外那截；点断口选中所属 BRAN。
6. 退出房间视图：直段回到全长，断口与房间体撤掉，可见性按隔离快照还原。
7. 房间体取不全（面板查询失败、网格缺失）时**不截断**，芯片置「不可用」并说原因，不装作截过。
8. 在房间视图里取回工作或重装：新装进来的直段照样被截断，房间体也重取一次。
9. 浏览器端同一套代码可用，只是设置不落盘。

---

## 二、勘探事实与架构约束

| 事实 | 位置 | 对本计划的影响 |
|---|---|---|
| 直段是共享单位原型加实例变换。圆管 `CylinderV3` 半径 1、**局部 Z∈[0,1]**；矩形风管 `BoxV1` 是**居中单位盒，Z∈[-0.5,0.5]**。长度与截面全折进 `world_trans`（夹具里平移 = 起点，scale = (r, r, L)），子实例变换是单位阵 | e3d-model `primitive_instance.rs` `build_canonical` / `instantiate_centered_cylinder`、`solid.rs:197` `box_solid(…, true)`；gen-model `persisted_world_transform`；夹具 `pipe26229-deleted-records.json` | 截断就是重映射原型的局部 Z 区间，不生成网格。**两种原型的 z 范围不同，只能从网格自己读；写死 [0,1] 会把风管截偏半根** |
| 直段没有参考号：三条读路径的直段根 refno 都是 BRAN。owner 各不相同：旧 `tubi_relate` 是自身，派生 `inst_relate` 是 PIPE，服务供数是**查询范围的标签** | `plant-ui-data/src/lib.rs` `tubi_to_geom` / `query_derived_insts_by_inputs`；`plant-ui-app/src/model_record_union.rs` | 截断按几何对每个直段根逐一算，不按身份对号；精确隔离不能靠 owner |
| 今天的房间视图：`Isolate` 按 refno‖owner 保留，`expand_room_model_targets` 再把含成员的已加载范围整包并入。结果是整根分支、甚至整个 ZONE 都亮 | view3d `lib.rs` `ModelAction::Isolate`（≈2176）；`main.rs` `expand_room_model_targets`（≈1670）与 `Evt::RoomDetail` 那一臂 | 截断开时换成「精确 refno + 不扩范围」；截断关保持今天的口径 |
| 归属边挂叶子；直段的边挂容器 BRAN（「隐式管身挂容器」），每个 (BRAN, 面板) 只留最强一条；旧的全量重建路径不测直段 | gen-model `room_model.rs:1797`、`merge_root_edges`、`cal_room_refnos_from` | 截断候选 = 成员里的 BRAN/HANG ∪ 成员的 BRAN/HANG 父。只认前者，旧数据上会漏掉整根光管 |
| 房间体 = 本房全部在册面板的棱柱。判定 = 焊接 0.01 mm + 剔退化三角 + 三射线奇偶三取二（2026-09-18 起；此前 parry `contains_point` 把盒外一米的点判成成员） | gen-model `room_predicate.rs` | 客户端不另写判定，共用抽出来的纯库（ADR-0029） |
| 客户端没有房间体：`RoomDetail` 只给面板 refno；面板几何只在「显示房间模型」时进场景 | `plant-ui-data/src/room.rs`；`plant-ui/src/lib.rs` `Cmd::ShowRoomModel` 文档 | 新增「房间体图层」。按面板取记录走读面 `model_instances`，两种供数同一条；网格走 `AssetServer`，原生与浏览器同一条 |
| 拾取是 CPU `MeshRayCast`：只打 `ModelPickTarget`，只打 `InheritedVisibility` 为真的实体，用的是变换后的三角形 | view3d `surface_hit`；bevy fork `mesh_picking/ray_cast/mod.rs` | 改子实体变换、藏子实体，拾取与旋转中心天然正确；着色器 discard 做不到（ADR-0028） |
| 离屏 + WebGL2 上自定义材质整段不画过一次，无效直段因此退回 `LineList` | view3d `load_models` 里那段注释 | 不引入新 `Material`，断口用 `StandardMaterial` |
| 尺寸标注层与 `SceneRoot` 平级，隔离、可见性、`Replace` 都碰不到它 | view3d `DimensionLayer` | 房间体图层照这个形态做，不进可见性账，不影响 eye |
| `Replace`（取回工作）不清 `isolate_restore` | view3d `load_models` | 截断状态要跨 `Replace` 活着，并给新 spawn 的直段自动补截 |
| HUD「剪裁 关」是设计稿里整视口的状态位，今天没有实现 | `design/plant-ui.pen`（S1 等多张与 S13-B） | 本功能不占这一格，留给以后的剖切盒 |
| `Settings` 整体 `serde(default)`，`tree_full_names` 是显示偏好落盘的先例；`settings_store::save` 只在原生端 | `plant-ui/src/settings.rs:93`；`plant-ui-app/src/settings_store.rs:71` | 开关落 `Settings` 一格；浏览器端只在会话内有效 |
| 两边都是 parry3d 0.17.0，来源不同：plant-ui 经 `vendor/forks/parry`（path，由 rs-core 引入），gen-model 经 gitee git（带 `serde-serialize` / `rkyv`）。plant-mbd 是两边用 path 依赖同一份纯库的先例 | `vendor/rs-core/Cargo.toml:56`；gen-model `Cargo.toml:158` / `:184` | 纯库依赖 `parry3d = "0.17"`，两个工作区各用 `[patch.crates-io]` 钉到自己那份 fork |
| 网格资产保留主世界数据（`RenderAssetUsages::default()`） | rs-core `shape/pdms_shape.rs` `gen_bevy_mesh` | 视口能直接读原型顶点（量 z 范围）与面板三角形（建房间体） |

---

## 三、定案（全部按推荐）

第一轮（问过）：

| # | 问题 | 定案 |
|---|---|---|
| Q1 | 「房间显示」「穿插」指什么 | 指**房间视图**（`Cmd::FocusRoom`，隔离 + 取景那条），不是「显示房间模型」；「穿插」就是直段穿越房间边界。五条词进 `CONTEXT.md`（本次已写）：房间视图、房间体、直段、直段截断、断口 |
| Q2 | 剪哪些 | **只截直段**：圆管、矩形风管、支吊架（HANG）上的直段；其他构件不剪。通用剖切另立「剖切盒」 |
| Q3 | 截断开时，房间外的管件 | **不显示**。管件按自己的归属边：有边连到本房面板的整件留下，跨墙法兰不截。BRAN 在册只说明它的直段是截断候选，不再把整根分支带进来。设备、结构等其他成员照归属边走 |
| Q4 | 按什么边界截 | **房间体**：本房全部在册面板棱柱的并集，与 gen-model 判定归属用的是同一份几何 |
| Q5 | 房间外那一截 | **不画** |
| Q6 | 断口画法 | **剖切色断口**：截断处盖一枚同原型压成的薄片，刷一档专用剖切色 |

第二轮（没有单独问，按推荐直接定，**请重点批注**）：

| # | 问题 | 定案 | 理由 |
|---|---|---|---|
| R1 | 截断位置由谁来算 | **视口里算**；「点在不在房间体内」用从 gen-model 抽出来的纯库（ADR-0029） | 直段的变换与原型网格只在视口手上。服务端算区间要新表新端点，而且直段在客户端没有身份，对不上号 |
| R2 | 开关放哪、叫什么、默认值、记不记 | 放在房间视图 HUD 徽章后面，一枚芯片「截断直段 开 / 关」，**默认开**；桌面端落 `Settings.room_view_truncate_tubes`。不占「剪裁」格，设置窗也不另摆一行 | 这个选项只在房间视图里有意义；状态写在字上，不只靠颜色 |
| R3 | 取景 | **对准房间体**，即面板世界盒的并集，发 `FocusBounds`。房间体取不到时退回今天的 `FocusGroup`。房间视图里「视图适应」与 ViewCube 的 Home 也对准房间体 | 今天框的是整根分支的并集，相机常常退到半个厂区外 |
| R4 | 房间体要不要自动 X-Ray | **要**。进入房间视图就把房间体画成半透明，与截断开关无关。场景里本房的 PANE 模型不再列入隔离目标，免得米色实体与半透明房间体叠画 | 断口要有一面看得见的墙做参照；这也是 S13-B「高亮」的那一半 |
| R5 | 斜穿墙时的断口 | **垂直于管轴**，截在轴线与房间体表面的交点 | 只改变换就能做。贴墙斜切要么靠着色器剖切，要么切网格，都是 ADR-0028 否掉的路。代价是墙面两侧各差 r·tanθ |
| R6 | 擦墙直段（管壁擦进房间，轴线没进） | **只看轴线**：轴线没进房间体就不画。短于 1 mm 的保留段丢掉，离管端不足 1 mm 的截点并到管端，与 e3d-model 的最小管长 1 mm 一致 | 口径一句话说得清。这类直段在归属里是成员、画面上却没有，由 HUD 与日志里的段数说明 |
| R7 | 与其他功能的衔接 | 尺寸标注层**不跟着截**，它标的是分支本身。选中高亮照常落在保留段上，断口不变色。重画或取回工作后新 spawn 的直段**自动补截**，房间体随取回工作重取一次。房间成员变了不跟，重进房间视图才更新，与今天的隔离一致 | 截断只改看法，不改模型，也不改归属 |

---

## 四、方案

### 4.1 进入一次房间视图的全过程

```
Cmd::FocusRoom(room)
 └─ Req::RoomDetail ──► Evt::RoomDetail：panels / member_refnos / tube_carriers（新）
      ├─ Isolate：截断开 = 精确 refno（成员 ∪ 候选 − 面板）；截断关 = 今天的目标集 − 面板
      ├─ Req::RoomVolume{epoch, room, panels}
      │    └─ ensure(room) → model_instances([room]) → 只留在册面板的记录
      └─ vm.room_view = { room, room_num, member_count, truncate, truncation: 装载中 }
Evt::RoomVolume{epoch, Ok(records)}
 ├─ FocusBounds(面板 world_aabb 并集)
 └─ view3d.set_room_volume(RoomVolumeBatch{ panels: records, tube_carriers, truncate })
视口：spawn 半透明房间体（不可拾取）→ 面板网格全部就位 → 建 RoomSolid
     → 候选直段逐帧限量截断 → room_clip_status() 回执 → HUD 芯片 / 日志
Cmd::SetTubeTruncation(on) → 落设置 → 按新口径重发 Isolate → view3d.set_room_truncation(on)
ExitIsolate / 重连 / 换供数 → view3d.clear_room_volume()：直段复原，断口与房间体撤掉
```

`Evt::RoomVolume` 失败或记录为空时，用 `FocusGroup` 兜底，芯片显示「不可用：房间体取不到（原因）」，不截断。
在房间视图里再点另一间房，就整层替换：先全部复原，再按新房间体截。

### 4.2 纯库 `plant-room`（`vendor/plant-room`，ADR-0029）

照 plant-mbd 的做法：独立仓 `vendor/plant-room/crates/plant-room`，只依赖 glam 0.29 与 parry3d 0.17，
不碰 aios_core、库与 HTTP，wasm 编得动。

- **原样搬过来**：
  - `panel_tri_mesh`，入参从 `&PlantMesh` 改为顶点 / 索引切片；
  - `point_inside_panel`、射线奇偶与 f64 Möller–Trumbore；
  - 八角点计数与 `AabbVerdict` 那一组。
  gen-model 的 `room_predicate.rs` 只留一个 `PlantMesh` 适配和 `pub use`。
- **新增** `RoomSolid`（在册面板三角网 + 各自的世界盒）与 `inside_intervals(a, b) -> Vec<[f64; 2]>`：
  1. 对与线段包围盒相交的每块面板，沿 a→b 收集全部穿越参数 t∈(0,1)，用同一套 f64 求交，排序去重；
  2. 用穿越点把 [0,1] 切成段，每段取中点问 `point_inside_panel`。三取二投票，公共边被重复计数也不影响结论；
  3. 各面板的「内」段取并集，相接或间隙小于 1 mm 的合并，保证两块面板共面相接处不会断开；
  4. 丢掉短于 1 mm 的段，离两端不足 1 mm 的端点并到 0 / 1。

### 4.3 视口（`plant-ui-view3d`，ADR-0028）

- **房间体图层**：
  - 接口 `set_room_volume` / `set_room_truncation` / `clear_room_volume` / `room_clip_status`，走 `ViewCommand`。
    形态与尺寸标注层相同：与 `SceneRoot` 平级，挂 `scene_transform`。
  - 每块面板的每个实例 spawn 一个子实体，用 `XRayMaterial` 与 `NotShadowCaster`，**不挂 `ModelPickTarget` / `ModelMesh`**。
    所以它点不中、不当旋转中心，也不进选中与可见性账。
  - 网格全部 `Loaded` 后，按 `world_trans × inst.transform`（毫米）建 `RoomSolid`。有网格加载失败就是「不可用」。
- **标记直段**：`load_models` 里 `inst.is_tubi || inst.is_invalid_tubi` 的子实体挂 `TubeBody { refno, base: inst.transform }`。
- **原型守卫**：按网格资产缓存 z 范围。全部顶点都落在 z = z₀ 或 z = z₁ 两个端面上，才算「沿局部 Z 的棱柱」；
  否则这一段不截，计入「未能截断」。无效直段的虚线网格按构造就是 [0,1]，不过守卫。
- **截一段**：设 M = 根变换（`world_trans`，毫米）× `base`，A = M·(0,0,z₀)，B = M·(0,0,z₁)。
  区间 [t₀, t₁] 对应局部 [za, zb]，其中 z = z₀ + t·(z₁ − z₀)。
  - 没有区间：子实体 `Visibility::Hidden`。**根不动**，eye 与隔离快照不受影响。
  - 整段 [0, 1]：不动。
  - 其余情况：
    - 第一段改子变换为 `base · T(0, 0, za − z₀·k) · S(1, 1, k)`，其中 k = (zb − za)/(z₁ − z₀)；
    - 第二段起 spawn 兄弟实体：同一 `Mesh3d`、同一当前材质，克隆 `ModelMesh` 与 `ModelPickTarget`，挂 `TubePiece`；
    - 每个落在 (0, 1) 内部的截点 spawn 一枚断口：
      - 用同一原型网格，局部 z 以截点为中心，厚 2 mm（k′ = 2 / L_mm）；XY 放大到 1.02 倍，盖住原型自带的端面；
      - 材质 `SectionMaterial`：unlit、双面，候选色 `#D946EF`，要与选中橙红、X-Ray 淡蓝、标注近白和各类型色都分得开，
        两套主题上实测后定；
      - 挂 `ModelPickTarget`，点断口选中 BRAN；**不挂 `ModelMesh`**，选中时不变色。
  - 无效直段同样重映射，但要把虚线网格换成截后长度的那一份，维持 100 / 80 mm 的节距，不加断口。
- **调度**：
  - 新系统排在 `apply_commands` 之后、`apply_selection` 之前。
  - 开关、房间体或候选一变，代次 +1，全部复原后重算。
  - 每帧限量处理，先定 256 段，实测再调；网格还没就位的直段留到下一帧。
  - `Added<TubeBody>` 的新直段（重画、取回工作）自动进队。
  - 候选先用房间体世界盒粗筛，盒外的直接藏掉，不做射线。
- **复原**：子变换回 `base`，改回 `Visibility::Inherited`，换回原虚线网格，`TubePiece` 与断口 despawn。
  卸载根时子实体一起带走，不需要另记账。
- **取景**：房间体图层就绪后，`FitAll` 与 `Snap { fit: true }` 框房间体世界盒。
- **回执**：`RoomClipStatus { phase: 装载中{done,total} | 计算中{done,total} | 就绪 | 不可用(原因), cut, hidden, whole, skipped }`，
  帧桥每帧抄进 `vm.room_view.truncation`。

### 4.4 宿主（`plant-ui-app`）

- `Evt::RoomDetail` 那一臂按开关算目标集，抽成纯函数 `room_view_targets(detail, truncate, scopes)` 以便测试：
  - 截断开：`(member_refnos ∪ tube_carriers) − panels`，发 `Isolate { matching: Exact }`，不走 `expand_room_model_targets`；
  - 截断关：今天的目标集去掉面板，发 `Isolate { matching: RefnoOrOwner }`，即今天的口径；
  - 这里不再发 `FocusGroup`，改由 `Evt::RoomVolume` 发 `FocusBounds`；只有兜底时才发 `FocusGroup`。
- 数据桥：
  - `Req::RoomVolume { epoch, room, panels }` 走交互通道，并发执行，不排在整场装载后面；
  - 调用与 `ModelLoad::Scopes` 那一段相同：`ensure_model(room)` 再 `face.model_instances`，身份四元组随请求带过去；
  - 回 `Evt::RoomVolume { epoch, room, result }`。`room_view_epoch` 在每次 `focus_room` 与退出时 +1，晚到的回包丢掉。
- 交给视口的指令放进 `pending_room_volume`，与 `pending_dimensions` 同形：这批记录是视口的类型，绘制层 crate 不认识它。
- 撤图层的时点：退出、重连（`reset_for_reconnect`）、换供数。取回工作不撤，收尾时若还在房间视图里，就对同一间房
  重发一次 `Req::RoomVolume`。
- `Cmd::SetTubeTruncation(bool)`：
  - 改 `Settings` 一格，走既有存盘（原生端）；
  - 按新口径重发一次 `Isolate`。快照只记第一次，不受影响；
  - 调 `set_room_truncation`。
- 日志：
  - 进入时：「房间视图：R301 · 面板 N · 成员 M · 截断直段 开」；
  - 就绪时：「直段截断：截断 a 段 · 房间外 b 段不画 · 整段在内 c 段（· 未能截断 d 段）」；
  - 不可用时说原因。
- `sim.rs` 给 `RoomVolume` 回一个空包，演示模式走「不可用」那条文案，不许装作截断过。

### 4.5 数据层（`plant-ui-data/src/room.rs`）

- `RoomDetail` 加 `tube_carriers: Vec<RefU64>`：成员中 noun ∈ {BRAN, HANG} 的，并上 owner.noun ∈ {BRAN, HANG} 的成员父。
  做法是 `room_detail_sql` 边表那一句多投影 `out.noun, out.owner, out.owner.noun`，在 Rust 侧归并。
- 性能要实测，用 AMS 上成员最多的那间房。今天那一句是 0.44 s，不能拖成秒级；拖了就改成对去重后的成员直选
  （`FROM pe:a, pe:b…`，同 `detail_follow_up_sql` 的做法）。

### 4.6 绘制层（`plant-ui`）

- `ModelAction::Isolate` 加一格 `matching: IsolateMatching { RefnoOrOwner, Exact }`。「房间」页签的「隔离成员」芯片用
  `RefnoOrOwner`，行为不变。
- `RoomViewVm` 加 `truncate: bool` 与 `truncation: TruncationVm`。
- HUD：在徽章与「退出」之间放一枚芯片，四态：
  - 开：「截断直段 开」，激活态。hover「只画直段在房间体内的那一截，房间外的管件不显示。截断 a 段 · 房间外 b 段」；
  - 计算中：「截断直段 开 …」，hover 带进度；
  - 关：「截断直段 关」，hover「整根分支显示」；
  - 不可用：灰字，hover 说原因，如「房间体缺 2 块面板网格」。点击照样改开关，就绪后生效。
- `Settings.room_view_truncate_tubes: bool`，出厂 `true`；老 `settings.ron` 缺这一格就按出厂值。
- `Cmd::FocusRoom` 的文档改成新口径：半透明房间体、对准房间体取景、直段截断。

---

## 五、分阶段任务

### M0 · 纯库 `plant-room`（约 1 天，gen-model 侧 + 新仓）

- 建仓、搬函数，新增 `RoomSolid::inside_intervals`；gen-model 改引用；两个工作区各加一条 `[patch.crates-io] parry3d`。
- gen-model 那边按它自己的流程立 ADR 与计划（抽库、零行为变化）。
- **验收**：
  - gen-model 房间相关测试全绿，包括 `room_predicate` 的全部用例、R320 真网格那条，以及「整库重算 == 从零初始化」
    逐边相等那组；
  - 纯库自测覆盖：
    - 线段在盒内、在盒外、穿一面、穿两面；
    - L 形凹房里穿出再穿入，得两段；
    - 斜穿；
    - 擦墙（共面）；
    - 管端恰好在墙上；
    - 两块面板共面相接处不断开；
    - 退化三角不改结论；
  - `cargo check --target wasm32-unknown-unknown -p plant-room` 通过。

### M1 · 数据层 + 通道（约 0.5 天）

- `tube_carriers`、`Req::RoomVolume` / `Evt::RoomVolume`、`room_view_epoch`、sim 回包。
- **验收**：
  - 归并纯函数单测：成员本身是 BRAN、成员的父是 BRAN 或 HANG、两者都有时去重；
  - 陈旧回包被丢弃的单测；
  - 在 AMS 上记下 `room_detail` 加列前后的耗时。

### M2 · 视口：房间体图层（约 1 天）

- 图层实体、X-Ray 材质、网格就位判定、`RoomSolid` 构建、就绪后让 `FitAll` / Home 框房间体、回执。
- **验收**（沿用现有 `MinimalPlugins + AssetPlugin` 的 App 测试写法）：
  - 房间体实体不挂 `ModelPickTarget` / `ModelMesh`；
  - `clear_room_volume` 撤干净；
  - 一块网格加载失败时回「不可用」，并带缺失数；
  - `Replace` 之后图层仍在。

### M3 · 视口：直段截断（约 1.5–2 天，核心）

- `TubeBody` 标记、原型守卫、截断系统、兄弟段、断口、无效直段、复原、开关、增量补截。
- **验收**：
  1. `CylinderV3` 形原型截 [0.3, 0.7]，世界端点误差 < 0.01 mm；
  2. `BoxV1` 居中原型截同一区间，世界结果与第 1 条一致，防半根偏移；
  3. 整段在外时子实体隐藏，根的 `Visibility` 与 `render_states` 不变，eye 不动；
  4. 凹房得两段时：一段改写、一个兄弟段、两枚断口；
  5. 射线打在被截掉的位置落空，打在断口上回 BRAN；
  6. 复原后变换与原值逐位相同，额外实体全部撤掉；
  7. 截断生效期间 `append` 进来的直段会被补截；
  8. 非棱柱网格被跳过，计入 `skipped`；
  9. 选中 BRAN 时，保留段与兄弟段变高亮，断口不变。

### M4 · 宿主编排 + HUD + 设置（约 1 天）

- 先在 `design/plant-ui.pen` 的 S13-B 旁补一张稿：芯片四态加断口特写。再实现。
- `IsolateMatching`、`room_view_targets`、`FocusBounds` 与兜底、撤图层的三个时点加取回工作重取、`SetTubeTruncation`、
  设置落盘、HUD 芯片、日志。
- **验收**：
  - `room_view_targets` 两种口径的单测：截断开时，BRAN 下不是成员的管件不在目标里，面板也不在目标里；
  - 老设置缺这一格时为开；
  - HUD 测试：点芯片发 `SetTubeTruncation`，四态文案各有一例；
  - 退出与重连都会发 `clear_room_volume`。

### M5 · 实机验收（约 0.5–1 天）

- 在 AMS 上挑三间：规整矩形房、船体型线凹房（R320）、带矩形风管的房。每间截断开 / 关各截一张图，记下候选数、
  截断耗时与 `room_detail` 耗时，写进 `docs/evidence/`。
- 桌面与浏览器各走一遍：进入 → 切开关 → 取回工作 → 退出。
- 在两套主题下核对剖切色。

---

## 六、风险与坑

1. **原型 z 范围不统一**：写死 [0,1] 会把矩形风管截偏半根。z 范围从网格读，再加棱柱守卫（M3 验收第 2 条）。
2. **服务供数下 owner 是查询范围的标签**：截断开用精确 refno 加数据层给的 `tube_carriers`，任何一步都不靠 owner。
3. **归属口径 ≠ 截断口径**：归属按 AABB 八角点加顶点兜底，截断按轴线区间。擦墙直段是成员却不画；旧全量路径没测
   直段的 BRAN，靠「成员的父」补进候选。这是设计如此，由 HUD 与日志里的段数说清。
4. **房间体不全**：宁可不截，也不按半个房间截。
5. **parry3d 来源**：`[patch]` 漏配会出现两份 parry，编译期类型对不上。这会响亮地失败，不会静默出错。fork 版本号
   一旦漂移，两边要同步改。
6. **性能**：一根分支可能穿过半个厂，候选直段先按房间体世界盒粗筛，每帧限量处理；浏览器端单线程，要实测。
7. **首次进入面板还没生成的房间**：`ensure` 要现生成，相机要等几秒才动，期间 HUD 显示「装载房间体」。
8. **斜穿墙误差**：断口垂直于管轴，墙面两侧各差 r·tanθ。DN300 的管 45° 斜穿时约 160 mm。按 R5 接受。
9. **gen-model 侧改动**要它的负责人认可，且必须零行为变化。M0 过不了就先停，不在客户端抄一份顶上。
10. **并发会话**：动手前确认 plant-ui 与 gen-model 的工作树没有别的会话在写（先例见 `room-info-feature.md` 风险 4）。

---

## 七、工作量与顺序

| 阶段 | 内容 | 估时 |
|---|---|---|
| M0 | 纯库 + gen-model 改引用 | 1 天 |
| M1 | 数据层 + 通道 | 0.5 天 |
| M2 | 房间体图层 | 1 天 |
| M3 | 直段截断 | 1.5–2 天 |
| M4 | 宿主 + HUD + 设置 | 1 天 |
| M5 | 实机验收 | 0.5–1 天 |
| 合计 | | **5.5–6.5 天** |

M0 与 M1 可以并行。M2 依赖 M0，M3 依赖 M2。M4 里 HUD 与设置那部分可以与 M2、M3 并行。

---

## 八、明确不做

- 通用剖切，即型钢、桥架、设备一起剪：另立「剖切盒」，HUD 的「剪裁」格留给它。
- 贴墙斜切的断口、墙外淡影或短残影、穿墙环、断裂符号。
- 让尺寸标注跟着截。
- 自动加载没显示过的成员：房间视图仍只作用于已加载的模型，与今天一致。
- 房间成员或面板变了时自动刷新房间视图。
- 由服务端算截断区间或开新端点。
- 设置窗里的开关行；「房间」页签里「缩放到房间」「隔离成员」两枚芯片也不动，它们不是房间视图。
- 房间视图里按 F 定位成员时框的是整根分支：这是今天的行为，不改。

---

## 九、随本计划落地的文档

- `CONTEXT.md` 新增「房间视图」一组五条（本次已写）。
- ADR-0028（直段截断改写实例变换，不做着色器剖切）、ADR-0029（房间体判定与 gen-model 共用纯库），本次已写。
- gen-model 侧的抽库 ADR 由 M0 在那边补。

---

## 十、实施记录（2026-09-30）

### 落地情况

| 阶段 | 状态 | 落在哪 |
|---|---|---|
| M0 纯库 | 前一半完成 | `crates/plant-room`：`predicate.rs` 逐字搬自 gen-model 2026-09-18 版，只把入参换成切片；`solid.rs` 是新写的 `RoomSolid` / `inside_intervals` |
| M0 gen-model 改用 | 未做 | gen-model 工作树有别的会话在改；抽库要走它自己的评审与 ADR |
| M1 数据层 + 通道 | 完成 | `plant-ui-data/src/room.rs`、`plant-ui-app/src/data.rs`、`sim.rs` |
| M2 房间体图层 | 完成 | `plant-ui-view3d/src/room_clip.rs` |
| M3 直段截断 | 完成 | 同上；`lib.rs` 的 `load_models` 给直段挂 `TubeBody` |
| M4 宿主 + HUD + 设置 | 代码完成，设计稿没补 | `plant-ui-app/src/main.rs`、`plant-ui/src/{lib,vm,settings}.rs`、`workbench/view3d.rs` |
| M5 实机验收 | 未做 | |

### 与计划不同的地方

- **纯库的位置**：计划写的是两仓之外的 `vendor/plant-room`，那里的写入没放行，库先放进 plant-ui 工作区
  （ADR-0029 已按此改写）。parry3d 走 path 依赖，指向 rs-core 用的同一份 fork，本仓不用打 `[patch]`。
- **线段求穿越点**（§4.2 第 1 步）：不走 BVH 射线，改为逐三角 f64 包围盒粗筛再求交。管轴多半沿坐标轴，打在墙面
  三角零厚度包围盒的边界上是 `0 × ∞`。重心坐标放宽 1e-9，轴线正好过面板对角线时不会漏掉交点。
- **成员身份**（§4.5）：没在 `room_detail_sql` 的边表上加投影，直接按记录 id 分块直选（每块 1000 个）。这就是计划里
  「拖成秒级就改」的那条路，先走了。耗时还没在 AMS 上量。
- **宿主纯函数**（§4.4）：`room_view_targets` 落成 `room_view_isolate(detail, truncate, scopes, with_panels)`，直接给出
  `Isolate` 指令；兜底取景是 `room_view_fallback_focus`。房间体取不到时面板照旧进隔离（`with_panels`），至少看得见墙。
- **房间体不可用有两道门**：
  - 宿主这边：在册面板有一块查不到模型记录，就带着原因交给视口，照样画半透明、不截；
  - 视口那边：有一块面板网格加载失败，就置「不可用」并报缺几块。

### 验证

- `cargo test -p plant-room -p plant-ui -p plant-ui-data -p plant-ui-view3d -p plant-ui-app --no-fail-fast` 全绿。
  plant-ui-data 的实库集成测试照旧是 `#[ignore]`，没跑。
- `cargo check -p plant-ui-app --target wasm32-unknown-unknown` 通过（plant-room 随之编过 wasm）。
- 有单测的验收条目：
  - M0：§五列的纯库自测各类都有用例（`solid.rs`）；
  - M1：候选归并、陈旧回包；
  - M2：不可拾取、整层撤掉、缺网格即不可用；
  - M3：第 1、2、4、6、7、8、9 条，第 3 条只测了根的可见性；
  - M4：两种隔离口径、老设置缺这一格按开、芯片文案与点击；退出、重连、兜底三处都撤图层（源码扫描，
    与仓里编排类测试同一写法）。
- 没有单测的：
  - M2「`Replace` 之后图层仍在」：只靠结构保证（图层与 `SceneRoot` 平级）；
  - M3 第 5 条射线拾取：断口挂着 BRAN 的 `ModelPickTarget` 有测；「打在截掉的位置落空」靠的是 Bevy 按
    `GlobalTransform` 求交、跳过不可见实体，没有端到端测；
  - M3 第 3 条里的 `render_states` 不变。
- 没在实库 / AMS 上跑过，§五 M5 全部待做。

### 还没做

- gen-model 改用本库（M0 后一半）。
- `design/plant-ui.pen` 补稿：芯片四态 + 断口特写。
- M5 实机验收：三间房截图，候选数、截断耗时、`room_detail` 耗时，桌面与浏览器各走一遍，两套主题下核对剖切色。
