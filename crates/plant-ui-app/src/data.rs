//! UI 线程与数据线程（tokio）之间的桥。
//!
//! eframe 的绘制是同步的，而 SUL_DB 全是 async：这里起一个常驻数据线程，
//! 请求经 channel 进、结果经 channel 出，UI 每帧非阻塞地收。每个结果落地后
//! `request_repaint`，免得空闲中的 UI 睡过结果。

use std::collections::HashSet;
use std::future::Future;
use std::sync::mpsc;

use futures::stream::{FuturesUnordered, StreamExt};
use plant_ui::data_publish::PublishRequest;
use plant_ui::model_update::{Enqueued, Preview, ProgressEvent};
use plant_ui::task_queue::Poll as QueuePoll;
use plant_ui_data::{EleTreeNode, RefU64};

use crate::search_index::{Scope, SearchIndex, SearchIndexState, SubstringHits};

/// 搜索一次最多带回多少条。下拉本来就只列得下十几行，多取的部分只是让包含匹配
/// 那条慢路多扫一会儿。取满这个数就等于「后面还有」，界面据此提示缩小范围。
pub const SEARCH_LIMIT: usize = 20;

/// Direct is the production default; `db` restores the previous SurrealDB tree reads.
fn direct_tree_enabled() -> bool {
    direct_tree_mode(std::env::var("PLANT_TREE_DATA_MODE").ok().as_deref())
}

fn direct_tree_mode(raw: Option<&str>) -> bool {
    !matches!(
        raw.unwrap_or("direct").trim().to_ascii_lowercase().as_str(),
        "db" | "surreal" | "surrealdb"
    )
}

async fn tree_sites() -> anyhow::Result<Vec<EleTreeNode>> {
    if direct_tree_enabled() {
        crate::model_update_api::tree_roots(&crate::model_update_api::base_url()).await
    } else {
        plant_ui_data::site_nodes().await
    }
}

async fn tree_children(refno: RefU64) -> anyhow::Result<Vec<EleTreeNode>> {
    if direct_tree_enabled() {
        crate::model_update_api::tree_children(&crate::model_update_api::base_url(), refno).await
    } else {
        plant_ui_data::child_nodes(refno.into()).await
    }
}

async fn tree_ancestors(refno: RefU64) -> anyhow::Result<Vec<RefU64>> {
    if direct_tree_enabled() {
        crate::model_update_api::tree_ancestors(&crate::model_update_api::base_url(), refno).await
    } else {
        plant_ui_data::ancestor_refnos(refno.into()).await
    }
}

pub enum Req {
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
    /// 房间浏览器的全表（重查询，全库扫描级；只在打开浮窗或手动刷新时发，
    /// 不进启动路径——计划风险 5）。
    RoomsOverview,
    /// 取回工作的三维重装：先让 `ensure_targets` 这些范围目标的模型追到文件最新
    /// （与 eye 同一条 `ensure` 路，ADR-0024），再加载 `roots` 这些模型 refno 下已经
    /// 生成的几何实例。`ensure_targets` 为空 = 跳过 ensure，按上次产物重装。
    Models {
        roots: Vec<RefU64>,
        ensure_targets: Vec<RefU64>,
        /// 这次重装是不是在清偿「数据已应用、三维欠着」那笔账（`Evt::Models` 原样带回）。
        debt_reload: bool,
        base: String,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// eye 显示路径：先让 Web API 确保节点范围的全部生成根与最新数据一致，再查模型。
    ModelScopes {
        epoch: u64,
        targets: Vec<RefU64>,
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
    /// 「重新生成模型」的清点：这些根底下已经生成过多少元素、归成多少个生成单元。
    ///
    /// **只读**，删除不在这条路上。名词表由 UI 侧从 `/health` 取好交下来——
    /// 数据线程不认识模型服务，也不该为了一个名单去认识它。
    RegenerateScope {
        epoch: u64,
        targets: Vec<RefU64>,
        /// 已折大写的交付单元名词。空表在 UI 侧就被拦下了，到不了这里。
        delivery_units: Vec<String>,
    },
    /// 「整片删一次」：把右键那几行的精确子树下已经生成的模型产物删掉。
    ///
    /// 删的是产物不是本体，`pe` 一行不动。一次请求删完全部落点——中途停不下来，
    /// 也不该停：删了一半的范围既不是旧样子也不是新样子。
    RegenerateDelete {
        epoch: u64,
        base: String,
        targets: Vec<RefU64>,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 逐根重做里的一个。**一次一条**，由宿主收到回执后再派下一条——
    /// 「停在这里」停的正是这个派发动作。
    RegenerateUnit {
        epoch: u64,
        /// 这是根列表里的第几个。回执带回来，宿主据此接着往下走。
        index: usize,
        base: String,
        refno: RefU64,
        project: String,
        mdb: String,
        namespace: String,
    },
    /// 树定位目标的祖先链。目标不在已加载的树里时才发，见 ADR-0014。
    Ancestors(RefU64),
    /// 丢查询缓存并重跑启动序列。连库失败后命令行上的「重试」走这条，结果仍走
    /// `Evt::Ready`。
    Reconnect,
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
}

/// 一次清点的结果：确认框要摆的那两个数字，外加确认之后要逐个重做的那份名单。
///
/// 名单**必须在这一刻定死**：它是从 `inst_relate` 上数出来的，删完那张表上
/// 没有它们了，再也算不回来。
pub struct RegenerateCount {
    /// `inst_relate` + `tubi_relate` 上属于这个范围的已生成元素数。
    pub elements: usize,
    /// 归并出来的生成根，条数是**上限**（见 `regenerate::regeneration_roots`）。
    pub roots: Vec<RefU64>,
}

/// 一次取回工作重新查回来的那部分树。
pub struct GetWork {
    pub sites: Vec<EleTreeNode>,
    pub reload_models: bool,
    /// 重查成功的分支及其新的直接子层。
    pub branches: Vec<(RefU64, Vec<EleTreeNode>)>,
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
    /// 开始读取根层的时刻。首次队列快照只补这之后完成的批次，避免启动期间漏刷新。
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub sites: Vec<EleTreeNode>,
}

pub enum Evt {
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
    /// 房间浏览器全表。
    RoomsOverview(anyhow::Result<Vec<plant_ui_data::room::RoomOverviewRow>>),
    Models(bool, anyhow::Result<Vec<aios_core::GeomInstQuery>>),
    /// 取回工作重装前对一个范围目标的 ensure 回执。成败都发；失败不阻断随后的重查。
    ReloadEnsured {
        target: RefU64,
        result: anyhow::Result<crate::model_update_api::EnsureReply>,
    },
    /// 取回工作重查模型的进度（每根一步）。
    ReloadProgress {
        done: usize,
        total: usize,
    },
    ModelScopeProgress {
        epoch: u64,
        target: RefU64,
        done: usize,
        total: usize,
    },
    ModelScopeEnsured {
        epoch: u64,
        target: RefU64,
        result: crate::model_update_api::EnsureReply,
    },
    ModelScope(u64, RefU64, anyhow::Result<Vec<aios_core::GeomInstQuery>>),
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
    /// 一次清点的结果。`epoch` 认帧：确认框换过目标之后，旧的那份数字贴上去
    /// 就成了另一个范围的账。
    RegenerateScope {
        epoch: u64,
        result: anyhow::Result<RegenerateCount>,
    },
    /// 「整片删一次」的回执。失败就整趟停在这里——删都删不动，往下发 ensure
    /// 只会在没删干净的范围上重做，结果谁也说不清。
    ///
    /// `deleted` 是**真删掉了几个落点**。失败时它多半不是零：前面几片已经空了，
    /// 而那件事必须说出来，不然人以为「失败 = 什么都没动」。
    RegenerateDeleted {
        epoch: u64,
        deleted: usize,
        result: anyhow::Result<()>,
    },
    /// 一个生成根的回执。`index` 原样带回：宿主据此接着派下一个。
    RegenerateUnit {
        epoch: u64,
        index: usize,
        result: anyhow::Result<crate::model_update_api::EnsureReply>,
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
    /// 取回工作的整批结果。根层查不动就是整次失败——根层没了树无从谈起。
    GetWork(anyhow::Result<GetWork>),
    /// 队列面板的一次轮询结果。四份数据一起换代，不留自相矛盾的中间态。
    QueuePoll(anyhow::Result<QueuePoll>),
    /// 暂停 / 恢复的回执。真值仍以下一次轮询的快照为准，这里只负责把失败说出来。
    QueueSetPaused(bool, anyhow::Result<()>),
    /// 复活死信的回执，带上是哪一个根。真值同样等下一拍快照。
    RetryPendingUnit(String, anyhow::Result<()>),
    DataPublish(PublishRequest, anyhow::Result<String>),
    CommandQuery {
        epoch: u64,
        label: String,
        result: anyhow::Result<crate::model_update_api::QueryReply>,
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
        ensure_targets: Vec<RefU64>,
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
}

fn route_model_load(req: Req, tx: &mpsc::Sender<ModelLoad>) -> Result<Option<Req>, ModelLoad> {
    match req {
        Req::Models {
            roots,
            ensure_targets,
            debt_reload,
            base,
            project,
            mdb,
            namespace,
        } => tx
            .send(ModelLoad::Replace {
                roots,
                ensure_targets,
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
/// 原样把旧的那份还回来，界面看着刷新过了、内容一个字没变。
async fn get_work(branches: &[RefU64], reload_models: bool) -> anyhow::Result<GetWork> {
    plant_ui_data::invalidate_all().await;
    let sites = tree_sites().await?;
    let mut loaded = Vec::with_capacity(branches.len());
    let mut failed = Vec::new();
    for refno in branches {
        match tree_children(*refno).await {
            Ok(kids) => loaded.push((*refno, kids)),
            Err(error) => failed.push((*refno, crate::logs::error_chain(&error))),
        }
    }
    Ok(GetWork {
        sites,
        reload_models,
        branches: loaded,
        failed,
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
async fn count_regeneration(
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

/// 启动序列：连库、抓工程标识、抓 SITE 根层。三步任一失败都算没连上。
async fn ready() -> anyhow::Result<ReadyInfo> {
    plant_ui_data::connect().await?;
    let (project, mdb, ns, db_nums) = plant_ui_data::project_identity().await?;
    let observed_at = chrono::Utc::now();
    let sites = tree_sites().await?;
    Ok(ReadyInfo {
        project,
        mdb,
        ns,
        db_nums,
        observed_at,
        sites,
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

/// 并发通道里的一次交互读：查询、回执、叫醒 UI，整段自包含。
///
/// 这里只放**互相独立**的读与 HTTP 往返——它们在 [`FuturesUnordered`] 里并发
/// 推进，树的子层查询不再排在属性 / 归属 / 队列轮询后面（双击展开时第一击
/// 触发的两条选中查询曾把 Children 压在队尾）。晚到回包由 UI 侧的陈旧门
/// 丢弃，完成顺序无所谓。全局手术（Reconnect / GetWork）不进这条道：它们
/// 要独占（见 worker 循环），并发会让晚到的读把刚失效的缓存填回旧数据。
async fn handle_read(
    req: Req,
    index: SearchIndex,
    scope: Scope,
    evt_tx: mpsc::Sender<Evt>,
    ctx: egui::Context,
) {
    match req {
        Req::Children(refno) => {
            let r = tree_children(refno).await;
            let _ = evt_tx.send(Evt::Children(refno, r));
        }
        Req::Props(refno) => {
            let r = plant_ui_data::element_props(refno.into()).await;
            let _ = evt_tx.send(Evt::Props(refno, r));
        }
        Req::ElementRooms(refno) => {
            let r = plant_ui_data::room::element_rooms(refno.into()).await;
            let _ = evt_tx.send(Evt::ElementRooms(refno, r));
        }
        Req::PanelRoom(refno) => {
            let r = plant_ui_data::room::panel_room(refno.into()).await;
            let _ = evt_tx.send(Evt::PanelRoom(refno, r));
        }
        Req::RoomPanels(refno) => {
            let r = plant_ui_data::room::room_panels(refno.into()).await;
            let _ = evt_tx.send(Evt::RoomPanels(refno, r));
        }
        Req::RoomDetail(refno) => {
            // 成员预览条数与「房间」页签的列表容量对齐；隔离 / 取景
            // 用的是 member_refnos 全量，不受这个数约束。
            let r = plant_ui_data::room::room_detail(refno.into(), 8).await;
            let _ = evt_tx.send(Evt::RoomDetail(refno, r));
        }
        Req::RoomsOverview => {
            let r = plant_ui_data::room::rooms_overview().await;
            let _ = evt_tx.send(Evt::RoomsOverview(r));
        }
        Req::ResolveName(name) => {
            let result = plant_ui_data::resolve_name(&name).await;
            let _ = evt_tx.send(Evt::ResolvedName(name, result));
        }
        Req::SearchElements { epoch, query } => {
            // 前缀先打库（15.8ms），回来之后再查索引——子串是同步的亚毫秒查询，
            // 排在后面既不多花时间，还能用上这期间可能刚换代的新索引。
            let prefix = plant_ui_data::search_names_by_prefix(&query, SEARCH_LIMIT).await;
            let substring = match index.search(&query, SEARCH_LIMIT) {
                Ok(hits) => hits,
                Err(error) => {
                    // 查询炸了要说出来，否则界面只表现为「子串一条都没有」。
                    let _ = evt_tx.send(Evt::SearchIndex(SearchIndexState::Failed(
                        crate::logs::error_chain(&error),
                    )));
                    SubstringHits::Unavailable
                }
            };
            let _ = evt_tx.send(Evt::SearchElements {
                epoch,
                query,
                prefix,
                substring,
            });
        }
        Req::RegenerateScope {
            epoch,
            targets,
            delivery_units,
        } => {
            let result = count_regeneration(&targets, &delivery_units).await;
            let _ = evt_tx.send(Evt::RegenerateScope { epoch, result });
        }
        Req::RegenerateDelete {
            epoch,
            base,
            targets,
            project,
            mdb,
            namespace,
        } => {
            let mut result = Ok(());
            let mut deleted = 0usize;
            for target in &targets {
                // 线上一律斜杠形（`24381/100677`）：`RefU64` 的 Display 是下划线
                // 形，服务端自己吐出来的 root_refno 从来都是斜杠形。
                let refno = target.to_slash_string();
                result = crate::model_update_api::delete_model_subtree(
                    &base, &refno, &project, &mdb, &namespace,
                )
                .await;
                // 一个落点删不动就别再删下一个：整趟本来就要停，多删一片只是
                // 多毁一片没人会去重做的模型。
                if result.is_err() {
                    break;
                }
                deleted += 1;
            }
            let _ = evt_tx.send(Evt::RegenerateDeleted {
                epoch,
                deleted,
                result,
            });
        }
        Req::RegenerateUnit {
            epoch,
            index,
            base,
            refno,
            project,
            mdb,
            namespace,
        } => {
            // `force = false`：删除已经把这一片清空了，第一个元素触发真生成，
            // 同根后面的读到已有产物直接回 AlreadyAvailable——嵌套单元的去重
            // 是服务端免费给的，客户端不自己裁剪。
            let result = crate::model_update_api::ensure_model(
                &base,
                &refno.to_slash_string(),
                false,
                &project,
                &mdb,
                &namespace,
            )
            .await;
            let _ = evt_tx.send(Evt::RegenerateUnit {
                epoch,
                index,
                result,
            });
        }
        Req::CheckSearchIndex => index.refresh(scope, false, evt_tx, ctx.clone()).await,
        Req::RebuildSearchIndex => index.refresh(scope, true, evt_tx, ctx.clone()).await,
        Req::Ancestors(refno) => {
            let result = tree_ancestors(refno).await;
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
        Req::Reconnect | Req::GetWork { .. } => {
            unreachable!("全局手术在 worker 循环里独占处理")
        }
        Req::Models { .. } | Req::ModelScopes { .. } => {
            unreachable!("模型请求已路由到专用任务")
        }
    }
    ctx.request_repaint();
}

/// 起数据线程：连库、抓工程标识与 SITE 根层，然后循环处理懒加载请求。
pub fn spawn(ctx: egui::Context, tasks: &bevy_wasm_tasks::Tasks<'_>) -> Bridge {
    let (req_tx, req_rx) = mpsc::channel();
    let (model_tx, model_rx) = mpsc::channel::<ModelLoad>();
    let (evt_tx, evt_rx) = mpsc::channel();
    let evt_tx_out = evt_tx.clone();
    let model_evt_tx = evt_tx.clone();
    let model_ctx = ctx.clone();
    let model_worker = move |mut task_ctx: bevy_wasm_tasks::TaskContext| async move {
        loop {
            while let Ok(load) = model_rx.try_recv() {
                match load {
                    ModelLoad::Replace {
                        roots,
                        ensure_targets,
                        debt_reload,
                        base,
                        project,
                        mdb,
                        namespace,
                    } => {
                        // sim 模式没有 SurrealDB 也没有网格文件：模型通道短路成
                        // 空结果，三维视口保持空场景，树与队列照常演。
                        let result = if crate::sim::enabled() {
                            Ok(Vec::new())
                        } else {
                            // 先让范围目标的模型追到文件最新（ADR-0024）：与 eye 那条
                            // 路同一个 `ensure_model(force = false)`——凭证当前的根服务端
                            // 直接算命中，只有被改到的根真重算。顺序做：一个范围可能
                            // 就是整个 ZONE，并发只会让服务端的 per-dbnum 锁互相撞。
                            // 失败不中止：空场景比旧几何更坏，重查照跑，回执里说清。
                            for target in &ensure_targets {
                                let result = crate::model_update_api::ensure_model(
                                    &base,
                                    &target.to_slash_string(),
                                    false,
                                    &project,
                                    &mdb,
                                    &namespace,
                                )
                                .await;
                                let _ = model_evt_tx.send(Evt::ReloadEnsured {
                                    target: *target,
                                    result,
                                });
                                model_ctx.request_repaint();
                            }
                            let progress_tx = model_evt_tx.clone();
                            plant_ui_data::model_instances_with_progress(&roots, |done, total| {
                                let _ = progress_tx.send(Evt::ReloadProgress { done, total });
                                model_ctx.request_repaint();
                            })
                            .await
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
                                    Ok(Vec::new()),
                                ));
                                model_ctx.request_repaint();
                                continue;
                            }
                            let ensured = crate::model_update_api::ensure_model(
                                &base,
                                &target.to_slash_string(),
                                false,
                                &project,
                                &mdb,
                                &namespace,
                            )
                            .await;
                            match ensured {
                                Ok(reply) => {
                                    let _ = model_evt_tx.send(Evt::ModelScopeEnsured {
                                        epoch,
                                        target,
                                        result: reply,
                                    });
                                    model_ctx.request_repaint();
                                }
                                Err(error) => {
                                    let _ = model_evt_tx.send(Evt::ModelScope(
                                        epoch,
                                        target,
                                        Err(error),
                                    ));
                                    model_ctx.request_repaint();
                                    continue;
                                }
                            }
                            let progress_tx = model_evt_tx.clone();
                            let result = plant_ui_data::model_instances_with_progress(
                                &[target],
                                |done, total| {
                                    let _ = progress_tx.send(Evt::ModelScopeProgress {
                                        epoch,
                                        target,
                                        done,
                                        total,
                                    });
                                    model_ctx.request_repaint();
                                },
                            )
                            .await;
                            let _ = model_evt_tx.send(Evt::ModelScope(epoch, target, result));
                            model_ctx.request_repaint();
                        }
                    }
                }
            }
            task_ctx.sleep_updates(1).await;
        }
    };
    let worker = move |mut task_ctx: bevy_wasm_tasks::TaskContext| async move {
        // sim 模式：整个数据线程改由进程内引擎供数——假身份直接 Ready，
        // 后续请求全部路由到剧本状态机，WS 同形明细由 pump 推送。
        let mut sim_engine = crate::sim::Engine::from_env();
        // 子串索引的把手与它的取材范围。范围跟着 `Ready` 走——重连之后当前 MDB
        // 可能就不是原来那个了，索引也得跟着换目录。
        let index = SearchIndex::default();
        let mut scope = Scope::default();
        let started = match sim_engine.as_ref() {
            Some(engine) => Ok(engine.ready_info()),
            None => ready().await,
        };
        if let Ok(info) = started.as_ref() {
            scope = Scope {
                ns: info.ns.clone(),
                mdb: info.mdb.clone(),
                dbnums: info.db_nums.clone(),
            };
        }
        let _ = evt_tx.send(Evt::Ready(started));
        ctx.request_repaint();

        // 在途的交互读。FuturesUnordered 让它们在同一个任务里**并发**推进：
        // 不需要按请求 spawn（wasm 上也没有 tokio::spawn 可用），一条慢查询
        // 也不再把整条桥堵成串行（模型加载在此之前就已单独分道）。
        let mut inflight: FuturesUnordered<InflightQuery> = FuturesUnordered::new();
        // 启动那一次校验：与后面的触发点走同一条路（单飞去重），只是没人替它
        // 发 Req——界面还没起来。sim 模式下索引这一路不存在，它自己回「就绪」。
        if sim_engine.is_none() {
            inflight.push(boxed_query(index.clone().refresh(
                scope.clone(),
                false,
                evt_tx.clone(),
                ctx.clone(),
            )));
        }
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
                        let reconnected = ready().await;
                        if let Ok(info) = reconnected.as_ref() {
                            scope = Scope {
                                ns: info.ns.clone(),
                                mdb: info.mdb.clone(),
                                dbnums: info.db_nums.clone(),
                            };
                        }
                        let _ = evt_tx.send(Evt::Ready(reconnected));
                        ctx.request_repaint();
                        inflight.push(boxed_query(index.clone().refresh(
                            scope.clone(),
                            false,
                            evt_tx.clone(),
                            ctx.clone(),
                        )));
                    }
                    Req::GetWork {
                        branches,
                        reload_models,
                    } => {
                        while inflight.next().await.is_some() {}
                        let result = get_work(&branches, reload_models).await;
                        let _ = evt_tx.send(Evt::GetWork(result));
                        ctx.request_repaint();
                        // 取回工作刚把数据换过一批，戳多半已经不一样了。
                        inflight.push(boxed_query(index.clone().refresh(
                            scope.clone(),
                            false,
                            evt_tx.clone(),
                            ctx.clone(),
                        )));
                    }
                    // 其余都是互相独立的交互读 / HTTP 往返，进并发道。树的子层
                    // 查询从此不再排在属性 / 归属 / 队列轮询后面。
                    req => inflight.push(boxed_query(handle_read(
                        req,
                        index.clone(),
                        scope.clone(),
                        evt_tx.clone(),
                        ctx.clone(),
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
    use super::{ModelLoad, Req, direct_tree_mode, route_model_load};
    use plant_ui::RefU64;
    use std::sync::mpsc;

    fn reload(roots: Vec<RefU64>, ensure_targets: Vec<RefU64>, debt_reload: bool) -> Req {
        Req::Models {
            roots,
            ensure_targets,
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
            Ok(ModelLoad::Replace { roots, ensure_targets, debt_reload: false, .. })
                if roots == vec![RefU64::default()] && ensure_targets.is_empty()
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

    /// 取回工作的重装请求要把范围目标与服务身份一起带到模型通道上（ADR-0024）：
    /// ensure 在数据线程里发，它不认识宿主的设置项，四个字段缺一个就打不出请求。
    /// 通道停了也要把 `debt_reload` 原样报回去——欠账恢复靠它。
    #[test]
    fn a_reload_carries_its_ensure_targets_and_identity_to_the_model_lane() {
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
            Ok(ModelLoad::Replace { roots, ensure_targets, debt_reload: true, base, project, mdb, namespace })
                if roots == vec![RefU64::from(1)]
                    && ensure_targets == vec![bran, zone]
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

    #[test]
    fn direct_tree_is_default_and_db_is_an_explicit_rollback() {
        assert!(direct_tree_mode(None));
        assert!(direct_tree_mode(Some("direct")));
        assert!(!direct_tree_mode(Some("db")));
        assert!(!direct_tree_mode(Some(" SurrealDB ")));
    }
}
