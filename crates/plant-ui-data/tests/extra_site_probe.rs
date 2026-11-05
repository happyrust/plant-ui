//! 真库探针：`extra_element_dbnums` 补充库的 SITE 要出现在模型树根层（gen-model ADR-095）。
//!
//! 运行：`cargo test -p plant-ui-data --test extra_site_probe -- --ignored --nocapture`
//!（工作目录需有带 `extra_element_dbnums = [251095, 251137]` 的 DbOption.toml，SurrealDB 8009 在跑）。

#[tokio::test(flavor = "multi_thread")]
#[ignore = "需要本地 SurrealDB 与带 extra_element_dbnums 的 DbOption.toml"]
async fn extra_dbnum_sites_join_the_model_tree_roots() -> anyhow::Result<()> {
    // cargo 给集成测试的 CWD 是 crate 目录，而 DbOption.toml 在 workspace 根（同 spooling_probe）。
    std::env::set_current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))?;
    plant_ui_data::connect().await?;
    let extra = plant_ui_data::extra_element_dbnums();
    anyhow::ensure!(
        extra.contains(&251095) && extra.contains(&251137),
        "DbOption.toml 没配补充名单：{extra:?}"
    );
    let nodes = plant_ui_data::site_nodes().await?;
    let names: Vec<&str> = nodes.iter().map(|node| node.name.as_str()).collect();
    println!("SITE_ROOTS count={} tail={:?}", nodes.len(), &names[names.len().saturating_sub(4)..]);
    for wanted in ["/PIPE_4DK_FWD", "/HVAC_2DK_FWD"] {
        assert!(names.contains(&wanted), "{wanted} 不在模型树根层：{names:?}");
    }
    let mut refnos: Vec<_> = nodes.iter().map(|node| node.refno.refno()).collect();
    let total = refnos.len();
    refnos.sort();
    refnos.dedup();
    assert_eq!(refnos.len(), total, "SITE 不许重复");
    assert!(
        nodes.iter().enumerate().all(|(i, node)| node.order as usize == i),
        "order 要按最终位置重排"
    );
    Ok(())
}
