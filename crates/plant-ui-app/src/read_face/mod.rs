//! 供数模式的读面（ADR-0026）：模型树 / 属性 / 搜索 / 三维实例 / 工程标识从哪儿读。
//!
//! 一个 enum、两个实现——**服务供数**（`Service`，经 gen-model HTTP，底下是 e3d-io 与
//! e3d-model）与**库供数**（`Store`，直连 SurrealDB；保留档，给已落盘的 rocksdb 部署、
//! 老版本模型服务与对拍用）。选择只在 `data::spawn` 与热切那一拍发生，`data.rs` 的读调用
//! 全部经它，数据线程内不许根据错误自行换面。
//!
//! 不在这里的两样：**房间**——它只有库一条路，服务供数下经 `/health.mirror` 门控懒连
//! （计划 D6）；**命令面**——`ensure` / 手动更新 / 队列 / 提资 / 命令查询永远走模型服务
//! （计划 D3），所以 `model_instances` 只读、`ensure` 循环留在 `data.rs`。
//!
//! 为什么是 enum 不是 `dyn Trait`：恰好两个变体、穷尽匹配；`async fn` 直接写、不上
//! `async-trait`；原生端 future 要 `Send`、wasm 端不要（`data.rs` 已为此分了两套
//! `InflightQuery`），enum 让两端各自成立，`dyn` + `async fn` 做不到。

use std::sync::mpsc;

use plant_ui_data::{Attr, EleTreeNode, NameHit, RefU64};

use crate::data::{Evt, RegenerateCount};
use crate::model_update_api::ModelRecords;
use crate::search_index::{Scope, SearchIndex, SubstringHits};

#[derive(Debug, Clone, PartialEq)]
pub struct SubtreeBounds {
    pub min_mm: [f32; 3],
    pub max_mm: [f32; 3],
    pub model_count: usize,
}

#[derive(Debug)]
pub struct NoRenderableGeometry;
impl std::fmt::Display for NoRenderableGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NO_RENDERABLE_GEOMETRY")
    }
}
impl std::error::Error for NoRenderableGeometry {}
pub fn is_no_renderable_geometry(error: &anyhow::Error) -> bool {
    error.downcast_ref::<NoRenderableGeometry>().is_some()
        || crate::model_update_api::failure_of(error).code == "no_renderable_geometry"
}

/// 对拍探针（计划 §5.4 / D14）：`plant-ui-app --read-face-parity`，无头子命令，两面并存
/// 只许在它里面。浏览器端没有命令行也没有库连接，不编进去。
#[cfg(not(target_arch = "wasm32"))]
pub mod parity;
mod service;
mod store;

/// 设置里那一格的值。类型住在绘制 crate（设置窗与接入点面板要画它），这里只是借用。
pub use plant_ui::settings::ReadFaceKind;
pub use service::ServiceReadFace;
pub use store::StoreReadFace;

/// 工程标识：启动序列里除 SITE 根层之外的那一半。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub project: String,
    /// 当前 MDB 名（带前导 `/`）。
    pub mdb: String,
    pub ns: String,
    /// 本期执行范围里的设计库。
    pub db_nums: Vec<u32>,
    /// `(dbnum, cache_epoch, cached_pe_rows)`，只有服务供数的读透形态给；其余为空。
    pub cache_versions: Vec<(u32, u64, u64)>,
}

/// 随请求带下来的服务身份四格。它们是宿主的设置项，数据线程不认识，所以由 `Req` 捎来；
/// 库供数不看它们。
#[derive(Debug, Clone, Copy)]
pub struct ServiceIdentity<'a> {
    pub base: &'a str,
    pub project: &'a str,
    pub mdb: &'a str,
    pub namespace: &'a str,
}

/// 一次三维实例读取要的东西。两面要的根不一样，所以两份都带，各取各的：
///
/// - `roots`：调用方自己的根——取回工作重装是快照里的模型 refno，eye 显示是点下去的那个
///   树目标。**库供数只查它**（v0.1.9 原样：`inst_relate.anc CONTAINS $root` 一根一条）。
///   拿并集去跑的话，嵌套在 `roots` 底下的生成根会把同一批实例数两遍。
/// - `generation_roots`：**服务供数要查的名单**，调用方按前一步 `ensure` 回执算好——重装
///   = `roots` ∪ 回执里的生成根；eye 显示 = 回执里的生成根，回执空则退到目标本身
///   （工作树落地时的形状，`/model/records` 按它分桶）。
#[derive(Debug, Clone, Copy)]
pub struct ModelInstancesReq<'a> {
    pub roots: &'a [RefU64],
    pub generation_roots: &'a [RefU64],
    pub identity: ServiceIdentity<'a>,
}

/// 一次搜索的两路结果。前缀那一路会失败，子串那一路只有「没就绪」——合成一个
/// `Result` 会让前缀失败时把子串命中一起吞掉。
pub struct SearchOutcome {
    pub prefix: anyhow::Result<Vec<NameHit>>,
    pub substring: SubstringHits,
    /// 子串索引查询本身炸了（不是「没就绪」）。调用方要把它作为 `Evt::SearchIndex(Failed)`
    /// 说出去，否则界面只表现为「子串一条都没有」。服务供数没有本地索引，恒 `None`。
    pub index_failure: Option<String>,
}

/// 三维实例读取的进度回调 `(done, total)`。服务供数一次整批回，不报进度；库供数按根报。
pub type Progress<'a> = &'a mut (dyn FnMut(usize, usize) + Send);

pub enum ReadFace {
    Service(ServiceReadFace),
    Store(StoreReadFace),
}

impl ReadFace {
    pub fn new(kind: ReadFaceKind) -> Self {
        match kind {
            ReadFaceKind::Service => Self::Service(ServiceReadFace),
            ReadFaceKind::Store => Self::Store(StoreReadFace),
        }
    }

    /// 接入点面板那一行「供数：服务 / 库」读它。
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn kind(&self) -> ReadFaceKind {
        match self {
            Self::Service(_) => ReadFaceKind::Service,
            Self::Store(_) => ReadFaceKind::Store,
        }
    }

    /// 工程标识（项目 / MDB / ns / 设计库名单）。
    pub async fn identity(&self) -> anyhow::Result<Identity> {
        match self {
            Self::Service(face) => face.identity().await,
            Self::Store(face) => face.identity().await,
        }
    }

    /// SITE 根层（MDB 世界的下一层）。
    pub async fn sites(&self) -> anyhow::Result<Vec<EleTreeNode>> {
        match self {
            Self::Service(face) => face.sites().await,
            Self::Store(face) => face.sites().await,
        }
    }

    /// 某节点的直接子层，按成员表原序。
    pub async fn children(&self, refno: RefU64) -> anyhow::Result<Vec<EleTreeNode>> {
        match self {
            Self::Service(face) => face.children(refno).await,
            Self::Store(face) => face.children(refno).await,
        }
    }

    /// 「自己 -> 上级 -> …」到库顶的祖先链。
    pub async fn ancestors(&self, refno: RefU64) -> anyhow::Result<Vec<RefU64>> {
        match self {
            Self::Service(face) => face.ancestors(refno).await,
            Self::Store(face) => face.ancestors(refno).await,
        }
    }

    /// 选中元素的 UI 属性表。
    pub async fn props(&self, refno: RefU64, scope: &Scope) -> anyhow::Result<Vec<Attr>> {
        match self {
            Self::Service(face) => face.props(refno, scope).await,
            Self::Store(face) => face.props(refno).await,
        }
    }

    /// 命令行按名称定位：精确匹配一个元素。
    pub async fn resolve_name(&self, name: &str) -> anyhow::Result<Option<RefU64>> {
        match self {
            Self::Service(face) => face.resolve_name(name).await,
            Self::Store(face) => face.resolve_name(name).await,
        }
    }

    /// 标题栏搜索框的一次查询。
    pub async fn search(&self, query: &str, limit: usize, index: &SearchIndex) -> SearchOutcome {
        match self {
            Self::Service(face) => face.search(query, limit).await,
            Self::Store(face) => face.search(query, limit, index).await,
        }
    }

    /// 校验 / 重建子串索引。状态经 `Evt::SearchIndex` 发出去；`force` = 跳过戳比对硬建。
    pub async fn refresh_search_index(
        &self,
        index: SearchIndex,
        scope: Scope,
        force: bool,
        evt_tx: mpsc::Sender<Evt>,
        ctx: egui::Context,
    ) {
        match self {
            Self::Service(face) => face.refresh_search_index(evt_tx, ctx).await,
            Self::Store(face) => {
                face.refresh_search_index(index, scope, force, evt_tx, ctx)
                    .await
            }
        }
    }

    /// 这些根下已经生成的几何实例。**只读**：把范围追到文件最新是 `ensure` 的事
    /// （ADR-0024），那是命令面，留在调用方；两面各取 `req` 里自己要的那份根（见
    /// [`ModelInstancesReq`]）。
    pub async fn model_instances(
        &self,
        req: &ModelInstancesReq<'_>,
        progress: Progress<'_>,
    ) -> anyhow::Result<ModelRecords> {
        match self {
            Self::Service(face) => face.model_instances(req).await,
            Self::Store(face) => face.model_instances(req.roots, progress).await,
        }
    }

    pub async fn subtree_bounds(
        &self,
        root: RefU64,
        scope: &Scope,
    ) -> anyhow::Result<SubtreeBounds> {
        match self {
            Self::Service(face) => face.subtree_bounds(root, scope).await,
            Self::Store(face) => face.subtree_bounds(root).await,
        }
    }

    /// 「重新生成模型」的清点：这些根底下已经生成过多少元素、归成多少个生成单元。
    pub async fn regeneration_count(
        &self,
        targets: &[RefU64],
        delivery_units: &[String],
    ) -> anyhow::Result<RegenerateCount> {
        match self {
            Self::Service(face) => face.regeneration_count(targets).await,
            Self::Store(face) => face.regeneration_count(targets, delivery_units).await,
        }
    }

    /// 丢掉这一面自己的查询缓存（取回工作 / 重连 / 换面前）。
    pub async fn invalidate(&self) {
        match self {
            Self::Service(face) => face.invalidate().await,
            Self::Store(face) => face.invalidate().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use plant_ui_data::RefU64;

    use super::{ModelInstancesReq, ReadFace, ReadFaceKind, ServiceIdentity};

    #[test]
    fn a_face_reports_the_kind_it_was_built_from() {
        assert_eq!(
            ReadFace::new(ReadFaceKind::Service).kind(),
            ReadFaceKind::Service
        );
        assert_eq!(
            ReadFace::new(ReadFaceKind::Store).kind(),
            ReadFaceKind::Store
        );
    }

    /// 两面要的根写在同一份请求里、由各自取，调用方不必知道哪一面在场：
    /// 服务供数拿 `generation_roots`，库供数拿 `roots`。这里钉的是两份各自独立、
    /// 不会被谁悄悄合成一份。
    #[test]
    fn a_model_request_carries_both_root_sets() {
        let roots = [RefU64::from(20)];
        let generation_roots = [RefU64::from(7), RefU64::from(20), RefU64::from(42)];
        let req = ModelInstancesReq {
            roots: &roots,
            generation_roots: &generation_roots,
            identity: ServiceIdentity {
                base: "http://127.0.0.1:8022",
                project: "SAM",
                mdb: "/MDB",
                namespace: "plant",
            },
        };
        assert_eq!(req.roots, &roots);
        assert_eq!(req.generation_roots, &generation_roots);
        assert_eq!(req.identity.base, "http://127.0.0.1:8022");
    }
}
