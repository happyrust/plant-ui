# 三维选中效果改进（染色着色 + 选中描边）· 验证记录（ADR-0032 修订 2026-11-18）

计划：`docs/plans/2026-11-18-selection-outline-plannotator.md`。
决策：`docs/adr/0032-viewport-look-is-a-viewport-setting-not-model-data.md` 文末「修订（2026-11-18）」。
参照：plant3d-web 默认外观的选中画法（`src/components/dock_panels/ViewerPanel.vue:439-440`、`:4025-4033`，`src/utils/three/dtx/outline/DTXOutlineHelper.ts`），只读。
工作树：plant-ui `feat/model-load-parity`，基于 `1435ce55f`。实测机：AMD Radeon RX590 GME，150% 缩放。桌面端缺省后端 Vulkan（驱动 26.1.1），另用 `WGPU_BACKEND=dx12`（驱动 31.0.21924.61）跑了一遍；网页端是同一块显卡上的 Chrome（ANGLE Direct3D11 → WebGL 2.0）。

## 1. 自动化

- `cargo test -p plant-ui -p plant-ui-view3d -p rs-plant`：plant-ui **267 + 1 + 1**、plant-ui-view3d **133**、rs-plant **279**，0 failed。
- `cargo clippy -p plant-ui -p plant-ui-view3d -p rs-plant --all-targets`：无 error。告警的行号逐条对过这次改动的行：落在新增 / 改动行上的只有 2 条，都是新文件里的 `function check is never used`（`ShaderType` 派生宏生成的，库里别的 uniform 结构体已有 13 条同类）。
- `cargo check -p plant-ui -p rs-plant --all-targets`、`cargo build -p rs-plant`：exit 0。

新增的单测：

| 单测 | 钉住什么 |
|---|---|
| `selection_outline::tests::the_mask_camera_rides_on_the_main_camera_and_follows_its_target` | 遮罩相机是主相机的子实体、只看遮罩层、不开多重采样；遮罩图是视口屏幕分辨率的 2 倍、覆盖率图是屏幕分辨率，尺寸与投影跟着主相机走 |
| `selection_outline::tests::the_mask_camera_runs_only_while_something_is_selected_and_the_viewport_renders` | 没有选中件、或主相机停画时，遮罩相机关着 |
| `selection_outline::tests::the_mask_is_twice_the_screen_resolution_within_a_pixel_budget` | 遮罩图 2 倍放不进像素预算（1600 万）时等比例缩，最少缩回屏幕分辨率，边长不超过设备的纹理上限 |
| `selection_outline::tests::the_outline_width_follows_the_screen_scale` | 宽度 = 2 逻辑点 × 屏幕缩放，夹在 1.5–6 物理像素 |
| `selection_outline::tests::the_uniform_matches_the_shader_layout` | Rust 侧 uniform 与着色器里的结构体布局一致；两段着色器对得上（第一段把遮罩的 alpha 写进覆盖率图，第二段读它） |
| `render_gate::tests::selection_outline_changes_wake_the_camera` | 挂层、摘层、描边参数变化都会唤醒空闲停画的相机 |
| `look_materials::tests::the_selection_palette_switches_fill_and_outline_together` | 配色开关同时换填色与描边色；只认 `web`，写错即缺省 |

改写的单测（`lib.rs`、`look_materials.rs`、`e3d_look/material.rs`）：选中 / 悬停保留元素透明度；E3D 外观选中 / 悬停的 `flags.z == SHADING_TINT`（不再是直出），X-Ray 两态仍是 `SHADING_FLAT`；选中件挂 `RenderLayers[0, 1]`、取消选中后摘掉；着色器文本钉住染色那一行与两个暗面系数。

## 2. 实拍

### 2.1 怎么拍的

- **场景**：`--look-demo` 的演示厂——三根管、一个设备方箱、一根柱一根梁、一间半透明的房间体（alpha 64），所有构件都在房间体里面，相机从下往上看。
- **启动**：`PLANT_UI_SIM=1 BEVY_ASSET_ROOT=<仓库根> rs-plant --look-demo`。PBR 外观用 `PLANT_UI_SETTINGS_FILE` 指向只有一行 `(look: (mode: pbr))` 的设置文件；品红配色再加 `PLANT_UI_SELECTION_PALETTE=web`；DX12 加 `WGPU_BACKEND=dx12`。
- **抓图**：用 egui 检视口（`EGUI_INSPECTION`）驱动点击与截图，窗口挪到屏幕外、不抢焦点。截图是逻辑分辨率（1601×1000）；并排图左右各裁同一块。
- **改前图**：用的是动手前 `target/debug/` 里现成的 `rs-plant.exe`（2026-11-05 11:05 构建，在 `b4823d31f` 那次基线快照之后 5 分钟），不是从 `1435ce55f` 重新编的。决定选中画法的四个文件（`look_materials.rs`、`e3d_look/sgl_look.wgsl`、`e3d_look/material.rs`、`e3d_look/colour_table.rs`）从 `b4823d31f` 到 `1435ce55f` 没有改动（`git diff --stat` 为空），所以画法可比；视口 crate 其余的差别（拖动降画质、包围盒缓存、装载计时、演示厂 `demo.rs` 26 行）让两组图的取景略有出入。
- **多选与「被遮挡」的图**：演示厂的视口只能单选（多选要从模型树来，演示厂的树里没有这些件）。这几张是临时在宿主里加了一个「从文件读选择集」的调试入口拍的，拍完已撤掉，**不在提交里**；涉及 `e3d-multi-native-vs-web.png`、`pbr-multi-native-vs-web.png`、`e3d-occluded-after.png` 与四张 `zoom-e3d-*` / `zoom-pbr-*`。改前的二进制没有这个入口，所以多选没有「改前」图。
- **哪些图是改遮罩之前拍的**：2.2 的图是上午拍的，那时遮罩还是「屏幕分辨率、遮罩相机 MSAA 4×」；之后遮罩改成了「2 倍分辨率、不开多重采样」（缘由见第 4 节）。Vulkan 上两种遮罩画出来的轮廓一样（选中房间体的描边像素 2099 → 2116，外接框不变），这些图没有重拍。最终代码的图是 2.3 那几张。
- **Vulkan 的图里构件没有黑色边线**：本机 Vulkan + GPU 间接绘制下 prepass 不落笔，边线 / 伪阴影全无，是已知的老问题（`docs/evidence/2026-10-22-e3d-look.md` §4.4），改前的图同样没有，与这次无关。DX12 与网页端的图里有边线。
- **网页端**：包是从最终代码重编的（`cargo build -p rs-plant --target wasm32-unknown-unknown --release --features look-demo`，再过 wasm-bindgen），放在不入库的临时目录（`tmp/`），**没有覆盖 `web/public/pkg`**。本机起一个只发静态文件的服务（没有后端接口），页面 `?look_demo=1`，PBR 外观加 `&look=pbr`。用无头 Chrome（`--use-angle=d3d11`，视口 1600×1000，设备像素比 1）点击、截图、量相邻两帧的间隔。
- **描边像素数**：在截图的三维视口范围里数「像描边色」的像素（缺省配色：`r > 215`、`40 < g < 120`、`b < 60`）。桌面截图是 150% 缩放下的逻辑分辨率，网页是 100%，两边的数不能直接比，只在同一端的前后之间比。

### 2.2 图（桌面 Vulkan）

都在 `docs/evidence/2026-11-18-selection-outline/`。

| 图 | 外观 / 配色 | 内容 | 看什么 |
|---|---|---|---|
| `e3d-one-before-after.png` | E3D / 缺省 | 选中设备方箱 | 改前三个面一个纯黄；改后三面有明暗，外沿一圈橙红 |
| `e3d-room-before-after.png` | E3D / 缺省 | 选中房间体 | 改前是一整块不透明的纯黄，里面全挡住；改后半透明黄 + 描边，里面看得见 |
| `e3d-hover-before-after.png` | E3D / 缺省 | 方箱已选中，鼠标停在房间体上 | 改前整间房不透明橙，已选中的方箱只剩一块锯齿状的黄；改后房间半透明橙、不描边，方箱的黄与描边都在 |
| `pbr-one-before-after.png` | PBR / 缺省 | 选中设备方箱 | 填色不变（橙红，照常打光），多一圈描边 |
| `pbr-room-before-after.png` | PBR / 缺省 | 选中房间体 | 改前是一整块不透明的砖红；改后保留半透明 + 描边 |
| `e3d-multi-native-vs-web.png` | E3D / 两套 | 主选中方箱，另选一根管与一根柱 | 缺省：黄 / 白 + 橙红描边；品红：都是品红 + 粉描边 |
| `e3d-room-native-vs-web.png` | E3D / 两套 | 选中房间体 | 同上，半透明件 |
| `pbr-multi-native-vs-web.png` | PBR / 两套 | 同上那组多选 | 缺省：橙红填色 + 橙红描边；品红填色 + 粉描边 |
| `e3d-occluded-after.png` | E3D / 缺省 | 主选中一根从横管后面穿过去的竖管，另选一根管 | 竖管被挡住的那一头，描边画在挡它的横管上 |
| `zoom-e3d-multi.png`、`zoom-e3d-multi-web.png`、`zoom-pbr-multi.png`、`zoom-e3d-occluded.png` | — | 对应图的局部，最近邻放大 3 倍 | 描边的宽度与边缘的抗锯齿 |

### 2.3 图（最终代码：网页端与 DX12）

| 图 | 后端 / 外观 | 内容 | 看什么 |
|---|---|---|---|
| `desktop-vs-web-e3d-room.png` | Vulkan 对 WebGL2 / E3D | 选中房间体 | 两端的填色与描边位置一致（网页端构件有黑色边线、Vulkan 没有，见 2.1） |
| `desktop-vs-web-pbr-room.png` | Vulkan 对 WebGL2 / PBR | 选中房间体 | 同上 |
| `web-e3d-room.png`、`web-pbr-room.png` | WebGL2 | 选中房间体，整个视口 | 半透明填色 + 一圈描边 |
| `web-e3d-hover-room.png` | WebGL2 / E3D | 方箱已选中，鼠标停在房间体上 | 房间半透明橙、不描边；方箱的描边还在 |
| `zoom-web-e3d-box.png`、`zoom-web-pbr-box.png` | WebGL2 | 选中方箱，局部最近邻放大 4 倍 | 描边连续、边缘平滑、没有坏点 |
| `dx12-e3d-room.png` | DX12 / E3D | 第一次选中房间体后 0.7 秒 | 描边完整，轮廓里面没有坏点 |
| `dx12-msaa-mask-first-select.png` | DX12 / E3D，**旧遮罩** | 第一次选中房间体 | 填色换了，描边没有 |
| `dx12-msaa-mask-speckle.png` | DX12 / E3D，**旧遮罩** | 第三次点选之后 | 轮廓里面一片成对的橙红坏点 |
| `zoom-dx12-msaa-mask-speckle.png` | DX12 / E3D，**旧遮罩** | 坏点的局部放大（这一张关了 GPU 间接绘制，坏点照样有） | 坏点的形状 |
| `dx12-pbr-before-change.png` | DX12 / PBR，**改前的程序** | 选中房间体 | 主场景画不出来，只有背景与零星坏点（老问题，见第 4 节末尾） |
| `dx12-pbr-after-change.png` | DX12 / PBR | 选中房间体 | 主场景同样画不出来；描边是完整的 |
| `dx12-pbr-main-msaa-off-experiment.png` | DX12 / PBR，**实验，没提交** | 关掉 PBR 主相机的多重采样后选中房间体 | 画面正常，描边也在 |

## 3. 读数

### 3.1 桌面 Vulkan（改遮罩之前量的）

截图上取像素（sRGB 字节）。

| 项 | 读数 |
|---|---|
| 描边核心色（缺省配色，`SELECT_COLOR` = `(255, 69, 0)`） | 约 `(247–249, 83–84, 17–25)`（边缘抗锯齿与底色混了一点） |
| 描边宽度 | 截图上约 2 像素（= 2 逻辑点 = 3 物理像素），E3D 外观（2× 超采样）与 PBR 外观一致 |
| E3D 外观其它选中白（隔着半透明墙看） | 管 214–224、柱 220–238；旁边没选中的件 164–200 |
| 同一项，暗面系数统一取 0.68 时（已弃用） | 选中管 177–202，比没选中的管（173–238）还暗——所以白只压到 0.88 |
| PBR 外观选中填色 | 约 `(182, 67, 45)`，外沿一圈亮橙红 |
| 帧时间 | 选中整间房、窗口有焦点：16.7 ms（60 帧封顶）。窗口失焦时无论有没有选中都是约 32 帧，那是程序自己的失焦限速 |

行为上看过的：取消选中后描边消失、没有残影；空闲停画状态下改选中，画面会醒来更新；屏幕上交叠的两个选中件之间没有线（共用一张遮罩，轮廓是并集，见 `zoom-e3d-multi.png` 方箱与管交叠处）；没选中的件挡在选中件前面时，描边画在它上面（同一张图里那根斜穿过去的细管）；八次运行的日志里没有崩溃与 wgpu 报错。

### 3.2 最终代码：桌面两种后端

同一个点连点四次（点在房间体上），每次点完截图、数描边像素。

| 后端 / 外观 / 开关 | 四次点选的描边像素 | 外接框（截图坐标） |
|---|---|---|
| Vulkan / E3D | 2116、2116、2116、2116 | x 533–1056，y 142–597 |
| Vulkan / E3D，`PLANT_UI_INDIRECT_DRAW=0` | 2127 ×4（这一次视口高了 1 个物理像素） | x 533–1056，y 142–598 |
| Vulkan / PBR | 1594 ×4 | x 533–1056，y 142–597 |
| DX12 / E3D | 2116 ×4 | x 533–1056，y 142–597 |
| DX12 / E3D，`PLANT_UI_IDLE_RENDER=0` | 2116 ×4 | x 533–1056，y 142–597 |

- 第一次选中后 +0.5 s、+2 s、+4 s 各截一张：三张的视口逐像素相同；第一次与第四次点选的视口也逐像素相同（DX12 / E3D、Vulkan / E3D、Vulkan / PBR 都比过，差 0 个像素）。
- 帧时间（窗口停在屏幕外，两秒平均值）：DX12 / E3D 没选中 16.7–17.0 ms，选中整间房 16.7–17.8 ms。Vulkan 的窗口停在屏幕外时被限在约 32 ms：E3D 选中前 32.2–32.5、选中后 31.5–32.7；PBR 选中前 32.3–36.2、选中后 35.4–40.2（各只有三五个读数，含截图的开销，不足以判断有没有变慢）。
- 五次运行的日志里没有 WARN / ERROR / panic / wgpu 报错。

### 3.3 最终代码：网页端（WebGL2）

| 项 | E3D 外观（离屏图 1968×1186 = 984×593 × 2） | PBR 外观（离屏图 984×593） |
|---|---|---|
| 没选中 | 0 | 0 |
| 选中方箱 | 490（x 660–787，y 345–465） | 450 |
| 选中房间体 | 1930（x 532–1056，y 142–598） | 1586 |
| 同一个件再选一次 | 视口差 0 个像素 | 视口差 0 个像素 |
| 鼠标停在没选中的房间体上 | 房间半透明橙，描边 0 | 画面不变（PBR 外观本来就没有悬停色） |
| 方箱已选中、鼠标停在房间体上再移开 | 描边的外接框不变；移开后与只选方箱时差 0 个像素 | — |

相邻两帧的最长间隔（毫秒；稳定 60 帧时是 17）：

| | 空闲 | 首次选中方箱 | 首次选中房间体 | 再选方箱 | 悬停 |
|---|---|---|---|---|---|
| E3D | 17 | 33 | 17 | 17 | 17 |
| PBR | 17 | 50（跑了两遍都是 50） | 17 | 17 | — |
| 改遮罩之前的包，E3D | 50 | 33 | 33 | 17 | 33 |
| 改遮罩之前的包，PBR | 17 | 17 | 17 | 17 | — |

没有超过 100 ms 的间隔。PBR 外观首次选中多出来的那两帧（17 → 50）是改遮罩之后才有的，只在头一回选中时出现一次，没有往下查是哪一步。

浏览器控制台：没有着色器、WebGL、wgpu 的报错；只有静态服务没有后端接口带来的 404（`/api/v1/*`、`/assets/config/e3d.project.ron`）。

## 4. 遮罩为什么不开多重采样（DX12 上查到的）

第一版遮罩是「屏幕分辨率、遮罩相机 MSAA 4×」。Vulkan 与 WebGL2 上没有问题，DX12 上（同一块 RX590）画出来是坏的。同一个程序、同一个点连点四次：

| 运行（第一版遮罩） | 每次点选后的描边像素 |
|---|---|
| Vulkan | 2099、2099、2099、2099 |
| DX12 | **0**（+0.5 s / +2 s / +4 s 三张都是 0）、2099、**3161**、**3161** |
| DX12，`PLANT_UI_IDLE_RENDER=0`（不停画） | 四次点选、12 张截图**全是 0** |

2099 是对的数；3161 多出来的一千多个像素是轮廓里面成对的坏点（`dx12-msaa-mask-speckle.png`）。关掉 GPU 间接绘制也一样：头一回是空的，之后带坏点（`zoom-dx12-msaa-mask-speckle.png`）。

排查时只把遮罩相机的多重采样关掉、别的不动：DX12 上六次点选都是 2175，没有坏点。所以问题出在「多重采样的遮罩相机」这一环；是显卡驱动的事，还是 wgpu / Bevy 在 DX12 上处理多重采样离屏目标的事，**没有定位**，也没有在别的显卡上试过。

最终方案不依赖各家驱动的多重采样解析：

- 遮罩相机 `Msaa::Off`，遮罩图按视口屏幕分辨率的 2 倍画（每个屏幕像素 2×2 个遮罩像素）。
- 全屏节点分两段：`selection_mask_resolve.wgsl` 用线性采样把 2×2 的 alpha 平均成一张屏幕分辨率的单通道覆盖率图（`R8Unorm`），抗锯齿出在这一步；`selection_outline.wgsl` 在覆盖率图上找边、叠色。
- 结果见 3.2：Vulkan 与 DX12 都是 2116，第一次选中就有描边，连点不变。

代价是显存（按纹理尺寸算的，没有实测）：有选中件时遮罩相机开着，多占 4 张遮罩大小的纹理（输出图、两张主纹理、深度，各 4 字节一像素）加一张覆盖率图，合每个屏幕像素 65 字节；第一版是 44 字节。本机视口 1477×889 物理像素，遮罩 2954×1778，约 85 MB（第一版约 58 MB）。遮罩的像素数封顶 1600 万（约 256 MB），视口大到 2 倍放不下时等比例缩，最少缩回屏幕分辨率。没有选中件时遮罩相机关着，只剩输出图与覆盖率图（本机约 22 MB）。

**另一件老问题，不在这次范围**：DX12 + PBR 外观下主场景本身画不出来，只有背景与零星坏点。改前的程序（11-05 构建）一样（`dx12-pbr-before-change.png`），改后也一样、但描边是完整的（`dx12-pbr-after-change.png`，描边像素 1471）。试着把 PBR 主相机的多重采样关掉，画面就正常了（`dx12-pbr-main-msaa-off-experiment.png`，描边 1588，连点三次不变）——这个实验没有提交，关掉它 PBR 外观就没有抗锯齿了，要不要处理、怎么处理等用户定。本机缺省后端是 Vulkan，不受影响。

## 5. 没验到的

- **真实大库**：只在演示厂（7 个件）上看过。选中几千上万个件时遮罩相机多画的那一趟要多少时间，没有数据；遮罩现在是 2 倍分辨率，这一趟只会比第一版更重。
- **网页端**只在无头 Chrome（ANGLE Direct3D11）+ 演示厂上验过：没有在带界面的浏览器里人手点过，没有连真实后端，没有试别的浏览器。
- **DX12** 只有这一块显卡。DX12 + PBR 外观因为上面那件老问题，整幅画面没法看，只确认了描边画得出来。
- **视口大到触发遮罩像素预算**（等比例缩）的情形只有单测，没有实拍。
- **X-Ray 叠选中**：演示厂开不了房间 X-Ray，只有单测（状态要描边、颜色与 alpha 不变），没有截图。
- **直段截断、孔洞预览下的选中**：只核对了代码路径（见计划「实施」末尾），没有实拍。
- **深色主题**：实拍都是浅色主题。
