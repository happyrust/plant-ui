# 房间详情成员查询恒 0 行的修复 · 验证记录（plant-ui #4）

缺陷：[plant-ui #4](https://github.com/happyrust/plant-ui/issues/4)。修复提交 `43c6dd2ab`（`crates/plant-ui-data/src/room.rs` + `tests/room_probe.rs`）。
现场：AMS 真库（dbnum 7997，SurrealDB 1516 的 rocksdb）+ gen-model 8023 + release `rs-plant.exe`（2026-10-25 09:36 构建）。
实测机 150% 缩放（`ppp = 1.5`），截图 1600×1000 逻辑点。

## 0. 缺陷一句话

`room_detail_sql` 的成员边写的是 `WHERE in IN $panels`。SurrealDB 2.1 在**只有复合索引 `(in, out)`** 的边表上执行 `in IN [数组]` 静默返回 0 行，
于是「成员数」整间变 0、「截断候选」跟着空。房间视图 HUD 那格「成员 0」是假的，R640 实际 225 名成员。

## 1. 单元测试

- `cargo test -j 2 -p plant-ui-data --lib`：**22 passed / 0 failed**。
- `sql_renderers_pin_tables_and_predicates` 钉住新形状（`array::flatten(SELECT VALUE ->room_relate FROM $panels)`），
  并加 `assert!(!detail.contains("WHERE in IN"))`——旧写法回不去。

必须带 `-j 2`：默认 `jobs = 8` 在这台机器上会把 rustc 打成 `STATUS_STACK_BUFFER_OVERRUN`（与本次改动无关，环境既有）。

## 2. 真库探针（`PLANT_TEST_DB_PORT=1516`）

`cargo test -j 2 -p plant-ui-data --test room_probe -- --ignored --nocapture`，`PLANT_ROOM_PROBE_ROOMS=R640`：

```
room_detail L505: 面板 1 / 成员 56 / 预览 5 / 待重算 false
room_detail R640: 面板 1 / 成员 225 / 截断候选 5 / 47 ms
element_rooms 24381/13699: 1 条归属，最强 Some(("L505", 8))
test result: ok. 1 passed; 0 failed
```

负向对照：把 `room.rs` 的成员边换回 `WHERE in IN $panels`，同一趟探针变红——`left: 0 / right: 56`（L505 的 56 名成员整批丢）。

成员边三条写法的实测对照（同一批 `room_relate` 边，`pe:24381_36219` 面板）：

| 写法 | 结果 |
|---|---|
| `WHERE in IN $panels` | **0 行** ✗ |
| `WHERE in IN [pe:24381_36219]` | **0 行** ✗ |
| `WHERE in = pe:24381_36219` | 225 行 ✓ |
| `array::flatten(SELECT VALUE ->room_relate FROM $panels)` | 225 行 ✓（2–3 ms） |
| `WHERE in IN (SELECT …)` | 225 行 ✓ 但 225 行实测 23 s |

只有复合索引的边表全中招（`room_relate` / `room_panel_relate` / `pe_owner`），有单独 `in` 索引的不受影响（`inst_relate` / `tubi_relate`）。

## 3. 实机（房间视图 R640）

驱动：`rs-plant.exe`（release）+ `EGUI_INSPECTION=127.0.0.1:5721`，同仓 `inspect` 探针（tree / click / type / key / shot）。
环境：`PLANT_ASSET_ROOT` = 临时资产根（`e3d.project.ron` 指向 `ws://127.0.0.1:1516` + `http://127.0.0.1:8023/`）、
`PLANT_UI_SETTINGS_FILE` = 同目录 `settings.ron`（`read_face: service`、`room_view_truncate_tubes: true`、`look: e3d`）、
`PLANT_MESH_DIR` = `D:\work\plant-code\old\test-issue-regen-model\meshes`（55912 个 `.mesh`）。

| 画面 | 图 | 看到的 | 判 |
|---|---|---|---|
| 修复前 | `00-before-member-0-truncate-0.png` | 用户 2026-10-24 23:38 的现场截图：HUD「面板 1 · 成员 0」 | ✗ 缺陷 |
| 修复后 | `01-after-member-225-truncate-4.png` | HUD 徽章「房间视图 · R640 · 成员 225」；「房间」页签「面板 1 · 成员 225 · 强度 8/8」「房号 R640」「房间码 1RX-R640」「面板数 1」「成员数 225」；日志「直段截断：R640 · 截断 4 段 · 房间外 14 段不画 · 整段在内 5 段」；视口里房间体半透明，穿出房间的直段留下一截 | ✓ |

进房间视图的路径与现场一致：房间树搜索 `R640` → 右键 → 「聚焦房间（房间视图）」。

**「截断 0 段」本身不是缺陷签名。** 首次进房间视图时日志同样是「截断 0 段」——那时场景里还没有这间房的直段：截断只动候选分支名下的直段实体，
`cut` / `hidden` / `whole` / `skipped` 四格全 0 就说明一条候选直段都不在场景里（在就必然落进其中一格，日志那句也没有「未能截断 N 段」的尾巴）。
给 R640 的 5 条截断候选 BRAN（`24381_145018` / `146262` / `147212` / `147412`、`24383_74349`）各点一次「显示模型」（每条 12 个元素、12–22 个网格实例）之后，
状态栏从 72 个元素涨到 448，再进房间视图才跑出「截断 4 段 · 房间外 14 段不画 · 整段在内 5 段」（4+14+5 = 23 段直段）。
修复前这条链路根本走不到：候选集恒空。

## 4. 顺带看过的两处（都不在本次范围，未动）

- **房间树在 R640 上是空的——这是已记录的库状态，不是缺陷。** 实机里 R640 行点「加载模型」报「房间树：R640 · /1RX-RM06-R640 下没有构件」，
  直查服务端树路由也是 `total_count: 0` / `room.count: 0`（`candidate_count: 261`）。这与 `docs/evidence/2026-10-25-room-tree-port.md` §3 记的是同一件事：
  这份 AMS 库的房间归属边还没算到候选上（`room_status.source = durable`、`matched = 0`），215 间房里 205 间树为空、10 间空房壳 404，
  plant3d-web 对同一台服务看到的也是空树。本次没有新结论，只是复现了那条记录。
- **房间体的「空」只有一句话。** `main.rs:6125` 把空记录一律说成「在册面板都还没有模型」；这间房一块面板都没在册时也是这句，
  读起来像「面板在册而模型缺席」。面板清单就在 `ctx.detail.panels` 手上，两种空可以分开说。这次 23:36 那次告警的措辞是准的（模型当时确实没生成完），
  所以只是文案可改进，不是缺陷。
