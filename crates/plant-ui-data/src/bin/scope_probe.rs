//! 排障探针：进程内跑与「显示模型」同一条 `model_instances` 路径，打印完整错误链。
//! 运行：cargo run -p plant-ui-data --bin scope_probe -- <packed_refno>
//! （工作目录需有 DbOption.toml；对隔离库用 DB_OPTION_FILE 指到对应配置。）

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    plant_ui_data::connect().await?;
    let packed: u64 = std::env::args()
        .nth(1)
        .expect("要一个 packed refno 参数")
        .parse()?;
    let root = aios_core::RefU64(packed);
    println!("scope query for packed={packed} refno={}", {
        let refno: aios_core::RefnoEnum = root.into();
        refno.refno().to_string()
    });
    match plant_ui_data::model_instances(&[root]).await {
        Ok(models) => {
            println!("OK: {} 条几何实例", models.len());
            for model in models.iter().take(20) {
                println!(
                    "  refno={} generic={:?} insts={} aabb_min={:?}",
                    model.refno.refno().to_string(),
                    model.generic,
                    model.insts.len(),
                    model.world_aabb.mins,
                );
            }
        }
        Err(error) => {
            println!("ERR: {error:#}");
            for (depth, cause) in error.chain().enumerate() {
                println!("  [{depth}] {cause}");
            }
        }
    }
    Ok(())
}
