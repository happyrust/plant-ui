# AMS 手动提资联调验证

验证日期：2026-10-08。项目：AvevaMarineSample，项目号/namespace 1516，MDB /ALL。

## 环境与边界

- 当前 AMS E3D 3.1 项目文件由本机模型服务读取，模型服务使用独立内存存储，未打开原有 RocksDB 工作库。
- /api/v1/health 核心 ready，71 条库路由；/api/v1/tree/roots 返回 38 个根节点；桌面端状态栏和树与实际接口一致。
- 手动提资通过本地测试接收端验证；请求不会转发到正式数据中心，未创建正式提资任务。
- 前端使用本次移植后的 rs-plant release 构建；后端使用已有 aios-database release。后台测试进程在验收后停止。

## 实测结果

| 验证项 | 结果 |
| --- | --- |
| 创建 → 手动提资接口 | 通过，窗口居中，AMS 项目保持连接 |
| 空表 | 提交按钮禁用 |
| 中文标题、设备名称、房间号输入 | 通过 |
| 单行成功提交 | 请求路径 /send_room_code_to_data_center，JSON 与输入一致，记录成功日志 |
| HTTP 200 但 Success=false | 明确记录业务失败，恢复提交能力 |
| HTTP 503 | 明确记录 HTTP 错误，恢复提交能力 |
| HTTP 200 非 JSON | 明确记录响应契约错误，恢复提交能力 |
| 四行填满后扩行 | 出现第五条空行 |
| 多行成功提交 | 四条完整记录准确提交，空行排除 |
| 失败后重新提交 | 通过 |
| 桌面 release 构建 | 通过；既有 warnings 保留 |
| UI 库回归测试 | 176 项通过 |

AMS 读取样本：/1WCC-R220.002、/1WCC-R220.003；AMS 房间树样本：/1RX-RM01-R101。
测试表格另含 /AMS-TEST-003 与 /AMS-TEST-004 两条明确的测试记录。此次测试只检查手动输入与传输，不断言这些设备的真实空间归属。

## 本轮修复

1. 网页画布已经重命名为 rs-plant，但事件绑定仍查找 plant-ui，导致启动失败。已统一事件绑定名称，网页实际加载了 AMS 模型树。
2. gen-model 内嵌资源清单仍引用已删除的 plant_ui_app 文件，且新脚本被当作 application/octet-stream 返回。已将资源清单及对应测试同步为 rs-plant。

第二项后端源码修复尚未重新构建：Cargo 在扫描 python/.pytest_cache 时被现有目录 ACL 拒绝访问。网页运行验收通过本地静态资源入口完成，不能作为更新后后端二进制的验收结果。

## 证据与复现

- final-success.png：AMS 项目、手动提资表格和成功/失败日志。
- requests.jsonl：五次提交的路径、Content-Type、模式和完整 JSON。
- ams-roots.json / ams-room-samples.json：真实 AMS 只读查询结果。
- scripts/verify_manual_publish_ams.py：本地可重复的静态页面、AMS 只读转发与提资测试接收端；响应模式由输出目录 response.json 控制。

真实 rs-server/数据中心服务当前未运行，正式端的权限、任务创建、LoginUrl 跳转和最终入库尚未联调。当前手动接口请求仅包含 title 和 room_codes，没有项目身份字段；正式服务如何识别 AMS 需按实际部署契约核实。
