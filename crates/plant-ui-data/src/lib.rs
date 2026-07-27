//! plant-ui-data：SUL_DB 查询 -> Vm，不含任何绘制。
//! 决定 3/7：不造 DTO，直接用 aios_core 类型；直连本地 SurrealDB，不做 mock。

use aios_core::{DBType, SUL_DB};
use anyhow::Result;

pub use aios_core::pdms_types::EleTreeNode;
pub use aios_core::{RefU64, RefnoEnum};

/// 连接本地 SurrealDB（读取工作目录的 DbOption.toml，走 aios_core 全局句柄 SUL_DB）。
pub async fn connect() -> Result<()> {
    aios_core::init_surreal().await
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
pub async fn child_nodes(refno: RefnoEnum) -> Result<Vec<EleTreeNode>> {
    aios_core::get_children_ele_nodes(refno).await
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

/// 连接后的工程标识：项目名、SurrealDB 命名空间、当前 MDB（DESI）库编号列表。
/// 外壳原则是不摆假数字，这些都取自真实配置与真实查询。
pub async fn project_identity() -> Result<(String, String, Vec<u32>)> {
    let opt = aios_core::get_db_option();
    let db_nums = aios_core::query_mdb_db_nums(DBType::DESI).await?;
    Ok((opt.project_name.clone(), opt.surreal_ns.clone(), db_nums))
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
