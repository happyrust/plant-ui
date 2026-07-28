# 先补索引，按 dbnum 的内存缓存推后

提案是：给每个 dbnum 在 SurrealDB 的 `mem` 引擎里做一份缓存加速读，用 LRU 管缓存大小、
设一个内存占用上限维持稳定驻留。**结论是这个方案该做但不是现在**——它对已经量到的两个
瓶颈之一完全无效，而对另一个，两行 `DEFINE INDEX` 已经拿到了比它更多的收益。

先说它成立的那部分，因为这两块基石确实结实，不是客套：**refno 前缀与 dbnum 基本是双射**
（`define_dbnum_event` 用 `array::at($id, 0)` 作键维护 `dbnum_info_table`；7997→24381、
8000→24384），从一个 record id 就能白嫖地知道分片归属，不用查库；**水位是现成的单调递增
每库版本号**（`dbnum_watermark.applied_sesno`，`CONTEXT.md` 明写「只前进不后退」），缓存
一致性最难的那半已经有了。很多系统想做分片缓存就是卡在这两点上。

站不住的是收益侧。2026-07-27 在 8009 实例（`pe` 表 200,873 行）上实测，扣掉 ~320ms 的
CLI 连接开销：

| plant-ui 交互路径 | 净耗时 |
|---|---|
| 模型树**根层**（启动后第一件事） | **2,155 ms** |
| 展开一个 50 子节点的 SITE | ~0 ms |
| 属性面板 `cat_attmap`（TEE，4 跳引用链） | ~56 ms |

卡的只有根层那一下，而它的元凶跟内存毫无关系。把那条查询拆开：`MDB`/`CURD` 取 dbnos
20ms、`children_count` 子查询 ~0ms，2,165ms 全在 `pe where noun='SITE' and dbnum in $dbnos`
这一跳。再拆：

- `where dbnum in [7997,7999,8000]`（**有** `pe_dbnum_index`）—— 2,779 ms
- `where noun='SITE'`（noun **无**索引，纯全表扫）—— 955 ms

**既有的 `pe_dbnum_index` 是负优化。** dbnum 只有 3 个值、覆盖全表 20 万行，选择性差到走
索引比顺序扫还慢三倍，而规划器偏挑它。补一条 `(noun, dbnum)` 复合索引后那一跳
2,165ms → 22ms，根层端到端 2,552ms → 421ms。

同一轮里还坐实了 ams7997 报告 P2 那条：`pe.owner` 一直没索引，`count` 一个 50 行的结果要
1.25s。旁边的 `define_owner_index` 名字很像，但它建的是 `pe_owner` **边表**的 `(in, out)`，
不顶用——这是这个洞一直没被发现的原因。补 `pe_owner_index` 后量不出来。

内存缓存对这类问题的上限是「把全表扫搬进内存」，也就是快几倍；把它变成索引查找是快两个
数量级。而报告 G2 那条「生成读阶段是小查询循环、被往返延迟兜底」，**缓存一点忙都帮不上**
——只要缓存还在 WebSocket 那头，往返次数一次不减，只能靠批量化。

## Considered Options

- **进程内嵌 `mem://`**：能真正省掉 WS 往返，是唯一能碰 G2 的形态。但 plant-ui 现在
  `default-features = false, features = ["protocol-ws"]`，进程里根本没有存储引擎；打开
  `kv-mem` 等于把整个 surrealdb-core 编进桌面壳。且它服务不了 gen-model，两个进程共享不了
  嵌入式引擎。
- **独立的 `surreal start memory` 缓存进程 + 客户端双连接路由**：能同时服务两个进程，缓存
  可降级（挂了系统还能跑，只是慢）。代价是 rs-core 里 ~250 处写死的全局 `SUL_DB` 要逐个手工
  判路由，而路由规则只能是「这条查询触及的**每一个** dbnum 都驻留且新鲜」——SQL 全是
  `format!()` 拼的字符串，静态推不出来。判错的表现不是报错而是**空结果**，正是
  `ui-rearchitecture.md` 记过的那类事故（少一个 `WORL` 节点，整棵树是空的）。
- **read-through 代理**：SurrealDB 没有分层存储/回源能力，这等于自造一个懂 SurrealQL 的
  数据库中间件。
- **给现有 `#[cached]` memoize 加容量上限变 LRU**：改动最小、miss 是毫秒级、不需要第二个
  进程。但只能加速「同一个元素反复查」，救不了首次的全表扫。
- **把 8009 改回 memory**：它历史上就是这么起的，07-27 为了「进程一死库就没了」才换成
  rocksdb 落盘。走过的路。

## Consequences

- **`define_pe_index` 补两条索引**，已落 `rs-core-pin`（gen-model 经 `[patch]` 实际编译的
  那棵树）与 plant-ui 的钉版拷贝，逐条理由与实测数字进了 `docs/plans/ui-rearchitecture.md`
  的「换钉版要重放」表：

  ```sql
  DEFINE INDEX pe_owner_index      ON TABLE pe COLUMNS owner;
  DEFINE INDEX pe_noun_dbnum_index ON TABLE pe COLUMNS noun, dbnum;
  ```

  试过的单列 `pe_noun_index` 被复合索引的前导列完全覆盖（129ms vs 134ms），已撤，不要
  重复加。
- **写入代价还没量。** `pe` 每多一条索引就多一份写入维护，而写吞吐本来就是 P1（157k 行时
  掉到 650 行/秒）。下次基线化大库前顺手对一下；真变慢就改成「解析前 `REMOVE INDEX`、
  解析完 `DEFINE INDEX`」。
- **`pe_dbnum_index` 是待观察的删除候选。** 根层那一跳已由复合索引接管，而它在低选择性
  场景下会把规划器带沟里。别急着删，先确认没有别的查询靠它。
- **G2 的小查询循环仍然开着**，那是下一个该动的地方，且只能靠批量化，不能靠缓存。已经定位到
  一处：`query.rs` `query_multi_children_refnos` 是一个 `for &refno in refnos` 里逐个
  `get_children_refnos(refno).await` 的循环，**批量版本就注释在它正上方**。plant-ui 的视口
  显示走 `query_deep_visible_inst_refnos` → 这里。拿 100 个 BRAN 实测：批量那条
  ~0ms（与基线无法区分），而 100 条单独语句**即便打包进同一次往返**也要 1,578ms，也就是
  单是服务端每条约 16ms 的固定开销；真实客户端里这 100 条还各自是一次 WebSocket 往返。
  没有直接改，因为有个真实取舍：循环版每条都吃 `#[cached]` 的逐元素缓存，重复调用是免费的，
  批量版没有。想两头都要就得先查缓存、只把未命中的那批打包，那是一次有设计内容的改动，
  不该顺手做。
- **`kv-mem` 帮不上「设定一个内存占用大小」这件事。** 本仓 fork（2.1.4，rev `45013fc`）的
  `kv-mem` 是 surrealkv 0.8.1，`disk_persistence=false` / `enable_versions=false`。内存模式下
  删键确实会从 VART 索引里拔掉、值内联在索引项里（`store.rs:663`），所以**淘汰能真的还
  内存**；但 `Options` 里没有任何约束内存模式占用的旋钮，也没有内存用量 API 暴露给客户端，
  `compact()` 在内存模式下直接 `Err(InvalidOperation)`（`compaction.rs:35`）。上限只能在外面
  按「灌了多少行 × 实测标定的系数」估，必须接受它是估算值而非硬约束。
- **一次 miss 是几十秒，不是几毫秒。** 按 8000 实测的 3,700 行/秒算，7997 的 157k 行要
  ~42 秒，而那个吞吐里含 `pe_chunk=300` 的同步分块往返，换内存后端也不会消失。dbnum 这个
  粒度上 LRU 一旦护不住工作集而抖动，用户看到的不是「慢一点」而是「卡几十秒」。
- **淘汰单位不能是裸 dbnum。** `get_cat_attmap` 的 `CATR_QUERY_STR` 是一条 4 跳引用链，从
  DESI 元素一路跑进 CATA 库；`MDB_DESI_DBNOS` 读 `MDB`/`CURD`；`get_uda_*` 读 DICT。真做的话
  SYS/DICT/MDB/CATA 必须常驻钉住，只有 DESI 库能参与 LRU。
- **今天的工作集是 3 个 DESI 库（~190k 行）**，`mdb_name = "ALL"` 声明 29 个、
  `manual_db_nums` 实际解析 3 个。这个量全装进内存毫无压力，LRU 与内存上限几乎永远不会触发。
  这套设计是为工作集真涨到 250+ 个库的那天准备的。
- **重新拾起时，这几个岔路已经定过了**，不必从头吵：一套缓存同时服务 plant-ui 与 gen-model
  （因此必须是独立进程）；客户端双连接自己路由（因此缓存可降级，但要付 250 处手工判断）；
  写者负责失效，gen-model 每推进一次水位就把该 dbnum 从缓存 drop；分层钉住，只有 DESI 库
  参与淘汰。
