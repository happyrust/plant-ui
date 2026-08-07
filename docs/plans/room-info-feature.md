# 房间信息 · 右键查看 / 房间页签 / 房间浏览器 · 开发计划

- 日期：2026-08-07
- 范围：`plant-ui`（绘制层）、`plant-ui-data`（数据层）、`plant-ui-app`（宿主）、`plant-ui-view3d`（Bevy 视口）
- 设计依据：`design/plant-ui.pen` 的 **S13**（右键菜单）、**S13-B**（房间页签 + 视口聚焦）、**S14**（房间浏览器）与画布注记「注-房间信息定案」；数据口径依 gen-model ADR-010
- **状态：计划，待批注定案；未动一行代码**

---

## 一、目标与完成定义

不是把三张设计稿画出来，而是让「元素 → 它的房间」这条链路两端都能走通：

1. 模型树任意行、三维视口任意构件右键 → 「查看所属房间 ▸」子菜单，房间按**归属强度**排序（inside_count 降序 → center_dist 升序 → room_num 升序），最强一条标「主归属」。
2. 点击子菜单里的房间 → 视口**隔离该房间成员并取景**，右侧「房间」页签同步展开该房间详情。
3. 「房间」页签展示：归属概要（房间码 / 面板数 / 成员数 / 强度）、全部归属、房间构成、成员预览；成员可点击定位回树与视口。
4. 房间浏览器列出全部房间，四态筛选（全部 / 待重算 / 空房间 / 命名不合规），行内可定位 / 隔离。
5. 待重算状态与任务队列 S12「房间」泳道同源，横幅可点击跳转 TaskQueue 页签。
6. 多归属完整展示（跨房贯穿件是常态），绝不只显示最强一条。
7. 独立壳（无实时渲染器，`view3d.live == false`）下：房间**数据**照常可查可看，仅隔离 / 取景等模型动作不出现——与现有 `row_menu` 的 live 门禁同一条规矩。

---

## 二、勘探事实与架构约束

| 事实 | 位置 | 对本计划的影响 |
|---|---|---|
| 数据层直连本地 SurrealDB（`aios_core::SUL_DB`，决定 3/7：不造 DTO） | `plant-ui-data/src/lib.rs:1-13` | 房间查询直接下 SurrealQL，**不需要新增 HTTP 端点** |
| 树右键菜单已存在，右键自动先选中 | `plant-ui/src/workbench/tree.rs:141-146, 198-274` | 只在 `row_menu` 里加一段 `ui.menu_button` |
| 命令体系：绘制层发 `Cmd`，宿主经 `Req`/`Evt` channel 异步查数 | `plant-ui/src/lib.rs:87`、`plant-ui-app/src/data.rs:14,95` | 房间查询照抄 `Props` 的四态 + `begin_query` 防闪烁模式 |
| 页签体系：`Pane` 枚举 + `WorkbenchState::focus(pane)` 可编程激活 | `plant-ui/src/workbench/panes.rs:22`、`mod.rs:77-81` | 「房间」页签 = 新增 `Pane::Room` 变体，与 Properties 同 dock 组 |
| 视口有 `view.bounds`（refno→AABB）与 `frame_bounds` 取景 | `plant-ui-view3d/src/lib.rs:1144-1150` | 房间取景 = 合并成员 AABB 后调同一个 `frame_bounds` |
| `ModelAction::Focus` 按设计只收单 refno；无 Isolate、无组取景 | `plant-ui/src/lib.rs:22-50` | 需新增 `FocusBounds` 与 `Isolate` 两个动作 |
| 视口与 ModelAction 里没有任何房间代码 | 全仓 grep | 全部新增，无历史包袱 |

---

## 三、数据口径（gen-model 侧，已按真实实现校准）

- **归属边** `room_relate`：`panel → element`，字段 `room_num` / `inside_count`（AABB 八顶点落在面板内的个数，0–8）/ `center_dist`（元素中心到面板中心距离）。排序键即 ADR-010 §5。
- **边挂在叶子元素上**。`fn::room_code` 开头那段 EQUI/BRAN 归口逻辑是死代码（算了 `$id` 没用它）；容器元素要拿房间，得照 `fn::get_room_names` 的模式聚合子层：`$id<-pe_owner.in<-room_relate`。
- **房间 → 面板** `room_panel_relate`：房间 FRMW → PANE，带 `room_num`。
- **房间识别**：FRMW 名含关键词（本项目 `room_key_word = ["-RM"]`，见 gen-model `DbOption.toml:173`），末段（`-` 分割）匹配 `^[A-Z]\d{3}$`（hd 规则）才参与归属；名形如 `/1RX-RM03-R301`。
- **房间码** `fn::room_code($pe)`：`{前缀}-{房号}`（前缀取面板 owner 名首段），如 `1RX-R301`。
- **最强归属** `fn::room_num_of($pe)`：按排序键取首条。
- **待重算**：gen-model 房间重算任务表（`model_update_pending.rs::count_room_targets` 读的那张，动作 `room_recalc`）；plant-ui 直连同一个库可径直查。

元素级查询（叶子）：

```sql
SELECT room_num, inside_count, center_dist, in AS panel,
       (SELECT VALUE in FROM room_panel_relate WHERE out = $parent.in)[0] AS room
FROM room_relate WHERE out = $pe
ORDER BY inside_count DESC, center_dist ASC, room_num ASC
```

---

## 四、开工前要拍板的决定（请直接在本节批注）

| # | 问题 | 默认取向 | 理由 |
|---|---|---|---|
| 1 | 容器元素（EQUI / BRAN / STRU…）的房间怎么算 | **聚合直接子层的归属边**，强度取子层最强；菜单 meta 注明「来自成员」 | 边只挂叶子；不聚合则 EQUI 右键永远「无房间」，与用户直觉相悖 |
| 2 | 视口「房间视图」第一版形态 | **隔离成员 + FocusBounds 取景**；设计稿的压暗 + 亮框后置到二期 | 压暗要动 Bevy 材质 / 后处理，成本高；隔离 + 取景已达成「看这间房」的语义 |
| 3 | 房间浏览器载体 | **`egui::Window` 浮窗**（参照 `model_update.rs` 窗体做法） | 设计稿是 1040×720 浮窗；用 Pane 会挤占 dock 且打开频率低 |
| 4 | 房号命名校验在 plant-ui 侧怎么配 | 关键词读 `DbOption.toml` 的 `room_key_word`；末段正则第一版**写死 hd**（`^[A-Z]\d{3}$`），hh 项目接入时再配置化 | gen-model 用编译期 feature 选；plant-ui 没有对应 feature，先不过度设计 |
| 5 | 元素归属的查询时机 | **跟随选中预取**（与 Props 同拍发 `Req::ElementRooms`） | 查询极轻（单元素十几条边）；菜单打开瞬间就有数据，不用画加载态菜单 |
| 6 | 属性面板标识行加房号芯片（S13-B 画了） | **做**，点击 → 激活房间页签 | 顺手且是设计稿的一部分；不做则页签少一个自然入口 |

---

## 五、分阶段任务

### M1 · 数据层（`plant-ui-data` 新增 `src/room.rs`，约 1 天）

```rust
pub struct RoomRelation {
    pub room: RefU64,          // 房间 FRMW
    pub room_num: String,      // R301
    pub room_code: Option<String>, // 1RX-R301（fn::room_code）
    pub panel: RefU64,         // 归属经由的面板
    pub inside_count: u8,      // 0-8
    pub center_dist: f32,
    pub via_member: bool,      // 决定 1：来自子层聚合
}
pub async fn element_rooms(refno: RefnoEnum) -> Result<Vec<RoomRelation>>;
pub async fn room_detail(room: RefnoEnum) -> Result<RoomDetail>;   // 名称/房号/面板集/成员数/成员预览/待重算
pub async fn rooms_overview() -> Result<Vec<RoomOverviewRow>>;     // S14 全表
```

验收：单元测试跑通三个查询（真库或 mem 库注入 `room_relate` / `room_panel_relate` 样本），排序口径与 ADR-010 §5 逐字一致；容器聚合用一个 EQUI 样本验证。

### M2 · 宿主通道（`plant-ui-app/src/data.rs`，约 0.5 天）

- `Req::ElementRooms(RefU64)` / `Req::RoomDetail(RefU64)` / `Req::RoomsOverview`，对应三个 `Evt`。
- App 持 `RoomVm`（四态，照 `PropsVm::begin_query` 防闪烁）；选中元素变化时随 Props 一并预取（决定 5）。

验收：选中元素后 `RoomVm` 在一拍内到 Ready；断库时到 Failed 且面板画 `pane_note` 错误态。

### M3 · 命令与视口动作（`plant-ui/src/lib.rs` + `plant-ui-view3d`，约 1–1.5 天）

```rust
// Cmd 新增
ShowRooms(RefU64),      // 激活房间页签并对准该元素
OpenRoomBrowser,
FocusRoom(RefU64),      // 宿主展开为：隔离成员 + FocusBounds
// ModelAction 新增
FocusBounds { min: [f32;3], max: [f32;3] },  // 房间 FRMW 无几何实体，不能复用 Focus(refno)
Isolate { refnos: Vec<RefU64> },             // 记录隔离前可见性，供「退出隔离」恢复
```

- 宿主处理 `FocusRoom`：取 `RoomDetail` 的成员集 → 合并 `view.bounds` 里已加载成员的 AABB（一个都没加载时先走 `SetVisible` 加载路径）→ 下发 `Isolate` + `FocusBounds`。
- 视口 HUD 加「房间视图 · R301」段与「退出隔离」按钮（复用 HUD 既有结构）。

验收：Bevy 宿主里点「缩放到房间」相机框住全部已加载成员；退出隔离恢复原可见性；独立壳整条不出现。

### M4 · 树 / 视口右键菜单（`tree.rs::row_menu` + `view3d.rs`，约 0.5 天）——**最小可用闭环在此**

- live 段之后加 `ui.menu_button("查看所属房间")`：子项按强度排序，行 = 房号 + 「主归属」徽章 + `顶点 n/8 · 距中心 x.x m` meta；点击 → `FocusRoom` + `ShowRooms`。
- 尾部固定两项：「打开房间浏览器…」（`OpenRoomBrowser`）、「在「房间」页签中查看」（`ShowRooms`）。
- 无归属：置灰一行「无所属房间」。数据未到（预取失败后重试中）：置灰「查询中…」。
- 视口右键构件复用同一段构造函数，两入口菜单内容一致。

验收：右键 → 点房间 → 视口聚焦 + 页签展开，全链路一次点击走通。

### M5 · 房间页签（新 `workbench/room.rs` + `Pane::Room`，约 1.5 天）

- `Pane` 枚举加 `Room`，标题 `(ph::DOOR_OPEN, "房间")`，默认布局与 Properties 同组；`Cmd::ShowRooms` → `WorkbenchState::focus(Pane::Room)`。
- 按 S13-B 稿实现：待重算横幅（点击 → `focus(Pane::TaskQueue)`）、归属概要卡（房间码 / 面板 / 成员 / 强度 + 缩放到房间 / 隔离成员）、全部归属列表（点击切换聚焦房间）、房间构成属性组、成员预览（点击 → `Cmd::LocateElement`）。
- 属性面板标识行加房号芯片（决定 6）。
- 四态复用 `widgets::pane_note`；样式全部走 `style/tokens`。

验收：与 S13-B 稿逐块对照；切换元素不闪烁；深浅主题 / 紧凑密度不破版。

### M6 · 房间浏览器（约 1 天）

- `egui::Window` 浮窗（决定 3），数据 `Req::RoomsOverview`；检索与四态筛选本地过滤；行操作：定位（`FocusRoom`）/ 隔离。
- 命名不合规行：红色警示，注明「不参与归属计算」；空房间行操作置灰。

验收：与 S14 稿对照；124 间量级滚动流畅（虚拟滚动可后置，先实测）。

---

## 六、风险与坑

1. **房间 FRMW 自身没有几何实体**：`view.bounds` 查不到它，任何「对房间取景」都必须走成员 / 面板 AABB 合并——M3 的 `FocusBounds` 是硬前提，别试图复用 `Focus(refno)`。
2. **成员未加载时取景为空**：合并 AABB 只覆盖已加载实体；第一版遇到空集时先触发成员 `SetVisible` 加载再取景，加载耗时长的房间要有 HUD 反馈。
3. **`fn::room_code` 死代码陷阱**：读它的源码别被开头的 EQUI/BRAN 归口迷惑，真实边在叶子上（第三节）。
4. **并发 agent**：动手前确认没有别的会话在 plant-ui 工作树上写（前车之鉴见 `model-tree-context-menu-and-viewport-toolbar.md` 第一节）。
5. 房间浏览器的全表聚合是一次全库扫描级查询（本项目 2889 FRMW 筛 124 间），放 `Req` 异步且只在打开浮窗时查，不进启动路径。

## 七、工作量与顺序

| 阶段 | 内容 | 估时 |
|---|---|---|
| M1+M2 | 数据层 + 通道 | 1.5 天 |
| M3+M4 | 命令 / 视口动作 + 右键菜单（**最小可用**） | 1.5–2 天 |
| M5 | 房间页签 | 1.5 天 |
| M6 | 房间浏览器 | 1 天 |
| 合计 | | **5.5–6 天**（最小可用 3–3.5 天） |

## 八、明确不做（防蔓延）

- 不做房间的编辑 / 重命名 / 手工归属调整——那是数据侧（E3D / gen-model）的事。
- 不做房间批量重算入口——S12 任务队列已有房间泳道与重试口。
- 不做树行常显房号列——噪声大于收益，右键与页签已覆盖查询路径。
- 视口压暗 + 亮框（S13-B 的完整视觉）二期再议，先用隔离 + 取景达成语义。
