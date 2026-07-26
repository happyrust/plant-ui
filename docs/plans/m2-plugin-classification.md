# M2 · rs-plant 侧插件分类与删除清单

- 对象：`D:\work\plant-code\old\rs-plant3-d`（分支 3.0，HEAD `fe8fb07a`）
- 日期：2026-07-27
- 依据：`docs/plans/ui-rearchitecture.md` 的 M2-1 / M2-2 / M2-3
- 性质：**只读分析，未改动 rs-plant 任何文件**

M2-3 说「38 个 plugin 里 UI 与业务逻辑混在一起，删之前必须逐个分清」。逐个分清之后，
第一个结论是这句话的分母不对。

---

## 一、38 个目录里只有 17 个在编译

`src/plugins/mod.rs` 是唯一的模块声明处，实测：

| 状态 | 数量 | 含义 |
|---|---|---|
| 参与编译 | 18 | 其中 `web_plugin` 带 `#[cfg(target_arch = "wasm32")]`，原生构建只有 **17** |
| 已在 `mod.rs` 里注释掉 | 19 | 编译器看不见 |
| 目录在、`mod.rs` 里连注释都没有 | 2 | `docs_plugin`、`fuzzy_search_plugin` |

**21 个目录已经是死代码。** 它们不参与编译，因此不可能有任何东西依赖它们——
删除风险接近零，不需要走 M2-3 那套「逐个分清」的流程。

已注释的 19 个：`account_plugin`、`atta_plugin`、`color_setting_plugin`、
`component_warehouse_plugin`、`hydraulic_plugin`、`mechanical_model_plugin`、
`metadata_plugin`、`pcf_plugin`、`pdf_plugin`、`pdms_modeler_plugin`、
`personnel_management_plugin`、`pipe_plugin`、`project_plugin`、`room_setting_plugin`、
`search_plugin`、`ssc_setting_plugin`、`suspension_and_support_plugin`、
`through_wall_construction_plugin`、`virtual_hole_plugin`。

### 删除前的核对（已做）

「不参与编译所以删了没事」这句要经得起查，核过四项：

1. 全仓库没有任何 `#[path = ...]` 指向这 21 个目录；`include!` / `include_bytes!`
   只有四处，都指向 `assets/`，唯一落在待删目录里的那处在 `docs_plugin` 自己身上。
2. `Cargo.toml` 里没有提到它们中的任何一个。
3. 活代码里唯一提到的是 `virtual_hole_plugin`，出现在 `e3d_plugin` 的
   `data_state/state.rs:19` 与 `asset_loader/virtual_hole_loader.rs:6`，**且没有被注释**。
4. 但这两个文件本身也没参与编译——`data_state/mod.rs` 里是 `// pub mod state;`，
   `asset_loader/mod.rs` 里是 `// pub(crate) mod virtual_hole_loader;`。

第 3、4 两条合起来是这次核对最值得记的一点：**判据不能是「文件在不在」，得是
「文件能不能从模块树走到」**。按文件存在与否去数，`e3d_plugin` 里这两个文件会被
误判成活代码，进而把 `virtual_hole_plugin` 误判成还有人依赖。

顺带得到一个附加结论：**死代码不只在那 21 个目录里，活插件内部也有**
（`e3d_plugin/data_state/state.rs`、`asset_loader/` 下三个 loader 等）。
这些不在 M2 的清单上，但清理它们的风险与那 21 个目录同级。

### 两处容易踩的坑

- **`metadata_plugin` 与 `meta_data_plugin` 是两个不同的目录**，只差一个下划线。
  前者已注释、是死的；后者在编译、注册着 `EditorTab::MetaDataTab`。别删错。
- **`docs_plugin` 从未被声明**，所以 `DocsPlugin` 也从未被 `add_plugins`，
  `EditorTab::PdmsDocs` 在运行时根本没有注册——真点开那个页签只会看到
  `EditorTabViewer::ui` 的兜底文案 `Not implemented panel`。

## 二、EditorTab 的实际注册情况

枚举在 `src/editor_ui/editor_tab.rs:18`，**20 个值**（计划文档写的 23 对不上，可能是当时的状态）。

| EditorTab | 注册处 | 去留 |
|---|---|---|
| `SceneView` | `editor_ui/editor_view.rs:30` | **留**（三维视图） |
| `PdmsSceneTree` | `plugins/e3d_plugin/plugin.rs:67` | **留**（模型树） |
| `PdmsAttView` | `plugins/e3d_plugin/plugin.rs:68` | **留**（属性） |
| `PdmsConsole` | `plugins/console_plugin/mod.rs:20` | **留**（命令行） |
| `DistanceMeasurement` | `editor_ui/ui_plugin.rs:229` | 删页签，留逻辑 |
| `PbsSceneTree` | `plugins/pbs_plugin/plugin.rs:42` | 删 |
| `MetaDataTab` | `plugins/meta_data_plugin/plugin.rs:27` | 删 |
| `Review3D` | `plugins/model_review_plugin/plugin.rs:64` | 删 |
| `StatusSetting` | `plugins/status_plugin/plugin.rs:44` | 删 |
| `VersionView` | `plugins/version_plugin/plugin.rs:57` | 删 |
| `VersionTimeline` | `plugins/version_plugin/plugin.rs:58` | 删 |
| `VersionTable` | `plugins/version_plugin/plugin.rs:59` | 删 |
| `PEVersionHistory` | `plugins/version_plugin/plugin.rs:60` | 删 |
| `ToastTest` | `plugins/toast_test_plugin/mod.rs:14` | 删 |
| `ControllableToast` | `plugins/controllable_toast_plugin/mod.rs:15` | 删 |
| `PdmsDocs` | `plugins/docs_plugin/plugin.rs:20`，但该模块未编译 | 已经是死的 |
| `MaterialStatisticReport` | `plugins/material_statistic_plugin/plugin.rs:27` | **计划没说**，见下 |
| `PdmsModelReview` | 无 | 枚举值从未注册 |
| `SearchTreeDemo` | 无 | 枚举值从未注册 |
| `Other(String)` | 无 | 兜底值，M2-2 的「必要内部值」 |

**计划那 12 个删除项，数目对得上、成员对不上。** 代码里写了 17 处注册，
其中 `PdmsDocs` 所在模块未编译，运行时实际注册 16 个，减去要留的 4 个正好 12 个。
但 M2-1 的清单里有 `PdmsDocs`（它已经是死的，不用删页签，直接删目录），
缺 `MaterialStatisticReport`。

按三层外壳的划分，S7 防火封堵材料统计属于**任务窗**而不是 dock 常驻视图，
所以它的 dock 注册照样得摘掉，只是面板代码将来要以任务窗形式回来
（`ui/plugging_material_statistics/` 那一支是另一份代码，不在 `plugins/` 下，需一并核对）。

## 三、逐个分类

对象是那 12 个页签背后的 **8 个活插件** 加 1 个已死的。

| 插件 | 结论 | 依据 |
|---|---|---|
| `toast_test_plugin` | **整体删除** | 只有一个页签和一个把 `CloseToastEvent` 转成 `ClearToastMessage` 的转发系统。`CloseToastEvent` 定义在 `ui/structs.rs:26`，全仓库只有这个插件在用 |
| `controllable_toast_plugin` | **整体删除** | 演示件。`simulate_network_requests` 用 `rand::random::<bool>()` 编造网络请求成败，没有真业务 |
| `meta_data_plugin` | **整体删除** | `build()` 除页签外只剩一句 `insert_resource(MetaDataUI::test_data())`，其余全被注释。数据是硬编码测试数据 |
| `docs_plugin` | **整体删除** | 未参与编译，删了对构建零影响 |
| `distance_measurement` | **仅删 UI，保留逻辑** | `DistanceMeasurementState` 被 `editor_ui/viewer/toolbar.rs` 读取——三维视口工具栏依赖它。页签是在插件外面注册的（`ui_plugin.rs:229`），删除面限于 `ui_plugin.rs` 的 57 / 229 两行与 `mod.rs:282-284` 的 `DistanceMeasurementTab` |
| `status_plugin` | **仅删 UI，保留逻辑** | `apply_node_status` 会写回节点状态，是数据变更而非展示；`fetch_status_code_map` 是查询。三个系统都在 `E3dAppState::InitFinished` 下跑 |
| `model_review_plugin` | **仅删 UI，保留逻辑** | `ViewportStore` 与 `restore_viewport_system` 动的是相机视口，`handle_file_list_event` 走文件服务。905 行的 `plugin.rs` 里 UI 与这些混在一起，得拆 |
| `pbs_plugin` | **仅删 UI，保留逻辑** | 见下，有跨插件事件耦合 |
| `version_plugin` | **仅删 UI，保留逻辑** | 见下，有跨插件事件耦合 |

## 四、两处会把「留下的四个视图」一起弄坏的耦合

这是 M2-3 那条风险的具体形态，删之前必须先处理：

**`ShowSscModelEvent`** —— 定义在 `e3d_plugin/pdms_events/mod.rs:93`，
由 `e3d_plugin/systems/show_ssc_system.rs:17` 消费，
但 `add_event` 是 **`pbs_plugin/plugin.rs:48`** 做的。
整体删掉 `pbs_plugin` 会抽走注册，而读它的系统留在 e3d_plugin 里，Bevy 取事件资源时会炸。

**`VersionFetchEvent`** —— `add_event` 在 `version_plugin/plugin.rs:62`，
而写它的是 **模型树**：`e3d_plugin/trees/e3d_tree.rs:138-139`
与 `trees/searchable_e3d_tree.rs:16`。模型树是要留的四个视图之一，
删掉 `version_plugin` 就等于让留下来的视图去写一个没注册的事件。

两处的处置是同一个形状：事件的注册要跟着**消费方**走，不能留在被删的插件里。

## 五、建议的执行顺序

1. ~~先删那 21 个不参与编译的目录，连同 `plugins/mod.rs` 里 19 行注释。~~
   **已做**：`old\rs-plant3-d` 的 `929d45d1`，20 个目录 / 124 个文件 / -15684 行
   （`account_plugin` 只在 mod.rs 里有注释，目录本身不存在）。
   随后 `cargo check` 干净通过（1m50s，exit 0），印证了「它们不参与编译」这个判断。

   提交时只暂存了删除相关的路径。那棵树上原有的 101 个未提交改动**一个都没碰**——
   其中 74 个未跟踪文件里六十多个是日志与截图产物，另外 27 个已跟踪的是别人未完成的
   源码改动，把它们一并提交既会污染仓库也会张冠李戴。`plugins/mod.rs` 当时恰好是干净的，
   所以改它不会裹挟别人的改动。
2. 再删 4 个「整体删除」的插件，同步摘掉 `lib.rs` 的 `use` 与 `add_plugins`。
3. 处理第四节那两处事件耦合——把 `add_event` 移到消费方所在的插件。
4. 然后才动 5 个「仅删 UI」的，逐个把面板代码与系统拆开。
5. 最后收 `EditorTab` 枚举（M2-2）与 `editor_ui` 外壳（M2-4）。

前两步做完就能跑一次 `cargo check` 拿到干净的基线，再往下每步都能验证。

## 六、两份 rs-plant3-d：本文的结论通用，但动手改哪一份要先定

`D:\work\plant-code\old\rs-plant3-d` 与 `D:\work\plant-code\rs-plant3-d` **是两份**，
都在分支 3.0 上。

**先说结论通用性**：本文第一、二、三节的分类对两份都成立。逐项核过——
`plugins/` 下 38 个目录的集合、18 个参与编译的模块、19 个已注释的模块，两份**完全一致**；
`editor_tab.rs`、`plugins/mod.rs`、`lib.rs`、`material_statistic_plugin/plugin.rs`
的差异全是 rustfmt 的导入排序与空白，无语义差别。

**但两份是双向分叉的，不是简单的一前一后**：

| | `old\rs-plant3-d` | `plant-code\rs-plant3-d` |
|---|---|---|
| HEAD | `fe8fb07a` | `519bddea`（多一笔 manual incremental model update UI） |
| 脏文件 | 101 | 183 |
| `ui_plugin.rs` 最后改动 | **07-27 00:37**（今天） | 07-25 14:33 |
| 默认 dock 布局 | **已裁到 5 个页签** | 仍挂着 VersionView / PdmsDocs / Review3D / VersionTimeline / VersionTable / PEVersionHistory |
| `setup_style` | 走 `RsTheme::default()` 的成套样式 | 内联 `RSStyle` 描边 |
| Ctrl+U 手动更新快捷键 | 无 | 有 |

关键的一格是**默认 dock 布局**：`old\` 那份的工作区里已经把六个页签从默认布局里摘掉了，
这本身就是 M2 的一部分，已经做了一半。`plant-code\` 那份还是全的。

所以「谁更新」这个问题没有单一答案——提交历史上 `plant-code\` 领先一笔，
但工作区里 `old\` 有更靠近 M2 目标的未提交改动。**动手删代码之前必须先定改哪一份，
否则会和已经做掉的那半截打架。**
