//! plant-ui-data：SUL_DB 查询 -> Vm，不含任何绘制。
//! 决定 3/7：不造 DTO，直接用 aios_core 类型；直连本地 SurrealDB，不做 mock。

use anyhow::Result;

/// 连接本地 SurrealDB（读取工作目录的 DbOption.toml，走 aios_core 全局句柄 SUL_DB）。
pub async fn connect() -> Result<()> {
    aios_core::init_surreal().await
}