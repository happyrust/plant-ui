# 切项目 = 换一个项目接入点，靠重启客户端完成

> **状态：设计与文档已定案（2026-08-08 拷问，十一题全部人工答复，无超时自决）。**
> 画板 S5 未画——本 ADR 同时**正式授权按设计令牌自拟**，了结 `docs/plans/ui-rearchitecture.md`
> 那条「S5–S10 补画还是授权自拟」的未决项。实现未开工。

「打开选择项目，看到的是『0 个可用项目配置』」——而当时客户端明明连着 AvevaMarineSample，
模型树好好地摆在那里。查下来这不是数据问题：**这扇窗从来没被接上过**；而且界面上根本没有
任何一处说得出「我此刻连的是哪个库」。

## 查证到的事实（决定了形状的那些）

1. **picker 从来没有数据源。** `main.rs` 用 `ProjectPickerState::new(Vec::new(), false)` 建窗，
   此后无人往 `entries` 里填；`Cmd::LoadProject(_)` 丢掉 payload 只 `open = false`。真正的启动
   路径不经过它：`run_native()` 解 asset_root → 读 `config/e3d.project.ron` →
   `aios_core::set_db_option()`。磁盘上也只有一份配置，就算真去扫也只能扫出一条。
2. **连接信息全程不露面。** 标题栏与状态栏只有项目名 + 项目代号（后者其实是 `surreal_ns`）；
   S6 设置只有主题 / 密度 / 两个服务地址。地址、端口、用户、MDB 一个都没有。
3. **`get_db_option()` 会静默回落**去读工作目录的 `DbOption.toml`。两份配置眼下指向同一处所以
   看不出来，一旦 `e3d.project.ron` 缺失或 asset_root 解错，界面不会吭声。
4. **进程内热切要动三个 `OnceCell`**，且都在 vendored `rs-core` 里、与 gen-model 共用同一份：
   - `lib.rs` 的 `DB_OPTION`——`set_db_option` 第二次直接报「已初始化，不能重复覆盖」；
   - `inst.rs` 的 `FLAT_READ_POOL`——4 条只读连接**建池时就 pin 死 ns/db**，切完之后
     **静默返回上一个项目的数据**，不报错；
   - `get_uda_info()`——`OnceCell`，而且它读的是工作目录的 `DbOption.toml`，根本不是 `DB_OPTION`。
5. **gen-model 是单项目服务。** `ServiceIdentity` 的文档注释原文是 "The immutable project identity
   served by this process"，每个请求 `validate` 一次，对不上 422 `identity_mismatch`。
6. **壳侧的换库清理已经写好了。** `main.rs` 的 `fn reconnect()` 清模型作用域、整棵树、vm、
   队列面板身份，注释原话就是「换库了，旧库的房间总览不作数」。
7. **客户端没有未保存工作。** 唯一改库的 `EditAttr` 直接发命令写库、没有本地草稿；提资是一次性
   提交；任务队列住在服务端。重启只丢本地视图状态。
8. **设置已经落盘了**，就在 exe 旁的 `config/settings.ron`（`settings_store.rs`，截至
   2026-08-08 尚未入库）。它自己的文件头注释交代得很清楚：在它出现之前，ADR-0008 那条
   「设置项 > 环境变量 > 出厂默认」只兑现了一半——一行不落盘，所以每次启动实际总是环境
   变量赢；从它起才真按那条优先级走。**这一条与 Q5 定的优先级链正面冲突，见文末「待决」。**

## 十一题定案（2026-08-08）

| # | 题 | 定案 |
|---|---|---|
| Q1 | 这一轮的范围 | 做**真能切项目**，不止于把连接信息显示出来 |
| Q2 | 切的边界 | **一台机器并排跑多套**（每套 = surreal + gen-model + 独立端口），UI 切项目 = 切一组端点。gen-model 的单项目身份不动 |
| Q3 | 接入点清单 | **一份总表 `config/projects.ron`**。探活（各 gen-model 的 `/health` 报自己的 identity）可后加一层，只用来标「这套在不在跑」，不作唯一来源 |
| Q4 | UI 侧切换机制 | **重启 UI 进程**。热切在这里买不到暖状态——`reconnect()` 本来就要清树与模型——而代价里含一个静默错数据的全局 |
| Q5 | 与 `e3d.project.ron` 的关系 | **每套自描述**：`e3d.project.ron` 补上 `model_api_url`，`projects.ron` 只列 asset_root。**缺省两层回落**：总表缺失 → 至少列出当前这一套（0 条不再可能）；bundle 字段缺失 → 走现行优先级链 |
| Q6 | 凭据 | 默认沿用现状（`PLANT_DB_USER`/`PLANT_DB_PASSWORD` → exe 旁 `DbOption.toml` → `root`/`root`）；某套要用不同凭据时，在它的 bundle 里放一份**不进版本库**的 `config/credentials.toml`。口令不进 `e3d.project.ron` |
| Q7 | 连接信息主位 | **状态栏那个数据库芯片**，悬浮 / 点开展开完整接入点，含**这份配置来自哪个文件** |
| Q8 | picker 形态 | **补成屏幕层 S5**，正式授权按设计令牌自拟 |
| Q9 | 接入点交接 | 环境变量 `PLANT_ASSET_ROOT` 传参 + 一份「上次用这套」记忆文件兜底。优先级 env > 记忆文件 > 默认 |
| Q10 | 重启前确认 | **分场景**：开机屏不拦；运行中从菜单切时拦一下，说清会重启、本地视图状态会丢、服务端任务不受影响 |
| Q11 | wasm 端 | **不提供切项目**，入口隐藏，只保留「当前接入点」只读展示。浏览器里哪个项目由宿主页面的路由说了算 |

## 两处推翻

- **ADR-0008 的优先级链新增一级，排序不变。** 0008 定的是「设置项 > 环境变量 > 出厂默认」，
  理由是 8020 的回环侧被占、错连会拿回一个像模像样的 HTTP 错误——那些理由一条不废。变的是
  前提：**当年只有一套，模型服务地址当全局值是对的；并排多套之后它必须跟着项目走**。因此
  `e3d.project.ron` 头上那句「模型服务地址不在这份文件里」作废，新链为
  **设置项 > 环境变量 > bundle 的 `model_api_url` > 出厂默认**——bundle 只是给「这套项目自己
  声称的地址」腾了一级，人为覆盖仍然更高。
- **`ui-rearchitecture.md` 的「S5–S10 补画还是授权自拟」未决项就此结掉**（授权自拟）。顺带
  记一笔那份文档的担心：S5 属于屏幕层，看上去得先等 M4-1 把八个散落的 `States` 收敛成屏幕
  路由。**重启机制让这笔账不用还**——「此刻在哪一屏」变成 `run_native()` 里的一个启动分支，
  不是运行时状态。

## Considered Options

- **只做「看得见」**（把当前连接摆出来，不做切换）：今天就能落地、不碰任何全局。被否——真实
  诉求是切项目。连接信息因此成了本 ADR 的一部分，而不是全部。
- **整套一起换**（选完重跑 `Start-Plant.ps1` 那条编排）：`vendor/rs-core` 一行不改，失败形态
  一眼可见。被否——每次切都要重启 surreal 与 gen-model，重解析的代价太大；并排多套把贵的
  那一半留在了原地。
- **让 gen-model 支持多项目**（`ServiceIdentity` 改按请求路由）：最彻底。被否——它自己也卡在
  同一份 rs-core 的 `DB_OPTION` 上，成本远超本轮。
- **进程内热切**（改三个 `OnceCell`）：切换瞬时、窗口不闪。被否——`reconnect()` 本来就要清掉
  树与模型，热切保不住任何有用的暖状态，只买到「不闪一下」；而 `FLAT_READ_POOL` 的失败形态是
  **安安静静给出另一个项目的数据**，正撞在 CONTEXT.md「不说谎」那条外壳原则上。
- **schema 的索引层 / 取代 / 两可**三种形态：被否于「一个接入点的定义不该再跨两个文件」——
  配置散在四处正是这次故障的病根，拆成「总表拿一半、bundle 拿一半」等于把病又传一遍。

## Consequences

- **配置面**：新增 `config/projects.ron`（只列 asset_root）；`e3d.project.ron` 新增可选
  `model_api_url`；bundle 可选 `config/credentials.toml`（须进 `.gitignore`）。三者缺失都不是
  错误，各有回落——这是 Q5 那两层缺省的直接要求。
- **界面**：S5 升为屏幕层（开机既无 `PLANT_ASSET_ROOT`、记忆文件也为空时进入）；状态栏数据库
  芯片可展开；picker 必须标出「当前正在用的是哪一行」——`project_picker.rs` 的 `default_index`
  注释早就预见过「打开选择框看到的选中项不是正在用的项目」。
- **记忆文件**：`settings_store` 已经在 exe 旁开了 `config/settings.ron`，「上次用这套」应当
  并进去还是单起一份，随下面那条待决一并定。
- **`vendor/rs-core` 一行不改。** 三个 `OnceCell` 全部绕开，与上游 rs-core 的分叉不因本轮扩大。
- **仍然没解的**：`get_uda_info()` 把 `included_projects` 里所有项目的 UDA 混成一张表（先到
  先得），且读的是工作目录的 `DbOption.toml`。重启方案让它每个进程重建一次，切项目不会串——
  但同一进程内多项目 UDA 混表这个既存问题原样留着。
- **部署侧欠一件事**：并排多套要各自错开 surreal 端口、gen-model 端口与 rocksdb 数据目录。
  `Start-Plant.ps1` 现在写死 `127.0.0.1:8009`、`8022` 与 `rocksdb:./data/surreal`，多套版本
  得参数化。这一条不在本 ADR 范围，但不做它 Q2 就落不了地。

## 待决：持久化设置与接入点自述谁说了算

收尾时才看见 `settings_store.rs`（未入库）——它把设置真落到了 **exe 旁**的
`config/settings.ron`，**跟着 exe 走，不跟着 bundle 走**。一个 exe 切多套时它只有一份，
与 Q6 凭据撞的是同一个结构问题，但后果更硬：

1. 在项目 A 里改过一次「模型服务地址」（联调时很常见），`settings.ron` 存下 A 的地址；
2. 切到项目 B，B 的 bundle 声明自己的 `model_api_url`；
3. 按 Q5 那条链（设置项 > 环境变量 > bundle > 出厂默认）**设置项赢**——B 的界面连着 A 的
   gen-model，每个请求 422 `identity_mismatch`。

三条出路，未定：

- **bundle 压过持久化设置**——bundle 是项目自述，设置只管跨项目的偏好（主题、密度）；
- **`settings.ron` 按接入点分开存**——每套一份，与 Q6 凭据同构；
- **服务地址退出设置窗**——承认它属于接入点而不属于偏好，S6 只留主题 / 密度 / 网格目录。

**这一条不定，T1 与 T7 都不该开工。** T4（状态栏展开当前接入点）不受影响，反而是验证它
的工具：展开项里同时报出「配置来自哪个文件」与实际生效的模型服务地址，冲突一眼可见。
