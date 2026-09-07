//! 服务供数：读面整条经 gen-model 的 HTTP 接口——树走 `/tree/*`（e3d-io 直读 + 内存骨架）、
//! 属性走 `/element/attributes`（e3d-io）、搜索走 `/search`（服务端快照 NAME 索引）、
//! 三维实例走 `/model/records`（按 dbnum 分桶、一桶一源，ADR-0025）。
//!
//! 它不认识 SurrealDB：这一面从头到尾没有一条打库的路，`service_face_never_touches_the_store`
//! 钉着。地址每次调用时取 `model_update_api::base_url()`，与设置窗那一格同步。

use std::collections::HashSet;
use std::sync::mpsc;

use plant_ui::task_queue::DbnumReport;
use plant_ui_data::{Attr, EleTreeNode, RefU64};

use super::{Identity, ModelInstancesReq, SearchOutcome};
use crate::data::{Evt, RegenerateCount, SEARCH_LIMIT};
use crate::model_update_api::{self, ModelRecords};
use crate::search_index::{Scope, SearchIndexState, SubstringHits};

pub struct ServiceReadFace;

impl ServiceReadFace {
    fn base(&self) -> String {
        model_update_api::base_url()
    }

    /// 启动序列只依赖 gen-model 核心 API（`/health` + `/dbnums`）；镜像离线不阻断树、属性与三维。
    pub async fn identity(&self) -> anyhow::Result<Identity> {
        let base = self.base();
        let (health, report) = futures::try_join!(
            model_update_api::service_health(&base),
            model_update_api::dbnum_report(&base),
        )?;
        let (db_nums, cache_versions) = desi_dbnums(&report);
        Ok(Identity {
            project: health.project,
            mdb: health.mdb.unwrap_or_else(|| "/ALL".into()),
            ns: health.namespace.unwrap_or_default(),
            db_nums,
            cache_versions,
        })
    }

    pub async fn sites(&self) -> anyhow::Result<Vec<EleTreeNode>> {
        model_update_api::tree_roots(&self.base()).await
    }

    pub async fn children(&self, refno: RefU64) -> anyhow::Result<Vec<EleTreeNode>> {
        model_update_api::tree_children(&self.base(), refno).await
    }

    pub async fn ancestors(&self, refno: RefU64) -> anyhow::Result<Vec<RefU64>> {
        model_update_api::tree_ancestors(&self.base(), refno).await
    }

    pub async fn props(&self, refno: RefU64, scope: &Scope) -> anyhow::Result<Vec<Attr>> {
        model_update_api::element_attributes(
            &self.base(),
            refno,
            &scope.project,
            &scope.mdb,
            &scope.ns,
        )
        .await
    }

    /// 服务端没有单独的「按名取一个」接口：搜一页再挑出大小写不敏感的精确命中。
    pub async fn resolve_name(&self, name: &str) -> anyhow::Result<Option<RefU64>> {
        model_update_api::search_names(&self.base(), name, SEARCH_LIMIT)
            .await
            .map(|hits| {
                hits.into_iter()
                    .find(|hit| hit.name.eq_ignore_ascii_case(name.trim()))
                    .map(|hit| hit.refno)
            })
    }

    /// gen-model 的快照 NAME 索引已经是子串索引，前缀那一路直接吃它；不再从库构建
    /// 第二份本地时点，子串那一路恒「不提供」。
    pub async fn search(&self, query: &str, limit: usize) -> SearchOutcome {
        SearchOutcome {
            prefix: model_update_api::search_names(&self.base(), query, limit).await,
            substring: SubstringHits::Unavailable,
            index_failure: None,
        }
    }

    /// 这一面没有本地子串索引可开可建：搜索由服务端的 epoch 钉住的 NAME 索引供给。
    pub async fn refresh_search_index(&self, evt_tx: mpsc::Sender<Evt>, ctx: egui::Context) {
        let _ = evt_tx.send(Evt::SearchIndex(SearchIndexState::Off));
        ctx.request_repaint();
    }

    /// 查 `generation_roots`（服务端在 ensure 时把每个范围解到精确的生成根，
    /// `/model/records` 按它们分桶）。整批一次回，没有逐根进度可报。
    pub async fn model_instances(
        &self,
        req: &ModelInstancesReq<'_>,
    ) -> anyhow::Result<ModelRecords> {
        let identity = req.identity;
        model_update_api::model_records(
            identity.base,
            req.generation_roots,
            identity.project,
            identity.mdb,
            identity.namespace,
        )
        .await
    }

    /// 清点一批根底下已经生成过的模型。
    ///
    /// **只读，且必须跑在任何删除之前**：记录是从服务端的模型行数出来的，删完就查不到了，
    /// 那时回的空集在调用方那里长得像「这里本来就没模型」。
    ///
    /// 多个根合成一份账。右键落在多选上时它们可能互相嵌套（选中一个 ZONE 连同它所在的
    /// SITE），记录按序列化后的身份去重——同一台设备数两遍，确认框上那个数字就是假的。
    /// 交付单元名词表这一面用不上：归根由服务端在 ensure 时做。
    pub async fn regeneration_count(&self, targets: &[RefU64]) -> anyhow::Result<RegenerateCount> {
        let base = self.base();
        let health = model_update_api::service_health(&base).await?;
        let mdb = health.mdb.as_deref().unwrap_or_default();
        let namespace = health.namespace.as_deref().unwrap_or_default();
        let mut records = HashSet::new();
        for record in
            model_update_api::model_records(&base, targets, &health.project, mdb, namespace)
                .await?
                .records
        {
            if let Ok(identity) = serde_json::to_string(&record) {
                records.insert(identity);
            }
        }
        let mut roots = targets.to_vec();
        roots.sort_unstable();
        roots.dedup();
        Ok(RegenerateCount {
            elements: records.len(),
            // 服务端在 ensure 时把每个范围解到精确的生成根；这里保留选中的范围本身，
            // 客户端不另养一份 owner / noun 的真值。
            roots,
        })
    }

    /// 这一面没有进程内缓存：每次读都是一次往返。
    pub async fn invalidate(&self) {}
}

/// `/dbnums` 里本期执行范围的设计库，以及读透形态下它们的缓存版本。
///
/// 读透形态（`data_face = read-through`）连 ISOD 一起算，并带回 `(dbnum, cache_epoch,
/// cached_pe_rows)` 给子串索引当戳；摄入形态只收 DESI、版本表为空。两种形态都不收
/// `not_in_project` 的行——MDB 声明了、项目目录里却没有的库，本期够不着。
fn desi_dbnums(report: &DbnumReport) -> (Vec<u32>, Vec<(u32, u64, u64)>) {
    let read_through = report.data_face.eq_ignore_ascii_case("read-through");
    let db_nums: Vec<u32> = report
        .dbnums
        .iter()
        .filter(|row| !row.not_in_project)
        .filter(|row| {
            row.db_type.eq_ignore_ascii_case("DESI")
                || (read_through && row.db_type.eq_ignore_ascii_case("ISOD"))
        })
        .map(|row| row.dbnum)
        .collect();
    let cache_versions = if read_through {
        report
            .dbnums
            .iter()
            .filter(|row| db_nums.contains(&row.dbnum))
            .map(|row| (row.dbnum, row.cache_epoch, row.cached_pe_rows))
            .collect()
    } else {
        Vec::new()
    };
    (db_nums, cache_versions)
}

#[cfg(test)]
mod tests {
    use plant_ui::task_queue::{DbnumReport, DbnumStatus};

    use super::desi_dbnums;

    fn row(dbnum: u32, db_type: &str, epoch: u64, rows: u64, not_in_project: bool) -> DbnumStatus {
        DbnumStatus {
            dbnum,
            db_type: db_type.to_owned(),
            cache_epoch: epoch,
            cached_pe_rows: rows,
            not_in_project,
            ..Default::default()
        }
    }

    #[test]
    fn read_through_counts_isod_and_carries_cache_versions() {
        let report = DbnumReport {
            data_face: "read-through".into(),
            dbnums: vec![
                row(7997, "DESI", 3, 400, false),
                row(7999, "ISOD", 1, 12, false),
                row(5052, "CATA", 9, 99, false),
                row(8004, "DESI", 2, 7, true),
            ],
        };
        let (db_nums, cache_versions) = desi_dbnums(&report);
        assert_eq!(db_nums, vec![7997, 7999]);
        assert_eq!(cache_versions, vec![(7997, 3, 400), (7999, 1, 12)]);
    }

    #[test]
    fn ingest_keeps_only_desi_and_no_cache_versions() {
        let report = DbnumReport {
            data_face: "ingest".into(),
            dbnums: vec![
                row(7997, "DESI", 3, 400, false),
                row(7999, "ISOD", 1, 12, false),
                row(8004, "DESI", 2, 7, true),
            ],
        };
        let (db_nums, cache_versions) = desi_dbnums(&report);
        assert_eq!(db_nums, vec![7997]);
        assert!(cache_versions.is_empty());
    }

    /// 服务供数从头到尾不打库：这一面若哪天悄悄读了 SurrealDB，就是按读面混源（ADR-0026）。
    #[test]
    fn service_face_never_touches_the_store() {
        let source = include_str!("service.rs");
        let body = source
            .split_once("#[cfg(test)]")
            .map(|(body, _)| body)
            .unwrap_or(source);
        for forbidden in [
            "plant_ui_data::site_nodes",
            "plant_ui_data::child_nodes",
            "plant_ui_data::ancestor_refnos",
            "plant_ui_data::element_props",
            "plant_ui_data::resolve_name",
            "plant_ui_data::search_names_by_prefix",
            "plant_ui_data::model_instances",
            "plant_ui_data::generated_scope",
            "plant_ui_data::nouns_of",
            "plant_ui_data::project_identity",
            "plant_ui_data::connect",
            "plant_ui_data::invalidate_all",
        ] {
            assert!(!body.contains(forbidden), "service.rs 打了库：{forbidden}");
        }
    }
}
