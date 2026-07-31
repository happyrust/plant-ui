/// 截图里的真实回归：ZONE 范围不能只返回管件，也必须包含 BRAN/HANG 的隐含直管段。
///
/// 前置条件：`PLANT_TEST_WORKSPACE` 指向带 DbOption.toml 的运行目录。
#[tokio::test]
#[ignore = "需要目标运行环境的 SurrealDB 与 DbOption.toml，用 --ignored 显式跑"]
async fn zone_scope_includes_implicit_straight_tubes() {
    let workspace = std::env::var_os("PLANT_TEST_WORKSPACE").expect("PLANT_TEST_WORKSPACE 未设置");
    std::env::set_current_dir(workspace).expect("无法切换到目标运行目录");
    plant_ui_data::connect().await.unwrap();

    let zone = "24383_66457".parse::<plant_ui_data::RefU64>().unwrap();
    let models = plant_ui_data::model_instances(&[zone]).await.unwrap();
    let tubi_count = models
        .iter()
        .filter(|model| model.insts.iter().any(|inst| inst.is_tubi))
        .count();
    let tubi_instances = models
        .iter()
        .flat_map(|model| &model.insts)
        .filter(|inst| inst.is_tubi)
        .count();
    let tubi_refnos = models
        .iter()
        .filter(|model| model.insts.iter().any(|inst| inst.is_tubi))
        .map(|model| model.refno)
        .collect::<std::collections::HashSet<_>>()
        .len();
    for model in models
        .iter()
        .filter(|model| model.insts.iter().any(|inst| inst.is_tubi))
    {
        assert!(model.world_trans.translation.is_finite());
        assert!(model.world_trans.scale.is_finite());
        assert!(model.world_trans.scale.length_squared() > 0.0);
        for inst in &model.insts {
            let path = format!("assets/meshes/{}.mesh", inst.geo_hash);
            assert!(
                std::path::Path::new(&path).is_file(),
                "缺少直段网格：{path}"
            );
        }
    }
    println!(
        "ZONE {zone}: {} models, {tubi_count} TUBI models, \
         {tubi_instances} TUBI instances across {tubi_refnos} refnos",
        models.len()
    );

    assert!(
        tubi_count > 0,
        "ZONE 返回了 {} 个模型，但没有任何隐含直管段",
        models.len()
    );
    assert!(
        tubi_instances > tubi_refnos,
        "真实 ZONE 应覆盖同一 BRAN 下有多段 TUBI 的回归场景"
    );
}
