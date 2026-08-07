# 项目接入点 · 施工顺序

- 对象：`D:\work\plant-code\old\plant-ui`
- 日期：2026-08-08
- 依据：`docs/adr/0018-switching-projects-restarts-the-client.md`（十一题定案）、
  `CONTEXT.md`（术语「项目接入点」）、`docs/plans/ui-rearchitecture.md`（S5 屏幕层，
  其「补画还是授权自拟」未决项已由 ADR-0018 结掉）
- 性质：设计已定案，本文件只排施工顺序与逐条验收口径。**代码未动。**

---

## 零、开工前必须先定的一条

ADR-0018 文末那条「待决」：`settings_store.rs`（未入库）把设置落在 **exe 旁**的
`config/settings.ron`，跟着 exe 走、不跟着 bundle 走。按 Q5 定的链，持久化的
「模型服务地址」会压过 bundle 自述——切到项目 B 之后界面仍连着项目 A 的 gen-model，
每个请求 422 `identity_mismatch`。

**T1 与 T7 在这一条定下来之前不开工。** T4 不受影响，且它正是验证这条冲突的工具。

---

## 一、先解的卡口：两套真的并排跑起来

Q2 那条「一台机器并排跑多套」是后面一切的地基。跑不起来两套，第三、四节的验收全是纸上谈兵。

| 事实 | 后果 |
|---|---|
| `Start-Plant.ps1` 写死 `--bind 127.0.0.1:8009` 与 `rocksdb:./data/surreal` | 第二套 surreal 要么端口撞车起不来，要么起来了却读同一个数据目录 |
| 同脚本写死健康检查 `8022` 与 `$env:PLANT_MODEL_API_URL = "http://127.0.0.1:8022"` | 第二套 gen-model 抢同一个端口 |
| `backend/DbOption.toml` 每套一份、钉着自己的项目 | 这条本来就对，不用改——但两套的 `mdb_name` / `project_name` / `surreal_ns` 必须与各自 bundle 的 `e3d.project.ron` 对上，否则 UI 一发请求就 422 `identity_mismatch` |

**T0 · `Start-Plant.ps1` 参数化**

- 加 `-SurrealPort` / `-ModelApiPort` / `-DataDir` 三个参数，缺省保持今天的 `8009` / `8022` /
  `./data/surreal`；健康检查与 `PLANT_MODEL_API_URL` 跟着参数走。
- **验收**：同机用两组端口各起一套，`curl http://127.0.0.1:<A>/api/v1/health` 与 `<B>`
  返回**不同的 project / namespace**；两个 surreal 进程的数据目录不同。

---

## 二、配置面

**T1 · `e3d.project.ron` 新增可选 `model_api_url`**

- `startup.rs` 的 `LegacyProjectConfig` 加 `#[serde(default)] model_api_url: Option<String>`，
  由 `legacy_runtime_config` 交出去。
- 优先级按 ADR-0018：**设置项 > 环境变量 > bundle > 出厂默认**。
- **验收**：① 不带该字段的老 ron 解析照旧，行为与今天一字不差；② 带该字段且未设
  `PLANT_MODEL_API_URL` 时，S6 设置窗打开显示的是 bundle 里那个地址；③ 两者都有时环境变量赢。
  单测放进 `startup.rs` 已有的 `mod tests`，与 `legacy_project_config_maps_to_the_current_runtime`
  同一批。

**T2 · `config/projects.ron` 与两层回落**

- 新增解析：一份 asset_root 路径表。相对路径**相对 `projects.ron` 自身所在目录**解，并把这句
  写进文件头注释——不写清楚，将来又是一轮「它到底扫了哪儿」。
- 两层回落（ADR-0018 Q5）：总表缺失 / 为空 / 条目全不可读 → 名单里至少有当前这一套
  （`resolve_asset_root` 解出来的那个）。
- **验收**：① 没有 `projects.ron` 时名单恰好 1 条，就是当前这套；② 有 `projects.ron` 但其中
  一条指向不存在的目录 → 那条**列出来并标不可用**，不是静默消失（这正是本次故障的教训，
  不许再有「扫了但不说」）；③ 每条都读得出项目名 / 项目号 / MDB / 库地址 / 模型服务地址。

**T3 · bundle 凭据覆盖 `config/credentials.toml`**（与 T1 平行）

- 优先级：`PLANT_DB_USER`/`PLANT_DB_PASSWORD` → bundle 的 `credentials.toml` →
  exe 旁 `DbOption.toml` → `root`/`root`。与 T1 同一条原则：人为覆盖高于 bundle 自述。
- 必须同时加 `.gitignore` 规则。
- **验收**：① 没有该文件时凭据解析与今天完全一致；② 有该文件时它压过 exe 旁的
  `DbOption.toml`、压不过环境变量；③ `git status` 里看不到它。

---

## 三、界面

**T4 · 状态栏数据库芯片展开当前接入点**（Q7；不依赖 T0–T3，可最先做）

- 展开内容：`ws://host:port`、namespace、database、MDB、用户名、模型服务地址、数据中心地址，
  以及**这份配置来自哪个文件的绝对路径**。口令不显示。
- 最后一项是重点：它就是为了让那条「`get_db_option()` 静默回落去读工作目录 `DbOption.toml`」
  再也藏不住。
- **验收**：① 故意把 `PLANT_ASSET_ROOT` 指到一个没有 `config/e3d.project.ron` 的目录，启动后
  芯片展开必须说出它落到了 `DbOption.toml`；② 展开的每一项与 `aios_core::get_db_option()`
  的实际值逐项相等——写断言测试，不靠眼睛。

**T5 · S5 升为屏幕层**（Q8）

- 进入条件：`PLANT_ASSET_ROOT` 未设 **且** 记忆文件为空；否则直入工作台。
- 全屏，按设计令牌自拟（ADR-0018 已正式授权）。每行摆完整接入点，**标出「当前」**——
  `project_picker.rs` 的 `default_index` 注释早就点过这个坑。
- 菜单入口保留；运行中叫出来时，同一份绘制内容装进弹窗容器。
- **验收**：① 无参启动停在 S5；② 带 `PLANT_ASSET_ROOT` 启动不经过 S5；③ 运行中从菜单叫出时，
  选中行就是正在用的那一套。

**T6 · 重启确认（分场景）**（Q10）

- 开机屏不拦；运行中从菜单切时拦一句，说清「会重启客户端 / 已加载的三维模型与视图状态会丢 /
  服务端任务不受影响」三件事。
- **验收**：两条路径各走一次，开机屏不出确认，菜单路径出确认且文案含那三句。

---

## 四、切换机制

**T7 · 重启自举 + 记忆文件**（Q4 / Q9）

- `Cmd::LoadProject(asset_root)` 真正落地：写记忆文件 → `std::process::Command` 带
  `PLANT_ASSET_ROOT` 重拉自己 → 本进程退出。
- 解析优先级：`PLANT_ASSET_ROOT` > 记忆文件 > 默认。
- 记忆文件写盘失败**必须出声**（日志 + 界面一行），不许静默降级回默认那套。
- **验收**：① 切到 B，新进程起来后状态栏芯片报的是 B 的接入点；② 关掉、手动双击 exe 仍进 B；
  ③ 删掉记忆文件后双击回默认那套；④ 把记忆文件设成只读，切换时界面报错而不是装作没事。

**T8 · wasm 端关口**（Q11）

- 菜单里的项目选择入口在 wasm 下隐藏，`Cmd::LoadProject` 在 wasm 分支不可达（或编译期切掉）。
- 状态栏芯片的只读展示两端一致。
- **验收**：wasm 构建里找不到切项目入口，但接入点信息看得见。

---

## 五、依赖顺序

```
T4                      ← 独立，建议最先做：最快回答「我连的是谁」
第零节待决 → T1 ┐
T0 ────────────┴→ T2 → T5 → T7 → T6 → T8
                 T3（与 T1 平行）
```

T0 不做，T5 与 T7 的验收无从谈起（只有一套可选，切不出去）。T4 不欠任何人，且它同时是
T7 的验收工具——切完之后靠它确认真的换了库。

---

## 六、明确不在本轮

- `get_uda_info()` 把 `included_projects` 里所有项目的 UDA 混成一张表、且读工作目录的
  `DbOption.toml`（ADR-0018 已记；重启方案下每进程重建一次，切项目不会串）。
- 主题 / 密度落盘。记忆文件开了口子，但本轮只存接入点。
- 探活：各 gen-model 的 `/health` 标「这套在不在跑」。
- `vendor/rs-core` 的三个 `OnceCell`。**本轮一行不碰**，这是选重启方案换来的。
