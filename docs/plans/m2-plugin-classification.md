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

其中两处容易踩：

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

1. 先删那 21 个不参与编译的目录，连同 `plugins/mod.rs` 里 19 行注释。零风险，
   且能把仓库体量先降下来，后面的搜索与 grep 都更干净。
2. 再删 4 个「整体删除」的插件，同步摘掉 `lib.rs` 的 `use` 与 `add_plugins`。
3. 处理第四节那两处事件耦合——把 `add_event` 移到消费方所在的插件。
4. 然后才动 5 个「仅删 UI」的，逐个把面板代码与系统拆开。
5. 最后收 `EditorTab` 枚举（M2-2）与 `editor_ui` 外壳（M2-4）。

前两步做完就能跑一次 `cargo check` 拿到干净的基线，再往下每步都能验证。

## 六、一处还没定的事

`D:\work\plant-code\old\rs-plant3-d` 与 `D:\work\plant-code\rs-plant3-d` **是两份**，
都在分支 3.0 上。前者停在 `fe8fb07a`、101 个脏文件；后者多一笔
`519bddea add manual incremental model update UI`、183 个脏文件。

本文分析的是前者，理由是计划文档写明 `old\` 是独立工作环境、rs-plant3-d 就在它旁边。
但后者更新。**M2 真要动手删代码之前，得先确认改哪一份。**
