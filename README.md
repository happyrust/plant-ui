# plant-ui

布置平台的桌面外壳：egui 画界面、Bevy/wgpu 画三维。五个常驻视图（模型树、属性、命令行、三维视口、任务队列）加上模型增量更新、三维数据提资等任务窗，另有一棵按房间浏览模型的「房间树」。

数据来自两处：模型本体库（SurrealDB）与 gen-model 服务（增量更新、按需生成）。gen-model 在另一个仓（`D:\work\plant-code\old\gen-model`，包名 `aios-database`），本仓只当它的客户端。术语口径统一在 `CONTEXT.md`，实现决策在 `docs/adr/`，施工顺序在 `docs/plans/`。

## 仓库结构

| crate | 包名 | 是什么 |
|---|---|---|
| `crates/plant-ui` | `plant-ui` | UI 库：工作台外壳、各视图、视图模型、任务队列与更新向导的契约解码。纯 egui，不碰 IO |
| `crates/plant-ui-app` | `rs-plant` | 应用本体（二进制 `rs-plant`），数据线程、读面、设置落盘；另带验收探针 `inspect` |
| `crates/plant-ui-data` | `plant-ui-data` | 数据访问：SurrealDB 查询、模型实例、名称索引、房间查询 |
| `crates/plant-ui-view3d` | `plant-ui-view3d` | 三维：Bevy 场景、E3D 外观、可见性回执、虚拟孔洞预览 |
| `crates/plant-room` | `plant-room` | 房间体判定的纯库，与 gen-model 共用一份口径（ADR-0029） |
| `crates/plant-csg` | `plant-csg` | 布尔运算的薄封装，原生端接 manifold 内核（ADR-0031） |

`vendor/` 下是可编辑的依赖源码树（`rs-core`、`bevy_egui35`、若干 fork），`vendor/registry` 是 `cargo vendor` 产物，`.cargo/config.toml` 把全部 crates.io 与 git 源替换到这里。

## 前置

- Rust 工具链按 `rust-toolchain.toml`（`nightly-2026-07-21`，带 `wasm32-unknown-unknown`）。
- Windows 上要能编 C 依赖（`ring` 等），构建 wasm 还需要带 wasm32 target 的 LLVM clang 或 Zig，`scripts/build-wasm.ps1` 会自己找。
- 编译并发在 `.cargo/config.toml` 里限到 `jobs = 8`：bevy / parry / wgpu 这一档单个 rustc 峰值能吃到数 GB，跑满会以 `STATUS_STACK_BUFFER_OVERRUN` 退出并在仓库根吐 `rustc-ice-*.txt`，看着像代码问题。
- 本机把 `CARGO_TARGET_DIR` 指到仓库外的 `D:\Rust\target`，与其他仓共用一份 target。要独占就自己设一个别的目录。

## 构建与运行

```powershell
cargo build -p rs-plant
cargo run -p rs-plant
```

直接跑 exe（不经 `cargo run`）时必须给 `BEVY_ASSET_ROOT` 指向仓库根，否则 Bevy 会去 exe 旁边找 `assets/`，找不到就停在资源未加载状态，界面中文全成方块：

```powershell
$env:BEVY_ASSET_ROOT = "D:\work\plant-code\old\plant-ui"
& "D:\Rust\target\debug\rs-plant.exe"
```

接真后端：先把 gen-model 起起来，再让本仓指过去。`PLANT_MODEL_API_URL` 只在这一进程里生效，界面里存过的地址会顶掉它（见 `crates/plant-ui-app/src/settings_store.rs`）。

```powershell
$env:PLANT_MODEL_API_URL = "http://127.0.0.1:8022"
cargo run -p rs-plant
```

出厂默认地址是 `http://127.0.0.1:8021`（`plant_ui::settings::DEFAULT_MODEL_API_URL`），与上面这份 gen-model 的 8022 不是同一个端口，别把「连不上」当成服务没起。

不接后端也能看界面：`PLANT_UI_SIM=1` 让整个数据线程改由进程内剧本供数（`crates/plant-ui-app/src/sim.rs`），界面顶上会标一句「模拟模式」。

```powershell
$env:PLANT_UI_SIM = "1"
cargo run -p rs-plant
```

## 环境变量

| 变量 | 作用 |
|---|---|
| `PLANT_MODEL_API_URL` | 模型服务地址（gen-model）。优先级：设置项 > 本变量 > 出厂默认 `http://127.0.0.1:8021` |
| `PLANT_READ_FACE` | 供数模式：`service`（默认，走模型服务）或 `store`（直连模型本体库）。在场时设置窗那一格灰掉并标注被环境变量压过（ADR-0026） |
| `PLANT_UI_SIM` | `1` = 用内置剧本供数，不需要任何后端 |
| `PLANT_UI_SIM_SERVICE` | 剧本演哪种服务形态：`direct-no-worker` / `read-through` |
| `EGUI_INSPECTION` | 挂上验收探针的服务端；值可以是 `1`（默认 `127.0.0.1:5719`）或显式的 `host:port` |
| `PLANT_UI_INDIRECT_DRAW` | `0` = 关掉三维的 GPU 间接绘制，缺省开 |
| `BEVY_ASSET_ROOT` | 直接跑 exe 时的资源根，指到仓库根 |
| `CARGO_TARGET_DIR` | 产物目录，本机指 `D:\Rust\target` |

## 检查与测试

```powershell
cargo check -p plant-ui -p rs-plant --all-targets
cargo test  -p plant-ui -p rs-plant
cargo clippy -p plant-ui -p rs-plant --all-targets
```

clippy 里有 deny 级 lint，报了就得改，不能压。

跑之前先关掉正在运行的 `rs-plant`：Windows 会锁住 exe，构建报 os error 32；同时跑还抢内存。

不要用 `cargo test --workspace`，也不要单跑 `-p plant-ui-data`：那一组集成测试硬要求 `PLANT_TEST_WORKSPACE` 指向带 `DbOption.toml` 的运行目录，缺了就 panic。它们多数还标了 `#[ignore]`，要跑得显式加 `--ignored` 并备好本地 SurrealDB。

## 分组规则成员（SCOSEL）

GPSET 规则组的成员不是 GPITEM 槽，而是一段 `SCOSEL` 规则；后端求值后按解析视图回成员行。本仓把这些行与 GPITEM 显式收录的成员行区分开：

- `TreeGroupLink` 新增 `pub rule: bool`：`true` = 由分组规则（`SCOSEL`）命中，`false` = GPITEM 显式收录（或主链子节点）。两处构造点同步填。
- 树行标签由 `crates/plant-ui/src/workbench/tree.rs` 的 `row_tags(row, unresolved)` 统一产出：规则成员挂「规则」（`ph::FUNNEL` 图标，`Status::Info`），GPITEM 成员仍挂「引用」（`ph::LINK_SIMPLE`）。`unresolved` 的文本拥有权留在调用处，`row_tags` 只借 `&str`。
- 悬停文案区分两者：规则成员是「分组规则命中：主位置在别处（右键「定位到原位置」）」；GPITEM 成员是「分组引用：经 GPITEM `<refno>` 收录，主位置在别处（右键「定位到原位置」）」。
- 右键「定位到原位置」对规则成员同样可用：它照旧走主链选中、展开祖先、滚进视野。
- `row_id` 未改动，同一个元素在主位置与分组下仍是同一条身份。

后端契约（`GET /api/v1/tree/children?refno=<GPSET>&resolved=1`）：规则成员是 `via: null` + `secondary: true` + `rule: true`；GPITEM 成员是 `via: <GPITEM refno>` + `secondary: true` + `rule: false`；主链子节点三个键都不出现。`ResolvedTreeRow` 的 `rule` 带 `#[serde(default)]`，老服务端不带这一格就是 `false`，行为退回改动前。后端开关 `AIOS_GROUP_SCOSEL` 默认关，关着时回执逐字不变。

测试：

```powershell
cargo test -p rs-plant
cargo test -p plant-ui
```

包名是 `rs-plant`（不是目录名 `plant-ui-app`）。

## 验收探针

`inspect` 走 `egui_inspection` 的线协议直连运行中的应用，读控件树、注入输入、截图。应用那一侧要用同一个 `EGUI_INSPECTION` 启动。

```powershell
$env:EGUI_INSPECTION = "127.0.0.1:5721"
cargo build -p rs-plant --bin inspect
& "D:\Rust\target\debug\inspect.exe" tree        # 列控件：角色 / 文本 / 逻辑坐标
& "D:\Rust\target\debug\inspect.exe" click 900 400
& "D:\Rust\target\debug\inspect.exe" shot out.png
```

坐标是 egui 逻辑点，不必按系统 DPI 换算。`shot` 回来是 1×1 的图时，设 `PLANT_UI_INDIRECT_DRAW=0` 再截。完整命令见 `crates/plant-ui-app/src/bin/inspect.rs` 顶部注释。

## 文档在哪

| 想知道 | 去哪 |
|---|---|
| 一个词到底指什么 | `CONTEXT.md` |
| 为什么这么定 | `docs/adr/`（0005 起，共 29 份） |
| 某项功能做什么、不做什么 | `docs/features/` |
| 施工顺序与验收口径 | `docs/plans/` |
| 界面字段逐格出处 | `design/QUEUE-FIELD-MAP.md`（任务队列）、`design/MODEL-UPDATE-FIELD-MAP.md`（增量更新） |
| 设计令牌与画板 | `design/DESIGN-SYSTEM.md`、`design/*.pen` |
| 实机证据 | `docs/evidence/` |
| 界面上会不一样的地方 | `CHANGELOG.md` |

`docs/plans/manual-update-and-get-work-on-direct-mode.md` 是手动增量更新与「取回工作」对接 gen-model direct 形态的那份计划；真机验收记录在 `docs/evidence/2026-11-02-direct-mode-live-acceptance.md`。队列终态行明细里「凭证前移 N/M 根」与判决明细的「已追平 / 尚未生成」两行，字段出处见 `design/QUEUE-FIELD-MAP.md` §1.5。

## 已知坑

- 构建 / 测试前先停 `rs-plant`，否则文件锁与内存双重失败。
- `cargo test --workspace` 与 `-p plant-ui-data` 会因为 `PLANT_TEST_WORKSPACE` 缺席而挂，不是代码坏了。
- 直接跑 exe 忘了 `BEVY_ASSET_ROOT` 会得到「中文全是方块」，不是字体或主题的 bug。
- 编译并发别放开：`jobs = 8` 是防 rustc 被 OOM 打死，不是保守。
- `CARGO_TARGET_DIR` 指到仓库外时，exe 与 `assets/` 不再相邻，凡是要直接跑 exe 的场合都得给 `BEVY_ASSET_ROOT`。
