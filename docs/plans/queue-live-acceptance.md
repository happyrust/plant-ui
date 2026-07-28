# 任务队列 · 真服务实机验收手册

- 日期：2026-07-28（plant-ui-103 会话起草；只固化操作步骤，不动代码）
- 还的是哪笔账：`task-queue-rollout.md` 六之四/六之五/六之七与第十节反复记的
  「实机 curl 验收仍然欠着」——至今客户端只对过**替身服务**（`output/queue-mock.ps1`）
  与**旧版真服务**（8021 上的 0.1.3，`/queue` 404，只验到了诚实降级）。
- 方案出处：第十节第 3 条（副本库 + `sync_live = true` + 放宽到十几个中等库含一个大库）、
  第九节第 9 条（不动实库）、第十节第 1 条（先修 H1 再验收——已随第十节那批修复落进
  gen-model 工作树，**尚未提交**）。
- 逐条口径基准：`design/QUEUE-FIELD-MAP.md`；逐条核对清单：`.review/QUEUE-REVIEW-CHECKLIST.md`
  一/二/三节（那份是一次性暂存，验收时若已删，按本文件第五节代替）。

---

## 一、前置条件（三件，缺一不开工）

1. **gen-model 侧把未提交的队列层修复入库。** 第十节记的 H1（task_id 熵）/ H3（锁中毒）
   与四条中等修复当时「242 单测全绿、未提交」。验收对着未入库的工作树跑，出了问题
   没有 `git diff` 可看——这正是第一节卡口一的教训，不重蹈。
2. **构建与端口窗口。** Windows 下运行中的 exe 不能被覆盖：8021 的旧服务
   （`aios-database`）不停，`cargo build` 连链接都过不去；若 8021 还被 ensure sweep
   （`scripts/Invoke-EnsureSweep.ps1`）占着，先等它收敛或换端口。
3. **副本库。** 复制数据目录、另起端口（下文记作 `:18021` 服务 / 副本数据库自用端口）。
   **两条红线**：不动 8009/8000 实库；**绝不在同一份数据上起第二个带 worker 的进程**
   ——worker 是无条件 spawn 的，两个消费者并发 freeze 会破坏 FIFO 串行、可能重复应用
   数据批次（六之七末尾、第九节第 9 条都记过）。

范围放宽的现行口径按 ADR-0013：**范围 = 当前 MDB 声明的 DESI**，不再是
`manual_db_nums`。放宽 = 让副本上的当前 MDB 声明十几个中等库 + 一个大库
（如 db7333，1 724 个待应用会话——批次要跑得够久，冻结才稳定可复现）。
若副本服务还是旧口径，退回改它的 `manual_db_nums`，并在验收记录里注明口径。

`sync_live = true`：不开的话 `async_watch` 压根不跑，ADR-011 §2「手动与自动合流」
这个核心命题验不到（第十节第 3 条理由 ①）。

---

## 二、服务端 curl 清单（客户端不在场也能全部做完）

基址 `http://127.0.0.1:18021/api/v1`，PowerShell 用 `curl.exe`（别用 alias）。

| # | 命令 | 必须看到 | 出处 |
|---|---|---|---|
| 1 | `curl.exe -s $base/health` | `started_at`（进程启动时刻）、`gen_spatial_tree`、`queue_paused`、`worker_alive`、`worker_idle_secs`、`version` ≠ 0.1.3 | 服务端第 7 项 + 第十节 H2 止血 |
| 2 | `curl.exe -s $base/dbnums` | 每行带 `anomaly` / `blocked` / `excluded`；在范围外的 DESI 是 `excluded`，五种异常里只有 `path_migrated` 不阻断 | 服务端第 8 项、ADR-0013 |
| 3 | `curl.exe -s $base/queue` | 快照 `{rows[], paused}`；行有 `task_id`（前缀 `db-`）、`dbnum`、`state`、会话区间 | 服务端第 6 项 |
| 4 | `curl.exe -s "$base/tasks?kind=data_batch"` | `queued` / `running` / 终态行；`created_at`（入队）与 `started_at`（开跑）是两个时刻 | ADR-011 §3 |
| 5 | `curl.exe -s -X POST $base/update/execute -H "Content-Type: application/json" -d '{"project":"...","mdb":"/ALL"}'` | 202 入队回执 `{scanned, enqueued:[{task_id,dbnum,position,…}], merged, already_covered, blocked, up_to_date, warnings}`，**不再是单个 task_id**，也永远不该见到 409/422 | 六之三、六之五契约修正 |
| 6 | `curl.exe -s -X POST $base/queue/pause` → **重启服务** → `/health` | 重启后 `queue_paused` 仍为 `true`，队列重建完不出队；`/queue/resume` 后恢复 | 第 9 项（暂停活过重启）|
| 7 | `curl.exe -s $base/update/pending-units` | 欠账单元清单（客户端「欠 N 个单元」的唯一来源）| QUEUE-FIELD-MAP §1 |

**排队 / 合并 / 冻结的实况**（这一段是整笔验收的核心，全部只用 4 号端点观察）：

1. 向范围内多个库连续保存会话（或起量大的初始化批次），看 FIFO 多库排队。
2. 对**排队中**的库再保存：不另开行，目标会话号被推高（`merged`）。
3. 对**运行中**的库（挑大库，批次跑得久）保存：另开一行接在后面
   （`BehindRunning`），同一 dbnum 恰好两行——`one_dbnum_occupies_at_most_two_rows`
   那条不变式在真服务上的样子。
4. 冻结口径按第十节第 5 条：入队时的 `end_sesno` 是**预期上界**，执行开始那次重扫
   回写真实值（`record_frozen_end`）。抓一次「排队期间又存了会话」的批次，比对
   终态 `DataBatchResult.start/end_sesno` 与入队时的区间。

## 三、客户端对连清单

副本服务起好后，把 `web/config.json` 的 `model_api_url` 指到 `:18021`
（键名见 `web/config.example.json`；`db.mdb` / `namespace` 保持与副本一致，
否则身份闸门会把写操作全部拦下——那本身也是一条要验的形态）。

| # | 看什么 | 判据 | 口径出处 |
|---|---|---|---|
| 1 | 向导预览 → S2 | 范围行三个数分开（DESI 在范围 / MDB 声明但项目内无文件 / 非 DESI 排除）；`ManualUpdatePreview.mdb` 回显与本端一致 | ADR-0013、S2 |
| 2 | 预览期间有批次在跑 | 汇总列出现「N 个库正在应用，以上数字可能偏大」警示条；没有批次时整条不画 | ADR-0011、S2 |
| 3 | 确认执行 | 回执进日志（入队/并入/阻断分开说）、窗口关闭、焦点跳「任务队列」页签 | 六之五 |
| 4 | 队列面板常态（S12） | 运行中置顶且默认展开；排队行「排在第 N 位」只数 queued；已排/已用两个起点；同 dbnum 两行时下面那行带「上一批已冻结，这是之后新存的会话」 | QUEUE-FIELD-MAP §1/§8 |
| 5 | 暂停（S12-B） | 横幅只说「不再出队」，正在跑的批次照常跑完；重启服务后「队列是按水位重建的；排队时长从重启起算…」与「仍处于暂停」同屏 | §4、第七节 |
| 6 | 断线降级 | 明细区转「状态未知」+「落后 N 条」，进度条照常走轮询 | §5、ADR-0005/0007 |
| 7 | 状态栏 | `队列 N` / `队列 N · 已暂停`；轮询失败时**整格不画**（不摆假 0） | 六之五 |
| 8 | 批次终态 | 日志出现「取回工作完成：已加载元素 N → M」，模型树与三维**不点任何菜单**自己换新 | c06732c 回路 |
| 9 | 「本期不执行」 | 阻断与排除分档；排除文案区分「非 DESI（CATA）」与「DESI，但不在当前 MDB 声明的名单里」 | 六之七、Q2 |
| 10 | 房间泳道 | 副本数据没有房间结构 → 只说「房间增量没开 / 还没有过房间轮记录」，不摆 0 | 六之二、六之七 |
| 11 | 身份闸门 | 故意把本端 `db.mdb` 改错：暂停/恢复/立刻扫一遍置灰且悬停说明原因，预览直接给 identity_mismatch 失败页 | `task_queue::Vm::can_mutate` |

六个行状态（排队中 / 应用中 / 生成中 / 已完成 / 部分完成 / 失败）至少拍到五个
——「失败」不必强求，天然出现时截图即可；「部分完成 · 欠 N 个单元」可拿一个
带坏单元的库触发。

## 四、归档与收尾

- 截图归 `output/queue-live-*.png`（常态 / 暂停 / 重建 / 断线 / 本期不执行至少五张），
  记下 `version` 与 `started_at`。
- 验收通过后要动的三处文档账：rollout 一句话现状与 ADR-011 状态段的「欠实机」删除；
  `QUEUE-REVIEW-CHECKLIST` 第四节那句「提交信息要写未经实机」失效；
  第十一节 Q3/Q4 的前提数字（`/dbnums` 实际行数、队列行数）如实回填，
  再决定那两笔改不改。
- 验收**不通过**的每一条，按「先记后改」写回 rollout 新开一节，不当场改代码。

## 五、若 `.review/QUEUE-REVIEW-CHECKLIST.md` 已删

逐条核这十二条口径：位置只数 queued 且不随筛选变；同 dbnum 两行不像重复项；搜索绕开
筛选芯片；跨项目过滤有微字；未知 state 不兜底成排队中；「有变化」= running + queued +
欠单元的 partial；已排/已用两个起点；应用中→生成中以第一条单元事件（轮询侧等价观测
`total_units` 出现）为界；重建横幅后半句不省；暂停只说不再出队；没快照不等于队列为空；
ETA / 阶段一分母 / ZONE 分组 / 单条取消四样不许出现。
