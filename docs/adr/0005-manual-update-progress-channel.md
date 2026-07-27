# 手动增量更新的执行进度走 WebSocket，不靠轮询

gen-model 在执行期间按行发两阶段事件（`DataBatchStarted/Finished`、`ModelUnitStarted/Finished`），
但只发在 `/api/v1/ws` 的 tasks 主题上；`GET /api/v1/tasks/{id}` 只给得出 `state`、`events_seen`
计数和终态 `result`。设计稿 S4 画的是逐行推进，要让它成立，plant-ui 必须自己连一条 WS，
而不是继续每秒轮询——这是本仓库第一条长连接。

## Considered Options

- **只轮询**：S4 退成阶段级（两条阶段状态 + 计数），执行中看不出卡在哪个单元。零客户端改造。
- **改后端契约**：让 `GET /api/v1/tasks/{id}` 带上最近 N 条事件。前端简单，但要动 gen-model 的
  任务表与契约，跨仓库协调。
- **两种形态都画**：WS 逐行为主、轮询阶段级为降级。覆盖最全，画板与实现都翻倍。

## Consequences

- 轮询不能删。WS 断开期间事件不补发（`seq` 是连接内单调，不是可回放的游标），所以断线后仍要靠
  `GET /api/v1/tasks/{id}` 兜住终态，两条通道并存。
- 断线是一个必须画出来的界面状态，不是异常分支：见画板 S4-B。落后了多少条事件是能算出来的
  （服务端 `events_seen` 减去本端收到的条数），这个数要显示出来，别让人以为进度是全的。
- 单元行的「排队中」在断线后不成立——没有事件就不知道它是不是已经开始了，那些行改说「状态未知」。
