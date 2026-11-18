# 三维选中效果改进（染色着色 + 选中描边）plant-ui 开发计划

日期：2026-11-18；状态：**已实施并本地提交，缺省配色待用户看图定**（用户 2026-11-18「参考 plant3d-web 选中模型的效果，完善 ../plant-ui 里的选中效果，目前选中的效果太难看了」；按 D1 – D8 的 (a) 项实施，见文末「进度」）。

## 当前基线

仓库：`D:/work/plant-code/old/plant-ui`（唯一落点，只改这一个仓）。参照侧 `D:/work/plant-code/old/plant3d-web` 只读，**一字不改**。

### 改动前选中是怎么画出来的

| 环节 | 位置 | 改动前 |
|---|---|---|
| 状态裁决 | `crates/plant-ui-view3d/src/lib.rs` 的 `material_state` | 六态 `Base / Hover / Highlight / Ce / XRay / XraySelected`，优先级 X-Ray > CE > Highlight > Hover > Base（这次不动） |
| E3D 外观配色 | `look_materials.rs` 的 `e3d_colour` | CE `#ffff00`、其它选中 `#ffffff`、悬停 `#ffbe00`，**alpha 一律 255** |
| E3D 外观着色 | `look_materials.rs` 的 `sgl()` → `sgl_look.wgsl` 的 `flags.z == 1` | CE / Highlight / Hover 全部**颜色直出**：不打光、不反射，一个件一个纯色 |
| PBR 外观 | `look_materials.rs` 的 `resolve()` | CE / Highlight 同落 `SELECT_COLOR`（橙红 `srgb(1, 0.27, 0)`），**alpha 255**，照常打光；悬停没有颜色 |
| 描边 | — | 没有。E3D 外观的黑色边线是外观自己的（参与的件都有），不表示选中 |

「直出」与「一律不透明」是 2026-10-29 那一轮定的（`docs/plans/2026-10-29-selection-highlight-visibility-plannotator.md` 的 D1 (a) / D3 (a)），当时要解决「选中色被环境反射洗淡」与「半透明件选了等于没选」。可辨性解决了，代价是下面四条。

### 难看在哪

图在 `docs/evidence/2026-11-18-selection-outline/`，每张左半是改动前。

1. **选中件是一块没有明暗的色块**。CE 黄的方箱三个面同一个 `(255,255,0)`，分不出哪面朝哪（`e3d-one-before-after.png`）。
2. **半透明件一选中就成了不透明的实心块**。房间体（alpha 64）选中后是一整块纯黄，里面的设备、管道全挡住（`e3d-room-before-after.png`）；PBR 外观下是一整块砖红（`pbr-room-before-after.png`）。
3. **鼠标掠过房间，整间房变成不透明的橙色块**。悬停与选中同一套「直出 + 不透明」，鼠标在房间里一动整屏橙色，已选中的件也被盖住（`e3d-hover-before-after.png`）。
4. **选中只靠颜色一条腿**。highlight 白落在浅色背景上对比度 1.12 : 1（10-29 第三轮算过），当时把「选中描边」列成独立议题、没排期；两种外观都没有「轮廓」这一层。

### plant3d-web 的选中是怎么画的（参照）

| 环节 | 位置 | 做法 |
|---|---|---|
| 选中色 / 悬停色 | `src/components/dock_panels/ViewerPanel.vue:439-440` | `WEB_SELECTION_COLOR = 0xff4fd8`（品红）、`WEB_HOVER_COLOR = 0xffaa44`（橙） |
| 换色 | `src/utils/three/dtx/DTXLayer.ts:2258`（`setObjectColor`）→ `DTXMaterial.ts:147-150` | 着色器里只把 `baseColor` 换成覆盖色的 rgb：**照常打光，不透明度还是构件原来的** |
| 描边 | `ViewerPanel.vue:4025-4033`、`src/utils/three/dtx/outline/DTXOutlineHelper.ts:128` | three.js `OutlinePass`：`edgeColor 0xff9ae8`（粉）、`edgeStrength 1.6`、`edgeThickness 1.0`、`edgeGlow 0` |
| 被挡住的轮廓 | `DTXOutlineHelper.ts:293-294` | `visibleEdgeColor` 与 `hiddenEdgeColor` 同色：**被别的构件挡住的那一段照画** |

一句话：**打着光的染色 + 一圈穿透的描边 + 不动透明度**。plant-ui 改动前这三条都是反的。

## 决策摘要（推荐项在前）

| 编号 | 决策 | 推荐与理由 |
|---|---|---|
| D1 选中 / 悬停怎么着色（E3D 外观） | (a) **染色着色**：颜色乘一个随朝向变的明暗系数，只缩放、不往白里加；(b) 回到完整公式（`colour·(Ka + Kd·N·L) + Ks·spec + Kr·env`）；(c) 维持直出 | **(a)**：(b) 就是 10-29 之前的样子——出厂预设 `Kr 0.8`，CE 黄被洗成 `(255,255,79)`，highlight 白与常态灰只差 60 个灰阶，那一轮正是为这个才改的直出；(c) 是现状。染色的最亮处就是色表上的原色，暗面按系数压暗：不洗淡，也有体积感 |
| D2 暗面压到几成 | (a) **看颜色的饱和度**：有色相的压到 0.68，没色相的（白）只压到 0.88，中间线性；(b) 统一 0.68 | **(a)**：统一 0.68 时实测 highlight 白的选中管读数 177–202，比旁边没选中的管（173–238）还暗——白一压暗就是常态的 lightgrey。分开之后白是 214–238，没选中的 164–200。CE 黄再往下压会发橄榄绿，0.68 是试出来的下限 |
| D3 选中件的透明度 | (a) **沿用构件自己的**（两种外观都是）；(b) 维持「一律不透明」 | **(a)**：房间体选中后还是半透明，里面看得见（plant3d-web 同口径）。10-29 改成不透明是因为「半透明件选了等于没选」——有了描边，这条理由不成立了 |
| D4 加不加描边 | (a) **加，两种外观都加**：选中件外沿一圈，被挡住的部分照画；(b) 只改染色 | **(a)**：把「选了谁」从颜色域里搬出来一层。白件落在白背景上、半透明件染色很淡时，轮廓还在。10-29 第三轮列的独立议题，这次做掉 |
| D5 描边怎么实现 | (a) **遮罩相机**：选中件多挂一层 `RenderLayers`，一台只看这一层的相机把它们画成剪影，全屏一段在剪影外沿叠色；(b) 把「选中」编进 prepass 标记位；(c) 法线外扩再画一遍 | **(a)**：(b) 是 10-29 否掉的路——`Rgb10a2Unorm` 的 2 bit 已满，动它要重跑远景 A/B，而且 PBR 外观没有这张 prepass；(c) 在方箱的棱角处会裂开，半透明件也画不对。遮罩相机两种外观共用，不碰既有管线，没有选中件时整台相机关着 |
| D6 描边的宽度与颜色 | (a) **2 个逻辑点 × 屏幕缩放，夹在 1.5–6 物理像素**；缺省橙红 `SELECT_COLOR`；(b) 固定物理像素 | **(a)**：高分屏上固定 1–2 物理像素会细成发丝。橙红在 E3D 外观的浅灰渐变底、PBR 外观的深浅两套底上都站得住 |
| D7 配色跟不跟 plant3d-web 的品红 | (a) **缺省不跟**，留开关 `PLANT_UI_SELECTION_PALETTE=web`；(b) 直接换成品红 / 粉 | **(a)**：品红 `#ff4fd8` 与本项目的剖切色 `SECTION_COLOR`（`#D946EF`，`room_clip.rs:37`）几乎同色，开着直段截断时会混；E3D 外观的「主选中黄、其它选中白」是 ADR-0032 D3 定的界面语义，品红口径两者不分。**这一条留给用户看图定**（并排图 `e3d-multi-native-vs-web.png` 等三张） |
| D8 悬停 | (a) **只染色、不描边**，同样沿用透明度；(b) 悬停也描边 | **(a)**：描边留给「选了谁」；鼠标一路掠过去，轮廓跟着闪太吵 |
| D9 非目标 | 不动 `material_state` 的优先级；不动 prepass 编码与边线 / 伪阴影；不动 `plant3d-web`；不给 PBR 外观加悬停色（原来就没有）；不加设置窗开关。（原先还列了「不做 WebGL2 端验证」，后来补验了，见「进度」） | — |

## 实施

1. **染色着色**（`e3d_look/material.rs`、`e3d_look/sgl_look.wgsl`）。`flags.z` 从「直出与否」扩成三档 `SHADING_LIT = 0 / SHADING_FLAT = 1 / SHADING_TINT = 2`，`SglLookMaterial::tint()` 与 `flat()` 并列。着色器里
   ```wgsl
   rgb = c * tint_shade(c, kd, ks, kr, ndl, spec, env);
   // tint_shade = mix(tint_shade_floor(c), 1, clamp((kd·ndl + ks·spec + kr·lum(env)) / max(kd + kr·sky, 1e-4), 0, 1))
   // tint_shade_floor(c) = mix(0.88, 0.68, (max − min) / max)
   ```
   分母取「正对头灯、反射到天顶」，所以出厂预设（`Kd 0 / Kr 0.8`）下明暗来自环境反射的亮度，换预设也成立。X-Ray 两态仍是直出。
2. **配色与透明度**（`look_materials.rs`）。新增 `SelectionPalette { Native, Web }`，填色与描边色在这一处定；`e3d_colour` 的 `Ce / Highlight / Hover` 不再把 alpha 写成 255，沿用元素自己的；PBR 分支的选中色带上元素的 alpha。X-Ray 叠选中仍是 CE 色 + alpha 160。
3. **选中描边**（新文件 `selection_outline.rs` + `selection_mask_resolve.wgsl` + `selection_outline.wgsl`）。
   - 遮罩图按视口屏幕分辨率的 2 倍画（与主相机的超采样倍数无关；超过 1600 万像素时等比例缩，最少缩回屏幕分辨率），遮罩相机是主相机的子实体：`order: -1`、`RenderLayers::layer(1)`、不开多重采样、清成透明、不挂 prepass 与后处理、起步关着。第一版是「屏幕分辨率 + MSAA 4×」，DX12 上画出来是坏的，才改成现在这样（见「进度」与「风险与已知限制」第 8 条）。
   - `apply_materials` 给 `MaterialState::outlined()`（`Ce / Highlight / XraySelected`）的网格挂 `RenderLayers[0, 1]`，不再选中就摘掉；只在要变时动。
   - 全屏节点排在 `Core3d` 的 `SglLookLabel` 之后、`Node3d::Tonemapping` 之前，分两段：先把遮罩的 alpha 平均成一张屏幕分辨率的单通道覆盖率图（`selection_mask_resolve.wgsl`，抗锯齿出在这一步）；再在覆盖率图上 16 个方向三圈采样，按邻域最大值归一后判内外（半透明选中件的覆盖率不到 1），在 sRGB 字节域按直通 alpha 叠上去（`selection_outline.wgsl`）。
   - 多个选中件共用一张遮罩：贴在一起的两个选中件之间没有线，轮廓是并集的。
4. **空闲停画的闸门**（`render_gate.rs`）。认 `Changed<RenderLayers>`、`RemovedComponents<RenderLayers>`、`Changed<SelectionOutline>`：选中变了要醒来画一帧。遮罩相机跟主相机一起开关。
5. **宿主**（`plant-ui-app/src/main.rs`）。每帧 `view3d.set_pixels_per_point(ctx.pixels_per_point())`，描边宽度靠它换算。

核对过的两处旁路：

- **孔洞预览**（`virtual_hole.rs` 的 `apply_hole_materials`）只换材质、不动图层。它与 `apply_materials` 对「是否选中」的判断恒等（孔洞 X-Ray 只在 `XRay` 与 `XraySelected` 之间换，选中与否不变），所以描边跟得上。
- **直段截断**（`room_clip.rs`）补出来的兄弟段带 `ModelMesh` 出生，材质与图层都由同帧的 `apply_materials` 给；断口不挂 `ModelMesh`，不单独描边。房间体图层不是 `ModelMesh`，从不描边。

## 风险与已知限制

1. **PBR 外观的选中态画面变了**（半透明件保留透明度、多一圈描边）。这偏离了 10-29 第二轮说的「PBR 画面逐像素不变」；常态画面没变。用户这次的要求就是改选中的观感，两种外观一起改才不会一边好看一边难看——写进了 ADR-0032 的修订。
2. **大库上的开销没量过**。遮罩相机把选中件再画一趟（2 倍屏幕分辨率、不开多重采样、没有 prepass 与后处理），开销随选中件数涨；整库全选时约等于多画一遍主通道。演示厂（7 个件）选中整间房：DX12 16.7–17.8 ms、网页端 60 帧，都顶在刷新率上，没有在真实大库上量。显存上，有选中件时多占每屏幕像素约 65 字节（本机视口约 85 MB，封顶约 256 MB；按纹理尺寸算的，没有实测）。空闲停画仍然生效，只有相机在动或选中在变时才画。
3. **WebGL2 / wasm 端只在无头 Chrome + 演示厂上验过**（evidence 3.3）：画面与桌面一致，控制台没有着色器报错，没有超过 100 ms 的帧间隔。没有在带界面的浏览器里人手点过，没有连真实后端，没有试别的浏览器。
4. **X-Ray 叠选中的描边只有单测**，演示厂开不了房间 X-Ray，没有截图。
5. **描边穿透是有意的**（plant3d-web 同口径）：被挡住的选中件会在挡它的构件上画出轮廓。选中一大片被遮挡的件时画面会偏乱。
6. **半透明的选中件没有 E3D 外观的黑边线与伪阴影**（半透明网格不进 prepass，ADR-0032 D4），与 10-29 之前一样；现在由描边补上。
7. **alpha 很低的选中件描边会跟着变淡**（遮罩里 alpha 低于 1/8 时按比例淡出），免得把噪声放大成一圈实线。
8. **遮罩不开多重采样，靠 2 倍分辨率自己平均**。本机 DX12（RX590）上多重采样的遮罩相机画出来是坏的：头一回选中遮罩是空的，之后带成对的坏点；Vulkan 与 WebGL2 上没有这个问题（evidence 第 4 节）。是驱动还是 wgpu / Bevy 的问题没有定位，也没有在别的显卡上试过；现在的做法绕开了它。另：DX12 + PBR 外观主场景本身画不出来是改动前就有的老问题（关掉 PBR 主相机的多重采样就正常），不在这次范围。

## 验证命令

```bash
cargo test -p plant-ui -p plant-ui-view3d -p rs-plant
cargo clippy -p plant-ui -p plant-ui-view3d -p rs-plant --all-targets
cargo check -p plant-ui -p rs-plant --all-targets
cargo build -p rs-plant

# 演示厂（不连库）：E3D 外观
PLANT_UI_SIM=1 BEVY_ASSET_ROOT=. target/debug/rs-plant --look-demo
# PBR 外观：设置文件写一行 (look: (mode: pbr))
PLANT_UI_SIM=1 BEVY_ASSET_ROOT=. PLANT_UI_SETTINGS_FILE=<那个文件> target/debug/rs-plant --look-demo
# 品红配色：再加 PLANT_UI_SELECTION_PALETTE=web
```

## 进度

### 2026-11-18：已落地

**改动清单**

| 文件 | 改动 |
|---|---|
| `crates/plant-ui-view3d/src/selection_outline.rs`（新） | 遮罩图与覆盖率图、遮罩相机、`SelectionOutline` 组件、四个系统（建、跟随、每帧更新、开关）、全屏节点（两段）与管线、五条单测 |
| `crates/plant-ui-view3d/src/selection_mask_resolve.wgsl`（新） | 把 2 倍分辨率的遮罩平均成屏幕分辨率的覆盖率图 |
| `crates/plant-ui-view3d/src/selection_outline.wgsl`（新） | 描边着色器（在覆盖率图上找边、叠色） |
| `crates/plant-ui-view3d/src/look_materials.rs` | `SelectionPalette`；选中 / 悬停沿用元素 alpha；E3D 外观选中 / 悬停走 `tint()`；PBR 选中色带 alpha；单测改写与新增 |
| `crates/plant-ui-view3d/src/e3d_look/material.rs` | `SHADING_LIT / SHADING_FLAT / SHADING_TINT`、`tint()`；着色器文本断言跟着改 |
| `crates/plant-ui-view3d/src/e3d_look/sgl_look.wgsl` | `tint_shade_floor`、`tint_shade`、`flags.z == 2` 分支 |
| `crates/plant-ui-view3d/src/e3d_look/post/mod.rs` | `SglLookLabel` 放开到 crate 内可见（描边节点要排在它后面） |
| `crates/plant-ui-view3d/src/lib.rs` | 挂插件与系统；`View3d.pixels_per_point` 与 `set_pixels_per_point`；`MaterialState::outlined()`；`apply_materials` 挂 / 摘遮罩层；相关单测 |
| `crates/plant-ui-view3d/src/render_gate.rs` | 闸门认遮罩层与 `SelectionOutline` 的变化；新单测 `selection_outline_changes_wake_the_camera` |
| `crates/plant-ui-app/src/main.rs` | 每帧上报 `pixels_per_point` |
| `docs/adr/0032-...md`、`CONTEXT.md`、`CHANGELOG.md`、`docs/evidence/2026-11-18-selection-outline*` | 修订、词条、界面变化、证据 |

**与 10-29 那一轮的关系**

- 撤回 D1 (a)「选中态直出」与 D3 (a)「选中态 alpha 归一到 255」——改成染色着色与沿用透明度；它们当初要解决的两件事（洗淡、半透明件选了等于没选）分别由「只缩放不加白」与描边接住。
- 做掉第三轮列的独立议题「选中描边」，但没走当时设想的 prepass 标记位，prepass 编码与远景 A/B 都没动。
- X-Ray 叠选中（D5）、悬停预选的接线（D6）、状态优先级都不变。

**验证**

```
cargo test -p plant-ui -p plant-ui-view3d -p rs-plant → plant-ui 267 + 1 + 1、plant-ui-view3d 133、rs-plant 279，全过
cargo clippy -p plant-ui -p plant-ui-view3d -p rs-plant --all-targets → 无 error
cargo check -p plant-ui -p rs-plant --all-targets → exit 0
cargo build -p rs-plant → exit 0
```

clippy 的告警：新文件里有 2 条 `function check is never used`（`ShaderType` 派生宏生成的，库里别的 uniform 结构体已有 13 条同类），其余都落在这次没动的行上（逐条对过行号）；这次没有新增别的。

新增 / 改写的单测覆盖：遮罩相机挂在主相机下、不开多重采样、跟着它的图与投影走；遮罩图是屏幕分辨率的 2 倍且受像素预算约束；只在「有选中件且主相机在画」时开；描边宽度跟屏幕缩放并夹在上下限内；uniform 与着色器布局一致；选中变化（挂层、摘层、描边参数变）会唤醒停画的相机；配色开关同时换填色与描边色；选中 / 悬停保留元素透明度、走染色而不是直出；选中件挂遮罩层、取消后摘掉。

**演示厂实拍**（`--look-demo`，150% 缩放的屏，窗口挪到屏幕外抓图；图与读数见 `docs/evidence/2026-11-18-selection-outline.md`）

| 项 | 读数 |
|---|---|
| 描边核心色（缺省配色） | 约 `(247–249, 83–84, 17–25)`，宽约 2 个逻辑点，两种外观一致 |
| E3D 外观其它选中白 | 管 214–224、柱 220–238；旁边没选中的件 164–200 |
| PBR 外观选中填色 | 约 `(182, 67, 45)`，外沿一圈亮橙红 |
| 帧时间 | 选中整间房、窗口有焦点：16.7 ms（60 帧封顶） |

取消选中后描边消失、没有残影；选中变化能唤醒停画的相机；各次运行日志里没有崩溃与 wgpu 报错。

**遮罩改版（同日）**

上面的读数是第一版遮罩（屏幕分辨率、遮罩相机 MSAA 4×）在 Vulkan 上量的。之后用 `WGPU_BACKEND=dx12` 跑，第一版在 DX12 上是坏的：头一回选中没有描边，再点几次轮廓里出现成对的坏点（描边像素数到了 3161，应为 2099）；Vulkan 每次都是 2099。只把遮罩相机的多重采样关掉就好了，于是改成现在的做法：遮罩不开多重采样、按 2 倍分辨率画，全屏节点先平均成覆盖率图再找边。改完 Vulkan 与 DX12 都是 2116，第一次选中就有描边，连点四次逐像素不变。数与图见 evidence 3.2 与第 4 节。

**网页端复验（同日）**

用最终代码重编 wasm 包（没有覆盖 `web/public/pkg`），无头 Chrome（ANGLE Direct3D11 → WebGL 2.0）跑演示厂：两种外观的填色与描边都与桌面一致，重复选中逐像素不变，悬停只染色不描边，控制台没有着色器 / WebGL 报错，没有超过 100 ms 的帧间隔（PBR 外观头一回选中有一次 50 ms）。数与图见 evidence 2.3 与 3.3。

**没做的**

- PBR 外观的悬停色（原来就没有，这次不加，要的话另加）。
- 设置窗里的配色开关（现在只有环境变量）。
- 大库上的帧时间、X-Ray 叠选中的截图——见「风险与已知限制」第 2、4 条；网页端只做了无头浏览器 + 演示厂这一层（第 3 条）。
- DX12 上多重采样遮罩为什么坏，没有往下查（第 8 条）。
- 多选与「被遮挡仍有描边」的截图：演示厂的视口只能单选，这几张是临时加了一个从文件读选择集的调试入口拍的，拍完已撤掉，没进提交。改动前的二进制同样没法在演示厂里多选，所以多选没有「改前」图。

**待用户定**

1. 缺省配色：保持「黄 / 白 + 橙红描边」，还是换成 plant3d-web 的「品红 + 粉描边」（D7）。
2. PBR 外观要不要也加悬停色。
3. DX12 + PBR 外观主场景画不出来的老问题要不要处理（关掉 PBR 主相机的多重采样能好，但 PBR 外观就没有抗锯齿了；本机缺省的 Vulkan 不受影响）。
