# 模型树右键菜单与三维视图左侧工具栏 · 计划、落地与审查

- 日期：2026-07-27
- 范围：`plant-ui` 绘制层、`plant-ui-app` 独立壳、`old/rs-plant3-d` Bevy 宿主
- 设计依据：`design/DESIGN-SYSTEM.md` 的 `C/ToolBtn`、S1 工作台、旧模型树与旧视口工具栏
- **状态：核心交付已落地并编译通过；真机未验；本文最后一节列着四条已知待修**

本文由两份同题计划合并而来，见第一节。

---

## 一、这份文档为什么是合并来的

2026-07-27 13:00 前后，**两个 agent 在同一棵未提交的工作树上做了同一件事**，各写了一份计划
（13:04 与 13:05），随后同时改代码。碰撞发生过三次：

| 时刻 | 情形 |
|---|---|
| 13:07–13:11 | A 落 `ModelAction` 与 `row_menu`/`toolbar`；B 正读着旧版 `lib.rs` 准备动手 |
| 13:25 | B 在宿主侧补 `Cmd::SetSelection` 分支，发现 A 已在 13:25:55 写好了同一个分支 |
| 13:27–13:30 | B 停手，A 继续 |

**结果是运气好而不是有机制。** 第二次碰撞里 A 那版反而更干净（`SetSelection` 设完选择集后把
`SelectElement(primary)` 推回队列，复用现有的属性查询路径，没把异步闭包抄第二遍），所以留了 A 的。
但两个写手同时修同一个编译错误，这次合上了，下次就是互相覆盖。

留给下一个人的教训只有一条：**动手前先确认没有别的 agent 在这棵树上写。**
`docs/plans/` 里一度并排躺着两份同题计划，谁也说不清该信哪份——现在合成了这一份。

---

## 二、目标与完成定义

不是只把菜单和按钮画出来，而是让两处入口走同一条真实动作链：

1. 模型树任意行可右键打开菜单。
2. 三维视图左侧出现真实可用的竖向工具栏。
3. 显示 / 隐藏 / 定位从树菜单和工具栏发出**相同**的语义命令，宿主只处理一次。
4. 工具栏按钮不会把点击穿透成视口拾取；快捷键不会在命令行、属性编辑框中误触。
5. 独立壳没有实时渲染器时不显示假工具栏。
6. 深浅主题、标准 / 紧凑密度、窄视口下都不遮挡 HUD，也不超出视口。

---

## 三、开工前定下的四条决定，以及它们的实际归宿

| # | 决定 | 归宿 |
|---|---|---|
| 1 | 要渲染器的动作**画出来但置灰** | **被事实推翻**。落地时改成「无渲染器就整条不显示」（`view3d.live == false` 时不画工具栏、树菜单不出模型动作）。理由见下 |
| 2 | 现在就上多选 | **已落地**，`Selection` 有序集合 + Ctrl 加减 / Shift 区间 |
| 3 | 按新架构重列动作清单，一次性任务归任务窗 | **已落地**，见第五节的逐项去向 |
| 4 | 模型树搜索框不带 | **保持**，单列一项 |

决定 1 被推翻的原因值得记：立这条决定时以为「显示 / 隐藏 / 定位全都要等 M3」，而 HEAD 的
`a8b81ba` 已经确认 **M3-1 / M3-2 打通**——`bevy_egui35` 桥接做好、`plant_ui_host.rs` 911 行
把相机渲染目标真接上了。在 Bevy 宿主里这些动作**当场就能真跑**，只有独立壳跑不了。
既然如此，「独立壳里整条不显示」比「画一排灰按钮」更符合「宁可少一格」。

同理作废的还有一套 `ViewportCaps { visibility, camera, measure }` 三位能力位：它的理由是
「M3 分三步接，一个布尔量不够用」，而 M3 已经接完了，`View3dVm::live` 一个布尔量就够。该结构
写出来过，已删除。

---

## 四、落地了什么

### 4.1 命令契约（`crates/plant-ui/src/lib.rs`）

```rust
pub enum ModelAction {
    /// 可见性作用于一整批：「隐藏这几个」是选中之后最常做的一件事。
    SetVisible { refnos: Vec<RefU64>, visible: bool },
    HideAll,
    /// 只作用于主选中，不收整批——理由见 4.4。
    Focus(RefU64),
    FitAll,
    ToggleMeasure,
}

pub enum Cmd {
    // …既有变体…
    SetSelection(vm::Selection),
    Model(ModelAction),
}
```

树菜单与工具栏共用 `ModelAction`，宿主只映射一次。**不建通用 Command Bus、不给每个按钮建一个
`Cmd` 变体、不复制旧的 `TOOLBAR_DRAW_TOOLS` 字符串协议。**

### 4.2 选择集（`crates/plant-ui/src/vm.rs`）

`WorkbenchVm::selected: Option<RefU64>` 换成 `selection: Selection`（有序集合 + 锚点）。
选择集由**绘制层算好结果整体交出**，宿主不复原 Ctrl / Shift 语义——区间要按可见行序算，
那个序只有绘制层手上的 `rows` 才有。

`selected` 的类型只被 9 处读到，一次改完，没留半态。

### 4.3 模型树（`workbench/tree.rs`）

| 交互 | 行为 |
|---|---|
| 左键 | 单选 |
| Ctrl + 左键 | 加减 |
| Shift + 左键 | 从锚到落点按可见行序取闭区间，整体替换 |
| 右键落在选择集**内** | 选择集不动，菜单作用于整批 |
| 右键落在选择集**外** | 先重设为这一行，菜单只作用于这一行 |

菜单项：展开 / 折叠（可展开的行才有）· 显示模型 · 隐藏模型 · 定位模型 · 复制 REFNO。
成批时标题报数（「隐藏模型 3 项」），单个时不报——「1 项」是废话。

两处关键实现约束：

- **行 id 必须 `push_id(row.refno)`**：虚拟滚动下自动 id 是「视野内第几行」，滚一下同一个 id
  就落到另一个元素上，右键菜单会串行。
- **菜单的作用对象在 `row_menu` 里就地算死**，不回头读 `vm.selection`：右键落在集外时重设选中的
  `Cmd` 要下一帧才被宿主处理，读 Vm 会让首帧的菜单作用到上一批元素上。

### 4.4 视口工具栏（`workbench/view3d.rs`）

只在 `View3dVm.live == true` 时绘制，视口左侧垂直居中，距左 12px，容器内边距 4、按钮间距 4，
复用 `widgets::tool_btn`（30×30）。窄视口先收间距，仍不足则放进垂直滚动区，不把按钮画到视口外。

| 顺序 | 工具 | 快捷键 | 启用条件 |
|---|---|---|---|
| 1 | 显示所选 | `S` | 选择集非空 |
| 2 | 隐藏所选 | `H` | 选择集非空 |
| 3 | 隐藏全部 | `Shift+H` | 实时视口 |
| 4 | 定位到当前元素 | `F` | 有主选中 |
| 5 | 视图适应 | `Home` | 实时视口 |

**显示 / 隐藏收整批，定位只取主选中。** 给一组元素取景要合并包围盒，而宿主侧的
`FocusModelEvent`（`editor_ui/viewer/interact.rs:41`）只认单个 refno，那个合并 AABB 的分支还注释着。
塞一组进去的结果是定位到其中某一个、还说不清是哪一个。所以工具栏 tooltip 写的是「定位到当前元素」
而不是「定位所选」，单测 `visibility_takes_the_batch_while_focus_takes_the_primary` 把这两种
作用域的分别钉死。

输入仲裁两条，都不能省：

- **快捷键只在视口背景 `Response` 持有焦点时处理**。点视口或点工具栏按钮后焦点归视口；
  命令行、属性输入框取得焦点后快捷键自然失效。不做全局热键，不靠「鼠标悬没悬停」猜焦点。
- **浮层先画完再判拾取**，工具栏矩形内的点击不产生 `Cmd::PickViewport`。漏了这条就是
  「点显示按钮的同时选中了背后的管子」——旧实现靠 `toolbar.rs:138` 那一行撑着。

`consume_exact` 严格按修饰键匹配：egui 的 `consume_shortcut` 走 `matches_logically`，
多按一个 Shift 也算命中，`H` 会把 `Shift+H` 一并吃掉。

### 4.5 组件层（`style/widgets.rs`）

- `TreeRow` 加 `primary`：主选中多一圈 1px `accent` 描边。不换底色——底色一换，多选时
  「选中的一片」就断成深浅两段，看着像两组而不是一组。
- `tool_btn` 补禁用态：**它原先完全没读 `ui.is_enabled()`**，调用点用 `add_enabled` 表达
  「这枚现在按不了」，画出来却和常态一模一样。禁用态走 `text_muted`，不加底色也不加描边——
  它悬在视口工具栏上，给填充反而比可用的那几枚还重。

### 4.6 宿主映射（`old/rs-plant3-d/src/plant_ui_host.rs`）

| `ModelAction` | Bevy 侧 |
|---|---|
| `SetVisible { refnos, visible }` | 一条 `ShowModelEvent`，`refnos` 整批（它本来就收集合，深度展开一次就够） |
| `HideAll` | `HideAllPdmsMeshEvent` |
| `Focus(refno)` | `FocusModelEvent { refno: Some(..), fit_camera: false }` |
| `FitAll` | `FocusModelEvent { refno: None, fit_camera: true }` |
| `ToggleMeasure` | 空实现，见第六节 |

显示 / 隐藏一律交给已有的 `ShowModelEvent`：它自己深度展开到实例层，也认得 `LevelMgr` 里
那些已显示的后代。**不在宿主里另写一套实体遍历**，那只会和它抢着改同一批 `Visibility`。

隐藏之后不动选择集：留着选中，用户点一下「显示所选」就回来了。

---

## 五、旧菜单 10 项的逐项去向

| 旧项 | 去向 |
|---|---|
| 显示模型 / 隐藏模型 / 定位模型 | **留**，常驻动作 |
| 隐藏全部模型 | **移到工具栏**：它不作用于右键点中的那个元素，摆在元素菜单里名不副实 |
| 显示所在房间 / 显示房间内模型 / 隐藏房间内模型 | **缓**，依赖 FRMW 空间包含索引，数据层没有 |
| 显示 SSC 模型 | **缓**，`ShowSscModelEvent` 的 `add_event` 还在 M2 待删的 `pbs_plugin` 里 |
| 变更记录 | **移任务窗（M4）**，有头有尾；`version_plugin` 在 M2 删 |
| 导出 obj | **移任务窗（M4）**，有头有尾、要选路径与进度 |

回迁原则：**先在 Bevy 宿主恢复真实处理链，真机验证通过后再扩展 `ModelAction` 和菜单项。
不接线就不显示，禁止先做灰色占位项。** `FRMW` 专属动作按 noun 条件显示。

明确跳过：旧代码里已注释的「显示同类型」、已注释的 STP / ANSYS / RVM 导出、
拖拽树节点后自动显示模型的隐式副作用。

---

## 六、代码审查结论（2026-07-27 13:31 的树）

按严重程度排，**只有第一条是非改不可的**。四条都还没动手。

### 6.1 主选中跟错了元素（真 bug）

`Selection` 把**锚点**与**主选中**当成了同一个字段：

```rust
pub fn primary(&self) -> Option<RefU64> {
    self.anchor.or_else(|| self.items.last().copied())
}
```

复现：行表 `[1,2,3,4,5]`，点行 4（anchor=4），Shift+点行 2 → `items=[2,3,4]`，而 `anchor`
按设计不动仍是 4。于是 `primary()` 返回 **4**——用户刚点的是行 2，属性面板和状态栏却在说行 4。

锚点不动是对的（连着 Shift 点几下都该以最初那一下为准），错在拿它兼任主选中。标准做法是两个
字段：`anchor` 给 Shift 当支点，`cursor`（= 主选中）跟最后一次点击走。`primary()` 的文档注释
写的「= 最后一次点中的元素」才是正确意图，代码没做到。

单测没拦住它：`shift_range_follows_visible_row_order_in_both_directions` 只断言了 `items`，
一次都没断言 `primary()`。修的时候要连这条断言一起补。

### 6.2 「隐藏全部」没有配对的「显示全部」

工具栏有 `HideAll`，没有 `ShowAll`。按下去之后只能靠「选中 + 显示所选」一批一批捞回来。
旧 UI 就是这个窟窿，原封不动继承了。不是代码错，是功能缺一块。

补的话要新增 `ModelAction::ShowAll` 与宿主映射，并先确认 `HideAllPdmsMeshEvent` 有没有对偶的
恢复事件——没有就得写一个。

### 6.3 宿主每次 `SetSelection` 都重查属性

`SetSelection` 把 `SelectElement(primary)` 推回队列，而 `SelectElement` 那支无条件
`props = Loading` 加一次异步查询。Ctrl 点掉一个非主选中的元素时主选中根本没变，却白打一次库。
独立壳的 `set_selection` 有 `same_primary` 守卫，宿主没有——两边行为不一致。

### 6.4 `row_menu` 每帧重建整批

菜单开着的每一帧都跑一遍 `selection.to_vec()` 一次 + `targets.clone()` 两次 + 两个 `format!`。
选了两千行就是每帧三个两千元素的 Vec。不致命，但把 `ModelAction` 挪进 `clicked()` 分支里构造
就没了。

### 6.5 两个没人拍过板的设计问题

- **折叠之后不可见的元素仍留在选择集里。** 状态栏报「+3」而屏上只高亮一行，
  「隐藏所选 4 项」会动到看不见的行。保留与随折叠剔除两种做法都有人用，得选一个。
- **`ModelAction::ToggleMeasure` 是死变体**：没有按钮发得出它（`Tool::BAR` 里没有尺子），
  宿主那支是空 `{}`。注释说得很清楚——要精确表面命中才接——但枚举里摆着一个没人能产生的值。

### 6.6 复制 REFNO 的分隔符

复制出的是 `24381_100060`（`RefU64` 的 `Display`），而 PDMS 与设计稿都写 `24381/100060`。
能贴回本应用的 `=<参考号>`（`FromStr` 认 `_` 也认 `/`，还会剃掉前导 `=`），贴不进 PDMS。

### 6.7 这几处是对的，别改

- `Cmd::Model(ModelAction)` 嵌一层——拦住了 `Cmd` 变成流水账
- 选择集由绘制层算好整体交出——区间要按可见行序，那个序只有绘制层有
- 快捷键靠视口 `Response` 的焦点开关，不猜鼠标悬停
- `push_id(row.refno)`——虚拟滚动下不这么写右键菜单会串行
- 工具栏矩形排除拾取
- `SetVisible` 整批走一条 `ShowModelEvent`
- `Focus` 只取主选中
- **菜单挂在行的 `Response` 上是可以的**：egui 0.35 在 `memory/mod.rs:795-802` 会清理
  「本帧没重新画出来」的 popup，所以行被虚拟化剔除时菜单干净地关掉且不会自己再冒出来，
  得到的正是「滚动即关闭」。曾经担心这里要把菜单挂到滚动区外面，查过之后确认不必。

---

## 七、验收现状

**已做（自动检查）：**

```powershell
# plant-ui：通过，增量 3.7s（M0-3 那道 ≤15s 的闸门仍成立）
cargo check --workspace --all-targets
cargo test -p plant-ui          # 12 passed

# rs-plant3-d：通过
cargo check
```

**运行时证据（2026-07-27 14:0x，`inspect` 探针注入 + 截图像素采样，浅色主题 / 标准密度）：**

| 验收项 | 结果 | 采样值 |
|---|---|---|
| `tool_btn` 禁用态与常态有分别 | **通过** | 30×30 区内最暗像素亮度：常态 129.2（`#79838D`）/ 激活 126.6（`#6386A1`）/ **禁用 194.1**（`#BEC3C8`）；激活底色 `#E1EBF3` = `accent_bg` 浅色值 |
| 主选中那圈 1px 描边看得出来 | **通过** | 顶边：从选中 `#E1EBF3`（无描边）/ 主选中 `#7997AE`；两行内部同为 `#E1EBF3` |
| 右键菜单开得出、项目对 | **通过** | 控件树两个 Button：`展开`、`复制 REFNO`；占位模式下模型动作正确地不出现 |
| 右键落在选择集外重设选中 | **通过** | 状态栏由行 1 跳到 `8000_200` |
| 占位纹理模式不显示工具栏 | **通过** | 全控件树无竖向 30×30 排（只有命令栏那三枚横的） |
| 复制 REFNO 真进系统剪贴板 | **通过** | 剪贴板得到 `8000_200`（下划线形式，即 6.6 说的那件事） |
| **Ctrl / Shift 多选** | **验不了** | 见下 |
| 工具栏本体 / 穿透拦截 / `s h f Home` 快捷键 | 未验 | 只在 `live == true` 时存在，要跑 Bevy 宿主 |
| 深色主题 × 紧凑密度 × 窄视口 | 未验 | |

证据图存 `output/verify-gallery-crop.png`、`output/verify-menu2-crop.png`。

**为什么 Ctrl / Shift 多选用探针验不了**（省得下一个人再试一遍）：
`tree.rs` 读的是 `ui.input(|i| i.modifiers)`，也就是帧级的 `RawInput::modifiers`；而
`egui_inspection` 的 `ApplyEvents` 只做一件事（`plugin.rs:212`）：

```rust
input.events.extend(events.iter().cloned());
```

**它从不碰 `input.modifiers`。** 所以事件里带什么修饰键都没用，那段代码看到的永远是「没按键」。
实测：连着 Ctrl 点三行，主选中依次变成 `8000_100 / 8000_200 / 8000_300`，`+N` 一次都没出现——
每一下都是替换而不是加减。`egui_inspection` 在 `vendor/registry/` 下带校验和，改不得。

这不表示多选坏了：真实会话里 winit 会把 `RawInput::modifiers` 填对，产品那么写也是 egui 的
地道写法。三条出路：人手验一次；把 `tree.rs` 改成从点击事件里读修饰键（可自动回归，但不如
现在地道）；或者就记成未验。

顺带：`inspect click` 已支持 `ctrl+` / `shift+` 前缀。它驱动不了上面这条路径，但**能**驱动
`consume_exact` 那一层（视口快捷键读的就是事件自带的修饰键）。新加的 `CTRL` 常量同时置
`command: true`——Windows 上 egui-winit 就是这么发的，而 `Modifiers::CTRL` 只置 `ctrl`。

抓窗口用 `PrintWindow` 而不是整屏截图，**不要在这台机器上做全局输入注入**——理由见
`DESIGN-SYSTEM.md` 第 8 节，那里记着按键落进微信、应用被误触关闭的实际代价。

---

## 八、后续

### 距离测量（带技术闸门）

先做 2 小时技术验证：把视口 UV 转成离屏相机世界射线，确认现有 Bevy mesh picking 能否对离屏
`RenderTarget` 返回真实 `hit.position`。可用就复用 `DistanceMeasurementState` 的两点状态机。

**停止条件**（命中任一就不交付）：只能拿到世界 AABB 交点而非模型表面点；为测量必须新增大型
几何依赖或复制整套 picking 后端；离屏坐标与绘制标签坐标无法稳定对齐。

拿包围盒交点顶上，量出来的数字是假的。

### 模型树搜索框

`design/search_tree.md` 里的需求，单列。它改的是「哪些行可见」，会同时动 App 侧展平逻辑、
`show_rows` 虚拟化、`reveal_offset` 定位偏移三处，和右键菜单不是一类改动。

### 建议的提交边界

1. `plant-ui`: 模型树右键动作与多选
2. `plant-ui`: 视口工具栏
3. `rs-plant3-d`: 宿主侧动作派发

每个提交都要能独立编译。**按路径暂存**——两个仓库的工作树里都有别人未完成的改动，
`m2-plugin-classification.md` 第五节记着上一次是怎么避开这件事的。
