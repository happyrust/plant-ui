# 三维选中效果改进（染色着色 + 选中描边）plant-ui 开发计划

日期：2026-11-18；状态：**已实施并本地提交；留给用户的几项已定，PBR 外观的悬停预选已追加**（用户 2026-11-18「参考 plant3d-web 选中模型的效果，完善 ../plant-ui 里的选中效果，目前选中的效果太难看了」；按 D1 – D8 的 (a) 项实施，见文末「进度」；已定的几项与追加的悬停预选在「进度」的最后两段）。

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
| D7 配色跟不跟 plant3d-web 的品红 | (a) **缺省不跟**，留开关 `PLANT_UI_SELECTION_PALETTE=web`；(b) 直接换成品红 / 粉 | **(a)**：品红 `#ff4fd8` 与本项目的剖切色 `SECTION_COLOR`（`#D946EF`，`room_clip.rs:37`）几乎同色，开着直段截断时会混；E3D 外观的「主选中黄、其它选中白」是 ADR-0032 D3 定的界面语义，品红口径两者不分。**这一条留给用户看图定**（并排图 `e3d-multi-native-vs-web.png` 等三张）。已定：缺省不换，见「进度」的「已定的四项」 |
| D8 悬停 | (a) **只染色、不描边**，同样沿用透明度；(b) 悬停也描边 | **(a)**：描边留给「选了谁」；鼠标一路掠过去，轮廓跟着闪太吵 |
| D9 非目标 | 不动 `material_state` 的优先级；不动 prepass 编码与边线 / 伪阴影；不动 `plant3d-web`；不给 PBR 外观加悬停色（原来就没有；同日改了，加了，见「进度」的「追加 PBR 外观的悬停预选」）；不加设置窗开关。（原先还列了「不做 WebGL2 端验证」，后来补验了，见「进度」） | — |

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
9. **共面处指到谁不稳定**（拾取的老行为，这次没动）。两个件的面贴在同一个平面上时，射线到两者等距，`surface_hit` 取到哪一个由浮点舍入定：相邻像素可以不同；同一个屏幕点先后拾取两次也翻过（第一次拾到方箱，第二次拾到房间体；两次的命中点相差约 1e-5，射线并不逐位相同，是哪一步带来的没有往下查）。演示厂里方箱、立柱的底面与房间地板同在 Z = 0，相机从下往上看，所以指着方箱底面时亮起来的有时是房间体；点选同理。悬停只是把它显出来了，与悬停怎么画无关。

   **已定（同日）：这一轮不改拾取**（用户把这一项也交给实施方结合现状选）。理由：

   - 演示厂的这种并列是它自己凑出来的：半透明的房间体在演示厂里是可拾取的构件（`e3d_look/demo.rs` 给每个件都挂 `ModelPickTarget`，透明度也是它自己写的），相机又在地板下面。程序里真正的房间体图层不参与拾取（`room_clip.rs` 的单测钉着「房间体点不中」），真实构件的颜色都来自 `type_color`，除 CE 外全是不透明的。真实模型里要出现并列，得是两个不透明件有一块重合的面同时朝着相机——那种地方画面本身就在深度打架，指到谁都说得通。有没有、有多少，还没有在真实模型上看过（见下面的订正）。
   - 要改就得让拾取知道「第二近的是谁」：要么多打一发射线，要么关掉「命中最近的就停」。`surface_hit` 不只管悬停和点选，旋转 / 平移起手和每一格滚轮缩放也靠它取落点；大模型上多出来的开销还没有量过（到现在只跑过 7 个件的演示厂；见下面的订正）。
   - 并列时谁该赢（不透明的优先？小的优先？）得先有规则。

   以后在真实模型上确实遇到再做：只在第二个命中与最近命中的距离差落在容差内时按规则改判，非并列的结果与现在逐位相同，并先量过悬停与滚轮缩放的开销。

   **订正（同日稍后）**：上面前两条理由里原先写的是「现在没有真实模型可看（本机数据库停着）」「开销现在量不了」。前一句是错的：写的时候没有重新查，本机的数据库（8009）与模型服务（8022）当天 10:24 起就在跑（不是这条线起的，进程来自当天的发布包目录）。所以成立的只是「还没看、还没量」，不是「看不了、量不了」。没有立刻去连那套服务，是另一个原因：桌面端点眼睛显示节点时，模型查不到或不全就会自动向模型服务发一发补缺口的生成请求（`plant-ui-app/src/main.rs` 的 `start_auto_generate` → `data::Req::GenerateModels { force: false }`），不保证只读，而那套服务不是这条线起的。打算在自己起的隔离后端上看：选中与悬停在真实模型上的观感、有没有共面并列、悬停与拾取的开销。看过之后回头复核「这一轮不改拾取」——它的另外两条理由（演示厂特例、并列规则未定）不受这处订正影响。

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

- PBR 外观的悬停色：这一段落地时没做，同日追加，见下面「追加 PBR 外观的悬停预选」。
- 设置窗里的配色开关（现在只有环境变量）。
- 大库上的帧时间、X-Ray 叠选中的截图——见「风险与已知限制」第 2、4 条；网页端只做了无头浏览器 + 演示厂这一层（第 3 条）。
- DX12 上多重采样遮罩为什么坏，没有往下查（第 8 条）。
- 多选与「被遮挡仍有描边」的截图：演示厂的视口只能单选，这几张是临时加了一个从文件读选择集的调试入口拍的，拍完已撤掉，没进提交。改动前的二进制同样没法在演示厂里多选，所以多选没有「改前」图。

**已定的四项（同日）**

上面留了三项给用户定（缺省配色、PBR 外观要不要悬停色、DX12 + PBR 外观的老问题要不要处理），另有一项「推不推送」。用户交代：按推荐项办；没有明确推荐的，由实施方结合现状选最合适的，并说明理由。于是这样定：

1. **缺省配色不换**：保持「黄 / 白 + 橙红描边」，品红那套仍用 `PLANT_UI_SELECTION_PALETTE=web` 打开。这是 D7 的推荐项，理由不变；没有改代码。
2. **PBR 外观加悬停色**（没有明确推荐，选了「加」）。理由：plant3d-web 有悬停色，PBR 外观本来就是对着它做的；E3D 外观已经有，两种外观不该一个有一个没有；悬停的判定两种外观本来就是同一套，PBR 只是没给颜色，改动很小。颜色用配色表里现成的悬停橙，不另起新色；保留元素自己的透明度，不描边。做法与验证见下一段。
3. **DX12 + PBR 外观主场景画不出来：这一轮不动，记为已知问题**（没有明确推荐，选了「不动」）。理由：只在这一块显卡上见到，原因没有定位；已知的绕法是关掉多重采样，那会让所有走 DX12 的机器在 PBR 外观下失去抗锯齿，拿一台机器的现象去改所有机器不稳妥；本机缺省的 Vulkan 不受影响。等有第二台机器能复现、或者确实有人要用 DX12 + PBR 时再做。
4. **先不推送**（没有明确推荐，选了「不推」）。理由：按本机记录，这条分支从没推到过远端，一推会新建远端分支，并把分支上别的本地提交一起发出去，超出这次的范围，而且收不回来。提交都只在本地分支 `feat/model-load-parity` 上，要推由用户说。

### 2026-11-18：追加 PBR 外观的悬停预选

**做法**

- 鼠标指着未选中的元素时，PBR 外观也换成悬停色：与 E3D 外观同一个（`SelectionPalette::hover()`，缺省橙 `#ffbe00`，`web` 口径 `#ffaa44`），沿用元素自己的透明度，不描边，选中件不被它覆盖。`material_state` 的优先级、悬停的拾取与节流、增量刷新都没动，改的只是 `LookMaterials::resolve` 里 PBR 分支对 `Hover` 的裁决（原来落回类型色）。
- **实体另加一点自发光**（`PBR_HOVER_GLOW`，悬停色线性值的 30%）。先做的一版只换颜色，实拍有两处看不出来：设备的类型色 `type_color("EQUI")` 与缺省悬停橙是同一个 `#ffbe00`，指上去 0 个像素变化；背光面本来就暗，颜色是乘上去的，换没换差不多。自发光是加上去的，两种情况都亮得起来；30% 不至于把明暗抹平。这比定这一项时说的「用现成的悬停橙」多了一步，颜色本身没有另起。
- 半透明件的自发光被着色器按透明度压两遍，alpha 64 的房间体上几乎不显，它们靠换色；无光照的回退线不吃自发光，只换色。
- 材质仍按（颜色, 是否线, 状态）缓存，同色的件共用一枚；没有新增每帧的开销。

**改动清单**

| 文件 | 改动 |
|---|---|
| `crates/plant-ui-view3d/src/look_materials.rs` | 配色表的 `e3d_hover` 改名 `hover`（两种外观共用）；PBR 分支的 `Hover`；`PBR_HOVER_GLOW` 与 `hover_material`；新单测 `pbr_look_hover_is_the_hover_colour_with_a_glow`，配色开关与回退线的单测各补一段 |
| `crates/plant-ui-view3d/src/lib.rs` | `MaterialState` 的注释；新单测 `pbr_look_tints_the_hovered_element_and_keeps_its_translucency` |
| `docs/adr/0032-...md`、`CONTEXT.md`、`CHANGELOG.md` | 修订新增第 8 条（第 4、5、7 条的字句跟着改）、词条、界面变化 |
| `docs/evidence/2026-11-18-selection-outline.md` 与同名目录 | 第 6 节与 5 张图 |

**验证**

cargo 都经本机的编译锁跑。当时工作区里另有两个与本任务无关、未提交的改动文件（`crates/plant-ui-app/src/data.rs`、`crates/plant-ui-app/src/model_update_api.rs`），测试是带着它们跑的。

```
cargo test -p plant-ui -p plant-ui-view3d -p rs-plant → plant-ui 267 + 1 + 1、plant-ui-view3d 135（多 2 条）、rs-plant 279，全过
cargo clippy -p plant-ui -p plant-ui-view3d -p rs-plant --all-targets → exit 0，这次改动的行上没有告警
cargo check -p plant-ui -p rs-plant --all-targets → exit 0
cargo build -p rs-plant → exit 0
```

演示厂实拍（PBR 外观；桌面走 Vulkan，网页端是用最终代码重编的 wasm 包 + 无头 Chrome，没有覆盖 `web/public/pkg`）。数与图见 evidence 第 6 节：

| 项 | 桌面（Vulkan） | 网页（WebGL2） |
|---|---|---|
| 指针移开后与常态相比 | 0 像素不同 | 0 |
| 指着蓝色立柱 | 2248 像素变，柱侧面 `(57,134,171)` → `(216,164,45)` | 2059，`(57,133,172)` → `(216,163,46)` |
| 指着设备方箱（类型色与悬停橙同色） | 9948 像素变，背光的底面 `(102,79,38)` → `(161,122,38)` | 9728，`(106,79,18)` → `(178,133,18)` |
| 指着半透明的房间体 | 140755 像素变，里面的件照样看得见 | 137381 |
| 悬停时的描边像素 | 0 | 0 |
| 指着已选中的件 | 0 像素变 | 0 |
| 选中一件、指着另一件 | 选中件的填色与描边不变，被指的件换色 | 同 |

网页端控制台没有着色器 / WebGL 报错（只有连不上后端的 404，演示厂不连库），各段最长帧间隔 17 ms。

**没验到的**

- DX12 下的 PBR 外观：主场景本来就画不出来（「已定的四项」第 3 条），没法看。
- E3D 外观的桌面端没有重拍：它的悬停画法没动（只是配色表的一个方法改了名），单测照过；网页端补拍了四张，与加悬停色之前的包逐像素相同（evidence 6.4）。
- 回退线的悬停、`web` 配色下的 PBR 悬停色：只有单测。
- 真实模型、带界面的浏览器里人手操作：都没有，只在演示厂上看过。
- 共面处指到谁不稳定（「风险与已知限制」第 9 条）：已定这一轮不改拾取，理由在那一条里。
