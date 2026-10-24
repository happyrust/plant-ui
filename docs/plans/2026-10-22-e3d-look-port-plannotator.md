# E3D 显示效果（SGL DX11 口径）移植 plant-ui 开发计划

## 当前基线

仓库：`D:/work/plant-code/old/plant3d-web`（来源，一字不改）+ `D:/work/plant-code/old/plant-ui`（落点）

### 来源侧：plant3d-web 已经做成、这次照搬口径的东西

全部在 `src/viewer/e3dLook/`，设计与逐轮验证数据见 `docs/rendering/e3d-sgl-look-prototype.md`（§1–§5.12）。它复刻的是 AVEVA E3D 3.1 的 `sglDx11.dll`，数值全部来自逆向，**本计划不重新推导任何常数，只搬**。

| 文件 | 内容 | 要搬的核心 |
|---|---|---|
| `sglLookMaterial.ts` | 前向着色 `rgb = c·(Ka + Kd·max(N·L,0)) + Ks·pow(N·H,Kse)·N·L + Kr·env(reflect(−V,N))`，眼空间头灯 (0,0,1)，无 gamma / tonemap，颜色按 **sRGB 字节直算** | 公式、`SGL_DEFAULT_LIGHT` {0.5, 0.8, 0.3, 0.35, 64}、`SGL_E3D31_VIEW_DEFAULT_LIGHT` {0.7, 0, 0, 0.8, 0}、三套策略 default / flat70 / unlit |
| `sglEnvCube.ts` + `public/texture/e3d/sgl31-envcube/{px,nx,py,ny,pz,nz}.png` | sglDx11 内嵌的灰度环境立方体贴图（128²×6，共 27 KB），双线性、无 mip、不做 sRGB 解码、不翻 Y | 六张 PNG 原样复制；采样换法 Z-up 世界 `(R.x, R.z, −R.y)` |
| `sglLookPipeline.ts` | 后处理链：法线/深度 → HBAO → 双边模糊 → HLR 边线 → 去杂 → 合成 → 抗锯齿；`E3D31_HBAO`（R 392.7327 mm、8 向、4 步、30°、0.2、1.25、模糊半径 12、锐度 16/(深度范围/2)）、HLR 阈值（50 mm、0.6、0.9999、h = 1000 mm·像素步长）、去杂（0 / 0.3 / 权重 1/9）、背景哨兵、渐变 t 0.233→0.9、合成 `alpha = max(1 − hlr, colour.a)` | 六段 GLSL 逐句翻成 WGSL；参与标记编码进法线长度（1.0 / 0.75 / 0.5 / 0.25） |
| `pdmsColourTable.ts` | PDMS / E3D 3.1 颜色表 365 项 + 61 色字典 + 出厂图形颜色（元素 lightgrey #bdbdbd、CE yellow、highlight white、aids blue、背景 grey #828282） | 生成 Rust 常量表 |
| `sglLookPresets.ts` | 两套预设 `e3d31-factory`（边线 / 伪阴影 / 渐变 / 4× AA 全开）与 `sgl-machine`（全关、C++ 默认光照、纯灰背景） | 原样 |
| `useRenderLookStore.ts` + `RenderModeSettingsPanel.vue` | 模式 PBR / SGL、预设、四个子开关（边线 / 伪阴影 / 背景渐变 / 抗锯齿）、「恢复预设出厂位」、localStorage + URL 覆盖 | 状态模型照搬，落到 `settings.ron` |
| `SelectionManager.primarySelectionColor` | E3D 外观下 **最近一次选进来的那批 = CE yellow**，更早的选中 = highlight white，整体染色、**没有描边** | 规则照搬 |

### 落点侧：plant-ui 三维视口现状（`crates/plant-ui-view3d/src/lib.rs`）

- Bevy 0.16（vendor 的 gitee fork）离屏视口：主相机（`ClearColorConfig::Custom(Color::NONE)`、`Tonemapping::None`）写一张 `Bgra8UnormSrgb` 图（`viewport_image`），egui 先画主题渐变（`workbench/view3d.rs::show` 的 `gradient(rect, top, bottom)`）再把这张图当纹理叠上去；代码里还留着一台背景相机（`order −1`、`BACKGROUND_LAYER` 渐变面片），但它 `is_active: false`，从不出图——**可见的视口背景全是 egui 画的**；主相机没挂 `Msaa` 组件，走 Bevy 默认 4×。
- 模型材质：`StandardMaterial` PBR（`MODEL_METALLIC` 0.1 / `MODEL_ROUGHNESS` 0.7，双面不剔），按 `type_color()` 一类一枚缓存（`load_models` 的 `material_cache`）；光照是 `DirectionalLight` 头灯 1600 lx + `AmbientLight` 220。
- 状态材质：`material_state()` 三态裁决 X-Ray > Selected > Base，`apply_selection` 按 `ModelMesh { base, highlight }` 两枚句柄换材质；选中色 `SELECT_COLOR` 橙红整体换色、无描边。
- 辅助几何（E3D 口径里的 handles / aids）：地面网格、原点三轴、尺寸标注 `LineList`、无效 TUBI 虚线（自定义 `InvalidTubiLineMaterial`，`prepass_enabled: false`）、房间体半透明 + 断口（`room_clip.rs`）、虚拟孔洞切削体与预览网格（`virtual_hole.rs`）。
- 没有 prepass、没有后处理节点；背景色由主题经 `Cmd::SetViewportBackground` 下发给 Bevy（`tokens.viewport_top/bottom/grid`——顶 / 底两色只喂给那台不出图的背景相机，`grid` 给地面网格），egui 侧直接拿 `t.viewport_top/bottom` 画底色。
- 设置：`crates/plant-ui/src/settings.rs` 的 `Settings`（`#[serde(default)]`，宿主 `plant-ui-app/src/settings_store.rs` 落 `settings.ron`，浏览器端 `persist_settings` 为空实现）。
- 单位：`MODEL_SCALE = 0.01`（1 世界单位 = 100 mm），装载变换把 PDMS Z-up 转成世界 Y-up：世界 = (x, z, −y)。
- 两端：原生桌面（`plant-ui-app` 二进制，wgpu Vulkan / DX12）与浏览器（`scripts/build-wasm.ps1` → `web/public/pkg`，wgpu WebGL2 后端，`webgl2` feature）共用同一份 view3d 代码。

方向（用户拍板，2026-10-22）：**移植后 plant-ui 三维视口默认就是 E3D 外观（出厂 E3D 3.1 预设）**，设置里可切回现在的 PBR；E3D 外观下元素默认 **一色 lightgrey**（按类型配色做成可选）；抗锯齿默认 **整帧 2×2 超采样**；**先把 PC（原生）端全链路跑通并验收，再做 Web（wasm / WebGL2）端**。

## 决策摘要

| 决策 | 结论 |
|---|---|
| 交付顺序 | 阶段一至三只在 **PC** 上开发与验收（不跑 wasm 构建），阶段四专门做 **Web**；但从第一行起就按两端共用写——不用 compute、不读多重采样纹理、不用 `std::env` / 文件系统、uniform 16 字节对齐，Web 阶段只做能力探测、降级与浏览器验收，不改口径 |
| 默认外观 | E3D 外观 + `e3d31-factory` 预设；`Settings.look.mode` 出厂 `E3d`，老 `settings.ron` 没这一格也按出厂值（于是老用户升级即切换，设置窗一键回 PBR） |
| 落点 | 全部进 `plant-ui-view3d`，新模块 `src/e3d_look/`（`colour_table.rs` / `presets.rs` / `material.rs` + `sgl_look.wgsl` / `env_cube.rs` / `post/`）；对外只多一个入口 `View3d::set_look(ViewportLook)`；契约层 `plant-ui` 加 `ViewportLook` 类型与 `Cmd::SetViewportLook`；`plant-ui-app` 只接线 |
| 实体着色 | 自定义 Bevy `Material`（`SglLookMaterial`），WGSL 逐句翻 `sglLookMaterial.ts`；颜色 uniform 存 sRGB 字节/255，公式在字节域算，**输出前做 sRGB→linear** 抵消 `Bgra8UnormSrgb` 目标的编码（否则 lightgrey 会画成 130 而不是 189，plant3d-web §5.5 栽过同一个坑）；双面：`cull_mode: None` + 背面翻法线 |
| 环境立方体贴图 | 六张 PNG `include_bytes!` 嵌进 crate（两端一致、不进资产目录、不走网络），启动时解码堆成 6 层 → cube view，线性采样、无 mip、`is_srgb = false`；plant-ui 世界 Y-up = PDMS (x, z, −y) **恰好就是** sglDx11 的采样换法，所以世界空间反射向量直接采样，不再旋转 |
| 元素颜色 | 新增 PDMS 颜色表常量；`ColourScheme::E3dFactory`（全 lightgrey #bdbdbd，默认）/ `ColourScheme::ByType`（现有 `type_color()`，可选）；两种都走 SGL 着色 |
| 选中 | E3D 外观下 CE yellow / 其它选中 white、无描边；CE = 宿主 `Selection::primary()`（最后一次点中的那一个，就是属性面板跟随的那一个），经 `View3d::set_selection(refnos, primary)` 一并下发——view3d 入口加一个形参，`plant-ui` 的 `Cmd` 契约不改；宿主本来就有这个概念，比在 view3d 里按前后两集之差推断可靠；X-Ray 照旧（它是辅助态，E3D 没有对应物） |
| 几何通道 | 用 Bevy 自带 `DepthPrepass` + `NormalPrepass`（E3D 外观时挂到主相机）；**参与标记写进法线纹理的 alpha**（`Rgb10a2Unorm` 的 2 bit 恰好装 edges / shadows 两位码）靠 `Material::prepass_fragment_shader` 自定义；面法线在后处理里由深度重建（等价 sglDx11 MRT1 的位置导数叉乘）；`PbrPlugin { prepass_enabled: false }` 让 `StandardMaterial` 的辅助几何不进 prepass = 不参与边线 / 伪阴影（与 E3D 默认 `EnhancedEdgesHandles` / `PseudoShadowsHandles` OFF 同义） |
| 后处理 | 一个 `ViewNode`（`SglLookNode`）插进 `Core3d` 图 `EndMainPass → Tonemapping` 之间，经 `ViewTarget::post_process_write()` 读写颜色；中间纹理 ND / AO / 模糊×2 / HLR / 去杂由 `RenderSystems::PrepareResources` 里的 prepare 系统经 `TextureCache` 按视图分配成组件；**全部片元着色器**，不碰 compute（WebGL2 没有） |
| 背景 | **不动 Bevy 背景相机**（它本来就 `is_active: false`，视口背景一直是 egui 在纹理底下画的渐变）：契约层 `View3dVm.background: Option<(Color32, Color32)>`，`None` = 主题 tokens（PBR 外观），`Some` = 外观色；宿主每帧按 `view3d.look()` 经 `look_background()` 填值，颜色来自 `e3d_look::presets::e3d_background(gradient)`：渐变开 = 顶 `mix(#828282, 白, 0.233)` ≈ #9f9f9f → 底 `mix(…, 0.9)` ≈ #f2f2f2，渐变关 = 纯 #828282。合成输出 `rgb = colour·hlr·ao`、`a = max(colour.a, 1 − hlr)`（同 sglDx11 Combined），离屏图保持透明底，egui 的 alpha 叠加就把边线压到背景上，不另画背景 |
| 抗锯齿 | 默认 SSAA 2×2：离屏目标按 2× 分配、egui 线性采样缩回视口尺寸（2× 整数倍时线性采样 = 4 像素盒式平均，正是 sglDx11 HLR 的 Σ/N 与 MSAA 解析的结构）；AO / 模糊按半分辨率跑（= 屏幕分辨率，等价 `halfRes`）；E3D 外观下主相机 `Msaa::Off`（WebGL2 读不了多重采样 prepass 纹理，PC 上也这么做，两端统一）；备选 FXAA（vendor 里已有 `bevy_anti_aliasing`，加 feature 即可，不新增下载）；PBR 外观维持今天的 MSAA 4× |
| 单位 | 所有 mm 口径的常数经 `MODEL_SCALE` 换成世界单位：HBAO R 3.927327、HLR 深度阈值 0.5、`gradientStep` 10；写成 `mm * MODEL_SCALE`，不手填数字；深度范围自动锐度用 `View3d.bounds` 的并集在眼空间取 [近, 远] |
| 持久化 | `Settings.look: ViewportLook { mode, preset, hlr, ao, gradient, aa, colour_scheme }`，随 `settings.ron` 按机器落盘（与 `theme` 同一条房规，不进项目 bundle）；浏览器端只认 `config_json` / URL 覆盖，不落盘（阶段四） |
| 不动 | plant3d-web、rs-plant3-d、gen-model；`room_clip` / `virtual_hole` 的几何逻辑；模型数据与可见性台账 |

### 几条需要展开的理由

**为什么先 PC 后 Web。** 两端同一份代码，差别只在 wgpu 后端能力与构建链；PC 上编译—运行—截图一分钟一轮，wasm 一轮是释放构建 + wasm-bindgen + 起站点，还没有像素探针。所以正确性（公式、常数、选中、设置）全部在 PC 上钉死，Web 阶段只回答「这张卡 / 这个浏览器能不能跑」和「多快」。代价是一条纪律：PC 阶段写的每一行都得是 WebGL2 能跑的写法（见「交付顺序」那行），否则阶段四变成返工。

**为什么用 Bevy prepass 而不是自己再画一趟法线/深度。** plant3d-web 是 `scene.overrideMaterial` 再画一遍场景；Bevy 里等价物就是 prepass，材质系统已经替每种 `Material` 生成 prepass 管线，`ViewPrepassTextures` 直接把深度与法线纹理交给后处理节点。自己起第二趟相机要复制可见性 / 变换同步，划不来。代价是两处迁就：法线纹理是 `Rgb10a2Unorm`，法线精度 10 bit（判边阈值 0.6 够用，面法线反正是深度重建的）；半透明材质（`AlphaMode::Blend`）不进 prepass，所以 E3D 的 `EnhancedEdgesTranslucent = ON` 这一条**做不到**，半透明几何在边线 / 伪阴影里当背景——记为已知偏差（见「非目标」）。

**为什么 SSAA 而不是 MSAA。** Bevy 开 MSAA 时 prepass 纹理也是多重采样的，片元着色器要用 `texture_depth_multisampled_2d`，WebGL2 没有；而 plant3d-web 那条「颜色 MSAA + 法线/深度 2×2 超采样」的路在 Bevy 里要把两张纹理做成两种分辨率，prepass 不支持。整帧 2× 一刀切，两端一套代码、结构上与 E3D 的 4 采样一致；填充率 4 倍由 AO 半分辨率与 `aa` 子开关兜底。

**为什么材质要「可互换」而不是 PBR 外观直接删掉。** 用户拍板的是「默认 E3D、可切回」，所以 `StandardMaterial` 路径保留。两种材质是两种组件类型（`MeshMaterial3d<StandardMaterial>` / `MeshMaterial3d<SglLookMaterial>`），切外观是一次全场换组件，不是改 uniform。为此把「这一刻该用哪枚材质」收敛到一个资源 `LookMaterials`（按外观 × 颜色键 × 状态 Base / Ce / Highlight / XRay 缓存句柄），`apply_selection`、`room_clip`（补出来的兄弟段抄材质）、`virtual_hole::apply_hole_materials` 三处都从它取，不再各自持 `Handle<StandardMaterial>`。

## 必须先于动手

1. **录来源侧的基准。** plant3d-web `npm run dev` 开 `/e3d-look-demo.html`，按 `?view=iso|front|top` × `preset=factory|machine` × `aa=0|4` 各截一张，连同页面状态行（深度范围、锐度、AA 档位）存到 `docs/evidence/2026-10-22-e3d-look/source/`；再用主界面 `?dtx_look=sgl&show_refno=24381_145018` 截 BRAN 24381_145018 的 Iso 3 一张。plant-ui 搬完逐张对。
2. **PC 上跑 prepass spike。** 已写好：`crates/plant-ui-view3d/examples/sgl_prepass_spike.rs`（+ 同名目录下 `material.wgsl` / `post.wgsl` / `index.html`），`cargo run -p plant-ui-view3d --example sgl_prepass_spike`。它验两件事：`prepass_fragment_shader` 能写 alpha（模式 3 里四种物件四种色）、`ViewPrepassTextures` 在 `EndMainPass` 之后还活着（模式 1 / 2 / 4 / 5 有图）；顺带钉 sRGB 字节直写（模式 0 左前平光盒子读 (189,189,189)）。任一不成立，先改决策再写正文。它在 WebGL2 上跑不跑得通是阶段四第一件事，不在这里等。**已跑（2026-10-22，AMD RX 590 / Vulkan，模式由环境变量 `SPIKE_MODE` 选）：三条都成立——模式 3 四种标记四种色、模式 4 面法线有图、模式 0 平光盒子读 (189,189,189)、背景 (130,130,130)；截图在 `docs/evidence/2026-10-22-e3d-look/spike/sgl-spike-{0,3,4}.png`。**

## 阶段一（PC）：材质、颜色、环境贴图、选中（不含后处理）

### 目标

E3D 外观能开能关、两套预设能切，`sgl-machine`（全关）预设与 plant3d-web 的「本机真机」同视角同像素；出厂预设此时只差边线 / 伪阴影两样。只在原生二进制上验。

### 实现

- `crates/plant-ui/src/lib.rs`：新增
  ```rust
  pub enum LookMode { Pbr, E3d }
  pub enum LookPreset { E3d31Factory, SglMachine }
  pub enum ColourScheme { E3dFactory, ByType }
  pub enum AntiAlias { Off, Ssaa2x2, Fxaa }
  pub struct ViewportLook { mode, preset, hlr: bool, ao: bool, gradient: bool, aa: AntiAlias, colour_scheme }
  ```
  `ViewportLook::for_preset(preset)` 给出该预设的出厂位；`Cmd::SetViewportLook(ViewportLook)` 紧挨 `SetViewportBackground`，注释同款：「不认识设置，只认结果」。
- `crates/plant-ui-view3d/src/e3d_look/colour_table.rs`：从 `pdmsColourTable.ts` 生成 `PDMS_COLOUR_TABLE: [(u16, &str, u32); 365]`、`PDMS_COLOUR_DICTIONARY`、`pdms_colour_hex(&str | u16)`（接受 `pdms:` 前缀与数字串）、`E3D_GRAPHICS_COLOUR_DEFAULTS`、`E3D_DEFAULT_ELEMENT_COLOUR = 0xbdbdbd`、`E3D_BACKGROUND_GREY = 0x828282`。文件头写明来源与生成日期，「勿手改」。
- `presets.rs`：`SglLight { ambient, diffuse, specular, reflection, specular_exponent, lamp_dir_eye }`，`SGL_DEFAULT_LIGHT` / `SGL_E3D31_VIEW_DEFAULT_LIGHT` / 三套策略，`preset_light(LookPreset)` / `e3d_background(gradient)`（grey、渐变端色白）。
- `material.rs` + `sgl_look.wgsl`：`SglLookMaterial`（`AsBindGroup`），从 spike 的 `SpikeMaterial` 长出来：
  - uniform：`colour: vec4`（sRGB 字节/255）、`ka kd ks kr kse`、`lamp_dir_eye: vec3`、`opacity`、`use_env`、`sky_lum 0.75` / `ground_lum 0.25`（无贴图兜底）、`effect_flags: u32`（阶段二用，先恒 3 = 边线 + 伪阴影），整体按 16 字节对齐（WebGL2）；
  - `#[texture(1, dimension = "cube")] #[sampler(2)] env: Option<Handle<Image>>`；
  - 顶点用 Bevy 默认网格顶点着色器；片元按 §2 公式，`V = normalize(−view_pos)`，`L = normalize(lamp_dir_eye)`，`spec = (ndl ≥ 0 && ndh ≥ 0) ? pow(max(ndh, 1e-6), kse) · ndl : 0`，反射向量换到世界空间直接 `textureSample(env, R)`；最后 `out = vec4(srgb_to_linear(rgb), a·opacity)`；
  - `alpha_mode`：`opacity < 1` → `Blend` 否则 `Opaque`；`specialize` 里 `cull_mode = None`，片元 `!front_facing` 翻 N；
  - `prepass_fragment_shader()` 指到同一文件的 `PREPASS_PIPELINE` 半边，输出 `normal = vec4(n·0.5 + 0.5, f32(effect_flags) / 3.0)`（阶段二接管）。
- `env_cube.rs`：`include_bytes!` 六张 PNG（放 `crates/plant-ui-view3d/src/e3d_look/sgl31-envcube/`，顺序 +X −X +Y −Y +Z −Z = wgpu 立方体面序），`Image::from_buffer` 解码 → 纵向堆成一张 128×768 → `reinterpret_stacked_2d_as_array(6)` → `texture_view_descriptor.dimension = Cube`，`ImageSampler` 线性；`Startup` 里插进 `Assets<Image>`，句柄存 `SglEnvCube(Handle<Image>)` 资源。
- `LookMaterials` 资源（`crates/plant-ui-view3d/src/look_materials.rs`，两种外观共用所以不放 `e3d_look/`）：`resolve(look, &ModelMesh, MaterialState, &mut MaterialAssets) -> ModelMaterial`，按**解析后**的颜色 × 画法 × 状态缓存（E3D 出厂配色下所有类型一色 = 一枚），外观一变整表清空。`ModelMaterial` 是 `enum { Pbr(Handle<StandardMaterial>), Sgl(Handle<SglLookMaterial>) }`，`attach(entity, Option<Mut<MeshMaterial3d<StandardMaterial>>>, Option<Mut<MeshMaterial3d<SglLookMaterial>>>, &mut Commands)` 同类就地改句柄、异类摘旧挂新。`ModelMesh` 改存 `colour: [u8; 4]`（类型色字节）+ `line: bool`（无光照线），去掉 `base` / `highlight` 两枚句柄；`InvalidTubiLine` 不动（它永远是辅助几何）。
- `load_models`：只记类型色字节，**不挂材质**；`apply_materials` 见 `Added<ModelMesh>` 同帧补上。配色方案在 `LookMaterials` 里解：`E3dFactory` 一律 `E3D_DEFAULT_ELEMENT_COLOUR`（alpha 跟元素自己），`ByType` 用类型色；PBR 外观始终类型色（= 改动前）。
- `apply_selection` → 改名 `apply_materials`：按 `material_state(selected, primary, xray, refno, owner)` 四态（XRay > Ce > Highlight > Base；PBR 外观下 Ce 与 Highlight 都落到今天的橙红）取 `LookMaterials` 句柄并 `attach`；触发 = `selection_dirty || material_dirty || Added<ModelMesh>`。`View3d.primary: Option<RefU64>` 直接来自宿主（`set_selection(refnos, primary)`，不在集内按没有算），不再推断。
- `room_clip.rs`：补出来的兄弟段只带 `ModelMesh` 出生（`Tubes` 查询不再要材质组件），同帧由 `apply_materials` 补材质；断口 `SectionMaterial` 继续 `StandardMaterial`（辅助几何）；`virtual_hole.rs::apply_hole_materials` 同一张表、同一套裁决，孔洞 X-Ray 叠最上。
- `set_look(look)`：进 `ViewCommand::Look(look)`；`apply_commands` 里：① 记下 `view.look`，变了才置 `material_dirty`（全场换组件由 `apply_materials` 完成）；② 背景不在 Bevy 侧做——宿主每帧读 `view3d.look()`，经 `look_background()` 填 `View3dVm.background`，egui 画底色（`set_background` 不用改、不需要 `theme_bg`）；③ 阶段二起再挂 / 摘 prepass 组件与 `Msaa`。
- 光照：E3D 外观下 `SglLookMaterial` 不读 Bevy 光源，`Headlight` / `AmbientLight` 留给 PBR 外观，不动。
- 设置与宿主：`Settings.look: ViewportLook`（`#[serde(default)]`，出厂 `E3d` + `E3d31Factory` + 全开 + `Ssaa2x2` + `E3dFactory`）；设置窗 `settings::show` 加「渲染外观」一节：外观（web PBR / E3D 外观）、预设（出厂 E3D 3.1 / 本机真机）、元素配色（E3D 出厂 / 按类型）、四个子开关（边线 / 伪阴影 / 背景渐变 / 抗锯齿，文案照抄 `RENDER_LOOK_EFFECT_TOGGLES`；边线 / 伪阴影在阶段二前置灰并注「待实现」），「恢复预设出厂位」按钮；换预设时四个子开关回到该预设出厂位（同 `setPreset`）。宿主 `main.rs` 在 `persist_settings` 旁比较 `sent_look != saved.look` 就发 `Cmd::SetViewportLook`，与 `sent_viewport_bg` 同一节奏；两处 `Cmd` 分发（`main.rs:639` 与 `:4363` 一带）各加一臂。
- 调试入口：沿 `--gallery` 的先例加 `--look-demo`：把 `demo/buildDemoPlant.ts` 的演示厂（几根圆柱 / 盒子 / 半透明房间盒 / 一个 aids 蓝球）直接在 view3d 里造出来，不要后端也能做像素验收；只在 debug 构建编进去（`e3d_look/demo.rs`，宿主见 flag 插 `LookDemo` 资源）。spike 的 example 留到阶段五收尾时删。
- 落盘字面：`LookPreset` / `ColourScheme` / `AntiAlias` 用 `snake_case`（`sgl_machine` / `by_type` / `ssaa2x2`）而不是 plant3d-web 的连字符——`settings.ron` 是 RON，枚举字面是标识符，连字符写得出读不回来；阶段四的 URL 参数解析自己认两种写法。

**进度（2026-10-23）**：以上全部落地；`cargo test -p plant-ui -p plant-ui-view3d` 与 `rs-plant` 的 `settings_store` 全绿。`--look-demo` 三张截图在 `docs/evidence/2026-10-22-e3d-look/phase1/`（E3D 出厂一色 / E3D 按类型 / PBR）。下面验收里的像素探针数值与同视角对照等来源截图到位再做。

### 验收（PC）

- `cargo test -p plant-ui -p plant-ui-view3d`：颜色表钉值（1 grey #828282、271 与 310 lightgrey #bdbdbd、305 black、`pdms:271` / `pdms:lightgrey` / `pdms:4` 都解得出、CSS 名 `gray` 解不出）；两套光照常量与 TS 一字不差；`ViewportLook::for_preset` 两套出厂位；`material_state` 四态与 `set_selection(refnos, primary)`（主选中不在集内按没有算、只换主选中也翻脏标）；`LookMaterials` 四例（PBR 同色同句柄与橙红选中、E3D lightgrey / CE 黄 / highlight 白 / 半透明保留 / X-Ray 不打光、线在两种外观下都是无光照、换预设整表重建）；`set_look` 后全部 `ModelMesh` 的材质组件类型互换且选中态保持；背景色按 look / 主题往返恢复；`mm * MODEL_SCALE` 换算钉 3.927327 / 0.5 / 10。
- 像素探针（`--look-demo`，PrintWindow 抓窗采样，见 `design/DESIGN-SYSTEM.md` §8）：`sgl-machine` + 临时 Ka = 1 时 lightgrey 盒子正面 (189,189,189)、背景 (130,130,130)；出厂预设（阶段一还没有边线 / AO，只验颜色）未选中 ≈ (220,220,220)、CE (255,255,79)、highlight (249,249,249)，容差 ±3——这几个数就是 plant3d-web §5.5 的 harness 结果。
- 与来源同视角：`sgl-machine` 预设下 BRAN 24381_145018 的 Iso 3 与「必须先于动手」第 1 条的截图并排，背景、管子横截面暗 / 亮比（≈ 0.38–0.41）一致。
- 切回 PBR 外观：画面与改动前逐像素一致（`cargo test -p plant-ui` 现有视口测试全过，`--gallery` 截图不变）。

## 阶段二（PC）：几何通道 + 边线（含去杂）+ 伪阴影

### 目标

出厂预设的边线与伪阴影落地，`hlr` / `ao` 两个子开关即时生效；PBR 外观下这条链一行不跑。

### 实现

- 插件：`View3dPlugin::build` 加 `MaterialPlugin::<SglLookMaterial> { prepass_enabled: true, shadows_enabled: false }`；`plant-ui-app/src/main.rs:372` 的 `DefaultPlugins` 加 `.set(PbrPlugin { prepass_enabled: false, ..default() })`。
- 相机：`sync_camera_look`（PostUpdate，变换传播之后）每帧按外观挂 / 摘 `DepthPrepass` + `NormalPrepass`，`Msaa` 在 `Off`（E3D）与 `Sample4`（PBR，= 今天的默认；`Camera3d` 要求这个组件，摘不掉）之间切；`SglLookPost`（参数 = uniform 本身）**总在相机上**，PBR 或两样效果都关时 `ExtractComponent` 返 `None`，渲染世界里没有它、节点不跑——摘主世界组件会在渲染世界留下旧拷贝（`extract_components` 只对还在的实体做插 / 删）。
- `e3d_look/post/mod.rs`（从 spike 的 `SpikePostPlugin` / `SpikeNode` 长出来）：`SglLookPostPlugin`——`ExtractComponentPlugin<SglLookPost>`、`UniformComponentPlugin<SglLookPost>`；`RenderSystems::PrepareResources` 里的 prepare 系统经 `TextureCache` 给每个视图分配中间纹理组件；`SglLookNode: ViewNode`，`ViewQuery = (&ViewTarget, &ViewPrepassTextures, &ViewUniformOffset, &SglLookTextures, &DynamicUniformIndex<SglLookPost>, &SglLookPost)`，`add_render_graph_node::<ViewNodeRunner<SglLookNode>>(Core3d, SglLookLabel)` 并接边 `EndMainPass → SglLookLabel → Tonemapping`。节点内按顺序跑六个全屏通道，每个一条 `CachedRenderPipelineId`：
  1. **ND**（`Rgba16Float`，全分辨率）：就是 spike `post.wgsl` 的 `SPIKE_ND` 半边——读 prepass 深度（reverse-Z，背景 = 0）与法线纹理 alpha，`view_from_clip` 重建眼空间位置，面法线 = 邻像素差分叉乘（左右 / 上下各取深度更近的一侧）；输出 `rgb = n · flag_scale(alpha)`（1.0 / 0.75 / 0.5 / 0.25）、`a = 线性深度`（世界单位，半浮点上限 65504 单位 = 6.5 km，够）；背景写 `a = 0`。下游 `sglIsBackground(d) := d <= 0.0`，替代 1e18 哨兵。
  2. **HBAO**（`R8Unorm`；阶段二全分辨率 = E3D 的 `halfRes` 关，阶段三上 SSAA 后改半分辨率 = 屏幕分辨率）：`AO_FRAGMENT` 逐句翻，`uFocalLen` / `uOrthoOffset` / `uIsPerspective` 从 `view.clip_from_view` 取（`[0][0]` / `[1][1]`、`[3][0]` / `[3][1]`、`[2][3] != 0`）；采样方向在眼空间（y 向上），走像素时 y 取反。
  3. **双边模糊 X / Y**（两张 `R8Unorm`）：`BLUR_FRAGMENT` 逐句翻；锐度每帧 CPU 侧按 `blur_sharpness_for_depth_range(range)` = 16 / (range / 2) 算（`View3d.bounds` 全集 8 角点过 `view` 矩阵取 [近, 远]，近端截到 `near`；算不出沿用上一帧，起始值 0.01 /mm = 1 /单位），走 uniform。
  4. **HLR**（`R8Unorm`，全分辨率；E3D 边线恒黑，一个通道够）：`HLR_FRAGMENT` 逐句翻——`edge_along(c, right, left, h.x) || edge_along(c, down, up, h.y)` 四条判据原样；像素坐标 y 向下，「下」= `px + (0, 1)`；h = 1000 mm × MODEL_SCALE × InvResolution。
  5. **去杂**（`R8Unorm`，全分辨率）：`DECLUTTER_FRAGMENT` 逐句翻，2×2 窗口 = 本像素、左、上、左上，权重 1/9，`t = saturate((sum − 0.0) / 0.3)`。单根斜线的窗口在 2 / 3 个边像素间交替，所以边线看着略有虚点——E3D 真机也如此，不是 bug。SSAA 下本通道与 HLR 都跑在 2× 分辨率上、合成后由 egui 缩回，所以 TS 里「每个样本先对 kx×ky 个子像素取平均」那一步**不需要**。
  6. **合成**：`post_process_write()` 的 source → destination：`rgb = col.rgb · hlr · ao`，`a = max(col.a, 1 − hlr)`；目标是 sRGB 纹理，乘法在字节域做（`srgb_to_linear(linear_to_srgb(col) · hlr · ao)`，同 sglDx11 直写 UNORM 目标）；背景不在这里画（视口背景是 egui 在纹理底下画的渐变，离屏图保持透明底，egui 的 alpha 叠加完成压暗）。`legacy`（两个子开关都关）时节点直接跳过，不做任何 copy。
- 参与标记：`SglLookMaterial.effect_flags` 恒 3；`StandardMaterial` 辅助几何不在 prepass 里 = 标记 0 且不写深度 → 等价 E3D handles 全关。
- 调试：`PLANT_UI_LOOK_DEBUG=nd|ao|hlr|declutter` 把对应中间纹理直出到视口（与 `PLANT_UI_WORLD_AXES` 同一类开关，不进设置；读环境变量的代码 `#[cfg(not(target_arch = "wasm32"))]`，wasm 上恒关）。**未做**：验收时用「单开一样效果的临时 settings.ron + 截图差分」代替就够了，需要时再补。
- 能力探测的钩子（阶段四填）：`SglLookCapabilities { float_targets: bool }` 资源由 `RenderDevice` 的 features / 格式能力算出，PC 上恒 true；节点和设置窗都读它。阶段二没留，阶段四连着降级一起做。

**进度（2026-10-23）**：除上述两条外全部落地；`settings.rs` 的 `POST_EFFECTS_AVAILABLE` 已翻 true，「边线 / 伪阴影」两格解灰。`cargo test -p plant-ui-view3d` 100 通过（含常量钉、深度范围、着色器文本钉）。`--look-demo` 截图在 `docs/evidence/2026-10-22-e3d-look/phase2/`：出厂预设全开 / 只开边线 / 只开伪阴影——设备盒三个面靠 1 px 黑边分开（阶段一里侧面和半透明房同灰分不出），管子穿过管子 / 柱梁交接处有软阴影（与阶段一同机位截图差分：变暗落在接触 / 重叠处，最大 67/255）；aids 蓝球无轮廓不接 AO，地面网格无轮廓。线像素（亮度 < 0.25）占比：边线开 0.14% / 关 0.02%（演示厂比来源 demo 稀疏，量级对得上）。已知偏差：辅助几何不进 prepass，挡在它前面时模型的边线会透过它画出来（E3D handles 全关的同款行为）；半透明几何不进 prepass，无轮廓。

### 验收（PC）

- `cargo test -p plant-ui-view3d`：常量钉（`E3D31_HBAO` 七个数、去杂 0 / 0.3 / 1/9、HLR 四阈值、`sglEffectFlagScale` 四档）；WGSL 源码钉（HLR 只看右 / 下邻居、背景判据 `<= 0.0`、合成 alpha 规则、ND 的标记解码 `round(length·4) − 1`）——照 `invalid_tubi_shader_uses_bevys_material_bind_group` 那种「钉着色器文本」的写法；PBR 外观下主相机没有 prepass 组件、节点不介入。
- 像素统计（`--look-demo`，与 plant3d-web `e3d-look-demo.html` 同口径）：出厂预设 HLR 开 / 关「线」像素（亮度 < 0.25）占比同量级（来源 0.64% / 0.00%）、纯黑占比（去杂开 0.06%、关 1.08%）；AO 开 / 关几何区平均亮度之差同向同量级（来源 0.538 → 0.513）；aids 蓝球默认无轮廓、不接收 AO；半透明房间盒**无轮廓**（这是本计划的已知偏差，验收写明）。
- 同视角对照：BRAN 24381_145018 出厂预设与来源截图并排——阀门 / 法兰 lightgrey + 1 px 黑边、管口与接触面发暗、背景 grey→白；整库级场景远处密线被去杂淡掉、不成黑团。

## 阶段三（PC）：抗锯齿、性能与 PC 端收口

### 实现

- SSAA：`View3d` 加 `render_scale: u32`（1 或 2）；`apply_resize` / `PreparedResize` 按 `size × render_scale` 建 `viewport_image`，`view.size` 仍是逻辑尺寸；egui 侧不动（线性采样缩回）；`pick` / `ndc_to_texture_uv` / 标签投影都是归一化坐标，不受影响；HBAO 半分辨率 = `size × render_scale / 2`。切 `aa` 子开关 = 一次 resize（走现有 `ViewportResizeSync` 的「新图渲染完再换」纪律，不闪）。
- FXAA 备选：workspace `bevy` features 加 `bevy_anti_aliasing`（vendor 里已有 0.16.0-dev，不新增下载），`aa == Fxaa` 时主相机挂 `Fxaa` 组件、`render_scale = 1`。
- `legacy` 与 `aa` 的关系照 `sglHlrSamplesFor`：PBR 外观或两个效果全关时不改 `render_scale`（SSAA 只服务 E3D 外观）。
- 性能门槛（PC）：出厂预设（SSAA 2×2 + HLR + 去杂 + AO）1200×700 视口，独显 ≥ 60 fps、核显 ≥ 30 fps；用 Bevy `FrameTimeDiagnosticsPlugin` 临时打日志量，数字写进 evidence。显存：ND 27 MB + HLR / 去杂各 13 MB + AO / 模糊各 1.7 MB ≈ 60 MB，写进文档。
- PC 端收口：设置窗四个子开关全部可用；`CHANGELOG.md` 写入「三维视口默认改为 E3D 外观」；首次启动日志一行「三维外观已切为 E3D 出厂口径，设置 → 渲染外观 可切回」。到这里 PC 端可以单独发一版（`scripts/package-release.ps1`）。

**进度（2026-10-23）**：全部落地，发版脚本没跑。实现与上文的出入：① `render_scale` 的推导只看 `mode` 与 `aa`（E3D + Ssaa2x2 → 2，其余 1），两个效果全关也保留超采样——同 plant3d-web 里颜色 MSAA 不读 legacy；② `View3d` 多一个 `image_scale`（当前图按哪个倍数建的），`apply_resize` 见它与 `render_scale` 不等就按当前逻辑尺寸重建，不等防抖；③ AO / 模糊纹理按 `size / render_scale` 分配（`SglLookPost.toggles.w` 带倍数），HLR / 去杂的邻居改为 1 个屏幕像素 = `render_scale` 个子像素、h 按屏幕像素步长——同 sglDx11 MSAA 版每个采样点读 ±1 像素邻居，边线缩回后仍是 1 px 实线；④ FXAA 用 `bevy::anti_aliasing::fxaa::Fxaa::default()`，workspace `bevy` 加 feature `bevy_anti_aliasing`（lock / vendor 里本来就有）；⑤ 帧时间没用 `FrameTimeDiagnosticsPlugin`（工作区没开 `bevy_log`，它的输出看不到），改成 `--look-demo` 自带的 `report_frame_time`（debug 构建，每 3 s 往控制台打一行）。实测（AMD RX 590 / Vulkan，**dev 构建**，演示厂）：关 / FXAA 在 1476×889 稳 16.7 ms（垂直同步 60 fps）；SSAA 2×2 在 1575×773 × 2 = 3150×1546 多数时段 16.7 ms、间歇落到 32–34 ms（被垂直同步量化到 30 fps，实际帧时间在 17–20 ms 一带）——release 构建与整库场景、核显的数字还没量，验收表待补。截图在 `docs/evidence/2026-10-22-e3d-look/phase3/`（关 / FXAA / SSAA 三张 + `aa-compare-zoom.png` 同区域 4× 并排）；视口区纯黑像素占比 关 0.02% → FXAA / SSAA 0.00%，暗灰（8..120）占比 1.55% → 0.89% → 0.46%（边线被平均成更浅的覆盖灰）。注意：这三张是在③的「邻居 1 个屏幕像素」改动之前抓的（当时边线还是 1 个子像素宽，SSAA 图里偏淡），改动后测试全绿但当时主程序正被占用、没能重抓，下次跑到 PC 端时把 SSAA 两张补一遍。

**审核修正（2026-10-23 晚）**：对照来源与截图像素做了三处修正，取代上文阶段二第 1 / 5 / 6 条与「背景」决策里的对应说法。① ND 的差分侧改为「用该侧两点线性外推最能命中中心深度」的一侧（平面上的硬件深度在屏幕上是仿射的，同面外推误差为 0）：原来只比深度差大小，深度连续的凸 / 凹棱两侧都跨棱取样、得到两面法线的中间值，判据④漏判——`phase2/app-look-demo-e3d-factory-hlr-only.png` 里盒子竖棱 61 行缺 10 行、斜棱 57 行缺 6 行（右侧轮廓 54 行不缺），「斜线略有虚点」主要是这个而不是去杂。没有改成 prepass 导数法线：世界坐标几千单位时 f32 的位置导数在近景下噪声大。② 合成在字节域里把背景渐变画进离屏图，半透明先除回直通色再叠到背景上，输出不透明（`SglLookPost` 加 `bg_top` / `bg_bottom`，`toggles.z` 改为「合成生效」，E3D 外观两样效果都关也跑合成；PBR 外观仍不提取）：原来透明底交给 egui，而 vendor 的 bevy_egui 在线性空间里混、还对 Bevy 图再乘一次 alpha——同一张截图里画在背景像素上的左 / 上轮廓与相邻亮度之比 0.63–0.66，画在几何像素上的右 / 下轮廓是 0.28（E3D 字节域应为 0.26）。egui 那层 E3D 背景（`look_background()`）留着做管线就绪前的底。③ 超采样下去杂改在屏幕分辨率上，每个样本先对 s×s 个 HLR 子像素取平均（同来源 `hlrResolved`）；合成对每个屏幕像素的 s×s 个子像素在字节域取平均并写同一个值，egui 线性采样缩回后就是字节域结果（上面③里 HLR 邻居按屏幕像素取的改动保留）。`cargo test -p plant-ui-view3d` 103 通过；七个着色器变体用 naga 24 离线解析 + 校验通过。运行时截图还没重抓（主程序被占用），`phase2` / `phase3` 的图都是修正前的。

### 验收（PC）

- `cargo test`：`render_scale` 与 `aa` / `mode` 的推导表（PBR → 1、E3D + Ssaa2x2 → 2、E3D + Fxaa / Off → 1）；resize 测试在 `render_scale = 2` 下仍满足「替换图渲染完才换」。
- 像素统计：`aa` 开 / 关纯黑像素占比下降、覆盖率灰像素上升（来源 2.86% → 1.82%、8.0% → 10.0% 同向）；`shots/aa-compare-zoom.png` 同款放大对照一张。
- 帧时间达标表（至少一台独显 + 一台核显）。
- 真模型全流程：显示 / 选中 / 房间视图（X-Ray + 直段截断）/ 虚拟孔洞预览 / 尺寸标注，在 E3D 外观下都还对；切回 PBR 一切如旧。

## 阶段四（Web）：wasm / WebGL2 适配与浏览器验收

### 目标

PC 上已验收的三个阶段在 Chrome / Edge（WebGL2）里同口径跑起来；跑不起来的部分按能力降级、界面如实说，不崩、不黑屏。不改公式、常数与选中口径。

### 实现

1. **先跑 spike**：`cargo build -p plant-ui-view3d --example sgl_prepass_spike --target wasm32-unknown-unknown --release`（clang / zig 环境变量照 `scripts/build-wasm.ps1`）→ `wasm-bindgen --target web --out-dir target/sgl-spike-web/pkg --out-name sgl_prepass_spike …` → 把 `examples/sgl_prepass_spike/index.html` 拷进去起静态服务器。要看：控制台无着色器编译错误；`Rgba16Float` 当渲染目标成不成（模式 1 / 3 / 4 有图）；`texture_depth_2d` 的 `textureLoad` 在 GLSL ES 3.0 里能不能过（模式 5 有图）。这一步是阶段四的门：哪条不过，下面对应的降级就从「备案」变成「必做」。
2. **能力探测与降级**（填阶段二留的 `SglLookCapabilities`）：按 `RenderDevice` 的 features / `TextureFormat` 能力判 `Rgba16Float` 可否作颜色附件（WebGL2 对应 `EXT_color_buffer_half_float` / `EXT_color_buffer_float`）。不可：退路 A = ND 改 `Rgba8Unorm` × 2 张（法线 8 bit + 深度 24 bit 定点分三通道），退路 B = 边线 / 伪阴影两格置灰 + 日志一行。**默认只做 B**，A 只在目标机器确实缺扩展时做（现代 Chrome / Edge 桶装都有）。材质 / 颜色 / 背景 / 选中不受影响。
3. **抗锯齿与性能**：WebGL2 上 SSAA 2×2 是 4 倍填充率，先量再定：整库 7997 级场景达不到 24 fps 就把 Web 端的 `aa` 出厂位改 `Fxaa`（`ViewportLook::for_preset` 按 `cfg!(target_arch = "wasm32")` 给不同出厂位，设置文案注明）；`FrameTimeDiagnosticsPlugin` 的数字走 `web_sys::console`。
4. **设置来源**：浏览器端 `persist_settings` 是空实现，所以 `startup::browser_runtime_config` 的 `config_json` 加可选 `look` 段（字段同 `ViewportLook`，站点部署给默认），URL `?look=sgl|pbr&look_preset=factory|machine&look_hlr=0&look_ao=0&look_gradient=0&look_aa=0|fxaa&look_colours=e3d|type` 只覆盖本次（照 plant3d-web `applyQueryOverrides`）；设置窗里改了只活到刷新，窗里注一句。
5. **构建链**：`scripts/build-wasm.ps1` 不用改（环境贴图是 `include_bytes!`，没有新资产）；确认 `web/public/pkg` 体积增量（六张 PNG 27 KB + WGSL 文本）并记入 evidence。
6. **浏览器端的已知差异清单**写进文档：没有 `PLANT_UI_LOOK_DEBUG`（没有环境变量）；没有 `settings.ron`；像素探针改用 Chrome DevTools 截图采样。

**进度（2026-10-23 夜）**：第 1、2、4、5 条落地，第 3 条量了演示厂、整库场景没量，第 6 条写在这段。实测机：AMD RX 590 / Chrome ANGLE D3D11 / WebGL2，窗 1600×1000 @ 150%。

- **spike 的门**：`Rgba16Float` 当颜色附件过；`texture_depth_2d` 的 `textureLoad` **不过**（naga GLSL 后端直接 panic：“WGSL `textureLoad` from depth textures is not supported in GLSL”）。于是改成**深度不走硬件深度纹理**：prepass 把线性眼空间深度拆成 3 × 10 bit 定点写进法线纹理的 RGB（`Rgb10a2Unorm`，范围 65504 世界单位 = 6.5 km，步长 ≈ 0.006 mm），alpha 照旧是参与标记；ND 只读法线纹理，相机不再挂 `DepthPrepass`。透视下外推误差在 1/深度域里比（平面上那才是仿射的量），正交直接比深度。PC 与 Web 一套着色器，编 / 解码两头的范围常数有测试钉着。
- **能力探测**：`RenderAdapter::get_texture_format_features(Rgba16Float)` 含 `RENDER_ATTACHMENT` 才挂 prepass / 后处理 / 超采样，否则留 MSAA、设置窗「边线 / 伪阴影」两格灰掉并说原因、日志一行（退路 B）。**2026-10-24 模拟缺扩展走了一遍**（藏扩展 + 强制探测为否两轮，见 evidence §5）：抓到并修了「后处理管线在 `from_world` 里无条件排队 → ND 那条在创建时 panic」，之后我们这边不崩、两格灰掉、日志一行都成立；再往下是 vendor Bevy 的 `tonemapping pipeline` 对每个视图都按 Rgba16Float 排、真缺扩展的设备上 Bevy 本身就起不来——给 vendor `bevy_core_pipeline` 打了一处本地补丁（非 HDR 视图不排那条管线，`.cargo-checksum.json` 同步，重新 vendor 时要带上），之后藏扩展那一轮整条走通：不崩、两格灰掉、日志一行、60 fps。wgpu 在 WebGL2 上按 `EXT_color_buffer_float` 判半浮点可渲染，文案改成了这个扩展名。
- **设置来源**：`config.json` 可选 `look` 段（字段同 `ViewportLook`，可只给一部分）打底，地址栏 `?look=sgl|pbr&look_preset=factory|machine&look_hlr=0|1&look_ao=0|1&look_gradient=0|1&look_aa=0|fxaa|ssaa&look_colours=e3d|type` 压一层、只活到刷新；设置窗在浏览器端多一句「不落盘，只活到刷新」。`?look_demo=1` = 桌面的 `--look-demo`（debug 构建总带，release 要开 `look-demo` feature）。
- **主界面 wasm 跟跑出来的两个 WebGL2 坑**（不改就是黑视口 + 丢上下文）：① 离屏图原来带一整块零字节，Bevy 启动时把它上传，WebGL2 不收 BGRA 的 `texSubImage2D`（控制台 `invalid format`），超采样时还是几十 MB；改成 `data: None`，物理尺寸夹进 `max_texture_dimension_2d`。② 浏览器里 bevy_egui 的相机首帧还没算好，egui 按缺省 10000×10000 布局，绘制层要了一张 6154×6839 的渲染目标（超采样再翻倍 = 12308×13678），ANGLE 报 “Texture total allocation size is too large” 然后丢上下文；宿主按「视口不可能比窗口大」把超过窗口物理尺寸的请求挡掉（`viewport_fits_window`）。之后控制台一条 GL 错误都没有。
- **性能**：演示厂、视口 1463×767 物理像素：出厂 E3D（边线 + 伪阴影 + SSAA 2×2，离屏图 2926×1534）16.7 ms / 60 fps；FXAA 60 fps；PBR 60 fps（都顶在 vsync）。整库 7997 级场景要后端，没量；Web 端出厂 AA **暂留 SSAA 2×2**，站点要换在 `config.json` 写 `"look": {"aa": "fxaa"}` 即可，不用改代码。
- **构建链**：`scripts/build-wasm.ps1` 没改；验证用的 release + `look-demo` 构建 `rs-plant_bg.wasm` 95.4 MB（未压缩），仓里的 `web/public/pkg` 没有重发。
- **浏览器端的已知差异**：没有 `PLANT_UI_LOOK_DEBUG`；没有 `settings.ron`（外观走 config.json + 地址栏）；像素探针用 PrintWindow 抓 Chrome 窗（启动要带 `--disable-features=CalculateNativeWinOcclusion`，否则被遮着的窗 Chrome 不出帧）。
- **截图**：`docs/evidence/2026-10-22-e3d-look/phase4/web-app-look-demo-{e3d-factory,e3d-factory-zoom,pbr,e3d-effects-off,e3d-fxaa}.png`（依次是默认 / `?look=pbr` / `?look_hlr=0&look_ao=0` / `?look_aa=fxaa`）；`web-spike-mode4-face-normals.png` 是 spike 在 panic 前的模式 4。来源侧基准在演示厂这一级已补（`docs/evidence/2026-10-22-e3d-look/source/`，plant3d-web `e3d-look-demo.html` 走它的 Vite dev server，headless Chrome 拿图；背景顶 / 底 159 / 242 实测与我们的常数一致）；与 PC 同视角同模型的像素对照仍等来源侧连上后端。「刷新 / 关页不卡死」用 headless Chrome + CDP 连刷 3 次 + 关页走过（evidence §7）。

### 验收（Web）

- spike 六个模式在 Chrome 里都有图、控制台无着色器 / 纹理格式报错；模式 0 平光盒子截图采样 (189,189,189)。
- 主界面 wasm 构建：默认即 E3D 出厂预设，边线 / 伪阴影 / 渐变可见；`?look=pbr` 回到今天的画面；`?look_hlr=0&look_ao=0` 回到阶段一画面；四个子开关在设置窗里即时生效。
- 与 PC 同视角同模型（BRAN 24381_145018 Iso 3）截图并排，背景顶 / 底、未选中 / CE / highlight 像素与 PC 版容差 ±3；边线像素占比同量级。
- 整库 7997 级场景帧时间 ≥ 24 fps（或按第 3 条改了 Web 出厂 AA 并记录）；刷新 / 关页不卡死（`bevy-wasm-tasks` 那条旧坑不被新节点踩到）。
- 降级路径至少在一台机器上真走一遍（Chrome 用 `--disable-webgl2` 或关掉扩展的方式模拟）：不崩、两格置灰、日志一行。

## 阶段五：收尾

- `CONTEXT.md` 新词条：「E3D 外观」「边线」「伪阴影」「预设」「CE 高亮 / 其它选中」「元素配色」，每条带 _Avoid_（例：不叫「描边」「AO」「主题」）。
- ADR-0032「三维外观是视口设置，不是模型数据」：外观 / 预设 / 子开关按机器落盘、不进项目 bundle、不影响可见性台账与拾取；记下 SSAA 代替 MSAA、半透明不参与边线两条偏差，以及 Web 端的降级规则。
- `docs/evidence/2026-10-22-e3d-look.md`：来源 / PC / Web 三列并排截图、像素统计表、帧时间表、降级路径截图。
- `CHANGELOG.md`：默认外观改变写在最上面一句（老用户第一眼就不一样）；Web 端单独一句。
- 删 `crates/plant-ui-view3d/examples/sgl_prepass_spike*`（它的两段着色器已长进正文）；`--look-demo` / `?look_demo=1` 留着：debug 构建总带，release 要开 `look-demo` feature（浏览器端的像素验收是 release wasm，阶段四改的）。

**进度（2026-10-24）**：全部落地——`CONTEXT.md` 新增「三维外观」一节七个词条（E3D 外观 / 预设 / 边线 / 伪阴影 / CE 高亮与其它选中 / 元素配色 / 超采样）；`docs/adr/0032-viewport-look-is-a-viewport-setting-not-model-data.md`（七条决定 + 七条与计划书的偏差）；`docs/evidence/2026-10-22-e3d-look.md`（自动化、截图表、像素统计、帧时间、降级路径、手工验收清单；来源侧基准那一列后来补了演示厂三张，真模型的仍空）；CHANGELOG 第一条就是默认外观改变、Web 端单独一条；spike 的 example 连同它的 `index.html` / 两段 WGSL 已删，`plant-ui-view3d` 的 web-sys 只剩 `console`。`docs/evidence/…/spike/` 的三张截图留作历史。

**AMS 真模型验收（2026-10-24）**：用本机 AvevaMarineSample（库供数，模型服务没起）跑了六类模型——设备 / 管道分支 / 小口径盘管 / 建筑墙体 / 暖通风管 / 支吊架，外加选中两档、PBR 与按类型切换，都按预期；截图与逐类说明在 `docs/evidence/2026-10-22-e3d-look.md` §6（`docs/evidence/2026-10-22-e3d-look/ams/`）。**整库 7997 帧时间（release，5.3 万网格实体）**：E3D 全开 + SSAA 45–57 fps、FXAA 40–56、关 41–46、PBR 60（垂直同步顶），阶段三 ≥ 24 fps 的门槛过了，详见 evidence §4.2。阶段三验收的「真模型全流程」里，尺寸标注与房间视图（进入 + 面板）当晚用服务供数（gen-model debug 8023）补跑了，在 E3D 外观下都照常（evidence §6 末两行）；房间视图的 X-Ray / 直段截断（沙箱里房间没有成员）还没跑；虚拟孔洞预览的检查用户拍板延到下一阶段，不在本计划的验收里。

## 施工顺序

「必须先于动手」两条 → 阶段一（PC，可独立交付：`sgl-machine` 预设已完整）→ 阶段二（PC，出厂预设完整）→ 阶段三（PC 收口，可发一版）→ 阶段四（Web）→ 阶段五。阶段一与阶段二之间不停，但阶段一结束要出一次像素对照，别带着颜色口径的错进后处理。阶段四的第 1 条（wasm spike）可以在阶段二期间顺手提前跑一次，早点知道降级要不要做；但不阻塞 PC 进度。

## 非目标

- 不改 plant3d-web；不把它的 OutlinePass（web PBR 口径的品红描边）搬过来——plant-ui 的 PBR 外观维持今天的橙红换色。
- 不做 autocolour 规则（项目级 `gphcolopt` 导出翻译）、`EnhancedEdgesLaser` / `PseudoShadowsLaser`、`SHINY` / `TRANSLUCENCY_STYLE` 分支——来源侧也没做。
- 半透明几何不进 prepass（Bevy 限制）：E3D 的 `EnhancedEdgesTranslucent = ON` 不复刻，房间体 / 切削体 / 半透明 CE 色在边线与伪阴影里当背景。要补得给半透明再开一条 prepass，另议。
- 不动 `room_clip` / `virtual_hole` 的几何与布尔逻辑，只换它们取材质的地方。
- 不做真机 E3D 对参（来源侧 §6 列的仍未验证项照旧未验证，本计划以 plant3d-web 的像素为准）。
- Web 端不做 WebGPU 后端（工作区是 `webgl2` feature，不动）、不做 Safari / Firefox 验收。

## 风险

| 风险 | 后果 | 对策 |
|---|---|---|
| vendored Bevy 0.16.0-dev 的 prepass 绑定 / `prepass_io` 结构与设想不符，自定义 alpha 写不进去 | 参与标记无处放 | spike 第一天验；退路：「所有 SGL 几何全参与」（正好是 E3D 默认，只丢 handles 两个开关；prepass 深度是 `Depth32Float`，没有 stencil 可借）|
| PC 阶段写出 WebGL2 跑不了的东西（compute、多重采样读取、非 16 字节对齐 uniform、`std::env`） | 阶段四返工 | 「交付顺序」那行纪律写进每个阶段的验收；阶段二期间顺手跑一次 wasm spike |
| WebGL2 设备没有半浮点颜色附件 | 边线 / 伪阴影不可用 | 能力探测 + 置灰 + 日志；材质与背景不受影响；确有需要再做定点 ND 退路 |
| SSAA 4 倍填充率在核显 / 浏览器掉帧 | 交互卡 | `aa` 子开关、FXAA 备选、AO 半分辨率；Web 端出厂 AA 按实测定；性能门槛写进验收 |
| sRGB 字节直写与 `Bgra8UnormSrgb` 目标双重编码 | lightgrey 画成 130 | spike 模式 0 与阶段一像素探针都钉 189 / 130 |
| 材质组件互换与 `room_clip` / `virtual_hole` 直接 `Query<&mut MeshMaterial3d<StandardMaterial>>` 的假设冲突 | 切外观后兄弟段 / 预览网格材质错 | 阶段一就把三处收进 `LookMaterials`，现有 `room_xray_overrides_selection_and_restores_the_highlight` 等测试在两种外观下各跑一遍 |
| 宿主主选中与 E3D 的 CE 口径不一致 | CE 黄落错元素 | 已改为直接下发 `Selection::primary()`（`set_selection(refnos, primary)`），不再按两集之差推断；主选中不在集内按没有算 |
| 默认外观切换让老用户以为模型颜色「坏了」 | 支持成本 | 首次启动日志一行 + CHANGELOG 首句；设置窗「元素配色」一键回按类型 |
