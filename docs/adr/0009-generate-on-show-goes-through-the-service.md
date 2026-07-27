# 显示时补齐模型走服务，不在渲染进程里现生成

「显示一个还没生成模型的元素」今天的行为是**安静地什么都不显示**。显示链路
（`rs-plant3-d` 的 `model_system.rs`）靠 `query_deep_visible_inst_refnos` 去查「有几何体的
refno」，模型没生成就查不到，查不到就没东西可画——既不报错也不提示。

那里其实已经有个雏形，而且注释自己就承认了不对：

```rust
//todo 这里仅限于调试，在这里生成模型
#[cfg(feature = "auto_gen")]
if auto_gen_mesh && !hide {
    // 每次显示都从磁盘重读一遍 DbOption，三个 unwrap
    gen_all_geos_data(target_refnos, &db_option, None).await.unwrap();
}
```

它是调试 feature；它在渲染进程里直接调生成代码而不走 gen-model 服务；它对所有 refno
**无条件重生成**，压根没有「判断」；而且它 `.unwrap()`，生成失败就是敲掉整个进程。

`POST /api/v1/model/ensure` 天生就是干这个的：`ensure_model_generated` 先数
`renderable_instance_count`，有就直接返 `AlreadyAvailable`；没有才拿 per-生成根锁、
二次检查、再生成。判断本来就在服务端做好了。

## Considered Options

- **客户端先查再决定发不发**：显示路径反正要跑 `query_deep_visible_inst_refnos`，查不到实例
  就是缺模型。省掉大量空请求，代价是客户端多养一套「有没有模型」的判据，要一直跟服务端对齐。
- **保留进程内 `gen_all_geos_data`，只把它从调试 feature 里放出来**：少一跳。但 `rs-plant3-d`
  得继续链接整套生成代码，而且它与 gen-model 服务的 `GENERATION_LOCKS` 互不相识——两边同时
  生成同一个根时没人拦。
- **让 gen-model 加一个批量 ensure 接口**：一次交一批 refno，服务端归并成根、进任务体系，
  进度跟手动更新同构。最干净，但后端又多一项新接口。

## Consequences

- **挂在 `ShowModelEvent` 展开之前那一层。** `generation_root` 是 ensure 的**返回值**，
  发之前无从得知，所以客户端没法按根归并。但 `event.refnos`（`deep` 展开成 `final_refnos`
  之前）天然就是归并点：人在树上点一个 BRAN 说「显示」，集合里就一个 BRAN，而 BRAN 自己
  就是交付单元类型。挂在 `final_refnos` 上就是几十倍的重复请求。
- **容器要客户端展一层。** `resolve_generation_root` 解不出根就 `bail!`，而按契约
  `WORL / SITE / ZONE` 恒被拒绝做生成根——直接 ensure 一个 ZONE 会拿到 500。显示 ZONE 是
  家常便饭，所以客户端在容器上展开一层、对子节点逐个 ensure（`child_nodes` 是现成的）。
- **顺带修掉一个判得太松的地方。** `renderable_instance_count` 走
  `query_deep_children_refnos`，数的是整棵子树。一个 ZONE 下面 20 个 BRAN 只要有 1 个有模型，
  ensure 就返 `AlreadyAvailable`，剩下 19 个继续隐形。展一层逐个 ensure 之后，判断落到
  交付单元这一级，这个盲区自然消失（单个 BRAN 内部是整根生成，不存在半拉子）。
- **必须异步 + 限并发 + 有进度。** ensure 是同步请求、上限 120 秒，而显示是交互动作。
  发出去要排队、限并发，界面上给一条「正在补齐 N 个模型」。这条进度与手动更新那条不是
  一回事，别共用一个任务窗。
- **`ensure` 幂等，但别指望它免费。** 每次显示都发一轮请求，即使全是 `AlreadyAvailable`
  也是一轮 HTTP。客户端要记住这一会话里已经 ensure 过的 refno，别在每次切可见性时重发。
- **服务地址在这一侧也写死了 8020。** `plant_ui_host.rs` 里那句
  `std::env::var("PLANT_MODEL_API_URL").unwrap_or_else(|_| "http://127.0.0.1:8020".into())`
  与 plant-ui 那份是同一个 bug 的两份拷贝，而**这份才是真正会发出去的那份**——plant-ui-app
  只是开发壳。按 ADR-0008 一并改成 `plant_ui::settings::DEFAULT_MODEL_API_URL`，并接上 S6
  的「模型服务地址」。
