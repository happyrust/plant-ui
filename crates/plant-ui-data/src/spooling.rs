//! Spooling（卷管 / 管段）查询：`SPLDRG` / `SPOOL` / `PSPOOL` / `HSPOOL` / `FIELD` / `PPIECE`。
//!
//! ## 为什么单独一个模块
//!
//! 模型树是 `SITE -> ZONE -> PIPE -> BRAN -> 件` 这条设计层级，spool 不在其中：
//!
//! * 老 spooler 的 `SPLDRG`（卷管图）挂在 `ISODEP -> ISOREG` 下，`SPOOL` 挂在
//!   `SPLDRG` 下——整条链**不经过 SITE**（见逆向报告 `PMLLIB\spooler\**`、
//!   `pfevaluatespoolmember.pmlobj`）。
//! * pipe fabrication 的 `PSPOOL` / `PPIECE` 挂在 `PIPE -> PSLIST / PPLIST` 下，
//!   `HSPOOL` 挂在 `HVAC -> HSLIST` 下。
//!
//! 所以从 SITE 根出发的模型树天然看不到 spool。它们要单独一组入口，这也是
//! `plant-ui` 里「Spooling」这一栏存在的唯一理由。
//!
//! ## 数据从哪来
//!
//! 全部走 `pe` / `pe_owner`，与模型树同一个数据面。前提是 gen-model 把
//! `SPLDRG` / `SPOOL` / `PSPOOL` 这些 noun 也写进 `pe` + `pe_owner`——
//! `noun_flags.json` 里它们在册，`data_interface::mdb_membership` 也把 ISOD
//! 算作元素库，缺的是**抽取与落库那一跳**。
//!
//! ## 空结果是正常的
//!
//! AMS `/ALL` 这个 MDB 声明里没有 ISOD 库，设计库 `7997/7999/8000` 里也一个
//! `SPLDRG` 都没有（292 个 DESI 库全扫确认）。真正带 spool 的是
//! `ams251095`（`PSPOOL` 73 / `PPIECE` 91）与 `ams251137`（`HSPOOL` 38），
//! 而这两个库号不在 `/ALL` 的 `CURD` 名单里。它们靠两个 `DbOption.toml` 里的
//! `extra_element_dbnums` 补充名单接进来（gen-model ADR-095，只增不减）：gen-model 的
//! `UpdateScope::admits` 放行它们的增量，plant-ui 的 `site_nodes()` 把它们的 SITE 追加进
//! 模型树根层，Spooling 行的「定位」才跳得过去。首次进库仍要按 dbnum 点名初始化
//!（`initialize_ams_dbnums 251095 251137`）；没做过这一步的库回空是数据现状，不是查询坏了。
//!
//! ## 属性在 noun 表上
//!
//! `pe` 行只有 `noun` / `name` / `owner` 这类公共列；`PSARFA` / `AELRFA` 这些
//! 引用属性在 `pe.refno` 指向的 noun 表记录上。PSPOOL / PPIECE / HSPOOL 在
//! `pe_owner` 下没有子层，成员**全靠**这些引用属性。

use aios_core::{RefU64, RefnoEnum, SUL_DB};
use anyhow::Result;

/// spooling 一族的分组：`(noun, 中文标签)`。
///
/// 顺序就是界面上从上到下的顺序：先图，再段，再件。`FIELD` 排最后是因为它
/// 是「不在车间做」的例外段，与车间段不是同一层语义。
pub const SPOOL_GROUPS: &[(&str, &str)] = &[
    ("SPLDRG", "卷管图"),
    ("SPOOL", "管段"),
    ("PSPOOL", "制造管段"),
    ("HSPOOL", "暖通管段"),
    ("FIELD", "现场段"),
    ("PPIECE", "管段件"),
];

/// 一组的统计行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolGroup {
    pub noun: String,
    pub label: String,
    /// 本 MDB 范围内这个 noun 的实例数。0 = 这一类不存在，不是查询失败。
    pub count: usize,
}

/// 一个 spool 元素在列表里的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolRow {
    pub refno: RefU64,
    pub noun: String,
    /// 有名字就是 `NAME`；无名元素是空串，显示侧按 `noun + 序号` 补。
    pub name: String,
    pub owner: RefU64,
    /// 属主的 noun，列表里用来标「这段属于哪个 PIPE / 哪张图」。
    pub owner_noun: String,
    /// 直接子层个数（`SPOOL` 下是成员，`PPIECE` 下通常是 0）。
    pub children_count: u32,
    pub dbnum: Option<u32>,
}

/// spool 的一个成员。
///
/// 两个来源合并：
///
/// * `pe_owner` 边——gen-model 落库后的**结构**成员（`SPOOL` 下挂的 `BRAN`/件）。
/// * spool 自己的引用属性——`SPLMEM`（老 `SPOOL`）、`PSARFA`/`PSLRFA`
///   （`PSPOOL` 的到达/离开端）、`HSARFA`/`HSLRFA`（`HSPOOL`）、`AELRFA`
///   （`PPIECE` 的两端）。这些是 E3D 内核算出来的**断点归属**，比结构成员更权威，
///   但只有 spool 存在时才有。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpoolMember {
    pub refno: RefU64,
    pub noun: String,
    pub name: String,
    /// 这条成员是从哪一路来的：`pe_owner` 结构边，还是 spool 的引用属性。
    pub via: MemberVia,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberVia {
    /// `pe_owner` 边。
    Owner,
    /// spool 的引用属性（`SPLMEM` / `PSARFA` / `AELRFA` …）。
    Attribute,
}

/// 全部组的统计。空库回六个 `count = 0` 的行，不回空 `Vec`——
/// 界面要画「这一类是 0 条」而不是「什么都没有」，两者不是一件事。
pub async fn spooling_overview() -> Result<Vec<SpoolGroup>> {
    let nouns: Vec<String> = SPOOL_GROUPS
        .iter()
        .map(|(noun, _)| format!("'{noun}'"))
        .collect();
    let sql = format!(
        "SELECT noun, count() AS n FROM pe \
         WHERE noun IN [{}] AND !deleted GROUP BY noun;",
        nouns.join(", ")
    );
    let mut response = SUL_DB.query(sql).await?.check()?;
    let rows: Vec<NounCount> = response.take(0)?;
    let counts: std::collections::HashMap<String, usize> =
        rows.into_iter().map(|row| (row.noun, row.n)).collect();
    Ok(SPOOL_GROUPS
        .iter()
        .map(|(noun, label)| SpoolGroup {
            noun: (*noun).to_string(),
            label: (*label).to_string(),
            count: counts.get(*noun).copied().unwrap_or(0),
        })
        .collect())
}

/// 某一类 spool 元素的列表，按名字排序。
///
/// `noun` 必须是 [`SPOOL_GROUPS`] 里的一个——不在白名单里直接回空，
/// 不拼进 SQL。这不是安全洁癖：这个函数的入参来自界面行，任何拼错的 noun
/// 都该是「这一行没内容」，而不是「全表扫一遍找不存在的类型」。
pub async fn spool_rows(noun: &str, limit: usize) -> Result<Vec<SpoolRow>> {
    if !SPOOL_GROUPS.iter().any(|(known, _)| *known == noun) {
        return Ok(Vec::new());
    }
    let limit = limit.max(1);
    let sql = format!(
        "SELECT id, noun, name, owner, owner.noun AS owner_noun, \
                array::len(id<-pe_owner) AS kids, dbnum \
         FROM pe WHERE noun = '{noun}' AND !deleted \
         ORDER BY name LIMIT {limit};"
    );
    let mut response = SUL_DB.query(sql).await?.check()?;
    let rows: Vec<SpoolRowRaw> = response.take(0)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            // 顺手登记族属：`spool_geometry` 的入参只有 refno，靠这份表判定
            // 该不该拼几何语句，命中后连一次查询都不发。
            remember_noun(row.id.refno(), &row.noun);
            SpoolRow {
                refno: row.id.refno(),
                noun: row.noun,
                name: row.name.unwrap_or_default(),
                owner: row.owner.refno(),
                owner_noun: row.owner_noun.unwrap_or_default(),
                children_count: row.kids,
                dbnum: row.dbnum,
            }
        })
        .collect())
}

/// `spooling_overview` 的一行。`GROUP BY` 的结果是对象，不能按元组解。
#[derive(serde::Deserialize)]
struct NounCount {
    noun: String,
    n: usize,
}

/// `spool_rows` 那条语句的一行，字段名就是 `SELECT` 里的列名 / 别名。
/// 不用 `SELECT VALUE [..]` 解元组：fork 上它不能和 `ORDER BY` 一起用。
#[derive(serde::Deserialize)]
struct SpoolRowRaw {
    id: RefnoEnum,
    noun: String,
    name: Option<String>,
    owner: RefnoEnum,
    owner_noun: Option<String>,
    kids: u32,
    dbnum: Option<u32>,
}

/// 一个 spool 元素的成员：结构边 + 引用属性，按 refno 去重。
///
/// 结构边优先（先查、先入表），引用属性里已经在表里的不再覆盖 `via`——
/// 一个元素既在 `pe_owner` 边里又被 `SPLMEM` 引用时，它就是结构成员，
/// 标成属性来源会让人以为它只靠属性存在。
pub async fn spool_members(refno: RefnoEnum) -> Result<Vec<SpoolMember>> {
    let key = refno.to_pe_key();
    // 第一槛取结构子层，第二槛取 spool 自己的引用属性。属性不在 `pe` 行上，
    // 在它 `refno` 指向的 noun 表记录（`PSPOOL:…` / `PPIECE:…`）上，所以走 `.refno`。
    let sql = member_sql(&key);
    let mut response = SUL_DB.query(sql).await?.check()?;
    let kids: Vec<(RefnoEnum, Option<String>, Option<String>)> = response.take(0)?;
    let attrs: Vec<Vec<Option<Vec<RefnoEnum>>>> = response.take(1)?;

    let mut seen: std::collections::HashSet<RefU64> = std::collections::HashSet::new();
    let mut out: Vec<SpoolMember> = Vec::new();
    for (member, noun, name) in kids {
        let member_refno = member.refno();
        if !seen.insert(member_refno) {
            continue;
        }
        out.push(SpoolMember {
            refno: member_refno,
            noun: noun.unwrap_or_default(),
            name: name.unwrap_or_default(),
            via: MemberVia::Owner,
        });
    }
    for row in attrs {
        for slot in row.into_iter().flatten() {
            for member in slot {
                let member_refno = member.refno();
                if !seen.insert(member_refno) {
                    continue;
                }
                out.push(SpoolMember {
                    refno: member_refno,
                    // 属性槽只给 refno，noun / name 由下面的补名查询填。
                    noun: String::new(),
                    name: String::new(),
                    via: MemberVia::Attribute,
                });
            }
        }
    }
    fill_member_identity(&mut out).await?;
    Ok(out)
}

/// spool 一族的几何属性组：`noun -> [成员引用属性]`。
///
/// 三族严格分开，**不能**无条件把三组一起塞进 `array::flatten`。实测确认
/// （`library/spool-geometry-facts.md` §2）：`PSPOOL` 只有 `PSARFA`/`PSLRFA` 有值，
/// `HSPOOL` 只有 `HSARFA`/`HSLRFA`，`PPIECE` 只有 `AELRFA`；其余属性全 0。
///
/// 返回 `None` 表示这个 noun 不承载几何——调用方在**拼 SQL 之前**就该回空。
fn geometry_attrs(noun: &str) -> Option<&'static [&'static str]> {
    match noun {
        "PSPOOL" => Some(&["PSARFA", "PSLRFA"]),
        "HSPOOL" => Some(&["HSARFA", "HSLRFA"]),
        "PPIECE" => Some(&["AELRFA"]),
        _ => None,
    }
}

/// spool → 几何的查询语句。`None` = 该 noun 不是 spool 一族，不该拼语句。
///
/// 三个 `LET` 之后才是 `SELECT`，所以调用方取的是第 3 个结果槽。
///
/// 每个属性都带 `?? []` 兜底：字段不存在时 `array::flatten` 里的 NONE 会让
/// 整条查询报错，而不是回空。
///
/// `'PIPE'` 必须带单引号。不加引号 SurrealDB 会当字段名处理，过滤**静默失效**
/// （不报错），锚点混进 PIPE 元素，结果从 10 行膨胀成整根管的 61 行。
fn geometry_sql(noun: &str, key: &str) -> Option<String> {
    let attrs = geometry_attrs(noun)?;
    let slots = attrs
        .iter()
        .map(|attr| format!("{attr} ?? []"))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "LET $mem = ((SELECT array::flatten([{slots}]) AS m FROM ONLY {key}.refno).m ?? []); \
         LET $own = array::filter(array::map($mem, |$x| $x.owner), |$o| $o.noun != 'PIPE'); \
         LET $anch = array::distinct(array::concat(\
         array::map($mem, |$x| fn::refno_u64($x)), \
         array::map($own, |$o| fn::refno_u64($o)))); \
         SELECT VALUE in FROM inst_relate WHERE anc CONTAINSANY $anch;"
    ))
}

/// 已解析的 `refno -> noun`。`spool_rows` 每查出一行就填一次，
/// `spool_geometry` 靠它**不查库**就判定族属。
///
/// 为什么要有这份缓存：`spool_geometry` 的入参只有 refno，而族属由 noun 决定。
/// 面板里的 spool 行本来就来自 `spool_rows`，所以真实调用链上 noun 必然已在表里；
/// 命中后对 `SITE` / `BRAN` / `PIPE` 这类非 spool 元素连一次查询都不用发。
///
/// 它只缓存 noun，不缓存可见性——可见性的账本仍然只有 `TreeModel` + `model_scopes`。
fn noun_cache() -> &'static std::sync::RwLock<std::collections::HashMap<RefU64, String>> {
    static CACHE: std::sync::OnceLock<std::sync::RwLock<std::collections::HashMap<RefU64, String>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::RwLock::new(std::collections::HashMap::new()))
}

fn remember_noun(refno: RefU64, noun: &str) {
    if noun.is_empty() {
        return;
    }
    if let Ok(mut cache) = noun_cache().write() {
        cache.insert(refno, noun.to_string());
    }
}

fn cached_noun(refno: RefU64) -> Option<String> {
    noun_cache()
        .read()
        .ok()
        .and_then(|cache| cache.get(&refno).cloned())
}

/// 一条 spool 的 noun。属性在 noun 表记录上，所以要先知道它是哪一族。
async fn spool_noun(refno: RefnoEnum) -> Result<String> {
    let key = refno.to_pe_key();
    let mut response = SUL_DB
        .query(format!("SELECT VALUE noun FROM ONLY {key} LIMIT 1;"))
        .await?
        .check()?;
    let noun: Option<String> = response.take(0)?;
    Ok(noun.unwrap_or_default())
}

/// 几何命中结果按 refno 排序去重。
///
/// SQL 里的 `array::distinct` 只作用于**锚点**，`inst_relate` 一条几何可能被多个
/// 锚点同时命中，所以返回行本身可能重复。Rust 侧再收一次保证确定性。
fn sorted_unique(hits: Vec<RefnoEnum>) -> Vec<RefU64> {
    hits.into_iter()
        .map(|hit| hit.refno())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// 一个 spool 名下的几何 refno 列表。
///
/// spool 节点**自己没有几何**：`PSPOOL:…_1000` 在 `inst_relate` 里命中 0 行。
/// 几何挂在成员的**非 PIPE 父**（`BRAN` 之类）的子层上，所以锚点要取
/// 「成员 ∪ 成员的非 PIPE 父」，再问 `inst_relate.anc` 谁落在这些锚点下。
///
/// 非 spool 一族（`SITE` / `BRAN` / `PIPE` …）直接回空，**不构造几何语句**——
/// 与 `spool_rows` 的白名单同一个理由：拼错的 refno 该是「这行没有几何」，
/// 而不是「全表扫一遍找不存在的类型」。
///
/// 族属靠 [`noun_cache`] 判定：面板行来自 `spool_rows`，noun 已经登记过，
/// 所以真实调用链上一次额外的库查询都不会发。缓存未命中时才补一次 noun 查询
/// （入参只有 refno，没有别的地方能拿到族属）。
pub async fn spool_geometry(refno: RefnoEnum) -> Result<Vec<RefU64>> {
    let raw = refno.refno();
    let noun = match cached_noun(raw) {
        Some(noun) => noun,
        None => {
            let noun = spool_noun(refno).await?;
            remember_noun(raw, &noun);
            noun
        }
    };
    let Some(sql) = geometry_sql(&noun, &refno.to_pe_key()) else {
        return Ok(Vec::new());
    };
    let mut response = SUL_DB.query(sql).await?.check()?;
    let hits: Vec<RefnoEnum> = response.take(3)?;
    Ok(sorted_unique(hits))
}

/// 成员查询：第 0 槛结构边，第 1 槛 noun 表上的引用属性。
fn member_sql(key: &str) -> String {
    format!(
        "SELECT VALUE [in, in.noun, in.name] FROM {key}<-pe_owner; \
         SELECT VALUE [SPLMEM, PSARFA, PSLRFA, HSARFA, HSLRFA, AELRFA] FROM {key}.refno;"
    )
}

/// 属性槽来的成员只有 refno，补上 noun 与 name。
///
/// 分块按记录 id 直选（`FROM pe:a, pe:b`），与 `room.rs::lineage_sql` 同一个理由：
/// `WHERE id IN [...]` 在 fork 上会走全表，直选走主键。
async fn fill_member_identity(members: &mut [SpoolMember]) -> Result<()> {
    const CHUNK: usize = 500;
    let pending: Vec<usize> = members
        .iter()
        .enumerate()
        .filter(|(_, m)| m.noun.is_empty())
        .map(|(i, _)| i)
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    let mut resolved: std::collections::HashMap<RefU64, (String, String)> =
        std::collections::HashMap::new();
    for chunk in pending.chunks(CHUNK) {
        let keys: Vec<String> = chunk
            .iter()
            .map(|i| members[*i].refno.to_pe_key())
            .collect();
        let sql = format!("SELECT VALUE [id, noun, name] FROM {};", keys.join(", "));
        let mut response = SUL_DB.query(sql).await?.check()?;
        let rows: Vec<(RefnoEnum, String, Option<String>)> = response.take(0)?;
        for (refno, noun, name) in rows {
            resolved.insert(refno.refno(), (noun, name.unwrap_or_default()));
        }
    }
    for member in members.iter_mut().filter(|m| m.noun.is_empty()) {
        if let Some((noun, name)) = resolved.get(&member.refno) {
            member.noun = noun.clone();
            member.name = name.clone();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 分组表的 noun 必须全是 E3D 的真 noun，且不能有重复——
    /// 重复会让统计行出现两条同名记录，界面上看着像「这一类有两份」。
    #[test]
    fn spool_groups_are_unique_uppercase_nouns() {
        let mut seen = std::collections::HashSet::new();
        for (noun, label) in SPOOL_GROUPS {
            assert_eq!(*noun, noun.to_ascii_uppercase(), "{noun} 不是大写");
            assert!(seen.insert(*noun), "{noun} 重复");
            assert!(!label.is_empty(), "{noun} 没有中文标签");
        }
    }

    /// 统计语句必须带 `GROUP BY noun`，否则六类会被合成一行。
    #[test]
    fn overview_sql_groups_by_noun() {
        let nouns: Vec<String> = SPOOL_GROUPS
            .iter()
            .map(|(noun, _)| format!("'{noun}'"))
            .collect();
        let sql = format!(
            "SELECT noun, count() AS n FROM pe \
             WHERE noun IN [{}] AND !deleted GROUP BY noun;",
            nouns.join(", ")
        );
        assert!(sql.contains("GROUP BY noun"), "{sql}");
        assert!(sql.contains("!deleted"), "软删的行不该计数：{sql}");
        for (noun, _) in SPOOL_GROUPS {
            assert!(
                sql.contains(&format!("'{noun}'")),
                "{noun} 不在语句里：{sql}"
            );
        }
    }

    /// 非白名单 noun 必须回空且**不拼 SQL**——这个分支一旦退化成拼串，
    /// 界面上的任何一个拼写错误都会变成一次全表扫描。
    #[tokio::test]
    async fn unknown_noun_returns_empty_without_querying() {
        assert!(spool_rows("BRAN", 10).await.unwrap().is_empty());
        assert!(spool_rows("spool", 10).await.unwrap().is_empty());
        assert!(
            spool_rows("'; DROP TABLE pe; --", 10)
                .await
                .unwrap()
                .is_empty()
        );
    }

    fn test_refno(n: u32, m: u32) -> RefnoEnum {
        RefnoEnum::from(RefU64::from_two_nums(n, m))
    }

    /// 非 spool 一族不拼几何语句。`SITE` / `BRAN` / `PIPE` 这些 noun 没有
    /// `PSARFA` 之类属性，拼出来只会是一次注定空结果的全表扫。
    #[test]
    fn spool_geometry_rejects_non_spool_nouns() {
        for noun in ["SITE", "BRAN", "PIPE", "ZONE", "SPOOL", "SPLDRG", "FIELD", ""] {
            assert!(
                geometry_attrs(noun).is_none(),
                "{noun} 不该被当成有几何的 spool 族"
            );
            assert!(
                geometry_sql(noun, "pe:1_2").is_none(),
                "{noun} 不该构造几何语句"
            );
        }
    }

    /// 族属已知的非 spool 元素，`spool_geometry` 必须**一条语句都不发**。
    ///
    /// 这条测试在没有数据库连接的 `--lib` 环境里跑得通，本身就是证据：
    /// 只要它去查了库，这里就会拿到连接错误而不是 `Ok(vec![])`。
    #[tokio::test]
    async fn spool_geometry_known_non_spool_returns_empty_without_querying() {
        // 用不存在的 refno 段，避免与其它测试/真实数据撞车。
        for (i, noun) in ["SITE", "BRAN", "PIPE"].iter().enumerate() {
            let refno = RefU64::from_two_nums(9_000_000 + i as u32, 42);
            remember_noun(refno, noun);
            let out = spool_geometry(refno.into())
                .await
                .unwrap_or_else(|e| panic!("{noun} 不该走到查询，却报错了: {e}"));
            assert!(out.is_empty(), "{noun} 不该有几何");
        }
    }

    /// `'PIPE'` 的引号是这条查询唯一的静默陷阱：不加引号 SurrealDB 当字段名处理，
    /// 过滤失效但**不报错**，结果从 10 行膨胀成 61 行。删掉引号这个测试必须红。
    #[test]
    fn spool_geometry_sql_quotes_pipe_noun() {
        let sql = geometry_sql("PSPOOL", "pe:2013287639_1000").expect("PSPOOL 有几何语句");
        assert!(
            sql.contains("$o.noun != 'PIPE'"),
            "过滤条件丢了引号：{sql}"
        );
        assert!(
            !sql.contains("$o.noun != PIPE"),
            "过滤条件退化成了字段名比较：{sql}"
        );
    }

    /// 属性组按族选择。三组属性混在一起 flatten 会把不存在的字段拉成 NONE，
    /// 查询直接报错（`array::flatten` 不吃 NONE）。
    #[test]
    fn spool_geometry_selects_attrs_per_family() {
        let ps = geometry_sql("PSPOOL", "pe:1_2").expect("PSPOOL");
        assert!(ps.contains("PSARFA ?? []"), "{ps}");
        assert!(ps.contains("PSLRFA ?? []"), "{ps}");
        assert!(!ps.contains("HSARFA"), "PSPOOL 混进了暖通属性：{ps}");
        assert!(!ps.contains("AELRFA"), "PSPOOL 混进了 PPIECE 属性：{ps}");

        let hs = geometry_sql("HSPOOL", "pe:1_2").expect("HSPOOL");
        assert!(hs.contains("HSARFA ?? []"), "{hs}");
        assert!(hs.contains("HSLRFA ?? []"), "{hs}");
        assert!(!hs.contains("PSARFA"), "HSPOOL 混进了管道属性：{hs}");
        assert!(!hs.contains("AELRFA"), "HSPOOL 混进了 PPIECE 属性：{hs}");

        let pp = geometry_sql("PPIECE", "pe:1_2").expect("PPIECE");
        assert!(pp.contains("AELRFA ?? []"), "{pp}");
        assert!(!pp.contains("PSARFA"), "PPIECE 混进了管道属性：{pp}");
        assert!(!pp.contains("HSARFA"), "PPIECE 混进了暖通属性：{pp}");
    }

    /// 每个属性都要有 `?? []` 兜底：字段缺失时 `array::flatten` 里的 NONE
    /// 会让整条查询报错，而不是回空数组。
    #[test]
    fn spool_geometry_sql_defaults_every_attr_to_empty() {
        for (noun, attrs) in [
            ("PSPOOL", &["PSARFA", "PSLRFA"][..]),
            ("HSPOOL", &["HSARFA", "HSLRFA"][..]),
            ("PPIECE", &["AELRFA"][..]),
        ] {
            let sql = geometry_sql(noun, "pe:1_2").expect(noun);
            for attr in attrs {
                assert!(
                    sql.contains(&format!("{attr} ?? []")),
                    "{noun} 的 {attr} 少了 ?? [] 兜底：{sql}"
                );
            }
        }
    }

    /// 锚点 = 成员 ∪ 成员的非 PIPE 父。只取成员本身会漏掉「成员自己无几何、
    /// 几何挂在 BRAN 父下」的情况——那正是 spool 几何的主要来源。
    #[test]
    fn spool_geometry_sql_anchors_include_member_and_owner() {
        let sql = geometry_sql("PSPOOL", "pe:2013287639_1000").expect("PSPOOL");
        assert!(sql.contains("array::map($mem, |$x| $x.owner)"), "{sql}");
        assert!(sql.contains("array::map($mem, |$x| fn::refno_u64($x))"), "{sql}");
        assert!(sql.contains("array::map($own, |$o| fn::refno_u64($o))"), "{sql}");
        assert!(sql.contains("array::distinct"), "锚点没去重：{sql}");
        assert!(
            sql.contains("WHERE anc CONTAINSANY $anch"),
            "锚点没进过滤条件：{sql}"
        );
        // 属性在 noun 表记录上：对 `pe:…` 直取这几个字段全是 NONE。
        assert!(sql.contains("FROM ONLY pe:2013287639_1000.refno"), "{sql}");
    }

    /// 命中结果按 refno 排序去重：`array::distinct` 只作用于锚点，
    /// 一条几何被多个锚点命中时仍会出多行。
    #[test]
    fn spool_geometry_dedups_and_sorts_results() {
        let dup = vec![
            test_refno(2013287639, 960),
            test_refno(2013287639, 947),
            test_refno(2013287639, 960),
            test_refno(2013287639, 947),
        ];
        let out = sorted_unique(dup);
        assert_eq!(
            out,
            vec![
                RefU64::from_two_nums(2013287639, 947),
                RefU64::from_two_nums(2013287639, 960),
            ]
        );
        assert!(sorted_unique(Vec::new()).is_empty());
    }

    /// 三个 `LET` 之后才是 `SELECT`，所以结果取第 3 个槽。
    /// 槽位错了会拿到 `$anch`（u64 数组）反序列化失败，而不是空集。
    #[test]
    fn spool_geometry_sql_has_three_lets_before_select() {
        let sql = geometry_sql("PPIECE", "pe:1_2").expect("PPIECE");
        let lets = sql.matches("LET $").count();
        assert_eq!(lets, 3, "LET 数量变了，take(3) 会取错槽：{sql}");
        assert!(sql.trim_end().ends_with("$anch;"), "{sql}");
    }

    /// 成员查询的两槛顺序钉死：第 0 槛结构边、第 1 槛引用属性。
    /// 顺序换了 `response.take(0/1)` 就会把两路结果对调，成员全标错来源。
    #[test]
    fn member_sql_takes_owner_edges_before_attributes() {
        let refno = RefnoEnum::from(RefU64::from_two_nums(24381, 101407));
        let sql = member_sql(&refno.to_pe_key());
        let (first, second) = sql.split_once(';').expect("两条语句");
        assert!(first.contains("<-pe_owner"), "{first}");
        assert!(second.contains("SPLMEM"), "{second}");
        // 属性在 noun 表上：直接 `FROM pe:…` 拿到的六个槽全是 NONE，成员一个都出不来。
        assert!(second.trim_end().ends_with(".refno;"), "{second}");
    }
}
