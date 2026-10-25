# 房间树（plant3d-web「房间」页签移植）· 验证记录（ADR-0033）

计划：`docs/plans/2026-10-25-room-tree-port-plannotator.md`（文末「进度」记了与计划的出入）。决策：`docs/adr/0033-room-tree-is-server-aggregated-and-service-face-only.md`。
来源：plant3d-web `src/composables/roomTreeNodes.ts` / `useRoomTree.ts` / `src/components/model-tree/RoomTreePanel.vue`（一字不改）。
工作树：plant-ui 本地工作树，2026-10-25 02:30–03:00（UTC+8）。实测机 150% 缩放（`ppp = 1.5`），截图 1600×1000 逻辑点。

## 1. 自动化

- `cargo test -p plant-ui`：**217 passed / 0 failed**（其中 `room_tree::tests` 12 例新增）。照 plant3d-web `roomTreeNodes.test.ts` / `useRoomTree.test.ts` 改写：
  标签与距离 / 长度格式；房间 → 专业 → 单元类型 → 单元 → 构件 → 直段的层次与「其他构件」分组；跨房标、无效直段标；
  三个收集器（直段不进 refno 集、BRAN 单元在单元处收住、未内联的组带选择器）；`unit=` / `other_noun=` 补叶子只填请求的那一组；
  根按房间号自然序（R43 < R101 < R432）与按房间号 / 名字 / refno 过滤；空房壳不发请求；展开展平与 eye 聚合（全 / 半 / 无，与
  `row_visibility` 同尺）；单点 / Ctrl / Shift 选中落到场景 refno（直段行 = 所属 BRAN）；外部选中只在取过树的房里找并展开父链；
  非 ready 状态折成注释；服务端形状的 JSON（带 `center` / `room_status` 等多余键、缺 `total_tube_count`）能解。
- `cargo test -p rs-plant`：**208 passed / 0 failed**（`spatial_api::tests` 2 例新增：路径带身份三格与 `tubes=1`、`unit=` / `other_noun=` 选择器；
  两种 404 分开（清单路由 = 老服务端、树路由 = 这间房没面板模型）、超时归「够不着」）。既有的 `data_rs_reads_only_through_read_face` 等纪律测试照常。
- `cargo check -p rs-plant -p plant-ui --tests`：通过，新文件无警告。

## 2. 独立壳（`PLANT_UI_SIM=1`，剧本里的两间房 + 一间空房壳）

`rs-plant.exe`（debug）带 `PLANT_UI_SIM=1` + `EGUI_INSPECTION=127.0.0.1:5723`，用同仓 `inspect` 探针（tree / click / type / key / shot）驱动。
图在 `docs/evidence/2026-10-25-room-tree-port/`。

| 画面 | 图 | 看到的 | 判 |
|---|---|---|---|
| 根清单 | `01-roots.png` | 左侧「模型树 ｜ 房间树」两个页签；搜索框「搜索房间号 / 名称（在册 3 间）」+ 刷新；状态行「在册 3 间房 · 展开一间房看 专业 → 最小交付单元 → 构件」；行按自然序 R12 < R101 < R102，R12 标「空房」无箭头 | ✓ |
| 展开到 BRAN 单元 | `02-r101-bran-expanded.png` | R101 · 控制室「22 个构件 · 4 条直段」→ 管道系统「7」→ BRAN「2 个单元 · 5」→ /SIM-P1/B1「3 · 3 条直段」→ 构件行（`9001_1011 ELBO · 0.25 m`…，跨房的带「跨 2 房」片）→ 直段行（`BRAN → ELBO 直段 · 141 mm`、无效的带「无效」片）；EQUI「1 个单元 · 2」；结构系统「233」 | ✓ |
| 其他构件 + 超上限补取 | `03-r101-others-pane-leaves.png` | 结构系统 → 其他构件 → SCTN / PANE；/SIM-P1/B2 与 PANE 组的叶子在剧本里不内联，展开时按 `unit=` / `other_noun=` 补回（B2 下 `9001_1021 TEE`、`TEE → REDU`；PANE 下 230 个面板行） | ✓ |
| 右键（单元行） | `04-unit-context-menu.png` | 折叠 ｜ 聚焦 · 隔离 · 取消隔离 ｜ 显示 · 隐藏 · 加载模型（3 个构件）｜ 查看关联房间 · 查看属性 · 复制 REFNO | ✓ |
| 「加载模型」走既有显示链 | `05-load-models-log.png` | 日志「房间树：加载 /SIM-P1/B1 下 3 个构件的模型」→「开始查询并显示模型：3 个目标」→ 范围查空走「默认重新生成」→ 演示模式没有模型服务失败（与模型树眼睛在剧本里的行为一致） | ✓ |
| 点行选中 → 属性 | `06-shift-select.png` | 点 `9001_1013` 后属性面板跟到 `9001_1013 =9001/1013`；点直段行「ELBO → VALV」后属性跟到所属 BRAN `9001_1010`（`inspect tree 9001_1010` 命中属性头） | ✓ |
| 外部选中联动 | `07-external-selection-reveal.png` | 先折叠 /SIM-P1/B1，再在标题栏搜索框输入 `9001/1012` 回车（树定位）：房间树重新展开 B1、选中 `9001_1012` 行；模型树那边照常报「树外元素」 | ✓ |
| 搜索过滤 | `08-filter.png` | 输入「泵」→「匹配 1 / 3 间房」，只剩 R102 · 泵房，框旁出现清空钮 | ✓ |
| 右键（房间行） | `09-room-context-menu.png` | 多「聚焦房间（房间视图）」「显示房间模型」；「加载模型（240 个构件）」 | ✓ |
| > 200 确认框 | `10-load-confirm-dialog.png` | 房间根上点「加载模型」：先补取 PANE 组未内联的叶子，再弹「加载数量较多——「加载模型」（R101 · 控制室）将加载 240 个模型（超过 200 个）…」，按钮「加载 240 个」/「取消」 | ✓ |
| 库供数注释态 | `11-store-face-note.png` | `PLANT_READ_FACE=store` 启动：面板是一句「房间树只在服务供数下有：它由 gen-model 服务端聚合（ADR 0068），库供数没有这一棵。设置 → 供数模式 切回「服务供数」即可」+ 重试 | ✓ |

没有在独立壳里验到的：

- **Ctrl / Shift 多选**：探针 `inspect click … shift+left` 只把修饰键写在指针事件上，不写 egui 的 `RawInput.modifiers`，而两棵树都读 `ui.input(|i| i.modifiers)`
  ——对照组：模型树上 `ctrl+left` 点两行同样只剩单选。区间 / 加减的语义由 `room_tree::tests::selection_resolves_to_scene_refnos_and_tubes_select_their_bran` 覆盖，绘制层与 `tree.rs` 同一段写法。
- **眼睛的显 / 隐在三维里的结果**：剧本没有模型服务，范围查询恒空，eye 停在「未加载」（与模型树同）；显隐 / 聚焦 / 隔离落到的是既有的
  `set_model_visible` / `FocusGroup` / `Isolate`，没有第二条路径。
- **「在房间树查看」**：剧本里元素没有房间归属，「房间」页签不出概要卡，那枚芯片点不到；它的宿主半边（取树、清过滤、展开、滚到）由
  `reveal_only_looks_inside_fetched_rooms_and_expands_the_path` 覆盖。

## 3. 连本机 gen-model（服务供数，`PLANT_MODEL_API_URL=http://127.0.0.1:8023`）

本机 8023 上跑着一份 gen-model（`aios-database.exe`，项目 AvevaMarineSample / ns 1516，库含 7997–8000），`/health` 里 `features.room.status = available`。

- `GET /api/v1/spatial/rooms`：`status = ready`，215 间在册房（面板数分布 0:10 / 1:187 / 2:13 / 3:4 / 5:1）。界面「在册 215 间房」，按自然序
  L505 … L598 → R000 → R101 → R111…；10 间 `panel_count = 0` 的标「空房」不给箭头（`12-live-gen-model-roots.png`）。
- `GET /api/v1/spatial/rooms/{a_b}/tree?tubes=1`：逐间扫过 215 间——205 间 `total_count = 0`，10 间（即空房壳）404。展开 L505 得「0 个构件 · 0 条直段」，
  没有子层就收起箭头。树为空是服务端数据的状态：`room_status.source = durable`、`matched = 0`，而同一盒内 `candidate_count = 1123`、
  `nearby` 全局 21443 个候选——这份库的房间归属边还没算到这些候选上，plant3d-web 对着同一台服务看到的也是空树。
- 因此计划里「同一间房与 plant3d-web 逐行核节点数 / 计数尾巴 / 直段条数」这一条**没有成立的数据**：本机没有一台带非空房间树的 gen-model，
  plant3d-web 仓里也没有留 `verification/spatial-room-hierarchy-tree-*/http/*.json`。协议层的对齐靠两条：真响应（清单 215 间、空树、404）
  全部由同一套 serde 类型解开并上屏；非空树的形状按 plant3d-web `genModelV1Api.ts` 的类型一字不增减地照搬，并用服务端形状的 JSON 单测钉住。
  等有一套房间归属算完的库，补一次逐行核对即可，不改代码。

## 3b. Web 端（阶段四前半；release wasm，headless Chrome / ANGLE D3D11 / WebGL2）

做法：按 `scripts/build-wasm.ps1` 的工具链（zig 给 ring）编 `cargo build -p rs-plant --target wasm32-unknown-unknown --release` + `wasm-bindgen --target web`，
产物放临时目录（**没动仓里的 `web/public/pkg`**，重发产物是单独的 `chore(web)`）；一个临时 node 静态服务器供页面并把 `/api/*` 同源转发到
本机 gen-model 8023；`config.json` 的 `db` 指 ns 1516 / AvevaMarineSample / `/ALL`；Playwright 1.58 驱动安装的 Chrome（视口 1600×1000，DPR 1，
`--use-angle=d3d11`），点页签 / 箭头 / 右键的坐标与 PC 探针同一套逻辑点。

- 浏览器发出的两条请求（Playwright 抓的响应）：
  `200 /api/v1/spatial/rooms?project=AvevaMarineSample&mdb=%2FALL&namespace=1516` → `status=ready rooms=215`；
  `200 /api/v1/spatial/rooms/24381_5062/tree?project=…&tubes=1` → `total=0 tubes=0 room=L505`。身份三格、`tubes=1` 与 PC 端同。
- `web-01-roots.png`：「在册 215 间房」、自然序、空房标记，与 PC 图 12 同。`web-02-room-expanded.png`：L505 展开「0 个构件 · 0 条直段」。
  `web-04-context-menu.png`：房间行右键菜单（聚焦 · 聚焦房间（房间视图）· 显示房间模型 · 隔离 · 取消隔离 ｜ 显示 · 隐藏 · 加载模型（…））。
- 控制台没有 panic / WebGL 错误（只有临时目录里没有的 `e3d.project.ron` / 字体资源 404）。
- 与 PC 端同一句话：这台 gen-model 的房间归属边没算，树为空；非空树的展开、直段行、补叶子走的是同一份契约层代码（见 §2 独立壳）。

## 4. 复跑要点

- 探针坐标 = 截图像素（1600 宽）；从缩过的图上估坐标要换算回去。dock 页签、箭头、眼睛、工具钮是自绘控件，不进 AccessKit 树，
  先 `shot` 再按图点；行（TreeItem）有标签 `room-tree <Kind>; label=…`，`inspect tree room-tree` 能列出当前可见行的包围盒。
  箭头 x ≈ 20 + 12.5 × 深度（逻辑点）。
- 本机另有一份 release 的 `rs-plant.exe` 占着 5721，用专用口（5723 / 5725 / 5727）并按 PID 收尾自己起的那份，不动别人的。
