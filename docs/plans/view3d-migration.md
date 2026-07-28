# 三维显示迁进 plant-ui · 迁移计划

把三维显示逻辑从 `old/rs-plant3-d` 搬进 plant-ui，此后 plant-ui 自成一个完整应用，
不再依赖 rs-plant3-d。目标同时支持**原生桌面与浏览器（wasm）两端**。

本文只记这次迁移定下来的东西与要干的活；术语见 `CONTEXT.md`，总体重构脉络见
`ui-rearchitecture.md`（其 M3 那节记的是已作废的「宿主制」，见下）。

## 一、方向变了，旧文档还没跟上

`ui-rearchitecture.md` 的 M3 写的是「rs-plant 改为宿主：装配 plant-ui」，而且标着完成。
代码也还在这么说：`rs-plant3-d/Cargo.toml` 里 `plant-ui-host` 是默认特性，`lib.rs:111`
直接 `plant_ui_host::run()` 就 return，依赖方向是 `rs-plant3-d → plant-ui`。

**新方向是反过来的**：rs-plant3-d 是移植的**来源**，不是落点。三维那一半搬进
`crates/plant-ui-view3d`，搬完 rs-plant3-d 退役。

这条差别很要命，因为它决定「宿主里那个缺口」算什么。宿主里自动增量跑完画面不会自己
对上（详见 §四工作项 3），按旧读法那只是旧代码的毛病；按新读法它是**产品缺口**，
而且这次迁移正是顺手收掉它的机会。

## 二、十二条决定

| # | 问题 | 结论 |
|---|---|---|
| 1 | 三维以什么形态跑 | 新建 `crates/plant-ui-view3d`（Bevy 插件库），`plant-ui-app` 从 eframe 换成 Bevy 装配点 |
| 2 | view3d 对外露什么 | 只露语义入口：`ModelAction` 扩成能表达 get work 那次重载，Bevy Event 全部内化为 crate 私有 |
| 3 | 可见性台账住哪层 | 应用层。view3d 无状态，只执行与回执 |
| 4 | 树的唯一表示 | `E3dTree`（`vendor/rs-core/src/basic/tree.rs`，id_tree），全局剪枝翻译过去 |
| 5 | 双端异步桥 | 全盘采用 `bevy-wasm-tasks`（含 `run_on_main_thread`），移植范围内那 24 处照搬 |
| 6 | 外壳 `Req`/`Evt` | 保留 `Evt` 与单一排空点（`pump_events` 改成 Bevy system），请求侧改成 per-request `spawn_auto`，`Req` 枚举删掉 |
| 7 | HTTP / WS 客户端 | `ehttp` + `ewebsock`，两端一套代码 |
| 8 | 浏览器侧凭据 | 沿用 `DbOption` 整份下发，接受内网信任模型 |
| 9 | 第一版范围 | 按 `Cmd` 契约倒推：绘制层已在发的命令都要有实现；`ToggleMeasure` 例外，留空并置灰 |
| 10 | 怎么切 | 两步走：先只换地基（功能一个不变）并验死，第二步才落三维 |
| 11 | 自动刷新撞上人在用 | 自动重载 + 「正在操作就推迟」的闸（相机在动 / 测量中 / 框选中挂起，手一松补上） |
| 12 | 回归网 | 数值基准：refno → 实体数 / AABB / 世界变换，搬前录、搬后比 |

### 几条需要展开的理由

**决定 3（台账抬到应用层）** 依据 ADR-0010 自己的定义：台账记的是**下过的指令**、不是渲染
结果。那它就是应用状态而不是渲染状态。副作用有两条真的：`despawn_hidden_models` 现在是
「遍历 `RefnoMeshEntMgr` 找实体还在但台账说隐藏的」，台账抬走之后要么先问 view3d 手上有
哪些实体、要么把卸载名单算好传下去；台账变更触发树重平原本白嫖 Bevy 的 `is_changed()`，
抬层后要自己做脏标记。

**决定 5（照搬 `run_on_main_thread`）** 是权衡后选的低摩擦路线，不是没有代价。它与决定 2 / 3
存在张力：`run_on_main_thread` 让异步任务中途伸手进 World，正是「无状态执行器」的反面。
只要 `TaskContext` 不越出 view3d crate，决定 2 仍然成立。建议顺手把 `from: String` 换成
发派时发号的 `ReloadTicket(u64)`（约 20 行），把「谁的回执先到谁解锁」那类 bug 从字符串
对暗号变成类型匹配——这一条未定，可以跳过。

**决定 8（整份下发 DbOption）** 的残留风险要如实记着：`DbOption` 里有 SurrealDB root 口令、
一串 `mysql://{user}:{pwd}@…`（`options.rs:412`）、MQTT host，以及整个 `SecondUnitDbOption`
的二号机组凭据，而界面一个字段都用不上。浏览器里这些谁都能看到。**界面本身是纯读的**——
`plant-ui-data` 里 UPDATE / CREATE / DELETE / INSERT / RELATE 零命中，写入全走 gen-model 的
HTTP——所以将来想收紧，一个 `ROLES VIEWER` 的库级用户就够，查询一行不用改。

**决定 9（按 `Cmd` 契约倒推）** 的意思是：`plant-ui/src/lib.rs` 里绘制层已经在发
`PickViewport` / `Camera(CameraGesture)` / `ResizeViewport` / `Model(ModelAction)`，而
`plant-ui-app` 里这四条全是空实现。搬完还留着空实现，等于「三维视口能用了」说了没做到。

## 三、要搬什么

按决定 9，第一版约 3000–4000 行，**跨两棵目录**——「从 `e3d_plugin` 搬」这个说法从一开始
就不完整：

| 要的东西 | 位置 | 行数 |
|---|---|---|
| 几何装卸、`RefnoMeshEntMgr` | `e3d_plugin/systems/model_system.rs` | 958 |
| 树分支重查 `FetchChildrenNodesEvent` | `e3d_plugin/systems/pdms_nodes_system.rs` | 388 |
| 剔除 | `editor_ui/viewer/culling.rs` | 366 |
| 事件定义（含 `ShowModelEvent` / `ShowModelFinished` / `FocusModelEvent`） | `e3d_plugin/pdms_events/mod.rs` | 244 |
| 相机控制器 | `editor_ui/camera_plugin.rs` | 169 |
| 拾取 | `editor_ui/viewer/picking.rs` + `select.rs` + `e3d_plugin/systems/selection_system.rs` | 118+71+109 |
| 可见性台账（按决定 3 落到应用层） | `e3d_plugin/visibility_ledger.rs` | 101 |
| HideAll | `e3d_plugin/systems/hide_all_mesh.rs` | 50 |
| get work 编排、手势翻译 | `plant_ui_host.rs`（含 `:1122-1160` 手势、`:1759` 起的测试） | 约 240 |

**明确不搬**：`e3d_plugin/manual_model_update.rs`（1074 行，旧的手动增量更新界面，plant-ui
已有 `model_update.rs` 3408 行取代）、`systems/http_system.rs`（463 行，对应
`model_update_api.rs`）、`review3d/` 整棵（约 2600 行）、`trees/` 里 room / ssc / metadata /
review 四棵（约 1450 行）、`p2p/`、`user/`、`login_system.rs`、`upload_file.rs`、
`download_file.rs`、`export_system.rs`、`show_2d_projection_system.rs`、`ptset.rs`。

`ToggleMeasure` 对应的 `measure_plugin`（592 行）与 `distance_measurement` /
`angle_measurement` 两个插件不进第一版，工具栏按钮置灰。

**一个好消息**：`on_screen()` 依赖的 `fn::ancestor` 已经在 plant-ui 自己的
`resource/surreal/common.surql` 里，数据库侧不用动。

## 四、工作项

### 必须先于动手

1. **录几何数值基准。** rs-plant3-d 现在还能跑，退役之后就再也录不出「搬之前是什么样」。

   **导出器已经写好了**：`rs-plant3-d/src/plugins/e3d_plugin/geo_baseline.rs`，挂在
   `PdmsPlugin` 上（旧壳与 plant-ui 宿主都装它，所以在哪个壳上录都行），
   `cargo check --lib` 通过。用法：

   ```
   set PLANT_GEO_BASELINE=D:\...\baseline-before.txt
   # 跑起来，显示那批固定 refno，然后按 Ctrl+Alt+B
   ```

   每行一个 refno，按 refno 串排序（BiMap 迭代序不稳，不排就逐行对不上），字段是
   mesh 数 / 可见性 / 是否参与负实体运算 / 世界 AABB / Transform。取数取的是 refno 那一层
   的实体——`model_system.rs:468-472` 正是在那一层挂 `ParryAabb` 与 `Transform` 的，
   真正的 mesh 在后代上，所以 mesh 数顺 `Children` 往下数。

   两个已知边界：浮点按 `PRECISION = 4` 定死位数落盘（为了 `diff` 直接可用，代价是容差
   写死在那个常量里）；材质、颜色、法线朝向都不在里面，**它是网不是全部**，人眼那一关
   还得过。

   还欠的：**搬完之后 plant-ui 侧要按同样格式写一份对应的导出器**，以及定一批固定 refno。
2. **回填 `ui-rearchitecture.md` 的 M3 那节**（已随本文一并做）。

### 搬的时候别错的

3. **`pump_model_update` 是删不是搬。** `ModelUpdateVm::Running` 在 `plant_ui_host.rs` 里
   出现三次（`:929` / `:1496` / `:1641`），**全是模式匹配、零构造点**——ADR-0011 合流之后
   execute 回执直接回 `Idle`。所以 `pump_model_update`（`:890`）永远吐不出东西，
   `queue: task_queue::Vm`（`:148`）那个字段存着但没人喂。活的那条自动链在
   `plant-ui-app`：`newly_finished`（`main.rs:900`）→ `refresh_for_units`（`:923`）→
   `start_get_work`（`:880`）。接的是这一条。
4. **宿主 `ModelUpdateBridge` 不能照搬。** `plant_ui_host.rs:182` 是 mpsc +
   `std::thread::spawn`，而 `:190` 那行 `#[cfg(not(target_arch = "wasm32"))]` 意味着浏览器里
   工作线程压根不起：channel 建了、请求发进去、没有人应答。不是编译错误，是**静默无响应**。
5. **`refresh_for_units` 里那条别丢**：除了 `root` / `old_owner` / `new_owner`，还要加上
   本端树里记着的当前父节点——元素被移走时后端的 `old_owner` 未必解得出来，本端的缓存却
   还留着移动前那一头。
6. **宿主 `:1759` 起那批相机手势测试跟着搬**（`PendingGesture` / Drag / Zoom）。
7. **忙碌态改成等齐两半。** 合并之后 get work 有树/属性与几何两半要等，而现在两边各等各的：
   独立壳只等数据（它没有几何），宿主只等几何。

### wasm 挡路的

8. **rs-core 那两处读文件要抬到调用方。** `get_db_option()`（`lib.rs:122`）与
   `init_surreal()`（`:243`）各自独立读一次 `DbOption.toml`、各自 `.unwrap()`，浏览器里
   两处都是开机即 panic。同文件 `get_default_pdms_db_info()`（`:135`）已经有个 `load_file`
   feature 在「读文件 vs `include_str!`」之间切，可以照那个先例办。
9. **`ws://` 在 HTTPS 页面里会被当混合内容拦掉。** `get_version_db_conn_str()`
   （`options.rs:384`）拼的就是 `ws://{ip}:{port}`。要么页面走 HTTP（内网可以），要么
   SurrealDB 上 `wss://`。
10. **`ureq` / `tungstenite` 换成 `ehttp` / `ewebsock`**（约 506 行：`model_update_api.rs` 256
    + `model_update_ws.rs` 250）。`ewebsock` 给的是一个每帧 poll 的接收端，正好接决定 6 的
    单一排空点；`model_update_ws.rs` 里处理重连、读超时、`set_read_timeout(TcpStream)` 那
    几段可以整段删掉。
11. **`.cargo/config.toml` 要新增四条源替换条目**（实测，见 §六）：
    `accesskit.git`、`winit`、`naga-oil.git`、`bevy-wasm-tasks`。bevy 与 wgpu 的条目已经写好了。
    另外该文件头自己写着：**重跑 vendor 前先移开本文件**。
12. **`std::thread` 共 8 处要清**（`data.rs` 7 处 + `model_update_ws.rs` 1 处），根
    `Cargo.toml:25` 的 `tokio` 开着 `rt-multi-thread`，wasm32-unknown-unknown 上没有。

### 得明说的

13. **M0-3 那道闸门会被撞。** 计划书记着「增量 `cargo check` 6.1s，闸门 15s」，而整个
    bevy + bevy_egui35 + bevy-wasm-tasks 进来之后这个数一定会动。计划书对撞线的处置原话是
    「不通过就改架构」。我的看法是真撞了应该**改闸门**——那道闸当初防的是 `aios_core` 情不
    自禁拖进来的依赖体量，不是防你故意引入一个渲染引擎——但那得是一次明说的修订。

## 五、施工顺序（决定 10）

**第一步：只换地基。** `plant-ui-app` 从 eframe 换成 Bevy 装配点，视口仍是占位纹理，
**功能一个不变、界面一个像素不变**。

这一步的判据零歧义，而且是整个迁移里唯一一次能有这种判据的机会：像素采样那套
（`PrintWindow` 抓窗 + 比对令牌值，见 `design/DESIGN-SYSTEM.md`）现成，
`cargo test -p plant-ui` 现成，通过条件就是「这些全过、截图对得上」。

这一步还要**前置验证探针**：`egui_inspection` 是通用 `egui::Plugin`，自带
`attach_from_env(&ctx)`，eframe 只是替你自动挂上；bevy_egui 里手动挂一行即可，控件树与
输入注入照旧。**只有 `shot` 与 `resize` 要验**——它俩走 `send_viewport_cmd`，bevy_egui
未必实现。那是整套验收手段的地基，要在搬 4000 行之前知道它瘸没瘸。

一处不能直接认的既有结论：计划书说「egui 桥接已过，`bevy_egui35` 打通」，但那是在
**rs-plant3-d 的 workspace 里**打通的。plant-ui 这边 lock 里只有 `bevy_ecs` /
`bevy_transform` 0.16.0-dev，完整 bevy、bevy_render、bevy_egui 一个都没有。换了 vendor 树、
再加一个 wasm 目标，那份「已过」不能直接搬过来用。

**第二步：落三维。** §三那 3000–4000 行，判据是 §四工作项 1 的数值基准。

## 六、依赖增量实测（2026-07-28）

把 `bevy` / `bevy_egui35` / `bevy-wasm-tasks` / `ehttp` / `ewebsock` 加进清单，移开
`.cargo/config.toml` 之后跑 `cargo metadata` 解析了一次完整依赖图（**只解析、没真跑
`cargo vendor`**，19 秒完成；探完清单已回滚）。bevy 的特性列表照抄 rs-plant3-d，
`aios_core` 一并开了 `render`。

**lock 包数 906 → 1024，增 118。** 分布：

| 来源 | 个数 |
|---|---|
| crates.io | 75 |
| `gitee/happydpc/bevy` | 29 |
| `gitee/happydpc/accesskit.git` | 5 |
| `gitee/happydpc/wgpu` | 4 |
| `gitee/happydpc/winit` | 2 |
| `gitee/happydpc/naga-oil.git` | 1 |
| `gitee/happydpc/bevy-wasm-tasks` | 1 |
| `rs-plant3-d/vendor/bevy_egui35`（path） | 1 |

**决定 7 的边际成本确认约等于零：`ehttp` + `ewebsock` 一共只增 2 个 crate。** 推断属实——

```
ehttp 0.7.1     deps: document-features, js-sys, ureq,       wasm-bindgen, wasm-bindgen-futures, web-sys
ewebsock 0.8.0  deps: document-features, js-sys, log, tungstenite, wasm-bindgen, wasm-bindgen-futures, web-sys
```

原生后端就是仓库里已有的 `ureq` 与 `tungstenite`，其余依赖也全都已在 lock 里，
`document-features` 同样已有。（`ehttp` 实际解析到 **0.7.1**，不是先前估的 0.5.x。）

**剩下 114 个全是渲染栈**：`bevy_*` 一族、`wgpu` / `wgpu-core` / `wgpu-hal` / `naga` /
`naga_oil`、`winit` / `accesskit*` / `dpi`、后端 `ash` / `metal` / `glow` / `khronos-egl`、
文本 `cosmic-text` / `swash` / `rustybuzz` / `fontdb` / `ttf-parser`、布局 `taffy`、
`tiny-skia`、`encase*`、`gpu-alloc*` / `gpu-allocator` / `gpu-descriptor*`、
`windows` 0.58 一族。**这一份是「引入一个渲染引擎」的真实标价，跟本次别的决定都无关。**

`bevy_egui35` 是 path 依赖（`rs-plant3-d/vendor/bevy_egui35`，13 个文件 0.2 MB），
`cargo vendor` 不管它，要手工把源码树复制进 `plant-ui/vendor/`。

### 仍然没核实的

- **M0-3 那道闸门到底撞多狠没量。** 要量就得真把这 118 个 crate vendor 下来并完整编一次，
  代价是几十分钟与若干 GB（D 盘当时剩 38.8 GB）。工作项 13 的处置不变。
- 第一版 3000–4000 行是按 §三那张表估的，依赖闭包（`defines.rs` / `consts.rs` /
  `globals.rs` / `config.rs` 要带多少）没有逐个核过。
