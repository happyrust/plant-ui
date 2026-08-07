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

/// 一个实例的对拍身份：refno → (owner, 排序后的网格 hash 集)。
///
/// owner 也纳入对拍：新路径的 owner 由写入时物化的 `anc[1]` 还原，旧路径是
/// 实时 `in.owner` 解引用——两者不一致即说明 anc 链已陈旧（搬家维护漏修）。
fn identity_map(
    models: &[aios_core::GeomInstQuery],
) -> BTreeMap<u64, (u64, Vec<String>)> {
    let mut map: BTreeMap<u64, (u64, Vec<String>)> = BTreeMap::new();
    for model in models {
        let entry = map
            .entry(model.refno.refno().0)
            .or_insert_with(|| (model.owner.refno().0, Vec::new()));
        for inst in &model.insts {
            entry.1.push(inst.geo_hash.clone());
        }
    }
    for (_, hashes) in map.values_mut() {
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

/// 只跑新路径的计时入口（P4 调优迭代用，整路两轮 + 分阶段净成本分解；
/// 对拍判据在上面那条）。
#[tokio::test]
#[ignore = "manual live: timing-only run of the anc path"]
async fn anc_path_timing_only() {
    use futures::stream::StreamExt;
    std::env::set_current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect("无法切换到工作区目录");
    plant_ui_data::connect().await.expect("连接配置库");
    let sites = plant_ui_data::site_nodes().await.expect("SITE 根");
    let roots: Vec<plant_ui_data::RefU64> =
        sites.iter().map(|node| node.refno.refno()).collect();
    println!("[timing] {} 个 SITE 根", roots.len());
    for round in 1..=2 {
        plant_ui_data::invalidate_all().await;
        let started = std::time::Instant::now();
        let anc = plant_ui_data::model_instances_anc(&roots, |_, _| {})
            .await
            .expect("新路径");
        println!(
            "[timing] 第 {round} 轮：{} 实例 / {:?}",
            anc.len(),
            started.elapsed()
        );
    }

    // ── 分阶段净成本（顺序执行，无根间并发；wall 会小于各阶段之和）──
    let started = std::time::Instant::now();
    let resolved: Vec<(Vec<aios_core::RefnoEnum>, Vec<aios_core::RefnoEnum>)> =
        futures::stream::iter(roots.iter().copied().map(|root| async move {
            let root: aios_core::RefnoEnum = root.into();
            futures::future::try_join(
                aios_core::query_inst_refnos_by_root_anc(root),
                aios_core::query_bran_refnos_by_root_anc(root),
            )
            .await
        }))
        .buffered(8)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<_, _>>()
        .expect("解析");
    let resolve_elapsed = started.elapsed();
    let inst_refnos: Vec<aios_core::RefnoEnum> =
        resolved.iter().flat_map(|(insts, _)| insts.iter().copied()).collect();
    let bran_refnos: Vec<aios_core::RefnoEnum> =
        resolved.iter().flat_map(|(_, brans)| brans.iter().copied()).collect();
    println!(
        "[stage] 解析(8路): {resolve_elapsed:?}，inst {} / bran {}",
        inst_refnos.len(),
        bran_refnos.len()
    );

    let started = std::time::Instant::now();
    let mut ready_total = 0usize;
    let mut missing_all: Vec<aios_core::RefnoEnum> = Vec::new();
    for chunk in inst_refnos.chunks(1500) {
        let (ready, missing) = aios_core::query_insts_flat(chunk.iter()).await.expect("flat");
        ready_total += ready.len();
        missing_all.extend(missing);
    }
    println!(
        "[stage] 平表投影(串行 {} 批): {:?}，齐活 {ready_total} / 兜底 {}",
        inst_refnos.len().div_ceil(1500),
        started.elapsed(),
        missing_all.len()
    );

    if !missing_all.is_empty() {
        let started = std::time::Instant::now();
        let mut slim_total = 0usize;
        for chunk in missing_all.chunks(500) {
            slim_total += aios_core::query_insts_slim(chunk.iter()).await.expect("slim").len();
        }
        println!("[stage] slim 兜底: {:?}，{slim_total} 行", started.elapsed());
    }

    // 并行地板探针：同一批平表投影，8 路真任务打到读连接池上——若 wall 明显低于
    // 串行值，说明尾巴在调度形态（根偏斜/串行链），值得重构成全局块并行；若持平，
    // 说明服务端就是单线程地板，重构无益。
    let started = std::time::Instant::now();
    let chunk_futs = inst_refnos
        .chunks(1500)
        .map(|chunk| {
            let chunk = chunk.to_vec();
            tokio::spawn(async move { aios_core::query_insts_flat(chunk.iter()).await })
        })
        .collect::<Vec<_>>();
    let mut par_total = 0usize;
    for handle in chunk_futs {
        par_total += handle.await.expect("join").expect("flat").0.len();
    }
    println!(
        "[stage] 平表投影(全并行 {} 批): {:?}，齐活 {par_total}",
        inst_refnos.len().div_ceil(1500),
        started.elapsed()
    );

    let started = std::time::Instant::now();
    let mut tubi_total = 0usize;
    for chunk in bran_refnos.chunks(500) {
        tubi_total += aios_core::query_tubi_insts_by_brans(chunk).await.expect("tubi").len();
    }
    println!("[stage] tubi 投影: {:?}，{tubi_total} 边", started.elapsed());
}
