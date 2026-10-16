//! UI 线程与数据线程（tokio）之间的桥。
//!
//! eframe 的绘制是同步的，而读面全是 async：这里起一个常驻数据线程，
//! 请求经 channel 进、结果经 channel 出，UI 每帧非阻塞地收。每个结果落地后
//! `request_repaint`，免得空闲中的 UI 睡过结果。
//!
//! 读面（树 / 属性 / 搜索 / 三维实例 / 工程标识）一律经 [`ReadFace`]（ADR-0026）：
//! 这个文件里不直接打 HTTP 读接口，也不直接查库——`data_rs_reads_only_through_read_face`
//! 钉着。留在这里直接调的只有命令面（生成 / 更新 / 队列 / 提资）、房间的镜像门，
//! 与 MBD 尺寸标注（只有模型服务这一条路，`pipe_dimensions_go_straight_to_the_model_service`）。
//!
//! 显示与生成分家（ADR-0030）：模型通道上的两臂（`ModelLoad::Scopes` / `Replace`）**只查**
//! 已生成的产物——以点下去的节点为祖先问 `/model/records`——从不调生成接口
//! （`display_never_generates` / `get_work_reloads_without_generating` 钉着）。

use std::future::Future;
use std::sync::Arc;
use std::sync::mpsc;

use futures::stream::{FuturesUnordered, StreamExt};
use plant_ui::data_publish::PublishRequest;
use plant_ui::manual_data_publish::RoomCodePublishRequest;
use plant_ui::model_update::{Enqueued, Preview, ProgressEvent};
use plant_ui::task_queue::Poll as QueuePoll;
use plant_ui_data::{EleTreeNode, RefU64};

use crate::read_face::{ModelInstancesReq, ReadFace, ReadFaceKind, ServiceIdentity};
use crate::search_index::{Scope, SearchIndex, SearchIndexState, SubstringHits};

/// 搜索一次最多带回多少条。下拉本来就只列得下十几行，多取的部分只是让包含匹配
/// 那条慢路多扫一会儿。取满这个数就等于「后面还有」，界面据此提示缩小范围。
pub const SEARCH_LIMIT: usize = 20;

/// Expected transport capability, not a failed query or an empty result.
#[derive(Debug)]
pub(crate) struct MirrorFeatureUnavailable(pub String);

impl std::fmt::Display for MirrorFeatureUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for MirrorFeatureUnavailable {}

fn require_direct_mirror_endpoint(
    health: &plant_ui::task_queue::Health,
    feature: &str,
) -> anyhow::Result<()> {
    if health.mirror.endpoint.as_deref() == Some("embedded:mem") {
        return Err(MirrorFeatureUnavailable(format!(
            "镜像不可直连：{feature} 仍使用 SurrealDB 兼容读面，而当前 gen-model 是 \
             store_mode=mem（库在服务进程里，无数据库端口）。请使用服务供数功能，或把 \
             gen-model 的 store_mode 改为 rocksdb 并连一台在跑的 SurrealDB。"
        ))
        .into());
    }
    if !health.mirror.status.eq_ignore_ascii_case("ready") {
        anyhow::bail!(
            "镜像不可用：{feature} 依赖 SurrealDB（当前状态 {}）",
            if health.mirror.status.is_empty() {
                "unknown"
            } else {
                &health.mirror.status
            }
        );
    }
    Ok(())
}

async fn require_mirror_feature(feature: &str) -> anyhow::Result<()> {
    let health =
        crate::model_update_api::service_health(&crate::model_update_api::base_url()).await?;
    require_direct_mirror_endpoint(&health, feature)?;
    // Core startup deliberately does not connect plant-ui to SurrealDB.  The
    // compatibility pages attach lazily only after gen-model reports it ready.
    plant_ui_data::connect().await
}

pub enum Req {
    /// 树范围聚焦（`Cmd::FocusTreeScope`）要的子树包围盒。只查：范围内没有已生成的模型
    /// 就是没有，相机不动、不悄悄生成（ADR-0030 D1）。
    SubtreeBounds { epoch: u64, target: RefU64 },
    /// 懒加载某节点的直接子层。
    Children(RefU64),
    /// 选中元素的 UI 属性表。
    Props(RefU64),
    /// 选中元素的房间归属（右键「查看所属房间」子菜单与「房间」页签共用），
    /// 与 `Props` 同拍预取。
    ElementRooms(RefU64),
    /// 选中对象若是一块在册 PANEL，返回它直属的房间；普通元素返回 None。
    PanelRoom(RefU64),
    /// X-Ray 专用轻量拓扑：一间房直属的全部 PANEL。
    RoomPanels(RefU64),
    /// 一间房的详情。`Cmd::FocusRoom` 的展开靠它拿全量成员集。
    RoomDetail(RefU64),
    /// 房间视图的房间体（2026-09-29 直段截断计划 M1）：这间房在册面板的模型记录。
    ///
    /// 与眼睛同一条路：先 `ensure` 房间 FRMW，再经读面取实例，只留 `panels` 里那些面板的记录。
    /// `epoch` 原样带回：连点两间房时，晚到的旧房间体靠它认出来丢掉。
    RoomVolume {
        epoch: u64,
        room: RefU64,
        panels: Vec<RefU64>,
    },
    /// 房间浏览器的全表（重查询，全库扫描级；只在打开浮窗或手动刷新时发，
    /// 不进启动路径——计划风险 5）。
    RoomsOverview,
    /// 取回工作的三维重装：**只重查**已生成的产物，不先生成（ADR-0030 D3）。
    ///
    /// `roots` 是快照里的模型 refno（库供数按它一根一条 `anc CONTAINS` 查）；`scope_targets`
    /// 是清场前点过眼睛的树目标——服务供数以它们为祖先问 `/model/records`（一个 ZONE 一次往返，
    /// 不必逐叶子问）。`scope_targets` 为空 = 老快照没有范围记录，服务供数退到按 `roots` 逐个查。
    Models {
        roots: Vec<RefU64>,
        scope_targets: Vec<RefU64>,
        /// 这次重装是不是在清偿「数据已应用、三维欠着」那笔账（`Evt::Models` 原样带回）。
        debt_reload: bool,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// eye 显示路径：以点下去的每个目标为祖先查已生成的模型（ADR-0030 D1）。查询这一臂
    /// 本身只查不生；查空 / 库无模型时**默认重新生成**的兜底在宿主（`Evt::ModelScope`
    /// 回包与 `Evt::DbnumModelProbe` 会合后由 [`Req::GenerateModels`] 发出）。
    ModelScopes {
        epoch: u64,
        targets: Vec<RefU64>,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 库级模型预检查：`GET /dbnums/{dbnum}/model/roots` 的就绪根快照（`ready_total > 0`
    /// 即「这个库有按当前源生成且仍最新的模型」）。与 `ModelScopes` 同帧同发——结果显示
    /// 路径用它抢在 records 回空之前认账；它是枚举级重查询，按库单飞，宿主去重。
    DbnumModelProbe {
        epoch: u64,
        dbnum: u32,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 命令行按名称定位元素。
    ResolveName(String),
    /// 标题栏搜索框的一次查询。前缀走库的名称索引（全库范围，毫秒级），子串走
    /// 本地 ngram 索引（当前 MDB 范围，亚毫秒），一条 Evt 把两路一起带回去。
    ///
    /// 不带搜索范围：子串那一路的范围是**建索引时**定下的，跟着索引走；前缀路
    /// 本来就不限库。
    SearchElements { epoch: u64, query: String },
    /// 校验子串索引的陈旧戳，该建就建。单飞，重复发不会叠加。
    CheckSearchIndex,
    /// 强制重建子串索引（命令行 `reindex`）：跳过戳比对。
    ///
    /// 它是「戳看不见的改动」唯一的门——没有水位的库里发生纯改名，行数与水位
    /// 都不动，自动校验永远发现不了。
    RebuildSearchIndex,
    /// 右键「重新生成模型」（ADR-0030 D2）：对选中的每个目标**各一发**
    /// `POST /model/generate { refno, force: true }`，服务端自己解根、删旧、重算。
    /// 客户端不清点、不解根、不删产物；一条请求里顺序跑完整批，回执整批回
    /// （`Evt::ModelsGenerated`），宿主再对成功的目标发显示。
    ///
    /// 房间参照模型不在这一趟的操心范围：服务端启动时就建好它，生成路径上的房间
    /// 计算照常被 `ensure_ready` 把关——真撞上还在建 / 建死，那一发如实 409 回来。
    GenerateModels {
        epoch: u64,
        refnos: Vec<RefU64>,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 右键「清理模型」（2026-10-10 清理模型计划）：对选中的每个目标**各一发**
    /// `DELETE /model/subtree?refno&confirm`，服务端按子树删产物行、不重算。与生成同形：
    /// 顺序跑完整批，回执整批回（`Evt::ModelsCleaned`），宿主再把成功目标名下的模型卸掉。
    CleanupModels {
        epoch: u64,
        refnos: Vec<RefU64>,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 树定位目标的祖先链。目标不在已加载的树里时才发，见 ADR-0014。
    Ancestors(RefU64),
    /// 丢查询缓存并重跑启动序列。连库失败后命令行上的「重试」走这条，结果仍走
    /// `Evt::Ready`。
    Reconnect,
    /// 换供数模式（ADR-0026 热切）：排干在途 → 丢缓存 → **两条通道同拍换面** → 重跑
    /// 启动序列，结果仍走 `Evt::Ready`。宿主在发它之前已经清场并拍好快照，`Ready` 之后
    /// 按取回工作那条路重装（计划 §5.3）。这是数据线程里换面的**唯一**入口——数据线程
    /// 自己不许根据错误换面。
    SwitchReadFace(ReadFaceKind),
    ModelUpdatePreview {
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    ModelUpdateExecute {
        base: String,
        project: String,
        mdb: String,
        namespace: String,
        /// 勾选集折算的 dbnum 子集（ADR-020）。`None` = 全范围。
        dbnums: Option<Vec<u32>>,
        /// `true` 只表示模型更新向导的确认按钮；队列里的即时扫描为 `false`。
        from_wizard: bool,
    },
    /// 库行「重算模型（整库）」：`POST /api/v1/dbnums/{dbnum}/model/rebuild`
    /// （2026-09-10 重新计算计划 R1）。回执一个 task_id，进度看任务队列。
    ModelRebuild {
        base: String,
        project: String,
        mdb: String,
        namespace: String,
        dbnum: u32,
    },
    /// 「重新生成房间」（全库）：`POST /api/v1/rooms/rebuild`——服务端清掉两张房间
    /// 边表、换代重建常驻参照模型、逐库整库重算房间关系。202 受理之后本端轮询
    /// `/health` 画进度（`Evt::ResidentWait`），全部段落走完回 `Evt::RoomsRebuilt`。
    ///
    /// `cancel` 是「后台继续」：重建一旦发出服务端收不回，置位只是让本端停止轮询、
    /// 摘掉进度屏——服务端照跑到底。
    RebuildRooms {
        epoch: u64,
        cancel: Arc<std::sync::atomic::AtomicBool>,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 库行「清理模型（整库）」：`DELETE /api/v1/dbnums/{dbnum}/model?confirm={dbnum}`。
    /// `epoch` 认帧，与右键清理同一套：晚到的回执对不上就丢掉。
    CleanupDbnumModel {
        epoch: u64,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
        dbnum: u32,
    },
    /// 取回工作：丢缓存，重查 SITE 根层与这些已展开分支的子层。
    ///
    /// 分支由 App 侧算好交下来。数据线程不认识展开状态，也不该认识——它只是
    /// 「照这张单子重查一遍」。
    GetWork {
        branches: Vec<RefU64>,
        /// 数据已应用但模型仍在后台生成时为 false：刷新树与属性，保留当前三维。
        reload_models: bool,
    },
    /// 队列面板的一次轮询（队列快照 + 任务表 + health + 持久欠账）。
    QueuePoll { base: String },
    /// 暂停 / 恢复出队。
    QueueSetPaused { base: String, paused: bool },
    /// 复活一行死信。它不排新的数据批次，结果一律等下一拍轮询。
    RetryPendingUnit {
        base: String,
        project: String,
        mdb: String,
        namespace: String,
        root_refno: String,
    },
    DataPublish {
        base: String,
        request: PublishRequest,
    },
    LookupDataPublishDeletions {
        base: String,
        request: PublishRequest,
    },
    RoomCodePublish {
        base: String,
        request: RoomCodePublishRequest,
    },
    CommandQuery {
        epoch: u64,
        label: String,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
        tool: String,
        arguments: serde_json::Value,
    },
    /// 一条 BRAN 的 MBD 尺寸标注（`GET /api/mbd/v2/pipe/{refno}`，
    /// `gen-model/.planning/2026-09-09-plant-ui-mbd-dimensions` B1）。
    ///
    /// 不走读面：尺寸标注只有模型服务这一条路（计划边界「MBD 永远走模型服务，与命令面
    /// 同口径」）——求解器要的成员结点视图只有服务端会拼，库供数下也一样。
    /// `epoch` 认帧：右键换了别的 BRAN、或者已经点了隐藏，晚到的旧结果不许再贴回来。
    PipeDimensions { epoch: u64, refno: RefU64 },
}

/// 一次取回工作重新查回来的那部分树。
pub struct GetWork {
    pub sites: Vec<EleTreeNode>,
    pub reload_models: bool,
    /// 重查成功的分支及其新的直接子层。
    pub branches: Vec<(RefU64, Vec<EleTreeNode>)>,
    /// 服务端明确确认不存在的分支；与暂时查询失败分开处理。
    pub missing: Vec<RefU64>,
    /// 重查失败的分支及原因。一个分支查不动不该让整次取回作废，
    /// 它那一层就保持原样，失败单独进日志。
    pub failed: Vec<(RefU64, String)>,
}

/// 启动序列（连接 + 工程标识 + SITE 根层）的合并产物。
pub struct ReadyInfo {
    pub project: String,
    /// 当前 MDB 名（带前导 `/`）。模型更新用它定本期执行范围。
    pub mdb: String,
    pub ns: String,
    pub db_nums: Vec<u32>,
    pub cache_versions: Vec<(u32, u64, u64)>,
    /// 开始读取根层的时刻。首次队列快照只补这之后完成的批次，避免启动期间漏刷新。
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub sites: Vec<EleTreeNode>,
    /// 根层每个 SITE 属于哪个库（`(库号, SITE 名)`）。模型更新面板的 SITE 列用它认库；
    /// 库供数给不出库号，那一面恒空。
    pub site_dbs: Vec<(u32, String)>,
}

pub enum Evt {
    SubtreeBounds {
        epoch: u64,
        target: RefU64,
        result: anyhow::Result<crate::read_face::SubtreeBounds>,
    },
    Ready(anyhow::Result<ReadyInfo>),
    Children(RefU64, anyhow::Result<Vec<EleTreeNode>>),
    Props(RefU64, anyhow::Result<Vec<plant_ui_data::Attr>>),
    /// 元素的房间归属，已按归属强度排序。带上请求时的 refno，晚到的旧结果要认得出来才好丢。
    ElementRooms(
        RefU64,
        anyhow::Result<Vec<plant_ui_data::room::RoomRelation>>,
    ),
    PanelRoom(RefU64, anyhow::Result<Option<RefU64>>),
    RoomPanels(RefU64, anyhow::Result<Vec<RefU64>>),
    /// 房间详情。`Ok(None)` = 这个 refno 在库里查不到（不是查询失败）。
    RoomDetail(
        RefU64,
        anyhow::Result<Option<plant_ui_data::room::RoomDetail>>,
    ),
    /// 房间体的面板记录，只含在册面板。`Err` = 读面没走通；空表 = 在册面板都还没有模型。
    RoomVolume {
        epoch: u64,
        room: RefU64,
        result: anyhow::Result<Vec<aios_core::GeomInstQuery>>,
    },
    /// 房间浏览器全表。
    RoomsOverview(anyhow::Result<Vec<plant_ui_data::room::RoomOverviewRow>>),
    /// 取回工作重装的整批模型记录，连同每个库认下的取数源（spec §4.12）：日志据此
    /// 说出哪几个库此刻由 API 从内存供数。
    Models(bool, anyhow::Result<crate::model_update_api::ModelRecords>),
    /// 取回工作重查模型的进度（库供数每根一步；服务供数整批一次回，只有收尾那一拍）。
    ReloadProgress {
        done: usize,
        total: usize,
    },
    /// 眼睛那一臂查 records 的进度（库供数每根一步；服务供数只有收尾那一拍）。
    ModelScopeProgress {
        epoch: u64,
        target: RefU64,
        done: usize,
        total: usize,
    },
    /// 以 `target` 为祖先查到的已生成模型。`Ok` 且为空 = 范围内没有已生成的模型
    /// （不是失败，ADR-0030 D1）；老服务端对没生成过的范围回 409 `not_generated`，
    /// 宿主把它当同一件事（ADR-0030 后果 6）。
    ModelScope(
        u64,
        RefU64,
        anyhow::Result<crate::model_update_api::ModelRecords>,
    ),
    /// 库级模型预检查的回包（`Req::DbnumModelProbe`）。`Ok` 且 `ready_total == 0` =
    /// 这个库没有按当前源生成的模型——宿主据此默认重新生成；`Err` 按「不知道」放行。
    DbnumModelProbe {
        epoch: u64,
        dbnum: u32,
        result: anyhow::Result<crate::model_update_api::DbnumModelRoots>,
    },
    ResolvedName(String, anyhow::Result<Option<RefU64>>),
    /// 一次搜索的结果。搜索框每敲一下就发一条，晚到的旧结果靠 `epoch` 认出来丢掉；
    /// `query` 原样带回，绘制层拿它确认手上这份命中是不是当前输入的。
    ///
    /// 两路分开报：前缀那一路打库，会失败；子串那一路查本地索引，「没就绪」
    /// 不是失败。合成一个 `Result` 就会让库断线时连本地索引的命中一起消失。
    SearchElements {
        epoch: u64,
        query: String,
        prefix: anyhow::Result<Vec<plant_ui_data::NameHit>>,
        substring: SubstringHits,
    },
    /// 子串索引的状态变化：启动打开、后台重建的每一格进度、失败各一条。
    SearchIndex(SearchIndexState),
    /// 右键「重新生成模型」整批回执：每个目标各一份成败，按请求顺序。`epoch` 认帧——
    /// 确认那一刻钉住的号，对不上的是上一趟的尾巴，丢弃。
    ModelsGenerated {
        epoch: u64,
        outcomes: Vec<(
            RefU64,
            anyhow::Result<crate::model_update_api::GenerateReply>,
        )>,
    },
    /// 「等房间参照模型装载 / 重建」的进度：只在 `RebuildRooms` 那一趟里发——
    /// `Building` 每拍一条，`Ready` 翻篇一条。`epoch` 认帧，与 `ModelsGenerated` 同一套。
    ResidentWait {
        epoch: u64,
        status: crate::model_update_api::ResidentStatus,
    },
    /// 「重新生成房间」的收摊回执：重建 + 逐库重算全部走完是 `Ok(())`；
    /// 建死 / 等超时是 `Err`。「后台继续」之后这条被帧号丢掉，不再发。
    RoomsRebuilt {
        epoch: u64,
        result: anyhow::Result<()>,
    },
    /// 右键「清理模型」整批回执：每个目标各一份成败，按请求顺序。`epoch` 认帧，与生成同一套。
    ModelsCleaned {
        epoch: u64,
        outcomes: Vec<(
            RefU64,
            anyhow::Result<crate::model_update_api::CleanupReply>,
        )>,
    },
    /// 目标的祖先链，「自己 -> 上级 -> …」序。带上请求时的那个 refno：
    /// 连续定位只算最后一次，晚到的旧链要认得出来才好丢。
    Ancestors(RefU64, anyhow::Result<Vec<RefU64>>),
    ModelUpdatePreview(anyhow::Result<Preview>),
    /// 「扫描 + 入队」的回执。合流之后它不再是单个 task_id。
    ModelUpdateExecute {
        from_wizard: bool,
        result: anyhow::Result<Enqueued>,
    },
    /// 「重算模型（整库）」的回执，带上是哪个库。真值等下一拍队列轮询。
    ModelRebuild {
        dbnum: u32,
        result: anyhow::Result<crate::model_update_api::ModelRebuildReceipt>,
    },
    /// 「清理模型（整库）」的回执。成功之后宿主按 `ref0s` 卸三维；缺席则只记日志。
    /// `epoch` 对不上当前确认框的那一帧就丢掉。
    DbnumModelCleaned {
        epoch: u64,
        dbnum: u32,
        result: anyhow::Result<crate::model_update_api::DbnumModelCleanupReceipt>,
    },
    /// 取回工作的整批结果。根层查不动就是整次失败——根层没了树无从谈起。
    GetWork(anyhow::Result<GetWork>),
    /// 队列面板的一次轮询结果。四份数据一起换代，不留自相矛盾的中间态。
    QueuePoll(anyhow::Result<QueuePoll>),
    /// 暂停 / 恢复的回执。真值仍以下一次轮询的快照为准，这里只负责把失败说出来。
    QueueSetPaused(bool, anyhow::Result<()>),
    /// 复活死信的回执，带上是哪一个根。真值同样等下一拍快照。
    RetryPendingUnit(String, anyhow::Result<()>),
    DataPublish(
        PublishRequest,
        anyhow::Result<crate::data_publish_api::SubmitResult>,
    ),
    DataPublishDeletions(anyhow::Result<Vec<plant_ui::data_publish::DeletionCandidate>>),
    RoomCodePublish(anyhow::Result<crate::data_publish_api::SubmitResult>),
    CommandQuery {
        epoch: u64,
        label: String,
        result: anyhow::Result<crate::model_update_api::QueryReply>,
    },
    /// 一条 BRAN 的尺寸标注回包。`epoch` / `refno` 原样带回：帧号对不上的旧结果丢弃。
    /// 错误是分好型的 [`crate::mbd_api::MbdError`]，界面按型给出路，不解析字符串。
    PipeDimensions {
        epoch: u64,
        refno: RefU64,
        result: Result<plant_mbd::MbdV2PipeData, crate::mbd_api::MbdError>,
    },
    /// 队列视图的逐单元明细，带发生在哪个任务上。
    ///
    /// 这三个只有原生端的 WebSocket 构造（`model_update_ws`）；wasm 的 Feed 是
    /// 轮询兜底的桩，只发 `QueueFeedDown`，所以 wasm 目标上它们没有构造点。
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    QueueProgress(String, ProgressEvent),
    /// 有任务起讫。只当醒钟用：叫轮询早一拍去取，不拿它改行状态。
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    QueueTaskChanged,
    /// 某个库的模型取数源翻面了（WS `model_source_changed`，spec §4.12 / §5.3）：
    /// 初始化发布收口 → 数据库；回退重建清库 → 内存。**只改库行那一格、不重载场景**
    /// ——内存投影与刚发布的行是同一版文件算出来的，下一次范围重载自然读到 rocksdb。
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    ModelSourceChanged {
        dbnum: u32,
        source: plant_ui::task_queue::ModelSource,
    },
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    QueueFeedLive,
    /// 这个构建压根不订阅逐单元明细（wasm 端）。与 `QueueFeedDown` 分开：
    /// 断线是连过又掉了、可以重连、中间那段确实漏了；这一种从来没连过。
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    QueueFeedUnsubscribed(String),
    QueueFeedDown(String),
}

pub struct Bridge {
    pub req: mpsc::Sender<Req>,
    pub evt: mpsc::Receiver<Evt>,
    /// 给数据线程之外的生产者用（现在只有那条 WebSocket）。UI 每帧只认一个收端，
    /// 长连接自己另开一条 channel 的话就得在 `pump_events` 里再轮询一次。
    pub evt_tx: mpsc::Sender<Evt>,
}

enum ModelLoad {
    Replace {
        roots: Vec<RefU64>,
        scope_targets: Vec<RefU64>,
        debt_reload: bool,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    Scopes {
        epoch: u64,
        targets: Vec<RefU64>,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 换面。模型通道是串行的：排在它前面的那条在途装载做完才换，其后排队的全是新面
    /// ——与交互通道「排干在途再换」是同一个意思。只由 worker 在处理
    /// `Req::SwitchReadFace` 时转发，不经 [`route_model_load`]。
    Switch(ReadFaceKind),
}

fn route_model_load(req: Req, tx: &mpsc::Sender<ModelLoad>) -> Result<Option<Req>, ModelLoad> {
    match req {
        Req::Models {
            roots,
            scope_targets,
            debt_reload,
            base,
            project,
            mdb,
            namespace,
        } => tx
            .send(ModelLoad::Replace {
                roots,
                scope_targets,
                debt_reload,
                base,
                project,
                mdb,
                namespace,
            })
            .map(|_| None)
            .map_err(|error| error.0),
        Req::ModelScopes {
            epoch,
            targets,
            base,
            project,
            mdb,
            namespace,
        } => tx
            .send(ModelLoad::Scopes {
                epoch,
                targets,
                base,
                project,
                mdb,
                namespace,
            })
            .map(|_| None)
            .map_err(|error| error.0),
        req => Ok(Some(req)),
    }
}

/// 取回工作：先丢本进程的查询缓存，再把根层与这些分支重查一遍。
///
/// 缓存必须先丢。重查 SITE 根层走的是带 memoize 的那条查询，缓存还在的话它会
/// 原样把旧的那份还回来，界面看着刷新过了、内容一个字没变。房间那条镜像连接的
/// 缓存不归读面管（D6），所以两份都丢。
async fn get_work(
    face: &ReadFace,
    branches: &[RefU64],
    reload_models: bool,
) -> anyhow::Result<GetWork> {
    plant_ui_data::invalidate_all().await;
    face.invalidate().await;
    let sites = face.sites().await?;
    let mut loaded = Vec::with_capacity(branches.len());
    let mut failed = Vec::new();
    let mut missing = Vec::new();
    for refno in branches {
        match face.children(*refno).await {
            Ok(kids) => loaded.push((*refno, kids)),
            Err(error) if crate::model_update_api::failure_of(&error).code == "not_found" => {
                missing.push(*refno);
            }
            Err(error) => failed.push((*refno, crate::logs::error_chain(&error))),
        }
    }
    Ok(GetWork {
        sites,
        reload_models,
        branches: loaded,
        missing,
        failed,
    })
}

/// 启动序列只依赖当前供数模式的读面（工程标识 + SITE 根层并发取）；镜像离线不阻断
/// 树、属性与三维。
async fn ready(face: &ReadFace) -> anyhow::Result<ReadyInfo> {
    let (identity, sites, site_dbs) =
        futures::try_join!(face.identity(), face.sites(), face.site_dbs())?;
    let observed_at = chrono::Utc::now();
    Ok(ReadyInfo {
        project: identity.project,
        mdb: identity.mdb,
        ns: identity.ns,
        db_nums: identity.db_nums,
        cache_versions: identity.cache_versions,
        observed_at,
        sites,
        site_dbs,
    })
}

/// 一次在途的交互读查询（[`handle_read`] 的装箱形态）。
/// 原生端 worker 由 tokio 驱动，future 必须 Send；浏览器端是 spawn_local
/// 单线程，SurrealDB 的 wasm future 本来也不是 Send，各给各的皮。
#[cfg(not(target_arch = "wasm32"))]
type InflightQuery = futures::future::BoxFuture<'static, ()>;
#[cfg(target_arch = "wasm32")]
type InflightQuery = futures::future::LocalBoxFuture<'static, ()>;

#[cfg(not(target_arch = "wasm32"))]
fn boxed_query(fut: impl Future<Output = ()> + Send + 'static) -> InflightQuery {
    Box::pin(fut)
}
#[cfg(target_arch = "wasm32")]
fn boxed_query(fut: impl Future<Output = ()> + 'static) -> InflightQuery {
    Box::pin(fut)
}

/// 房间体的取材与眼睛显示同一条只读路径（ADR-0030）：查询房间范围内已生成的几何，
/// 只留在册面板的记录。生成仍由右键发起。
async fn room_volume_records(
    face: &ReadFace,
    scope: &Scope,
    room: RefU64,
    panels: &[RefU64],
) -> anyhow::Result<Vec<aios_core::GeomInstQuery>> {
    let base = crate::model_update_api::base_url();
    let mut progress = |_: usize, _: usize| {};
    let loaded = face
        .model_instances(
            &ModelInstancesReq {
                roots: &[room],
                scope_roots: &[room],
                identity: ServiceIdentity {
                    base: &base,
                    project: &scope.project,
                    mdb: &scope.mdb,
                    namespace: &scope.ns,
                },
            },
            &mut progress,
        )
        .await?;
    Ok(panel_records(loaded.records, panels))
}

/// 只留在册面板的记录。房间 FRMW 底下未必只有面板，不在册的面板也不参与归属计算。
fn panel_records(
    records: Vec<aios_core::GeomInstQuery>,
    panels: &[RefU64],
) -> Vec<aios_core::GeomInstQuery> {
    let wanted: std::collections::HashSet<RefU64> = panels.iter().copied().collect();
    records
        .into_iter()
        .filter(|record| wanted.contains(&record.refno.refno()))
        .collect()
}

/// 并发通道里的一次交互读：查询、回执、叫醒 UI，整段自包含。
///
/// 这里只放**互相独立**的读与 HTTP 往返——它们在 [`FuturesUnordered`] 里并发
/// 推进，树的子层查询不再排在属性 / 归属 / 队列轮询后面（双击展开时第一击
/// 触发的两条选中查询曾把 Children 压在队尾）。晚到回包由 UI 侧的陈旧门
/// 丢弃，完成顺序无所谓。全局手术（Reconnect / GetWork）不进这条道：它们
/// 要独占（见 worker 循环），并发会让晚到的读把刚失效的缓存填回旧数据。
async fn handle_read(
    req: Req,
    face: Arc<ReadFace>,
    index: SearchIndex,
    scope: Scope,
    evt_tx: mpsc::Sender<Evt>,
    ctx: egui::Context,
    task_ctx: bevy_wasm_tasks::TaskContext,
) {
    match req {
        Req::SubtreeBounds { epoch, target } => {
            let r = face.subtree_bounds(target, &scope).await;
            let _ = evt_tx.send(Evt::SubtreeBounds {
                epoch,
                target,
                result: r,
            });
        }
        Req::Children(refno) => {
            let r = face.children(refno).await;
            let _ = evt_tx.send(Evt::Children(refno, r));
        }
        Req::Props(refno) => {
            let r = face.props(refno, &scope).await;
            let _ = evt_tx.send(Evt::Props(refno, r));
        }
        Req::ElementRooms(refno) => {
            let r = match require_mirror_feature("房间").await {
                Ok(()) => plant_ui_data::room::element_rooms(refno.into()).await,
                Err(error) => Err(error),
            };
            let _ = evt_tx.send(Evt::ElementRooms(refno, r));
        }
        Req::PanelRoom(refno) => {
            let r = match require_mirror_feature("房间").await {
                Ok(()) => plant_ui_data::room::panel_room(refno.into()).await,
                Err(error) => Err(error),
            };
            let _ = evt_tx.send(Evt::PanelRoom(refno, r));
        }
        Req::RoomPanels(refno) => {
            let r = match require_mirror_feature("房间").await {
                Ok(()) => plant_ui_data::room::room_panels(refno.into()).await,
                Err(error) => Err(error),
            };
            let _ = evt_tx.send(Evt::RoomPanels(refno, r));
        }
        Req::RoomDetail(refno) => {
            // 成员预览条数与「房间」页签的列表容量对齐；隔离 / 取景
            // 用的是 member_refnos 全量，不受这个数约束。
            let r = match require_mirror_feature("房间历史").await {
                Ok(()) => plant_ui_data::room::room_detail(refno.into(), 8).await,
                Err(error) => Err(error),
            };
            let _ = evt_tx.send(Evt::RoomDetail(refno, r));
        }
        Req::RoomVolume {
            epoch,
            room,
            panels,
        } => {
            let result = room_volume_records(&face, &scope, room, &panels).await;
            let _ = evt_tx.send(Evt::RoomVolume {
                epoch,
                room,
                result,
            });
        }
        Req::RoomsOverview => {
            let r = match require_mirror_feature("房间").await {
                Ok(()) => plant_ui_data::room::rooms_overview().await,
                Err(error) => Err(error),
            };
            let _ = evt_tx.send(Evt::RoomsOverview(r));
        }
        Req::ResolveName(name) => {
            let result = face.resolve_name(&name).await;
            let _ = evt_tx.send(Evt::ResolvedName(name, result));
        }
        Req::SearchElements { epoch, query } => {
            let outcome = face.search(&query, SEARCH_LIMIT, &index).await;
            // 子串索引查炸了要说出来，否则界面只表现为「子串一条都没有」。
            if let Some(failure) = outcome.index_failure {
                let _ = evt_tx.send(Evt::SearchIndex(SearchIndexState::Failed(failure)));
            }
            let _ = evt_tx.send(Evt::SearchElements {
                epoch,
                query,
                prefix: outcome.prefix,
                substring: outcome.substring,
            });
        }
        Req::GenerateModels {
            epoch,
            refnos,
            base,
            project,
            mdb,
            namespace,
        } => {
            // 生成只有这一个入口（ADR-0030 D2）。`force = true`：范围内已有的产物一律重算替换，
            // 解根 / 删旧 / 重算都在服务端。顺序做：一个目标可能就是整个 ZONE，并发只会让
            // 服务端的 per-dbnum 锁互相撞。线上一律斜杠形（`24381/100677`）。
            // 房间参照模型不在这趟的操心范围：服务端启动就建好它；真撞上还在建，那一发
            // 如实 409 回来按失败记（「可重试」写在文案里，人再点一次就是）。
            let mut outcomes = Vec::with_capacity(refnos.len());
            for refno in refnos {
                let result = crate::model_update_api::generate_model(
                    &base,
                    &refno.to_slash_string(),
                    true,
                    &project,
                    &mdb,
                    &namespace,
                )
                .await;
                outcomes.push((refno, result));
            }
            let _ = evt_tx.send(Evt::ModelsGenerated { epoch, outcomes });
        }
        Req::RebuildRooms {
            epoch,
            cancel,
            base,
            project,
            mdb,
            namespace,
        } => {
            // 「重新生成房间」：受理（202）之后把「还在建 / 逐库重算」等过去，每拍发
            // `Evt::ResidentWait` 画进度；「后台继续」置位了就不发收摊回执——那一帧
            // 已被宿主作废。409 updating = 启动首建或上一趟重建还在跑：不算失败，
            // 直接挂上同一趟的进度等它翻篇（再点一次这项就是重新观战）。
            let mut task_ctx = task_ctx;
            let accepted = crate::model_update_api::rebuild_rooms(
                &base, &project, &mdb, &namespace,
            )
            .await;
            let result = match accepted {
                Err(error) if !crate::model_update_api::resident_building(&error) => {
                    Some(Err(error))
                }
                _ => match wait_resident_ready(&base, epoch, &cancel, &evt_tx, &mut task_ctx)
                    .await
                {
                    ResidentWaitOutcome::Ready => Some(Ok(())),
                    ResidentWaitOutcome::Failed(error) => Some(Err(error)),
                    ResidentWaitOutcome::Cancelled => None,
                },
            };
            if let Some(result) = result {
                let _ = evt_tx.send(Evt::RoomsRebuilt { epoch, result });
            }
        }
        Req::CleanupModels {
            epoch,
            refnos,
            base,
            project,
            mdb,
            namespace,
        } => {
            // 清理只有这一个入口。顺序做，理由同生成：一个目标可能就是整个 ZONE，并发只会让
            // 服务端的 per-dbnum 锁互相撞。线上一律斜杠形。
            let mut outcomes = Vec::with_capacity(refnos.len());
            for refno in refnos {
                let result = crate::model_update_api::delete_model_subtree(
                    &base,
                    &refno.to_slash_string(),
                    &project,
                    &mdb,
                    &namespace,
                )
                .await;
                outcomes.push((refno, result));
            }
            let _ = evt_tx.send(Evt::ModelsCleaned { epoch, outcomes });
        }
        Req::CheckSearchIndex => {
            face.refresh_search_index(index, scope, false, evt_tx.clone(), ctx.clone())
                .await;
        }
        Req::RebuildSearchIndex => {
            face.refresh_search_index(index, scope, true, evt_tx.clone(), ctx.clone())
                .await;
        }
        Req::Ancestors(refno) => {
            let result = face.ancestors(refno).await;
            let _ = evt_tx.send(Evt::Ancestors(refno, result));
        }
        Req::ModelUpdatePreview {
            base,
            project,
            mdb,
            namespace,
        } => {
            let result = crate::model_update_api::preview(&base, &project, &mdb, &namespace).await;
            let _ = evt_tx.send(Evt::ModelUpdatePreview(result));
        }
        Req::ModelUpdateExecute {
            base,
            project,
            mdb,
            namespace,
            dbnums,
            from_wizard,
        } => {
            let result = crate::model_update_api::execute(
                &base,
                &project,
                &mdb,
                &namespace,
                dbnums.as_deref(),
            )
            .await;
            let _ = evt_tx.send(Evt::ModelUpdateExecute {
                from_wizard,
                result,
            });
        }
        Req::DbnumModelProbe {
            epoch,
            dbnum,
            base,
            project,
            mdb,
            namespace,
        } => {
            let result = crate::model_update_api::dbnum_model_roots(
                &base, &project, &mdb, &namespace, dbnum,
            )
            .await;
            let _ = evt_tx.send(Evt::DbnumModelProbe {
                epoch,
                dbnum,
                result,
            });
        }
        Req::ModelRebuild {
            base,
            project,
            mdb,
            namespace,
            dbnum,
        } => {
            let result =
                crate::model_update_api::model_rebuild(&base, &project, &mdb, &namespace, dbnum)
                    .await;
            let _ = evt_tx.send(Evt::ModelRebuild { dbnum, result });
        }
        Req::CleanupDbnumModel {
            epoch,
            base,
            project,
            mdb,
            namespace,
            dbnum,
        } => {
            let result = crate::model_update_api::delete_dbnum_model(
                &base, &project, &mdb, &namespace, dbnum,
            )
            .await;
            let _ = evt_tx.send(Evt::DbnumModelCleaned {
                epoch,
                dbnum,
                result,
            });
        }
        Req::QueuePoll { base } => {
            let result = crate::model_update_api::poll_queue(&base).await;
            let _ = evt_tx.send(Evt::QueuePoll(result));
        }
        Req::QueueSetPaused { base, paused } => {
            let result = crate::model_update_api::set_queue_paused(&base, paused).await;
            let _ = evt_tx.send(Evt::QueueSetPaused(paused, result));
        }
        Req::RetryPendingUnit {
            base,
            project,
            mdb,
            namespace,
            root_refno,
        } => {
            let result = crate::model_update_api::retry_pending_unit(
                &base,
                &project,
                &mdb,
                &namespace,
                &root_refno,
            )
            .await;
            let _ = evt_tx.send(Evt::RetryPendingUnit(root_refno, result));
        }
        Req::LookupDataPublishDeletions { base, request } => {
            let result = crate::data_publish_api::lookup_deletions(&base, &request).await;
            let _ = evt_tx.send(Evt::DataPublishDeletions(result));
        }
        Req::RoomCodePublish { base, request } => {
            let result = crate::data_publish_api::submit_room_codes(&base, &request).await;
            let _ = evt_tx.send(Evt::RoomCodePublish(result));
        }
        Req::DataPublish { base, request } => {
            let result = crate::data_publish_api::submit(&base, &request).await;
            let _ = evt_tx.send(Evt::DataPublish(request, result));
        }
        Req::CommandQuery {
            epoch,
            label,
            base,
            project,
            mdb,
            namespace,
            tool,
            arguments,
        } => {
            let result =
                crate::model_update_api::query(&base, &project, &mdb, &namespace, &tool, arguments)
                    .await;
            let _ = evt_tx.send(Evt::CommandQuery {
                epoch,
                label,
                result,
            });
        }
        Req::PipeDimensions { epoch, refno } => {
            // 服务地址与命令面同源：设置窗那一格「模型服务」，MBD 端点就在
            // 同一进程同一端口上（服务端 `web_service/mbd.rs`），不另起一格配置。
            let result =
                crate::mbd_api::pipe_dimensions(&crate::model_update_api::base_url(), refno).await;
            let _ = evt_tx.send(Evt::PipeDimensions {
                epoch,
                refno,
                result,
            });
        }
        Req::Reconnect | Req::SwitchReadFace(_) | Req::GetWork { .. } => {
            unreachable!("全局手术在 worker 循环里独占处理")
        }
        Req::Models { .. } | Req::ModelScopes { .. } => {
            unreachable!("模型请求已路由到专用任务")
        }
    }
    ctx.request_repaint();
}

/// 宿主还在不在。两条 worker 每睡完一拍都要问一次，没了就收工。
///
/// Bevy 退出（原生关窗、浏览器刷新 / 关页 / 跳转时 winit 收到 `pagehide`）会走
/// `exiting()` → `World::clear_all()`，把 `bevy-wasm-tasks` 的 `UpdateTicks` 连同它手上
/// 唯一那份 watch `Sender` 一起 drop。此后 `sleep_updates` 不再挂起而是**立刻返回**——
/// 不问这一句，`loop` 就成了一个没有任何 await 真正让出的死循环。原生端它跑在 tokio
/// 线程上、进程随即退出，没人看见；浏览器里它与页面共用主线程，那个微任务永不结束，
/// `pagehide` 之后的卸载流程整个被堵死，刷新就是「页面无响应」（2026-09-18 实机：
/// 刷新 180 s 不回来，渲染进程一颗核 100%）。
fn host_alive(task_ctx: &bevy_wasm_tasks::TaskContext) -> bool {
    // `has_changed` 只在所有 Sender 都没了时回 `Err`——正是「宿主已退出」那一刻。
    task_ctx.tick_rx.has_changed().is_ok()
}

/// 「等房间参照模型装载 / 重建」的轮询节奏与总上限：120 拍 ≈ 2 秒（60 fps），
/// 上限 300 秒——`「重新生成房间」`一趟可能跑几分钟（建拓扑 + 预热网格 + 逐库重算边）。
const RESIDENT_POLL_UPDATES: usize = 120;
const RESIDENT_WAIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(300);

/// [`wait_resident_ready`] 的三种结局。
enum ResidentWaitOutcome {
    /// 放行了——就绪 / 这一格不存在 / 健康拉不到（后两种按「没什么可等」收摊）。
    Ready,
    /// 「后台继续」：用户把进度屏摘了（重建那发服务端照跑到底，这里只是停止轮询）。
    Cancelled,
    /// 等不到：建死（`blocked`）或超上限。携带可直接当回执用的错误。
    Failed(anyhow::Error),
}

/// 把「房间参照模型还在建 / 逐库重算没跑完」这一段等过去：就绪一次往返即回；
/// `Building` 每拍一条 `Evt::ResidentWait` 给界面画进度；建死 / 超上限回 `Failed`。
///
/// 只为 `RebuildRooms` 服务：常驻模型的换代（startup 首建、`rooms/rebuild`）都把
/// `loading` 期与 `edge-recompute` 段算进 `Building`。`/health` 拉不到按放行处置——
/// 那不是「还在建」。
async fn wait_resident_ready(
    base: &str,
    epoch: u64,
    cancel: &std::sync::atomic::AtomicBool,
    evt_tx: &mpsc::Sender<Evt>,
    task_ctx: &mut bevy_wasm_tasks::TaskContext,
) -> ResidentWaitOutcome {
    use std::sync::atomic::Ordering;
    let started = std::time::Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return ResidentWaitOutcome::Cancelled;
        }
        match crate::model_update_api::resident_status(base).await {
            Ok(crate::model_update_api::ResidentStatus::Ready) => {
                let _ = evt_tx.send(Evt::ResidentWait {
                    epoch,
                    status: crate::model_update_api::ResidentStatus::Ready,
                });
                return ResidentWaitOutcome::Ready;
            }
            Ok(crate::model_update_api::ResidentStatus::Blocked { reason }) => {
                return ResidentWaitOutcome::Failed(anyhow::anyhow!(
                    "服务端房间参照模型初始化失败（{}）；原因见服务端 /health.room_build.resident",
                    reason.unwrap_or_else(|| "未给出".into())
                ));
            }
            Ok(status @ crate::model_update_api::ResidentStatus::Building { .. }) => {
                let _ = evt_tx.send(Evt::ResidentWait { epoch, status });
            }
            Err(_) => return ResidentWaitOutcome::Ready,
        }
        if started.elapsed() >= RESIDENT_WAIT_LIMIT {
            return ResidentWaitOutcome::Failed(anyhow::anyhow!(
                "等房间参照模型初始化超过 {} 秒还没就绪",
                RESIDENT_WAIT_LIMIT.as_secs()
            ));
        }
        task_ctx.sleep_updates(RESIDENT_POLL_UPDATES).await;
        // 宿主没了（关窗 / 刷新页面）睡眠立刻返回——不问这一句就是死循环。
        if !host_alive(task_ctx) {
            return ResidentWaitOutcome::Cancelled;
        }
    }
}

/// 起数据线程：按供数模式造读面、抓工程标识与 SITE 根层，然后循环处理懒加载请求。
///
/// 交互通道与模型通道各持一份读面句柄，`spawn` 里一起造——两条通道必须永远是同一面，
/// 树是库、三维是服务就是按读面混源（ADR-0026）。
pub fn spawn(ctx: egui::Context, tasks: &bevy_wasm_tasks::Tasks<'_>, kind: ReadFaceKind) -> Bridge {
    let (req_tx, req_rx) = mpsc::channel();
    let (model_tx, model_rx) = mpsc::channel::<ModelLoad>();
    let (evt_tx, evt_rx) = mpsc::channel();
    let evt_tx_out = evt_tx.clone();
    let face = Arc::new(ReadFace::new(kind));
    let model_face = Arc::clone(&face);
    let model_evt_tx = evt_tx.clone();
    let model_ctx = ctx.clone();
    let model_worker = move |mut task_ctx: bevy_wasm_tasks::TaskContext| async move {
        let mut model_face = model_face;
        loop {
            while let Ok(load) = model_rx.try_recv() {
                match load {
                    ModelLoad::Switch(kind) => {
                        // 串行通道：走到这儿说明前面排着的装载都已做完，换了就是新面。
                        model_face = Arc::new(ReadFace::new(kind));
                    }
                    ModelLoad::Replace {
                        roots,
                        scope_targets,
                        debt_reload,
                        base,
                        project,
                        mdb,
                        namespace,
                    } => {
                        // sim 模式没有 SurrealDB 也没有网格文件：模型通道短路成
                        // 空结果，三维视口保持空场景，树与队列照常演。
                        let result = if crate::sim::enabled() {
                            Ok(crate::model_update_api::ModelRecords::default())
                        } else {
                            // 只重查，不先生成（ADR-0030 D3）：取回工作重装的是「上次显示过
                            // 的那片已生成产物」，SAVEWORK 之后三维不自己追新，追新只有右键
                            // 「重新生成模型」。服务供数以清场前点过眼睛的范围目标为祖先问
                            // `/model/records`（服务端按祖先回答，ADR-086）；老快照没有范围
                            // 记录就退到按快照里的模型 refno 逐个问。库供数仍只查快照根
                            // （两份都进 `ModelInstancesReq`，各取各的）。
                            let scope_roots = if scope_targets.is_empty() {
                                roots.clone()
                            } else {
                                scope_targets
                            };
                            let progress_tx = model_evt_tx.clone();
                            let progress_ctx = model_ctx.clone();
                            let result = model_face
                                .model_instances(
                                    &ModelInstancesReq {
                                        roots: &roots,
                                        scope_roots: &scope_roots,
                                        identity: ServiceIdentity {
                                            base: &base,
                                            project: &project,
                                            mdb: &mdb,
                                            namespace: &namespace,
                                        },
                                    },
                                    &mut move |done, total| {
                                        let _ =
                                            progress_tx.send(Evt::ReloadProgress { done, total });
                                        progress_ctx.request_repaint();
                                    },
                                )
                                .await;
                            let _ = model_evt_tx.send(Evt::ReloadProgress {
                                done: roots.len(),
                                total: roots.len(),
                            });
                            result
                        };
                        let _ = model_evt_tx.send(Evt::Models(debt_reload, result));
                        model_ctx.request_repaint();
                    }
                    ModelLoad::Scopes {
                        epoch,
                        targets,
                        base,
                        project,
                        mdb,
                        namespace,
                    } => {
                        for target in targets {
                            if crate::sim::enabled() {
                                let _ = model_evt_tx.send(Evt::ModelScope(
                                    epoch,
                                    target,
                                    Ok(crate::model_update_api::ModelRecords::default()),
                                ));
                                model_ctx.request_repaint();
                                continue;
                            }
                            // 点下去的节点就是查询范围（ADR-0030 D1）：两面都以它为祖先查
                            // 已生成的实例——库供数 `anc CONTAINS`，服务供数 `/model/records`
                            // 按祖先回答（ADR-086）。不持锁、不写模型库；查空就是空。
                            let progress_tx = model_evt_tx.clone();
                            let progress_ctx = model_ctx.clone();
                            let result = model_face
                                .model_instances(
                                    &ModelInstancesReq {
                                        roots: &[target],
                                        scope_roots: &[target],
                                        identity: ServiceIdentity {
                                            base: &base,
                                            project: &project,
                                            mdb: &mdb,
                                            namespace: &namespace,
                                        },
                                    },
                                    &mut move |done, total| {
                                        let _ = progress_tx.send(Evt::ModelScopeProgress {
                                            epoch,
                                            target,
                                            done,
                                            total,
                                        });
                                        progress_ctx.request_repaint();
                                    },
                                )
                                .await;
                            let _ = model_evt_tx.send(Evt::ModelScopeProgress {
                                epoch,
                                target,
                                done: 1,
                                total: 1,
                            });
                            let _ = model_evt_tx.send(Evt::ModelScope(epoch, target, result));
                            model_ctx.request_repaint();
                        }
                    }
                }
            }
            task_ctx.sleep_updates(1).await;
            if !host_alive(&task_ctx) {
                return;
            }
        }
    };
    let worker = move |mut task_ctx: bevy_wasm_tasks::TaskContext| async move {
        // 交互通道手上的那一面。只在下面 `Req::SwitchReadFace` 那一臂整体替换，别处不动。
        let mut face = face;
        // sim 模式：整个数据线程改由进程内引擎供数——假身份直接 Ready，
        // 后续请求全部路由到剧本状态机，WS 同形明细由 pump 推送。
        let mut sim_engine = crate::sim::Engine::from_env();
        // 子串索引的把手与它的取材范围。范围跟着 `Ready` 走——重连之后当前 MDB
        // 可能就不是原来那个了，索引也得跟着换目录。
        let index = SearchIndex::default();
        let mut scope = Scope::default();
        let started = match sim_engine.as_ref() {
            Some(engine) => Ok(engine.ready_info()),
            None => ready(&face).await,
        };
        if let Ok(info) = started.as_ref() {
            scope = Scope {
                project: info.project.clone(),
                ns: info.ns.clone(),
                mdb: info.mdb.clone(),
                dbnums: info.db_nums.clone(),
                cache_versions: info.cache_versions.clone(),
            };
        }
        let _ = evt_tx.send(Evt::Ready(started));
        ctx.request_repaint();

        // 在途的交互读。FuturesUnordered 让它们在同一个任务里**并发**推进：
        // 不需要按请求 spawn（wasm 上也没有 tokio::spawn 可用），一条慢查询
        // 也不再把整条桥堵成串行（模型加载在此之前就已单独分道）。
        let mut inflight: FuturesUnordered<InflightQuery> = FuturesUnordered::new();
        // 子串索引的启动态由读面决定：服务供数没有本地索引可开，报 Off。
        face.refresh_search_index(
            index.clone(),
            scope.clone(),
            false,
            evt_tx.clone(),
            ctx.clone(),
        )
        .await;
        loop {
            while let Ok(req) = req_rx.try_recv() {
                // 模型实例冷加载在大库上可达 88 秒，且 Replace / Scopes 之间有
                // 先后语义，PC 与 Web 都送到专用串行任务；交互读走下面的并发道，
                // 不与它同席。
                let req = match route_model_load(req, &model_tx) {
                    Ok(Some(req)) => req,
                    Ok(None) => continue,
                    Err(ModelLoad::Replace { debt_reload, .. }) => {
                        let _ = evt_tx.send(Evt::Models(
                            debt_reload,
                            Err(anyhow::anyhow!("模型查询任务已停止")),
                        ));
                        ctx.request_repaint();
                        continue;
                    }
                    Err(ModelLoad::Scopes { epoch, targets, .. }) => {
                        for target in targets {
                            let _ = evt_tx.send(Evt::ModelScope(
                                epoch,
                                target,
                                Err(anyhow::anyhow!("模型查询任务已停止")),
                            ));
                        }
                        ctx.request_repaint();
                        continue;
                    }
                    Err(ModelLoad::Switch(_)) => {
                        unreachable!("换面由 worker 自己转发到模型通道，不经 route_model_load")
                    }
                };
                if let Some(engine) = sim_engine.as_mut() {
                    crate::sim::handle(engine, req, &evt_tx);
                    ctx.request_repaint();
                    continue;
                }
                match req {
                    // 全局手术要独占：先让在途查询全部落地再动手，否则一条晚到
                    // 的读会把 invalidate_all 刚丢掉的缓存又填回旧数据（重连同理，
                    // 旧连接上的回包不该落在新会话后面）。
                    Req::Reconnect => {
                        while inflight.next().await.is_some() {}
                        // 缓存先丢，理由与 `get_work` 同一条：根层与库编号那两条查询
                        // 带 memoize，键里只有 MDB 与库类型，没有连接本身，重连也不
                        // 会把它们冲掉。留着它，重连就是从内存里读上一次的那份——
                        // 界面看着重连过了，这中间新增的 SITE 一个都不在。
                        plant_ui_data::invalidate_all().await;
                        face.invalidate().await;
                        let reconnected = ready(&face).await;
                        if let Ok(info) = reconnected.as_ref() {
                            scope = Scope {
                                project: info.project.clone(),
                                ns: info.ns.clone(),
                                mdb: info.mdb.clone(),
                                dbnums: info.db_nums.clone(),
                                cache_versions: info.cache_versions.clone(),
                            };
                        }
                        let _ = evt_tx.send(Evt::Ready(reconnected));
                        ctx.request_repaint();
                    }
                    // 热切（ADR-0026）：与重连同一副骨架，多两步——换掉手上的面、
                    // 叫模型通道同拍换。排干在途的理由同上：旧面上晚到的回包不该落在
                    // 新面后面，而且它们要的缓存刚被丢掉。
                    Req::SwitchReadFace(kind) => {
                        while inflight.next().await.is_some() {}
                        plant_ui_data::invalidate_all().await;
                        face.invalidate().await;
                        face = Arc::new(ReadFace::new(kind));
                        // 两条通道必须永远是同一面：树是库、三维是服务就是按读面混源。
                        // 模型通道停了就没有三维可言，那条路上的失败由后续装载自己报。
                        let _ = model_tx.send(ModelLoad::Switch(kind));
                        let switched = ready(&face).await;
                        if let Ok(info) = switched.as_ref() {
                            scope = Scope {
                                project: info.project.clone(),
                                ns: info.ns.clone(),
                                mdb: info.mdb.clone(),
                                dbnums: info.db_nums.clone(),
                                cache_versions: info.cache_versions.clone(),
                            };
                        }
                        let _ = evt_tx.send(Evt::Ready(switched));
                        ctx.request_repaint();
                        // 子串索引的状态跟着面走：库供数要开 / 建，服务供数报 Off。
                        face.refresh_search_index(
                            index.clone(),
                            scope.clone(),
                            false,
                            evt_tx.clone(),
                            ctx.clone(),
                        )
                        .await;
                    }
                    Req::GetWork {
                        branches,
                        reload_models,
                    } => {
                        while inflight.next().await.is_some() {}
                        let result = get_work(&face, &branches, reload_models).await;
                        let _ = evt_tx.send(Evt::GetWork(result));
                        ctx.request_repaint();
                    }
                    // 其余都是互相独立的交互读 / HTTP 往返，进并发道。树的子层
                    // 查询从此不再排在属性 / 归属 / 队列轮询后面。
                    req => inflight.push(boxed_query(handle_read(
                        req,
                        Arc::clone(&face),
                        index.clone(),
                        scope.clone(),
                        evt_tx.clone(),
                        ctx.clone(),
                        task_ctx.clone(),
                    ))),
                }
            }
            if let Some(engine) = sim_engine.as_mut() {
                let events = engine.pump();
                if !events.is_empty() {
                    for evt in events {
                        let _ = evt_tx.send(evt);
                    }
                    ctx.request_repaint();
                }
            }
            if inflight.is_empty() {
                task_ctx.sleep_updates(1).await;
            } else {
                // 有在途查询：FuturesUnordered 把它们并发推进，同时每拍醒一次
                // 去收新请求——新点击不用等上一条查询做完才被看见。
                let tick = std::pin::pin!(task_ctx.sleep_updates(1));
                let _ = futures::future::select(inflight.select_next_some(), tick).await;
            }
            // 宿主没了就结束自己，在途查询随之丢掉——页面 / 进程本来就在退出。
            if !host_alive(&task_ctx) {
                return;
            }
        }
    };
    #[cfg(target_arch = "wasm32")]
    {
        tasks.spawn_wasm(model_worker);
        tasks.spawn_wasm(worker);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        tasks.spawn_tokio(model_worker);
        tasks.spawn_tokio(worker);
    }
    Bridge {
        req: req_tx,
        evt: evt_rx,
        evt_tx: evt_tx_out,
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelLoad, Req, panel_records, require_direct_mirror_endpoint, route_model_load};
    use plant_ui::RefU64;
    use std::sync::mpsc;

    /// 房间体只留在册面板的记录：房间 FRMW 底下别的几何（与不在册的面板）不进房间体。
    #[test]
    fn a_room_volume_keeps_only_registered_panels() {
        let page: serde_json::Value = serde_json::from_str(include_str!(
            "fixtures/pipe26229-deleted-records.json"
        ))
        .unwrap();
        let records: Vec<aios_core::GeomInstQuery> =
            serde_json::from_value(page["items"].clone()).unwrap();
        let first = records[0].refno.refno();
        let expected = records
            .iter()
            .filter(|record| record.refno.refno() == first)
            .count();
        assert!(expected > 0 && expected < records.len(), "夹具里要有别的元素");

        let kept = panel_records(records, &[first]);
        assert_eq!(kept.len(), expected);
        assert!(kept.iter().all(|record| record.refno.refno() == first));
        assert!(panel_records(Vec::new(), &[first]).is_empty());
    }

    /// 房间体的读面：只经 `face.model_instances(`，与眼睛同一条路；房间体这一臂不绕开读面。
    #[test]
    fn the_room_volume_reads_through_the_read_face() {
        let helper = body()
            .split_once("async fn room_volume_records(")
            .expect("房间体取材")
            .1
            .split_once("fn panel_records(")
            .expect("取材之后是过滤")
            .0;
        assert!(helper.contains(".model_instances("), "{helper}");
        assert!(!helper.contains("ensure_model("), "{helper}");
        assert!(!helper.contains("generate_model("), "{helper}");
        assert!(helper.contains("scope_roots: &[room]"), "{helper}");
        assert!(!helper.contains("plant_ui_data::"), "{helper}");
    }

    /// 本文件的非测试部分。源码钉都只看这一半，免得禁单里的字面值被测试自己撞上。
    fn body() -> &'static str {
        let source = include_str!("data.rs");
        source
            .split_once("#[cfg(test)]")
            .map(|(body, _)| body)
            .unwrap_or(source)
    }

    #[test]
    fn only_embedded_transport_absence_is_a_capability_state() {
        let mut health = plant_ui::task_queue::Health::default();
        health.mirror.status = "ready".into();
        health.mirror.endpoint = Some("embedded:mem".into());
        let error = require_direct_mirror_endpoint(&health, "房间").unwrap_err();
        assert!(
            error
                .downcast_ref::<super::MirrorFeatureUnavailable>()
                .is_some()
        );
        health.mirror.endpoint = Some("ws://127.0.0.1:8000".into());
        health.mirror.status = "failed".into();
        let error = require_direct_mirror_endpoint(&health, "房间").unwrap_err();
        assert!(
            error
                .downcast_ref::<super::MirrorFeatureUnavailable>()
                .is_none()
        );
        health.mirror.status = "ready".into();
        assert!(require_direct_mirror_endpoint(&health, "房间").is_ok());
    }

    #[test]
    fn embedded_mirror_never_falls_through_to_a_configured_tcp_store() {
        let mut health = plant_ui::task_queue::Health::default();
        health.mirror.status = "ready".into();
        health.mirror.endpoint = Some("embedded:mem".into());
        let error = require_direct_mirror_endpoint(&health, "房间")
            .unwrap_err()
            .to_string();
        assert!(error.contains("store_mode=mem"), "{error}");
        assert!(error.contains("无数据库端口"), "{error}");
        // 建议的出路必须是现行的两档之一，不能把人引向已退役的 spawned-* 写法。
        assert!(error.contains("rocksdb"), "{error}");
        assert!(!error.contains("spawned"), "{error}");
    }

    /// 属性面板只经读面：不直接打 HTTP 读接口，也不回退到库（ADR-0026）。
    #[test]
    fn property_requests_go_through_the_read_face() {
        let branch = body()
            .split_once("Req::Props(refno) =>")
            .expect("property request branch")
            .1
            .split_once("Req::ElementRooms")
            .expect("next request branch")
            .0;
        assert!(branch.contains("face.props("));
        assert!(!branch.contains("model_update_api::element_attributes"));
        assert!(!body().contains("plant_ui_data::element_props"));
    }

    /// 读面（树 / 属性 / 搜索 / 三维实例 / 工程标识）在这个文件里只有一条路：`ReadFace`。
    /// 直接打 HTTP 读接口或直接查库都是绕过供数模式——树是服务、三维是库就是从这儿漏的。
    /// 命令面（生成 / 更新 / 队列）与房间的镜像门（`service_health`）不在禁单里。
    #[test]
    fn data_rs_reads_only_through_read_face() {
        let body = body();
        for forbidden in [
            "model_update_api::tree_roots(",
            "model_update_api::tree_children(",
            "model_update_api::tree_ancestors(",
            "model_update_api::search_names(",
            "model_update_api::element_attributes(",
            "model_update_api::model_records(",
            "model_update_api::dbnum_report(",
            "plant_ui_data::site_nodes(",
            "plant_ui_data::child_nodes(",
            "plant_ui_data::ancestor_refnos(",
            "plant_ui_data::element_props(",
            "plant_ui_data::resolve_name(",
            "plant_ui_data::search_names_by_prefix(",
            "plant_ui_data::model_instances",
            "plant_ui_data::generated_scope(",
            "plant_ui_data::nouns_of(",
            "plant_ui_data::project_identity(",
        ] {
            assert!(
                !body.contains(forbidden),
                "data.rs 绕过了 ReadFace：{forbidden}"
            );
        }
        // 读面确实在用，不是把调用整个删了。
        for expected in [
            "face.identity()",
            "face.sites()",
            "face.children(",
            "face.ancestors(",
            "face.props(",
            "face.resolve_name(",
            "face.search(",
            "face.refresh_search_index(",
            ".model_instances(",
        ] {
            assert!(body.contains(expected), "data.rs 少了读面调用：{expected}");
        }
    }

    /// 尺寸标注不进读面（计划边界：MBD 永远走模型服务，与命令面同口径）：求解器要的
    /// 成员结点视图只有服务端会拼，库供数下也没有第二条路。这一臂直接打 `mbd_api`，
    /// 地址与命令面同源；哪天有人把它「顺手」搬进 `ReadFace`，这里先红。
    #[test]
    fn pipe_dimensions_go_straight_to_the_model_service() {
        let arm = body()
            .split_once("Req::PipeDimensions { epoch, refno } =>")
            .expect("尺寸标注那一臂")
            .1
            .split_once("Req::Reconnect")
            .expect("下一臂")
            .0;
        assert!(arm.contains("crate::mbd_api::pipe_dimensions("));
        assert!(arm.contains("crate::model_update_api::base_url()"));
        assert!(!arm.contains("face."), "尺寸标注不该经读面：{arm}");
        assert!(arm.contains("Evt::PipeDimensions"));
    }

    /// 库供数的启动序列不等模型服务（D12 / 计划 §5.5 T4）。
    ///
    /// `ready()` 只并发跑当前读面的工程标识与 SITE 根层：库供数下这两条都直连库，
    /// gen-model 不在场照样连得上、树照常展开。`/health` 与 `/dbnums` 只喂队列面板，
    /// 它们失败只让那块面板说「模型服务离线」，不该把启动整个判死——把任何一条搬进
    /// 这个函数，库供数就又被模型服务绑住了。
    #[test]
    fn a_missing_model_service_in_store_mode_does_not_block_ready() {
        let ready = body()
            .split_once("async fn ready(face: &ReadFace)")
            .expect("启动序列")
            .1
            // 顶格的右花括号 = 函数收尾。不写 `\n}\n`：CRLF 检出时那个形状对不上。
            .split_once("\n}")
            .expect("函数体")
            .0;
        assert!(ready.contains("face.identity()"));
        assert!(ready.contains("face.sites()"));
        for forbidden in ["health", "dbnum", "mirror"] {
            assert!(
                !ready.contains(forbidden),
                "启动序列被模型服务绑住了：{forbidden}"
            );
        }
    }

    /// worker 循环里处理 `Req::SwitchReadFace` 的那一臂。
    fn switch_arm() -> &'static str {
        body()
            .split_once("Req::SwitchReadFace(kind) => {")
            .expect("热切那一臂")
            .1
            .split_once("Req::GetWork {")
            .expect("下一臂")
            .0
    }

    /// 热切与重连同席：先排干在途、丢掉旧面的缓存，然后才换面——否则旧面上晚到的
    /// 回包会落在新面后面，而且刚丢掉的缓存会被它填回旧数据。
    #[test]
    fn a_switch_drains_inflight_before_swapping() {
        let arm = switch_arm();
        let drain = arm
            .find("while inflight.next().await.is_some() {}")
            .expect("排干在途");
        let invalidate = arm.find("face.invalidate().await").expect("丢旧面缓存");
        let swap = arm
            .find("face = Arc::new(ReadFace::new(kind))")
            .expect("换面");
        let ready = arm.find("ready(&face).await").expect("新面重跑启动序列");
        assert!(drain < invalidate && invalidate < swap && swap < ready);
    }

    /// 两条通道同拍换面（ADR-0026）：交互通道换了自己那份，还要叫模型通道换它那份，
    /// 而模型通道那头确实接了——漏一条就是「树是库、三维是服务」。
    #[test]
    fn a_switch_swaps_both_lanes() {
        assert!(switch_arm().contains("model_tx.send(ModelLoad::Switch(kind))"));
        let lane = body()
            .split_once("ModelLoad::Switch(kind) => {")
            .expect("模型通道的换面臂")
            .1
            .split_once("ModelLoad::Replace {")
            .expect("下一臂")
            .0;
        assert!(lane.contains("model_face = Arc::new(ReadFace::new(kind))"));
    }

    /// 面只在三处造：`spawn` 造第一份，热切时两条通道各换一份。数据线程别处——尤其
    /// `handle_read`——不许根据错误自行换面（计划默认清单 ⑤）。
    #[test]
    fn a_face_is_only_chosen_at_spawn_or_switch() {
        let body = body();
        assert_eq!(body.matches("ReadFace::new(").count(), 3);
        let handle_read = body
            .split_once("async fn handle_read(")
            .expect("handle_read")
            .1
            .split_once("pub fn spawn(")
            .expect("spawn")
            .0;
        assert!(!handle_read.contains("ReadFace::new("));
    }

    /// 两条 worker 每睡完一拍都得问一句宿主还在不在（`host_alive`），没了就 `return`。
    /// Bevy 退出时 `World::clear_all()` 把 `UpdateTicks` 连同 watch Sender 一起 drop，
    /// `sleep_updates` 从此立刻返回；少一处检查，浏览器里刷新就是一个永不结束的微任务
    /// ——页面无响应（2026-09-18 实机复现）。
    #[test]
    fn every_worker_sleep_is_followed_by_a_host_check() {
        let spawn = body().split_once("pub fn spawn(").expect("spawn").1;
        let (model_lane, worker_lane) = spawn
            .split_once("let worker = move |")
            .expect("交互通道 worker");
        for (lane, name) in [(model_lane, "模型通道"), (worker_lane, "交互通道")] {
            let sleep = lane
                .rfind("task_ctx.sleep_updates(1)")
                .unwrap_or_else(|| panic!("{name}要睡拍"));
            let check = lane
                .rfind("if !host_alive(&task_ctx)")
                .unwrap_or_else(|| panic!("{name}睡完要问宿主"));
            assert!(sleep < check, "{name}：宿主检查要紧跟在睡拍之后");
            let tail = &lane[check..];
            assert!(
                tail.split_once('}')
                    .is_some_and(|(block, _)| block.contains("return;")),
                "{name}：宿主没了就 return，不许接着 loop"
            );
        }
        assert!(
            body().contains("task_ctx.tick_rx.has_changed().is_ok()"),
            "宿主判据只认 watch 通道是否关闭"
        );
    }

    /// 模型通道里取回工作重装那一臂的正文（`ModelLoad::Replace` 的处理，不是枚举定义也不是路由）。
    fn replace_lane() -> &'static str {
        body()
            .split_once("ModelLoad::Switch(kind) => {")
            .expect("模型通道的换面臂")
            .1
            .split_once("ModelLoad::Replace {")
            .expect("Replace 臂")
            .1
            .split_once("let _ = model_evt_tx.send(Evt::Models(debt_reload, result));")
            .expect("Replace 臂收尾")
            .0
    }

    /// 模型通道里眼睛那一臂的正文（`ModelLoad::Scopes` 的处理，不是枚举定义也不是路由）：
    /// 从 `Replace` 臂收尾那一行到模型通道循环的睡眠之间。
    fn scopes_lane() -> &'static str {
        body()
            .split_once("let _ = model_evt_tx.send(Evt::Models(debt_reload, result));")
            .expect("Replace 臂收尾")
            .1
            .split_once("task_ctx.sleep_updates(1).await;")
            .expect("模型通道循环收尾")
            .0
    }

    /// 显示 = 按祖先查已生成的产物，所有显示入口都不调生成接口（ADR-0030 D1，实施约束 1）。
    /// 眼睛那一臂只有一处实例查询、以点下去的目标为祖先，`generate_model` / `ensure_model`
    /// 一个字都不许出现——查空就是空，要生成走右键。交互通道里也没有「为聚焦先生成」
    /// 那一臂（`FocusTreeScope` 同样只查包围盒）。
    #[test]
    fn display_never_generates() {
        let lane = scopes_lane();
        for forbidden in ["generate_model(", "ensure_model(", "/api/v1/model/"] {
            assert!(!lane.contains(forbidden), "眼睛这一臂在生成：{forbidden}");
        }
        assert_eq!(
            lane.matches(".model_instances(").count(),
            1,
            "实例查询在这一臂只该有一处"
        );
        assert!(
            lane.contains("roots: &[target],") && lane.contains("scope_roots: &[target],"),
            "两面都以点下去的目标为祖先查：{lane}"
        );
        assert!(
            !body().contains("EnsureForFocus"),
            "聚焦不许为显示先生成一次"
        );
        let bounds = body()
            .split_once("Req::SubtreeBounds { epoch, target } =>")
            .expect("聚焦查包围盒那一臂")
            .1
            .split_once("Req::Children(refno) =>")
            .expect("下一臂")
            .0;
        assert!(bounds.contains("face.subtree_bounds("));
        for forbidden in ["generate_model(", "ensure_model("] {
            assert!(!bounds.contains(forbidden), "聚焦在生成：{forbidden}");
        }
    }

    /// 取回工作重装只重查、不先生成（ADR-0030 D3，实施约束 4）：服务供数以快照里的范围目标
    /// 为祖先问 records（一个 ZONE 一次往返），老快照没有范围记录就退到按模型 refno 问；
    /// 库供数照旧只查 `roots`。
    #[test]
    fn get_work_reloads_without_generating() {
        let lane = replace_lane();
        for forbidden in [
            "generate_model(",
            "ensure_model(",
            "/api/v1/model/",
            "Evt::ReloadEnsured",
        ] {
            assert!(!lane.contains(forbidden), "重装在生成：{forbidden}");
        }
        assert_eq!(lane.matches(".model_instances(").count(), 1);
        assert!(lane.contains("let scope_roots = if scope_targets.is_empty() {"));
        assert!(lane.contains("roots: &roots,") && lane.contains("scope_roots: &scope_roots,"));
    }

    fn reload(roots: Vec<RefU64>, scope_targets: Vec<RefU64>, debt_reload: bool) -> Req {
        Req::Models {
            roots,
            scope_targets,
            debt_reload,
            base: "http://127.0.0.1:8022".into(),
            project: "SAM".into(),
            mdb: "/MDB".into(),
            namespace: "plant".into(),
        }
    }

    #[test]
    fn model_load_uses_dedicated_lane() {
        let (model_tx, model_rx) = mpsc::channel();
        assert!(matches!(
            route_model_load(
                reload(vec![RefU64::default()], Vec::new(), false),
                &model_tx
            ),
            Ok(None)
        ));
        assert!(matches!(
            model_rx.try_recv(),
            Ok(ModelLoad::Replace { roots, scope_targets, debt_reload: false, .. })
                if roots == vec![RefU64::default()] && scope_targets.is_empty()
        ));
        let target = RefU64::from(42);
        assert!(matches!(
            route_model_load(
                Req::ModelScopes {
                    epoch: 7,
                    targets: vec![target],
                    base: "http://127.0.0.1:8022".into(),
                    project: "SAM".into(),
                    mdb: "/MDB".into(),
                    namespace: "plant".into(),
                },
                &model_tx
            ),
            Ok(None)
        ));
        assert!(matches!(
            model_rx.try_recv(),
            Ok(ModelLoad::Scopes { epoch: 7, targets, base, project, mdb, namespace })
                if targets == vec![target]
                    && base == "http://127.0.0.1:8022"
                    && project == "SAM"
                    && mdb == "/MDB"
                    && namespace == "plant"
        ));
        assert!(matches!(
            route_model_load(Req::Props(RefU64::default()), &model_tx),
            Ok(Some(Req::Props(_)))
        ));
        assert!(matches!(
            route_model_load(Req::Children(RefU64::default()), &model_tx),
            Ok(Some(Req::Children(_)))
        ));

        drop(model_rx);
        assert!(matches!(
            route_model_load(
                Req::ModelScopes {
                    epoch: 8,
                    targets: vec![target],
                    base: "http://127.0.0.1:8022".into(),
                    project: "SAM".into(),
                    mdb: "/MDB".into(),
                    namespace: "plant".into(),
                },
                &model_tx
            ),
            Err(ModelLoad::Scopes { epoch: 8, targets, .. }) if targets == vec![target]
        ));
    }

    /// 取回工作的重装请求要把范围目标与服务身份一起带到模型通道上（ADR-0030 D3）：
    /// records 在数据线程里发，它不认识宿主的设置项，四个字段缺一个就打不出请求。
    /// 通道停了也要把 `debt_reload` 原样报回去——欠账恢复靠它。
    #[test]
    fn a_reload_carries_its_scope_targets_and_identity_to_the_model_lane() {
        let (model_tx, model_rx) = mpsc::channel();
        let zone = RefU64::from(20);
        let bran = RefU64::from(7);
        assert!(matches!(
            route_model_load(
                reload(vec![RefU64::from(1)], vec![bran, zone], true),
                &model_tx
            ),
            Ok(None)
        ));
        assert!(matches!(
            model_rx.try_recv(),
            Ok(ModelLoad::Replace { roots, scope_targets, debt_reload: true, base, project, mdb, namespace })
                if roots == vec![RefU64::from(1)]
                    && scope_targets == vec![bran, zone]
                    && base == "http://127.0.0.1:8022"
                    && project == "SAM"
                    && mdb == "/MDB"
                    && namespace == "plant"
        ));

        drop(model_rx);
        assert!(matches!(
            route_model_load(reload(vec![RefU64::from(1)], vec![zone], true), &model_tx),
            Err(ModelLoad::Replace {
                debt_reload: true,
                ..
            })
        ));
    }
}
