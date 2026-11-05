# 三维外观是视口设置，不是模型数据

状态：**已接受**（2026-10-22 拍板：默认就是 E3D 外观（出厂 E3D 3.1 预设）、元素一色 lightgrey、抗锯齿整帧 2×2 超采样；
先 PC 后 Web。与计划书的偏差逐条记在本文末尾与计划书各阶段的「进度」段。）
计划：`docs/plans/2026-10-22-e3d-look-port-plannotator.md`。来源：plant3d-web `src/viewer/e3dLook/`（对 AVEVA E3D 3.1 `sglDx11.dll` 的逆向），一字不改。
验证：`docs/evidence/2026-10-22-e3d-look.md`。

## 背景

三维视口一直是 Bevy PBR（按类型配色、定向光 + 环境光、橙红选中），和设计师天天对着的 E3D 不是一个样子：
没有边线分不清面，没有伪阴影看不出接触，一色的正式图纸感没有。plant3d-web 已经把 sglDx11 的着色公式、
环境立方体贴图、后处理链（HBAO / HLR / 去杂）、颜色表和两套预设逐一逆向并验过像素，这次是**搁过来**，不重新推导。

要定的不是画法（那是来源侧的事），而是这套画法在 plant-ui 里**算什么**：它是模型的属性，还是看模型的人的属性；
一台机器上切了外观，别的机器、别的项目、模型库里的东西该不该跟着变。

## 决定

### D1　外观是视口设置，按机器落盘，不进项目

`Settings.look: ViewportLook { mode, preset, hlr, ao, gradient, aa, colour_scheme }` 随 `settings.ron` 按机器落盘，
与 `theme` 同一条房规：不进项目 bundle，不进模型库，不进版本管理。老文件没有这一节按出厂位读（`serde(default)`），
所以老用户升级即切到 E3D 外观，启动日志说一句去哪切回。浏览器端没有 `settings.ron`：站点在 `config.json` 的
`look` 段给默认，地址栏 `?look=…` 压一层、只活到刷新，设置窗里改的不落盘。

### D2　外观不碰模型数据、可见性台账与拾取

切外观是一次全场换材质组件（`MeshMaterial3d<StandardMaterial>` ↔ `MeshMaterial3d<SglLookMaterial>`），
「这一刻该用哪枚材质」收敛在一个资源 `LookMaterials`（外观 × 颜色 × Base / Ce / Highlight / XRay）。
`ModelMesh` 只记颜色字节与「是不是线」，不持材质句柄；`room_clip`、`virtual_hole` 换材质都从它取。
可见性台账、拾取（射线对网格）、相机、模型数据一样都不读外观。

### D3　选中按 E3D 口径，CE 由宿主给

E3D 外观下主选中染 CE 黄、其它选中染 highlight 白，整体换色、没有描边；web PBR 下仍是橙红。
CE = 宿主 `Selection::primary()`（属性面板跟随的那一个），经 `View3d::set_selection(refnos, primary)` 一并下发；
视口不自己按前后两集之差猜。X-Ray 照旧压过选中（它是辅助态，E3D 没有对应物）。
选中态**颜色直出**（不打光不反射）且**一律不透明**——2026-10-29 修订，见文末「修订」第 1、2 条。

### D4　辅助几何与半透明几何不进边线 / 伪阴影

地面网格 / 三轴 / 断口 / 标注线 / 无效 TUBI 虚线走 `StandardMaterial` 且 `PbrPlugin { prepass_enabled: false }`，
不进 prepass = 不参与（同 E3D `EnhancedEdgesHandles` / `PseudoShadowsHandles` OFF）。半透明几何（房间体 / X-Ray）
因 Bevy 的 `AlphaMode::Blend` 不进 prepass，在边线与伪阴影里当背景——E3D 的 `EnhancedEdgesTranslucent = ON` 这一条**不复刻**，
是已知偏差。挡在它们前面的模型边线会透过来。

### D5　抗锯齿用整帧 2×2 超采样代替 MSAA，两端一套

Bevy 开 MSAA 时 prepass 纹理也是多重采样的，WebGL2 读不了；所以 E3D 外观下主相机 `Msaa::Off`，离屏图按
`size × 2` 分配、egui 线性采样缩回（2× 整数倍 = 4 像素盒式平均，就是 sglDx11 HLR 的 Σ/N 与 MSAA 解析的结构）；
AO / 模糊 / 去杂跑屏幕分辨率，HLR 的邻居是 1 个屏幕像素，合成在字节域里对 s×s 子像素取平均。备选 FXAA，
PBR 外观维持今天的 MSAA 4×。

### D6　深度走法线纹理，不读硬件深度（Web 逼出来的，PC 一起改）

naga 的 GLSL 后端做不了 `texture_depth_2d` 的 `textureLoad`。prepass 把线性眼空间深度拆成 3 × 10 bit 定点
写进法线纹理的 RGB（`Rgb10a2Unorm`，范围 65504 世界单位 = 6.5 km，步长 ≈ 0.006 mm），alpha 是参与标记；
后处理只读法线纹理，面法线由深度差分重建（法线纹理里不存法线）。一套着色器，没有按能力分叉的第二条 ND 路。

ND 纹理是 `Rgba32Float`（能当颜色附件就用；同来源 `_ndType = canFloat ? FloatType : HalfFloatType`，只有半浮点附件的设备才退 `Rgba16Float`）。
它的 a 通道是 HLR / HBAO / 模糊比对的线性深度：f16 只有 11 bit 尾数，百米外（> 1024 单位）量化步长 ≥ 1 单位 = 100 mm，超过 50 mm 的边线深度阈值，
每跨一个量化台阶 HLR 判据③就打一条边，在子像素格上打成条纹 / 同心环（2026-10-25 整库全视，evidence §4.3）。计划里写 f16 时只论了范围（65504 单位够）没论精度。

### D7　Web 端按能力降级，界面如实说

`Rgba16Float` 不能当颜色附件（WebGL2 缺 `EXT_color_buffer_float`，wgpu 按它判半浮点可渲染）时：不挂 prepass、不跑后处理、
不超采样（留 MSAA），后处理的管线一条都不排（`PipelineCache` 排了就编，排了就是校验错误转 panic），
设置窗「边线 / 伪阴影」两格灰掉并说原因，日志一行；材质、颜色、背景、选中照常；设置值照存照发，换台机器就生效。
不做定点 ND 的第二套管线（现代 Chrome / Edge 桶装都有这个扩展）。配套给 vendor Bevy 打了一处本地补丁：`bevy_core_pipeline` 的
`prepare_view_tonemapping_pipelines` 原来对每个视图都按 Rgba16Float 排一条色调映射管线（非 HDR 视图排了也没人用），真缺这个扩展的设备上
应用起不来——改成非 HDR 视图不排。没有这一刀，这条降级只对“Bevy 能起来但半浮点不可渲染”的设备有意义（见 evidence §5）。

## 与计划书的偏差（均记在计划书各阶段「进度」段）

1. **背景**原定留给 egui 在纹理底下画、离屏图透明底；改为合成在字节域里把背景渐变画进离屏图、输出不透明——
   vendor 的 bevy_egui 在线性空间里混、还对 Bevy 图再乘一次 alpha，边线会被压浅。egui 那层 E3D 背景留着做管线就绪前的底。
2. **ND 差分侧**改为「用该侧两点线性外推最能命中中心深度」的一侧（透视在 1/深度域里比），原来只比深度差大小，
   深度连续的凸 / 凹棱漏判。
3. **深度来源**从硬件深度纹理改为法线纹理里的 3 × 10 bit 线性深度（D6）；`DepthPrepass` 不再挂。
4. **去杂**在超采样下跑屏幕分辨率、每个样本先对 s×s 子像素取平均（同来源 `hlrResolved`），不是计划里「跑 2× 分辨率再由 egui 缩回」。
5. **`render_scale`** 只看 `mode` 与 `aa`，两个效果全关也保留超采样（同 plant3d-web 颜色 MSAA 不读 legacy）。
6. **调试开关** 没用 `PLANT_UI_LOOK_DEBUG` 这个名字：`--look-debug=<模式>` / `?look_debug=<模式>`（`SglLookDebugView`，任何构建都带）把后处理链的中间量
   直出到视口（ND 深度 / ND 深度相对 prepass 30 bit 深度的误差 / 面法线 / 参与标记 / 去杂前边线 / 伪阴影）；`--look-demo` / `?look_demo=1` 演示厂（debug 构建总带，
   release 要开 `look-demo` feature），`=far` 再多摆一件远景检验件（安全壳尺寸的筒体 + 穹顶，相机 180 m 外）并只框它取景。
7. **Web 端出厂 AA** 留 SSAA 2×2：整库全视在浏览器里只有 9–10 fps（evidence §4.3）；2026-10-25 晚同场对照，FXAA 全视 9–11 fps vs SSAA 6–7 fps，省三成帧时间但救不回 24 fps（提交端在卡），所以不按 `cfg!(wasm32)` 分出厂位；站点要换在 `config.json` 写 `"look": {"aa": "fxaa"}`。
8. **ND 纹理**从计划里的 `Rgba16Float` 改为能当颜色附件时 `Rgba32Float`（D6）：计划只论了 f16 的范围够用，没论精度；百米外 f16 的量化步长超过边线阈值，
   是 2026-10-25 整库全视条纹 / 同心环的根因。来源本来就是 `canFloat ? FloatType : HalfFloatType`，这是回到来源。
9. **GPU 间接绘制的逃生口** `PLANT_UI_INDIRECT_DRAW=0`（主相机挂 Bevy `NoIndirectDrawing`，退到 `PreprocessingOnly`），计划里没有：2026-10-25 本机 RX 590 + Vulkan 上
   Bevy 0.16 的间接路径让 prepass 整段不落笔（边线 / 伪阴影全无；上游 bevy#18904 / #19000 同类机器相关缺陷），直接 draw 或 DX12 即好。缺省不开：整库直接 draw
   全视 24–26 fps，同场景 DX12 间接绘制 32–34 fps，Vulkan 间接正常那天 47–57 fps；本机首选 `WGPU_BACKEND=dx12`。启动时 `[渲染]` 一行报适配器 / 后端 / 开关（evidence §4.4 末尾）。用户 2026-10-25 拍板：Windows 缺省后端不改。
10. **视口底色归绘制层，Bevy 不画**（2026-10-26）：拆掉那台从不出图的 Bevy 背景相机（`order −1`、`is_active: false`，连同 `BACKGROUND_LAYER` 上的渐变面片与 `BackgroundMesh`）。视口底色一直是 egui 在纹理底下画的那层（E3D 下再由合成段把背景渐变画进离屏图），那条路只是每帧占着一个相机实体、一张 mesh 和一条只改顶点色、谁也看不见的命令。
   主题送三维侧的只剩网格线色：`Cmd::SetViewportBackground { top, bottom, grid }` → `Cmd::SetViewportGrid(grid)`，`View3d::set_background` → `set_grid_color`，`ViewCommand::Background` → `GridColor`。
   这推翻了计划阶段「拷问定案第 2 题」里「用户点名在 Bevy 内画背景」的原始口径——那条从未真正生效过（相机自 2026-10-22 起就 `is_active: false`），本条件把现状写进定案，画面不变。
11. **legacy 的屏幕空间无效 TUBI 带**（2026-10-26）：拆掉 `InvalidTubiLineMaterial` + `invalid_tubi_line.wgsl`、`InvalidTubiLine` 组件、`InvalidTubiLineMesh` / `InvalidTubiLineStateMaterials` 资源、`MaterialState::choose` 与五个 `*_PX` 常量。
   生产里无效 TUBI 早已是 `ModelMesh { line: true }` 经 `LookMaterials` 取材质，那条路径没有任何 spawn 点，只剩测试在自己造实体。`invalid_tubi_fallback_line_mesh`（按真实长度切段的 `LineList`）与 `INVALID_TUBI_DASH_MM / GAP_MM` 是生产路径，照留。
12. **两样效果都关时不挂 prepass**（2026-10-26）：legacy 下相机不再挂 `NormalPrepass`，后处理的中间纹理只分配 1×1 占位——那时 ND / HBAO / HLR 一段都不跑、法线纹理没人读，整库场景白跑的那一遍 prepass（§4.2 量过 3–5 ms）与超采样下几十 MB 的 ND 都省下来；`SglLookNode` 的 `ViewPrepassTextures` 改为可选，合成照跑。

## 后果

- **老用户第一眼就不一样**：升级即 E3D 外观。补偿是设置窗一键回 web PBR、启动日志一句、CHANGELOG 第一条。
- **两套材质类型并存**：换外观是换组件类型不是改 uniform，所以任何「自己持 `Handle<StandardMaterial>` 换材质」的新代码都会在 E3D 外观下出错；
  一律经 `LookMaterials::resolve`。
- **显存**：超采样下 1500×800 视口（离屏图 3000×1600）粗算 ≈ 180 MB：离屏图 19 + Bevy 主纹理 a/b 38 + prepass 深度 / 法线 38 + ND f32 77 + HLR 5 + 屏幕分辨率的 AO / 模糊 / 去杂四张 5（ND 从 f16 换 f32 多出约 38 MB；下游读它的带宽也翻倍）；两样效果都关时不挂 prepass、中间纹理只是 1×1 占位，只剩离屏图 + 主纹理。超宽窗的物理尺寸夹在 `max_texture_dimension_2d`。
- **浏览器端头几帧**：egui 在 bevy_egui 算好相机前按 10000×10000 布局，宿主按「视口不可能比窗口大」挡掉那一帧的渲染目标请求；
  离屏图不带 CPU 数据（WebGL2 不收 BGRA 上传）。这两条与外观无关，但是超采样把它们从「报错一行」放大成「丢上下文」。
- **已知偏差要说出口**：半透明不进边线 / 伪阴影（D4）、SSAA 代 MSAA（D5）、没做 autocolour / Laser / SHINY 分支（来源侧也没做）、
  选中色与真机逐像素值脱钩（修订 1：真机 CE 是 `(255,255,79)`、highlight 是 `(249,249,249)`，现在是纯黄 / 纯白）。
- **vendor Bevy 带一处本地补丁**（`vendor/registry/bevy_core_pipeline/src/tonemapping/mod.rs`，`.cargo-checksum.json` 同步，补丁头上的注释写明来历）：
  重新 vendor / 升级 Bevy 前先把 `vendor/patches/0001-bevy_core_pipeline-….patch` `git am` 到 happydpc/bevy 并推上去（或确认上游已不给非 HDR 视图排那条管线）；
  cargo 不跟踪 vendor 源码改动，改完要 `cargo clean -p bevy_core_pipeline`。

## 为什么不是别的做法

- **外观进项目 bundle / 模型库**：同一份模型在不同机器上该长一样吗？不该——这是看的人的偏好和显卡的能力，和主题一样是机器的事。
- **直接删掉 PBR**：用户拍板的是「默认 E3D、可切回」；且 PBR 是没有半浮点颜色附件的设备上仍然完整的那一套。
- **自己再画一趟法线 / 深度（第二台相机）**：Bevy prepass 已经替每种 `Material` 生成管线并把纹理交给后处理节点；第二趟要复制可见性 / 变换同步。
  代价是 D4 的半透明偏差。
- **MSAA**：WebGL2 读不了多重采样 prepass 纹理，两端就得两套代码；整帧 2×2 一刀切，结构上与 E3D 的 4 采样一致。
- **定点 ND 的第二套管线**（没有半浮点附件的设备）：多一套着色器要养，而现代浏览器都有这个扩展；灰掉并说明就够。

## 修订（2026-10-29）

计划：`docs/plans/2026-10-29-selection-highlight-visibility-plannotator.md`。起因：用户「对比 e3d 里高亮选中的效果，
目前 plant-ui 里这里高亮的效果并不是特别好」。查下来不是配色错，是配色被画法洗淡了。

1. **选中态直出颜色**（`look_materials.rs::sgl` 的 `flat` 从「只有 X-Ray」扩到「X-Ray / XraySelected / CE / Highlight」）。
   出厂预设 `Kr = 0.8`，CE 黄的蓝通道被环境反射抬到约 0.4，屏上从纯黄变成 `(255,255,79)`；highlight 白被 `0.7 + 0.8·env`
   顶到 `(249,249,249)`，与常态 `(189,189,189)` 只差 60 个灰阶——**选中几乎看不出**。直出后屏上字节等于配色表字节。
   **代价**：与 E3D 3.1 真机的逐像素值脱钩（真机那两组数见 `docs/evidence/2026-10-22-e3d-look.md:110`）。
   取舍理由：CONTEXT 把「CE 高亮 / 其它选中」定义为界面语义，没承诺逐像素复刻；可见性优先。
   要复刻真机值时改回 `flat` 判定即可，不改公式与常数。
2. **选中态 alpha 归一到 255**（`e3d_colour` 的 `Ce` / `Highlight` 分支）。原来沿用元素自己的 alpha，
   半透明件选中后既被稀释、又落进 `AlphaMode::Blend`——半透明网格不进 prepass，于是**选中件没有边线与伪阴影**，
   等于「选了像没选」。这一条修的是 D4 记的那个缺口的一半（E3D `EnhancedEdgesTranslucent = ON`），
   另一半（未选中半透明件也描边）仍不做，仍要另开一条半透明 prepass。
3. **第五态 `XraySelected`**：X-Ray 命中且同时在选中集里时，颜色换 CE 黄、alpha 从 64 抬到 160，仍 `flat + Blend`。
   优先级不变（X-Ray 仍压过 CE / Highlight），只是命中后再看是不是选中——房间视图里点中什么不再只是一片淡蓝。
   PBR 外观不区分这两态（仍落那枚共享的半透明蓝），画面与改动前一致。`virtual_hole` 的孔洞 X-Ray 同口径。
4. **悬停预选（阶段四）**：新增 `MaterialState::Hover`，颜色取 PDMS `orange` #ffbe00（E3D 的 active），
   同样直出、不透明；优先级 `X-Ray > CE > Highlight > Hover > Base`——悬停只落在未选中件上，与 plant3d-web
   `SelectionManager` 的「选中件不被悬停色覆盖」同口径。宿主侧新增 `Cmd::HoverViewport(Option<[f32;2]>)`，
   与 `PickViewport` 分开（悬停是连续动作、拾取是一次性语义动作）；**节流在绘制层**（`HOVER_STEP_PX = 5`，
   指针不在视口 / 落在浮层 / 拖拽中不发，离开时发一次 `None`），视口侧只在目标真的换了才翻 `hover_dirty`。
   悬停的射线拾取走 `surface_hit(..., quiet: true)`，不打诊断日志（否则每次移动两行）。
   PBR 外观下悬停不另配色（仍是类型色），改动前的画面不变。
5. **悬停走增量刷，不整库重刷**（`apply_materials` 拆两条路径）。第 4 条最初把悬停接到了 `selection_dirty` 上，
   而那个旗子意味着「整库重挑材质」——悬停一次拖动会掠过几十上百个构件，等于每移动几个像素就把五万网格的
   `material_state` 与 `LookMaterials` 重算一遍。现在 `hover_dirty` 单立，`hover_applied` 记「上一帧刷过的悬停
   目标」，`apply_materials` 只重挑 refno / owner 命中「旧目标 + 新目标」的实体。选择集 / 外观 / 新网格出生仍走全场刷。
6. **背景渐变与选中白的对比度不改**（阶段二判定为不做）。背景两端字节（159 / 242）与选中色都是常量，按 WCAG 算：
   highlight 白落在背景底是 **1.12 : 1**，CE 黄是 1.04 : 1。把渐变下端拉回 grey 会同时把常态件的对比度从 1.68 压到 1.10，
   方向是错的；加粗选中轮廓要扩 prepass 标记位（`Rgb10a2Unorm` 的 2 bit 已被边线 / 伪阴影占满）并重跑整库远景 A/B，
   收益不抵风险。真机同样是 1.12 : 1，可见性靠 HLR 黑轮廓——`flat` 只关颜色项，轮廓本来就在。
   若日后仍判定不够，该做的是「选中描边」独立议题，不是改背景色。
7. **悬停的输入侧两条修正**：节流阈值改成两个轴各自折算（原式用视口宽度统一归一，扁视口里竖直阈值会掉到
   2 pt 以下，一抖就发一发射线）；滚轮缩放期间撤掉悬停色（指针不动、相机在动，留着会指着错的元素）。
   `surface_hit` 的遍历代价已核：Bevy 的 `MeshRayCast::cast_ray` 先并行做 AABB 求交再只对命中者求三角，
   不再自己加预筛。
8. **悬停账随构件卸载一起忘**（`forget_model_state`）。`hovered` / `hover_applied` 是新增的按 refno 记的状态，
   不进卸载清单就会泄漏：同一个 refno 的模型重新装回来时一出生就是 active orange，而鼠标不在那儿。
   孔洞预览的 `apply_hole_materials` 不需要脏标（它的 query 只匹配被布尔替换过网格的那几十个实例，
   `attach` 内部先比句柄）；`room_clip` 没有独立的材质裁决分支（房间面板与断口截面都不参与三态裁决）。
