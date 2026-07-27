# 预览也做成异步任务，与执行对称

`POST /api/v1/update/preview` 现在是个同步处理器：`update_preview` 直接 await 掉
`preview_manual_update(&project)`，再把整个 `Preview` 当响应体返回，客户端那侧的超时给到 **600 秒**。
而预览要逐个 dbnum 打开设计库文件、按会话号比对，单库可达数分钟——界面上却只有一个转圈，
十分钟里说不出任何进展，人只会以为卡死了。

执行侧早就不是这样：`update_execute` 走 `TaskRegistry::new_task_id("mu")` +
`insert_running(kind, project)`，返回 202 加一个 `task_id`，进度经 `/api/v1/ws` 推、终态落在
`GET /api/v1/tasks/{id}`。**这套设施是按 kind 泛化的**，任务注册表、任务查询、WS 主题都不认死
manual_update。预览换个 kind 就能复用，不是新建一套。

所以：预览照 `update_execute` 抄一遍。202 + `task_id`；扫描期间按 dbnum 发
`PreviewDbStarted { dbnum, db_type, file_path }` 与
`PreviewDbFinished { dbnum, pending_sessions, changed_elements }`；终态 `result` 就是现在这个
`Preview` 结构，一个字段都不用改。

## Considered Options

- **只画一个诚实的等待态**：转圈 + 本地已用时 + 「取消等待」。零后端改动，但十分钟里界面依旧
  说不出任何进展；「取消」也只是客户端放弃等待，服务端照跑不误。
- **保持同步响应，只在扫描期间往 WS 发事件**：省掉响应体改动，但一条挂十分钟的 HTTP 请求本身
  就脆，中间任何一跳超时就前功尽弃，而且预览与执行成了两套形态。
- **让后端给一个预计耗时**：与「每个数字都要指到契约里的字段」直接冲突。历史耗时和本次库大小
  之间没有稳定关系，算出来也是猜。

## Consequences

- 客户端 `Vm` 的 `Loading` 从「挂着一个 future」变成一个带 `task_id` 的运行实例，和 `Running`
  同构。两处可以共用同一套 WS 订阅与轮询兜底——断线仍要靠轮询收口，同 ADR-0005。
- 「取消预览」这才谈得上真取消：有 `task_id` 才有得停。但服务端目前没有 cancel 接口，所以这一版
  按钮只放弃客户端等待，**界面上必须说清楚，不能承诺服务端会停**。
- 逐库进度是拿两条新事件换来的，不是现成的。画板 S2-A 上那三行（已扫描 / 扫描中 / 不需扫描）
  和 `1 / 2 个 DESI 库` 这个分母，全部依赖上面两条事件；事件不落地，S2-A 就得退回只剩已用时的形态。
- 非 DESI 的库压根不进扫描循环，所以在 S2-A 上是「不需扫描」，**不占分母**——与 S2 / S2-B / S4
  上「排除」的说法保持一致，不要写成「已跳过」。
