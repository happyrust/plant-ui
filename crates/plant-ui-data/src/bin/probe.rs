//! 数据层验收探针：直连本地 SurrealDB，覆盖 M0-4 采样与 M1-2 模型树查询。
//! 运行：cargo run -p plant-ui-data --bin probe（工作目录需有 DbOption.toml）

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    plant_ui_data::connect().await?;

    let (project, ns, db_nums) = plant_ui_data::project_identity().await?;
    println!("project={project} ns={ns} desi_dbs={db_nums:?}");

    let rows = plant_ui_data::sample_named_elements(20).await?;
    println!("sample_named_elements -> {} rows", rows.len());
    for (refno, name, noun) in rows.iter().take(5) {
        println!("  {refno:<16} {noun:<6} {name}");
    }

    let sites = plant_ui_data::site_nodes().await?;
    println!("site_nodes -> {} rows", sites.len());
    for s in &sites {
        println!(
            "  {:<12} {:<6} {:<20} children={}",
            s.refno.refno().to_string(),
            s.noun,
            s.name,
            s.children_count
        );
    }

    if let Some(first) = sites.first() {
        let kids = plant_ui_data::child_nodes(first.refno).await?;
        println!("child_nodes({}) -> {} rows", first.name, kids.len());
        for k in kids.iter().take(5) {
            println!(
                "  {:<12} {:<6} {:<20} order={} children={}",
                k.refno.refno().to_string(),
                k.noun,
                k.name,
                k.order,
                k.children_count
            );
        }
    }
    Ok(())
}
