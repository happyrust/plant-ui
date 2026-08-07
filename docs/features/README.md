# 功能点索引

一个功能点一份短文档，每份固定四段：**它是什么**（指向 `CONTEXT.md` 的词汇）、**行为边界**（做什么、明确不做什么）、**入口与状态**、**出处**（ADR / 字段表 / 画板）。

这里只做索引与边界，不装实现细节：字段逐格出处归两份 FIELD-MAP，设计令牌与画板归 `design/DESIGN-SYSTEM.md`，词汇归根目录 `CONTEXT.md`，决策归 `docs/adr/`。

状态口径：**已实现**（新壳 `crates/plant-ui` 有代码）/ **已画未实现**（画板在 `design/plant-ui.pen`，代码未跟上）/ **规划**（画板与代码都还没有）。

## 主工作台

| 功能点 | 文档 | 状态 | 代码落点 |
|---|---|---|---|
| 主工作台外壳（窗格布局、页签、状态栏） | [workbench-shell.md](./workbench-shell.md) | 已实现 | `workbench/chrome.rs`、`panes.rs` |
| 模型树（树定位、右键菜单） | [model-tree.md](./model-tree.md) | 已实现 | `workbench/tree.rs` |
| 行内可见性眼睛 | [visibility-eye.md](./visibility-eye.md) | 已实现 | `workbench/tree.rs` + `vm.rs`（RowVisibility） |
| 属性面板 | [props-panel.md](./props-panel.md) | 已实现 | `workbench/props.rs` |
| 命令行 | [command-line.md](./command-line.md) | 已实现 | `workbench/command_line.rs` |
| 日志（分级） | [logs.md](./logs.md) | 已实现 | `workbench/logs.rs` |
| 三维视口（工具栏、ViewCube、显示优化） | [viewport.md](./viewport.md) | 已实现 | `workbench/view3d.rs`、`viewcube.rs` |

## 任务窗

| 功能点 | 文档 | 状态 | 代码落点 |
|---|---|---|---|
| 模型增量更新（两步向导） | [model-update.md](./model-update.md) | 已实现（向导两步化、死信拆分等 2026-08-05 画板先行） | `model_update.rs` |
| 任务队列视图 | [task-queue.md](./task-queue.md) | 已实现 | `task_queue.rs` |
| 三维数据提资 | [data-publish.md](./data-publish.md) | 已实现 | `data_publish.rs` + `plant-ui-app/data_publish_api.rs` |
| 项目选择 | [project-picker.md](./project-picker.md) | 已实现（画板 S5 未画，代码先行） | `project_picker.rs` |
| 设置 | [settings.md](./settings.md) | 已实现（画板 S6 未画，代码先行） | `settings.rs` |

## 规划中（画板或代码都还没有）

| 功能点 | 编号 | 状态 |
|---|---|---|
| 模糊查询 | S13（原 S11） | 规划 |
| 保存 / 搜索记录 | S14（原 S12） | 规划 |
| 防火封堵材料统计 | S7 | 规划 |
| 状态词汇 | S8 | 规划（设计令牌已先收敛，见 DESIGN-SYSTEM） |
| 创建元件 | S9 | 规划 |
| 元件库 | S10 | 规划 |
