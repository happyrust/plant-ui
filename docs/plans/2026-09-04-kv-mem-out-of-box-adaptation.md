# plant-ui 适配 gen-model「kv-mem 开箱即用」形态 · 勘探结论与开发计划

- 日期：2026-09-04
- 提出：用户「默认使用 kv-mem 模式，不解析任何数据，直接由 e3d-io 解析 + e3d-model 实时生成，
  把 pe、属性与模型数据存进 kv-mem，做到开箱即用；让 plant-ui 来适配这种模式」
- 范围：`plant-ui-app`（`data.rs` / `main.rs` / `model_update_api.rs` / `startup.rs`）、
  `plant-ui-data`（整 crate 的 SurrealDB 读面）、`plant-ui`（`task_queue.rs` / `vm.rs`）。
  gen-model 侧前置单列（§5），**不在本仓施工**。
- 状态：**D1 已定案（2026-09-04 用户拍板：选 A′）**。gen-model 在内存形态下自己拉起一个 memory 后端的
  surreal 子进程，绑到配置里那对 `v_ip:v_port`；**plant-ui 的读面一行不改**。gen-model 侧的施工计划另出一份：
  `gen-model/docs/plans/2026-09-04-spawned-memory-store-out-of-box.md`。
  本仓剩下的活是 §3 的 N4 / N5 / N6 / N7 四条**冷库诚实性**——它们与 D1 无关，A′ 之后一条不少。

---

## 0. 一句话

**没有适配。** 而且分两种情形，坏法完全不同：

- **进程内嵌 kv-mem**（`in_memory_db = true`，即 gen-model 今天那个开关）：plant-ui **连启动都过不去**。
  `data.rs::ready()` 第一步就是 `plant_ui_data::connect()` → `aios_core::init_surreal()` 连
  `v_ip:v_port`；库在 gen-model 进程里，外面**没有端口**。这不是「某个面板空了」，是三步启动序列
  第一步失败，整个外壳停在未连接态。
- **外置 memory 后端 surreal**（端口还在，`Start-Surreal8009.ps1 -Memory`）：plant-ui **能启动**，
  模型与树这两条主路今天就通；但有 6 处会踩空，全部来自同一个新事实——
  **库不再是「早就解析好的」，而是每次开机从零灌、且分两条链异步灌**。

gen-model 自己的配置注释早就把这条边界写死了（`options.rs:539-543`）：
「要留证据或要让别的进程读库，就别开 `in_memory_db`——那时该用的是部署脚本的 `-InMemory`」。
**plant-ui 就是「别的进程」。** 所以第一件要拍的事不是改哪个文件，是选哪一种 kv-mem（§4 D1）。

---

## 1. 名词先对齐：两种 kv-mem 不是一回事

| | A · 进程内嵌 kv-mem | B · 外置 memory 后端 |
|---|---|---|
| 开关 | `DbOption.toml: in_memory_db = true` / `AIOS_IN_MEMORY_DB=1` | `scripts/Start-Surreal8009.ps1 -Memory` |
| 引擎 | gen-model 进程里 `SUL_DB.connect("mem://")`（`lib.rs:1223`） | 独立 surreal 进程，`--memory`，仍监听 8009 |
| 端口 | **没有**。`/health.sul_db.endpoint` 报 `"embedded:mem"`（`handlers.rs:223`） | 有，`v_ip:v_port` 照旧 |
| 谁能读 | 只有 gen-model 自己 | 任何 WS 客户端，包括 plant-ui |
| 进程退出 | 整库消失 | 整库消失（surreal 进程退出时） |
| 实例锁 | 不拿（`lib.rs:167`），因为两个实例不共享库 | 照拿 |
| 已实跑 | ✅ 7997：GENERATE 29.2s / PERSIST 22.1s / READBACK 47475+4623（changelog 2026-09-04） | 常规路径 |

两者对 gen-model 是同一句话（「介质换掉，其余照走」），对 plant-ui 是「能不能活」的分水岭。
下文凡说「kv-mem」而不带 A / B，指的是共同点：**冷库、易失、零预解析**。

---

## 2. 现状勘探（全部为 2026-09-04 工作树实位）

### 2.1 gen-model 这一侧，今天已经站住的

| 事实 | 位置 |
|---|---|
| `direct`（e3d-io 直读）**已是默认读模式**，只有显式 `AIOS_DATA_READ_MODE=db` 才退回 legacy | ADR-058 状态行（2026-09-04 10:27） |
| 启动 = 一次退化窗口：首次纳入的 DESI 库走 `stream_baseline` + `render_baseline_batches`，与稳态窗口共用同一份行渲染 | ADR-058 D3 实施注记 |
| 持久层里只剩两样：**模型面自己的状态**（`gen_root` / 产物行 / mesh / `ref_rev`）与**增量摄入的 DESI 属性数据**（`pe` / `ATT_*`） | ADR-058 D1 |
| **CATA 永不进持久层**，模型面经 `E3dDbResolver` 从文件读 | ADR-058 D1 / 新不变量 S3 |
| 模型面**不等** `data_ready`，两条链并行 | ADR-058 D7 |
| e3d-model 生成的产物已由 `model_db_adapter::persist_root` 落库，接在编排层 `generate_roots_report` 之后 | changelog 2026-09-04 |
| 产物行带 `anc` 打包祖先链、带 `aabb_d` / `world_trans_d` / `insts_flat` 三个行内副本 → plant-ui 平表快路**一行都不用退回 slim** | changelog 2026-09-04；`model_db_adapter.rs:596` |
| 模型行 id 去前缀；本地变换从 `trans` 行挪到 `geo_relate.transform` | ADR-060（row-ids）；changelog 2026-09-04 |
| `sync_live` 不再被 `direct` 钉死，watcher 起得来 | ADR-058 D8 / 09-04 S5-1～3（**未 live**） |

### 2.2 plant-ui 这一侧，今天读什么

| 读面 | 走哪 | 打哪张表 | kv-mem 下如何 |
|---|---|---|---|
| 启动 `ready()`（`data.rs:497-509`） | **SurrealDB** | `connect()` + `query_mdb_db_nums(DESI)` | A：连不上，整个外壳起不来 |
| 模型树（roots / children / ancestors） | **HTTP**（`data.rs:24-57`，默认 direct） | — | ✅ 已经适配 |
| 三维模型实例（`model_instances_anc`） | **SurrealDB** | `inst_relate.anc` + `query_insts_flat` / `query_tubi_insts_by_brans` + `geo_relate` | B：通（2.1 末四行就是为它做的）；A：读不到 |
| `anc` 就绪探测（响亮失败） | **SurrealDB** | `inst_relate` 抽样（`inst.rs:627`） | **冷库空表判 `true`**，正好不误报，但也就到此为止 |
| 属性面板（`element_props`，`lib.rs:550`） | **SurrealDB** | `pe` / `ATT_*`（`get_ui_named_attmap`） | 冷库期为空；CATA 元素**永远**为空 |
| 名称前缀搜索（`lib.rs:447`） | **SurrealDB** | `pe.name` 范围扫 | 冷库期查不到；CATA 名字永远查不到 |
| 子串搜索的 ngram 语料与陈旧戳（`name_index.rs:167/170/304`） | **SurrealDB** | `pe` 全量 + **`dbnum_watermark`** | 戳表在零解析部署里是空的；每次开机都要重建整份索引 |
| 房间（`room.rs:387-447`） | **SurrealDB** | `pe_owner` / `room_relate` / `room_panel_relate` / `model_update_pending` / `pe` | 依赖模型产物与空间树，排在最后；冷库期整片空 |
| 重新生成取材（`generated_scope`、`nouns_of`） | **SurrealDB** | `inst_relate.anc` / `pe` | `nouns_of` 在冷库期缺行 → 生成根归并少算 |
| 队列 / 水位 / 健康 | **HTTP** | — | ✅ 已经适配（M3a 已退役 `dbnum_watermark` 直读） |
| 网格文件 | 文件系统 | `PLANT_MESH_DIR` / 设置窗 | 与库无关，只要指到 gen-model 的 `meshes_path` |

### 2.3 gen-model 的 HTTP 面上今天**没有**什么

`web_service/mod.rs:296-357` 全表 22 条：health / update\* / tasks / model\*（ensure、subtree、history、rebuild）/
query / tree\* / dbnums\* / trace / error-log / batch-failures / queue\* / ws。

**没有**：属性、模型实例、名称搜索、房间。

`/api/v1/query` 那 12 个 `e3d.*` 工具（`query_service.rs:24-37`）**不能顶替**：它们经 `E3dDriver` 驱动
E3D TTY 取真值，是对拍取证面，不是生产读面；`e3d.element.attributes` 默认只给 name / type / owner / position 四格。

---

## 3. 缺口清单（按「用户会先撞上哪一个」排）

| # | 缺口 | 只在 A 出现？ | 后果 |
|---|---|---|---|
| N1 | 启动序列硬依赖 SurrealDB 连接 | **A 独有** | 外壳起不来。没有第二条路 |
| N2 | 属性面板、名称搜索、房间、`nouns_of` 全走 SurrealDB | A 独有（B 下能读） | A 下这四块要各开一个 HTTP 端点，其中属性那条是 G4，早在 09-02 计划里就列为 gen-model 前置且**至今未做** |
| N3 | 三维实例查询走 SurrealDB，且是「整个界面里最贵的一次查询」 | A 独有 | A 下要把 47k 行的投影搬上 HTTP。**这是 A 方案里最硬的一块**，不是加个 handler 就完 |
| N4 | **冷库期没有任何界面语言** | A、B 都有 | 开机后头一分钟：树有了（走文件）、属性空的、搜索查不到、三维在长出来。今天 plant-ui 把这些一律当「查空 = 没有」，不会说「还在灌」 |
| N5 | **CATA 元素在库里永远没有 `pe` 行**（ADR-058 S3 是不变量，不是过渡态） | A、B 都有 | 点开一个元件库元素：属性面板空、名称搜索搜不到、`nouns_of` 缺行。**这不是 bug，是新架构的既定边界**，界面必须自己说清楚 |
| N6 | `name_index` 的陈旧戳读 `dbnum_watermark`（零解析部署里为空） | A、B 都有 | 每次开机都判「戳变了」重建整份 ngram 索引；易失库下这其实是对的，但代价没人度量过 |
| N7 | `Health` 不解 `in_memory_db` / `sul_db.endpoint` / `initialization` 三个键 | A、B 都有 | 界面无从分辨「服务端是内存形态」「库还在灌」——它们**服务端早就给了**（`handlers.rs:223/454`、`/health.initialization.startup`） |

**N4 + N5 + N7 是两种方案共有的**，而且是「界面不说谎」这条老纪律的直接延伸——先做它们，不依赖 D1。

---

## 4. 要拍板的决定

### D1（阻塞全部）· 用哪一种 kv-mem —— **已定案：A′**

| 选项 | plant-ui 工作量 | 换来什么 | 代价 |
|---|---|---|---|
| B · 外置 memory 后端 | 小：只做 §3 的 N4 / N5 / N6 / N7 | 零预解析、零 rocksdb 落盘 | 人要自己先起 surreal，多一条命令 |
| A · 进程内嵌 kv-mem | **大**：N1 + N2 + N3，等于把 plant-ui 的 SurrealDB 读面整条换成 HTTP，还要 gen-model 新开 4 类端点（属性 / 实例 / 搜索 / 房间） | 真的只剩一个后端进程 | N3 那条 47k 行投影上 HTTP 的性能与分块协议要从零设计；`/health` 探针、`rvm_verify`、取证脚本全部够不着库（`options.rs:540`） |
| **A′ · gen-model 自己拉起 memory 后端的 surreal 子进程**（**选定**） | **读面零改动**（等价于 B，但对用户只有一条命令） | 「开箱即用」解决在**部署层**，不在协议层：协议、读口、取证工具、`/sql` 探针全部照旧可用 | gen-model 要管子进程的生命周期、端口冲突与孤儿进程 |

**为什么是 A′**：用户要的「开箱即用」= 不预解析、不落盘、起来就能用 + **一条命令**。
A′ 三条全中，而 A 额外买到的只是「进程数 2 → 1」，代价是重写 plant-ui 的整个读面。
**注意 A′ 不等于「plant-ui 没活」**：它省掉的是 N1 / N2 / N3（架构级改动），
N4 / N5 / N6 / N7 那四条冷库诚实性一条不少——库仍然每次开机从零灌、CATA 仍然永不入库。

### D2 · 冷库期界面怎么说话（N4）

- **推荐**：新增一条**灌库进度**读面，来源 `/health.initialization`（服务端已有 `StartupCoverage` /
  `StartupReconcile`）。属性 / 搜索 / 房间三处在 `data_ready == false` 时，空结果一律显示
  「数据仍在装载（第 N / M 库）」而不是「无」。
- 备选：只在标题栏挂一条全局横幅，各面板不改。**不推荐**——「查空」与「还没灌到」在面板上长得一样，
  正是 CONTEXT.md 反复禁止的那种说谎。

### D3 · CATA 元素怎么说（N5）

- **推荐**：属性面板对「refno 属于 CATA 库」的元素显示一句定论
  「元件库元素不入模型本体库；属性请在 E3D 中查看」，而不是空表。判据从 `/dbnums` 的库类型来。
- 备选：给 CATA 也开一条直读端点（gen-model 侧 `E3dDbResolver` 本来就能读文件）。
  代价是把 ADR-058 D1 的边界从「不入库」变成「不入库但有读面」——**要 gen-model 拍板，不是 plant-ui 的账**。

### D4 · 易失库下 ngram 索引怎么办（N6）

- **推荐**：陈旧戳改从 `/api/v1/dbnums` 的 `applied_sesno` / `file_latest_sesno` 算（M3a 已经为水位提示
  铺过同一条路），`dbnum_watermark` 直读退役；索引落磁盘缓存按 `(项目, MDB, 各库 applied_sesno)` 取键，
  库虽易失、索引可复用。
- 备选：每次开机全量重建。简单，但 AMS 量级的语料是 6.4 MB / 878 万行级别，得先量一次。

---

## 5. 施工计划（D1 = A′：本仓只剩冷库诚实性四条）

### M0 · 服务形态与灌库进度进 `Health`（不依赖任何决定，先做）

`plant-ui/src/task_queue.rs::Health` 补三格（全部 `serde(default)`，老服务端不给就整格不画）：

```rust
/// gen-model 的持久层在哪。`"embedded:mem"` = 进程内嵌 kv-mem（gen-model
/// `in_memory_db`），本进程**读不到那个库**；其余是 `ip:port`。
#[serde(default)]
pub sul_db_endpoint: Option<String>,
/// 持久层是不是进程内嵌的内存库。与上一格同源，服务端两处都给。
#[serde(default)]
pub in_memory_db: Option<bool>,
/// 启动灌库的覆盖面与复核结果（`/health.initialization.startup`）。
#[serde(default)]
pub initialization: Option<Initialization>,
```

判据一条：`Vm::store_unreachable_reason()` —— `in_memory_db == Some(true)` 且本进程连的不是同一个库时，
返回「模型服务的库在它自己进程里，本界面读不到属性 / 搜索 / 房间」。**这是 A 形态误配时的唯一自辩**，
不做的话用户看到的是一个连不上库的空壳，没有任何一句话指向原因。

测试：`an_embedded_store_is_named_as_unreachable`、`a_shared_endpoint_stays_silent`、
`old_servers_without_the_endpoint_key_stay_silent`。

### M1 · 冷库期不说谎（D2）

- `Vm` 加 `loading: Option<LoadingProgress>`，从 `Health.initialization` 解；队列轮询那一拍顺带更新，
  不新开轮询。
- 属性面板 / 搜索下拉 / 房间页签三处的空态文案分档：`loading.is_some()` → 「数据仍在装载（N / M 库）」；
  否则维持今天的「无」。
- 三维那一半**不改**：`ensure` 那条路本来就按需生成，冷库对它是正常输入。

测试：`an_empty_result_during_loading_says_loading_not_none`、`a_warm_store_keeps_the_old_empty_text`。

### M2 · CATA 元素给定论（D3 推荐项）

- `Vm` 记一份 `catalogue_dbnums`（从 `/dbnums` 的库类型解，`DESI` 之外的 `CATA` 归此）。
- `element_props` 回空且 refno 的 dbnum 落在该集合里 → 显示定论文案，不显示空表。

测试：`a_catalogue_element_gets_a_verdict_not_an_empty_table`。

### M3 · ngram 索引换戳（D4 推荐项）

- `name_index::staleness_stamp` 不再读 `dbnum_watermark`；改吃 `task_queue::Vm` 已有的
  `DbnumStatus.applied_sesno`（M3a 已解出来了，白拿）。
- 磁盘缓存键加 `applied_sesno` 向量，易失库重启后能命中上一次的索引。

测试：`the_stamp_comes_from_the_dbnum_poll_not_the_watermark_table`、`a_restarted_volatile_store_reuses_the_index`。

### M4 · 开箱即用的文档（A′ 之后只剩两段，脚本的活归 gen-model）

- 起 surreal 那一段由 gen-model 自己完成（A′），本仓不再出 `Start-OutOfBox.ps1`。
  文档只需说清两段：**起 gen-model → 起 plant-ui**，以及「plant-ui 的 `DbOption.toml` 里
  `v_ip` / `v_port` 必须与 gen-model 那份一致」——A′ 把子进程绑到的正是那对值，对不上就连不上。
- `PLANT_MESH_DIR` 默认指向 gen-model 的 `meshes_path`——网格文件不随库消失，这一条要写进文档，
  否则每次开机看到的是一场「实例有了、网格全是加载失败」。
- `CONTEXT.md` 新增词条**冷库**：「每次启动都从零灌的模型本体库。树与三维经模型服务走文件，
  属性 / 搜索 / 房间要等灌到才有。空不等于没有。」

### M5 · 实测验收

在 AvevaMarineSample 上，gen-model 以 A′ 内存形态起（自带 surreal 子进程）、direct + `sync_live=true`、
先用 `watch_dbnums` 限定到 7997 / 7999：

1. 只起 gen-model 与 plant-ui 两个进程，plant-ui 正常连上，树立刻可展开（走文件，不等灌库）。
2. 灌库期间点一个元素：属性面板说「数据仍在装载」，不是空表；灌完再点，属性齐活。
3. 点一个元件库元素：拿到 M2 的定论文案。
4. 点眼睛显示一个 ZONE：`ensure` → 三维出来；平表快路命中率 100%（对照 changelog 那次 47475 行的数字）。
5. 关掉 gen-model 再起一遍：surreal 子进程随它退出（**要确认没留孤儿占着 8009**）、库空了、
   网格文件还在；重复 1–4，**耗时记成 before**。
6. 误配一次真·进程内嵌形态（gen-model 走 `mem://` 那条老路）：plant-ui 给出 M0 那句话，
   而不是一个无解释的连接失败。这一格是 A′ 落地后**唯一**还会撞上 `embedded:mem` 的场合，
   若 gen-model 把那条老路整个删掉，M0 的判据保留但永不触发（当护栏用）。

记录落 `docs/2026-08-12_live-test-ledger.md` 同款格式。

---

## 6. gen-model 侧前置（不在本仓施工）

| # | 事 | M 依赖 |
|---|---|---|
| G0 | **A′ 本体**：内存形态下拉起 memory 后端 surreal 子进程，绑 `v_ip:v_port`，随父进程退出而死 | 全部（没有它，本仓这几条也没有可跑的现场） |
| G1 | `/health` 顶层露出 `sul_db.endpoint` 与 `in_memory_db`（**已有**，`handlers.rs:223/454`）；A′ 之后 `endpoint` 要报**真端口**而不是 `embedded:mem`，介质另开一格 | M0 |
| G2 | `/health.initialization.startup` 的覆盖面字段稳定成契约（服务端已落地，横幅半边未做——见 09-04 审核 §4 第 4 步） | M1 |
| G3 | `/dbnums` 每行带库类型（DESI / CATA / SYST…），今天只有 DESI 口径 | M2 |

---

## 7. 风险与不做

- **A 方案的真实体量被低估的风险**：N3 那条 47k 行、16.5k 条 `geo_relate` 的投影今天靠 4 条 WS 连接池 +
  8 路并发 + 1500 行分块跑到 16 s。搬上 HTTP 要重新设计分块、背压与进度，且失去 SurrealDB 的
  索引下推。**在 D1 拍 A 之前，先要一份 HTTP 传输的量级估算**，不能凭「加个 handler」开工。
- **N5 不是过渡态**：ADR-058 把「CATA 不入 `pe` / `ATT_*`」写成了不变量 S3。任何「等以后灌进去就好了」
  的界面文案都会长期说谎，M2 必须给定论而不是给「加载中」。
- **两条链异步是设计而非缺陷**（ADR-058 D7）：会出现「三维已经有了、属性还没到」的中间态，
  M1 的文案要覆盖这一格，不能只覆盖「全都没到」。
- **`sync_live` + watcher 在 direct 下第一次面对真实文件事件流至今未 live 验证**
  （gen-model 09-04 审核 §5 第 3 条）。M5 必须先限定 `watch_dbnums`，否则 plant-ui 这一侧看到的
  任何异常都分不清是谁的账。
- **不做**：不改 `plant-ui-view3d`（网格加载与库无关）；不动手动增量更新 / 取回工作的既有形状
  （09-02 那份计划的 M1 / M2 / M3a 已完工，本计划不回头改它们）；不为 kv-mem 单开一套数据层抽象
  ——B 方案下读的还是同一个 SurrealDB，抽象是 A 方案落地时才付得起的账。
