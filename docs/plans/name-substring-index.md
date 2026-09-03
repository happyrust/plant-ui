# 名称子串搜索 · tantivy 本地索引 · 开发计划

- 日期：2026-08-21
- 范围：`plant-ui-data`（新模块 name_index + 删包含匹配查询）、`plant-ui-app`
  （数据桥改形 + 索引生命周期 + reindex 动词）、`plant-ui`（搜索框下拉两段化、
  拆回车两段式）、`Cargo.toml` / `vendor/registry`（收编 tantivy）。
- 依据：ADR-0023（子串搜索跑在本地 ngram 索引上）；ADR-0022 的两段式交互随之退役，
  其余口径（前缀路、树外、分流）不动；CONTEXT.md「本期执行范围」「树外元素」。
- 状态：施工中。M0 已完成（2026-08-22），M1 代码落地、真库验收待补
  （2026-08-23，两段施工记录都在第五节末尾），M2 起未动。

---

## 一、要改什么（一句话）

搜索框每敲一下并发两路——前缀照旧打库（全库范围，15.8ms），子串查本地 tantivy 索引
（当前 MDB 设计库范围，亚毫秒）——下拉里前缀命中在前、子串补充在后；「前缀搜空了按
回车逐条搜」那个两段式交互整个拆掉；索引由 plant-ui 自建自管：落盘缓存、启动亚秒打开、
数据变了后台重建。

## 二、现状与勘探事实

| 事实 | 出处 | 对本计划的影响 |
|---|---|---|
| 子串在库内无药可救：91 万行上 83.6s（子查询限库）/ 88.9s（单 WHERE）/ 116.4s（NOINDEX），病根是 11 列文档逐行读取本身 | 探针 `_probe_table*.log`（已跑，待删） | 库内路线出局，`search_names_containing` 删除 |
| 窄表出局：物化 103.5s、库内扫 19.5s、当加载源分页读 47.7s > 从 `pe` 分库直读 31.4s | 同上 | 方案里不出现窄表 |
| 真语料：91 万行中**有名字的 143,244 行**，TSV 6.1MB；一次性拉 91 万行会压断 WS 通道，分库拉稳定（最大单库 36 万行 8.5s，全程 31.4s） | `_dump_corpus.log`、`name_dump_probe.rs` | 语料加载必须分库；31.4s 是重建耗时上限（服务端 `name != NONE` 过滤有望更低，M1 量） |
| tantivy 0.26.1 沙箱（ngram 2..3 + LowerCaser + contains 验真）：建索引 152ms、体积 7.4MB、重开 2.2ms、单查含验真几百 µs；`rs-c` 14 条与库内一致、`rs-c1` 0 条经反查为真、大小写不敏感成立 | `%TEMP%\tantivy_probe`（仓库外沙箱，待删） | 参数照搬：ngram(2,3)、候选 AND、验真 |
| 本仓库全 vendored 离线构建；tantivy 不在依赖树，收编带进约 140 个子 crate；沙箱已在本机编译通过 | `.cargo/config.toml`、`Cargo.lock`、沙箱 `_build.log` | M0 一次性成本；vendor 重跑要先移开 config（文件头注释就是这么写的） |
| `plant-ui-data` 是 wasm 双端 crate（tokio 已按 target 分组） | `crates/plant-ui-data/Cargo.toml` | tantivy 必须进 `cfg(not(wasm32))` 依赖组，模块整体 cfg 门 |
| 搜索链路现状：`Cmd::SearchElements{query, contains}` → `Req::SearchElements{epoch, query, contains, dbnums}` → `search_names_by_prefix` / `search_names_containing` → `Evt::SearchElements{epoch, query, contains, result}`；epoch 作废晚到结果；`SEARCH_LIMIT = 20` | `search.rs`、`data.rs:44-58,340-357`、`main.rs:2771-2789` | Req/Evt 去掉 `contains`，一次回两路结果 |
| 回车两段式在绘制层：`can_widen` + `Hotkey::Submit` 空行分支 + 「回车逐条搜…」文案 | `search.rs:196-204,307-314,357-363` | 整段拆除 |
| 本地状态的房规：跟着发行包走，`<exe 旁>/config/settings.ron`；开发构建落 `target/debug/` 下、`cargo clean` 带走属可接受 | `settings_store.rs` 文件头 | 索引放 `<exe 旁>/search-index/`，同一条房规 |
| 陈旧信号现成：行数 count 走 dbnum 索引（91 万行 360ms）；gen-model 水位表 `dbnum_watermark.applied_sesno` 已有读法先例 | 探针、`plant-ui-data/lib.rs:336-350` | 戳 = 每库 (dbnum, applied_sesno, count)，亚秒可查 |
| 数据变更的落点：名字只经 gen-model 数据批次改动；宿主已有钩子——`get_work` / `Reconnect` 都先 `invalidate_all`，任务队列快照能看到批次到终态 | `data.rs:243-247,559-572`、`main.rs` 队列轮询 | 重建触发点齐了，不需要新通道 |
| 命令行动词表：`help` / `clear` / `/名称` / `=参考号` / `q …` | `command.rs:139-177` | `reindex` 动词有现成的挂法 |
| sim 剧本里前缀与包含共用一段内存过滤 | `sim.rs:444-460,1504-1520` | 改成一次回两路，路径打通不掉线 |

## 三、已拍板的决定

| # | 问题 | 决定 |
|---|---|---|
| 1 | 子串范围 | 当前 MDB 的 DESI（今天 91 万行里有名字的 14.3 万）；全库继续走前缀路 |
| 2 | 子串引擎 | tantivy 索引落盘（用户点名：全库扩展和排序都要留） |
| 3 | 交互形态 | 拆两段式：每键并发前缀 + 子串，前缀在前、子串补后带范围标注（交接指示按提案继续；本计划评审即最后的否决点） |
| 4 | 索引的家 | `<exe 旁>/search-index/<ns>__<mdb>/v1-<戳散列>/`，settings.ron 同款房规；目录名即戳——在就开、不在就建，建完临时目录改名落位，开不动当不存在重建 |
| 5 | 陈旧戳 | 每库 `(dbnum, applied_sesno ?? 0, 行数)` + 格式版本号；行数管增删、水位管改名；「够不着」的库无水位、行数不变时改名戳看不见——`reindex` 手动兜底 |
| 6 | 重建触发 | 启动、取回工作、重连、队列批次到终态、`reindex`；数据线程单飞（在建则跳过），重建期间旧索引若在**继续服务** |
| 7 | 展示配额 | 总行数照旧 ≤ 20：子串有命中时前缀最多占 15、子串补满到 20；任一路到顶沿用「命中太多」提示 |
| 8 | 排序 | 验真命中按：命中落在分段开头（段界 = 名字头或 `/` `-` `_` 之后）> 段中，再名字短优先，再字典序；BM25 只用于候选池（200 条）的挑选 |
| 9 | 最短针 | 2 字符（与 ngram 下限一致；前缀路本来就有 `len > 1` 门槛） |
| 10 | wasm | 无索引；子串路回「不提供」，下拉在前缀搜空时说一句「按中间片段搜索仅桌面端提供」 |
| 11 | 旧物处置 | `search_names_containing` 删除；三个探针测试、`_name_corpus.tsv`、`_probe*/_dump*` 日志、`%TEMP%\tantivy_probe` 沙箱全部清掉，新增一个常驻的 ignored 集成探针 |

## 四、改动设计

### 4.1 依赖与 vendor（M0）

1. `plant-ui-data/Cargo.toml` 的 `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`
   加 `tantivy = "0.26.1"`（默认特性，沙箱同款）。
2. 按 `.cargo/config.toml` 文件头的房规重跑 vendor：移开 config → `cargo vendor
   vendor/registry` → 把它产出的 `[source.*]` 片段与手写的 `[build]` / wasm 段拼回。
3. 验收：断网状态 `cargo check -p plant-ui-data --all-targets` 通过；wasm 侧证明 cfg 门
   有效——`cargo tree -p plant-ui-app --target wasm32-unknown-unknown -i tantivy` 打不出东西
   （native 同一条命令能打出 `tantivy → plant-ui-data`）。

   > 计划原本写的是「`cargo check -p plant-ui-data --target wasm32-unknown-unknown` 通过」，
   > 那条**验收不成立，与 tantivy 无关**：`getrandom` 的 `wasm_js` 特性只由 plant-ui-app /
   > plant-ui-view3d 打开，单独 check plant-ui-data 必然撞上 `.cargo/config.toml` 里
   > `getrandom_backend="wasm_js"` 这条 rustflag 与特性对不上的编译错。整包
   > `cargo check -p plant-ui-app --target wasm32-unknown-unknown` 在本机也停在 `ring v0.17.14`
   > 的构建脚本上——它要 clang，本机没装；同样是先于本期就在的账。

### 4.2 `plant-ui-data/src/name_index.rs`（M1，整模块 `cfg(not(wasm32))`）

**schema 与沙箱一致**：`name`（TEXT，tokenizer `ngram23` = NgramTokenizer(2,3) +
LowerCaser，`IndexRecordOption::Basic`，STORED）、`refno`（U64 STORED，取
`RefU64` 的 packed 原值，不走字符串解析）、`noun`（STRING STORED）、`dbnum`
（U64 STORED）。

```rust
pub struct NameIndex { index: Index, reader: IndexReader, fields: Fields }

/// 戳：亚秒可查的陈旧信号。一条 SQL 拿行数（GROUP BY dbnum，走索引），
/// 一条拿水位（dbnum_watermark，DESI，缺行按 0）。序 + 格式版本一起散列进目录名。
pub struct IndexStamp(Vec<(u32, i32, u64)>);

pub async fn stamp(dbnums: &[u32]) -> Result<IndexStamp>;
pub fn open(dir: &Path) -> Result<NameIndex>;          // 2.2ms，失败=当不存在
pub async fn build(dir: &Path, dbnums: &[u32],
    progress: impl FnMut(usize, usize)) -> Result<NameIndex>;
pub fn search(&self, needle: &str, limit: usize) -> Vec<NameHit>;
```

- **build**：分库 `SELECT VALUE [id, name, noun] FROM pe WHERE dbnum = $dbnum AND
  name != NONE`（服务端过滤先量一次；不快就退回全拉客户端过滤的已证路径 31.4s），
  每库完成回一格 progress；写进 `…/building-<pid>` 临时目录，commit 后改名为
  `v1-<戳散列>` 落位；同级残留的其他目录顺手删（上一版索引、上次没建完的尸体）。
- **search**：针小写化 → 取 n-gram（针长 ≥3 取 3-gram，否则 2-gram）全部 AND →
  `TopDocs::with_limit(200)` 候选 → 存储名小写 `contains` 验真 → 按决定 8 排序 →
  截断 limit。返回既有 `NameHit`（refno/name/noun/dbnum 齐活）。
- 单元测试：gram 切法（含 needle ≤ n 的整串退化）、段界判定（`/A-RS-C1` 里 `rs-c`
  是段首、`s-c` 是段中）、排序键、戳散列稳定性。
- 集成探针 `tests/name_index_probe.rs`（ignored，常驻）：真库建索引，断言
  `rs-c` 与 `search_names_by_prefix` 口径下的库内逐行结果一致（沙箱的 14 条对拍
  逻辑搬进来）、打印建索引/打开/单查耗时。

### 4.3 数据桥（M2，`plant-ui-app/src/data.rs`）

```rust
Req::SearchElements { epoch: u64, query: String, dbnums: Vec<u32> }   // contains 死
Req::RebuildSearchIndex                                                // reindex 动词
Evt::SearchElements { epoch, query, prefix: Result<Vec<NameHit>>,
                      substring: SubstringHits }
Evt::SearchIndex(SearchIndexState)   // 就绪(条数)/构建中(done,total)/失败(原因)/不提供

enum SubstringHits { Hits(Vec<NameHit>), Building, Unavailable }
```

- 数据线程持 `Arc<RwLock<Option<Arc<NameIndex>>>>`；搜索处理器 `join!` 前缀查询与
  索引查询（后者同步、亚毫秒，锁读即查），一条 Evt 回去——合并延迟 = 前缀路的
  15.8ms，不值得两条消息。
- **索引生命周期任务**（单飞，`spawn` 后台）：算戳 → 目录在？`open` 并广播就绪 :
  `build` 并逐库广播进度 → 换进 slot。触发点：`ready()` 之后（启动）、
  `Evt::GetWork` / `Reconnect` 处理完（跟着既有 invalidate_all 的位置）、
  `Req::RebuildSearchIndex`（跳过戳比对强制建）、宿主发来的批次终态通知（下条）。
- 失败：日志 + `SearchIndexState::失败(原因)`，旧索引（若在）继续服务；下个触发点
  自然重试。

### 4.4 宿主（M2，`plant-ui-app/src/main.rs`）

- `SearchVm` 装配：`hits`（前缀）与 `sub_hits`（子串）分开装，按决定 7 配额截断；
  `search.sub_state` 跟 `Evt::SearchIndex` 走。
- **队列终态钩子**：队列快照对比中已有行状态迁移的判定处，凡有批次进入终态
  （已完成 / 部分完成 / 失败）→ 发一次 `Req::CheckSearchIndex`（即触发戳校验，
  单飞去重）。
- **`reindex` 动词**（`command.rs`）：`parse` 加一臂；执行发
  `Req::RebuildSearchIndex`，命令面板回「搜索索引重建已开始（后台），完成后日志
  可见」；就绪 / 失败经 `Evt::SearchIndex` 落日志：
  「子串索引已就绪：143,244 个名字（打开 2ms / 重建 31s）」句式。
- 日志口径（logs 面板）：启动打开、后台重建开始 / 完成 / 失败各一条，数字说真话。

### 4.5 绘制层（M3，`plant-ui/src/workbench/search.rs`、`vm.rs`）

- `SearchVm`：`contains` 死；加 `sub_hits: Vec<SearchHitVm>`、
  `sub_state: SubIndexVm { Ready, Building { done, total }, Failed(String), Off }`。
  `SearchRunVm` 只剩 `query`。
- `search.rs`：`can_widen` / 回车空行分支 / 「回车逐条搜…」「正在逐条比对…」文案
  全删；模块头注释改写成两路并发的新账。行集合 = 前缀行 ++ 子串行（键盘上下
  穿行两段），子串段前插一行不可选的节标注。
- 文案（原样落码）：
  - 子串节标注：`名字中间含「{q}」——{N} 个设计库（当前 MDB）`
  - 构建中（子串暂缺）：`子串索引准备中（{done}/{total} 库）…先看开头匹配的`
  - 双空且已定稿：`没有名字以「{q}」开头的元素；{N} 个设计库里也没有名字含它的`
  - 索引失败：`子串搜索不可用：{原因}（命令行输入 reindex 重建）`
  - wasm（`Off` 且前缀空）：`按中间片段搜索仅桌面端提供`
  - 任一路到顶：沿用`命中太多，只列前几条；多打几个字缩小范围`
- `classify`、参考号路、树外标、掐中间省略：一律不动。
- 测试：既有 `classify` / `elide_middle` 保绿；补「回车在无行时不再发起任何查询」
  「两段行拼接与光标环绕」的纯逻辑断言。

### 4.6 sim（M3，`plant-ui-app/src/sim.rs`）

`engine.search` 改成一次回 `(prefix, substring)` 两路（同一段内存过滤按「开头 /
含」分拣），`SubstringHits::Hits` 常备——剧本模式里索引永远就绪，注释仍说明
「分不出快慢」。

### 4.7 文档与清理（M4）

- `CHANGELOG.md` 未发布 / 搜索节改写：两段式退役（回车那条删掉重写）、子串跟着
  打字走、范围与首次索引构建各说一句、`reindex` 动词一句。
- ADR-0023 已入库（本计划的另一半交付物）；ADR-0022 不改文——两段式那段被 0023
  显式取代，前缀路口径仍有效。
- 删：`tests/name_search_probe.rs`、`tests/name_table_probe.rs`、
  `tests/name_dump_probe.rs`、`_name_corpus.tsv`、`_probe_search*.log`、
  `_probe_table*.log`、`_dump_corpus.log`、`%TEMP%\tantivy_probe`。

## 五、施工顺序与测试

| 步 | 内容 | 验收 |
|---|---|---|
| M0 | 收编 tantivy + vendor 重跑 | ✅ 断网 `cargo check --workspace --all-targets` 过；wasm 侧 cfg 门经 `cargo tree` 证明 |
| M1 | `name_index` 模块 + 单测 + 常驻集成探针 | 单测绿 ✅；真库探针（对拍一致、建/开/查三个耗时打印）⏳ 等一份稳定真库 |
| M2 | 数据桥改形 + 生命周期任务 + 队列钩子 + reindex | `cargo check --all-targets`；epoch 作废、单飞、失败留旧索引各一条纯逻辑测试 |
| M3 | 绘制层两段下拉 + 拆回车 + sim | 既有测试绿 + 新增断言；`cargo test -p plant-ui` |
| M4 | CHANGELOG + 清理探针与日志残留 | `git status` 里探针与语料不再出现 |
| M5 | 真库验收（AMS `/ALL`，91 万行） | 见下 |

### M0 施工记录（2026-08-22）

- 落码：`plant-ui-data/Cargo.toml` 的 `cfg(not(wasm32))` 组加 `tantivy = "0.26.1"`。
- vendor 重跑照文件头房规走（移开 `.cargo/config.toml` → `cargo vendor vendor/registry` →
  拼回）：**生成的 `[source.*]` 与原文件逐字一致**，所以直接还原了备份，手写的
  `[build] jobs = 8` 与 wasm rustflags 原封未动。
- 依赖增量比预估小：Cargo.lock +35 个包（不是 140），`vendor/registry` 1017 → 1033 个目录，
  磁盘 1.06GB 起步。没有任何包被移出锁文件。
- 耗时：vendor 30s；断网 `cargo check -p plant-ui-data --all-targets` 2m02s；
  断网 `cargo check --workspace --all-targets` 9m34s，0 error（warning 是先于本期就有的）。
- **一处需要你知道的副作用**：`cargo vendor` 会清掉不在锁文件里的目录，这一跑删掉了 1002 个
  **被 git 跟踪的陈旧 vendored 文件**（`roxmltree` / `ttf-parser` / `cosmic-text` / `swash` /
  `taffy` / `bevy_ui` / `bevy_text` 等，共 22 万行）。它们在**本次改动之前就已不在依赖图里**
  ——`.gitignore` 有 `/vendor/registry`，这批是加忽略之前就提交进去的遗骸。为了不让 M0 的
  diff 掺进 22 万行无关删除，已用 `git checkout` 全部还原；**要不要真删是另一笔账，等你发话**。
  留下的 8 个改动全在 `vendor/registry/ordered-float`：那也是一份陈旧遗骸，被 tantivy 真正
  需要的版本覆盖了，属于本期必须留的改动。

### M1 施工记录（2026-08-23）

- 落码：`plant-ui-data/src/name_index.rs`（戳 / 建 / 开 / 查四件套，整模块
  `cfg(not(wasm32))`）、6 条单元测试、常驻探针 `tests/name_index_probe.rs`。
  单测全绿，`cargo check -p plant-ui-data --all-targets` 0 error、本 crate 0 warning。
- **探针在真库上抓出两个 bug，都已修**——它们都属于「单测全绿而界面撒谎」的那一类，
  正是这条探针存在的理由：

  1. **算戳退化成扫表（AMS 91 万行 14.1 秒）**。原写法
     `SELECT dbnum, count() AS rows FROM pe WHERE dbnum IN […] GROUP BY dbnum`
     的执行计划是 `UnionIndexScan → Filter → Aggregate`：每一行都要把 11 列文档
     读出来，与子串搜索当初被赶出数据库的是同一个病根。改成**一库一条**
     `SELECT count() FROM pe WHERE dbnum = <字面量> GROUP ALL`，计划变
     `IndexCountScan`——只数索引条目，不碰文档。8030 基线库上两种形状对照实测：
     同一个 9,115 行的库 30.7ms → 3.99ms；按每行成本外推到 91 万行 ≈ 0.4 秒，
     与本计划第二节引的 360ms 对得上。
     **dbnum 必须是字面量**：实测 `LET $d = 8000` 会让规划器退回
     `Aggregate(IndexScan)`，优化当场消失。dbnum 是 `u32`，拼进 SQL 没有注入面。
  2. **语料漏筛空名字**。没名字的元素在 `pe` 里存的是**空串**，不是 NONE，
     `name != NONE` 一条挡不住。补成 `name != NONE AND name != ''`——91 万行里
     只有 14.3 万行真有名字，少这一条就是把 76 万条空名字喂进索引。
- 顺手改宽容一处：`dbnum_watermark` **整张表不存在**时（没跑过 gen-model 的库就
  这样，8030 基线库即是）原先会让整个戳失败，现在当全 0，行数照样管得住增删。
- **未完：真库探针没跑到底**（对拍一致 + 建 / 开 / 查三个耗时）。8009 在 8/22 23:07
  换成了一个 `memory` 实例：`pe` 上一条索引都没有，内容还在被反复重写（隔 6 秒采
  三次，总行数 26,466 → 1），在它上面跑出来的数字都不作数。AMS `/ALL`（20 个设计库、
  910,319 行）那份不在任何一个活着的实例上；磁盘上 `gen-model/.surreal/ams-8009`
  还躺着 1.70GB（8/11 最后写入）。M1 的验收就差这一步，**换回一份稳定真库即可补**。
  另两个活着的实例都不合用：8030 是 `ams-8000-baseline031.db`（17,419 行，索引齐全，
  EXPLAIN 对照就是在它上面量的）、8073 是 32 行的 fixture；而 `connect()` 会
  `define_pe_index`，往别人的基线库里加索引不是这条探针该干的事。

M5 验收清单：

1. 冷启动（无索引目录）：应用可用，下拉见「准备中（x/20 库）」，日志报重建完成
   （≈30s 量级）；期间前缀搜索正常。
2. 二次启动：日志报「打开 2ms」量级；输入 `RS-C` 每一键下拉即时更新，子串段
   14 条与旧库内路数目一致，排序符合决定 8。
3. 输入 `24381` 这类纯数字短串：走名字路，两段都有机会命中；`24381/100819`
   仍走参考号直达。
4. 树外元素命中子串段：标「树外」，定位行为与 ADR-0014 一致。
5. 跑一次小的模型增量更新到终态 → 不重启，日志出现后台重建，完成后新名字可搜。
6. `reindex`：命令面板回执 + 日志两条（开始 / 完成）。
7. 断库拔线：前缀报错、子串段照常服务（索引是本地的）；下拉错误文案不撒谎。

## 六、风险与边界

- **vendor 体积与构建时长**：+140 crate 一次性；离线自洽性靠 M0 断网验收兜底。
- **候选池 200 上限**：短针（如 `-c`）候选溢出时，BM25 挑进池子的未必是段首命中，
  排序盲区客观存在；针 ≥3 字符时候选急剧收敛，实用面不受影响。M5 观察，真挤爆
  再谈加大池子或 Count collector 提示。
- **陈旧窗口**：戳只在触发点校验；「够不着」的库（水位缺失）里发生的纯改名不推进
  行数与水位，戳看不见——`reindex` 是唯一的门，文档与日志都要说这句话。
- **Windows 文件占用**：改名落位只发生在新建目录上（无打开句柄），旧目录删除失败
  容忍（下次启动再扫）；绝不在打开中的索引目录上做原地写。
- **构建中的窗口**：首启 ≈30s 子串缺席，界面明说；这窗口与旧方案「按一次回车等
  84 秒」不可同日而语。
- **全库 625 万的将来**：schema 与目录布局今天就带格式版本号；扩容是换语料范围与
  参数（内存上限、合并策略），不是换架构。
