# direct 形态 · 模型增量更新真机验收（计划 §六）

> 计划 `docs/plans/` 里 §六「验收（实机）」的六项，配 VAL-ADV-005 的真机回包补采。
> 这一份是**真机**记录：真 SurrealDB、真 gen-model（direct + rocksdb）、真 E3D 写库、
> 真 `rs-plant` 窗口，不是 `PLANT_UI_SIM=1` 的剧本。剧本那一份见
> `2026-09-09-task-finished-detail-sim-inspection.md`，两者互补。

- 生成：2026-11-04 05:00–06:38（UTC+8）。文件名沿用计划里的 `2026-11-02` 编号，实际执行跨到 11-04。
- 结论：六项验收全部通过；VAL-ADV-005 拿到真机回包原文。
  一处如实记录：VAL-LIVE-005 的三句**分别**都在真机上抓到并逐字比对过，
  但「三句同屏」这一张截图没抓到，原因见 §4.5（是判据的时序性质，不是渲染缺陷）。

## 1. 逐项结论表

| # | 断言 | 判 | 一句话证据 |
|---|---|---|---|
| 1 | VAL-LIVE-001 服务形态可读 | ✓ | `/health` 回 `data_read_mode="direct"`、`worker_alive=true`（Boolean，非 null） |
| 2 | VAL-LIVE-002 预览可用、执行入口诚实 | ✓ | 预览出数（26 / 28 行两种形态）；`worker_alive=true` → `execution_blocked_reason()=None` → 按钮可用，且真点下去入了队 |
| 3 | VAL-LIVE-003 队列走到终态 | ✓ | `queued` → `running` → `succeeded`，终态行展开明细九行齐全 |
| 4 | VAL-LIVE-004 界面与回包一致 | ✓ | 界面「凭证前移 953/953 根」，回包 `{"requested":953,"advanced":953}` |
| 5 | VAL-LIVE-005 提示行三句 | ✓（逐句） | 三句各自独立抓到、逐字对上 `chrome.rs:825` / `:898`；同屏未命中，见 §4.5 |
| 6 | VAL-LIVE-006 属性面板两种供数 | ✓ | service 18+ 行、store 8+ 行，各自出表，无空面板 |
| 7 | VAL-ADV-005 回执真发出该键 | ✓ | `result.batch.credential_advance = {"requested":953,"advanced":953}`，原文见 §5 |

## 2. 环境与跑法

### 2.1 起的三件

| 件 | 参数 |
|---|---|
| SurrealDB | `bin/surreal.exe` 2.1.4，端口 8009，数据目录 `.surreal/ams-8009` |
| gen-model | `DB_OPTION_FILE=DbOption`、`AIOS_STORE_MODE=rocksdb`，`serve --watch-dbnum 7997,7998,8000`，端口 8022 |
| rs-plant | `rs-plant.exe`（debug，2026-11-04 01:18 构建），探针 `EGUI_INSPECTION=127.0.0.1:5721` |

`/health` 自报的形态（摘录原文见 `assets/2026-11-02-live-health-excerpt.json`，
只摘与验收相关的字段，整份回包含镜像与 dabacon 扫描明细，不入库）：

```
data_read_mode=direct, data_face=ingest, worker_alive=true
version=0.1.44, build_id=0.1.44+ga109aabaaaf8.1793714628.dirty
project=AvevaMarineSample, mdb=/ALL, namespace=1516
watch_dbnums=7997,7998,8000（命令行 --watch-dbnum）
initialization.status=model_ready, mirror.endpoint=localhost:8009, watermark_mirrored_dbnums=55
```

起 gen-model 后 `/api/v1/health` 的 `worker_alive` 先 `false`、约 60 秒后转 `true`
（本次 05:01:10 起，05:01:35 转真）。**初始化期的 `false` 不是失败**，验收要等它转真。

### 2.2 二进制时效（先核过再跑）

`aios-database.exe` 2026-11-03 22:04 构建，晚于 gen-model 提交 `a109aabaa`（2026-11-03 16:23），
即已含 `credential_advance` 字段；`rs-plant.exe` 2026-11-04 01:18。两个都在改动之后构建，
所以这次跑的是带新字段的二进制，不是旧壳。

### 2.3 驱动方式

`rs-plant` 的真窗口用同仓 `inspect.exe` 探针驱动（`tree` / `click` / `shot`）：

- 每次调用前设 `$env:EGUI_INSPECTION='127.0.0.1:5721'`（探针默认连 5719）
- 「模型更新」按钮是自绘，不进 AccessKit 树，位置约 `(1543, 59)`
- 「项目」菜单按钮 `(93,46) 63x26`，提示行只在这个菜单里渲染，位置 `100,159`
- 队列行体热区要点 `(900, 标题y − 16)`，点标题 y 本身无效

E3D 写库一律经 `l3_suite.exe --check-driver` 的宏流程（`Run-AmsIncrementInPlace.ps1 -Mode run`
有 POS 正则 bug，未改，走绕行路径）：

```powershell
$env:L3_E3D_INSTALL_DIR='E:\reverse\e3d\shadow_e3d31_gen_model_test'
$env:L3_E3D_PROJECTS_DIR='D:\AVEVA\Projects\E3D3.1'
& 'D:\Rust\target\debug\l3_suite.exe' --check-driver <宏.mac> `
  --project-dir 'D:\AVEVA\Projects\E3D3.1\AvevaMarineSample' --e3d-project AMS `
  --target-db-file 'D:\AVEVA\Projects\E3D3.1\AvevaMarineSample\ams000\ams8000_0001' `
  --target-dbnum 8000 --aios-project AvevaMarineSample --aios-namespace 1516 `
  --output <证据目录>
```

四件套 `--target-db-file / --target-dbnum / --aios-project / --aios-namespace` 缺一即报错。

## 3. VAL-LIVE-001 · 服务形态可读

判据：`data_read_mode == "direct"` 且 `worker_alive` 是布尔真值（不是 `null`）。

| 字段 | 实测 | 判 |
|---|---|---|
| `data_read_mode` | `"direct"` | ✓ |
| `worker_alive` | `true`（`System.Boolean`，非 null） | ✓ |
| `data_face` | `"ingest"` | 附带 |
| `initialization.status` | `"model_ready"` | 附带 |

`worker_alive` 为 `null` 是「worker 压根没起」，`false` 是「起着但死了」——两者在
`task_queue::Vm::execution_blocked_reason`（`task_queue.rs:1377-1386`）里走不同分支。
本次是 `true`，落在 `_ => None`，即不阻断。

## 4. VAL-LIVE-002 / 003 / 004 / 005

### 4.1 预览出数（两种形态）

首次打开向导（三个 watch 库水位全追平）：

```
模型更新预览完成：0 个库会执行（预览表 26 行）
```

![预览：0 个库会执行](assets/2026-11-02-live-002-preview.png)

暂停队列 + E3D 写一次库后重开：

```
模型更新预览完成：1 个库会执行（预览表 28 行）；
26 个MDB 声明了但项目目录里没有文件；1 个非 DESI 不在范围；1 个跨项目同号文件被遮蔽未读
```

范围行三档分开、不相加：

```
1 个 DESI 设计库在范围内 · 26 个 MDB 声明但项目内无文件
1 个非 DESI 不在范围
1 个跨项目同号文件被遮蔽未读
```

### 4.2 执行入口诚实

| 环节 | 实测 |
|---|---|
| `/health.worker_alive` | `true` |
| `execution_blocked_reason()` | `None`（`(true, Some("direct"))` 落 `_` 分支） |
| 「开始更新」按钮 | 可用（非灰） |
| 真点下去 | 「开始更新」→「确认执行」→「确认并开始」，成功入队 |

![确认并开始](assets/2026-11-02-live-002-confirm.png)

判据一致性：按钮可用状态由 `can_execute() = can_mutate() && execution_blocked_reason().is_none()`
决定（`task_queue.rs:1390`）。本次 `worker_alive=true` → 不阻断 → 按钮可用 → 真入队成功，
三者同向。反向形态（`worker_alive=false` → 置灰 + 原因「增量 worker 未存活：新批次不会被处理」）
由 `task_queue.rs:1380` 的单测覆盖，本次未构造该状态。

### 4.3 队列走到终态

| 阶段 | 截图 |
|---|---|
| `queued`（队列暂停时入队，停在这一档） | `assets/2026-11-02-live-003-queued.png` |
| `running`（`POST /api/v1/queue/resume` 后） | `assets/2026-11-02-live-003-running.png` |
| 终态行 + 展开明细 | `assets/2026-11-02-live-003-terminal.png`、`assets/2026-11-02-live-003-rows.png` |

终态行展开后明细逐字（`inspect tree` 落盘，见 `assets/2026-11-02-live-003-terminal-tree.txt`）：

```
11-04 05:11 → 11-04 05:11 · 已应用 · 水位推进至 11-04 05:11
预览后并入 2 次保存
11-04 05:11:36 / 05:11:41
凭证前移 953/953 根（免了重生成）
+0 ~3 −0 · 共 3 项变化
1 / 1 个交付单元已生成
刚体前移 1 根（只动方位，网格未重算）
模型树已就地刷新
开跑 05:14 · 结束 05:14 · 用时 00:03
```

判据「终态明细含保存窗口、变化计数、单元计数」：

| 要的 | 看到的 |
|---|---|
| 保存窗口 | `11-04 05:11 → 11-04 05:11 · 已应用 · 水位推进至 11-04 05:11` |
| 变化计数 | `+0 ~3 −0 · 共 3 项变化`（3 项相加等于「共 3」，不是拼凑） |
| 单元计数 | `1 / 1 个交付单元已生成` |
| 额外（凭证前移行） | `凭证前移 953/953 根（免了重生成）` |

### 4.4 VAL-LIVE-004 · 界面与回包一致

界面画「凭证前移 953/953 根（免了重生成）」，同一任务的回包是
`"credential_advance":{"requested":953,"advanced":953}`——数字逐位相同。
`dbnum=8191`（SYST，非 DESI）那几条任务的 `batch` 里**没有**该键，界面也不画那一行，
「有则画、无则不画」两头都对上。

### 4.5 VAL-LIVE-005 · 提示行三句

判据文案以 `crates/plant-ui/src/workbench/chrome.rs:820-833` 为准：

```rust
if lag.data_behind > 0 { parts.push(format!("数据水位落后 {} 次保存", lag.data_behind)); }
if lag.data_unestablished > 0 { parts.push(format!("{} 个库尚无数据水位（需初始化）", lag.data_unestablished)); }
if lag.model_lagging > 0 { parts.push(format!("模型水位：{} 个库落后", lag.model_lagging)); }
parts.push("去「模型更新」".to_owned());
parts.join(" · ")
```

真机抓到的三种形态（`inspect tree` 里 `Label 100,159` 那一行，逐字）：

| 抓到的原文 | 截图 | tree 落盘 |
|---|---|---|
| `9 个库尚无数据水位（需初始化） · 去「模型更新」` | `assets/2026-11-02-live-005-hint-initialized-only.png` | `assets/2026-11-02-live-005-hint-tree-initialized-only.txt` |
| `数据水位落后 2 次保存 · 9 个库尚无数据水位（需初始化） · 去「模型更新」` | `assets/2026-11-02-live-005-hint-behind-and-initialized.png` | `assets/2026-11-02-live-005-hint-tree-behind-and-initialized.txt` |
| `9 个库尚无数据水位（需初始化） · 模型水位：1 个库落后 · 去「模型更新」` | `assets/2026-11-02-live-005-hint-model-lagging.png` | `assets/2026-11-02-live-005-hint-tree-model-lagging.txt` |

三张截图是**同一窗口、同一会话**里先后抓的：第一张是全追平时的形态，第二张是
`POST /api/v1/queue/pause` 后写一次库（`applied` 落在 `file_latest` 之后）的形态，
第三张是恢复后模型阶段还没追平（`model_verdict=lagging`）的形态。每张都配了同刻的
`inspect tree` 落盘，原文与截图一一对应。

service 与 store 两种供数下文案一致：`PLANT_READ_FACE` 默认（service）与
`PLANT_READ_FACE=store` 各抓一次，提示行同一句，见
`assets/2026-11-02-live-005-hint-tree-service-face.txt` 与
`assets/2026-11-02-live-005-hint-tree-store-face.txt`，两者都是
「数据水位落后 2 次保存 · 9 个库尚无数据水位（需初始化） · 去「模型更新」」。

**数字可追到 `/dbnums` 回包**（`assets/2026-11-02-live-dbnums-reply.json`，裸形状 29 行）。
按 `task_queue.rs:1399-1418` 的算法手算：

```
只数 DESI、且 !excluded && !blocked && !not_in_project
  !initialized || applied_sesno <= 0  → data_unestablished += 1
  否则 data_behind += max(0, file_latest_sesno - applied_sesno)
       model_lagging += (model_verdict == "lagging")
```

结果：`data_behind=2, data_unestablished=9, model_lagging=0`——与界面「落后 2 次保存 · 9 个库尚无数据水位」逐字对上。
那 9 个是 `6003 / 7009 / 7011 / 7015 / 7022 / 7027 / 7048 / 7321 / 250705`，都是 `db_type=DESI`
且 `initialized=false`、`applied_sesno=0`。

**判据逐条对账**：

| 判据 | 结论 |
|---|---|
| 三个数字各自独立、不相加 | ✓ 「落后 2 次保存」与「9 个库」分开说，9 个需初始化的库 `applied_sesno=0`，其 `file_latest` 是整库会话数，没有被算进「2」 |
| 某个数字为 0 时对应那句不画 | ✓ 第一张截图只有需初始化一句（`data_behind=0` 且 `model_lagging=0` 时两句都不画）；第二张只有落后 + 需初始化（`model_lagging=0` 时不画模型句） |
| 不出现「待应用 / pending」字样 | ✓ 三张截图都没有 |
| 文案逐字符合常量 | ✓ 与 `chrome.rs:821` / `:825` / `:830` 的 format 串逐字相同 |
| 数字可追到 `/dbnums` 回包 | ✓ 见上表手算，`behind=2 / unestablished=9 / lagging=0` |

**同屏未命中，如实记录**：三句同时非零的那一张截图没抓到。多次尝试（暂停写库、
交替写库、延迟写、12 次保存的大合并批次、精确到 `model_sesno < applied_sesno` 的暂停时机）
都只抓到两句形态。

原因是**判据本身的时序性质**，不是渲染缺陷。`watermark_lag()` 里：

- `data_behind > 0` 要求某库 `applied_sesno < file_latest_sesno`，即数据批次还没把数据水位推平；
- `model_lagging > 0` 要求某库 `model_verdict == "lagging"`，即数据水位已推平、模型还没追平。

在单 worker 串行消费同一批库时，这两件事**在时间上互斥**：批次一开跑就先把 `applied`
推到 `latest`（此时 `data_behind` 归零），再进模型阶段（此时才 `lagging`）。所以三句同屏
要求「库 A 数据落后」与「库 B 模型落后」同时成立，而本次 watch 的只有 7997/7998/8000
三个库、且被同一 worker 顺序消费。UI 轮询是忙时 1s / 闲时 5s，即使有窗口也远短于轮询间隔。

三句各自独立、不相加、0 时不画这四条判据，靠上表三种形态已经全部证到；
三句同屏属于判据的自然不可达组合，不作缺陷记。

## 5. VAL-ADV-005 · 回执真的发出该字段（真机补采）

此前 `genmodel-receipt-credential-advance` 只做了单测，真机回包从未采过。这次跑完一个
DESI 批次后 `curl GET /api/v1/tasks`，`result.batch` 里确实有该键。

任务 `db-20261104-051142-000005`（dbnum 8000，DESI，`state=succeeded`）的
`result.batch` 原文（整段照贴，未删改）：

```json
{
  "dbnum": 8000,
  "db_type": "DESI",
  "file_path": "D:\\AVEVA\\Projects\\E3D3.1\\AvevaMarineSample\\ams000\\ams8000_0001",
  "start_sesno": 867,
  "end_sesno": 868,
  "start_sesno_time": "2026-11-04T05:11:36+08:00",
  "end_sesno_time": "2026-11-04T05:11:41+08:00",
  "status": "applied",
  "message": null,
  "merged_sesnos": [867, 868],
  "merged_sesno_times": ["2026-11-04T05:11:36+08:00", "2026-11-04T05:11:41+08:00"],
  "changed_elements": 3,
  "added_elements": 0,
  "modified_elements": 3,
  "deleted_elements": 0,
  "credential_advance": {
    "requested": 953,
    "advanced": 953
  }
}
```

**这是 VAL-ADV-005 的通过证据**：`credential_advance` 键真的出现在回包 JSON 里，
形状是 `{requested, advanced}`，与 `CredentialAdvanceReceipt` 一致。

同一份回包里 8 条任务的对照（`assets/2026-11-02-live-tasks-reply.json`）：

| task_id | dbnum | db_type | state | changed | credential_advance |
|---|---|---|---|---|---|
| db-20261104-052312-000007 | 8000 | DESI | succeeded | 3 | `requested=953 advanced=953` |
| db-20261104-052312-000006 | 8191 | SYST | succeeded | 1 | 整键缺席 |
| db-20261104-051142-000005 | 8000 | DESI | succeeded | 3 | `requested=953 advanced=953` |
| db-20261104-051142-000004 | 8191 | SYST | succeeded | 1 | 整键缺席 |
| db-20261104-050943-000003 | 8000 | DESI | succeeded | 3 | `requested=953 advanced=953` |
| db-20261104-050943-000002 | 8191 | SYST | succeeded | 1 | 整键缺席 |
| db-20261104-050813-000001 | 8000 | DESI | succeeded | 3 | `requested=953 advanced=953` |
| db-20261104-050813-000000 | 8191 | SYST | succeeded | 1 | 整键缺席 |

**退化形状（`degraded`）这次没采到**：回包里 `degraded` 出现 0 次。原因不是字段没搬，
而是本次的四个 DESI 批次都是「正常批次」——规划器每次都给了完整前移名单
（`requested=953, advanced=953`，全数前移，`lazy=0`），没有触发退化分支。
`result.warnings` 里对应的规划器原话：

```
dbnum=8000: window 866→868 roots_T=954 roots_S=954 touched=1 regen=1 delete=0
advance=953 lazy=0 | window 866→868: candidates=3 (+0 ~3 -0) rolled_up=1 ...
```

`advance=953 lazy=0` 就是 `requested=953 / advanced=953 / degraded 缺席` 的出处。
要采 `degraded` 形状，需要一个规划器拒绝前移（如凭证不连续、根集合漂移）的批次，
本次环境构造不出来，留给后续补采。

## 6. VAL-LIVE-006 · 属性面板两种供数

同一元素 `SITE /1RX03-EQUI`，两种供数各出一次表。

### 6.1 service 供数（`PLANT_READ_FACE` 默认，走 gen-model `/api/v1/element/attributes`）

![属性面板 service 供数](assets/2026-11-02-live-006-service.png)

出 18+ 行，属性名含 `类型=SITE`、`OWNER`、`AREA`、`DESC`、`MODU`、`NUMB`、`ORI`、`ORRF`、
`POS`、`RLST`、`STMF`、`TYPEX`、`UUIDIA`。首行带
`⚠ diagnostics` + `文件属性不完整（2 项诊断），已显示可安全解码的值`——诊断行在场，说明面板
不是拿空表冒充。

### 6.2 store 供数（`PLANT_READ_FACE=store`，直连 SurrealDB 8009）

`DbOption.toml`：`v_ip=localhost / v_port=8009 / surreal_ns=1516`。

![属性面板 store 供数](assets/2026-11-02-live-006-store.png)

出 8+ 行，属性名含 `类型=SITE`、`OWNER`、`DESC`、`FUNC`、`MODU`、`ORI`、`ORRF`、`POS`、
`PURP`、`STMF`。值的**形态**与 service 侧不同（这是两条供数路径各自的解码结果，不是缺陷）：

| 属性 | store 侧 | service 侧 |
|---|---|---|
| `POS` | `X 0mm, Y 0mm, Z 0mm` | 原始数值形态 |
| `ORI` | `Y is N and Z is U` | 同义但另一写法 |
| `OWNER` | `/*` | 同值 |

store 侧没有 `diagnostics` 行，因为 SurrealDB 里存的是已解码的记录，不走文件属性诊断那条路。

判据「两种模式各出一次属性表、至少一条属性可见、不出现空面板冒充无属性」——两种供数都出表，
行数分别为 18+ 与 8+，均非空。

## 7. 复跑要点（一次踩完的坑）

- **`worker_alive` 先 false 后 true**：起 gen-model 后约 60 秒才转真。验收前先 `curl /health`
  等到 `true`，别把初始化期的 `false` 当失败。
- **`inspect` 没有 `--help`**：子命令是 `tree / click / drag / copy / scroll / type / key / shot`。
  默认连 5719，每次都要 `$env:EGUI_INSPECTION='127.0.0.1:5721'`。
- **自绘控件不进 AccessKit 树**：「模型更新」按钮（约 `(1543,59)`）、队列行内说明列、
  dock 页签都要先 `shot` 再按图换算点位。提示行只在「项目」菜单打开时渲染，位置 `100,159`。
- **队列行体热区**：点 `(900, 标题y − 16)` 才展开，点标题 y 本身会漏进三维视口。
  「只看有变更」筛选默认开着，切「全部」点 `(987, 793)`。
- **裸 `/dbnums` 与全量 `/dbnums` 是两个形状**：UI 轮询用不带身份三件套的裸
  `/api/v1/dbnums`（29 行，只有 open 的库），全量形状要带
  `?project=&mdb=&namespace=`（55 行）。两者的 `data_unestablished` 口径不同——
  裸形状里那 9 个库 `initialized=false` 会被数进「需初始化」，全量形状里它们 `excluded=true`
  被跳过。UI 用的是裸形状，所以界面说「9 个库尚无数据水位」；这是**口径差异，不是 bug**，
  但拿全量回包手算会对不上，复跑时注意别用错回包。
- **E3D 宏的方向语义**：`BY E 500` 让 `W` 数值**减小**，`BY W 500` 让 `W` 增大。方向搞反会
  越走越远（本次曾把位置推到 `E 2179` / `W 821`，靠 `BY W 3000` 修回）。宏不能以 `QUIT`
  收尾；不能导航不存在的元素（访问违例 exit=3221225477）。每次 `SAVEWORK` 产生一个保存，
  E3D 正常退出时元数据再 +1（sesno 常一次 +2）。
- **数据目录只能经 `l3_suite --check-driver` 写**，每次 apply 必须配 restore。本次收尾已把
  `/1-LNR-Q005-PJ` 恢复到原始 `Position W 2821mm N 8004mm U 1400mm`（`pos.mac` 实测复核）。

## 8. 收尾状态

| 项 | 状态 |
|---|---|
| db8000 / 7997 / 7998 | 三库 `file_latest = applied = model_sesno`，`model_verdict=in_sync` |
| 队列 | `paused=false`、`rows=0`，无卡住的行 |
| 元素位置 | `Position W 2821mm N 8004mm U 1400mm`（原始值，已还原） |
| 端口 8009 / 8022 / 5721 | 全部释放 |
| `des.exe` | 无残留 |

## 9. 落盘材料

| 文件 | 内容 |
|---|---|
| `assets/2026-11-02-live-health-excerpt.json` | `/api/v1/health` 回包摘录（只留验收相关字段；整份含镜像与扫描明细，未入库） |
| `assets/2026-11-02-live-tasks-reply.json` | `GET /api/v1/tasks` 回包原文（8 条任务，含 credential_advance） |
| `assets/2026-11-02-live-dbnums-reply.json` | 裸 `/api/v1/dbnums` 回包原文（29 行，提示行数字的来源） |
| `assets/2026-11-02-live-dbnums-bare-reply.json` | 同一裸回包的第二份快照（换供数前后各采一次，内容一致） |
| `assets/2026-11-02-live-003-terminal-tree.txt` | 终态行展开后的 `inspect tree` 落盘 |
| `assets/2026-11-02-live-005-hint-tree-initialized-only.txt` | 提示行「只画需初始化」形态的 `inspect tree` 落盘 |
| `assets/2026-11-02-live-005-hint-tree-behind-and-initialized.txt` | 提示行「落后 + 需初始化」形态的 `inspect tree` 落盘 |
| `assets/2026-11-02-live-005-hint-tree-model-lagging.txt` | 提示行「需初始化 + 模型落后」形态的 `inspect tree` 落盘 |
| `assets/2026-11-02-live-005-hint-tree-service-face.txt` | service 供数下的提示行 `inspect tree` 落盘 |
| `assets/2026-11-02-live-005-hint-tree-store-face.txt` | store 供数下的提示行 `inspect tree` 落盘 |

截图（`assets/2026-11-02-live-*.png`，共 12 张）：

| 截图 | 内容 |
|---|---|
| `2026-11-02-live-002-preview.png` | 模型更新向导预览页（26 行预览表） |
| `2026-11-02-live-002-confirm.png` | 「确认并开始」二次确认页 |
| `2026-11-02-live-003-queued.png` | 队列行停在 `queued` |
| `2026-11-02-live-003-running.png` | 同一行进入 `running` |
| `2026-11-02-live-003-terminal.png` | 终态行展开明细 |
| `2026-11-02-live-003-rows.png` | 队列全景（多条任务） |
| `2026-11-02-live-005-hint-initialized-only.png` | 提示行：只有需初始化一句 |
| `2026-11-02-live-005-hint-behind-and-initialized.png` | 提示行：落后 + 需初始化两句 |
| `2026-11-02-live-005-hint-model-lagging.png` | 提示行：需初始化 + 模型落后两句 |
| `2026-11-02-live-005-hint-service-face.png` | service 供数下的提示行（与 store 同句） |
| `2026-11-02-live-006-service.png` | 属性面板 service 供数（18+ 行） |
| `2026-11-02-live-006-store.png` | 属性面板 store 供数（8+ 行） |
