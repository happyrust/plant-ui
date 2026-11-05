# 三维选中高亮可辨性修复 plant-ui 开发计划

日期：2026-10-29；状态：**阶段一 / 三 / 四已完成，阶段二判定为「不做」**（用户 2026-10-29「对比 e3d 里高亮选中的效果，目前这个 plant-ui 里这里高亮的效果并不是特别好，帮我分析提出修复方案」→「使用 plannatator 制定开发计划」→「开始执行」→「继续」；按 D1 (a) / D2 (a) / D3 (a) / D5 (a) / D6 (a) 实施，见文末「进度」）。

## 当前基线

仓库：`D:/work/plant-code/old/plant-ui`（唯一落点，只改这一个仓）。来源侧 `D:/work/plant-code/old/plant3d-web` 只作口径参照，**一字不改**。

### 今天选中是怎么画出来的

| 环节 | 位置 | 现状 |
|---|---|---|
| 四态裁决 | `crates/plant-ui-view3d/src/lib.rs:1703-1733` | `MaterialState::{Base, Highlight, Ce, XRay}`；优先级 **X-Ray > CE > Highlight > Base**；`refno` 或 `owner` 任一命中即生效 |
| 状态下发 | `crates/plant-ui-view3d/src/lib.rs:479-487` | 宿主 `Selection` 整集下发 + `primary`；`selection_dirty` 驱动全场重挑材质 |
| 材质挑选 | `crates/plant-ui-view3d/src/look_materials.rs:110-147` | PBR 外观：CE / Highlight 都落 `SELECT_COLOR`（橙红，alpha 255）；E3D 外观：走 `e3d_colour` |
| E3D 配色 | `crates/plant-ui-view3d/src/look_materials.rs:217-231` | 常态 `#bdbdbd`、CE `#ffff00`、Highlight `#ffffff`、X-Ray 半透明蓝；**CE / Highlight 沿用元素自己的 alpha `a`** |
| 材质构造 | `crates/plant-ui-view3d/src/look_materials.rs:177-211` | 只有 `XRay` 走 `material.flat()`（颜色直出不打光），其余状态都走完整前向着色 |
| 前向着色 | `crates/plant-ui-view3d/src/e3d_look/sgl_look.wgsl:99-141` | `rgb = colour·(Ka + Kd·N·L) + Ks·spec + Kr·env(reflect(−V,N))` |
| 出厂光照 | `crates/plant-ui-view3d/src/e3d_look/presets.rs:48-55` | `Ka 0.7 / Kd 0 / Ks 0 / Kr 0.8 / Kse 0` —— 无漫反射无高光，体积感全来自环境立方体反射 |
| 后处理 | `crates/plant-ui-view3d/src/e3d_look/post/sgl_look_post.wgsl:490-529` | 合成 `bytes = col · hlr · ao`，字节域输出不透明 |
| 参与标记 | `crates/plant-ui-view3d/src/e3d_look/material.rs:34-38`、`sgl_look.wgsl:73-88` | prepass 片元写 `Rgb10a2Unorm`，2 bit 表示「边线 / 伪阴影」；半透明网格不进 prepass |

### 与来源侧（E3D 口径）的已知偏离

| 项 | E3D 3.1 真机 | plant-ui 今天 |
|---|---|---|
| CE 屏上字节 | `(255,255,79)` | 同样是 `(255,255,79)`（`docs/evidence/2026-10-22-e3d-look.md:110`） |
| highlight 屏上字节 | `(249,249,249)` | 同样 `(249,249,249)` |
| 未选中 | `(189,189,189)` | 同样 `(189,189,189)` |
| 半透明件被选中 | 真机 `EnhancedEdgesTranslucent = ON`，仍描边 | **不进 prepass → 无边线、无伪阴影**，且黄色被元素 alpha 稀释（计划书自己记了这个缺口：`docs/plans/2026-10-22-e3d-look-port-plannotator.md:221`） |
| 悬停预选 | 有（active orange） | **没有**，`MaterialState` 只有四态 |

### 已经定位到的四处缺陷

1. **选中色被前向着色洗淡（主因）**。出厂预设 `Kr 0.8`，CE 黄的蓝通道被环境反射抬到约 0.4 → 屏上 `(255,255,79)`；highlight 白被 `0.7 + 0.8·env` 顶到 `(249,249,249)`；而未选中是 `(189,189,189)`。**白选中与灰常态只差 60 个灰阶**，黄选中靠色相撑着。
2. **highlight 白撞背景渐变下端**。`presets.rs:100-118`：出厂预设背景 `grey → white`，顶部 159、底部 242。视口下半部的背景就是 242，选中白 249 —— 差 7 个灰阶，选中件在下半屏几乎消失。
3. **半透明件选中后等于没选**（明确的实现缺口）。`look_materials.rs:224-225` 的 `Ce / Highlight` 分支返回元素自己的 `a`，`material.rs:149-155` 于是返回 `AlphaMode::Blend`，Bevy 的半透明网格不进 prepass → 无边线、无伪阴影。对照 PBR 分支（`look_materials.rs:126-133`）用的是 `SELECT_COLOR`（alpha 255）—— 两种外观行为不一致，说明是漏改而非设计。
4. **X-Ray 吃掉选中**。`lib.rs:1717-1733` 的优先级里 X-Ray 最高；房间视图一开 X-Ray，点中元素只有淡蓝 `(89,169,255,64)`，CE 黄永远不出现。现有测试 `lib.rs:4382` 明确断言了这个行为。

### 需要用户确认的一件事

截图内容本会话读不到（模型不支持图像输入），所以上面四条是按代码推出来的「可能的四类现象」。计划按**四条全修**组织，实施时若某条与实际现象不符可整段跳过，不影响其余阶段。

## 决策摘要（推荐项在前）

| 编号 | 决策 | 推荐与理由 |
|---|---|---|
| D1 选中态是否直出 | (a) **CE / Highlight 一律走 `flat()`（颜色直出、不打光不反射）**；(b) 保留前向着色，只把 `Kr` 在选中态降到 0；(c) 不改，接受 `(255,255,79)` / `(249,249,249)` | **(a)**：一处分支就能让屏上字节等于配色表字节，且 `flat` 已经在 `XRay` 上用着（`look_materials.rs:202-207`），不新增机制。(b) 要改 `SglLookMaterial` 的 uniform 语义（同一枚材质的光照常量按状态分裂），键与缓存都要重算，收益一样。**代价**：与真机逐像素值不再一致，见 D2 |
| D2 是否保留「与真机像素一致」 | (a) **改基线**：把 evidence 里的期望值从 `(255,255,79)/(249,249,249)` 改成 `(255,255,0)/(255,255,255)`，并在证据里写明这是「可见性优先」的有意偏离；(b) 加设置项 `选中色直出`（默认开），关掉即回真机值；(c) 不加设置，也不改基线（等于放弃像素验收） | **(a)**：CONTEXT.md 把「CE 高亮 / 其它选中」定义为界面语义（「主选中染 CE 黄、其它染 highlight 白」），没承诺逐像素复刻；加设置项（b）会让「设置窗多一格、两套观感要各测一遍」，而这一格几乎没人会关。若后续真要做真机对照，再加开关不迟 |
| D3 半透明件选中 | (a) **选中态 alpha 归一到 255**（选中件变不透明）；(b) 保留元素 alpha，另开一条半透明 prepass 让它也描边；(c) 保留元素 alpha，接受无边线 | **(a)**：一行改动同时解决「黄色被稀释」与「无边线」，且这正是 E3D `EnhancedEdgesTranslucent = ON` 想要的观感。(b) 是 `docs/plans/2026-10-22-e3d-look-port-plannotator.md:221` 里明确记着「另议」的活，动的是 prepass 管线与纹理格式，不该塞进这次 |
| D4 highlight 白在亮背景沉底 | (a) **先只做 D1+D3，用像素探针量对比度，不够再上**；(b) 换暖白 `#fff2a8`；(c) 给选中件描深色轮廓（把「是否选中」编进 prepass 的第 3 个 bit） | **(a)**：D1 之后选中件有黑色 HLR 边线与 AO 压暗，`bytes = col·hlr·ao` 会在轮廓处落到近黑——**「白块 + 黑轮廓」在 242 背景上是有边界的**，多半够用。(b) 破「highlight = white」的语义。(c) 要改 `Rgb10a2Unorm` 的编码（2 bit 已满）、两个 wgsl 与合成段的边线颜色，是本计划里最重的一档，留到阶段三按需再做 |
| D5 X-Ray 与选中的关系 | (a) **保持 X-Ray 优先不变**，另加第五态 `XraySelected`：颜色 CE 黄、alpha 提到 160、仍 `flat + Blend`；(b) 改优先级为 `Ce > Highlight > XRay`；(c) 不动 | **(a)**：X-Ray 是用户在房间面板主动打开的「看穿」模式，取消它等于取消该模式；叠一层更实的黄既保留穿透又能看见点中了谁。(b) 会翻 `lib.rs:4382` 那条测试，且房间视图里就再也看不到房间体内部——那是 X-Ray 存在的理由 |
| D6 悬停态 | (a) **单立阶段四**：`MaterialState::Hover` + 宿主下发 hover + 颜色 `#ff7e00`（PDMS active orange）；(b) 不做 | **(a)**：需要新 `Cmd`、拾取路径改造、`View3d` 新字段与脏标记，是独立一块能力，不该和「修好现有高亮」混在一个 PR 里 |
| D7 落点与两端 | 只改 `plant-ui`；PC 先跑通并验收，材质与 wgsl 两端共用，Web 自动生效、只做浏览器回归 | 与 E3D 外观移植同一条纪律（`docs/adr/0032-...md`） |
| D8 非目标 | 不动 `SglLookMaterial` 公式与 `presets.rs` 光照常量、不动 HLR / HBAO 参数、不动 `plant3d-web`、不做 autocolour 规则、不做「选中件描边」除非 D4 判定需要、不加设置项 | — |

## 实施阶段

### 阶段一（P0）：选中态直出 + alpha 归一

**目标**：屏上 CE 黄 = `(255,255,0)`、highlight 白 = `(255,255,255)`，选中件一律不透明并重新进入 prepass（于是有黑边线与伪阴影）。

1. `crates/plant-ui-view3d/src/look_materials.rs:217-231`（`e3d_colour`）：`Ce` / `Highlight` 分支的 alpha 从 `a` 改成 `255`。注释同步改写，说明「选中是界面语义，不继承元素的半透明」。
2. `crates/plant-ui-view3d/src/look_materials.rs:192-211`（`sgl`）：把只对 `XRay` 生效的 `flat` 扩成
   ```rust
   let flat = matches!(
       state,
       MaterialState::XRay | MaterialState::Ce | MaterialState::Highlight
   );
   let material = if flat { material.flat() } else { material };
   ```
   `MaterialKey` 不动（`state` 已在键里，`colour` 的 `a` 变了自然换键）。
3. 不碰 `sgl_look.wgsl`：`flags.z == 1` 的直出分支已经存在（`sgl_look.wgsl:135-139`），prepass 片元只读 `flags.x`（参与标记），所以选中件**照样参与边线与伪阴影**——这正是 D4 (a) 成立的前提。
4. 不碰 `material.rs`：`alpha_mode()` 读 `shading.y`，第 1 步之后自然返回 `Opaque`。

**验收**
- `cargo test -p plant-ui-view3d` 全绿（含改过的断言，见下）。
- 单测改动清单：
  - `look_materials.rs:386-393`：「半透明元素」那条的 CE 期望从 `200/255` 改成 `1.0`，并新增一条常态仍为 `200/255`（证明只有选中态归一）。
  - `look_materials.rs:368` 的用例末尾补：CE / Highlight 的 `params.flags.z == 1`（与 X-Ray 那三条 `lib.rs:402` 同形）。
  - `lib.rs:4281` 用例：颜色期望不变（`[255,255,0]` / `[255,255,255]` 本来就是色值），但补一条「选中件组件是 `SglLookMaterial` 且 `opacity == 1.0`」。
  - `lib.rs:3000` 的 `material_state` 用例不动（裁决逻辑没改）。
- 像素探针（PC，`--look-demo`，PrintWindow 抓窗，口径同 `docs/evidence/2026-10-22-e3d-look.md` §2）：同视角取四点 —— 常态 `(189,189,189)`、CE `(255,255,0)`、highlight `(255,255,255)`、背景底部 `(242,242,242)`，容差 ±3。
- 半透明件实测：造一个 alpha 200 的构件，选中前后各抓一张，确认选中后不透明且轮廓有黑边。

### 阶段二（P1）：highlight 在亮背景的可辨性（**已判定不做**，见「第三轮」）

**触发条件**：阶段一的探针显示「highlight 白 `(255,255,255)` 落在背景 `(242,242,242)` 上时，除轮廓外无区分」，且用户判定仍不够。

**判定结论（2026-10-29 第三轮）**：两个候选方案都否决。背景两端的字节是代码常量，对比度按 WCAG 公式直接算得出（highlight 白 / 背景底 = 1.12 : 1，CE 黄 / 背景底 = 1.04 : 1），不需要像素探针。拉回 grey 会同时把常态件的对比度从 1.68 压到 1.10，方向是错的；加粗选中轮廓要扩 prepass 标记位、重跑整库远景 A/B，收益不抵风险。真机同样是 1.12 : 1，可见性靠 HLR 黑轮廓——我们的轮廓本来就在。详见文末「第三轮 · 二」。

原方案留档（不执行）：
1. ~~首选：把出厂预设的背景渐变下端从「白」拉回「grey」~~ —— 会连常态件一起变糊，否决。
2. ~~次选：给选中件描深色轮廓~~ —— 扩标记位 + 重跑远景 A/B，改为独立议题，不排期。

### 阶段三（P2）：X-Ray 与选中叠加

1. `lib.rs:1703-1713` 加 `MaterialState::XraySelected`。
2. `lib.rs:1717-1733` 的 `material_state`：X-Ray 命中且同时在选中集里 → `XraySelected`，否则维持今天的优先级。
3. `look_materials.rs:217-231`：`XraySelected => (E3D_CE_COLOUR, 160)`；`sgl()` 的 `flat` 判定把它一起包进去。
4. 单测：`lib.rs:3000` 补一条「X-Ray ∩ 选中 = XraySelected」；`lib.rs:4382` 那条 PBR 用例不动（PBR 外观仍走 `XRayMaterial`，D5 (a) 不改 PBR 行为）。
5. 探针：房间视图开 X-Ray 后点中一个构件，取色应读 `(255,255,0)` 叠在 `(89,169,255,64)` 上的混合值，且比周围 X-Ray 件明显更实。

### 阶段四（P3，单立）：悬停预选

1. `View3d` 加 `hovered: Option<RefU64>` 与 `hover_dirty`；`set_hovered(refno)` 入口（`lib.rs:479` 旁边）。
2. 宿主：拾取路径（`plant-ui-app/src/main.rs:663` 的 `take_picked` 附近）在每次指针移动时按当前 uv 求命中，节流后下发。若这条路径开销不可接受，退到「只在已有 `picked` 变化时更新」，并在计划外单立优化。
3. `MaterialState::Hover`，颜色 `#ff7e00`（`colour_table.rs:473` 的 `active`），`flat`，alpha 255。
4. 优先级：`Ce > XraySelected > Hover > Highlight > XRay > Base`（Hover 只落在未选中件上，与 plant3d-web `SelectionManager` 的「选中件不被悬停色覆盖」一致，见 `SelectionManager.ts:239`）。

## 风险

- **与真机像素基线脱钩**（D2 a）。缓解：evidence 里逐条写明「可见性优先」的偏离与理由，并保留一张改前 / 改后并排图，任何人要复刻真机值都能从这两张图反推。
- **选中件变成纯色块**的观感风险。缓解：`flat` 只关掉颜色项，`bytes = col·hlr·ao` 的边线与 AO 仍在，接触面照旧发暗；阶段一的探针要专门取「选中件的棱」与「选中件与地面的接触处」两点，确认边线与 AO 都还在。
- **半透明件选中后变不透明**会让「这层是半透明」的信息暂时消失。缓解：这是选中态的临时表现，取消选中即恢复；与 E3D `EnhancedEdgesTranslucent = ON` 的口径一致。
- **阶段二的编码改动会牵动整库远景画面**。缓解：只在阶段一判定不足时才做，且必须重跑 `docs/evidence/2026-10-22-e3d-look.md:268` 那组 A/B（`Rgba32Float` ND 的远景条纹 / 同心环检验件）。
- **X-Ray 叠加态**（阶段三）在 `Rgb10a2Unorm` 之外没有新纹理开销，但会让 `material_state` 从四态变五态，`virtual_hole.rs:929-939` 与 `room_clip.rs` 的材质裁决都要跟着看一遍（两处都走同一个 `material_state`，只需确认没有独立分支）。**已核（第五轮）**：`room_clip` 没有独立分支；`virtual_hole` 走同一套裁决，孔洞 X-Ray 叠在最上。

## 待用户确认

1. **现象确认**：你贴的那张截图本会话读不到。四条缺陷里哪几条与你看到的一致？（对号表见下）
2. **D2 取舍**：接受「与真机逐像素值脱钩，换取选中可见」吗？还是要加一格设置项保留两套？
3. **范围**：本计划只做阶段一，还是阶段一 + 阶段三一起？

## 现象对号表

| 看到的现象 | 对应缺陷 | 对应阶段 |
|---|---|---|
| 选中的黄发白 / 发灰 / 没有分量 | 1（前向着色洗淡） | 阶段一 |
| 加选之后「其它选中」几乎看不见 | 1 + 2 | 阶段一，不足再阶段二 |
| 选中件在下半屏消失、上半屏还看得见 | 2（撞背景 242） | 阶段二 |
| 半透明件选中像没选 | 3（alpha 未归一） | 阶段一 |
| 选中件没有黑边线、其它件都有 | 3（落进 Blend 不进 prepass） | 阶段一 |
| 房间视图里点了不变黄 | 4（X-Ray 优先） | 阶段三 |
| 鼠标掠过没有任何反馈 | 5（无 Hover 态） | 阶段四 |

## 验证命令

```powershell
cd D:\work\plant-code\old\plant-ui
cargo test -p plant-ui-view3d
cargo test -p plant-ui
cargo clippy -p plant-ui-view3d --all-targets
```

像素探针（PC，需先起本机模型服务或库供数）：

```powershell
cargo run -p plant-ui-app --release -- --look-demo
# 另开一个终端抓窗采样（口径见 docs/evidence/2026-10-22-e3d-look.md §2）
```

## 进度

### 2026-10-29：阶段一 + 阶段三已落地

用户「开始执行」。实施按 D1 (a) / D3 (a) / D5 (a) 三项推荐走；D2 (a) 一并执行（改基线，不加设置项）。

**改动清单**

| 文件 | 改动 |
|---|---|
| `crates/plant-ui-view3d/src/look_materials.rs` | `e3d_colour`：`Ce` / `Highlight` 的 alpha 从元素自己的 `a` 改成 `255`；新增 `XraySelected => (E3D_CE_COLOUR, XRAY_SELECTED_ALPHA)`（160）；新常量 `XRAY_SELECTED_ALPHA`。`sgl()`：`flat` 从「只有 `XRay`」扩到 `XRay / XraySelected / Ce / Highlight`。`standard()`：`XraySelected` 与 `XRay` 同落那枚共享半透明蓝（PBR 外观画面不变）。`resolve()` 的 PBR 分支同步补 `XraySelected` 臂 |
| `crates/plant-ui-view3d/src/lib.rs` | `MaterialState` 加第五态 `XraySelected`；`material_state` 改为「X-Ray 命中时再看是不是选中」，优先级链 `X-Ray > CE > Highlight > Base` 不变 |
| `crates/plant-ui-view3d/src/virtual_hole.rs` | 孔洞 X-Ray 与选中同时命中时也走 `XraySelected`（同口径，不让孔洞预览吃掉选中） |
| `CONTEXT.md` | 「CE 高亮 / 其它选中」补「颜色直出 + 一律不透明」；新增词条「X-Ray 叠选中」 |
| `docs/adr/0032-...md` | D3 补一句指向修订；「已知偏差」加一条（与真机像素值脱钩）；新增「修订（2026-10-29）」三条 |
| `CHANGELOG.md` | 「三维视口」节加两条界面变化（选中一眼能看见 / X-Ray 里点中什么看得出来），原「选中配色跟 E3D」那条保留 |

**没做的（与计划的偏差）**

- **阶段二（P1）没有触发**：需要像素探针量「highlight 白 `(255,255,255)` 落在背景 `(242,242,242)` 上」的实际对比度才能判定。本机没有起模型服务 / 库供数，探针没跑，所以阶段二维持「条件触发」，`presets.rs` 的背景渐变与 `Rgb10a2Unorm` 的编码都没动。
- **阶段四（P3 悬停）按计划单立**，没有动。`View3d` 里没有 `hovered` 字段，宿主拾取路径也没改。
- **计划里写的「补一条常态仍为 200/255」**在 `look_materials.rs` 的用例里做了（`常态保留半透明`），`lib.rs` 的用例补的是「选中态 `flags.z == 1` + `opacity == 1.0` + `alpha_mode == Opaque`」三条。

**验证**

```
cargo test -p plant-ui-view3d --lib     → 106 passed; 0 failed
cargo test -p plant-ui                  → 225 + 1 + 1 passed; 0 failed
cargo check --workspace --all-targets   → exit 0
cargo clippy -p plant-ui-view3d --all-targets → exit 0（warning 均为改动前既有）
```

单测覆盖到的行为：CE 屏上色 `(255,255,0)` 且 `flags.z == 1`；Highlight `(255,255,255)` 且 `flags.z == 1`；半透明件常态保留 `200/255`、选中态归一成 `1.0`；`XraySelected` 是 `(255,255,0)` + `160/255` 且与纯 `XRay` 不是同一枚；`material_state` 五条分支（含「X-Ray 命中且选中」「只 X-Ray 没选中」「撤掉 X-Ray 回 CE」）。

**待办**（第一条已被第三轮判定替代，见下）

- ~~像素探针（PC，`--look-demo`）确认四点读数：常态 `(189,189,189)` / CE `(255,255,0)` / highlight `(255,255,255)` / 背景底 `(242,242,242)`。这一条决定阶段二要不要做。~~ 第三轮改为按常量直接算对比度，判定阶段二不做。
- 真机对照图：改前 / 改后并排，写进 `docs/evidence/`（本轮没截图，因为需要起模型服务）。

### 2026-10-29 第二轮：阶段四（悬停预选）已落地

用户「继续」。按 D6 (a) 实施。

**改动清单**

| 文件 | 改动 |
|---|---|
| `crates/plant-ui-view3d/src/e3d_look/colour_table.rs` | 新增 `E3D_ACTIVE_COLOUR = 0xffbe00`（PDMS `orange`，E3D 的 active） |
| `crates/plant-ui-view3d/src/lib.rs` | `MaterialState` 加 `Hover`；`material_state` 多一个 `hovered: Option<RefU64>` 形参，优先级 `X-Ray > CE > Highlight > Hover > Base`；`View3d` 加 `hovered` 字段与 `hover(Option<[f32;2]>)` 入口；`ViewCommand::Hover(Option<[f32;2]>)`；`surface_hit` 多一个 `quiet: bool`（悬停不打诊断日志） |
| `crates/plant-ui-view3d/src/look_materials.rs` | `e3d_colour` 的 `Hover => (E3D_ACTIVE_COLOUR, 255)`；`flat` 集合加 `Hover`；PBR 分支的 `Hover` 落回类型色（改动前的画面不变） |
| `crates/plant-ui-view3d/src/virtual_hole.rs` | 孔洞材质裁决同步传 `view.hovered` |
| `crates/plant-ui/src/lib.rs` | `Cmd::HoverViewport(Option<[f32;2]>)` |
| `crates/plant-ui/src/workbench/view3d.rs` | 新增 `hover_pick()`：`HOVER_STEP_PX = 5` 节流、浮层排除、拖拽中不发、离开发一次 `None`；上一发位置记在 egui 临时数据（按 response id 分开） |
| `crates/plant-ui-app/src/main.rs` | 分发 `Cmd::HoverViewport(uv) => view3d.hover(uv)`；直通臂补一条 |

**与计划的出入**

- **节流阈值与判据**：计划写「按屏幕位移节流」；实施时发现「指针是否被 egui 占用」不能当拖拽判据（正常悬停时就为真），改成 `input.pointer.any_down()`；指针位置用 `ctx().pointer_hover_pos()` + `rect.contains` 而不是 `response.hover_pos()`——后者在测试的 Area 布局下拿不到。
- **PBR 外观**：计划没写，实施时决定 `Hover` 在 PBR 下仍落类型色，不加新反馈——「PBR 画面逐像素不变」是 ADR-0032 D2 的硬约束。
- **诊断日志**：`surface_hit` 加了 `quiet` 形参，悬停走静默路径；点击与相机手势照旧打日志（那两条是一次性动作）。

**验证**

```
cargo test -p plant-ui-view3d --lib   → 107 passed（+1：hover_only_paints_unselected_elements_and_matches_refno_or_owner）
cargo test -p plant-ui                → 227 passed（+2：悬停发命令 / 节流）
cargo test -p rs-plant                → 229 passed
cargo check --workspace --all-targets → exit 0
cargo clippy -p plant-ui -p plant-ui-view3d --all-targets → exit 0
```

注：`cargo clippy --workspace` 在 `crates/plant-ui-app/src/sim.rs:2289` 有两处既有 `FRAC_1_SQRT_2` 告警（改动前就有，与本次无关），`-D warnings` 下会挂；本次只跑了受影响的两个 crate 的 clippy。

### 2026-10-29 第三轮：悬停增量刷新 + 阶段二判定

用户「继续」。

#### 一、修阶段四留下的性能缺陷

阶段四把悬停接到了 `selection_dirty` 上，而 `apply_materials` 见到 `selection_dirty` 就整库重挑材质。悬停是连续动作（拖动掠过几十上百个构件），等于每次鼠标移动都把整库五万网格的 `material_state` 重算一遍、`LookMaterials` 重查一遍、`attach` 重走一遍变更检测——这是阶段四的实施瑕疵，不是设计意图（计划原文写的是「只重挑被波及的实体」）。

**改动**

| 文件 | 改动 |
|---|---|
| `crates/plant-ui-view3d/src/lib.rs` | `View3d` 加 `hover_dirty: bool` 与 `hover_applied: Option<RefU64>`；`ViewCommand::Hover` 只翻 `hover_dirty`，不再动 `selection_dirty`；`apply_materials` 拆成「全场刷」（`selection_dirty` / `material_dirty` / 有新 `Added<ModelMesh>`）与「悬停增量刷」（只重挑 refno 或 owner 命中「旧目标 + 新目标」的实体）两条路径 |

`hover_applied` 是「上一帧已经刷进材质的悬停目标」：悬停从 a 换到 b 时，a 要回常态、b 要上 active orange，两个都要重挑，其余实体的悬停态没变、句柄原样留着。

**新测试** `hover_change_repaints_only_the_affected_entities`：三个网格，先人为摘掉一个无关实体的材质组件当哨兵，然后连续做「上悬停 → 换目标 → 离开视口」三次增量刷，每次都断言哨兵仍无材质（证明走的真是增量路径而不是恰好句柄相同）；最后借 `material_dirty` 走一次全场刷，断言哨兵被补回来。

#### 二、阶段二（P1）判定：不做颜色改动

计划把阶段二挂成「等像素探针」。这一轮发现**不需要探针**：背景渐变两端的字节是代码里的常量（`presets.rs` 的 `e3d_background(true)` = 顶 159 / 底 242），选中色也是常量（`E3D_CE_COLOUR` / `E3D_HIGHLIGHT_COLOUR`），对比度是算出来的，不是量出来的。按 WCAG 相对亮度公式算：

| 前景 | 背景 | 对比度 |
|---|---|---|
| highlight 白 `(255,255,255)` | 背景底 `(242,242,242)` | **1.12 : 1** |
| highlight 白 | 背景顶 `(159,159,159)` | 2.65 : 1 |
| CE 黄 `(255,255,0)` | 背景底 | **1.04 : 1** |
| 常态 `(189,189,189)` | 背景底 | 1.68 : 1 |
| highlight 白 | 常态 189 | 1.88 : 1 |

**结论：颜色层面无解，且不该在这里解。**

1. 把背景渐变下端从白拉回 grey（计划首选方案）**会让常态件更难分辨**：189 落在更暗的背景上，对比度反而从 1.68 掉到 1.10 上下。方向是错的。
2. 反过来把背景推更亮（全白）会保住常态件、彻底抹掉 highlight 白。同样是错的。
3. 这不是移植缺陷：E3D 真机的背景渐变下端也是 242、highlight 也是纯白，1.12 : 1 是它自己的固有表现。真机上「下半屏选中白件还看得见」靠的是 HLR 黑轮廓，而我们的轮廓本来就在（`flat` 只关颜色项，`flags.x` 仍是 3，合成段 `bytes = col · hlr · ao` 照跑）。
4. 唯一能把「选中」这一层信息搬出颜色域的手段是**加粗 / 变色选中轮廓**，需要扩 prepass 的标记位。现在 `Rgb10a2Unorm` 的 alpha 只有 2 bit、`flags.x ∈ 0..3` 已占满（bit0 边线、bit1 伪阴影），要加「选中」位就得换纹理格式或从 30 bit 深度里挪一位——两者都会牵动 HLR / AO 的判据，必须重跑 `docs/evidence/2026-10-22-e3d-look.md:268` 那组「整库全视远景条纹」A/B。收益（下半屏白块多一圈粗边）不抵这个风险。

所以阶段二**关闭为「不做」**，理由按上面四条记档。若日后用户仍判定下半屏选中件看不清，下一步该做的是「选中描边」这个独立议题（扩标记位 + 重跑远景 A/B），不是改背景色。

**验证**

```
cargo test -p plant-ui-view3d --lib   → 108 passed（+1：hover_change_repaints_only_the_affected_entities）
cargo test -p plant-ui                → 227 passed
cargo test -p rs-plant                → 229 passed
cargo check --workspace --all-targets → exit 0
cargo clippy -p plant-ui -p plant-ui-view3d --all-targets → exit 0
```

**剩余待办**

- 真机对照图：改前 / 改后并排，写进 `docs/evidence/`。仍需起模型服务，本机没起。
- 阶段二已判定不做；「选中描边」记为另议项，不排期。

### 2026-10-29 第四轮：悬停的三处边角

用户「继续」。前一轮把悬停改成了增量刷，但悬停本身的输入侧还有三个洞。

#### 一、节流阈值两个轴不等价

原式用视口**宽度**统一归一：

```
dx² + dy² < (HOVER_STEP_PX / rect.width())²
```

UV 的 x 与 y 各铺满纹理的一整条边，而视口宽高比通常不是 1:1。1200×700 的视口里，竖直方向 5 pt 折成 `5/700 = 0.00714` 个 UV，而阈值是 `5/1200 = 0.00417`——竖直方向实际阈值只有约 **2.9 pt**。视口越扁越糟：1600×500 时竖直阈值掉到 2 pt 以下，鼠标轻轻一抖就换一发射线，正对着「别白烧」的反面。

改成 `hover_step_in_points(rect, texture, delta)`：x 按裁切后的 UV 宽、y 按裁切后的 UV 高各自折算，与 `pointer_texture_uv` 的映射严格互逆。

#### 二、滚轮缩放时悬停停在旧元素上

滚轮缩放时指针不动、相机在动，指针底下的元素一直在换。上一轮的 `hover_pick` 只在指针位置变化时动作，所以缩放期间悬停色会**停在缩放前指着的那个元素上**，直到鼠标再动一次——那是错的反馈，比没有更糟。

`hover_pick` 多一个 `camera_busy` 形参，`show()` 里先读 `smooth_scroll_delta` 再调 `camera`（后者会把它清零，读晚了就永远是 0），条件与 `camera` 里那条缩放判定同口径（指针在视口、不在浮层上）。

#### 三、`surface_hit` 的遍历代价已核实，不改

担心悬停射线太贵，去读了 Bevy 的实现（`bevy_picking/src/mesh_picking/ray_cast/mod.rs::cast_ray`）：它**先对全部候选实体做 AABB 求交、`par_iter` 并行**，按近到远排序，只对 AABB 命中的实体做三角求交，并且在 `nearest_blocking_hit` 之外提前收手。也就是说「整库五万网格」这一趟在 AABB 阶段就被筛掉了，真正的三角求交只落在射线穿过的那几个包围盒上。**不需要自己再加一层预筛**——加了只会重复它已经做过的事。

节流值维持 `HOVER_STEP_PX = 5`（修正后两个轴都是真 5 pt）。

**新测试**

| 测试 | 钉住什么 |
|---|---|
| `hover_throttle_holds_on_the_vertical_axis_in_a_wide_viewport` | 折算本身（竖直 3 pt 折回 3 pt）+ 竖直 3 pt 不发、竖直 40 pt 发 |
| `zooming_clears_the_hover_instead_of_leaving_it_on_a_stale_element` | 同位置滚一格滚轮要发 `None`，且该帧末态无悬停 |

**验证**

```
cargo test -p plant-ui-view3d --lib   → 108 passed
cargo test -p plant-ui                → 229 passed（+2）
cargo test -p rs-plant                → 229 passed
cargo check --workspace --all-targets → exit 0
```

`CONTEXT.md` 的「悬停预选」词条补两句：缩放时不显示、节流两轴各自折算。

### 2026-10-29 第五轮：悬停账的卸载清理 + 孔洞刷新核实

用户「继续下一步」。这轮顺着「悬停引入的新状态有没有人负责清理」往下查。

#### 一、修：卸载构件时悬停账没清（真实缺陷）

`forget_model_state`（`lib.rs:1775`）负责在构件卸载时把各种按 refno 记的账抹掉：`desired_visibility` / `render_states` / `bounds` / `selected` / `primary` / `xray` / `isolate_restore`。上一轮加的 `hovered` 与 `hover_applied` **没进这张清单**。

后果有两条：

1. **状态泄漏**：卸掉的构件如果正是悬停目标，`hovered` 会一直指着它。同一个 refno 的模型重新装回来时（换房间、重新生成、切供数模式都会），它一出生就被涂成 active orange，而鼠标根本不在那儿。
2. **增量刷空转**：`hover_applied` 指向已 despawn 的实体，下一轮悬停增量刷会去重挑一个死 refno，白走一趟 `LookMaterials` 查询。

修法：`forget_model_state` 里判断 `hovered` / `hover_applied` 是否落在 `removed` 里，是就清空。只清 `hover_applied` 不翻 `hover_dirty` 是安全的——那个实体已经 despawn，没有材质要刷。

测试挂在已有的 `unloading_a_model_forgets_its_render_and_interaction_state` 上：先让 `hovered` / `hover_applied` 都指向「将被卸掉的那一个」（与保留件分开，否则测不出泄漏），断言卸载后两笔账都是 `None`。

#### 二、核实：`apply_hole_materials` 不需要脏标（不改）

怀疑它排在 `apply_materials` 之后、每帧无条件全场刷，会把上一轮的增量优化整个盖掉。读下来不是：

- 它的 query 是 `(Entity, &ModelMesh, &HolePreviewed, ...)`，**只匹配被布尔替换过网格的实例**——开孔目标的那么几个到几十个，不是整库。
- `ModelMaterial::attach`（`look_materials.rs:40`）在同类组件下先比句柄，`current.0 != handle` 才写，没变就不碰，不白触发变更检测。

几个到几十个实体每帧比一次句柄，代价可以忽略；为它加一套脏标只会多一份要和 `HolePreview` 状态同步的东西，是负收益。

#### 三、核实：`room_clip` 没有独立的材质裁决分支（不改）

`room_clip.rs` 里只有 `MeshMaterial3d(xray.clone())`（房间面板直接挂共享 X-Ray 材质）与 `MeshMaterial3d(section.clone())`（断口截面），两处都不是 `material_state` 的裁决对象——房间体是辅助几何，本来就不参与选中 / 悬停 / X-Ray 三态。补出来的兄弟段带 `ModelMesh` 出生，材质由同帧的 `apply_materials` 按 `Added<ModelMesh>` 补，走的是同一条裁决。与计划「风险」段那条待确认项对上了：**确认没有独立分支，不用改**。

**验证**

```
cargo test -p plant-ui-view3d --lib   → 108 passed
cargo test -p plant-ui                → 229 passed
cargo test -p rs-plant                → 229 passed
cargo check --workspace --all-targets → exit 0
```



