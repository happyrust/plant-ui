//! plant-ui-data：SUL_DB 查询 -> Vm，不含任何绘制。
//! 决定 3/7：不造 DTO，直接用 aios_core 类型；直连本地 SurrealDB，不做 mock。

use aios_core::{DBType, SUL_DB};
use anyhow::Result;

pub use aios_core::pdms_types::EleTreeNode;
pub use aios_core::{RefU64, RefnoEnum};

pub mod room;

/// 连接本地 SurrealDB（读取工作目录的 DbOption.toml，走 aios_core 全局句柄 SUL_DB）。
pub async fn connect() -> Result<()> {
    aios_core::init_surreal().await?;
    // 平表读连接池后台预热（P4）：4 条连接的握手+签入约 2s，放启动期消化，
    // 首次整场重载不再吃这口冷启动（并发安全，重载若抢先会等同一次初始化）。
    #[cfg(not(target_arch = "wasm32"))]
    tokio::spawn(aios_core::prewarm_flat_read_pool());
    Ok(())
}

/// M0-4 验收：读一批带名称的元素（refno, name, noun）。
pub async fn sample_named_elements(limit: usize) -> Result<Vec<(String, String, String)>> {
    let sql = format!(
        "SELECT VALUE [record::id(id), refno.NAME, noun] FROM pe WHERE refno.NAME != NONE LIMIT {limit}"
    );
    let mut response = SUL_DB.query(&sql).await?;
    let rows: Vec<(String, String, String)> = response.take(0)?;
    Ok(rows)
}

/// 模型树根层：当前 MDB 世界下的 SITE 节点（带 children_count）。
pub async fn site_nodes() -> Result<Vec<EleTreeNode>> {
    let mdb = aios_core::helper::to_e3d_name(&aios_core::get_db_option().mdb_name).into_owned();
    aios_core::get_mdb_world_site_ele_nodes(mdb, DBType::DESI).await
}

/// 任意元素的直接子层（无名节点由查询侧按 noun 补默认名）。
///
/// 走 [`aios_core::get_children_tree_nodes`] 精简查询：模型树只吃
/// refno / noun / name / children_count，旧壳树才用的 `order` / `mod_cnt`
/// 两个逐行子查询占了近半耗时（769 子的 ZONE 实测 330ms → ~190ms）。
pub async fn child_nodes(refno: RefnoEnum) -> Result<Vec<EleTreeNode>> {
    aios_core::get_children_tree_nodes(refno).await
}

/// 元素到库顶的祖先链，顺序是「自己 -> 上级 -> …」（第 0 项就是它本人）。
///
/// 树定位专用（ADR-0014）：目标还没被任何一次展开物化时，客户端手上没有它的
/// OWNER 链，只能现查一条出来。链长由查询侧的 `fn::ancestor` 定死（见
/// `resource/surreal/common.surql`），比那个层数更深的元素在这里回不出根，
/// 定位那边会把它当树外元素——真遇到就得先把那个函数放深。
pub async fn ancestor_refnos(refno: RefnoEnum) -> Result<Vec<RefU64>> {
    Ok(aios_core::query_ancestor_refnos(refno)
        .await?
        .into_iter()
        .map(|refno| refno.refno())
        .collect())
}

/// 当前模型树根下已经生成的几何实例。数据库只给实例与网格 hash，
/// 网格文件仍由 Bevy AssetServer 从 `assets/meshes` 加载。
///
/// **这是整个界面里最贵的一次查询**，而调用点（`Req::Models`）在它之前刚
/// `invalidate_all` 过，所以每次都是冷的。旧路径在 AvevaMarineSample / MDB ALL
/// 上实测一次冷启动 88 秒，八成时间花在按根串行解可见实例集
/// （`query_deep_visible_inst_refnos`，69.9 s / 79%）。
///
/// 现在默认走 anc 索引路径（见 [`model_instances_anc`]）：解析成本归零，
/// 剩下的地板是实例投影本身；根间 8 路并发。合成 AMS 量级对拍实测
/// 346 s → 16 s（gen-model `docs/plans/2026-08-07-inst-relate-anc-u64-
/// hierarchy-query-plan.md` P2 前置验证节）。
pub async fn model_instances(roots: &[RefU64]) -> Result<Vec<aios_core::GeomInstQuery>> {
    model_instances_with_progress(roots, |_, _| {}).await
}

/// 查询已经生成好的模型，并按查询分块报告进度。
///
/// 唯一路径是 [`model_instances_anc`]（每根一条 `anc CONTAINS` 索引查询，
/// 根间并发）。旧深遍历路径（`model_instances_legacy`）与
/// `PLANT_UI_LEGACY_MODEL_QUERY` 回退开关已随层级查询优化 P3 退役——它经
/// `query_inst_refnos_by_zone` 消费 `inst_relate.zone_refno`，而 gen-model
/// 已不再写该列，旧路径对新行只会静默漏，不配再当回退保险丝。
///
/// `anc` 未回填的库不再静默降级，而是响亮失败：升级 gen-model 并对该库启动
/// 一次（启动序列的幂等自愈回填）即恢复。这里仍然只读 SurrealDB；mesh 文件
/// 由 View3d 的 AssetLoader 消费，不会触发模型生成。
pub async fn model_instances_with_progress(
    roots: &[RefU64],
    progress: impl FnMut(usize, usize),
) -> Result<Vec<aios_core::GeomInstQuery>> {
    match aios_core::inst_relate_anc_ready().await {
        Ok(true) => model_instances_anc(roots, progress).await,
        Ok(false) => anyhow::bail!(
            "inst_relate.anc 未回填，模型查询无法进行：用新版 gen-model 对该库启动一次\
             （启动序列自愈回填）后重试"
        ),
        Err(error) => Err(error.context("anc 覆盖探测失败")),
    }
}

/// 新路径（层级查询优化 P2）：每根一条 `anc CONTAINS $root` 索引查询解出
/// 实例/支管 refno 列表（替代深遍历解析，id 列表载荷百 KB 级封顶），投影仍走
/// 久经考验的分批 `query_insts` / `query_tubi_insts_by_brans`（500/批——整根
/// 全投影一条响应在大 SITE 上会撑爆单条 WS 消息，AMS 实测教训）。根间 8 路
/// 并发（`buffered` 保输入序，结果顺序确定）。根类型无关——SITE/ZONE/PIPE/
/// BRAN/叶子一律同一条查询，不再需要辨名词、SITE→ZONE 中转与深遍历。
///
/// 进度口径：每根完成算 1 步（退役前的旧路径按 500 行分块计步，粒度不同但
/// 语义同为「已完成/总数」）。pub 供计时探针（`tests/anc_model_query_parity.rs`
/// 的 timing 用例；对拍基线已随旧路径退役，验收记录见 gen-model 方案文档 P2 节）
/// 与排障直接调用。
pub async fn model_instances_anc(
    roots: &[RefU64],
    mut progress: impl FnMut(usize, usize),
) -> Result<Vec<aios_core::GeomInstQuery>> {
    use futures::stream::StreamExt;
    const CONCURRENCY: usize = 8;
    let total = roots.len();
    let mut done = 0;
    progress(done, total);

    // 1) 解析：每根两条 anc 索引查询（8 路真任务）。
    let resolutions = roots.iter().copied().map(|root| {
        spawn_query(async move {
            let root: aios_core::RefnoEnum = root.into();
            futures::future::try_join(
                aios_core::query_inst_refnos_by_root_anc(root),
                aios_core::query_bran_refnos_by_root_anc(root),
            )
            .await
        })
    });
    let resolved: Vec<(Vec<aios_core::RefnoEnum>, Vec<aios_core::RefnoEnum>)> =
        futures::stream::iter(resolutions)
            .buffered(CONCURRENCY)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect::<Result<_>>()?;

    // 2) 全局块队列：跨根摊平再并行。按根并发时，巨型 SITE 的十几个批在自己根
    //    的 future 里串成链、成为整场的尾巴（AMS 实测根偏斜让 8 路根并发几乎
    //    白并）；摊平后所有块在同一池子里跑，尾巴只剩最后一个块。
    enum Job {
        Inst(usize, Vec<aios_core::RefnoEnum>),
        Bran(usize, Vec<aios_core::RefnoEnum>),
    }
    let mut jobs = Vec::new();
    let mut remaining = vec![0usize; total];
    for (idx, (inst_refnos, bran_refnos)) in resolved.iter().enumerate() {
        // 平表行很瘦（~0.4KB），1500/批约 600KB 一响应，离 WS 单条上限很远
        // （撑爆上限的是整根全投影那种 MB 级载荷）。
        for chunk in inst_refnos.chunks(1500) {
            jobs.push(Job::Inst(idx, chunk.to_vec()));
            remaining[idx] += 1;
        }
        for chunk in bran_refnos.chunks(500) {
            jobs.push(Job::Bran(idx, chunk.to_vec()));
            remaining[idx] += 1;
        }
    }
    for &r in &remaining {
        if r == 0 {
            done += 1;
        }
    }
    progress(done, total);

    // 3) 执行：平表两段式（P4 写时物化）——第一段平表副本零解引用零子查询；
    //    缺副本的行（清扫未及、pre-P4 存量）聚拢走 slim 现值兜底。正确性不依赖
    //    物化覆盖率，覆盖率只买速度。
    let executions = jobs.into_iter().map(|job| {
        spawn_query(async move {
            match job {
                Job::Inst(idx, chunk) => {
                    let (mut models, missing) = aios_core::query_insts_flat(chunk.iter()).await?;
                    if !missing.is_empty() {
                        models.extend(aios_core::query_insts_slim(missing.iter()).await?);
                    }
                    anyhow::Ok((idx, models))
                }
                Job::Bran(idx, chunk) => anyhow::Ok((
                    idx,
                    aios_core::query_tubi_insts_by_brans(&chunk)
                        .await?
                        .into_iter()
                        .map(tubi_to_geom)
                        .collect(),
                )),
            }
        })
    });
    let mut models = Vec::new();
    let mut stream = futures::stream::iter(executions).buffered(CONCURRENCY);
    while let Some(result) = stream.next().await {
        let (idx, batch) = result?;
        models.extend(batch);
        remaining[idx] -= 1;
        if remaining[idx] == 0 {
            done += 1;
            progress(done, total);
        }
    }
    Ok(models)
}

/// 非 wasm 下把查询未来包进 `tokio::spawn` 真任务：`buffered` 本身是单任务
/// 轮询，51k 行的响应反序列化会全部挤在一个线程上（release 实测 ~56µs/行，
/// 单线程地板 ~3s）——spawn 让解析散到多线程运行时，与 rs-core 侧的平表读
/// 连接池（4 条 WS）配对才能真并行。wasm 无多线程运行时，原样轮询。
#[cfg(not(target_arch = "wasm32"))]
fn spawn_query<F>(fut: F) -> impl std::future::Future<Output = F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    let handle = tokio::spawn(fut);
    async move { handle.await.expect("查询任务 join 失败") }
}

#[cfg(target_arch = "wasm32")]
fn spawn_query<F: std::future::Future>(fut: F) -> F {
    fut
}

/// 直管段实例 → 几何实例的统一换装（`owner` 取自身、单实例 `is_tubi`、
/// generic 缺省 PIPE 都是退役前旧路径的既有口径，anc 路径原样继承）。
fn tubi_to_geom(tubi: aios_core::rs_surreal::inst::TubiInstQuery) -> aios_core::GeomInstQuery {
    let refno = tubi.refno;
    aios_core::GeomInstQuery {
        refno,
        old_refno: tubi.old_refno,
        owner: refno,
        world_aabb: tubi.world_aabb,
        world_trans: tubi.world_trans,
        insts: vec![aios_core::ModelHashInst {
            geo_hash: tubi.geo_hash,
            transform: Default::default(),
            is_tubi: true,
            is_invalid_tubi: tubi.invalid,
        }],
        has_neg: false,
        generic: tubi.generic.unwrap_or_else(|| "PIPE".into()),
        pts: None,
        date: tubi.date,
    }
}

/// 一个范围里**已经生成过模型**的元素，连同各自的祖先链（refno 的 u64 原值）。
///
/// 「重新生成模型」的取材查询。走的是模型查询同一条 `inst_relate.anc` 索引，
/// 任意根类型通吃（SITE / ZONE / PIPE / BRAN / 叶子一律同一条），所以右键
/// 落在容器行上也不必先展开子层。
///
/// 两条纪律：
///
/// - **必须在任何删除之前调**。它认的是 `inst_relate` 行，删完就查不到了；
///   删完再查回的是空集，而空集在调用方那里长得像「这里本来就没模型」。
/// - `anc` 未回填的库响亮失败，与 [`model_instances_with_progress`] 同一句话。
///   这条路没有深遍历回退——那条旧路径已随层级查询优化 P3 退役。
/// **直管段单独算一份。** 隐含直管走 `tubi_relate` 而不是 `inst_relate`，
/// 只有直管没有管件的 BRAN 在上一条查询里一行都没有。漏掉它们的话，一整根
/// 光管的支管会被当成「没生成过」，重新生成时直接跳过。
pub async fn generated_scope(root: RefU64) -> Result<GeneratedScope> {
    match aios_core::inst_relate_anc_ready().await {
        Ok(true) => {}
        Ok(false) => anyhow::bail!(
            "inst_relate.anc 未回填，取不到已生成元素：用新版 gen-model 对该库启动一次\
             （启动序列自愈回填）后重试"
        ),
        Err(error) => return Err(error.context("anc 覆盖探测失败")),
    }
    let root: RefnoEnum = root.into();
    let (elements, tubing) = futures::future::try_join(
        aios_core::query_generated_subtree_with_anc(root),
        aios_core::query_bran_refnos_by_root_anc(root),
    )
    .await?;
    Ok(GeneratedScope {
        elements,
        tubing_branches: tubing.into_iter().map(|refno| refno.refno()).collect(),
    })
}

/// 一个范围里已经生产出来的模型，按两张边表分开装。
#[derive(Debug, Clone, Default)]
pub struct GeneratedScope {
    /// `inst_relate` 上的几何元素，各自带祖先链。
    pub elements: Vec<aios_core::rs_surreal::inst::GeneratedElement>,
    /// `tubi_relate` 上带直管的 BRAN / HANG。它们本身就是交付单元粒度。
    pub tubing_branches: Vec<RefU64>,
}

impl GeneratedScope {
    /// 确认框上报的「已生成元素」数。两张表各数各的，不去重——
    /// 一根 BRAN 既有管件又有直管时，那是两类产物，都要重做。
    pub fn element_count(&self) -> usize {
        self.elements.len() + self.tubing_branches.len()
    }
}

/// 一批 refno 的 noun。缺行的不进表——**按对返回而不是按位置**，
/// 中间少一行不会把后面所有 noun 都错位一格。
pub async fn nouns_of(refnos: &[RefU64]) -> Result<std::collections::HashMap<RefU64, String>> {
    // 与模型查询同一个分批口径：id 列表载荷太大时单条 WS 消息会撑爆。
    const CHUNK: usize = 1500;
    let mut out = std::collections::HashMap::with_capacity(refnos.len());
    for chunk in refnos.chunks(CHUNK) {
        let keys = chunk
            .iter()
            .map(|refno| RefnoEnum::from(*refno).to_pe_key())
            .collect::<Vec<_>>()
            .join(",");
        let mut response = SUL_DB
            .query(format!("select value [id, noun] from [{keys}]"))
            .await?;
        let rows: Vec<(RefnoEnum, String)> = response.take(0)?;
        out.extend(rows.into_iter().map(|(refno, noun)| (refno.refno(), noun)));
    }
    Ok(out)
}

/// 设计库里还没被应用到模型的会话数。
///
/// 直连 gen-model 的 `dbnum_watermark` 表读，不走它的 HTTP 接口：取回工作是纯
/// 数据库操作，不该因为那个服务没起就连提示都给不出。**代价是这里认得后端的表名
/// 和字段名**——那张表改了结构，这一处得跟着改，而编译器不会提醒。
///
/// 两个边界必须知道：
///
/// - `file_latest_sesno` 是**上一次扫描或应用时记下的**，不是实时读文件。所以这个
///   数只配当提示，不能拿来判断「需不需要取回」。
/// - 水位为 0 的库是从没应用过的（gen-model 那边叫「需初始化」），不参与这个加法。
///   算进去的话一个新登记的库会报出一个天文数字。
pub async fn pending_sessions() -> Result<u32> {
    let sql = format!(
        "SELECT VALUE [applied_sesno, file_latest_sesno] FROM {WATERMARK_TABLE} \
         WHERE db_type = 'DESI' AND applied_sesno > 0 AND file_latest_sesno > applied_sesno"
    );
    let mut response = SUL_DB.query(&sql).await?;
    let rows: Vec<(i32, i32)> = response.take(0)?;
    Ok(rows
        .into_iter()
        .map(|(applied, latest)| (latest - applied).max(0) as u32)
        .sum())
}

/// gen-model 的水位表。跟着 `gen-model/src/data_interface/dbnum_state.rs` 走。
const WATERMARK_TABLE: &str = "dbnum_watermark";

/// 取回工作前先把本进程的查询缓存丢干净。
///
/// 增量更新跑在 gen-model 那个进程里，它清的是它自己那份 memoize；本进程这份
/// 从连上库那一刻起就没人动过。不清就会出现「树重查过了，属性还是旧的」这种
/// 最难查的一类现象——树子层查询恰好没上缓存，属性和根层却上了。
pub async fn invalidate_all() {
    aios_core::clear_all_caches_wholesale().await;
}

/// 按 PDMS 名称查元素，供命令行的 `/名称` 使用。
pub async fn resolve_name(name: &str) -> Result<Option<RefU64>> {
    Ok(aios_core::get_refno_by_name(name)
        .await?
        .map(|refno| refno.refno()))
}

/// 连接后的工程标识：项目名、当前 MDB 名、SurrealDB 命名空间、当前 MDB（DESI）
/// 库编号列表。外壳原则是不摆假数字，这些都取自真实配置与真实查询。
///
/// MDB 名带前导 `/`：模型更新把它发给 gen-model 当本期执行范围的口径，
/// 而库里 `MDB.NAME` 存的就是 `/ALL` 这种形态。
///
/// `mdb_name` 配空时回空串，不回 `to_e3d_name` 那个 `"/"`。摆一个 `MDB /` 出去，
/// 界面会照着它说本期范围、请求也会照着它发，而它谁都不是——空串至少让上面
/// 那几处的空值分支有机会接住。
pub async fn project_identity() -> Result<(String, String, String, Vec<u32>)> {
    let opt = aios_core::get_db_option();
    let mdb = match opt.mdb_name.trim() {
        "" => String::new(),
        name => aios_core::helper::to_e3d_name(name).into_owned(),
    };
    let db_nums = aios_core::query_mdb_db_nums(DBType::DESI).await?;
    Ok((
        opt.project_name.clone(),
        mdb,
        opt.surreal_ns.clone(),
        db_nums,
    ))
}

/// 属性值的可编辑形态。旧壳靠 `bevy_reflect` 下转拿到这个信息来决定给什么控件，
/// 新链路里 `NamedAttrValue` 的变体就是它。
///
/// 引用、方位、数组一律 `Opaque`：它们要元素选择器或方位串解析器才能安全地改，
/// 给个自由文本框只会写进去一个非法值。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrKind {
    /// 未设值。PDMS 里给未设值的属性赋值需要先知道它的字典类型，暂不可改。
    Unset,
    Int,
    Real,
    Bool,
    Text,
    Opaque,
}

/// 一条属性：真实属性名、面板显示串、可编辑形态。
#[derive(Debug, Clone)]
pub struct Attr {
    pub name: String,
    pub value: String,
    pub kind: AttrKind,
}

/// 元素的 UI 属性表：走 `get_ui_named_attmap`（含 UDA、引用转全名、
/// POS/ORI 转 PDMS 方位串、unset 补齐），格式化成显示串。
/// BTreeMap 保证字母序，分组逻辑在 App 侧。
///
/// 类型另取自 `get_named_attmap_with_uda`：UI 版会把引用和未设值都压成
/// `StringType`，只看它的话「引用」「未设值」「真文本」三者分不开，可编辑判定
/// 就会把引用错当文本放开。原始版带 `#[cached]`，第二次查同一元素不再打库。
pub async fn element_props(refno: RefnoEnum) -> Result<Vec<Attr>> {
    let attmap = aios_core::get_ui_named_attmap(refno).await?;
    let mut raw = aios_core::get_named_attmap_with_uda(refno).await.ok();
    // UI 版取类型之前先补了一轮字典默认值，这里不补两张表的键集就对不上：凡是取自
    // 默认值的属性在 raw 里查不到，`map_or` 会把它们一律当成 Opaque 锁成只读。
    // 实测一个 BRAN 有 32 个键走这条路，其中 9 个是本该可编辑的标量。
    if let Some(raw) = &mut raw {
        raw.fill_explicit_default_values();
    }
    Ok(attmap
        .map
        .iter()
        .map(|(k, v)| {
            let value = fmt_attr(v);
            // TYPE / REFNO 在库里是普通字符串，但改元素的类型和引用号不是编辑一个
            // 值，是换一个元素，不能给文本框。
            let kind = if value == "unset" {
                AttrKind::Unset
            } else if matches!(k.as_str(), "TYPE" | "REFNO") {
                AttrKind::Opaque
            } else {
                raw.as_ref()
                    .and_then(|m| m.map.get(k))
                    .map_or(AttrKind::Opaque, attr_kind)
            };
            Attr {
                name: k.clone(),
                value,
                kind,
            }
        })
        .collect())
}

fn attr_kind(v: &aios_core::NamedAttrValue) -> AttrKind {
    use aios_core::NamedAttrValue as V;
    match v {
        V::InvalidType => AttrKind::Unset,
        V::IntegerType(_) | V::LongType(_) => AttrKind::Int,
        V::F32Type(_) => AttrKind::Real,
        V::BoolType(_) => AttrKind::Bool,
        V::StringType(_) | V::WordType(_) => AttrKind::Text,
        _ => AttrKind::Opaque,
    }
}

/// NamedAttrValue -> 面板显示串。`get_ui_named_attmap` 已把引用 / 方位类
/// 转成字符串，这里只兜剩余的标量与数组形态。
fn fmt_attr(v: &aios_core::NamedAttrValue) -> String {
    use aios_core::NamedAttrValue as V;
    fn f32s(x: f32) -> String {
        // 去浮点尾巴：120.0 -> "120"，10.5 保留一位。
        if x.fract().abs() < f32::EPSILON {
            format!("{}", x as i64)
        } else {
            format!("{x}")
        }
    }
    let s = match v {
        V::InvalidType => "unset".into(),
        V::IntegerType(i) => i.to_string(),
        V::LongType(i) => i.to_string(),
        V::F32Type(x) => f32s(*x),
        V::BoolType(b) => if *b { "true" } else { "false" }.into(),
        V::StringType(s) | V::ElementType(s) | V::WordType(s) => s.clone(),
        V::Vec3Type(p) => format!("{} {} {}", f32s(p.x), f32s(p.y), f32s(p.z)),
        V::F32VecType(xs) => xs.iter().map(|x| f32s(*x)).collect::<Vec<_>>().join(" "),
        V::StringArrayType(xs) => xs.join(" "),
        V::BoolArrayType(xs) => xs
            .iter()
            .map(|b| if *b { "true" } else { "false" })
            .collect::<Vec<_>>()
            .join(" "),
        V::IntArrayType(xs) => xs.iter().map(i32::to_string).collect::<Vec<_>>().join(" "),
        V::RefU64Type(r) => r.to_string(),
        V::RefnoEnumType(r) => r.refno().to_string(),
        V::RefU64Array(rs) => rs
            .iter()
            .map(|r| r.refno().to_string())
            .collect::<Vec<_>>()
            .join(" "),
    };
    // 空数组与空串一并归到 unset。一个什么都没有的值格看不出是「空值」还是「没查到」，
    // 而 unset 在面板上是弱化色的明确一行。实测每个 EQUI / NOZZ 都带一行空 DESP。
    if s.trim().is_empty() {
        "unset".into()
    } else {
        s
    }
}
