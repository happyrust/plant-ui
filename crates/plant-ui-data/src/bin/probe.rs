//! M0-4 验收探针：直连本地 SurrealDB，读一批元素 refno 与名称并打印。
//! 运行：cargo run -p plant-ui-data --bin probe（工作目录需有 DbOption.toml）

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    plant_ui_data::connect().await?;
    let rows = plant_ui_data::sample_named_elements(20).await?;
    println!("AvevaMarineSample (ns 1516) -> {} rows", rows.len());
    for (refno, name, noun) in &rows {
        println!("  {refno:<16} {noun:<6} {name}");
    }
    Ok(())
}