use crate::pdms_types::EleTreeNode;
use crate::pe::SPdmsElement;
use crate::{NamedAttrMap, RefnoEnum};
use crate::{SUL_DB, SurlValue};
use crate::{get_db_option, helper, types::*};
use cached::proc_macro::cached;
use indexmap::IndexMap;
use itertools::Itertools;
use num_enum::{IntoPrimitive, TryFromPrimitive};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::f32::consts::E;
use std::sync::Mutex;

#[derive(IntoPrimitive, TryFromPrimitive, Clone, Copy, Hash, Eq, PartialEq, Debug)]
#[repr(u8)]
pub enum DBType {
    DESI = 1,
    CATA = 2,
    PROP = 3,
    ISOD = 4,
    PADD = 5,
    DICT = 6,
    ENGI = 7,
    SCHE = 14,
    UNSET,
}

/// 按名字取一个 MDB 的 DESI 数据库号列表，同名多条时**取 CURD 最长的那条**。
///
/// 设计库与目录库的 SYS 同时解析时，同名 `/ALL` 会并存（gen-model ADR-007 遗留①），
/// 而目录侧那条的 CURD 往往只有一项甚至为空。原先的 `limit 1` 在两条之间随机挑，
/// 挑中目录侧就只剩一个库可展，模型树看着像"只加载了一个库"。
///
/// 用 `$mdb` / `$db_type` 两个绑定参数，调用点 `bind` 上即可。
const MDB_DESI_DBNOS: &str = r#"(select dbnos, array::len(dbnos) as n
    from (select (select value DBNO from CURD.refno where STYP = $db_type) as dbnos
          from MDB where NAME = $mdb)
    order by n desc limit 1)[0].dbnos ?? []"#;

/// Executes a query on the SUL_DB database to retrieve information from MDB and DB tables.
///
/// # Arguments
///
/// * `mdb` - The name of the MDB to query.
/// * `db_type` - The type of DB to filter by.
///
/// # Returns
///
/// The response containing the refno, noun, name, owner, and children_count fields from the query.
#[cached(result = true)]
pub async fn get_mdb_world_site_ele_nodes(
    mdb: String,
    module: DBType,
) -> anyhow::Result<Vec<EleTreeNode>> {
    let db_type: u8 = module.into();
    let sql = format!(
        r#"
        let $dbnos = {MDB_DESI_DBNOS};
        let $site_refnos = select value REFNO from SITE where REFNO.dbnum in $dbnos;
        let $worlds = select value id from (
            select REFNO.id as id, array::find_index($dbnos, REFNO.dbnum) as o
            from WORL where REFNO.dbnum in $dbnos order by o
        );
        return $site_refnos;
        select refno, noun, name, owner, array::len(select value in from <-pe_owner) as children_count
        from array::flatten(select value in from $worlds<-pe_owner);
        "#
    );
    let mut response = SUL_DB
        .query(&sql)
        .bind(("mdb", mdb.clone()))
        .bind(("db_type", db_type))
        .await?;
    let site_refnos: Vec<RefnoEnum> = response.take(3)?;
    let mut nodes: Vec<EleTreeNode> = response.take(4)?;
    nodes.retain(|node| node.noun == "SITE");
    if site_refnos.len() != nodes.len()
        || site_refnos
            .iter()
            .any(|refno| !nodes.iter().any(|node| node.refno == *refno))
    {
        let sql = format!(
            r#"
            let $dbnos = {MDB_DESI_DBNOS};
            select refno, noun, name, owner, array::len(select value in from <-pe_owner) as children_count
            from (select REFNO as id, array::find_index($dbnos, REFNO.dbnum) as o
                  from SITE where REFNO.dbnum in $dbnos order by o).id;
            "#
        );
        let mut response = SUL_DB
            .query(&sql)
            .bind(("mdb", mdb))
            .bind(("db_type", db_type))
            .await?;
        nodes = response.take(1)?;
    }
    for (i, node) in nodes.iter_mut().enumerate() {
        node.order = i as _;
        if node.name.is_empty() {
            node.name = format!("SITE {}", i + 1);
        }
    }
    //检查名称，如果没有给名字的，需要给上默认值, todo 后续如果是删除了又增加，名称后面的数字可能会继续增加
    Ok(nodes)
}

pub async fn create_mdb_world_site_pes_table(mdb: String, module: DBType) -> anyhow::Result<bool> {
    let db_type: u8 = module.into();
    let mut response = SUL_DB
        .query(format!(r#"
            let $dbnos = {MDB_DESI_DBNOS};
            let $a = (select value id from (select REFNO.id as id, array::find_index($dbnos, REFNO.dbnum) as o from WORL where REFNO.dbnum in $dbnos order by o));
            array::flatten(select value in.* from $a<-pe_owner[? in.noun='SITE'])
        "#))
        .bind(("mdb", mdb))
        .bind(("db_type", db_type))
        .await?;
    let sites: Vec<SPdmsElement> = response.take(2)?;
    if sites.is_empty() {
        return Ok(false);
    }
    let mut relate_sql = String::new();
    let mdb_world = sites[0].owner.to_pe_key();
    for (i, site) in sites.into_iter().enumerate() {
        relate_sql.push_str(&format!(
            "relate {}->site_relate:[{}, {i}]->{};",
            site.refno.to_pe_key(),
            &mdb_world,
            &mdb_world
        ));
    }

    Ok(true)
}

pub async fn query_type_refnos_by_dbnums(
    nouns: &[&str],
    dbnums: &[u32],
) -> anyhow::Result<Vec<RefnoEnum>> {
    let mut result = vec![];
    for noun in nouns {
        let sql = if dbnums.is_empty() {
            format!("select value id from {noun}")
        } else {
            format!(
                "select value id from {noun} where REFNO.dbnum in [{}]",
                dbnums.into_iter().map(|x| x.to_string()).join(",")
            )
        };
        let mut response = SUL_DB.query(&sql).await?;
        let refnos: Vec<RefnoEnum> = response.take(0)?;
        result.extend(refnos);
    }
    Ok(result)
}

///通过dbnum过滤指定类型的参考号
/// 通过has_children 指定是否需要有children，方便跳过一些不变要的节点
/// todo 在属性里直接加上DBNO这个属性，而不是需要去pe里去取
pub async fn query_type_refnos_by_dbnum(
    nouns: &[&str],
    dbnum: u32,
    has_children: Option<bool>,
    only_history: bool,
) -> anyhow::Result<Vec<RefnoEnum>> {
    let mut result = vec![];
    for noun in nouns {
        let table = if only_history {
            format!("{noun}_H")
        } else {
            format!("{noun}")
        };
        let sql = match has_children {
            Some(true) => {
                format!(
                    "select value id from {table} where REFNO.dbnum={dbnum} and (REFNO<-pe_owner.in)[0] != none"
                )
            }
            Some(false) => {
                format!(
                    "select value id from {table} where REFNO.dbnum={dbnum} and (REFNO<-pe_owner.in)[0] == none"
                )
            }
            None => {
                format!("select value id from {table} where REFNO.dbnum={dbnum}")
            }
        };
        // println!("query_type_refnos_by_dbnum sql: {}", sql);
        let mut response = SUL_DB.query(&sql).await?;
        let refnos: Vec<RefnoEnum> = response.take(0)?;
        result.extend(refnos);
    }
    Ok(result)
}

//额外检查SPRE  和 CATR 不能同时为空
pub async fn query_use_cate_refnos_by_dbnum(
    nouns: &[&str],
    dbnum: u32,
    only_history: bool,
) -> anyhow::Result<Vec<RefnoEnum>> {
    let mut result = vec![];
    for noun in nouns {
        let table = if only_history {
            format!("{noun}_H")
        } else {
            format!("{noun}")
        };
        let sql = format!(
            "select value id from {table} where REFNO.dbnum={dbnum} and (SPRE != none or CATR != none)"
        );
        let mut response = SUL_DB.query(&sql).await?;
        let refnos: Vec<RefnoEnum> = response.take(0)?;
        result.extend(refnos);
    }
    Ok(result)
}

//去掉父类型是BRAN 和 HANGER的
// pub async fn query_type_refnos_by_dbnum_exclude_bran_hang(nouns: &[&str], dbnum: u32) -> anyhow::Result<Vec<RefnoEnum>> {
//     let mut result = vec![];
//     for noun in nouns {
//         let sql = format!("select value id from {noun} where REFNO.dbnum={dbnum} and OWNER.noun not in ['BRAN', 'HANG']");
//         let mut response = SUL_DB.query(&sql).await?;
//         let refnos: Vec<RefnoEnum> = response.take(0)?;
//         result.extend(refnos);
//     }
//     Ok(result)
// }

#[cached(result = true)]
pub async fn query_mdb_db_nums(module: DBType) -> anyhow::Result<Vec<u32>> {
    let db_type: u8 = module.into();
    let mdb = &get_db_option().mdb_name;
    let mdb = crate::helper::to_e3d_name(mdb);
    // 与 `get_mdb_world_site_ele_nodes` 同源：库标识报的是模型树真正展示出来的那些库，
    // 而不是 MDB 声明了什么。MDB 里列着但一个元素都没同步过来的库不该出现在状态栏上。
    let mut response = SUL_DB
        .query(format!(r#"
            let $dbnos = {MDB_DESI_DBNOS};
            array::distinct(select value REFNO.dbnum from (select REFNO, array::find_index($dbnos, REFNO.dbnum) as o
                from SITE where REFNO.dbnum in $dbnos order by o));
        "#))
        .bind(("mdb", mdb))
        .bind(("db_type", db_type))
        .await?;
    let pe: Vec<u32> = response.take(1)?;
    Ok(pe)
}

///查询mdb的world下的所有pe
#[cached(result = true)]
pub async fn get_mdb_world_site_pes(
    mdb: String,
    module: DBType,
) -> anyhow::Result<Vec<SPdmsElement>> {
    let db_type: u8 = module.into();
    let mut response = SUL_DB
        .query(format!(r#"
            let $dbnos = {MDB_DESI_DBNOS};
            let $a = (select value id from (select REFNO.id as id, array::find_index($dbnos, REFNO.dbnum) as o from WORL where REFNO.dbnum in $dbnos order by o));
            array::flatten(select value in.* from $a<-pe_owner)[?noun = 'SITE']
        "#))
        .bind(("mdb", mdb))
        .bind(("db_type", db_type))
        .await?;
    let pe: Vec<SPdmsElement> = response.take(2)?;
    Ok(pe)
}

/// Represents the response obtained from the database query.
#[cached(result = true)]
pub async fn get_world(mdb: String) -> anyhow::Result<Option<SPdmsElement>> {
    let sql = format!(
        r#"
            let $dbnos = {MDB_DESI_DBNOS};
            (select value REFNO.* from (select REFNO, array::find_index($dbnos, REFNO.dbnum) as o
                from WORL where REFNO.dbnum in $dbnos and REFNO.noun='WORL' order by o) limit 1)[0]
        "#
    );
    let mut response = SUL_DB
        .query(sql)
        .bind(("mdb", mdb))
        .bind(("db_type", 1u8))
        .await?;
    let pe: Option<SPdmsElement> = response.take(1)?;
    Ok(pe)
}

/// Represents the response obtained from the database query.
#[cached(result = true)]
pub async fn get_world_refno(mdb: String) -> anyhow::Result<RefnoEnum> {
    let mdb_name = if mdb.starts_with('/') {
        mdb.clone()
    } else {
        format!("/{}", mdb)
    };
    // 顺延：在 MDB 的 DESI 库里按 CURD 顺序取**第一个真的有 WORL 的**，而不是死认
    // `$dbnos[0]`——列在前面的库未必解析过，那种情况下旧写法直接返回空世界。
    let sql = format!(
        r#"
            let $dbnos = {MDB_DESI_DBNOS};
            (select value REFNO from (select REFNO, array::find_index($dbnos, REFNO.dbnum) as o
                from WORL where REFNO.dbnum in $dbnos and REFNO.noun='WORL' order by o) limit 1)[0]
        "#
    );
    let mut response = SUL_DB
        .query(sql)
        .bind(("mdb", mdb_name))
        .bind(("db_type", 1u8))
        .await?;
    let id: Option<RefnoEnum> = response.take(1)?;
    Ok(id.unwrap_or_default())
}
