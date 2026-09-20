//! anc 路径的手动计时探针（层级查询优化方案，gen-model
//! `docs/plans/2026-08-07-inst-relate-anc-u64-hierarchy-query-plan.md`）。
//!
//! 本文件原是 P2 的新旧路径对拍验收：2026-08-07 在 AMS 实库通过
//! （41 SITE 根 / 51,422 实例，新旧 refno/owner/网格 hash 集完全一致，
//! 253.3s → 2.73s），验收记录固化在上述方案文档 P2/P4 节。P3 退役旧深遍历
//! 路径（`model_instances_legacy`）与 `PLANT_UI_LEGACY_MODEL_QUERY` 开关后，
//! 对拍基线不复存在，对拍用例随之删除；保留计时用例作 P4 调优迭代的探针。
//!
//! 运行：`cargo test -p plant-ui-data --test anc_model_query_parity -- --ignored --nocapture`

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
    let roots: Vec<plant_ui_data::RefU64> = sites.iter().map(|node| node.refno.refno()).collect();
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
    let inst_refnos: Vec<aios_core::RefnoEnum> = resolved
        .iter()
        .flat_map(|(insts, _)| insts.iter().copied())
        .collect();
    let bran_refnos: Vec<aios_core::RefnoEnum> = resolved
        .iter()
        .flat_map(|(_, brans)| brans.iter().copied())
        .collect();
    println!(
        "[stage] 解析(8路): {resolve_elapsed:?}，inst {} / bran {}",
        inst_refnos.len(),
        bran_refnos.len()
    );

    let started = std::time::Instant::now();
    let mut ready_total = 0usize;
    let mut missing_all: Vec<aios_core::RefnoEnum> = Vec::new();
    for chunk in inst_refnos.chunks(1500) {
        let (ready, missing) = aios_core::query_insts_flat(chunk.iter())
            .await
            .expect("flat");
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
            slim_total += aios_core::query_insts_slim(chunk.iter())
                .await
                .expect("slim")
                .len();
        }
        println!(
            "[stage] slim 兜底: {:?}，{slim_total} 行",
            started.elapsed()
        );
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
        tubi_total += aios_core::query_tubi_insts_by_brans(chunk)
            .await
            .expect("tubi")
            .len();
    }
    println!(
        "[stage] tubi 投影: {:?}，{tubi_total} 边",
        started.elapsed()
    );
}
