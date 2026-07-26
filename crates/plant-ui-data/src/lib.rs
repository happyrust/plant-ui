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

/// 连接后的工程标识：项目名、SurrealDB 命名空间、当前 MDB（DESI）库编号列表。
/// 外壳原则是不摆假数字，这些都取自真实配置与真实查询。
pub async fn project_identity() -> Result<(String, String, Vec<u32>)> {
    let opt = aios_core::get_db_option();
    let db_nums = aios_core::query_mdb_db_nums(DBType::DESI).await?;
    Ok((opt.project_name.clone(), opt.surreal_ns.clone(), db_nums))
}
