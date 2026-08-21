# 布置平台 UI 设计系统 · 落地说明

设计源文件：`design/plant-ui.pen`（Pencil 打开，勿用文本编辑器；原名 `rs-plant3d-ui.pen`，2026-07-27 已 rename 归位，见 `docs/plans/task-queue-rollout.md` 第九节第 3 条）
HTML 参照：`design/export/*.html`（离线可看，含精确尺寸与颜色）
字段对照：`design/MODEL-UPDATE-FIELD-MAP.md`（模型更新八张画板）与 `design/QUEUE-FIELD-MAP.md`（任务队列视图），都是实现的验收基准

---

## 1. 画板索引

这是**规划**，不是 `.pen` 的现状：截至 2026-07-27 晚，文件里有 S1–S4 加后补的 S2-A、S2-B、S2-D、S2-E、S4-B、S4-C，
再加 S11（模型树行内可见性眼睛）与 S12 / S12-B（任务队列），
组件 7 个（C/Tab、C/TreeRow、C/PropRow、C/LogRow、C/ToolBtn、C/Button、C/QueueRow），S5–S10 一张都没画。
「已画」一列就是为此加的——查过 `old\rs-plant3-d\design\rs-plant3d-ui.pen` 那份原始文件，
体积是本项目这份的三倍，顶层节点却完全一样，所以不是迁移时漏拷。
下游要照着某张画板做之前，先看这一列。

**编号注意**：S11 / S12 两个号在 07-27 被新画板占用，原规划的「模糊查询」「保存 / 搜索记录」
顺延为 S13 / S14（仍未画）。2026-08-05 补 S2-F（阻断卡与补充行形态），并按 ADR-0011
把 S2 / S2-A / S2-B 的步骤条改为两步、S2 变化树收成 dbnum → 交付单元两层。
2026-08-07 按 ADR-0017 补 **S2-G / S2-H**（SITE 视图重构）：预览与确认两步的验收基准移过去，
S2 / S2-B 退为历史参照。

| 画板 | 尺寸 | 已画 | 对应代码 |
|---|---|---|---|
| S1 主工作台（深色） | 1600×1000 | 是 | `dock.rs`、`editor_ui/ui_plugin.rs`、`ui/menu_bar.rs` |
| S2 模型更新 · 预览 | 1040×720 | 是 | **已随 ADR-0017 退为历史参照（2026-08-07）**，验收基准移 S2-G；字段结论仍在 `MODEL-UPDATE-FIELD-MAP.md` §2 |
| S2-A 模型更新 · 正在预览 | 1040×720 | 是 | 尚无实现；预览改异步任务后的等待态，见 ADR-0006 |
| S2-B 模型更新 · 确认执行 | 1040×720 | 是 | **已随 ADR-0017 退为历史参照（2026-08-07）**，验收基准移 S2-H |
| S2-D 模型更新 · 不可用与失败态 | 1040×720 | 是 | 错误包封 `{ code, message, detail }` 的五种分型；是形态表不是窗 |
| S2-E 模型更新 · 设计库行形态表 | 1040×720 | 是 | `DbnumPreview` 的 `anomaly` / `blocked` / `initialization_required` 七种行状态；同为形态表。SITE 视图下这些行形态不变，只是归位到三段树（见 §2-G） |
| S2-F 模型更新 · 阻断卡与补充行形态 | 1040×720 | 是 | 四种阻断卡（含 `duplicate` 的 `paths[]` 列出形态）+「够不着」行（`not_in_project`）；S2 右栏阻断卡的形态出处，2026-08-05 补 |
| S2-G 模型更新 · 预览 · SITE 视图 | 1040×720 | 是 | **SITE 平铺**三段树（可执行 / 首次导入 / 本期不执行）+ 勾选框（同库联动）+ SITE 详情行（两个 E3D 时间 · sesno 区间 · 联动名单）；文件名只在首次导入 / 阻断行露面；验收基准 `MODEL-UPDATE-FIELD-MAP.md` §2-G；依赖 gen-model ADR-020 契约字段；2026-08-07 补（ADR-0017，同日验收改判 SITE 平铺） |
| S2-H 模型更新 · 确认执行 · SITE 视图 | 1040×720 | 是 | S2-B 的 SITE 视图版：会执行批次行以同库 SITE 名单为题（dbnum 收进括号）、新增「未勾选 · 本次跳过」行、排除行删除、重扫提示限定勾选范围；验收基准同文档 §3 追记；2026-08-07 补 |
| S3 主工作台（浅色） | 1600×1000 | 是 | 同 S1，仅主题切换 |
| S4 模型更新 · 执行进度 | 1040×720 | 是 | **已随 ADR-0011 退役**，内容迁 S12 队列视图；保留作历史参照，其上 ZONE 列已过时 |
| S4-B 模型更新 · 实时通道断开 | 1040×720 | 是 | **已退役**，断线降级形态归 S12-B |
| S4-C 模型更新 · 部分完成 | 1040×720 | 是 | **已退役**，`partial` 终态归队列行内明细；行内重试按钮也在那里 |
| S5 项目选择 | 1600×1000 | 否 | `ui/ui_project_page.rs`（当前整体注释掉） |
| S6 设置 | 880×740 | 否 | `editor_ui/dashboard/preference.rs` |
| S7 防火封堵材料统计 | 1240×820 | 否 | `ui/plugging_material_statistics/` |
| S8 状态词汇 | 1240×560 | 否 | 所有数据面板通用；M1-7 已改按设计令牌收敛，不等它 |
| S9 创建元件 | 900×680 | 否 | `plugins/pipe_plugin/component_creation_ui.rs` |
| S10 元件库 | 1400×860 | 否 | `UiOption::ComponentWarehouseUI` |
| S11 模型树 · 行内可见性眼睛 | 自适应 | 是 | `workbench/tree.rs` 眼睛列 + `vm.rs` 的 `RowVisibility` 三态，见 ADR-0010 |
| S12 任务队列 · 常态 | 1040×720 | 是 | 尚无实现；常驻 dock 队列视图，见 ADR-0011，验收基准 `QUEUE-FIELD-MAP.md` |
| S12-B 任务队列 · 暂停 / 重建 / 断线降级 | 1040×720 | 是 | 同 S12 的三种降级形态 |
| S12-C 任务队列 · 终态行内明细形态表 | 1040×720 | 是 | 队列行三种终态（succeeded / partial / failed）的展开明细；S4-C 内容的行内化，「立刻重试」与死信文案在此；2026-08-05 补 |
| S13 模糊查询（原编号 S11） | 1240×820 | 否 | `ui/fuzzy_search/` |
| S14 保存 / 搜索记录（原编号 S12） | 1280×560 | 否 | `fuzzy_search/save_ui.rs`、`import_ui.rs` |

### 向导两步：预览 → 确认执行（第三步随 ADR-0011 迁任务队列）

**2026-08-05 起向导只剩两步**（ADR-0011）：确认后入队执行，进度看 S12 队列视图；
S2 / S2-A / S2-B 的步骤条已改为两步，S4 系三张退役保留作参照。下文是三步时代的
逐格核对记录，字段结论仍然有效，流程描述按上面这句折算。

S2 / S2-B / S4 是同一个任务窗的三步，步骤条六张画板上都有；S2-A 是第一步的等待态，
S4-B 是第三步的断线降级形态，S4-C 是第三步的终态。另有 **S2-D 是形态表而不是窗**，
单独收拢五种不可用与失败形态，S2-E 同样是形态表，收拢七种设计库行状态。
2026-07-27 照 gen-model 的 `manual_update.rs`、`dbnum_state.rs` 与 `web_service/mod.rs`
逐格核过一遍，八条要点：

- **执行范围不等于预览列表。** 阻断的库不跑、非 DESI 的库压根不在范围内、需初始化的库会跑、
  上次失败的待重试单元会并入。S2-B 就是为把这四类差异摆清楚才补的——按下去的是一个不可取消的按钮。
- **「阻断」与「排除」不是一回事**，来自契约里的不同位置，界面上不许合成一行：阻断是 DESI 库出了
  文件异常（`FileAnomaly` 五种），排除是非 DESI 压根不进本期。
- **ZONE 只用于归类**，不参与执行；向上找不到交付单元的变化计入 `no_generation` 并告警，
  **不会**退化成一个 ZONE 级的兜底单元（`manual_update.rs:21`、`:492` 各写了一遍）。
- **界面上每个数字都要能指到契约里的字段。** 原稿上的「预计生成耗时 ~ 4 分 20 秒」与单元行的
  「生成中 62%」都没有来源，已拿掉；已用时、落后事件条数、`14 / 24` 计数都算得出来，留着。
  交付单元的分母是 24 = 预览的 23 个新单元 + 1 个待重试单元，S2-B / S4 / S4-B / S4-C 四张必须同一个数。
- **终态有四种，`partial` 才是常态。** `aggregate_manual_status` 按「有成功也有失败」判定，
  给出 `Success` / `Partial` / `Failed` / `UpToDate`。S4-C 按最难的 `partial` 画：数据批次一旦
  `Applied`，水位不因后面的模型生成失败而回退；失败的交付单元单独登记为待重试，带累计 `attempts`。
  结果摘要里**必须逐条列出 `merged_sesnos`** —— 预览扫描之后才并入本批次的会话，是契约写死的要求，
  也是「执行范围 ≠ 预览列表」在终态上的最后一次交代。
- **第一步也得有等待态，预览的超时上限是 600 秒。** S2-A 按 ADR-0006 的异步任务形态画：逐库推进
  （已扫描 / 扫描中 / 不需扫描）、本地已用时，以及扫描期间仍摆着的上一次预览结果（灰掉、标明已过期，
  比空屏强）。非 DESI 不进扫描循环，写「不需扫描」且不占分母。「取消预览」只放弃客户端等待——
  服务端没有 cancel 接口，界面上不许暗示它会停。
- **失败按 `code` 分型，不许把错误链摊给人看。** 服务端的错误包封是 `{ code, message, detail }`
  （`web_service/mod.rs` 的 `ApiError`），三种拒绝各有各的码：`sync_live` 拒绝是
  **422 · precondition**、同项目任务冲突是 **409 · conflict**、超时是 **504 · timeout**，
  其余才落到 **500 · internal**。S2-D 把这三种、加上 `up_to_date` 空态与
  `data_source_ok = false` 的未初始化态，五种一次摆齐，每种都给原因、影响和一个出路按钮。
  **只有 `internal` 那一类才需要把原始 message 摊出来**，还得收进「详情」默认折起。
- **五种 `FileAnomaly` 里只有一种不阻断。** `preview_dbnum` 里写死了
  `matches!(anomaly, Rollback | TypeChanged)`，项目扫描器另把 `Duplicate` / `Missing` 直接置
  `blocked = true` —— **只有 `PathMigrated` 照常执行**，登记路径还能自动跟新。四种阻断的出路
  各不相同（换回文件 / 人工确认身份 / 人工挑一个 / 补文件或注销登记），不许合并成一句「不可更新」。
  还有 `initialization_required` 的库：契约根本不为它解会话区间，`net_*` 全是 0，**但它确实会跑**，
  所以绝不能显示成「无变化」。七种行状态见 S2-E。

---

## 2. 设计令牌

强调色沿用代码里已有的钢蓝（`theme/default.rs` 的 `accent_light` / `accent_dark`），不另起品牌色。
中性色在原 Tailwind neutral 基础上向强调色色相偏移了极小的量，避免纯灰在大面积深色下显脏。

| 令牌 | 深色 | 浅色 | 用途 |
|---|---|---|---|
| `bg-app` | `#14181C` | `#E9ECF0` | 窗口最底层 |
| `bg-chrome` | `#0F1317` | `#FFFFFF` | 标题栏、状态栏、弹窗页眉页脚 |
| `bg-panel` | `#1A2026` | `#FFFFFF` | 面板主体 |
| `bg-header` | `#1F262D` | `#F3F5F8` | 页签栏、表头、分组头、斑马行 |
| `bg-elevated` | `#242D35` | `#FFFFFF` | 悬浮于视口之上的工具条、菜单 |
| `bg-input` | `#10151A` | `#F3F5F8` | 输入框、下拉、进度条槽 |
| `bg-hover` | `#232C34` | `#EDF1F5` | 悬停态、只读芯片 |
| `border` | `#2A333B` | `#DDE3E9` | 常规分隔线与描边 |
| `border-strong` | `#3A454F` | `#C4CDD6` | 弹窗外框、未选中控件描边 |
| `text-primary` | `#E8EDF2` | `#16202A` | 主文本 |
| `text-secondary` | `#A9B7C3` | `#4C5A67` | 次级文本、字段值 |
| `text-muted` | `#8494A1` | `#64707C` | 标签、元信息（已保证 ≥4.5:1） |
| `accent` | `#74A7CC` | `#4C7392` | 主操作、当前选中、状态指示 |
| `accent-strong` | `#9AC4E2` | `#3A5E7B` | 选中行的文字 |
| `accent-ink` | `#0B1116` | `#FFFFFF` | 强调色底上的文字/图标 |
| `accent-bg` | `#1E323F` | `#E1EBF3` | 选中行背景、信息提示底 |
| `danger` / `danger-bg` | `#F0888A` / `#3A2124` | `#C42B2B` / `#FBE6E6` | 错误、阻断、删除 |
| `warn` / `warn-bg` | `#E9B44C` / `#332A18` | `#9A6206` / `#FBF0DA` | 待处理、冲突、未配置 |
| `success` / `success-bg` | `#5CBF82` / `#16301F` | `#1B7A44` / `#E1F2E7` | 成功、已应用、新增 |
| `viewport-top` / `viewport-bottom` | `#232F3A` / `#0E1318` | `#DCE7F2` / `#F4F7FA` | 三维视口渐变底 |

圆角：`3`（芯片、小控件）/ `6`（按钮、输入、卡片）/ `10`（弹窗、悬浮工具条）。**不要出现 16 以上的圆角。**
间距标度：`4 / 8 / 12 / 16`，面板内边距统一 `12–16`。

### tokens.rs 骨架

```rust
use egui::Color32;

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub bg_app: Color32,
    pub bg_chrome: Color32,
    pub bg_panel: Color32,
    pub bg_header: Color32,
    pub bg_elevated: Color32,
    pub bg_input: Color32,
    pub bg_hover: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_strong: Color32,
    pub accent_ink: Color32,
    pub accent_bg: Color32,
    pub danger: Color32,
    pub danger_bg: Color32,
    pub warn: Color32,
    pub warn_bg: Color32,
    pub success: Color32,
    pub success_bg: Color32,
    pub viewport_top: Color32,
    pub viewport_bottom: Color32,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

impl Tokens {
    pub const fn dark() -> Self {
        Self {
            bg_app: rgb(0x14, 0x18, 0x1C),
            bg_chrome: rgb(0x0F, 0x13, 0x17),
            bg_panel: rgb(0x1A, 0x20, 0x26),
            bg_header: rgb(0x1F, 0x26, 0x2D),
            bg_elevated: rgb(0x24, 0x2D, 0x35),
            bg_input: rgb(0x10, 0x15, 0x1A),
            bg_hover: rgb(0x23, 0x2C, 0x34),
            border: rgb(0x2A, 0x33, 0x3B),
            border_strong: rgb(0x3A, 0x45, 0x4F),
            text_primary: rgb(0xE8, 0xED, 0xF2),
            text_secondary: rgb(0xA9, 0xB7, 0xC3),
            text_muted: rgb(0x84, 0x94, 0xA1),
            accent: rgb(0x74, 0xA7, 0xCC),
            accent_strong: rgb(0x9A, 0xC4, 0xE2),
            accent_ink: rgb(0x0B, 0x11, 0x16),
            accent_bg: rgb(0x1E, 0x32, 0x3F),
            danger: rgb(0xF0, 0x88, 0x8A),
            danger_bg: rgb(0x3A, 0x21, 0x24),
            warn: rgb(0xE9, 0xB4, 0x4C),
            warn_bg: rgb(0x33, 0x2A, 0x18),
            success: rgb(0x5C, 0xBF, 0x82),
            success_bg: rgb(0x16, 0x30, 0x1F),
            viewport_top: rgb(0x23, 0x2F, 0x3A),
            viewport_bottom: rgb(0x0E, 0x13, 0x18),
        }
    }

    pub const fn light() -> Self {
        Self {
            bg_app: rgb(0xE9, 0xEC, 0xF0),
            bg_chrome: rgb(0xFF, 0xFF, 0xFF),
            bg_panel: rgb(0xFF, 0xFF, 0xFF),
            bg_header: rgb(0xF3, 0xF5, 0xF8),
            bg_elevated: rgb(0xFF, 0xFF, 0xFF),
            bg_input: rgb(0xF3, 0xF5, 0xF8),
            bg_hover: rgb(0xED, 0xF1, 0xF5),
            border: rgb(0xDD, 0xE3, 0xE9),
            border_strong: rgb(0xC4, 0xCD, 0xD6),
            text_primary: rgb(0x16, 0x20, 0x2A),
            text_secondary: rgb(0x4C, 0x5A, 0x67),
            text_muted: rgb(0x64, 0x70, 0x7C),
            accent: rgb(0x4C, 0x73, 0x92),
            accent_strong: rgb(0x3A, 0x5E, 0x7B),
            accent_ink: rgb(0xFF, 0xFF, 0xFF),
            accent_bg: rgb(0xE1, 0xEB, 0xF3),
            danger: rgb(0xC4, 0x2B, 0x2B),
            danger_bg: rgb(0xFB, 0xE6, 0xE6),
            warn: rgb(0x9A, 0x62, 0x06),
            warn_bg: rgb(0xFB, 0xF0, 0xDA),
            success: rgb(0x1B, 0x7A, 0x44),
            success_bg: rgb(0xE1, 0xF2, 0xE7),
            viewport_top: rgb(0xDC, 0xE7, 0xF2),
            viewport_bottom: rgb(0xF4, 0xF7, 0xFA),
        }
    }

    pub const fn of(dark_mode: bool) -> Self {
        if dark_mode { Self::dark() } else { Self::light() }
    }
}

pub mod radius {
    pub const SM: u8 = 3;
    pub const MD: u8 = 6;
    pub const LG: u8 = 10;
}

pub mod space {
    pub const S1: f32 = 4.0;
    pub const S2: f32 = 8.0;
    pub const S3: f32 = 12.0;
    pub const S4: f32 = 16.0;
}
```

---

## 3. 字体

| 用途 | 设计稿字体 | 应用实际使用 | 说明 |
|---|---|---|---|
| 界面 / 中文 | Noto Sans SC | **AlibabaPuHuiTi 2** | 走 Bevy 资源管线从 `assets/fonts/zh-CN/*.ttf.egui` **运行时加载**，不是编译期嵌入 |
| 数据 / 日志 / 编号 | Fira Mono | **egui 自带的 Hack** | 不需要新增字体。原来没有等宽，是因为中文字体被 `insert(0, ..)` 到了 Monospace 家族首位，把 Hack 挤到了后面 |

**必须走等宽的内容**：REFNO、DBNUM、会话号、坐标、面积/体积、时间戳、文件路径、日志正文前缀。
现在这些用比例字体渲染，数字不对齐，是表格看起来乱的主要原因。

### 三个已经踩过的坑

**一、`ch_fonts()` 只是兜底，不是主路径。** 正常运行时中文字体来自资源目录；`ch_fonts()` 里的 `include_bytes!` 只在资源缺失时才用得上。因此它**只嵌入常规一档**——为一条正常不走的路径多嵌两档字重要付 16 MB 二进制体积。代价是兜底状态下没有真字重层次。

**二、字重只能表达成独立的命名家族，而 egui 对未绑定字体的命名家族是直接 panic。** 界面在 Bevy 资源就绪之前就已经开始绘制，`setup_egui_fonts` 却挂在 `OnEnter(PlantAssetsStates::Loaded)` 上，比首帧晚。所以 `Font::title / page / strong` 内部会先查 `WEIGHTS_READY` 标志，未就绪就退回常规字重。**新增任何用到命名家族的代码，都不要绕过这个入口。**

**三、字重家族必须在图标字体并入之后再注册。** 它是从 Proportional 链复制出来的，复制得早了，凡是走 `Font::title` / `Font::strong` 渲染的 phosphor 图标全部变成豆腐块。

字号只用这 6 档（固定 px，不做流体缩放）：`10` 微标签 · `11` 元信息 / 表格 · `12` 正文 / 字段 · `13` 面板标题 · `14` 弹窗标题 · `18` 页面标题。

---

## 4. 组件映射

| 设计组件 | egui 落点 | 关键规格 |
|---|---|---|
| `C/Tab` | `editor_tab.rs` / dock 页签 | 高 32；激活页签底色 = `bg-panel`、顶部 2px `accent` 描边；未激活 = `bg-header` + `text-secondary` |
| `C/TreeRow` | `e3d_tree.rs`、`pbs_plugin` | 高 26（模型树）/ 30（变化树）；缩进 13–16px/层；选中 = `accent-bg` 底 + `accent-strong` 字 |
| `C/PropRow` | `e3d_plugin/ce.rs` 属性面板 | 高 26；键 88px `text-muted`，值 `fill_container`；偶数行 `bg-header` 斑马 |
| `C/LogRow` | `console_plugin` | 高 24；时间等宽 `text-muted` + 52px 分级标签 + 正文；ERROR 行整行 `danger-bg` |
| `C/ToolBtn` | `viewer/toolbar.rs` | 30×30，圆角 6，激活 = `accent-bg` 底 + `accent` 图标；**必须带 hover 标签 + 快捷键** |
| `C/Button` | `widgets/rs_button.rs` | 高 26/28/30/32 四档；主操作 = `accent` 底 + `accent-ink` 字、无描边；次操作 = `bg-elevated` + 1px `border` |

固定高度：标题栏 `38` · 命令栏 `40` · 页签栏 `32` · 状态栏 `26` · 弹窗页眉 `46–52` · 弹窗页脚 `52–56` · 表头 `32–34` · 表行 `32–36`。

---

## 5. 需要改动的现有文件

1. ~~**`src/style/rs_style.rs` 整体废弃。**~~ **已改为令牌适配层，见 ADR-0003。** 实测发现 `RSStyle` 在 28 个文件、73 处直接画填充色，令牌层单独上会得到「深色文字 + 浅色填充」的不可读混杂态。所以没有逐点删除，而是把 `RSStyle` 的字段与 `set_style` / `set_title_style` / `ui_scroll_slider_style` 等自由函数整体改为读 `theme_tokens::current()`，73 个调用点一行未动就跟上了令牌。**逐文件删除这些调用现在是纯技术债清理，不再阻塞观感。**
2. **`src/editor_ui/theme/mod.rs::apply_theme`** 里一次性完成 `Visuals` 装配：`widgets.{noninteractive,inactive,hovered,active,open}` 五态、`selection`、`extreme_bg_color`、`window_fill`、`panel_fill`、圆角 3/6/10。装配完成后禁止在业务代码里再调 `ui.style_mut()` 做局部覆盖。**已完成**；同时移除了 `ui_plugin::setup_style` 里硬写的 `ctx.set_theme(Theme::Light)`——它比 `setup_egui_fonts` 更早执行，会把令牌装配整个冲掉。
3. **新增底部状态栏**（`TopBottomPanel::bottom`）。**已完成**，见 `src/editor_ui/status_bar.rs`，注册顺序必须排在 `menubar_ui_system` 之后、`show_editor_ui` 之前——`DockArea::show` 走 `CentralPanel` 会把剩余空间吃满。
   目前只显示 rs-plant 确实持有的数据：E3D 数据源就绪状态、当前选中元素 refno、用户与专业。**设计稿上的已应用 sesno / 文件 sesno / 待更新批次 / 待重试单元属于 gen-model 侧，本仓库拿不到**，需要先定义两个仓库之间的数据边界才能补上。宁可少一格，也不摆一个永远是 0 的数字。
4. **`src/plugins/console_plugin`**：日志按 INFO / WARN / ERROR 分级，加严重级计数筛选，停止用整片红字。**已完成**（错误行的处置入口「查看详情 / 重试」尚未做）。
   保留了 `TextEdit` 而没有换成组件层的 `log_row`：`log_row` 是绘制型只读行，换过去会弄丢控制台文本可选中复制的能力。
   **分级词表必须照着本仓库真实会打进控制台的语句挑，不是通用日志词表。** 用通用词表时 `{} 不符合命名规则` 与 `正在显示模型,请勿重复操作` 这两条真警告会全部落进 INFO，而 WARN 一级一条都匹配不上。词表由 `classifies_real_console_corpus` 这组用例锁着，语料直接抄自 `PrintConsoleLine::new` / `scrollback.push` 的实参。
   关键词判定终究是猜。要让分级变准，得让**写日志的一侧带上级别**（`PrintConsoleLine` 换成带 `Level` 的事件），那是另一件事。
5. **`src/editor_ui/viewer/toolbar.rs`**：工具栏底色从 `Color32::from_black_alpha(150)` 改为 `bg-elevated` + 1px `border`（浅色主题下半透明黑会糊成一团），并补 hover 标签。

---

## 6. 建议落地顺序

1. ~~`tokens.rs` + `apply_theme` 统一~~ **已完成**（并连带 RSStyle 适配层，两者必须同一批做，见 ADR-0003）
2. ~~字体三档注册 + 等宽接入~~ **已完成**
3. ~~底部状态栏~~ **已完成**（数据源受限，见上节第 3 条）
4. ~~三维视口底色接令牌~~ **已完成**（`consts::BG_3D_COLOR` 取自 `viewport_top`）
5. ~~控制台分级~~ **已完成**（错误行的处置入口还没做）
6. ~~组件层可编辑 `prop_row` / 纹理图标 `tree_row`~~ **已完成**，属性面板与模型树都已改由组件排版
7. 其余几棵树（PBS / 房间 / 校审）套同一模式 —— 下一步
8. egui_dock 页签自绘（激活页签顶部 2px 强调条）——`egui_dock::Style` 表达不了，必须自绘
9. 模型更新的三步向导（S2 / S2-B / S4，外加 S2-A 的预览等待、S4-B 的断线降级、S4-C 的 `partial` 终态，
   以及 S2-D 那张五形态表）；进度通道见 ADR-0005，预览改异步任务见 ADR-0006
10. 其余面板按 S7 / S11 范式逐个套

### 组件层：源文件只有一份，两个面板的阻碍已经解除

组件层的三个文件（`tokens.rs` / `theme_tokens.rs` / `widgets.rs`）**由主工程 `src/style/` 持有，沙盒 `#[path]` 引用同一份源**，不再各存一份。见 ADR-0004。

曾经挡住属性面板与模型树的两条结构性约束都解决了，做法值得记下来，因为它们不是「把组件做得更全」，而是**改变了组件与调用点的分工**：

| 面板 | 原来的阻碍 | 解法 |
|---|---|---|
| 属性面板 | 行由 `reflect_att_map` 反射驱动，值是**可编辑**的；`prop_row` 是只读展示 | `prop_row_edit` 给的是**容器**：铺底、键名、列宽归组件，值区交回调用点画。值控件是反射出来的任意类型，组件穷举不了 |
| 模型树 | 类型图标是**图片纹理**不是字形，整行又包在 `drag_source` 里 | `RowIcon::Texture` 支持纹理；`tree_row_ui` 把**箭头命中区 `caret_rect` 返回给调用点**，拖拽与右键菜单仍由调用点的 `drag_source` 拥有 |

两条都遵循同一个原则：**组件负责排版与配色，交互归属留在调用点**。整行只有一个 `Response`，组件自己再开一个交互区只会和调用点抢事件。

行尾标签 `RowTag { icon, text, tone }` 的图标与文本必须分开绘制——图标字形只并入了比例字体家族，跟着等宽正文一起排会变成豆腐块。

只读的 `prop_row` 与可编辑的 `prop_row_edit` 共用 `prop_key_w`，分组标题栏的右半区读 `prop_value_x`。三处对齐同一条竖线；标题栏原来按「宽度的 35%」摆，面板一窄一宽就和值列错开。

---

## 7. 校验清单

改完任一面板，对照检查：

- 正文对背景对比度 ≥ 4.5:1，`text-muted` 也不例外
- 同一屏内只有一个 `accent` 底的主操作
- 每个数据面板都实现了加载 / 空 / 错误 / 未初始化四态（参照 S8；模型更新这条流程见 S2-A 与 S2-D）
- 数值列右对齐且等宽
- 每个数字都指得到契约里的字段或本地可算的量，指不到就不摆
  （模型更新那八张画板已经逐条列在 `MODEL-UPDATE-FIELD-MAP.md`，改画板要同步改它）
- **真实界面上不出现契约字段名**（`status = partial`、`anomaly.kind = rollback` 这类）。
  字段出处归 `MODEL-UPDATE-FIELD-MAP.md`；形态表 S2-D / S2-E 是给实现者看的，可以留。
  用户界面只放人要做决定时用得上的信息——**能少一格就少一格**
- 没有出现 16px 以上圆角、没有装饰性侧边色条、没有渐变文字

---

## 8. 怎么跑起来，怎么验收

### 直接运行 exe 必须带 `BEVY_ASSET_ROOT`

```powershell
$env:BEVY_ASSET_ROOT = "D:\work\plant-code\rs-plant3-d-ui-redesign"
& "D:\Rust\target\debug\rs-plant.exe"
```

`CARGO_TARGET_DIR` 指向仓库外时，exe 落在 target 目录里，Bevy 会去 **exe 旁边**找 `assets/`：

```
ERROR bevy_asset::server: Path not found:
  D:\Rust\target\debug\assets\fonts/zh-CN/AlibabaPuHuiTi-2-65-Medium.ttf.egui
```

找不到资源 → `PlantAssetsStates` 走不到 `Loaded` → `setup_egui_fonts` 不执行 → **字体和主题一起不生效**，界面中文全是豆腐块。这个现象很像代码改坏了，实际只是启动方式不对。`cargo run` 不受影响（Cargo 会设 `CARGO_MANIFEST_DIR`）。

### 验收以像素采样为准，不要只看截图

桌面截图工具在本机会返回过期或缩放错误的画面，曾经连续两次报告「主题没生效」，而同一时刻采样得到的却是精确的令牌值。判断颜色是否正确，直接采样并与 `tokens.rs` 里的值比对：

```powershell
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$sz  = New-Object System.Drawing.Size 2560,1440
$bmp = New-Object System.Drawing.Bitmap 2560,1440
([System.Drawing.Graphics]::FromImage($bmp)).CopyFromScreen(0,0,0,0,$sz)
$c = $bmp.GetPixel(200,500)   # 左侧面板
"#{0:X2}{1:X2}{2:X2}" -f $c.R,$c.G,$c.B   # 期望 #242D35 = bg_elevated(dark)
```

需要看图时，**裁剪原分辨率区域**，不要整屏缩放后再看——缩放过的图在本机会失真到看不出深浅。

### 抓窗口用 `PrintWindow`，不要跟别的程序抢 z 序

整屏抓图要求目标窗口在最上面，而本机桌面上常年有别的窗口（PChat 全屏、微信、任务管理器）和另一个自动化在抢焦点与 z 序。`PrintWindow` 直接让窗口把自己画进位图，**被完全遮挡也能抓到**：

```powershell
# 省略 P/Invoke 声明：EnumWindows / GetWindowRect / PrintWindow
$r = ...                       # GetWindowRect(hwnd)
$bmp = New-Object System.Drawing.Bitmap ($r.Right-$r.Left),($r.Bottom-$r.Top)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$hdc = $g.GetHdc(); [W]::PrintWindow($hwnd,$hdc,2); $g.ReleaseHdc($hdc)
```

注意抓出来的位图是**窗口坐标**：最大化窗口的原点在屏幕 `(-11,-11)`，换算屏幕坐标要减掉这个偏移。

### 不要在这台机器上做全局输入注入

为了给控制台造几条日志，我用过 `SendKeys` + 全局点击，代价是：微信中途抢走焦点，**有按键落进了聊天窗口**；应用窗口被误触关闭过一次、最小化过一次。

- `PostMessage` 投递 `WM_CHAR` / `WM_LBUTTONDOWN` 给窗口句柄虽然不影响别的程序，但 egui 拿不到键盘焦点，文字进不去输入框
- 真要用全局输入，**每一次 `SendKeys` 之前都要重新校验前台窗口标题**，不一致立刻中止

更靠谱的路子是**在 `ui-next` 沙盒里验证组件**：它带模拟数据、是一次性工具，在上面点击零风险，而且现在与主工程共用同一份组件源，所见即主工程所得。
