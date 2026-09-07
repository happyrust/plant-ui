# 供数模式 · 服务供数 / 库供数双轨 · 开发计划

- 日期：2026-09-07
- 提出：用户「plant-ui 的模型树和模型显示分为两种模式：API 模式（经 e3d-io / e3d-model 提供的 API 驱动显示与绘制）
  与原先全部查 SurrealDB 的模式；分析如何实现」
- 范围：`plant-ui-app`（`data.rs` / `main.rs` / `model_update_api.rs` / `settings_store.rs` / `search_index.rs`）、
  `plant-ui`（`settings.rs` / `vm.rs` / `workbench/chrome.rs`）、`plant-ui-data`（只被调用，不改形状）、
  `CONTEXT.md`、`docs/adr/0026`。**`plant-ui-view3d` 零改动；gen-model 零改动。**
- 依据：`CONTEXT.md`「项目接入点 / 模型来源 / 翻面」；ADR-0008（模型服务地址是设置）、ADR-0009（显示时补齐走服务）、
  ADR-0018（切项目重启客户端）、ADR-0021（取回工作清场重装已加载集）、ADR-0024（取回工作先 ensure）、
  ADR-0025（模型来源由服务端按库判决、一次装载一个桶一个源）；gen-model ADR-053 / 058 / 059、
  `docs/specs/web-service-api.md` §4.10–4.12。
- 状态：**已定案**（2026-09-07 grill 两轮，D1–D15 全按推荐项 + 用户追加「下拉框」；Plannotator `approved`）。
  **M0 已完成**（2026-09-07：vendor / Cargo.lock 单提 `12a9d93e9`，工作树落地 `f58580aff`，三 crate 测试
  121+2 / 104 / 18 全绿）；ADR-0026、`CONTEXT.md` 三词条与两份旧计划的追记（M5 文档半边）随本计划一并落下。
  **M1 已完成**（2026-09-07：`crates/plant-ui-app/src/read_face/{mod,service}.rs` 新建，`data.rs` 的读调用全部改经
  `ReadFace`，纯重构；三 crate 测试 121+2 / 109 / 18 全绿，`cargo check --target wasm32-unknown-unknown -p plant-ui-app` 过）。
  **M2a 已完成**（2026-09-07：`read_face/store.rs` 从 v0.1.9 逐字接回九条、`ReadFace::Store` 接上；
  `Settings.read_face` + `resolve_read_face` + `PLANT_READ_FACE`；`ModelInstancesReq` / `SearchOutcome.index_failure`
  两处签名定形；测试 123+2 / 113 / 18，wasm check 过——`PLANT_READ_FACE=store` 已能整机跑库供数，只是还没有界面入口）。
  M2b（设置窗下拉 / 接入点一行 / 热切）未动。

---

## 一、一句话

plant-ui 的读面（模型树 / 属性 / 搜索 / 三维实例 / 工程标识）今天**只有服务供数一条路**（工作树，未提交），
库供数那一半的代码还躺在 `plant-ui-data` 里没人调。本计划把两条路收到**一个 `ReadFace` 缝**后面，
由项目接入点的一格 `read_face` 选一次；默认服务供数，库供数是**保留档**（兼容旧 rocksdb 部署与对拍），
不是对等档。缺的格要说出来，不许安静空着。

---

## 二、现状勘探（2026-09-07 工作树实位，全部核过）

### 2.1 plant-ui 两个版本的读面

分支 `codex/increment-ui-closure`，HEAD = `4ec446f5a`（v0.1.9 release），工作树未提交 +1302 / −211 行。

| 读面 | HEAD（v0.1.9） | 工作树（未提交） | 返回类型 |
|---|---|---|---|
| 树 roots / children / ancestors | `data.rs::direct_tree_enabled()`：`PLANT_TREE_DATA_MODE=db` 走 `plant_ui_data::{site_nodes,child_nodes,ancestor_refnos}`，否则 `/api/v1/tree/*` | 只剩 HTTP（`data.rs:23-33`），开关删除 | `Vec<EleTreeNode>` / `Vec<RefU64>` |
| 属性面板 | `plant_ui_data::element_props`（`pe` / `ATT_*`，`lib.rs:566`） | `model_update_api::element_attributes` → `POST /api/v1/element/attributes`（`model_update_api.rs:275`）；测试 `property_requests_have_no_database_fallback`（`data.rs:1148`）钉死不回退 | `Vec<plant_ui_data::Attr>` |
| 名称定位 `ResolveName` | `plant_ui_data::resolve_name`（`lib.rs:429`） | `search_names` + 精确匹配（`data.rs:649-662`） | `Option<RefU64>` |
| 名称搜索 | 前缀打库 `search_names_by_prefix`（`lib.rs:460`）+ 本地 ngram 子串索引（`search_index.rs` / `name_index.rs`，ADR-0022/0023） | `GET /api/v1/search`（`model_update_api.rs:208`）；ngram 置 `SearchIndexState::Off`（`data.rs:746-751`），代码仍在 | `Vec<NameHit>` + `SubstringHits` |
| 三维实例 | `ensure`（HTTP，ADR-0024）→ `plant_ui_data::model_instances_with_progress` / `model_instances_anc`（`lib.rs:105/130`） | `ensure` → `POST /api/v1/model/records`（`model_update_api.rs:677`，`ModelRecords { records: Vec<GeomInstQuery>, sources }`，ADR-0025 分桶） | **同为 `Vec<GeomInstQuery>`**（view3d `load()` 只认它，`plant-ui-view3d/src/lib.rs:351`） |
| 重新生成清点 | `generated_scope` + `nouns_of`（`lib.rs:355/395`） | `model_records`（`data.rs:476-504`） | `RegenerateCount` |
| 启动 `ready()` | `connect()` → `project_identity()`（读 `MDB` / `CURD` / `WORL`，`lib.rs:517`）→ `site_nodes` | `/health` + `/dbnums` + `/tree/roots`（`data.rs:507-563`）；**启动不连 SurrealDB** | `ReadyInfo` |
| 房间 | SurrealDB（`room.rs`） | 仍 SurrealDB，先看 `/health.mirror.status == ready` 再懒连（`data.rs:35-51`） | — |
| 命令面（ensure / 预览 / 执行 / 队列 / 死信 / 提资 / 命令查询） | HTTP | HTTP | — |
| 网格文件 | 本地 `mesh_dir` 的 `.mesh`（`mesh_source.rs`） | 不变 | — |

**九条读面两边返回类型逐一相同**——缝的代价是一个分派点，不是一层转换。

### 2.2 gen-model 已经给了什么（`src/web_service/mod.rs:318-399`，分支 `e3d-direct`）

`/tree/{roots,children,ancestors}`（e3d-io `DirectTreeService` + 内存骨架，不查 `pe`）、`/search`（快照 NAME 索引）、
`/element/attributes`（e3d-io 直读，CATA 可读、DESI 异步写透）、`/element/ensure`、`/model/ensure`、`/model/records`
（按 dbnum 带 `model_source`）、`/meshes/{hash}.glb`、`/dbnums`（带 `data_face` / `db_type` / `cache_epoch` /
`cached_pe_rows` / `model_source`）、`/health`（带 `data_face`、`mirror` 一节）、`/ws`。**本计划不要 gen-model 再加任何东西。**

### 2.3 文档落后于代码的三处

| 文档 | 它说 | 代码已经 | 本计划处置 |
|---|---|---|---|
| `docs/plans/2026-09-04-kv-mem-out-of-box-adaptation.md` D1 = A′ | 「plant-ui 读面一行不改；不为 kv-mem 单开数据层抽象」 | 读面整条换成 HTTP | 状态行加「读面已由 2026-09-07 计划越过」（M5） |
| `docs/plans/manual-update-and-get-work-on-direct-mode.md` D3 | 「属性面板走 gen-model 直读端点」（M4 等 G4） | 已做（`element_attributes`） | 状态行标 M4 完成于工作树（M5） |
| gen-model `2026-09-05-memory-first-…-merged-plan.md` D6 | 「属性先 ensure 再读库」 | 该文顶部注记已自行更正为「属性直读 e3d-io」 | 不动（那是 gen-model 的账） |

---

## 三、术语（进 `CONTEXT.md`「项目与连接」，随本计划落下）

**供数模式**：一个项目接入点的模型树 / 属性 / 搜索 / 三维实例从哪儿读，只有两种。它是接入点的一格，整个接入点只认一种，
不按读面混；命令面（补齐 / 更新 / 队列）与它无关，永远走模型服务。
_Avoid_：API 模式、数据库模式（与「模型来源：数据库」撞）、数据源、直读 / direct（那是 gen-model 自己读文件的词）

**服务供数**：经模型服务的 HTTP 接口读，底下是 e3d-io（数据）与 e3d-model（几何）。出厂默认。
_Avoid_：API 模式、走服务、在线模式

**库供数**：直连 SurrealDB 的 `pe` / `pe_owner` / `ATT_*` / `inst_relate` 读。保留档：给已落盘的 rocksdb 部署、
老版本模型服务与对拍用；服务供数有而它缺的格要标出来，不许安静空着。
_Avoid_：数据库模式、SurrealDB 模式、离线模式、旧模式

「改供数模式」不叫翻面——翻面是服务端按库的模型来源在数据库 / 内存之间换。

---

## 四、已拍板的决定（2026-09-07，两轮 grill）

| # | 题 | 定案 | 被否的备选 |
|---|---|---|---|
| D1 | 库供数留下来干什么 | **兼容档 + 对拍档**；默认服务供数；新读面只保证服务供数有 | 对等双轨（每个新读面做两遍） |
| D2 | 开关落哪、谁定 | **接入点一格 `read_face`** + 环境变量开发期压过 + **设置窗下拉框**（用户追加）；**不自动探测** | 客户端按 `/health` 自动退回库（= 同一视口两版数据悄悄拼在一起，ADR-0025 刚为模型面挡过） |
| D3 | 库供数纯到什么程度 | **读面走库、命令面照旧 HTTP**（= v0.1.9 形状）；库供数精确地等于「读面直连库」 | 纯库零 HTTP（没有 ensure，回到 ADR-0009 之前「未生成就安静不画」） |
| D4 | 代码上的缝 | **一个 `ReadFace` 类型、两个实现**，在 `data::spawn` 与热切那一拍选一次；`Req/Evt` 桥不动 | 每个函数里 `if mode`（HEAD 形状）；两套 bridge |
| D5 | 服务供数下网格从哪来 | **两种模式都保持本地 `mesh_dir`**；`/meshes/{hash}.glb` 单列后续 ADR（那是「远程客户端」不是「供数模式」） | 服务供数改走 glb + 磁盘缓存 |
| D6 | 房间 | **不动**：只有库一条，服务供数下照旧 `mirror` 门控；写进不做清单 | 顺带给房间开服务供数（gen-model 要新端点） |
| D7 | 未提交工作树 | **先原样落地提交**（服务供数成为唯一读面），再开缝、从 HEAD 接回库供数 | 不提交直接重构 |
| D8 | 两条路对拍 | **要**：树 + 属性必做，实例集合按 dbnum 桶比；差异只出报告不判对错（两边时点可能不同） | — |
| D9 | ADR | **ADR-0026** + `CONTEXT.md` 三词条 | — |
| D10 | 下拉框放哪、换了之后 | **设置窗、模型服务地址下面**；**热切** = `Reconnect` 那条路（排干在途 → 丢缓存 → 重跑 `ready()`）+ 一次取回工作（清场重装已加载集，ADR-0021）；三维不许留旧供数方的几何 | 标题栏常驻；像切项目一样重启（ADR-0018） |
| D11 | 落盘字面与环境变量 | `Settings.read_face: "service" \| "store"`，`serde(default)` = `service`；`PLANT_READ_FACE` 压过设置；在场时下拉框**禁用并标「由 PLANT_READ_FACE 压过」**；住 exe 旁 `settings.ron`（按机器，不进项目 bundle）；**`PLANT_TREE_DATA_MODE` 直接不认** | 认旧变量一个版本 |
| D12 | 库供数的 `ready()` 等不等 gen-model | **不阻断**：`connect()` → `project_identity()` → `site_nodes`；`/health` `/dbnums` 失败只让队列面板报「模型服务离线」；两边都在场而 project/mdb/ns 不一致 → 命令行视图警告一句 | 仍算启动失败 |
| D13 | 库供数下的搜索 | **原样接回 HEAD**：前缀打库 + ngram 子串索引；服务供数维持 `Off` | 只保前缀、ngram 退役；库供数借 `/search`（按读面混，D2 禁） |
| D14 | 对拍探针放哪 | **住 `plant-ui-app`**，无头跑；见 §5.4 的实现注记（bin crate 没有 lib target，做成主程序的无头子命令） | `plant-ui-data/tests/` env 门控；命令行视图 `parity` 命令（客户端同时持两个读面，D2 要挡的形状） |
| D15 | 里程碑与首次可发布点 | M0–M6（§六）；**首次可发布点在 M3 之后**；M4 / M5 可与 M3 并行 | M2 之后即发 |

### 默认清单（grill 时一并确认）

1. `ReadFace` 九条：`identity` / `sites` / `children` / `ancestors` / `props` / `resolve_name` / `search` /
   `model_instances(roots, progress)` / `regeneration_count(targets)`，外加 `invalidate()`。库供数的 `model_instances` =
   `ensure`（HTTP，失败不阻断，ADR-0024）→ `model_instances_with_progress`；服务供数 = `ensure` → `/model/records`。房间不进。
2. 库供数下要标出来的格：库行「模型来源」整格不画（不走 `/model/records` 就没有那一格，ADR-0025 允许 `None`）；
   CATA 元素属性面板给定论「元件库元素不入模型本体库；切到服务供数可看」（判据：refno 的 dbnum 落在 `/dbnums` 的
   CATA 行里；gen-model 不在场时退到「属性为空」）；搜索下拉标「只覆盖已入库元素」。服务供数下房间照旧由 `mirror` 门控。
3. 接入点面板（`AccessPointVm`）多一行「供数：服务 / 库」；状态栏不动。
4. wasm 端同一套代码，不特殊处理（`connect()` 本来就有 wasm 分支；网格照旧走站点 `meshes/`）。
5. `ReadFace` 的选择只在 `data::spawn` 与热切那一拍发生；数据线程内不许根据错误自行换面（源码钉一条）。
6. `CONTEXT.md` 三词条如 §三。

---

## 五、设计

### 5.1 `ReadFace`：一个类型、两个实现、一个分派点

新模块 `crates/plant-ui-app/src/read_face/{mod.rs, service.rs, store.rs}`。

```rust
/// 供数模式的读面。房间不在这里（只有库一条，ADR-0026）；命令面也不在这里。
pub enum ReadFace {
    Service(ServiceReadFace),   // 包 model_update_api::{tree_*, search_names, element_attributes, model_records, service_health, dbnum_report}
    Store(StoreReadFace),       // 包 plant_ui_data::{site_nodes, child_nodes, ancestor_refnos, element_props, resolve_name, search_names_by_prefix, model_instances_with_progress, generated_scope, nouns_of, project_identity, invalidate_all}
}

impl ReadFace {
    pub fn new(kind: ReadFaceKind) -> Self;                               // 只在 data::spawn 与热切那一拍调
    pub fn kind(&self) -> ReadFaceKind;                                   // Service | Store
    pub async fn identity(&self) -> anyhow::Result<Identity>;             // project / mdb / ns / db_nums / cache_versions；ready() 拿它 + sites() 拼 ReadyInfo
    pub async fn sites(&self) -> anyhow::Result<Vec<EleTreeNode>>;
    pub async fn children(&self, refno: RefU64) -> anyhow::Result<Vec<EleTreeNode>>;
    pub async fn ancestors(&self, refno: RefU64) -> anyhow::Result<Vec<RefU64>>;
    pub async fn props(&self, refno: RefU64, scope: &Scope) -> anyhow::Result<Vec<Attr>>;
    pub async fn resolve_name(&self, name: &str) -> anyhow::Result<Option<RefU64>>;
    pub async fn search(&self, query: &str, limit: usize, index: &SearchIndex) -> SearchOutcome;   // prefix: Result<Vec<NameHit>>, substring: SubstringHits, index_failure: Option<String>
    pub async fn refresh_search_index(&self, index: SearchIndex, scope: Scope, force: bool, evt_tx: mpsc::Sender<Evt>, ctx: egui::Context);  // 状态经 Evt::SearchIndex 出去；服务供数恒报 Off
    pub async fn model_instances(&self, req: &ModelInstancesReq<'_>, progress: Progress<'_>) -> anyhow::Result<ModelRecords>;  // 只读；ensure 留在调用方
    pub async fn regeneration_count(&self, targets: &[RefU64], delivery_units: &[String]) -> anyhow::Result<RegenerateCount>;
    pub async fn invalidate(&self);
}
```

`ServiceIdentity<'_>` 是随 `Req` 捎下来的服务身份四格（`base` / `project` / `mdb` / `namespace`，宿主的设置项，
数据线程不认识），库供数不看它；`Progress<'_>` = `&mut (dyn FnMut(usize, usize) + Send)`，服务供数整批一次回不报进度，
库供数按根报。

**`ModelInstancesReq { roots, generation_roots, identity }`（M2a 定形）**：两面要的根不一样，所以两份都带、各取各的。
`roots` 是调用方自己的根（取回工作重装 = 快照里的模型 refno；eye 显示 = 点下去的树目标），**库供数只查它**
——v0.1.9 原样，`inst_relate.anc CONTAINS $root` 一根一条，拿并集去跑会把嵌套在 `roots` 底下的生成根数两遍。
`generation_roots` 是**服务供数要查的名单**，调用方按 `ensure` 回执算好（重装 = `roots` ∪ 回执根；eye = 回执根，
回执空则退到目标本身），与工作树落地时 `/model/records` 的分桶口径一字不差。

`SearchOutcome.index_failure`：库供数下子串索引**查询本身炸了**（不是「没就绪」）要作为 `Evt::SearchIndex(Failed)`
说出去，v0.1.9 在 `handle_read` 里直接发；读面没有 `evt_tx`，所以放进返回值由 `data.rs` 转发。服务供数恒 `None`。

各面的方法只收自己用得上的参数（`ServiceReadFace::props(refno, scope)` / `StoreReadFace::props(refno)`、
`ServiceReadFace::search(query, limit)` / `StoreReadFace::search(query, limit, index)` …），enum 那一层是唯一的适配点。

- **为什么是 enum 不是 `dyn Trait`**：恰好两个变体，穷尽匹配；`async fn` 直接写，不用 `async-trait`；
  原生端 future 要 `Send`、wasm 端不 `Send`（`data.rs:568-580` 已经为此分了两套 `InflightQuery`），
  enum 让两端各自成立，`dyn` + `async fn` 做不到这一点。将来真要第三个实现再谈 trait。
- `ModelRecords` 是公共返回形状：库供数填 `records`，`sources` 留空（= 服务端没说，ADR-0025 的 `None` 语义，
  库行那一格因此整格不画，默认清单 ②）。
- `data.rs` 的 `handle_read` / `get_work` / `ready` / `count_regeneration` / 模型通道两处只改调用对象。
  **源码钉**（M1 验收）：`data.rs` 里不再直接出现 `model_update_api::{tree_roots,tree_children,tree_ancestors,search_names,element_attributes,model_records,dbnum_report}` 与
  `plant_ui_data::{site_nodes,child_nodes,ancestor_refnos,element_props,resolve_name,search_names_by_prefix,model_instances*,generated_scope,nouns_of,project_identity}` 的调用；
  `ensure_model` 例外（命令面）。
- 数据线程持有 `ReadFace`（不是 `Arc<dyn>`），随 `Req::SwitchReadFace` 整体替换；模型通道（`model_worker`）
  与交互通道各持一份，同一拍一起换（两条通道都由 `spawn` 起，换面消息两条都发）。

### 5.2 设置、环境变量、下拉框

- `plant_ui::settings::Settings` 加 `read_face: ReadFaceKind`（`#[serde(default)]`，`Default` = `Service`，
  serde 字面 `"service"` / `"store"`），与 `model_api_url` 一样住 `settings_store`（exe 旁 `settings.ron`）。
  **`ReadFaceKind` 本身住 `plant_ui::settings`**（绘制 crate：设置窗要画它、接入点面板要报它），
  `plant-ui-app::read_face` 只是 `pub use`；`label()` 给「服务供数 / 库供数」两处共用。
- 解析优先级（`settings_store::resolve_read_face(configured, env) -> ResolvedReadFace { kind, overridden, warning }`，
  纯函数）：`PLANT_READ_FACE` 认得出 → 用它并记 `overridden = true`；认不出 → `warning` 出声一次、按设置值；
  未设置 → 设置值。**不认 `PLANT_TREE_DATA_MODE`**。它与网格目录那条**相反**（环境变量压过设置）：这一格是开发期
  临时切面用的，进程一起就该说了算。结果放进 `settings_store::Startup.read_face`，`App::new` 据此造读面、
  存 `App.read_face`；浏览器端没交底 = `Service`。启动日志「已连接 …」尾巴上带当前供数模式。
- 设置窗（`settings.rs::show`）在「模型服务地址」下面加一格下拉「供数模式：服务供数 / 库供数」；`overridden` 时
  禁用 + 一句「由 PLANT_READ_FACE 压过」。保存路径与今天三格同一条（`main.rs` 收到 `saved` → `persist_settings`）。
- 接入点面板 `AccessPointVm` 加 `read_face: ReadFaceKind`，绘制层一行「供数：服务 / 库」。

### 5.3 热切

`main.rs` 收到 `saved.read_face != self.read_face`：

1. 拍快照：与取回工作同一份（已加载集 + 范围目标 + 显隐方向，ADR-0021 / ADR-0024 的 `model_reload_restore`）。
2. 清场：`ModelAction::Unload` 全部、`TreeModel` 归零、选中清空、属性面板清空、搜索下拉清空。
3. 发 `Req::SwitchReadFace(kind)`。数据线程按「全局手术」处理（与 `Reconnect` 同席，`data.rs:1065`）：
   排干在途 → 旧面 `invalidate()` → 换面 → 新面 `identity()` + `sites()` → `Evt::Ready`。
4. `Evt::Ready` 到了之后走取回工作那条路（`Req::GetWork` + `Req::Models { ensure_targets, debt_reload: true }`）
   把快照重装回来。相机不动。
5. 命令行视图一条日志：「供数模式：服务 → 库；已清场并重装 N 个已加载范围」。
6. 换面期间下拉框禁用；`Evt::Ready` 失败则界面停在未连接态、下拉框可再改回去（与连库失败后「重试」同形）。

不重启客户端的理由：接入点没变，只换了一条读路；两条既有路（`Reconnect` + 取回工作）拼起来正是它。

### 5.4 对拍探针

`plant-ui-app` 是 bin crate、没有 lib target，`model_update_api.rs` 又依赖 `plant_ui` 的类型，搬进 `plant-ui-data`
会让数据 crate 反过来依赖绘制 crate。所以探针做成**主程序的无头子命令**：

```
plant-ui-app --read-face-parity --depth 2 --sample 200 [--roots 24381/2,…] [--out report.md]
```

- 两个 `ReadFace` 各起一份（这是**唯一**允许两面并存的进程形态，且不进 UI）。
- 树：roots 集合 diff；逐层 children 按 refno 集合 + **原序**比（成员序是语义，gen-model spec 6.5.1）。
- 属性：抽样 refno 逐字段比，四档：相同 / 只差首尾空白 / 只差括号空格 / 不同（照 gen-model
  `direct-mode-expression-dialect.md` 的口径），CATA 元素单独列（库供数必空）。
- 实例：抽样根 `model_instances` 后按 dbnum 桶比 `refno + geo_hash` 集合。
- 报告只列差异与计数，不判对错——服务读文件最新、库读水位，两边时点可以不同，报告头写出两枚凭证
  （`/dbnums` 的 `file_latest_sesno` / `applied_sesno`）。
- 不进 CI（要活的 gen-model + SurrealDB）；`scripts/` 加一条一键跑法。

### 5.5 库供数下不说谎（M3）

| 格 | 服务供数 | 库供数 |
|---|---|---|
| 库行「模型来源」 | 服务端给什么画什么（ADR-0025） | `sources` 为空 → 整格不画（不猜「数据库」） |
| CATA 元素属性 | e3d-io 直读，有值 | 空 → 定论「元件库元素不入模型本体库；切到服务供数可看」；gen-model 不在场退到「属性为空」 |
| 搜索下拉 | 全库快照索引 | 前缀 + ngram；空结果标「只覆盖已入库元素」 |
| 房间 | `mirror` 门控 | 直连（今天的路） |
| 队列面板 | — | gen-model 不在场 → 「模型服务离线」，树 / 属性 / 三维照常 |
| 设置窗下拉 | 环境变量在场 → 禁用 + 标注 | 同 |

---

## 六、里程碑

| 步 | 内容 | 依赖 | 验收 / 测试 |
|---|---|---|---|
| ✅ **M0** | 落地工作树：13 个已改文件（`CONTEXT.md` 只取模型来源 / 翻面那一段）+ 未跟踪的 `model_record_union.rs` / `source_versions.rs` / `fixtures/` / 两条 live 测试 / `docs/adr/0025-*.md` / `docs/plans/2026-09-04-kv-mem-*.md`，一次提交「服务供数成为唯一读面」（**`f58580aff`**）；`vendor/registry/ordered-float` 与 `vendor/rs-core` + `Cargo.lock` **单独**一提（**`12a9d93e9`**，它们不是本仓的读面） | 已核：13 个文件最后写入 09-07 11:09，之后无人动 | **已过**：`cargo test -p plant-ui -p plant-ui-app -p plant-ui-data` 全绿，基线 plant-ui 121（+2 集成）/ plant-ui-app 104 / plant-ui-data 18，增量编译 23.7 s；日志 / `.codex-*` / 截图 / `web/public/assets/meshes`（667 MB）均未入库 |
| ✅ **M1** | `read_face/{mod,service}.rs` + `ReadFace::Service` 接进 `data.rs` 五处（`ready` / `get_work` / `handle_read` / 模型通道两处）；纯重构 | M0 | **已过**（2026-09-07）：既有测试一个不少，`cargo test -p plant-ui -p plant-ui-app -p plant-ui-data` = 121+2 / 109 / 18；净增 5 条——源码钉 `data_rs_reads_only_through_read_face`（§5.1）与 `service_face_never_touches_the_store`、`desi_dbnums` 纯函数两条（读透含 ISOD 带 `cache_versions` / 摄入只 DESI）、`a_face_reports_the_kind_it_was_built_from`；`property_requests_have_no_database_fallback` 改名 `property_requests_go_through_the_read_face`，钉 `face.props(`；`cargo check --target wasm32-unknown-unknown -p plant-ui-app` 过（`Arc<ReadFace>` 与 `Progress` 的 `Send` 在 `LocalBoxFuture` 下同样成立） |
| **M2** | `read_face/store.rs`（从 HEAD `git show 4ec446f5a:crates/plant-ui-app/src/data.rs` 接回九条）；`Settings.read_face` + `resolve_read_face` + `PLANT_READ_FACE`；设置窗下拉；`AccessPointVm` 一行；`Req::SwitchReadFace` 热切（§5.3） | M1 | 单测：`resolve_read_face` 三态（设置 / 环境变量压过 / 认不出出声）；`the_default_read_face_is_service`；`old_settings_without_read_face_are_service`；`a_switch_drains_inflight_before_swapping`；`a_switch_clears_the_scene_and_reloads_the_snapshot`（沿 `data.rs` 既有的 `route_model_load` 测试形状）；设置窗 8 帧高度测试补一档（`settings.rs::window_heights` 同款） |
| **M3** | 不说谎五格（§5.5）；命令行视图切换日志 | M2 | `a_catalogue_element_in_store_mode_gets_a_verdict`、`store_mode_search_names_its_coverage`、`store_mode_never_paints_a_model_source`、`an_overridden_read_face_disables_the_dropdown`、`a_missing_model_service_in_store_mode_does_not_block_ready` |
| **M4** | 对拍探针（§5.4）+ `scripts/Run-ReadFaceParity.ps1` | M2 | AvevaMarineSample `depth 2 / sample 200` 出一份报告存 `docs/evidence/2026-09-xx-read-face-parity.md`；树 roots 集合与原序零差异是硬标准，属性四档计数入档 |
| **M5** | ADR-0026、`CONTEXT.md` 三词条、09-02 / 09-04 两份计划状态行追记（§2.3）——**这三样已随本计划落下**；剩 `CHANGELOG.md` 用户可见条目（等 M2 有东西可说时写） | — | 文档互引核对：ADR-0026 ↔ 本计划 ↔ `CONTEXT.md` 三处名字一致 |
| **M6** | 实机验收（§七） | M3、M4 | 记进 `docs/2026-08-12_live-test-ledger.md` 同款格式 |

**顺序**：M0 → M1 → M2 → M3；M4 / M5 与 M3 并行；M6 收尾。**首次可发布点在 M3 之后**（D15）。

---

## 七、实机验收（AvevaMarineSample，两套接入点各一遍）

服务供数：gen-model `spawned-mem` + `read-through`、`watch_dbnums = 7997,7999`；库供数：gen-model `external` + `ingest` 对着
已落盘 rocksdb。

1. **默认启动** = 服务供数：`settings.ron` 无 `read_face` 键；接入点面板显示「供数：服务」；启动全程 `plant_ui_data::connect` 不被调用（日志钉）。
2. **设置窗切到库供数并保存**：清场 → 重装；已加载集一个不少、相机不动；接入点面板翻成「供数：库」；日志一句。
3. **库供数下点一个 CATA 元素**：属性面板是定论文案不是空表；切回服务供数再点，属性齐活。
4. **库供数下搜索**：前缀命中；子串命中来自 ngram（索引状态不是 `Off`）；空结果带「只覆盖已入库元素」。
5. **库供数下关掉 gen-model**：树可展开、属性可看、已生成模型可装；队列面板「模型服务离线」；点未生成元素的眼睛 → 失败出声、不崩。
6. **`PLANT_READ_FACE=store` 启动**：下拉禁用并标注；改设置无效且界面说明为什么。
7. **对拍**：M4 报告 roots 与原序零差异；属性四档计数；实例按桶差异只在两枚凭证不同的库上出现。
8. **wasm 构建**跑一遍 1–2（网格从站点 `meshes/`）。

---

## 八、风险与不做

- **两条通道要同一拍换面**：交互通道与模型通道各持一份 `ReadFace`（§5.1），漏换一条就是「树是库、三维是服务」——
  正是 D2 禁的形状。`a_switch_swaps_both_lanes` 钉它。
- **库供数的时点是水位、服务供数是文件最新**：同一个元素两边属性可能不同，这不是缺陷；对拍报告头写两枚凭证，
  界面上库供数不额外标时点（ADR-0019 的措辞留给队列面板）。
- **HEAD 接回来的九条要对着 4ec446f5a 逐字比**，别顺手「改进」——那是保留档，改了就没有对拍的参照。
- **`.codex-*` 与 `_*.log` 不入库**（M0）；`vendor/` 两处改动来路不明（`ordered-float` / `rs-core inst.rs`），M0 单提并写清。
- **不做**：房间的服务供数（D6）；网格走 HTTP（D5，另立 ADR）；自动探测退回（D2）；认 `PLANT_TREE_DATA_MODE`（D11）；
  动 `plant-ui-view3d`；改 gen-model；对等双轨（D1）。
