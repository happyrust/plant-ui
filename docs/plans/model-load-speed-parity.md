# 模型显示提速 · 对齐 plant3d-web 的施工计划

- 对象：`D:\work\plant-code\old\plant-ui`（`master` @ `3d917317c`）；参照 `D:\work\plant-code\old\plant3d-web`（`main` @ `be2b5b92`）；后端 gen-model（`refactor` @ `2ed067be0`）**不改**
- 日期：2026-11-12
- 依据：审核报告 `gen-model/output/plant-ui-model-load-audit-20261112/REPORT.md`（只读审核，下文「审核」）；plant3d-web `docs/plans/2026-11-10-model-load-speed-generate-and-freshness-plan.md`（下文「方案」）与 `docs/verification/model-load-speed-2026-11-12/README.md`（下文「测速」）；gen-model `docs/specs/web-service-api.md` §4.5.2；本仓 ADR-0016 / 0021 / 0026 / 0030 / 0033、ADR-0034（已接受）；`CONTEXT.md` 术语
- 性质：完整开发计划，2026-11-12 20:01 评审批准（plannotator，无批注）。设计上新开一份 ADR（ADR-0034 落定重载，已接受）；其余都是照已有决定把慢的地方改快，不改供数模式、不改眼睛语义

> **评审记录。** 这份计划经过两轮询问：第一轮十条用户 2026-11-12 19:16 已定（全部按推荐）；第二轮六条（Q11–Q16）按协调者的推荐写入，2026-11-12 20:01 用户在 plannotator 里批准、没有批注，文中标「按推荐，评审已确认」。
> 评审时重点看过的三处：第七节「已定」、第一节目标表、以及 T3b（显示预算）与 T7（落定重载）这两个改用户可见行为的任务。
> 其余任务是照已有决定把慢的地方改快，不改供数模式、不改眼睛语义、不改 gen-model。批准后按第五节顺序开工，第八节是排期，第九节是每一期的回退口，第十节是给用户看的演示步骤。

**一句话现状：** plant-ui 没有跟进 plant3d-web 11-10～11-12 这一轮的显示提速。慢在两段——点眼睛后把节点当**一个**祖先、5000 条一页**串行翻页**取记录（8 万件 SITE 在同一后端实测 21.3 s，按 ZONE 6 路并发 2.1 s）；记录收齐后在**同一帧**生成全部实体，期间没有进度、不能停。渲染一侧不落后。另有三处自己的问题：Bevy `multi_threaded` 没开（推断桌面端网格读盘解码在主线程）、每个库第一次显示都发一次 15–40 s 的整库根枚举并据此误判「库里没模型」、项目里任何一个设计库一有新保存就**整场重装**。

---

## 一、目标与边界

**目标（能量出来的）：** 以审核第 4 节的四个节点为准，改完之后——

| 节点 | 现在（推断 / 已知） | 目标 | plant3d-web 现在（测速 1.3 节） |
|---|---|---|---|
| 小 ZONE `24383_91690`（137 件） | 没量过 | 出图 ≤ 0.3 s | 90 ms |
| 大 ZONE `17496_8516`（1,888 件） | 没量过 | 出图 ≤ 0.8 s、画完 ≤ 1.5 s | 438 / 522 ms |
| 整 SITE `24383_80211`（19 个 ZONE、7,400 件） | 没量过 | 出图 ≤ 0.5 s、画完 ≤ 2 s | 110 / 779 ms |
| 大 SITE `23717_2`（531 个 ZONE、79,409 件）※ | 记录层约 21 s，之后整批生成 | 出图 ≤ 1 s、画完 ≤ 8 s、期间最长一帧 ≤ 100 ms | 97 ms / 4.85 s |

※ 大 SITE 一行受 5 万件显示预算限制（T3b）：缺省点下去装到约 5 万件就停；表里的「画完 ≤ 8 s」是全量数，量它时用 `PLANT_UI_SHOW_BUDGET=0` 关掉预算再量。

另外三条硬指标：显示任何节点时后端收到的 `dbnums/*/model/roots` 请求 = 0；在**别的库**里保存一次之后，已加载场景不重装、后端收到的 `model/records` 请求 = 0；加载途中按「停止」后 ≤ 100 ms 内不再发请求。

**不做：** 粗网格 LOD（桌面端全视本来 47–57 fps，网页端卡在 draw call 不在三角形数）；网格本地持久缓存（桌面端读本地文件）；换供数模式；改 gen-model（主线冻结，而且这一轮用到的接口后端都已经有）。plant3d-web 独有的 DTX 数据纹理层不搬，Bevy 的合批与剔除够用。

---

## 二、架构决定

1. **取数的形状照 plant3d-web，不另起炉灶。** 顶层容器拆成成员、有界并发、成员多时用服务端批量形、每收完一个成员就交出去——这四条在 plant3d-web 上都量过有效，服务端契约（§4.5.2）两边一样。plant-ui 的实现在 Rust 里重写，但批次大小、并发数、拆分阈值的常量与 plant3d-web 对齐（6 路、16 个一发、超过 64 个成员才打包、头几发 1/1/2/4/8、每页 5000 条不变），日后两边对照不用换算；打包用环境变量 `PLANT_UI_RECORDS_BATCH` 关掉或改每发个数，对应 plant3d-web 页面地址上的 `gm_records_batch`（Q11，按推荐，评审已确认）。
2. **「同库不混源」的纪律不动（ADR-0025）。** 批量形的一批根必须同库（服务端也这么要求），按成员分组前先按 dbnum 分桶；每个 ZONE 的回执仍带 `dbnum` / `source` 进 `ModelRecordUnion` 校验。
3. **眼睛仍只读实际可见性（ADR-0016）。** 边取边画只是把「一个目标一批」变成「一个目标多批」，每批都走现有的 `View3d::append` → `render_states` → `settled()` 那条账；加载途中眼睛不提前变。
4. **显示路径不生成、不预检、不留捷径（ADR-0030 D1）。** 去掉 `model/roots` 探针之后，不拿 `/dbnums` 行（`initialized`、`model_sesno`、`model_source`）推断「这个库有没有模型」：所有显示一律先查记录——没生成过的库回 200 空，毫秒级——查空再走默认补齐（Q12，按推荐，评审已确认）。
5. **自动重装收窄到变了的单元 / 库（ADR-0034，已接受）。** 变了哪几个库就只重载涉及这几个库的范围（有单元清单时精确到单元），走右键重新生成已在用的「按目标卸旧装新」（`View3d::refresh`）；其余范围连记录都不重查。取回工作（ADR-0021 的「清场重装」）保留为手动动作。备选是照 plant3d-web 只标「有更新」由人点——写进 ADR 的「否决项」，用户拍板——2026-11-12 用户定 (a)，见 ADR-0034。边界两条（元件库变了、只登录不改几何）见 ADR-0034 后果 2 / 3；设置项「落定后自动重载」缺省开，关 = ADR-0021 原文字（回退口，第九节）。
6. **先桌面端，网页端跟在后面。** 网页端的网格取法（限并发、重试、打包）单列一期，打包取要等 gen-model 把 `meshes/batch` 提交进主线。
7. **每一期都能单独发版。** 期与期之间有检查点；任何一期做完都比现在快、没有半成品留在树上。
8. **边取边画时眼睛按 ADR-0016 的聚合规则走。** 几何行随每批落定逐个亮起，容器行途中半可见，收齐定格；不给 ADR-0016 加例外。
9. **加载途中的「停止」。** 已上屏的留着，没取完的目标回未加载（不是已隐藏），再点只取没取完的成员。
10. **测速用线上后端 8022，起来之后只发只读请求。** 在 master 树上开分支 `feat/model-load-parity`，做完切回。
11. **大范围按预算制，不预先确认（Q13，按推荐，评审已确认）。** 一次显示装到 50,000 件就停止取后面的成员，剩下的目标回未加载、日志提示按 ZONE 分别显示；再点同一节点只取没取完的成员，与「停止」同一套账（T3b）。总数只有每个 ZONE 第一页回来后才知道，预先确认拿不到数。房间树「超过 200 个单元先确认」照旧（ADR-0033 D5）。
12. **相机只在落进空场景时取景（Q15，按推荐，评审已确认）。** 首批上屏取一次、收齐再对一次焦；往非空场景里显示别的节点不动相机。

---

## 三、施工环境与规矩

- **在哪改（已定，第七节第一轮第 7 条）：** 在 `D:\work\plant-code\old\plant-ui`（现在干净，只有一个未跟踪的日志）的 master 树上开分支 `feat/model-load-parity` 做，做完切回 `master`。理由：本机 cargo 按检出路径分产物目录，这棵树的产物在 `D:\Rust\.cargo\targets\plant-ui-8e44c653`（3.8 GB、11-09 编过）是**热的**；另开工作树就是 bevy 全家冷编（估 20–40 分钟、4 GB）。备选（用户改主意要不占 master 树时）：开 `plant-ui-loadspeed` 工作树，必须是 `D:\work\plant-code\old\` 下的兄弟目录——`plant-mbd` 是 `../vendor/...` 路径依赖。
- **人手（Q16，按推荐，评审已确认）：** 一人，不并行派第二人；期 1 做完先交。
- **编译一律经编译锁：** `pwsh -NoProfile -File D:\work\plant-code\old\gen-model\output\ops\with-build-lock.ps1 -Who pu-load -WorkDir <树> -Log <日志> -CargoArgs 'test,-p,plant-ui-app,--lib' -Jobs 4 -StartCommitFreeGb 14`。`CARGO_BUILD_JOBS` 环境变量压过本仓 `.cargo/config.toml` 的 `jobs = 8`。退出码 90 / 91 = 重排，不改门槛、不绕锁。先在树里 `cargo --version` 确认拿到的是 `rust-toolchain.toml` 钉的 `nightly-2026-07-21`，不是就 `rustup run nightly-2026-07-21 cargo …`。**现状（2026-11-12 19:32）：** 全机可提交内存余量 7–9 GB，编译锁门槛 14–16 GB，已有 4 个编译任务在排队——所以先写不需要编译的部分，锁放行再编、再量（第八节）。
- **测试：** 单测 `cargo test -p plant-ui-app --lib`、`cargo test -p plant-ui-view3d`、`cargo test -p plant-ui`；桌面构建 `cargo build -p plant-ui-app --release --locked`（与 `.github/workflows/release.yml` 同）；网页端 `./scripts/build-wasm.ps1`。`--locked`：这一轮**不新增依赖**（`futures` 已在树里；有界并发用 `futures::stream::FuturesUnordered` / `buffer_unordered`），`Cargo.lock` 不该变。
- **量：** 第十二节。改动前后交替量，每档三次取中位数，带 release 构建。plant-ui 现在没有任何显示耗时日志，所以 **T0 先做**。
- **后端：** 只读请求。用哪个后端量，见第七节第一轮第 1 条（8022；现在没在跑，起来之前先做 T0 的脚本与埋点）；不碰别人的沙箱（8023 / 8029 / 8199 / 9199 那几套），不跑 `stack.ps1`。
- **不动的：** gen-model 源码；plant3d-web；`DbOption.toml`（在仓库根，有登录串，不读不抄）；别的 `plant-ui-*` 工作树。
- **秘密：** 提交说明、日志、测速记录里不写口令、地址、本机绝对路径（plant-ui 是 GitHub 公开地址的仓库，推送前再扫一遍）。
- **提交：** 一个任务一到两笔，说明带「T 号」和量到的数；文档（本文件、ADR、evidence）随代码同一笔。不推送——推送由用户点头。

---

## 四、任务清单

规模：XS 1 文件 / S 1–2 / M 3–5 / L 5–8。人日是估的，含量测与回归。

### 期 0 · 量具

#### T0 装载链路的阶段计时与基线

**描述：** 给桌面端装载链路加一个环境变量开关的阶段计时（`PLANT_UI_LOAD_TIMING=1`），记到日志面板和 `tracing`：点击（`set_model_visible`）、记录首页到、记录末页到（`Evt::ModelScope`）、首批实体生成（`load_models` 第一批）、网格全部落定（`update_mesh_progress` 收口）、加载期间最长一帧。同时写一个接口层回放脚本（照 plant3d-web `.scratch/model-load-speed/api-replay.mjs` 的做法，放 `scripts/`），用三种取法（整根翻页 / 按 ZONE 6 路 / 批量形）直接问后端，给出「不经客户端最快多快」的下限。然后在改任何代码之前，对第十二节四个节点各量三次，记成 `docs/evidence/2026-11-xx-model-load-baseline.md`。

**验收：**
- [ ] 开关关着时没有任何额外开销（没有多余的 `Instant::now()` 以外的工作）；开着时日志面板一行能看全五个时刻
- [ ] 基线文档里四个节点各有三次的中位数：点击→出图、点击→画完、记录请求数、最长一帧、进程内存
- [ ] 回放脚本对 `23717_2` 三种取法的数与方案 13.2 节同量级（整根翻页约 20 s、6 路并发约 2 s）

**验证：** `cargo test -p plant-ui-app --lib` 过；release 构建过；手工在 `--look-demo` 之外的真连接上跑一遍。
**依赖：** 无。**文件：** `crates/plant-ui-app/src/main.rs`（埋点）、`crates/plant-ui-view3d/src/lib.rs`（`load_models` / `update_mesh_progress` 两处回报）、`scripts/model-load-replay.mjs`（新）、`docs/evidence/…`（新）。**规模：** M，1 人日。

### 期 1 · 取记录（收益最大）

#### T1 容器按 ZONE 拆开、有界并发取

**描述：** `read_face/service.rs::model_instances` 里，`scope_roots` 中的顶层 SITE 先 `tree_children`（`model_update_api.rs:468` 已有）拿 ZONE 列表，再对成员以 6 路有界并发各发单根 `model/records`（翻页逻辑沿用，每页 5000 条不变——并发数与页大小都与 plant3d-web 一致，Q11，按推荐，评审已确认）；非 SITE 的目标照旧一个祖先一发。回执按 dbnum 分桶进 `ModelRecordUnion`，每个成员的 `dbnum` / `source` 都进同源校验。`tree_children` 失败、或 SITE 下没有 ZONE 时退回整根取法。库供数那一面（`store.rs`）不动。

**验收：**
- [ ] `23717_2` 点击→记录收齐从约 21 s 降到 ≤ 3 s（T0 的量法）；记录请求数 = ZONE 数 + 1
- [ ] 在途单根请求最多 6 个：7 个以上 ZONE 时第 7 个要等前面某个回来（单测）
- [ ] 同库不混源：人为让一个 ZONE 回 `model-memory`、其余回 `model-database`，整次按现有纪律报错重来（单测）
- [ ] 非 SITE 目标、`tree_children` 失败、SITE 无 ZONE 三种情形与改前逐字节同结果（单测对拍 JSON）

**验证：** 新增单测 ≥ 5 条（照 plant3d-web `modelRecords.test.ts` 的「多个祖先有界并发，各查一次」「分页读完；truncated 却没有 next_cursor 时失败」）；`cargo test -p plant-ui-app --lib`；对四个节点重量一遍写进 evidence。
**依赖：** T0。**文件：** `crates/plant-ui-app/src/read_face/service.rs`、`crates/plant-ui-app/src/model_update_api.rs`（把单根取法抽成 `model_records_one_root`）、`crates/plant-ui-app/src/model_record_union.rs`（如需按桶合并）。**规模：** M，1.5 人日。

#### T2 成员多时用服务端批量形

**描述：** `model_update_api` 加批量形：请求体 `generation_roots: [..]`（同库、≤ 64、不重复），响应按 `roots[]` 的逐根 `total` 把平铺的 `items` 切回各个祖先（`items` 顺序 = 请求顺序 × 每根稳定顺序）。整批超过一页时，装完的祖先照收，没装完的按条数重新分组再发，**不拿游标翻批量形**；单独一根就超过一页的走单根分页。头几发带 1、1、2、4、8 个（让首批照旧十几毫秒到），之后每发 16 个；成员 ≤ 64 个不打包；`/health.capabilities.model_records_batch_max_roots` 没声明就不打包（`Health` 结构体加 `capabilities`，`#[serde(default)]`）。批量请求回 400 / 422、或响应对不上（没有 `roots`、根名不符、条数矛盾）时这一组逐个重问、余下不再打包。常量照抄 plant3d-web（Q11，按推荐，评审已确认）；环境变量 `PLANT_UI_RECORDS_BATCH`：`0` / `1` 关掉打包（退回 T1 的逐成员取法），`N` 改稳态每发个数（上限仍 64），对应 plant3d-web 页面地址上的 `gm_records_batch`。

**验收：**
- [ ] `23717_2` 记录请求数从 532 降到约 50；记录收齐 ≤ 2.5 s；`24383_80211`（19 个 ZONE）不打包、与 T1 结果相同
- [ ] 单测覆盖：切回祖先、一页装不下重新分组、单根超页走分页、旧后端 422 退回逐个、响应对不上退回逐个、上限 64、`capabilities` 缺席 = 1（对应 plant3d-web `modelRecords.test.ts:252–464` 那一段）
- [ ] `PLANT_UI_RECORDS_BATCH=0` 时请求形状与 T1 逐字节相同；`=32` 时稳态每发 32 个；没设时 1、1、2、4、8、16…（单测）
- [ ] `/health` 多出 `capabilities` 不影响老字段解析（现有 `Health` 单测过）

**验证：** `cargo test -p plant-ui-app --lib`；evidence 补一行。
**依赖：** T1。**文件：** `model_update_api.rs`、`crates/plant-ui/src/task_queue.rs`（`Health.capabilities`）、`read_face/service.rs`。**规模：** M，1.5 人日。

### 检查点 A（期 1 完）
- [ ] 四个节点的「点击→记录收齐」全部达标；`cargo test --workspace` 过；release 构建过
- [ ] 把 T0 的基线和 T1 / T2 的数并排写进 evidence，给用户看一眼再往下
- [ ] 发 0.1.15（第八节）

### 期 2 · 边取边画与生成实体

#### T3 模型通道逐成员交付

**描述：** 把 T1 里「每收完一个成员」的时机暴露出来：`ReadFace::model_instances` 增加 `on_scope_records: &mut dyn FnMut(ScopeDelivery)` 回调（成员 refno、这一批记录、`dbnum` / `source`），`data.rs` 的模型通道把每批发成新的 `Evt::ModelScopePart { epoch, target, records, done, total }`，最后仍发 `Evt::ModelScope(epoch, target, Ok(ModelRecords { records: vec![], sources }))` 当作收口信号（空记录 + 已交付过 = 不触发「查空补齐」）。`main.rs` 收到 `ModelScopePart` 时按 plant3d-web 的节奏攒批：第一批立刻交 `View3d::append`，之后攒到不少于已装数量、或距上一批超过上批耗时的 6 倍再交；`model_scopes` / `model_owner` / `loaded_models` 的记账逐批做，收口时登记 `scope_targets`。`Evt::ModelScope` 的查空分支只在「一批都没交过」时才走。**相机**（Q15，按推荐，评审已确认）：只在落进空场景时取景——首批上屏取一次、收齐再对一次焦；往非空场景里显示别的节点不动相机。view3d 的 `should_frame_batch(scene_empty)` 只看场景空不空这条纪律不变，宿主只是在一次显示的首批与收口各评一次。

**验收：**
- [ ] `23717_2` 点击→出图 ≤ 1 s（T0 量法），画完不比期 1 慢
- [ ] 眼睛在加载途中不变（ADR-0016 的单测仍过）；房间树的取景只在首批和收口各一次（`pending_room_frame`）
- [ ] 空场景点 `23717_2`：相机在首批上屏时取一次景、收齐后再对一次焦；场景非空时再点别的 ZONE，相机不动（单测 + 手工）
- [ ] 途中换代（`model_scope_epoch` 变了）：晚到的 `ModelScopePart` 全部丢弃（单测）

**验证：** `cargo test -p plant-ui-app --lib`（含新事件的状态机单测 ≥ 4 条）；`sim` 模式下把 `model_scope_fixture` 拆成两批走一遍。
**依赖：** T1。**文件：** `read_face/mod.rs`、`read_face/service.rs`、`read_face/store.rs`（库供数一次交齐，接口兼容）、`data.rs`、`main.rs`（`ModelScope` 处理那 120 行拆成函数）。**规模：** L→拆成「读面与事件」「宿主攒批」两笔提交，2 人日。

#### T3b 显示预算（大范围装到 5 万件就停）

**描述：** 一次显示（一个 `model_scope_epoch` 里的全部目标）累计装进的记录数到 50,000 件就停止取后面的成员：模型通道不再对剩下的 ZONE 发请求（与 T8「停止」同一条取消路），已上屏的留着；没取的目标回「未加载」（不是「已隐藏」）；日志一句「范围超过 5 万件，已显示前 5 万件；要看其余请按 ZONE 分别显示」；相机按已装的取景（T3 的规则）；再点同一节点只取没取完的成员（T3 的逐成员记账，与 T8 同一套账）。预算用环境变量 `PLANT_UI_SHOW_BUDGET` 定：缺省 50000，`0` 关掉预算（量全量数用）。不预先确认——总数只有每个 ZONE 第一页回来后才知道，预先确认拿不到数（Q13，按推荐，评审已确认；代替第一轮 Q5 的「超过 5 万件先确认」）。房间树「加载模型 > 200 个单元先确认」照旧（ADR-0033 D5）。

**验收：**
- [ ] `23717_2`（79,409 件）点下去装到约 5 万件停（成员整批装，越线的那一批装完才停）；日志有那句提示；没取的 ZONE 眼睛是未加载，SITE 行半可见
- [ ] 再点 `23717_2`：只对没取完的 ZONE 发请求，把其余装齐；`24383_80211`（7,400 件）不受预算影响
- [ ] `PLANT_UI_SHOW_BUDGET=0`：全量装完，与 T3 结果相同（第十二节量全量数用它）
- [ ] 预算停下之后再点、切 MDB、取回工作，互不干扰（单测）

**验证：** `cargo test -p plant-ui-app --lib`（状态机单测 ≥ 3 条）；真连接手工一次。
**依赖：** T3。**文件：** `main.rs`、`data.rs`。**规模：** S，0.5–1 人日。

#### T4 view3d 分帧生成，材质与进度统计增量化

**描述：** `load_models` 一次只处理一个批次，单批仍可能几万条。改成按时间片生成：每帧最多用 6 ms（`Instant` 预算，网页端用 `web_time`），剩下的留在队头下一帧继续；`clears_scene()` 的 Replace 批第一帧先清场。配套两处全量扫描改增量：`apply_materials` 现在只要有新实体出生就全场重挑（`lib.rs:2794`），改成只给 `Added<ModelMesh>` 的实体挑材质，选择集 / 外观脏时才全场；`update_mesh_progress` 每帧从头数 `loading_meshes`（`lib.rs:2150–2175`），改成只检查还没落定的那部分（落定的移出列表，逐模型计数累加而不是重算——注释里担心的「多跑几帧记两次」靠移出列表来避免）。

**验收：**
- [ ] `23717_2` 加载期间最长一帧 ≤ 100 ms（T0 量法），画完 ≤ 8 s
- [ ] `apply_materials` 在 8 万实体场景里新进一批 1,000 个时只处理这 1,000 个（单测用 `load_models_app` 那套 headless App 数一次调用里挑了几个）
- [ ] 零网格记录、`is_invalid_tubi` 直线、`TubeBody` 标记、`render_states` 收口与改前逐项相同（现有 view3d 单测全过）

**验证：** `cargo test -p plant-ui-view3d`；`--look-demo` 的 `report_frame_time` 前后对照。
**依赖：** T3（没有 T3 也能做，但量不出收益）。**文件：** `crates/plant-ui-view3d/src/lib.rs`（`load_models`、`apply_materials`、`update_mesh_progress`、`ModelBatch`）。**规模：** M，2 人日。

#### T5 核实并打开 Bevy `multi_threaded`（桌面端）

**描述：** 先 `cargo tree -e features -i bevy_tasks` 核实：工作区与 vendor 里确实没有任何 Cargo.toml 打开 `multi_threaded`（审核第 5 节第 1 条；本次用 Select-String 复核过所有非 registry 的 Cargo.toml，没有）。成立则在根 `Cargo.toml` 的 bevy 特性列表加 `"multi_threaded"`（`vendor/registry` 里 `bevy` / `bevy_internal` / `bevy_asset` / `bevy_tasks` 都有这个特性，lock 不变）。网页端不受影响（`bevy_tasks` 在 wasm32 上固定单线程池）。顺带核对 `mesh_source.rs:88-91` 那条注释是否成立。

**验收：**
- [ ] 大 ZONE `17496_8516` 加载期间最长一帧比 T4 之后再降（目标 ≤ 50 ms）；点击→网格全部落定不变慢
- [ ] 整库全视帧时间不变差（`--look-demo`）；`cargo test --workspace` 过；`Cargo.lock` 无改动
- [ ] 若 `cargo tree` 显示已被别处打开，本任务改为「记录结论、关闭」，不动代码

**验证：** 现有全部单测；`--look-demo`；桌面 release 真连接走一遍四个节点。
**依赖：** T4。**文件：** `Cargo.toml`。**规模：** XS，回归 1 人日。

**提示：** T5 的 `multi_threaded` 在 vendor/registry 的 bevy、bevy_internal、bevy_asset、bevy_tasks、bevy_ecs、bevy_render 的 Cargo.toml 里都有定义，打开它不需要新下载。

### 检查点 B（期 2 完）
- [ ] 四个节点的出图 / 画完 / 最长一帧全部达标；eye 单测与 view3d 单测全过
- [ ] 给用户演示一次 8 万件 SITE 的边取边画，再往下
- [ ] `23717_2` 缺省装到约 5 万件停、再点补齐（T3b）演示一次；`PLANT_UI_SHOW_BUDGET=0` 的全量数写进 evidence

### 期 3 · 白耗与误判

#### T6 去掉 `model/roots` 预检，不留捷径

**描述：** 删掉显示路径上的库级探针（`main.rs:7279-7316` 发探针、`:3836-3869` 收 `Evt::DbnumModelProbe`、`data.rs:1198-1214`、`model_update_api.rs:248-276`），连同 `dbnum_model_known` / `dbnum_model_probing` 那套状态与 `Req` / `Evt::DbnumModelProbe` 一起删（`main.rs` 约 15 处、`data.rs` 7 处、`sim.rs` 1 处；`spatial_api.rs` 注释里提到 `dbnum_model_roots` 的那一处随函数保留）。**不留 `/dbnums` 行的捷径**：所有显示一律先查记录——没生成过的库回 200 空，毫秒级——查空再走默认补齐，与 ADR-0030 D1 一致（Q12，按推荐，评审已确认）。`model_update_api::dbnum_model_roots` 函数留着给任务队列用，不删。

**验收：**
- [ ] 显示任何节点时后端收到的 `dbnums/*/model/roots` 请求 = 0（抓后端访问日志或 `Server-Timing`）
- [ ] 后端冷重启后（`ready_total` 恒 0 的情形）点一个已生成的 ZONE：直接查记录并显示，不触发补齐
- [ ] 没初始化的库点眼睛：先查记录（空），再走默认补齐，用户看到的行为与改前相同（单测 + 手工）
- [ ] 源码里不再出现 `DbnumModelProbe`、`dbnum_model_known`、`dbnum_model_probing`（源码钉）

**验证：** `cargo test -p plant-ui-app --lib`。**依赖：** 无（可与期 1 并行）。**文件：** `main.rs`、`data.rs`、`sim.rs`、`model_update_api.rs`。**规模：** S，0.5–1 人日。

#### T7 落定重载只换变了的单元 / 库里的范围（ADR-0034）

**描述：** `source_versions.rs::observe` 改成返回变了的 dbnum 集合（接口 `observe(&rows) -> BTreeSet<u32>`，测试照改）；装载时按回执给每个范围目标记下涉及的 dbnum（`ModelRecords.sources` 已按库分桶）；`main.rs:4373-4429` 的 `QueuePoll` 处理里，`model_reload_owed` 从 bool 变成「欠重载的 dbnum 集合」，后台模型落定后只对涉及变动库或包含落定单元的范围走 `View3d::refresh`（按目标卸旧装新），其余范围不重查；树照旧刷新；没有任何范围涉及变动库时一个 `model/records` 都不发。手动「取回工作」仍是 ADR-0021 的清场重装。ADR-0034 已写并于 2026-11-12 20:01 随本计划评审通过、状态改「已接受」（边界两条——CATA 变了、只登录不改几何——见其后果 2 / 3）。加一个设置项**「落定后自动重载」**（`plant_ui::settings::Settings` 加一格 bool，`#[serde(default)]` 缺省开，随 `settings.ron` 按机器落盘，设置窗一行）：关 = ADR-0021 原文字的「只换树 + 日志提示再点取回工作」，是这一任务的回退口（第九节）。

**验收：**
- [ ] 先装 `23717_2`（7333 库），再在另一个库（如 7999）里保存一次：场景不重装、后端 `model/records` 请求 = 0、相机不动
- [ ] 在 7333 库里保存一次：只有 7333 名下的范围重载，8000 库的范围不动（单测用两个范围、两个库的夹具）
- [ ] 设置项「落定后自动重载」关掉后，在 7333 库里保存一次：只刷树 + 日志提示，不发 `model/records`、场景不动（单测）
- [x] ADR-0034 入库（已提出 → 已接受，2026-11-12 20:01），`CONTEXT.md` 的「取回工作」词条补一句「自动重装只重变了的库」（已补：「后台模型落定后的自动重载只换变了的那部分」）

**验证：** `cargo test -p plant-ui-app --lib`（`source_versions` 三条测试改成集合语义 + 新增两条）；真连接手工各一次。
**依赖：** T3（按范围 refresh 要用逐批记账）。**文件：** `source_versions.rs`、`main.rs`、`crates/plant-ui/src/settings.rs`（设置项与设置窗一行）、`docs/adr/0034-settled-reload-is-scoped-to-changed-units-or-dbnums.md`（新，已接受）、`CONTEXT.md`。**规模：** M，1.5–2 人日。

#### T8 加载中可以停

**描述：** 视口工具栏加「停止加载」（加载中才显示）：翻 `model_scope_epoch`，模型通道收到 `ModelLoad::Cancel(epoch)` 后放弃剩下的成员、在途请求靠 `tokio::select!` / `AbortHandle` 取消；已交付的批留在场景里；没取完的目标回「未加载」（不是「隐藏」，CONTEXT 术语）；再点同一目标只取没取完的成员（T3 的逐成员记账）。停止按钮放在进度条那一行，加载中才出现。与 T3b 的预算停共用同一条取消路和同一套逐成员记账。

**验收：**
- [ ] `23717_2` 画出一半时点停：≤ 100 ms 内不再发请求，已画的留着，眼睛显示未加载
- [ ] 停之后再点：只对没取完的 ZONE 发请求（请求数 < 首次）
- [ ] 停与换代（切 MDB、取回工作）互不干扰（单测）

**验证：** `cargo test -p plant-ui-app --lib`；手工。**依赖：** T3。**文件：** `data.rs`、`main.rs`、`crates/plant-ui/src/`（工具栏 VM）。**规模：** M，1–2 人日。

### 检查点 C（期 3 完）
- [ ] 三条硬指标（`model/roots` = 0、别库保存不重装、停止 ≤ 100 ms）达标
- [ ] 设置项「落定后自动重载」开 / 关各走一遍本库保存
- [ ] `cargo test --workspace`、release 构建、wasm 构建都过；evidence 齐；给用户过一遍再决定做不做期 4
- [ ] 发 0.1.16（第八节）

### 期 4 · 网页端网格（已定只做 T9）

#### T9 网页端网格限并发、自动重试

**描述：** 网页端也注册一个自定义 `AssetReader`（照桌面端 `SwappableMeshReader` 的壳），内部用信号量把在途请求限在 8 个，失败按 0.5 / 1 / 2 s 退避重试 3 次；重试仍失败的照旧进 `failed_meshes`。桌面端不变。

**验收：** 7997 整库（plant-ui 10-25 的做法：release wasm、headless Chrome）「mesh 文件加载失败」从 5,934 降到 0；整库摆进场景 < 5 分钟。
**验证：** `./scripts/build-wasm.ps1`；10-25 那套实测复跑。**依赖：** 无。**文件：** `crates/plant-ui-view3d/src/mesh_source.rs`、`lib.rs`（注册）。**规模：** S–M，1–2 人日。

#### T10 网页端网格打包取（等 gen-model 提交 `meshes/batch`）

**描述：** 在 T9 的读取器里攒批（256 键一批、4 批并发）走 `POST meshes/batch` 二进制帧流，按帧拆给各个读请求；后端没这条路由（`/health.capabilities`）时退回 T9 的逐个取。网页端从「站点静态文件」改成「从模型服务取」。

**验收：** 大 ZONE 网格请求 1,486 → ≤ 6；整库摆进场景再降一半。**依赖：** T9；gen-model 主线提交 `mesh_batch.rs`。**规模：** L（Bevy 每个资产一次读调用，要自己写合批调度），3–5 人日。**先不排期**（第十一节）。

### 最终验收
- [ ] 第一节目标表四个节点全部达标，三条硬指标达标，数写进 evidence 并与 plant3d-web 测速并排
- [ ] `cargo test --workspace`、`cargo build -p plant-ui-app --release --locked`、`./scripts/build-wasm.ps1` 全过；`Cargo.lock` 只在 T5 之外无改动
- [ ] 真机走一遍：APS 与 AMS 各一个库，点眼睛、停止、右键重新生成、别库保存、本库保存（第十节的演示脚本）
- [ ] `CHANGELOG.md` 一节；`CONTEXT.md` 词条；ADR-0034；本文件第十三节的「已落地」补齐
- [x] 第二轮六条（Q11–Q16）评审确认后，第七节去掉「待评审确认」、ADR-0034 改「已接受」（2026-11-12 20:01 批准，已改）
- [ ] 用户看过数和演示，点头后再推送

---

## 五、并行与顺序

```
T0 ──► T1 ──► T2 ──┐
        │          ├──► 检查点 A（发 0.1.15）
        └──► T3 ──► T3b ──► T4 ──► T5 ──► 检查点 B
              │
              ├──► T7（ADR-0034 评审通过后）
              └──► T8
T6（随时，与期 1 并行）────────────► 检查点 C（发 0.1.16）
T9（随时，网页端）──► T10（等后端提交）
```

已定一人做（Q16）：顺序 T0 → T1 → T2 →（检查点 A）→ T3 → T3b → T4 → T5 →（检查点 B）→ T6 → T7 → T8 →（检查点 C）→ T9。T6 不依赖别的任务，编译锁排不上时可以提前写它的代码与单测。

备忘（当初按两人写的拆法，现在不派第二人）：两个人同时做的话，一人走 T0→T1→T2→T3→T4→T5，另一人 T6→T9，之后接 T7 / T8。同一文件（`main.rs`）两人都要改时按函数拆：`ModelScope` 处理（T3）与 `QueuePoll` 处理（T7）互不相干，先抽成独立函数再各改各的。

---

## 六、风险

| 风险 | 影响 | 对策 |
|---|---|---|
| 批量形响应的 `items` 平铺顺序与 `roots[]` 对不上（服务端版本差异） | 记录归错祖先，眼睛错位 | 条数核对不上就整组退回逐个（T2 验收第 2 条）；对着 r7 后端先用回放脚本验一次形状；`PLANT_UI_RECORDS_BATCH=0` 可整个关掉 |
| 边取边画后 `model_scopes` 逐批记账，与取回工作快照（ADR-0021）、重新生成的 `regenerate_refresh` 交错 | 重装漏范围或重复装 | T3 收口事件统一登记 `scope_targets`；现有「取回工作重装」单测照跑；手工按 ADR-0030 D2 的三步走一遍 |
| `multi_threaded` 打开后暴露单线程下隐含的系统顺序假设（资产事件、`apply_materials` 与 `load_models` 同帧） | 材质缺、实体闪 | T5 放在 T4 之后，先把两处全量扫描改掉；回归 `--look-demo` 与全部 view3d 单测；不行就关掉并记录 |
| 分帧生成期间用户再点别的节点 | 批次顺序错乱 | `pending_models` 本来就是队列，分帧只是把队头切片；换代（epoch）照旧丢弃晚到的 |
| 预算停在 5 万件，用户以为整个 SITE 都装了 | 漏看一部分模型 | 日志那句提示 + 没取的 ZONE 眼睛是未加载、SITE 行半可见（ADR-0016 聚合）；再点就补；`PLANT_UI_SHOW_BUDGET=0` 可关 |
| 自动重装收窄后漏掉跨库的几何变化（元件库 CATA 改了，设计库会话没变） | 场景过时 | 源版本判据本来就不看 CATA，靠批次 / 单元落定那一路（`fresh`）兜住，否则不动、追新靠右键重新生成——已写进 ADR-0034 后果 2 |
| E3D 只登录不改几何也让会话号涨，触发本库范围重载 | 没必要的几秒重载 | 只重这个库名下的范围，第一版接受（ADR-0034 后果 3）；设置项「落定后自动重载」可关；要判「会话里有没有几何改动」得后端配合，不在本轮 |
| 测速用的后端在忙（别的会话在跑回归） | 数被搅 | 第十二节口径：记 CPU 与有无 rustc，被搅的单列；前后交替量 |
| 编译锁放行晚（内存余量 7–9 GB，门槛 14–16 GB，前面排着 4 个） | 开编与量测往后推 | 先写代码与单测（T0 脚本与埋点、T1 / T2、T6），放行再编；8022 起来前做不了基线，脚本先备好 |
| 这棵树被用户同时用来做别的事 | 分支混 | 开工前问一句；或者改用工作树（冷编 20–40 分钟） |

---

## 七、已定

### 第一轮（2026-11-12 19:16 用户：全部按推荐）

1. 测速用线上后端 8022（起来之后只发只读请求）；8022 起来之前先做 T0 的埋点与脚本。
2. 边取边画时眼睛按 ADR-0016 的聚合规则走：几何行随每批落定逐个亮起，容器行途中半可见，收齐定格；不给 ADR-0016 加例外。
3. 显示途中的「停止」：已上屏的留着，没取完的目标回未加载（不是已隐藏），再点只取没取完的成员。
4. 停止按钮放在进度条那一行，加载中才出现。
5. 大范围：原 Q5 定「超过 5 万件先确认」；第二轮 Q13 修订为预算制（见下），因为总数只有每个 ZONE 第一页回来后才知道，预先确认拿不到数。
6. 自动重装收窄到变了的单元 / 库（Q6 选 (a)，ADR-0034）；取回工作仍是手动的清场重装。
7. 在 `D:\work\plant-code\old\plant-ui` 的 master 树上开分支 `feat/model-load-parity`，做完切回 master。
8. 期 4 只做 T9（限并发 + 重试）；T10 等 gen-model 提交 `meshes/batch` 再排。
9. 期 1 完发 0.1.15，期 2–3 完发 0.1.16。
10. T5 打开 Bevy `multi_threaded`：vendor/registry 里 bevy 全家的 Cargo.toml 都有这个特性，不需要新下载。

### 第二轮（Q11–Q16，按推荐，2026-11-12 20:01 plannotator 评审已确认）

- **Q11 常量照抄 plant3d-web。** 6 路并发；成员超过 64 个才打包；一发带 16 个；头几发 1、1、2、4、8 个；每页 5000 条不变。环境变量 `PLANT_UI_RECORDS_BATCH`（`0` / `1` 关掉打包，`N` 改每发个数），对应 plant3d-web 页面地址上的 `gm_records_batch`。落在 T1 / T2。
- **Q12 去掉探针后不留捷径。** 所有显示一律先查记录（没生成过回 200 空，毫秒级），查空再默认补齐，与 ADR-0030 D1 一致；删掉 `dbnum_model_known` / `dbnum_model_probing` 那套状态（`main.rs` 约 15 处、`data.rs` 7 处、`sim.rs` 1 处）；`model_update_api::dbnum_model_roots` 函数留给任务队列。落在 T6。
- **Q13 大范围改预算制**（代替第一轮 Q5 的预先确认）。一次显示装到 50,000 件就停止取后面的成员，剩下的目标回「未加载」，日志提示「范围超过 5 万件，已显示前 5 万件；要看其余请按 ZONE 分别显示」，相机按已装的取景；再点同一节点只取没取完的成员（与 T8「停止」同一套账）；房间树「超过 200 个单元先确认」照旧。环境变量 `PLANT_UI_SHOW_BUDGET`（缺省 50000，`0` 关）。落在 T3b 与第一节目标表的补注。
- **Q14 ADR-0034 的边界。** (1) 元件库（CATA）变了、设计库会话没变：源版本判据本来就不看 CATA，后端批次落定给出单元清单（`fresh`）就按单元重载，否则不动，追新靠右键「重新生成模型」。(2) E3D 只登录不改几何也会让会话号涨（plant3d-web ADR 0072）：按现在的判据会当成「有新保存」，触发的只是这个库名下范围的重载（一个库最多几秒），第一版接受，不做「会话里有没有几何改动」的判断（那要后端配合）。写进 ADR-0034 后果 2 / 3；状态行随评审批准改为「已接受（2026-11-12 20:01 plannotator 评审批准）」。
- **Q15 相机。** 只在落进空场景时取景，首批上屏取一次、收齐再对一次焦；往非空场景里显示别的节点不动相机。落在 T3。
- **Q16 谁做、何时开编。** 现在派一人，分支 `feat/model-load-parity`；先做不需要编译的部分（T0 的回放脚本、埋点代码、T1 / T2 的代码和单测先写好），编译锁放行时再编、再量；期 1 做完交；不并行派第二人。现状：全机可提交内存余量 7–9 GB，编译锁门槛 14–16 GB，已有 4 个编译任务在排队；线上后端 8022 没在跑。落在第三、五、八节。

---

## 八、里程碑与排期

人日是净工时（含量测与回归），不含等编译锁和等 8022 的时间；一人做，日历时间看锁放行。

| 期 | 任务 | 本期人日 | 累计人日 | 交付物 | 发版 |
|---|---|---|---|---|---|
| 期 0 · 量具 | T0 | 1 | 1 | 阶段计时开关、回放脚本 `scripts/model-load-replay.mjs`、基线 evidence（四个节点各三次） | — |
| 期 1 · 取记录 | T1、T2 | 3 | 4 | `23717_2` 记录层 21 s → ≤ 2.5–3 s；T1 / T2 单测；evidence 与基线并排；检查点 A | **0.1.15** |
| 期 2 · 边取边画与生成实体 | T3、T3b、T4、T5 | 5.5–6 | 9.5–10 | 出图 ≤ 1 s、画完 ≤ 8 s、最长一帧 ≤ 100 ms；显示预算；相机纪律；`multi_threaded`；检查点 B 演示 | — |
| 期 3 · 白耗与误判 | T6、T7、T8 | 3–5 | 12.5–15 | `model/roots` = 0；落定重载（ADR-0034 已接受、设置项）；停止；检查点 C | **0.1.16**（期 2–3 一起发） |
| 期 4 · 网页端网格 | 只 T9 | 1–2 | 13.5–17 | 网页端网格失败 5,934 → 0；T10 不排 | 随下一版 |

**开编受内存制约的说明。** 2026-11-12 19:32 的现状：全机可提交内存余量 7–9 GB，编译锁门槛 14–16 GB（本计划用 `-StartCommitFreeGb 14`），已有 4 个编译任务在排队；线上后端 8022 没在跑。所以开工顺序是：开分支后先把不需要编译的都做了——T0 的回放脚本（Node，跑它只要 8022 起来）、T0 的埋点代码、T1 / T2 的代码与单测、T6 的删改——锁放行后第一轮编译就是 `cargo test -p plant-ui-app --lib`，过了再 release 构建、再量；8022 起来之前量不了基线，脚本与埋点先备好。期 1 做完交付（0.1.15），期 2 起再排；不并行派第二人。

---

## 九、回退办法

原则：能用开关关的先关，关不掉的按「一任务一到两笔」的提交逆转（`git revert`），不手改；每一期发版后出问题也按这个顺序。

| 任务 | 怎么关 / 怎么退 |
|---|---|
| T0 阶段计时 | 不设 `PLANT_UI_LOAD_TIMING` 就没有额外开销；回放脚本在 `scripts/`，不参与运行 |
| T1 按 ZONE 拆开 | 没有开关；逆转提交（接口对调用方兼容，`store.rs` 没改） |
| T2 批量形 | `PLANT_UI_RECORDS_BATCH=0` 关掉打包，退回 T1 的逐成员取法；仍不行逆转提交 |
| T3 逐成员交付 | 逆转提交（两笔：「宿主攒批」「读面与事件」，倒序退） |
| T3b 显示预算 | `PLANT_UI_SHOW_BUDGET=0` 关掉预算 |
| T4 分帧生成 | 逆转提交 |
| T5 `multi_threaded` | 改回一行 `Cargo.toml`（去掉 `"multi_threaded"`），`Cargo.lock` 不变 |
| T6 去探针 | 逆转提交 |
| T7 落定重载 | 设置项「落定后自动重载」关 = ADR-0021 原文字的「只换树 + 提示」；要整条退就逆转提交（ADR-0034 随之标「已取代」） |
| T8 停止 | 逆转提交 |
| T9 网页端网格 | 逆转提交（桌面端不受影响） |
| 发版 0.1.15 / 0.1.16 | 先用上面的开关止血，再按任务逆转、补发 |

---

## 十、验收演示脚本（给用户看，十分钟）

**准备（演示前）：** release 构建，环境变量 `PLANT_UI_LOAD_TIMING=1`；服务供数接线上后端 8022（只读）；项目 AMS，MDB 含 7999 / 1112 / 7333 三个库；日志面板、任务队列面板摆在看得见的地方；后端访问日志另开一个窗口，用来数 `model/roots` 与 `model/records`。保存那两步要有人在 E3D 里操作，演示前约好。

| 分钟 | 操作 | 看什么 | 期望 |
|---|---|---|---|
| 0–1 | 空场景，点小 ZONE `24383_91690`（7999）的眼睛 | 日志一行五个时刻；相机 | 出图 ≤ 0.3 s；相机首批取一次景、收齐对一次焦；后端 `model/roots` 请求 0 |
| 1–2 | 点大 ZONE `17496_8516`（1112） | 出图 / 画完 / 最长一帧；相机 | 出图 ≤ 0.8 s、画完 ≤ 1.5 s、最长一帧 ≤ 50 ms；场景非空，相机不动 |
| 2–3 | 点整 SITE `24383_80211`（7999） | 边取边画：SITE 行途中半可见、收齐定格 | 出图 ≤ 0.5 s、画完 ≤ 2 s；记录请求数 = ZONE 数 + 1 |
| 3–5 | 点大 SITE `23717_2`（7333） | 出图；装到约 5 万件停；日志提示；没取的 ZONE 眼睛 | 出图 ≤ 1 s；最长一帧 ≤ 100 ms；日志「范围超过 5 万件…」；没取的 ZONE 未加载、SITE 行半可见。再点一次 `23717_2`：只对剩下的 ZONE 发请求、装齐 |
| 5–6 | 重开客户端（冷态），点 `23717_2`，进度条走到一半点「停止」 | 停止 ≤ 100 ms；已画的留着 | 不再发请求；没取完的 ZONE 未加载；再点只补剩下的 |
| 6–7 | 别库保存：在 E3D 里对 7999 库 SAVEWORK 一次（只登录一次也行，会话号同样涨） | 树、场景、相机；后端日志 | 树刷新；场景不动、相机不动；`model/records` 请求 0 |
| 7–8 | 本库保存：对 7333 库 SAVEWORK 一次 | 哪些范围重载；相机 | 只有 7333 名下的范围落定重载（日志列出）；7999 / 1112 的范围不动；相机不动。再把设置「落定后自动重载」关掉重复一次：只刷树 + 日志提示 |
| 8–9 | 对小 ZONE `24383_91690` 右键「重新生成模型」 | 确认框、生成、显示 | 一个不带数字的确认框；生成完成后显示，卸旧装新；相机不动 |
| 9–10 | 收尾 | 后端访问日志；evidence 表 | 全程 `dbnums/*/model/roots` 请求 = 0；四个节点的数与 plant3d-web 测速并排 |

全量数（`PLANT_UI_SHOW_BUDGET=0` 关掉预算、`23717_2` 画完 ≤ 8 s）另起一次进程量，不占这十分钟。

---

## 十一、与其他工作线的关系

- **gen-model：** 主线冻结，本计划不改它。这一轮用到的后端接口——`model/records` 的单根与批量形、`/health` 的 `capabilities`、`tree_children`、`/dbnums`——都已在 gen-model HEAD（`refactor` @ `2ed067be0`）。`node/model-status` 与 `meshes/batch` 在 gen-model 工作区**未提交**：等它们进主线后才排 T10（网格打包取）和节点级新旧（plant3d-web ADR 0072 那套「有更新」标记在 plant-ui 的对应物），本计划不等它们。
- **plant3d-web：** 不动。只作参照（方案、测速、`modelRecords.test.ts`、`gm_records_batch`），常量与它对齐是为了日后并排对照。
- **别的 plant-ui 工作树与沙箱后端：** 不碰；只在 `feat/model-load-parity` 分支上做，只对 8022 发只读请求。
- **文档：** `CONTEXT.md` 四个新词条（出图 / 画完 / 边取边画 / 落定重载）与「取回工作」的补句、ADR-0034 已随本计划写入，评审通过后随 T7 的代码一起提交。

---

## 十二、量法（沿用 plant3d-web 的办法）

- **节点：** 小 ZONE `24383_91690`（7999，137 件）、大 ZONE `17496_8516`（1112，1,888 件、1,486 种网格）、整 SITE `24383_80211`（7999，19 个 ZONE、7,400 件）、大 SITE `23717_2`（7333，531 个 ZONE、79,409 件、966 种网格）。网页端另加 7997 整库。对照的数要逐项相同：对象数 79,409、几何 966、三角形 17,960,752。
- **指标：** 点击→第一批实体上屏（出图）；点击→网格全部落定（画完）；记录请求数与字节；加载期间最长一帧；进程内存；后端收到的 `model/roots` 请求数。
- **口径：** 冷态（每次重开客户端进程、后端不重启）；每档三次取中位数；改动前后**交替**量；release 构建；记 CPU 占用和有没有 `rustc` 在跑，开始或结束时有编译、或开始时 CPU ≥ 85% 的标「被搅」单列。大 SITE 的全量数用 `PLANT_UI_SHOW_BUDGET=0` 量；缺省（预算 5 万）另记一行「装到预算停」的出图与停下的时刻。
- **工具：** T0 的阶段计时（`PLANT_UI_LOAD_TIMING=1`）；帧时间用 `--look-demo` 的 `report_frame_time`；接口层下限用 T0 的回放脚本；后端那一段看 `Server-Timing`（`read` / `total` / `conn`）。
- **记录：** 每个检查点一份 `docs/evidence/2026-11-xx-model-load-*.md`，表头与 plant3d-web 测速 1.3 节一致，方便并排。

---

## 十三、已落地

（施工时按任务填：日期、提交号、量到的数。）
