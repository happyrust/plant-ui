# 布置平台 UI 重构 · 开发计划

- 形态：**脱离 worktree，另起独立项目**，落位 `D:\work\plant-code\old\plant-ui`（用户指定：`old\` 是与 `plant-code\` 主目录分开的独立工作环境，旁边已有 gen-model / pdms-io / rs-plant3-d / rs-server）
- 日期：2026-07-26
- 设计源：`design/rs-plant3d-ui.pen`、`design/DESIGN-SYSTEM.md`。**画板只有 S1–S4**（主工作台深浅两版、模型更新的预览与执行进度）加 6 个组件；`DESIGN-SYSTEM.md` 第一节那张 S1–S12 的表是规划，S5 往后一张都没画，该表已补「已画」一列
- 前置 ADR：0001 沙盒 crate、0002 ViewModel 层、0003 RSStyle 适配层、0004 组件层单一源

---

## 一、为什么废弃旧 UI 架构

不是「旧代码写得不好」，是**旧结构到不了设计稿**。四条实测证据：

**1. 屏幕模型对不上。** 设计稿是屏幕流：项目选择（S5，1600×1000 全屏）→ 主工作台（S1，1600×1000 全屏）→ 模型更新（S2/S4，1040×720 弹窗）。画板尺寸本身就在说「哪些是屏幕、哪些是弹窗」。而代码里 `EditorTab` 有 23 个枚举值、注册了 18 个，**一次性任务与常驻视图平起平坐地摆在同一排页签里**：「防火封堵材料统计」「元件库」「模糊查询」和「三维视口」「模型树」是同一个层级。在这个结构上打补丁，永远排不出设计稿的样子。

**2. 没有统一的应用状态机。** 至少 8 个互不相干的 `States` 枚举，分散在 `lib.rs`、`shared.rs`、`ui_plugin.rs`、`app_state.rs`、e3d plugin 等处，其中**有两个同名的 `AppState`**。没人说得清「应用现在处于什么状态」，界面也就无法按状态组织。

**3. 界面与数据源绑死。** 控制台画一屏要套三层 `world.resource_scope`：

```rust
world.resource_scope(|world, mut state: Mut<ConsoleState>| {
    world.resource_scope(|world, console_open: Mut<ConsoleOpen>| {
        world.resource_scope(|world, mut command_entered: Mut<Events<..>>| {
```

这是「边拿数据边画」的必然结果。这样的代码既没法单独验证，也没法脱离 Bevy 运行。

**4. 反馈环太长。** rs-plant 有 60 个直接依赖（29 个 git fork）、1483 个包、186 MB 产物；改一行界面代码的增量构建实测 30 秒到 4 分半。ui-next 沙盒 434 个包，同样一行改动 **1.5 秒**。差了两个数量级。

---

## 二、已定的十条决定

| # | 决定 |
|---|---|
| 1 | 向设计稿靠齐，废弃旧 UI 架构，重新设计，先接测试数据 |
| 2 | 抽独立 UI crate，UI 只依赖「数据接口 + 组件层」 |
| 3 | 不另造 DTO，参照原有查询接口、直接用查到的数据（`SUL_DB.query` + `aios_core` 类型） |
| 4 | 三层外壳：屏幕 / dock 常驻视图 / 任务窗。dock **只要四个**：模型树、三维视图、命令行、属性 |
| 5 | 里程碑 1 只做主工作台 S1 |
| 6 | 新项目用 egui 0.35；bevy_egui 桥接升级单列里程碑 |
| 7 | 测试数据直连本地 SurrealDB，**不做 mock 层** |
| 8 | 旧 `editor_ui` 与不要的面板要删干净 |
| 9 | **脱离 worktree，作为独立项目管理**；依赖放 `vendor/` |
| 10 | vendor 分类处理：第三方只读、自家代码可编辑；**vendor 里的 egui 生态一并升到 0.35** |

### 决定 9 带来的一处顺序修正

原计划按决定 8 把「删旧壳」排在最前。**独立项目之后这条不再是前置条件**——新项目从第一天起就不碰 rs-plant，删除工作自然移到接入之前（M3）。这样中间态也不会出现「应用不可用」的窗口期。

### 决定 6 背后的版本冲突（仍然存在，只是延后）

| bevy | bevy_egui | egui |
|---|---|---|
| **0.16** | 0.34–0.36 | 约 0.31–0.32 |
| 0.19 | 0.40–0.41 | **0.35** |

**egui 0.35 在上游要求 Bevy 0.19。** Bevy 不动就只能自己维护桥接。而 egui 0.34 是破坏性重构（主入口 `Context` → `Ui`、`App::update` → `App::ui`、面板 API 变更、字体后端 ab_glyph → Skrifa）。这笔账在 M3 付。

一个真实收益：egui 0.35 自带 **`egui_mcp`**，AI agent 可直接读取并操作运行中的 egui 应用。本轮开发大量时间浪费在抢窗口焦点、误触关闭应用、按键落进无关程序上，这个能力能从根上消除那类问题。

---

## 三、目标架构

### 三层外壳

| 层 | 判定标准 | 内容 |
|---|---|---|
| 屏幕 | 互斥的全局阶段 | 登录 → 项目选择(S5) → 主工作台(S1) |
| dock 常驻视图 | **需要与三维视口同屏可见** | 模型树、三维视图、命令行、属性 |
| 任务窗 | 有头有尾、完了就关 | 模型更新(S2/S4)、设置(S6)，以及后续 S7/S9/S10/S11 |

### 数据流

```
SurrealDB (本地测试库)
   |  SUL_DB.query(...)         <- aios_core 的全局静态句柄
   v
数据层：把查询结果整理成每屏的 Vm 结构（装 aios_core 原生类型）
   |  &Vm 进
   v
UI 层：纯绘制函数，不认识 Bevy、不认识数据库
   |  Vec<Cmd> 出
   v
派发层：独立应用里直接执行；接进 Bevy 后转成 Event / Command
```

三维视口对 UI 层而言**只是一个 `TextureId`**——已核实：Bevy 把相机渲染到 `RenderTarget::Image`，再经 `EguiUserTextures::add_image` 注册给 egui。所以独立应用里先给占位纹理，接进 Bevy 后换成真相机，UI 代码一行不改。

### 新项目目录

```
D:\work\plant-code\old\plant-ui\
  Cargo.toml              workspace
  .cargo/config.toml      源替换指向 vendor/registry/
  crates/
    plant-ui/             egui 0.35 + 组件层 + 各屏绘制 + Vm 定义
    plant-ui-data/        SUL_DB 查询 -> Vm；不含任何绘制
    plant-ui-app/         独立可执行：eframe + 上面两个 + 占位视口
  vendor/
    registry/             cargo vendor 产物（只读、带校验和）
    rs-core/              aios_core 可编辑源码树（path 依赖，不走校验和）
  design/                 从现仓库带过来：.pen 源文件、DESIGN-SYSTEM.md、画板导出
```

可编辑源码树不能直接混进 `cargo vendor` 的目录源（目录源要求每个子目录都带 `.cargo-checksum.json`），所以只读部分放 `vendor/registry/`、可编辑部分放 `vendor/rs-core/`，都在 `vendor/` 下但机制分开。

新项目独立建 git 仓库，从 M0 起小步提交；本计划文档同时保存一份在新项目 `docs/plans/` 下。

### vendor 分类

| 依赖 | 方式 | 说明 |
|---|---|---|
| 第三方不改的 | `cargo vendor` 只读 + `.cargo/config.toml` 源替换 | 可复现、可离线；改了会报校验和错误 |
| 自家代码 `aios_core` | `vendor/rs-core/` 可编辑源码树 + path 依赖 | **拷贝自 `rs-core-pin` 钉版**（= git rev 5667b70 + get_world 顺延修复，与 gen-model 当前 patch 一致），不是脏检出的 `plant-code\rs-core` |
| Bevy 生态 fork | **M3 才引入**，届时以可编辑源码树进入 | 里程碑 1 用不到 |

`vendor/rs-core` 与 `vendor/forks/*` 是 path 依赖，也就是 workspace 成员：**不要跑 `cargo fmt --all`**，它会把这几棵树整个重排（一次 183 个文件），钉版差异从此看不清。格式化只对自己的三个 crate 跑：`cargo fmt -p plant-ui -p plant-ui-app -p plant-ui-data`。

#### `vendor/rs-core` 的改动记录（换钉版时要重放）

| 位置 | 改动 | 缘由 |
|---|---|---|
| `rs_surreal/query.rs` `get_named_attmap_with_uda` | `pub(crate)` -> `pub` | UI 版属性表 `get_ui_named_attmap` 会把引用与未设值都压成 `StringType`，属性面板判「哪些字段可编辑」需要压之前的原始类型。该函数带 `#[cached]`，多取一次不额外打库 |

### 里程碑 1 的完整依赖清单（已核实全部在 crates.io 上游，**零 fork**）

| crate | 版本 | 用途 |
|---|---|---|
| `egui` | 0.35.0 | 界面 |
| `eframe` | 0.35 | 独立应用外壳 |
| `egui_extras` | 0.35 | 表格/图片 |
| `egui_dock` | **0.20.1**（2026-06-28，标注 egui 0.35） | 工作台 dock |
| `egui-phosphor` | **0.13.0**（2026-07-22，标注 egui 0.35） | 图标 |
| `aios_core` | `vendor/rs-core` path，`default-features = false` | 领域类型与 `SUL_DB` 查询 |

「零 fork」指 egui 侧。`aios_core` 关掉默认 features（occ/gen_model/sql）后避开 OpenCASCADE 与 manifold，`render` 不开就没有 bevy_render/wgpu；但它**无条件**带进 bevy_ecs / bevy_reflect / bevy_math / bevy_transform（gitee bevy fork）、surrealdb fork（protocol-ws）、sqlx(mysql) / duckdb / redb，另有 nalgebra / parry / rstar / calamine 等 8 个 gitee fork（已核对 rs-core-pin Cargo.toml）。它不依赖 egui——新项目里 egui 0.35 是唯一的 egui。这个体量必须实测，见 M0。

---

## 四、里程碑与任务

### M0 · 地基验证（阻塞后续，不通过就回头改架构）

| 任务 | 验收标准 |
|---|---|
| M0-1 建 workspace 骨架与 `vendor/`，配置源替换 | `cargo build` 通过；`cargo vendor` 产物可离线构建 |
| M0-2 引入上表六个依赖 | 记录 Cargo.lock 包数与冷编译耗时 |
| M0-3 增量编译实测 | 改一个 UI 源文件后重新构建 **≤ 15 秒**。超过则本方案的快迭代前提不成立，回到决定 3 重新评估「Vm 是否直接装 aios_core 原生类型」 |
| M0-4 连上本地 SurrealDB，跑通一条真实查询 | 从 AvevaMarineSample（ns 1516 / db 7997）读出一批 refno 与名称并打印 |
| M0-5 接上 `egui_mcp` | agent 能读取运行中应用的控件树并触发一次点击 |
| M0-6 把组件层与设计资产迁入 | `tokens.rs` / `theme_tokens.rs` / `widgets.rs` 迁入并适配 egui 0.35；组件画廊能跑起来逐个看 |

**M0-3 是唯一的量化闸门。** 过不了就不要往下走。

### M1 · 主工作台（本计划的核心交付）

| 任务 | 验收标准 |
|---|---|
| M1-1 外壳：标题栏 38 / 命令栏 40 / 页签栏 32 / 状态栏 26 | 与 S1 画板逐项比对尺寸与配色 |
| M1-2 模型树视图 | 真实项目数据；万行级滚动流畅；长 PDMS 名不撑破行；展开/折叠/选中可用 |
| M1-3 属性视图 | 反射驱动的可编辑字段保持可编辑；键列与值列对齐；空值/异常值有明确显示 |
| M1-4 命令行视图 | 分级、计数芯片筛选、错误行处置入口；文本可选中复制 |
| M1-5 三维视图（占位纹理） | 占位图按视口令牌配色；尺寸与布局与 S1 一致 |
| M1-6 四视图联动 | 树选中 → 属性跟随；命令行输出可定位到元素 |
| M1-7 状态四态 | 四个视图的「没内容可看」共用一套视觉；每个视图只实现自己真会出现的那几态 |

验收统一用 `egui_mcp` + 像素采样，不依赖桌面截图工具。

M1-3 的「可编辑」到**发出 `Cmd::EditAttr` 并更新内存 Vm** 为止，独立应用里不写回 SurrealDB：写回要连会话号、撤销、依赖重算一起做，归 M3-3 命令派发接现有 Event。可编辑的只有标量（整数 / 实数 / 布尔 / 文本）；引用、方位串、数组、未设值一律只读——它们要元素选择器或方位串解析器才能安全地改。

M1-4 的命令行只收**应用自己发的业务消息**，不桥接 `log` / `tracing`：库内部的 span 与调试输出混进来，这一屏就从「操作回执」退化成「日志转储」，筛选也跟着失去意义。级别在发消息的那一刻定死，因此不复用旧壳那套词表（见第六节的更正）。缓冲上限 2000 行，超出就整批丢掉最早的四分之一。错误行给「重试」的前提是 App 还原得出可重做的操作，目前有重连数据源 / 重查子层 / 重查属性三种；`Cmd::RetryLog(id)` 按行号回指，App 侧存着行号到操作的映射，行被限长丢弃时映射一并清掉。

M1-5 的三维视图画三层：`viewport-top` → `viewport-bottom` 的竖向渐变打底，占位图按 cover 裁切铺满（只裁不拉伸，靠 UV 偏移实现，窗口比例怎么变形状都不歪），最上面一枚 HUD 芯片写明「占位纹理 · 未接渲染器」。S1 画板上那些 FPS、已加载数、光标坐标一个都没做——渲染器没接，这些数字只能编，宁可少一格也不摆假读数。纹理的所有权拆两处：`TextureHandle` 挂在 App 上管生命周期，Vm 里只放 `TextureId` 加尺寸，UI 层不碰加载。图片加载失败不影响启动，退化成纯渐变加一行说明。占位图 `assets/textures/viewport-placeholder.jpg` 是临时资产，M3 接回 Bevy 时连同 `textures.rs` 一起删。

**M1-7 的原验收标准写的是「参照 S8」，而 S8 不存在**（见开头对设计源的更正）。分寸改按设计令牌定：前三态说的都是「还没有内容」，共用 `text-muted` 图标加 `text-secondary` 文字，靠图标与措辞区分；只有错误态换 `danger` 并给出「重试」——出了事才值得跳出来。收敛前模型树 / 属性 / 命令行 / 三维视图各写了一份居中提示，图标 24 与 28、顶部间距 40 与 28 与垂直居中，同一个应用里四块面板的「没东西可看」长四个样；现在是组件层一个 `pane_note`。

也不给每个视图都凑齐四态——造一个永远不出现的态就是摆假状态，与外壳「宁可少一格也不摆假数字」是同一条：

| 视图 | 未初始化 | 加载中 | 空 | 错误 |
|---|---|---|---|---|
| 模型树 | 无：启动即发连接请求，`TreeVm` 默认值就是 `Loading` | 有 | 有：当前库下没有 SITE | 有，「重试」= 重连 |
| 属性 | 有：尚未选中元素 | 有 | 无：属性表至少带 TYPE 与 NAME | 有，「重试」= 重查当前选中 |
| 命令行 | 无 | 无：日志是 App 自己写的，不查库也不会失败 | 有：缓冲空与筛选后空两种文案 | 无 |
| 三维视图 | 无：占位图启动时同步载入 | 无 | 无 | 有：纹理载入失败 |

因此 `PropsVm::Empty` 改名 `Uninit`——它说的是「还没轮到它」而不是「查完了没有」。M3 接回 Bevy 后，相机还没给出渲染目标的那一段才会有三维视图真正的未初始化态。

加载中画 egui 的 `Spinner` 而不是静态的转圈字形：不转的转圈图标看着就像界面卡死了。顶距按内容块高度算居中而非固定值——面板从命令行那样的窄条到整根属性栏都有，固定顶距会在矮面板上贴边、在高面板上浮到上三分之一。**已知不足**：块高只按一行正文估算，错误态那种换行到三行的文案会被压到中线以下约两行；单行的态居中是准的。

验收方法留一笔：错误态在这套数据上没有自然的触发路径，是把 `DbOption.toml` 的 `v_port` 临时指到一个没人监听的端口逼出来的，全程不碰任何数据库；三维视图的错误态则是把占位图临时改名。属性的错误态与模型树的空态在本项目的库上够不着（库里实有 3 个 SITE，而属性查询失败要求连着库的同时查询失败），它们与已验证的几态走同一个 `pane_note`，差别只在接线。

### M2 · rs-plant 侧拆除（可与 M1 并行，互不阻塞）

| 任务 | 验收标准 |
|---|---|
| M2-1 删除 12 个不保留的已注册页签及其面板代码 | `ControllableToast` / `DistanceMeasurement` / `MetaDataTab` / `PbsSceneTree` / `PdmsDocs` / `PEVersionHistory` / `Review3D` / `StatusSetting` / `ToastTest` / `VersionTable` / `VersionTimeline` / `VersionView` |
| M2-2 清理 `EditorTab` 未注册枚举值与 dock 装配 | 枚举只剩四个视图 + 必要内部值 |
| M2-3 区分「纯 UI」与「业务逻辑」，只删前者 | 每个被删 plugin 出一行说明：整体删除 / 仅删 UI 保留逻辑。几何生成、拾取、测量、同步等逻辑一律保留 |
| M2-4 删除旧 `editor_ui` 外壳 | `cargo build` 通过 |

M2-3 是风险点：38 个 plugin 里 UI 与逻辑混在一起，**删之前必须逐个分清**，不能按目录整体删。

### M3 · 接回 Bevy

| 任务 | 验收标准 |
|---|---|
| M3-1 `bevy_egui` 升级到 egui 0.35（Bevy 保持 0.16），以可编辑源码树进 `vendor/` | rs-plant 编译通过 |
| M3-2 rs-plant 改为宿主：装配 plant-ui，提供真实视口 `TextureId` | 三维视口显示真实相机内容 |
| M3-3 命令派发接到现有 Event | 树选中、显示模型、定位等动作在真机可用 |

M3-1 是整个计划中最不确定的一项。若代价过高，备选是把 UI 降回 0.33，或将 Bevy 一并升到 0.19（`aios_core` 也绑着 Bevy 0.16 fork，需一并评估）。

### M4 · 屏幕层与任务窗

| 任务 | 验收标准 |
|---|---|
| M4-1 屏幕路由与统一应用状态机 | 8 个散落的 `States` 收敛；两个同名 `AppState` 消除 |
| M4-2 项目选择 S5 | 从零实现（`ui_project_page.rs` 与 `project_plugin` 现已整体注释掉） |
| M4-3 设置 S6（任务窗） | 与 S6 画板一致 |
| M4-4 模型更新 S2/S4（任务窗） | **跨仓库**：对应代码在 gen-model 侧的 `model_update_plan.rs` / `increment_pipeline.rs`，需先定两仓库的数据边界 |

**M4-2 与 M4-3 的画板都还没画**（S5、S6 均不在 `.pen` 里，只有 M4-4 的 S2/S4 是现成的）。M1-7 碰上同样的情况时是按设计令牌自行收敛的，但那只是一个居中提示；整屏的项目选择与设置照这个办法做等于替设计拍板，开工前得先定：是先补画板，还是明确授权按令牌自拟。

---

## 五、风险与未决项

| 风险 | 说明 | 应对 |
|---|---|---|
| 依赖体量 | `aios_core` 带进 bevy_ecs / bevy_reflect / surrealdb，快迭代前提可能不成立 | M0-3 量化门槛，不通过就改架构 |
| egui 桥接 | egui 0.35 + Bevy 0.16 脱离上游支持矩阵，要自行维护 | M3-1 单列；留降级与升 Bevy 两条备选 |
| 组件层迁移 | 现有组件层写在 egui 0.33 上，0.34 的 `Context`→`Ui` 重构会波及 | M0-6 先迁再开工，问题早暴露 |
| 删除误伤 | 38 个 plugin 里 UI 与业务逻辑混在一起 | M2-3 逐个分类后再删，不按目录整体删 |
| 数据边界 | sesno 水位、待更新批次等在 gen-model 侧 | M4-4 之前先定边界 |
| 两处代码分叉 | 独立项目与 rs-plant 并存期间，组件层可能又出现两份 | 组件层只留新项目一份；rs-plant 在 M3 之前不再动它 |

### 未决项

- 屏幕层的登录环节是否保留、放在哪一层
- 任务窗是独立系统窗口还是 egui 内部模态层
- 被删的 12 个面板中，哪些将来要以任务窗形式回来
- S5–S12 是补画还是授权按设计令牌自拟（M4-2 / M4-3 开工前必须有答案，见上）

---

## 六、附：可直接迁移复用的部分

`ui/redesign` 分支上已有、迁进新项目后不必重做的东西：

- 设计令牌与组件层（`tokens.rs`、`theme_tokens.rs`、`widgets.rs`）——需适配 egui 0.35
- 组件：按钮、芯片、状态标签、导航项、页签、工具按钮、开关、分段控件、树行（含纹理图标与行尾标签）、属性行（只读与可编辑两种）、日志行、分组标题
- 控制台的计数芯片筛选交互（**分级词表这一项作废**：旧壳的 `PrintConsoleLine` 只拿得到一个字符串，只能靠词表猜级别，校准过仍会错判；M1-4 改成发消息时定级，猜这一环连同词表和它的单元测试一起去掉了）
- 沙盒 6 屏可交互原型（含主工作台布局），可作为新项目的起点
- 本机验收方法：`PrintWindow` 抓窗口（被遮挡也能抓）、像素采样比对令牌值

**一处返工要如实记下**：本轮把 PBS / SSC / 房间 / 校审 / 元数据五棵树统一切到了组件层，而按决定 4，其中四棵对应的页签在 M2 就要删除。那次改动的范围本可以更小。