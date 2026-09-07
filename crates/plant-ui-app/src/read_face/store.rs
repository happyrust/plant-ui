//! 库供数：读面整条直连 SurrealDB——树走 `pe` / `pe_owner`、属性走 `ATT_*`、搜索走库里的
//! 名称索引加本地 ngram 子串索引（ADR-0022 / 0023）、三维实例走 `inst_relate.anc`。
//!
//! 这是**保留档**（计划 D1）：给已落盘的 rocksdb 部署、老版本模型服务与对拍用。九条读
//! 从 v0.1.9（`4ec446f5a:crates/plant-ui-app/src/data.rs`）**逐字**接回，不顺手「改进」——
//! 改了就没有对拍的参照。服务供数有而它缺的格由 M3 在界面上标出来，这里不猜。
//!
//! 它不认识模型服务：这一面从头到尾没有一条打 HTTP 读接口的路，
//! `store_face_never_touches_the_service` 钉着。命令面（`ensure` 等）本来就不在读面里。

use std::collections::HashSet;
use std::sync::mpsc;

use plant_ui_data::{Attr, EleTreeNode, RefU64};

use super::{Identity, Progress, SearchOutcome};
use crate::data::{Evt, RegenerateCount};
use crate::model_update_api::ModelRecords;
use crate::search_index::{Scope, SearchIndex, SubstringHits};

pub struct StoreReadFace;

impl StoreReadFace {
    /// 启动序列：连库、抓工程标识（`MDB` / `CURD` / `WORL` 表）。两步任一失败都算没连上。
    /// 模型服务在不在场与这一步无关（计划 D12）：树、属性、三维照常，队列面板自己报离线。
    pub async fn identity(&self) -> anyhow::Result<Identity> {
        plant_ui_data::connect().await?;
        let (project, mdb, ns, db_nums) = plant_ui_data::project_identity().await?;
        Ok(Identity {
            project,
            mdb,
            ns,
            db_nums,
            // 只有服务供数的读透形态有缓存版本；库供数的子串索引拿库水位当戳。
            cache_versions: Vec::new(),
        })
    }

    /// 也先连库：`data::ready` 把 `identity()` 与 `sites()` 并发跑（v0.1.9 是先连库再
    /// 顺序取），这一条若不自己等连接就会在 `connect()` 还没落地时打 `SUL_DB`，报
    /// 「Connection uninitialised」。`connect()` 是 `OnceCell`，第二个调用者等第一个，不重连。
    pub async fn sites(&self) -> anyhow::Result<Vec<EleTreeNode>> {
        plant_ui_data::connect().await?;
        plant_ui_data::site_nodes().await
    }

    pub async fn children(&self, refno: RefU64) -> anyhow::Result<Vec<EleTreeNode>> {
        plant_ui_data::child_nodes(refno.into()).await
    }

    pub async fn ancestors(&self, refno: RefU64) -> anyhow::Result<Vec<RefU64>> {
        plant_ui_data::ancestor_refnos(refno.into()).await
    }

    /// `pe` 一行加 `ATT_*`。元件库（CATA）元素不入模型本体库，这里回的是空表——
    /// 那一格的定论文案由界面给（M3），这一面不替服务端说话。
    pub async fn props(&self, refno: RefU64) -> anyhow::Result<Vec<Attr>> {
        plant_ui_data::element_props(refno.into()).await
    }

    pub async fn resolve_name(&self, name: &str) -> anyhow::Result<Option<RefU64>> {
        plant_ui_data::resolve_name(name).await
    }

    /// 前缀先打库（15.8ms），回来之后再查索引——子串是同步的亚毫秒查询，
    /// 排在后面既不多花时间，还能用上这期间可能刚换代的新索引。
    pub async fn search(&self, query: &str, limit: usize, index: &SearchIndex) -> SearchOutcome {
        let prefix = plant_ui_data::search_names_by_prefix(query, limit).await;
        let (substring, index_failure) = match index.search(query, limit) {
            Ok(hits) => (hits, None),
            // 查询炸了要说出来，否则界面只表现为「子串一条都没有」。
            Err(error) => (
                SubstringHits::Unavailable,
                Some(crate::logs::error_chain(&error)),
            ),
        };
        SearchOutcome {
            prefix,
            substring,
            index_failure,
        }
    }

    /// 校验戳，该开的开、该建的建；`force` = 跳过戳比对硬建（命令行 `reindex`）。
    pub async fn refresh_search_index(
        &self,
        index: SearchIndex,
        scope: Scope,
        force: bool,
        evt_tx: mpsc::Sender<Evt>,
        ctx: egui::Context,
    ) {
        index.refresh(scope, force, evt_tx, ctx).await
    }

    /// `inst_relate.anc CONTAINS $root` 一根一条，逐根报进度。`sources` 留空：这一面
    /// 没有服务端来说每个库的模型来源，那一格在库行上整格不画（ADR-0025 的 `None` 语义）。
    pub async fn model_instances(
        &self,
        roots: &[RefU64],
        progress: Progress<'_>,
    ) -> anyhow::Result<ModelRecords> {
        let records = plant_ui_data::model_instances_with_progress(roots, |done, total| {
            progress(done, total)
        })
        .await?;
        Ok(ModelRecords {
            records,
            sources: Vec::new(),
        })
    }

    /// 清点一批根底下已经生成过的模型。
    ///
    /// **只读，且必须跑在任何删除之前**：`generated_scope` 认的是 `inst_relate`
    /// 行，删完就查不到了，那时回的空集在调用方那里长得像「这里本来就没模型」。
    ///
    /// 多个根合成一份账。右键落在多选上时它们可能互相嵌套（选中一个 ZONE 连同
    /// 它所在的 SITE），元素与直管支管都按 refno 去重——同一台设备数两遍，
    /// 确认框上那个数字就是假的。
    pub async fn regeneration_count(
        &self,
        targets: &[RefU64],
        delivery_units: &[String],
    ) -> anyhow::Result<RegenerateCount> {
        let units: HashSet<String> = delivery_units.iter().cloned().collect();
        let mut merged = plant_ui_data::GeneratedScope::default();
        let mut seen_elements = HashSet::new();
        let mut seen_tubing = HashSet::new();
        for target in targets {
            let scope = plant_ui_data::generated_scope(*target).await?;
            for element in scope.elements {
                if seen_elements.insert(element.refno.refno()) {
                    merged.elements.push(element);
                }
            }
            for bran in scope.tubing_branches {
                if seen_tubing.insert(bran) {
                    merged.tubing_branches.push(bran);
                }
            }
        }
        // 归根要问 noun 的不止元素自己，还有它们整条祖先链。先去重再问：一条链上
        // 的祖先被同一根 BRAN 底下几十个管件共用，按元素逐个问就是几十倍的行数。
        let mut refnos: HashSet<RefU64> = HashSet::new();
        for element in &merged.elements {
            refnos.insert(element.refno.refno());
            refnos.extend(element.anc.iter().copied().map(RefU64));
        }
        let refnos: Vec<RefU64> = refnos.into_iter().collect();
        let nouns = plant_ui_data::nouns_of(&refnos).await?;
        Ok(RegenerateCount {
            elements: merged.element_count(),
            roots: crate::regenerate::regeneration_roots(&merged, &nouns, &units),
        })
    }

    /// 丢本进程的查询缓存：根层与库编号那两条查询带 memoize，键里没有连接本身，
    /// 不丢就是从内存里读上一次的那份。
    pub async fn invalidate(&self) {
        plant_ui_data::invalidate_all().await
    }
}

#[cfg(test)]
mod tests {
    /// 库供数从头到尾不打模型服务的读接口：这一面若哪天悄悄读了 `/tree/*` 或
    /// `/search`，就是按读面混源（ADR-0026）。
    #[test]
    fn store_face_never_touches_the_service() {
        let source = include_str!("store.rs");
        let body = source
            .split_once("#[cfg(test)]")
            .map(|(body, _)| body)
            .unwrap_or(source);
        // `ModelRecords` 这个公共返回形状可以借用，打接口的函数一个都不许出现。
        for forbidden in [
            "model_update_api::tree_roots",
            "model_update_api::tree_children",
            "model_update_api::tree_ancestors",
            "model_update_api::search_names",
            "model_update_api::element_attributes",
            "model_update_api::model_records",
            "model_update_api::dbnum_report",
            "model_update_api::service_health",
            "model_update_api::ensure_model",
            "base_url(",
        ] {
            assert!(
                !body.contains(forbidden),
                "store.rs 打了模型服务的接口：{forbidden}"
            );
        }
        // 九条读确实都在，不是把哪一条留给了别人。
        for expected in [
            "plant_ui_data::connect()",
            "plant_ui_data::project_identity()",
            "plant_ui_data::site_nodes()",
            "plant_ui_data::child_nodes(",
            "plant_ui_data::ancestor_refnos(",
            "plant_ui_data::element_props(",
            "plant_ui_data::resolve_name(",
            "plant_ui_data::search_names_by_prefix(",
            "plant_ui_data::model_instances_with_progress(",
            "plant_ui_data::generated_scope(",
            "plant_ui_data::nouns_of(",
            "plant_ui_data::invalidate_all()",
        ] {
            assert!(body.contains(expected), "store.rs 少了库读调用：{expected}");
        }
    }

    /// 启动序列把 `identity()` 与 `sites()` 并发跑，两条都得自己把连接等到手——只在
    /// `identity()` 里连，`sites()` 就会抢在连接落地前打库（实机：`PLANT_READ_FACE=store`
    /// 启动报「Connection uninitialised」）。
    #[test]
    fn identity_and_sites_each_wait_for_the_connection() {
        let source = include_str!("store.rs");
        for method in ["pub async fn identity(", "pub async fn sites("] {
            let body = source
                .split_once(method)
                .expect(method)
                .1
                .split_once("\n    }\n")
                .expect("方法结尾")
                .0;
            assert!(
                body.contains("plant_ui_data::connect().await?"),
                "{method} 没有先等连接"
            );
        }
    }
}
