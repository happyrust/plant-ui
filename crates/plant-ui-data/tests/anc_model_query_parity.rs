//! P2 对拍验收（层级查询优化方案，gen-model
//! `docs/plans/2026-08-07-inst-relate-anc-u64-hierarchy-query-plan.md`）：
//! 配置库（AMS 等）上，新路径（`model_instances_anc`，anc CONTAINS 索引查询）
//! 与旧路径（`model_instances_legacy`，深遍历解析）的 refno 集合必须完全一致。
//!
//! 前置：gen-model 新版已对该库启动过一次（索引 + anc 回填自愈完成）。
//! 运行：`cargo test -p plant-ui-data --test anc_model_query_parity -- --ignored --nocapture`
//!
//! 已知的合法差异形态（AMS 上预期为空，真出现会在失败输出里逐条列出）：
//! SITE 根下不挂任何 ZONE 的可见元素——旧路径 SITE→ZONE 中转会漏掉它们，
//! 新路径按祖先链正确包含。出现这类差异时按「新路径为准」人工裁定。

use std::collections::BTreeMap;

/// 一个实例的对拍身份：refno + 排序后的网格 hash 集（网格集合不同也算不一致）。
fn identity_map(
    models: &[aios_core::GeomInstQuery],
) -> BTreeMap<u64, Vec<String>> {
    let mut map: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for model in models {
        let entry = map.entry(model.refno.refno().0).or_default();
        for inst in &model.insts {
            entry.push(inst.geo_hash.clone());
        }
    }
    for hashes in map.values_mut() {
        hashes.sort();
    }
    map
}

#[tokio::test]
#[ignore = "manual live: needs configured SurrealDB with gen-model anc backfill done"]
async fn anc_and_legacy_model_paths_agree() {
    // rs-core 的 `try_get_db_option` 按进程 CWD 找 `DbOption.toml`，先站到工作区根。
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("无法切换到工作区目录");
    plant_ui_data::connect().await.expect("连接配置库");

    assert!(
        aios_core::inst_relate_anc_ready().await.expect("anc 覆盖探测"),
        "inst_relate.anc 未回填：先用新版 gen-model 对该库启动一次（自愈回填）再跑对拍"
    );

    let sites = plant_ui_data::site_nodes().await.expect("SITE 根");
    let roots: Vec<plant_ui_data::RefU64> =
        sites.iter().map(|node| node.refno.refno()).collect();
    assert!(!roots.is_empty(), "当前 MDB 下没有 SITE 根");
    println!("[parity] {} 个 SITE 根", roots.len());

    let started = std::time::Instant::now();
    let legacy = plant_ui_data::model_instances_legacy(&roots, |_, _| {})
        .await
        .expect("旧路径");
    let legacy_elapsed = started.elapsed();

    plant_ui_data::invalidate_all().await;

    let started = std::time::Instant::now();
    let anc = plant_ui_data::model_instances_anc(&roots, |_, _| {})
        .await
        .expect("新路径");
    let anc_elapsed = started.elapsed();

    println!(
        "[parity] 旧路径 {} 实例 / {legacy_elapsed:?}；新路径 {} 实例 / {anc_elapsed:?}",
        legacy.len(),
        anc.len()
    );

    let legacy_map = identity_map(&legacy);
    let anc_map = identity_map(&anc);
    let missing: Vec<_> = legacy_map
        .iter()
        .filter(|(refno, _)| !anc_map.contains_key(*refno))
        .map(|(refno, _)| plant_ui_data::RefU64(*refno).to_string())
        .collect();
    let extra: Vec<_> = anc_map
        .iter()
        .filter(|(refno, _)| !legacy_map.contains_key(*refno))
        .map(|(refno, _)| plant_ui_data::RefU64(*refno).to_string())
        .collect();
    let hash_drift: Vec<_> = legacy_map
        .iter()
        .filter_map(|(refno, hashes)| {
            anc_map
                .get(refno)
                .filter(|anc_hashes| *anc_hashes != hashes)
                .map(|anc_hashes| {
                    format!(
                        "{}: legacy {hashes:?} != anc {anc_hashes:?}",
                        plant_ui_data::RefU64(*refno)
                    )
                })
        })
        .collect();

    assert!(
        missing.is_empty() && extra.is_empty() && hash_drift.is_empty(),
        "新旧路径不一致：\n旧有新无（{}）: {missing:?}\n新有旧无（{}）: {extra:?}\n网格集漂移（{}）: {hash_drift:?}",
        missing.len(),
        extra.len(),
        hash_drift.len()
    );
    println!("[parity] refno 集合与网格集完全一致（{} 实例）", anc_map.len());
}
