use crate::aios_db_mgr::aios_mgr::AiosDBMgr;
use crate::basic::aabb::ParryAabb;
use crate::pdms_types::PdmsGenericType;
use crate::{RefU64, RefnoEnum, SUL_DB, get_inst_relate_keys};
use bevy_transform::components::Transform;
use chrono::{DateTime, Local, NaiveDateTime};
use glam::{DVec3, Vec3};
use parry3d::bounding_volume::Aabb;
use serde_derive::{Deserialize, Serialize};
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize, Deserialize, Debug)]
pub struct TubiInstQuery {
    #[serde(alias = "id")]
    pub refno: RefnoEnum,
    pub old_refno: Option<RefnoEnum>,
    pub generic: Option<String>,
    pub world_aabb: Aabb,
    pub world_trans: Transform,
    pub geo_hash: String,
    pub date: Option<surrealdb::sql::Datetime>,
}

pub async fn query_tubi_insts_by_brans(
    bran_refnos: &[RefnoEnum],
) -> anyhow::Result<Vec<TubiInstQuery>> {
    let pes: String = bran_refnos
        .iter()
        .map(|x| x.to_pe_key())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
             select
                in.id as refno,
                in.old_pe as old_refno,
                in.owner.noun as generic, aabb.d as world_aabb, world_trans.d as world_trans,
                record::id(out) as geo_hash,
                fn::ses_date(in.id) as date
             from  array::flatten([{}]->tubi_relate) where leave.id != none and aabb.d != none
             "#,
        pes
    );
    // println!("Query tubi insts: {}", &sql);
    let mut response = SUL_DB.query(&sql).await?;
    // dbg!(&response);

    let r = response.take::<Vec<TubiInstQuery>>(0)?;
    Ok(r)
}

pub async fn query_tubi_insts_by_flow(refnos: &[RefnoEnum]) -> anyhow::Result<Vec<TubiInstQuery>> {
    let pes: String = refnos
        .iter()
        .map(|x| x.to_pe_key())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        r#"
        array::group(array::complement(select value
        (select in.id as refno, in.owner.noun as generic, aabb.d as world_aabb, world_trans.d as world_trans, record::id(out) as geo_hash,
            fn::ses_date(in.id) as date
            from tubi_relate where leave=$parent.id or arrive=$parent.id)
                from [{}] where in.id != none and  owner.noun in ['BRAN', 'HANG'], [none]))
             "#,
        pes
    );
    // println!("Sql query_tubi_insts_by_flow: {}", &sql);
    let mut response = SUL_DB.query(sql).await?;

    let r = response.take::<Vec<TubiInstQuery>>(0)?;
    Ok(r)
}

#[serde_as]
#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ModelHashInst {
    pub geo_hash: String,
    #[serde(default)]
    pub transform: Transform,
    #[serde(default)]
    pub is_tubi: bool,
}

#[derive(Debug)]
pub struct ModelInstData {
    pub owner: RefnoEnum,
    pub old_refno: Option<RefnoEnum>,
    pub has_neg: bool,
    pub insts: Vec<ModelHashInst>,
    pub generic: PdmsGenericType,
    pub world_trans: Transform,
    pub world_aabb: ParryAabb,
    pub ptset: Vec<Vec3>,
    pub is_bran_tubi: bool,
    pub date: NaiveDateTime,
}

///
/// 几何实例查询结构体
#[derive(Serialize, Deserialize, Debug)]
pub struct GeomInstQuery {
    /// 构件编号，别名为id
    #[serde(alias = "id")]
    pub refno: RefnoEnum,
    /// 历史构件编号
    pub old_refno: Option<RefnoEnum>,
    /// 所属构件编号
    pub owner: RefnoEnum,
    /// 世界坐标系下的包围盒
    pub world_aabb: Aabb,
    /// 世界坐标系下的变换矩阵
    pub world_trans: Transform,
    /// 几何实例列表
    pub insts: Vec<ModelHashInst>,
    /// 是否包含负实体
    pub has_neg: bool,
    /// 构件类型
    pub generic: String,
    /// 点集数据
    pub pts: Option<Vec<Vec3>>,
    /// 时间戳
    pub date: Option<surrealdb::sql::Datetime>,
}

/// 几何点集查询结构体
#[derive(Serialize, Deserialize, Debug)]
pub struct GeomPtsQuery {
    /// 构件编号，别名为id
    #[serde(alias = "id")]
    pub refno: RefnoEnum,
    /// 世界坐标系下的变换矩阵
    pub world_trans: Transform,
    /// 世界坐标系下的包围盒
    pub world_aabb: Aabb,
    /// 点集组，每组包含一个变换矩阵和可选的点集数据
    pub pts_group: Vec<(Transform, Option<Vec<DVec3>>)>,
}

/// 根据最新refno查询最新insts
/// 根据构件编号查询几何实例信息
///
/// # 参数
///
/// * `refnos` - 构件编号迭代器
/// * `enable_holes` - 是否启用孔洞查询
///
/// # 返回值
///
/// 返回几何实例查询结果的向量
pub async fn query_insts(
    refnos: impl IntoIterator<Item = &RefnoEnum>,
    enable_holes: bool,
) -> anyhow::Result<Vec<GeomInstQuery>> {
    let refnos = refnos.into_iter().cloned().collect::<Vec<_>>();

    //需要区分历史模型和当前最新模型

    let inst_keys = get_inst_relate_keys(&refnos);

    let sql = if enable_holes {
        format!(
            r#"
            select
                in.id as refno,
                in.old_pe as old_refno,
                in.owner as owner, generic, aabb.d as world_aabb, world_trans.d as world_trans, out.ptset.d.pt as pts,
                if booled_id != none {{ [{{ "geo_hash": booled_id }}] }} else {{ (select trans.d as transform, record::id(out) as geo_hash from out->geo_relate where visible && out.meshed && trans.d != none && geo_type='Pos')  }} as insts,
                booled_id != none as has_neg,
                dt as date
            from {inst_keys} where aabb.d != none
        "#
        )
    } else {
        format!(
            r#"
            select
                in.id as refno,
                in.old_pe as old_refno,
                in.owner as owner, generic, aabb.d as world_aabb, world_trans.d as world_trans, out.ptset.d.pt as pts,
                (select trans.d as transform, record::id(out) as geo_hash from out->geo_relate where visible && out.meshed && trans.d != none && geo_type='Pos') as insts,
                booled_id != none as has_neg,
                dt as date
            from {inst_keys} where aabb.d != none "#
        )
    };
    // println!("Query insts sql: {}", &sql);
    let mut response = SUL_DB.query(sql).await?;
    let mut geom_insts: Vec<GeomInstQuery> = response.take(0)?;
    // dbg!(&geom_insts);

    Ok(geom_insts)
}

/// [`query_insts`] 的热路径瘦身版（层级查询优化 P2+）：只投影 UI 真正消费的
/// 字段——refno（直接取边上的 `in` 链接，**不解引用 pe 行**）、generic、anc、
/// `aabb.d` / `world_trans.d`、insts 子查询。owner 从写入时物化的 `anc[1]`
/// 还原（省一次 pe 解引用）；old_refno / pts / dt / has_neg 一律缺省——
/// plant-ui 全链路无消费者，要这些字段的调用方仍走 [`query_insts`]。
///
/// AMS 实库整表探针（53,582 行）：旧全投影 ~14.1s，本投影 ~11.1s。省下的是
/// 每行 3 次 pe 解引用（in.id / in.old_pe / in.owner）与 ptset 链走读；剩余
/// 大头是 insts 子查询（~8.7s，图跳 + 每边 trans/meshed 解引用）与 aabb/trans
/// 解引用（~2.0s），那两档只有写时物化能消掉（列为后续项）。
pub async fn query_insts_slim(
    refnos: impl IntoIterator<Item = &RefnoEnum>,
) -> anyhow::Result<Vec<GeomInstQuery>> {
    #[derive(Deserialize)]
    struct SlimInstRow {
        refno: RefnoEnum,
        anc: Option<Vec<u64>>,
        generic: Option<String>,
        world_aabb: Aabb,
        world_trans: Transform,
        insts: Vec<ModelHashInst>,
    }
    let refnos = refnos.into_iter().cloned().collect::<Vec<_>>();
    let inst_keys = get_inst_relate_keys(&refnos);
    let sql = format!(
        r#"
        select
            in as refno, anc, generic, aabb.d as world_aabb, world_trans.d as world_trans,
            (select trans.d as transform, record::id(out) as geo_hash from out->geo_relate where visible && out.meshed && trans.d != none && geo_type='Pos') as insts
        from {inst_keys} where aabb.d != none "#
    );
    let mut response = SUL_DB.query(sql).await?;
    let rows: Vec<SlimInstRow> = response.take(0)?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let owner = row
                .anc
                .as_ref()
                .and_then(|anc| anc.get(1).copied())
                .map(|packed| RefnoEnum::from(RefU64(packed)))
                .unwrap_or(row.refno);
            GeomInstQuery {
                refno: row.refno,
                old_refno: None,
                owner,
                world_aabb: row.world_aabb,
                world_trans: row.world_trans,
                insts: row.insts,
                has_neg: false,
                generic: row.generic.unwrap_or_default(),
                pts: None,
                date: None,
            }
        })
        .collect())
}

/// 任意根子树的全部可见实例 refno：`inst_relate` 上一条 `anc CONTAINS $root`
/// 索引查询（层级查询优化方案 P2，gen-model
/// `docs/plans/2026-08-07-inst-relate-anc-u64-hierarchy-query-plan.md`）。
///
/// 替代「`query_deep_visible_inst_refnos`：两遍 12 层 `<-pe_owner<-` 深遍历 +
/// 巨型 IN 内联」的旧解析形态。`anc` 是 gen-model 写入侧物化的 RefU64 打包
/// 祖先链（含自身，向上到顶），配普通索引 `idx_inst_relate_anc`；根可以是
/// SITE/ZONE/PIPE/BRAN 乃至叶子元素本身，无需先辨类型。
///
/// **刻意只回 id 列表**：投影仍走既有的分批 [`query_insts`]（500/批）。整根
/// 全投影一条响应在大 SITE 上会撑爆单条 WS 消息（AMS 41 根实测直接把连接
/// 打死），id 列表则再大的根也只有百 KB 级。
///
/// 回填完成前的旧行 `anc = NONE`，CONTAINS 天然不命中——调用方先用
/// [`inst_relate_anc_ready`] 探测覆盖，再决定走本函数还是旧路径。
pub async fn query_inst_refnos_by_root_anc(root: RefnoEnum) -> anyhow::Result<Vec<RefnoEnum>> {
    let root_u64 = root.refno().0;
    let mut response = SUL_DB
        .query(format!(
            "select value in.id from inst_relate where anc contains {root_u64} and aabb.d != none"
        ))
        .await?;
    Ok(response.take(0)?)
}

/// 任意根子树里**带直管段**的 BRAN/HANG refno：`tubi_relate` 上同款
/// `anc CONTAINS $root` 索引查询（与 [`query_inst_refnos_by_root_anc`] 配对）。
/// 替代「深遍历过滤子树全部 BRAN/HANG」的旧解析形态；边投影仍走既有的
/// [`query_tubi_insts_by_brans`]（500 支管/批）。`tubi_relate.anc` 由 gen-model
/// 建边时按所属 BRAN 的祖先链写入。
pub async fn query_bran_refnos_by_root_anc(root: RefnoEnum) -> anyhow::Result<Vec<RefnoEnum>> {
    let root_u64 = root.refno().0;
    let mut response = SUL_DB
        .query(format!(
            "return array::distinct((select value in.id from tubi_relate \
             where anc contains {root_u64} and leave.id != none));"
        ))
        .await?;
    Ok(response.take(0)?)
}

/// `anc` 回填覆盖探测：库里是否已有带祖先链的 `inst_relate` 行。
///
/// gen-model 的启动序列会对存量行做幂等回填（`backfill_inst_relate_anc`）；
/// 在那之前 anc 全为 NONE，`anc CONTAINS` 查什么都是空集。读侧在选路前
/// 探一次（`LIMIT 1`，最坏整表扫一遍 id，3.8 万行毫秒级），空库（0 行）视作
/// 就绪——两条路径都只能回空，走新路径省掉深遍历。
pub async fn inst_relate_anc_ready() -> anyhow::Result<bool> {
    let sql = "return array::len((select value id from inst_relate limit 1)) == 0 \
               || array::len((select value id from inst_relate where anc != none limit 1)) > 0;";
    let mut response = SUL_DB.query(sql).await?;
    let ready: Option<bool> = response.take(0)?;
    Ok(ready.unwrap_or(false))
}

// 根据历史refno查询历史insts
// pub async fn query_history_insts(
//     refnos: impl IntoIterator<Item = &RefnoEnum>,
// ) -> anyhow::Result<Vec<GeomInstQuery>> {
//     let refnos = refnos.into_iter().cloned().collect::<Vec<_>>();

//     //需要区分历史模型和当前最新模型

//     let inst_keys = get_inst_relate_keys(&refnos);

//     let sql = format!(
//         r#"
//             select
//                 in.id as refno,
//                 in.old_pe as old_refno,
//                 in.owner as owner, generic, aabb.d as world_aabb, world_trans.d as world_trans, out.ptset.d.pt as pts,
//                 if booled_id != none {{ [{{ "geo_hash": booled_id }}] }} else {{ (select trans.d as transform, record::id(out) as geo_hash from out->geo_relate where visible && out.meshed && trans.d != none && geo_type='Pos')  }} as insts,
//                 fn::ses_date(in.id) as date
//             from {inst_keys} where aabb.d != none
//         "#
//     );
//     // println!("Query insts sql: {}", &sql);
//     let mut response = SUL_DB.query(sql).await?;
//     let mut geom_insts: Vec<GeomInstQuery> = response.take(0)?;
//     // dbg!(&geom_insts);

//     Ok(geom_insts)
// }

// todo 生成一个测试案例
// pub async fn query_history_insts(
//     refnos: impl IntoIterator<Item = &(RefnoEnum, u32)>,
// ) -> anyhow::Result<Vec<GeomInstQuery>> {
//     let history_inst_keys = refnos
//         .into_iter()
//         .map(|x| format!("inst_relate:{}_{}", x.0, x.1))
//         .collect::<Vec<_>>()
//         .join(",");

//     //todo 如果是ngmr relate, 也要测试一下有没有问题
//     //ngmr relate 的关系可以直接在inst boolean 做这个处理，不需要单独开方法
//     //ngmr的负实体最后再执行
//     let sql = format!(
//         r#"
//     select in.id as refno, in.owner as owner, generic, aabb.d as world_aabb, world_trans.d as world_trans, out.ptset.d.pt as pts,
//             if (in<-neg_relate)[0] != none && $parent.booled {{ [{{ "geo_hash": record::id(in.id) }}] }} else {{ (select trans.d as transform, record::id(out) as geo_hash from out->geo_relate where visible && trans.d != none && geo_type='Pos')  }} as insts
//             from {history_inst_keys} where aabb.d != none
//             "#
//     );
//     // println!("Query insts: {}", &sql);
//     let mut response = SUL_DB.query(sql).await?;
//     let mut geom_insts: Vec<GeomInstQuery> = response.take(0).unwrap();

//     Ok(geom_insts)
// }

/// 根据区域编号查询几何实例信息
///
/// # 参数
///
/// * `refnos` - 区域编号迭代器
/// * `enable_holes` - 是否启用孔洞查询
///
/// # 返回值
///
/// 返回几何实例查询结果的向量
pub async fn query_insts_by_zone(
    refnos: impl IntoIterator<Item = &RefnoEnum>,
    enable_holes: bool,
) -> anyhow::Result<Vec<GeomInstQuery>> {
    let zone_refnos = refnos
        .into_iter()
        .map(RefnoEnum::to_pe_key)
        .collect::<Vec<_>>()
        .join(",");

    let sql = if enable_holes {
        format!(
            r#"
            select
                in.id as refno,
                in.old_pe as old_refno,
                in.owner as owner, generic, aabb.d as world_aabb, world_trans.d as world_trans, out.ptset.d.pt as pts,
                if booled_id != none {{ [{{ "geo_hash": booled_id }}] }} else {{ (select trans.d as transform, record::id(out) as geo_hash from out->geo_relate where visible && out.meshed && trans.d != none && geo_type='Pos')  }} as insts,
                booled_id != none as has_neg,
                fn::ses_date(in.id) as date
            from inst_relate where zone_refno in [{}] and aabb.d != none
            "#,
            zone_refnos
        )
    } else {
        format!(
            r#"
            select
                in.id as refno,
                in.old_pe as old_refno,
                in.owner as owner, generic, aabb.d as world_aabb, world_trans.d as world_trans, out.ptset.d.pt as pts,
                (select trans.d as transform, record::id(out) as geo_hash from out->geo_relate where visible && out.meshed && trans.d != none && geo_type='Pos') as insts,
                booled_id != none as has_neg,
                fn::ses_date(in.id) as date
            from inst_relate where zone_refno in [{}] and aabb.d != none
            "#,
            zone_refnos
        )
    };

    println!("Query insts by zone sql: {}", &sql);

    let mut response = SUL_DB.query(sql).await?;
    let geom_insts: Vec<GeomInstQuery> = response.take(0)?;

    Ok(geom_insts)
}

pub async fn query_inst_refnos_by_zone(
    refnos: impl IntoIterator<Item = &RefnoEnum>,
) -> anyhow::Result<Vec<RefnoEnum>> {
    let zone_refnos = refnos
        .into_iter()
        .map(RefnoEnum::to_pe_key)
        .collect::<Vec<_>>()
        .join(",");
    let mut response = SUL_DB
        .query(format!(
            "select value in.id from inst_relate where zone_refno in [{zone_refnos}] and aabb.d != none"
        ))
        .await?;
    Ok(response.take(0)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RefnoEnum, init_test_surreal};

    #[tokio::test]
    async fn test_query_insts() -> anyhow::Result<()> {
        init_test_surreal().await;
        // Test case 1: Query single refno
        let refnos = vec!["17496/496442".into()];
        let result = query_insts(&refnos, false).await?;
        assert!(!result.is_empty(), "Should return at least one instance");
        dbg!(&result);

        // Verify returned instance has expected fields
        let first_inst = &result[0];
        // assert!(
        //     first_inst.world_aabb.is_some(),
        //     "World AABB should be present"
        // );
        // assert!(
        //     first_inst.world_trans.is_some(),
        //     "World transform should be present"
        // );
        // assert!(
        //     !first_inst.insts.is_empty(),
        //     "Should have geometry instances"
        // );

        assert!(
            first_inst.has_neg == true,
            "Should not have negative geometry"
        );

        // Test case 2: Query multiple refnos
        // let refnos = vec![RefnoEnum::Pe(24383_84088), RefnoEnum::Pe(24383_84089)];
        // let result = query_insts(&refnos).await?;
        // assert!(result.len() >= 2, "Should return multiple instances");

        // // Test case 3: Query non-existent refno
        // let refnos = vec![RefnoEnum::Pe(0)];
        // let result = query_insts(&refnos).await?;
        // assert!(
        //     result.is_empty(),
        //     "Should return empty for non-existent refno"
        // );

        Ok(())
    }

    #[tokio::test]
    async fn test_query_insts_by_zone() -> anyhow::Result<()> {
        init_test_surreal().await;

        // Test case: Query instances by zone
        let zone_refnos = vec!["24383_66457".into()];
        let result = query_insts_by_zone(&zone_refnos, false).await?;

        // Verify the results
        assert!(!result.is_empty(), "Should return instances for the zone");

        // Check the first instance has all required fields
        if let Some(first_inst) = result.first() {
            assert!(
                first_inst.refno.to_string().len() > 0,
                "Should have valid refno"
            );
            assert!(first_inst.insts.len() > 0, "Should have geometry instances");
        }

        Ok(())
    }
}
