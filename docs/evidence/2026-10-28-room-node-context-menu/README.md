# 模型树房间节点右键：查看 / 隐藏房间内模型

2026-10-28，`plant-ui` `master`。现场：`aios-database.exe` 8022（`mirror=ready`、`room=available`）、
SurrealDB 8009（`1516/AvevaMarineSample`，661,966 条 `pe`、230 条 `room_panel_relate`）。

## 判据

房间节点要同时满足两条：

1. `noun == "FRMW"`（大小写不敏感）。
2. 这个 refno 在 `room_panel_relate` 的 `in` 侧出现过。

只看类型名不行：全库 `noun = "FRMW"` 有 **7308 行**，里头绝大多数是结构框架
（`/MDS/FT03H/3-FRMW1` 这类）。登记表才是权威判据——`room_panel_relate` 实测 230 行、
230 个不同面板、205 个不同房间，整表查询 163 ms，宿主连上取一次就够。

**方向**：`in` = 房间（`FRMW`）、`out` = 墙面板（`PANE`）。

```sql
SELECT out, in FROM room_panel_relate LIMIT 3;
-- {in: pe:24381_1008, out: pe:24381_1010}   room=FRMW /1RX-RM02-R232, panel=PANE
-- {in: pe:24381_1038, out: pe:24381_1040}
-- {in: pe:24381_1068, out: pe:24381_1070}
```

## 截图

| 文件 | 内容 |
| --- | --- |
| `01-room-node-selected.png` | `=24381_1008` 定位后，模型树里 `/1RX-RM02-R232` 选中，右侧属性面板显示 `FRMW`、房号 `L505` |
| `02-context-menu-hide-state.png` | 右键该房间节点，菜单含「隐藏房间内模型」（此时房间几何显示着） |
| `03-after-hide-room-model.png` | 点「隐藏房间内模型」后，房间墙面板从视口消失 |
| `04-context-menu-show-state.png` | 再右键，同一项翻成「查看房间内模型」 |

## 一项两态

判据是「下一次点眼睛会让它出来吗」，不是当前四态。含在途方向——点了「隐藏」到回执
落地前，菜单得跟着翻，否则再右键还是「隐藏」，与已经翻面的图标打架。

| 当前态 | 菜单项 |
| --- | --- |
| `Shown` | 隐藏房间内模型 |
| `Unloaded` / `Hidden` / `Partial` | 查看房间内模型 |

## 现场踩到的坑

`room_panel_relate` 只有复合索引 `(in, out)`。`WHERE out IN [...]` 会**静默返回 0 行**，
所以 SQL 不能加 `WHERE`，必须整表 `SELECT out, in`。

也不能写成 `SELECT VALUE [out, in]`：那样返回的是一条扁平数组
（`["pe:…","pe:…"]`），230 行只剩两个元素，反序列化成元组直接失败。

## 涉及提交

- `fd3342ccd` feat(tree): 模型树房间节点右键「查看房间内模型」
- `698f44e26` feat(tree): 房间节点右键补「隐藏房间内模型」，与查看成对
