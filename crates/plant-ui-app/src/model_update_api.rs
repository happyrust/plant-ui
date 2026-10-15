use anyhow::Context;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

#[path = "model_record_union.rs"]
mod model_record_union;

use plant_ui::model_update::{Enqueued, Failure, Preview};
use plant_ui::task_queue::{
    DbnumReport, Health, ModelSource, PendingUnits, Poll, QueueSnapshot, ResidentProgress,
    TaskList,
};
use plant_ui_data::{Attr, AttrKind};

/// 服务端的统一错误包封 `{ code, message, detail }`（`web_service/mod.rs` 的 `ApiError`）。
///
/// 原样带上 `code` 交出去：界面按 `code` 分型给出路（S2-D），**不解析 message 字符串**。
/// 连不上与超时在客户端这一侧也归进 `timeout`——对用的人来说是同一件事，出路也一样。
#[derive(Debug)]
pub struct ApiError(pub Failure);

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}：{}", self.0.code, self.0.message)
    }
}

impl std::error::Error for ApiError {}

/// 从 `anyhow` 链上把结构化失败取回来；取不到就归到 internal，
/// 界面那一类会把原始 message 收进「详情」默认折起。
pub fn failure_of(error: &anyhow::Error) -> Failure {
    error
        .chain()
        .find_map(|e| e.downcast_ref::<ApiError>())
        .map(|e| e.0.clone())
        .unwrap_or_else(|| Failure::new("internal", crate::logs::error_chain(error)))
}

/// 解析模型服务地址。优先级：S6 设置项 > 环境变量 > 出厂默认，
/// 这里负责后两级，设置项由 `settings::State::adopt` 之后的保存覆盖。
pub fn base_url() -> String {
    BASE_URL
        .get()
        .cloned()
        .or_else(|| std::env::var("PLANT_MODEL_API_URL").ok())
        .unwrap_or_else(|| plant_ui::settings::DEFAULT_MODEL_API_URL.into())
        .trim_end_matches('/')
        .to_owned()
}

static BASE_URL: OnceLock<String> = OnceLock::new();

/// 只有 wasm 的 `start()` 会调它——地址由宿主页面注入，钉死一次不再变。原生端
/// 不设这一格，让 [`base_url`] 保持「设置项 > 环境变量 > 出厂默认」那条优先级。
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn set_base_url(base: String) -> anyhow::Result<()> {
    BASE_URL
        .set(base.trim_end_matches('/').to_owned())
        .map_err(|_| anyhow::anyhow!("模型服务地址已初始化，不能重复覆盖"))
}

/// 预览。`mdb` 决定本期执行范围——服务端照它解出「当前 MDB 声明的 DESI 库号」
/// 作为扫描白名单。**由客户端给**：范围既然由 MDB 定，界面显示的范围与服务端
/// 真跑的范围就必须同源；让服务端读自己那份 `DbOption.toml`，两边配置一错开
/// 就是静默的。
pub async fn preview(
    base: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<Preview> {
    post(
        base,
        "/api/v1/update/preview",
        serde_json::json!({ "project": project, "mdb": mdb, "namespace": namespace }).to_string(),
        Duration::from_secs(600),
    )
    .await
}

/// 扫描 + 入队。合流之后它**一律入队**，回执是入队的批次数组而不是单个 task_id
/// ——进度去任务队列视图看（ADR-0011）。范围口径与 [`preview`] 同源。
///
/// `dbnums` 是勾选集折算的范围内子集（gen-model ADR-020）：`None` 不发该字段，
/// 服务端按全范围走（老服务端也认识这份请求体）；`Some` 时未勾选的库不入队、
/// 水位不动，范围外的号被服务端拒进回执 warnings。
pub async fn execute(
    base: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
    dbnums: Option<&[u32]>,
) -> anyhow::Result<Enqueued> {
    post(
        base,
        "/api/v1/update/execute",
        execute_body(project, mdb, namespace, dbnums),
        Duration::from_secs(60),
    )
    .await
}

/// `POST /api/v1/dbnums/{dbnum}/model/rebuild` 的 202 回执（gen-model spec §4.5.1；
/// `model_rebuild.rs::ModelRebuildReceipt`）。同库已有在飞重建时服务端回同一个 `task_id`。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ModelRebuildReceipt {
    pub task_id: String,
    pub dbnum: u32,
    /// 应重算的生成根数——服务端开工时数出来的，比 `/dbnums` 四格拼的粗和准；界面上以它为准。
    #[serde(default)]
    pub expected_roots: usize,
    /// 开工时作废掉的陈旧增量行数（R6）。老服务端不给，缺省 0。
    #[serde(default)]
    pub discarded_pending: u64,
    #[serde(default)]
    pub state: String,
}

impl ModelRebuildReceipt {
    /// 日志那一行。
    pub fn summary(&self) -> String {
        let mut line = format!(
            "已提交重算模型（整库）db{}：任务 {}，应重算 {} 根",
            self.dbnum, self.task_id, self.expected_roots
        );
        if self.discarded_pending > 0 {
            line.push_str(&format!("，作废旧增量行 {} 条", self.discarded_pending));
        }
        line.push_str("；进度看任务队列里它那一行");
        line
    }
}

/// 整库模型重建：把该库全部权威生成根投进模型队列重算（2026-09-10 重新计算计划 R1）。
///
/// 不改数据水位、不预删旧模型；重建期间同库 `model/generate` 回 409。`watch_scope` 外 400；
/// 服务端 `model_full_rebuild_enabled=false` 时同样是一条失败包封（message 原文带那个键名，
/// 界面照 `code` 分型、原文进日志，不解析它）。两代并存：老服务端没这条路由 → 404，
/// 调用方按「这一档服务端没有」处理，不重试。
pub async fn model_rebuild(
    base: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
    dbnum: u32,
) -> anyhow::Result<ModelRebuildReceipt> {
    post(
        base,
        &format!("/api/v1/dbnums/{dbnum}/model/rebuild"),
        serde_json::json!({ "project": project, "mdb": mdb, "namespace": namespace }).to_string(),
        Duration::from_secs(60),
    )
    .await
}

/// `DELETE /api/v1/dbnums/{dbnum}/model` 的 200 回执（gen-model spec §4.5.1D）。
///
/// `ref0s` 缺席 = 老服务端没给这一格，不能据此卸载三维；空数组是「这个库没有 Ref0」。
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct DbnumModelCleanupReceipt {
    pub dbnum: u32,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub ref0s: Option<Vec<u32>>,
    #[serde(default)]
    pub range_statements: usize,
    #[serde(default)]
    pub elapsed_ms: u64,
    #[serde(default)]
    pub model_source_after: Option<String>,
}

/// 整库模型清理：只删这个库的模型面，不重算（清理模型计划 C5）。
///
/// 复用 [`delete`]：无 body、超时 125 秒。`confirm` 填路径上的同一个 dbnum。老服务端没有
/// 这条路由时是 404，调用方按 [`endpoint_missing`] 降级成一句 INFO。
pub async fn delete_dbnum_model(
    base: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
    dbnum: u32,
) -> anyhow::Result<DbnumModelCleanupReceipt> {
    let path = dbnum_model_path(dbnum, project, mdb, namespace);
    delete(base, &path, Duration::from_secs(125)).await
}

fn dbnum_model_path(dbnum: u32, project: &str, mdb: &str, namespace: &str) -> String {
    format!(
        "/api/v1/dbnums/{dbnum}/model?confirm={dbnum}&project={}&mdb={}&namespace={}",
        urlencode(project),
        urlencode(mdb),
        urlencode(namespace)
    )
}

#[derive(Debug, Deserialize)]
pub struct QueryReply {
    pub tool: String,
    pub result: Value,
}

/// Execute one fixed read-only E3D/model query through the configured model service.
pub async fn query(
    base: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
    tool: &str,
    arguments: Value,
) -> anyhow::Result<QueryReply> {
    post(
        base,
        "/api/v1/query",
        serde_json::json!({
            "project": project,
            "mdb": mdb,
            "namespace": namespace,
            "tool": tool,
            "arguments": arguments,
        })
        .to_string(),
        Duration::from_secs(1220),
    )
    .await
}

#[derive(Debug, Deserialize)]
struct TreeNodesReply {
    source: String,
    nodes: Vec<aios_core::pdms_types::EleTreeNode>,
}

#[derive(Debug, Deserialize)]
struct TreeAncestorsReply {
    source: String,
    refnos: Vec<aios_core::RefnoEnum>,
}

/// `/tree/roots` 的另一种读法：节点连同服务端外包的库号，见 [`tree_root_dbs`]。
#[derive(Debug, Deserialize)]
struct TreeRootDbReply {
    source: String,
    #[serde(default)]
    nodes: Vec<TreeRootDb>,
}

#[derive(Debug, Deserialize)]
struct TreeRootDb {
    /// 整个节点也要解出来：名字得先过 [`fill_legacy_default_names`]，那一道要 noun 与 owner。
    #[serde(flatten)]
    node: aios_core::pdms_types::EleTreeNode,
    /// 老服务端不注这一格；新服务端答不出库号时给 `null`（spec §4.10）。两种都是
    /// 「说不出它属于哪个库」，调用方按「没有」处理。
    #[serde(default)]
    dbnum: Option<u32>,
}

fn require_direct(source: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        source == "direct",
        "模型树接口返回了非 direct 数据源: {source}"
    );
    Ok(())
}

/// 2026-09-20 之前的模型服务不给无名元素起名：节点的 `name` 回的是 noun 本身（`SITE`），
/// 也不带 `short_name`，树上那一行就只剩类型、没有名字（gen-model `web-service-api.md`
/// §4.10）。对着这一档服务端，客户端按 E3D 的规矩补短名 `<名词全词> <同名词序号>`
/// （`SITE 10`、`ZONE 4`、`ELBOW 1`）：序号沿同一属主的成员原序数，有名字的兄弟也占号
/// ——与新服务端（e3d-io `element_name`）、库供数（`aios_core::e3d_full_name`）同一套规则。
///
/// 只补得出短形态。整条 `FLNM` 要属主链上各级的名字，这一档 `/tree/*` 一个都不给，所以
/// `name` 也落短名：「显示全称」对着老服务端显示的就是短名，悬停不挂。根层不受这一限：
/// SITE 直接挂在 WORLD（库根，不进名字）下，短名就是全名。
///
/// `nodes` 必须是服务端回的整批——`/tree/children` 的整张成员表，或 `/tree/roots` 的整张
/// SITE 表（按 `owner` 分开数，一个 WORLD 一套序号）；少一个兄弟，后面的序号全错一位。
/// 带 `short_name` 的节点（新服务端）与有名字的节点原样不动。
fn fill_legacy_default_names(nodes: &mut [aios_core::pdms_types::EleTreeNode]) {
    let mut ordinals: HashMap<(aios_core::RefnoEnum, String), usize> = HashMap::new();
    for node in nodes {
        let ordinal = ordinals.entry((node.owner, node.noun.clone())).or_default();
        *ordinal += 1;
        let name = node.name.trim();
        let unnamed = name.is_empty() || name == node.noun.trim();
        if node.short_name.is_none() && unnamed {
            let short = aios_core::e3d_full_name::with_tail(&node.noun, *ordinal, "");
            node.name.clone_from(&short);
            node.short_name = Some(short);
        }
    }
}

pub async fn tree_roots(base: &str) -> anyhow::Result<Vec<aios_core::pdms_types::EleTreeNode>> {
    let mut reply: TreeNodesReply = get(base, "/api/v1/tree/roots").await?;
    require_direct(&reply.source)?;
    fill_legacy_default_names(&mut reply.nodes);
    Ok(reply.nodes)
}

/// 根层每个 SITE 连同它所在的库号。
///
/// `EleTreeNode` 里没有库号那一格，而 `/tree/roots` 的每个节点上服务端另外注了一个
/// `dbnum`（gen-model `handlers.rs::tree_nodes_with_dbnum`）——[`tree_roots`] 按
/// `EleTreeNode` 反序列化，那一格就被丢掉了。模型更新面板要靠它认库，所以这里重解一遍
/// 同一份回执，最后只交出「库号 + SITE 名」两格。
pub async fn tree_root_dbs(base: &str) -> anyhow::Result<Vec<(u32, String)>> {
    let reply: TreeRootDbReply = get(base, "/api/v1/tree/roots").await?;
    require_direct(&reply.source)?;
    Ok(root_dbs(reply.nodes))
}

/// [`tree_root_dbs`] 的纯函数那一半。名字与树上那一行过同一道补名，面板里的 SITE 名
/// 才与树上一字不差。
fn root_dbs(rows: Vec<TreeRootDb>) -> Vec<(u32, String)> {
    let (mut nodes, dbnums): (Vec<_>, Vec<_>) =
        rows.into_iter().map(|row| (row.node, row.dbnum)).unzip();
    fill_legacy_default_names(&mut nodes);
    nodes
        .into_iter()
        .zip(dbnums)
        .filter_map(|(node, dbnum)| match dbnum {
            Some(dbnum) if dbnum != 0 && !node.name.is_empty() => Some((dbnum, node.name)),
            _ => None,
        })
        .collect()
}

pub async fn tree_children(
    base: &str,
    refno: aios_core::RefU64,
) -> anyhow::Result<Vec<aios_core::pdms_types::EleTreeNode>> {
    let path = format!(
        "/api/v1/tree/children?refno={}",
        urlencode(&refno.to_slash_string())
    );
    let mut reply: TreeNodesReply = get(base, &path).await?;
    require_direct(&reply.source)?;
    fill_legacy_default_names(&mut reply.nodes);
    Ok(reply.nodes)
}

pub async fn tree_ancestors(
    base: &str,
    refno: aios_core::RefU64,
) -> anyhow::Result<Vec<aios_core::RefU64>> {
    let path = format!(
        "/api/v1/tree/ancestors?refno={}",
        urlencode(&refno.to_slash_string())
    );
    let reply: TreeAncestorsReply = get(base, &path).await?;
    require_direct(&reply.source)?;
    Ok(reply
        .refnos
        .into_iter()
        .map(|refno| refno.refno())
        .collect())
}

#[derive(Debug, Deserialize)]
struct SearchReply {
    #[serde(default)]
    items: Vec<SearchItem>,
}

#[derive(Debug, Deserialize)]
struct SearchItem {
    refno: String,
    name: String,
    #[serde(default)]
    noun: String,
    dbnum: u32,
}

pub async fn search_names(
    base: &str,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<plant_ui_data::NameHit>> {
    let path = format!("/api/v1/search?query={}&limit={}", urlencode(query), limit);
    let reply: SearchReply = get(base, &path).await?;
    reply
        .items
        .into_iter()
        .map(|item| {
            Ok(plant_ui_data::NameHit {
                refno: item
                    .refno
                    .parse()
                    .map_err(|_| anyhow::anyhow!("模型服务返回非法 refno {}", item.refno))?,
                name: item.name,
                noun: item.noun,
                dbnum: item.dbnum,
            })
        })
        .collect()
}

#[derive(Debug, Default, Deserialize)]
struct ElementAttributesReply {
    #[serde(default)]
    source: String,
    #[serde(default)]
    complete: bool,
    #[serde(default)]
    attributes: Vec<ElementAttribute>,
    #[serde(default)]
    diagnostics: ElementAttributeDiagnostics,
}

#[derive(Debug, Default, Deserialize)]
struct ElementAttribute {
    #[serde(default)]
    name: String,
    #[serde(default)]
    value_type: String,
    #[serde(default)]
    display: String,
    #[serde(default)]
    is_unset: bool,
    #[serde(default)]
    editable: bool,
    #[serde(default)]
    is_uda: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ElementAttributeDiagnostics {
    #[serde(default)]
    outside_schema: Vec<String>,
    #[serde(default)]
    shape_conflicts: Vec<Value>,
    #[serde(default)]
    undecoded: Vec<Value>,
    #[serde(default)]
    uda_issues: Vec<String>,
}

/// Complete file-side attributes.  There is intentionally no SurrealDB
/// fallback: an empty cache is a valid read-through state, not an empty
/// attribute set.
pub async fn element_attributes(
    base: &str,
    refno: aios_core::RefU64,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<Vec<Attr>> {
    let reply: ElementAttributesReply = post(
        base,
        "/api/v1/element/attributes",
        element_attributes_body(refno, project, mdb, namespace),
        Duration::from_secs(30),
    )
    .await
    .map_err(|error| {
        let failure = failure_of(&error);
        if failure.code == "not_found"
            || failure.message.contains("HTTP 404")
            || failure.message.contains("405")
        {
            anyhow::anyhow!("gen-model 版本不支持文件侧属性，请升级 gen-model")
        } else {
            error
        }
    })?;
    render_element_attributes(reply)
}

fn render_element_attributes(reply: ElementAttributesReply) -> anyhow::Result<Vec<Attr>> {
    anyhow::ensure!(
        reply.source == "e3d-io",
        "属性接口返回了非 e3d-io 数据源: {}",
        reply.source
    );
    let mut rows = reply
        .attributes
        .into_iter()
        .map(|attribute| Attr {
            name: attribute.name,
            value: attribute.display,
            is_uda: attribute.is_uda,
            kind: if attribute.is_unset {
                AttrKind::Unset
            } else if !attribute.editable {
                AttrKind::Opaque
            } else {
                match attribute.value_type.as_str() {
                    "int" => AttrKind::Int,
                    "real" => AttrKind::Real,
                    "bool" => AttrKind::Bool,
                    "text" | "word" => AttrKind::Text,
                    _ => AttrKind::Opaque,
                }
            },
        })
        .collect::<Vec<_>>();
    if !reply.complete {
        let issue_count = reply.diagnostics.outside_schema.len()
            + reply.diagnostics.shape_conflicts.len()
            + reply.diagnostics.undecoded.len()
            + reply.diagnostics.uda_issues.len();
        rows.insert(
            0,
            Attr {
                name: "⚠ diagnostics".into(),
                value: format!("文件属性不完整（{issue_count} 项诊断），已显示可安全解码的值"),
                kind: AttrKind::Opaque,
                is_uda: false,
            },
        );
    }
    Ok(rows)
}

fn element_attributes_body(
    refno: aios_core::RefU64,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> String {
    serde_json::json!({
        "project": project,
        "mdb": mdb,
        "namespace": namespace,
        "refno": refno.to_slash_string(),
    })
    .to_string()
}

/// `None` 必须**整个不发** `dbnums` 键——发 `null` 或空表都不行：老服务端的
/// 请求体解析没有这个字段，多出来的键无害，但语义上 `Some([])` 是「一个批次
/// 都不排」，与「全范围」是两个相反的东西，混了会把勾选门变成摆设。
fn execute_body(project: &str, mdb: &str, namespace: &str, dbnums: Option<&[u32]>) -> String {
    let mut body = serde_json::json!({ "project": project, "mdb": mdb, "namespace": namespace });
    if let Some(dbnums) = dbnums {
        body["dbnums"] = serde_json::json!(dbnums);
    }
    body.to_string()
}

/// 队列面板的一次轮询。
///
/// 四个只读接口打包成一次往返再一起交出去：分开更新会出现「队列已经空了、顶栏
/// 摘要还说有一条在跑」这种自相矛盾的中间态。
///
/// 分工是定死的——**排队与运行中的行以 `/queue` 为准**（那一份不封顶，287 行也全在），
/// `/tasks` 只补计时、单元计数与终态历史，它服务端那边 `limit` 钳到 200。
pub async fn poll_queue(base: &str) -> anyhow::Result<Poll> {
    let queue: QueueSnapshot = get(base, "/api/v1/queue").await?;
    // `/tasks` 之后的几份取不到都不该让整次轮询作废：队列行已经在手上了。
    // 任务表失败只退化计时与终态历史（沿用上一份快照，`Vm::adopt` 负责保留），
    // 一条解不动的 result 不许把整个面板冻在旧快照上。
    let tasks = get::<TaskList>(base, "/api/v1/tasks?limit=200").await;
    let (tasks, tasks_error) = match tasks {
        Ok(list) => (list.tasks, None),
        Err(error) => (Vec::new(), Some(crate::logs::error_chain(&error))),
    };
    let health = get::<Health>(base, "/api/v1/health").await.ok();
    // 欠账表有三种答案，不是两种。取到了是一种；**服务端没有这个端点**是另一种
    // ——那不是「暂时取不到」，是这一档在服务端已经不存在（gen-model 2026-09-08 起
    // 模型工作内联进 dbnum 任务，欠账表退役）。把它按失败处理，界面会把上一份快照
    // 冻在那儿当成实况，越冻越旧。所以：端点不在 = 答案已知且为空，另记一格让界面
    // 整段不画；只有真取不到（超时 / 500）才沿用上一份。
    let pending = get::<PendingUnits>(base, "/api/v1/update/pending-units").await;
    let pending_unsupported = pending.as_ref().err().is_some_and(pending_endpoint_retired);
    let pending_known = pending.is_ok() || pending_unsupported;
    let pending = pending.map(|p| p.units).unwrap_or_default();
    // `/dbnums` 要重扫项目目录，是这四个里最慢的一个；取不到就少画「本期不执行」
    // 那一格，不该拖垮整次轮询。
    let dbnums = get::<DbnumReport>(base, "/api/v1/dbnums")
        .await
        .map(|r| r.dbnums)
        .unwrap_or_default();
    Ok(Poll {
        queue,
        tasks,
        tasks_error,
        health,
        pending,
        pending_known,
        pending_unsupported,
        dbnums,
    })
}

/// 这次失败是不是「服务端没有这个端点」。
///
/// 只认 404 / 405 与 `not_found`：那说明这一档在服务端已经不存在，答案是**可信的空**。
/// 超时、500、连不上都不算——那些是「暂时看不见」，界面该沿用上一份快照。
/// 判据与 `element_attributes` 那条老服务端探测同一把尺子（本文件上方）。
fn pending_endpoint_retired(error: &anyhow::Error) -> bool {
    endpoint_missing(error)
}

/// 「这一档服务端没有这条路由」（404 / 405 / `not_found`）。两代服务端并存的通用判法
/// （ADR-0027）：欠账表退役、整库重建 / 房间重算 / 重新解析三条新口在老服务端上缺席，
/// 界面都按「服务端没给」整格降级，不重试、不当成故障。
pub fn endpoint_missing(error: &anyhow::Error) -> bool {
    let failure = failure_of(error);
    failure.code == "not_found"
        || failure.message.contains("HTTP 404")
        || failure.message.contains("HTTP 405")
}

/// Current file/cache coverage.  The startup path uses this instead of the
/// SurrealDB project query in read-through mode, where an uncached DESI quite
/// deliberately has no `pe` rows yet.
pub async fn dbnum_report(base: &str) -> anyhow::Result<DbnumReport> {
    get(base, "/api/v1/dbnums").await
}

pub async fn service_health(base: &str) -> anyhow::Result<Health> {
    get(base, "/api/v1/health").await
}

/// `/health` 归一出的常驻房间参照模型状态——「重新生成模型」发出前的等待循环只认这三档。
/// 归一而不是透传：老服务端（没有 `room_build` 一格）与放行态在这里压成同一个答案，
/// 调用方不必自己分得清「没这门」和「已就绪」。
#[derive(Debug, Clone)]
pub enum ResidentStatus {
    /// 放行，可以照发：`ready` / `degraded`（带缺口也照算）/ `disabled` /
    /// `unsupported`（本进程没有房间体制）/ 老服务端根本没这一格。
    Ready,
    /// 还在建：界面画「等房间参照模型装载」档（可取消——生成请求还没发）。
    /// `progress` 缺 = 服务端比这一版老，只画 spinner 与阶段名。
    Building {
        phase: Option<String>,
        progress: Option<ResidentProgress>,
    },
    /// 建死了（`blocked`）：等也等不来。`reason` 取 `problems` 第一条点名。
    Blocked { reason: Option<String> },
}

/// 从一份 `/health` 快照归一（等待循环与测试共用同一处）。
pub fn resident_status_of(health: &Health) -> ResidentStatus {
    let Some(resident) = health
        .room_build
        .as_ref()
        .and_then(|build| build.resident.as_ref())
    else {
        // 老服务端 / 没装房间体制的服务端压根不报这一格 → 没有要等的东西。
        return ResidentStatus::Ready;
    };
    match resident.status.as_str() {
        "loading" => ResidentStatus::Building {
            phase: resident.phase.clone().or_else(|| {
                resident
                    .progress
                    .as_ref()
                    .map(|progress| progress.phase.clone())
                    .filter(|phase| !phase.is_empty())
            }),
            progress: resident.progress.clone(),
        },
        "blocked" => ResidentStatus::Blocked {
            reason: resident.problems.first().cloned(),
        },
        // ready / degraded / disabled / unsupported：放行——除了「重新生成房间」的
        // 第三段。换代装上之后生命周期就翻 ready，逐库房间边重算（`edge-recompute`）
        // 却在它之后跑；那段没跑完前都算 Building，不然进度屏会提前收摊。
        _ => match &resident.progress {
            Some(progress)
                if progress.phase == "edge-recompute"
                    && progress.dbnums_done < progress.dbnums_total =>
            {
                ResidentStatus::Building {
                    phase: Some(progress.phase.clone()),
                    progress: resident.progress.clone(),
                }
            }
            // 真不该放行的那半句由服务端 `ensure_ready` 自己兜底
            // （它会回 RESIDENT_MODEL_UPDATING → 409）。
            _ => ResidentStatus::Ready,
        },
    }
}

const ROOMS_REBUILD_PATH: &str = "/api/v1/rooms/rebuild";

/// 「重新生成房间」（全库）：`POST /api/v1/rooms/rebuild`。服务端 202 受理后后台清两张
/// 房间边表 → 换代重建常驻参照模型 → 逐库整库重算房间关系；进度读
/// `/health.room_build.resident.progress`。受理即 `Ok(())`；`409 updating`
/// （启动首建或上一趟重建还在跑）与其余失败按 `Failure` 原样上抛——由调用方把
/// updating 当「挂上已在跑的那趟」处理。
pub async fn rebuild_rooms(
    base: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<()> {
    let body = serde_json::json!({
        "project": project,
        "mdb": mdb,
        "namespace": namespace,
    })
    .to_string();
    let _: serde_json::Value = post(
        base,
        ROOMS_REBUILD_PATH,
        body,
        Duration::from_secs(30),
    )
    .await?;
    Ok(())
}

/// 拉一次 `/health` 并归一出常驻房间参照模型的状态。取不到（超时 / 500 / 连不上）
/// 按 `Err` 上浮，由调用方决定是再试还是放行——别在这里吞成 `Ready`：服务端够不着
/// 与「就绪」是两个相反的答案。
pub async fn resident_status(base: &str) -> anyhow::Result<ResidentStatus> {
    Ok(resident_status_of(&service_health(base).await?))
}

/// 这次失败是不是「房间参照模型还没装好」（服务端 ADR-067 的 `RESIDENT_MODEL_UPDATING`）：
/// 新服务端回 `409 / code = "updating"`，老服务端把这串字面埋在 503 `room_reference_unavailable`
/// 或 500 `internal` 的 message 里——两个都认，都按「还在建，可等」处置。
pub fn resident_building(error: &anyhow::Error) -> bool {
    let failure = failure_of(error);
    failure.code == "updating" || failure.message.contains("RESIDENT_MODEL_UPDATING")
}

/// 暂停 / 恢复出队。暂停**只挡出队**，正在跑的那一批会跑完为止。
pub async fn set_queue_paused(base: &str, paused: bool) -> anyhow::Result<()> {
    let path = if paused {
        "/api/v1/queue/pause"
    } else {
        "/api/v1/queue/resume"
    };
    let _: serde_json::Value = post(base, path, "{}".to_owned(), Duration::from_secs(15)).await?;
    Ok(())
}

/// 复活一行死信。自动路径到了重试上限就永不再碰它，这是唯一的出路。
///
/// 服务端只改那一行（`attempts` 清零、`revision` 加一、清 `last_error`）再叫醒
/// 调度器，**不排新的数据批次**——所以回执里没有 task_id 可跟，结果一律等下一拍
/// 轮询。行不存在回 404，那多半是它刚被别人复活并跑掉了。
pub async fn retry_pending_unit(
    base: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
    root_refno: &str,
) -> anyhow::Result<()> {
    let body = serde_json::json!({
        "target_refno": root_refno,
        "project": project,
        "mdb": mdb,
        "namespace": namespace,
    });
    let _: serde_json::Value = post(
        base,
        "/api/v1/update/pending-units/retry",
        body.to_string(),
        Duration::from_secs(15),
    )
    .await?;
    Ok(())
}

/// `POST /api/v1/model/generate` 的回执状态。
///
/// 三档对界面是三件不同的事，别压成一个布尔：`Generated` 是真做了一趟，
/// `AlreadyAvailable` 是凭证当前的根直接命中（右键 `force = true` 时不会出现），
/// `NoRenderableGeometry` 是这一片本来就没有可画的东西——它不是失败，重试一百遍
/// 还是同一个结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerateStatus {
    Generated,
    AlreadyAvailable,
    NoRenderableGeometry,
    /// 服务端换了新状态名。当成「做过了」计数，但要在日志里点名。
    Unknown,
}

#[derive(Debug, Clone)]
pub struct GenerateReply {
    pub status: GenerateStatus,
    /// 服务端解出来的生成根（请求的 refno 自己是生成根时就是它）。只进日志：客户端不拿
    /// 根清单去查显示——显示按祖先查（ADR-0030 D1），回执里的 `generation_roots[]` 不收。
    pub generation_root: String,
    pub model_available: bool,
    pub generation_root_count: usize,
    pub cached_root_count: usize,
    pub generated_root_count: usize,
    /// 这批模型此刻在哪儿（spec §4.12）：`Memory` = 服务端没等初始化队列、直接从文件算进
    /// 进程内投影，活不过服务端重启，持久由初始化发布负责；`Database` = 已落 rocksdb。
    /// 老服务端不给这一格 → `None`，日志里不多说。
    pub model_source: Option<ModelSource>,
    /// `memory` 的理由（`initialization_publishing` / `data_watermark_unestablished` / `read-through`）。
    pub model_source_reason: Option<String>,
    /// 这一发是退回旧路径 `/api/v1/model/ensure` 才成的（服务端还没有 `/model/generate`，
    /// ADR-0030 后果 7）。日志据此点名一次「模型服务是旧版」。
    pub legacy_endpoint: bool,
}

impl GenerateReply {
    /// 日志里跟在计数后面的那半句。只有「内存」值得说：人得知道现在看到的是 API 现算的、
    /// 翻面后同一版会从 rocksdb 读出来；「数据库」是常态，不出声。
    pub fn source_note(&self) -> Option<String> {
        match self.model_source? {
            ModelSource::Database => None,
            ModelSource::Memory => Some(format!(
                "；{}，由 API 供数",
                ModelSource::Memory
                    .label(self.model_source_reason.as_deref())
                    .trim_start_matches("模型来源：")
            )),
        }
    }
}

#[derive(Deserialize)]
struct GenerateBody {
    #[serde(default)]
    status: String,
    #[serde(default)]
    generation_root: String,
    #[serde(default)]
    model_available: bool,
    #[serde(default)]
    generation_root_count: usize,
    #[serde(default)]
    cached_root_count: usize,
    #[serde(default)]
    generated_root_count: usize,
    #[serde(default)]
    model_source: Option<String>,
    #[serde(default)]
    model_source_reason: Option<String>,
}

/// 生成接口的新名（gen-model ADR-086 D4）。老服务端只有旧名，404 时退回它（ADR-0030 后果 7）。
const GENERATE_PATH: &str = "/api/v1/model/generate";
/// 旧名。**只在 404 回退分支里出现**——源码钉 `no_ensure_left_outside_the_fallback`。
const LEGACY_GENERATE_PATH: &str = "/api/v1/model/ensure";

/// 对一个节点范围生成模型（`POST /api/v1/model/generate`，ADR-0030 D2）：服务端自己解根、
/// 删旧、重算，客户端只说「生成这个范围」。`force = true` 是右键「重新生成模型」——范围内
/// 已有的产物一律重算替换；`force = false` 时凭证当前的根直接回 `AlreadyAvailable`。
///
/// **显示路径不调它**（ADR-0030 D1）：眼睛 / 聚焦 / 房间模型 / 取回工作都只按祖先查
/// `/model/records`。生成只有这一个入口。
///
/// 新客户端对老服务端：`/model/generate` 还没有（404 / 405）就用同一个请求体退回
/// `/model/ensure`，回执带 `legacy_endpoint = true` 供日志点名一次。
///
/// 超时给到 125 秒，比服务端那道 120 秒稍长：让服务端的超时语义先生效
/// （它回 504 并说明后台继续跑），而不是客户端先把连接掐掉、什么都不知道。
pub async fn generate_model(
    base: &str,
    refno: &str,
    force: bool,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<GenerateReply> {
    let body = serde_json::json!({
        "refno": refno,
        "force": force,
        "project": project,
        "mdb": mdb,
        "namespace": namespace,
    })
    .to_string();
    let timeout = Duration::from_secs(125);
    let (reply, legacy_endpoint): (GenerateBody, bool) =
        match post(base, GENERATE_PATH, body.clone(), timeout).await {
            Ok(reply) => (reply, false),
            // 老服务端没有这条路由：同一请求体退回旧名（ADR-0030 后果 7）。
            Err(error) if endpoint_missing(&error) => {
                (post(base, LEGACY_GENERATE_PATH, body, timeout).await?, true)
            }
            Err(error) => return Err(error),
        };
    Ok(GenerateReply {
        status: generate_status(&reply.status),
        generation_root: reply.generation_root,
        model_available: reply.model_available,
        generation_root_count: reply.generation_root_count,
        cached_root_count: reply.cached_root_count,
        generated_root_count: reply.generated_root_count,
        model_source: reply.model_source.as_deref().and_then(ModelSource::parse),
        model_source_reason: reply.model_source_reason,
        legacy_endpoint,
    })
}

#[derive(Deserialize)]
struct CleanupBody {
    #[serde(default)]
    scope: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    model_source: Option<String>,
    #[serde(default)]
    model_source_reason: Option<String>,
}

/// `DELETE /api/v1/model/subtree` 的回执（gen-model spec §4.5.1）。`requested_refno` 不收：
/// 回执按请求顺序整批回来，目标是哪一个本端自己知道。
#[derive(Debug, Clone)]
pub struct CleanupReply {
    /// 服务端的删除口径，今天恒为 `exact_subtree`；只进日志。
    pub scope: String,
    /// 今天恒为 `deleted`；只进日志，认不出的要说出来。
    pub status: String,
    /// 清的是哪一面（spec §4.12 口径）：`Memory` = 只清了进程内投影（读透另进 spool 墓碑），
    /// `Database` = 库 + 投影 + 账本。老服务端不给 → `None`。
    pub model_source: Option<ModelSource>,
    pub model_source_reason: Option<String>,
}

impl CleanupReply {
    /// 成功那一行日志的服务端半句：口径 + 认不出的状态 + 内存供数。常态只有 `exact_subtree`。
    ///
    /// 清的是内存投影就要说出来——库里那一面（若有）没动，服务端重启后这次清理不复存在；
    /// 状态不是 `deleted` 也要说出来：按「删过了」计，但人得知道服务端换了说法。
    pub fn server_note(&self) -> String {
        let mut note = format!("服务端 {}", self.scope);
        if self.status != "deleted" {
            note.push_str(&format!(
                "，回了一个不认识的状态「{}」，按「删过了」计",
                self.status
            ));
        }
        if self.model_source == Some(ModelSource::Memory) {
            note.push_str(&format!(
                "；{}，清的是 API 内存里的投影",
                ModelSource::Memory
                    .label(self.model_source_reason.as_deref())
                    .trim_start_matches("模型来源：")
            ));
        }
        note
    }
}

const CLEANUP_PATH: &str = "/api/v1/model/subtree";

/// 对一个节点范围清理已生成的模型产物（`DELETE /api/v1/model/subtree?refno&confirm`，spec §4.5.1）：
/// 服务端按子树删产物行——不删设计元素、不删共享单位网格、不重算；与右键重新生成里的「先删」是
/// 服务端同一份实现。`confirm` 必须与 `refno` 原文一致，这里替调用方填同一个值：确认在界面上
/// 已经做过了（确认框），不在这里再问一遍。
///
/// 右键「清理模型」是唯一调用方。显示路径不调它（ADR-0030 D1），重新生成也不调它（服务端在
/// `force = true` 里自己清）。
///
/// 超时给到 125 秒：服务端这条没有 120 s 预算包装，大范围同步删到底；客户端超时时服务端可能已删
/// 一部分，日志要把这一点说出来，让人点眼睛重查。
pub async fn delete_model_subtree(
    base: &str,
    refno: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<CleanupReply> {
    let path = cleanup_path(refno, project, mdb, namespace);
    let reply: CleanupBody = delete(base, &path, Duration::from_secs(125)).await?;
    Ok(CleanupReply {
        scope: reply.scope,
        status: reply.status,
        model_source: reply.model_source.as_deref().and_then(ModelSource::parse),
        model_source_reason: reply.model_source_reason,
    })
}

/// `DELETE /model/subtree` 的请求行。抽出来是为了让测试不必起网络就能钉住两件事：
/// `confirm` 与 `refno` 是同一个值、斜杠形 refno 编码过。
fn cleanup_path(refno: &str, project: &str, mdb: &str, namespace: &str) -> String {
    let refno = urlencode(refno);
    format!(
        "{CLEANUP_PATH}?refno={refno}&confirm={refno}&project={}&mdb={}&namespace={}",
        urlencode(project),
        urlencode(mdb),
        urlencode(namespace)
    )
}

#[derive(Debug, Deserialize)]
struct ModelRecordsPage {
    #[serde(default)]
    items: Vec<aios_core::GeomInstQuery>,
    next_cursor: Option<usize>,
    /// `model-memory` / `model-database`——这一页是从哪儿读的（老字段）。
    #[serde(default)]
    source: String,
    /// 这根所属的库（spec §4.12 新加）。老服务端不给。
    #[serde(default)]
    dbnum: Option<u32>,
}

impl ModelRecordsPage {
    fn scope(&self) -> model_record_union::RecordScope {
        model_record_union::RecordScope {
            dbnum: self.dbnum,
            source: ModelSource::from_records_label(&self.source),
        }
    }
}

/// 一次装载的回执：记录之外还带上每个库认下的取数源，日志据此说出「哪几个库此刻
/// 还在吃内存投影」。
#[derive(Debug, Default)]
pub struct ModelRecords {
    pub records: Vec<aios_core::GeomInstQuery>,
    /// dbnum 升序；老服务端没给 dbnum 的那一桶键为 `None`。
    pub sources: Vec<(Option<u32>, ModelSource)>,
}

impl ModelRecords {
    /// 还在吃内存投影的库号。空 = 全部以 rocksdb 为准（或服务端根本没说）。
    pub fn memory_dbnums(&self) -> Vec<u32> {
        self.sources
            .iter()
            .filter_map(|(dbnum, source)| {
                (*source == ModelSource::Memory).then_some(*dbnum).flatten()
            })
            .collect()
    }

    /// 日志里跟在「N 个元素、M 个网格实例」后面的那半句：哪几个库的模型此刻是 API
    /// 现算的。全部以 rocksdb 为准（或服务端没说）时什么都不说——那是常态，不值得占一句。
    pub fn memory_note(&self) -> Option<String> {
        let memory = self.memory_dbnums();
        if memory.is_empty() {
            // 老服务端不给 dbnum 时整次装载是一个桶：那一桶若是内存也得说出来。
            return self
                .sources
                .iter()
                .any(|(dbnum, source)| dbnum.is_none() && *source == ModelSource::Memory)
                .then(|| {
                    "；由 API 从内存供数（初始化收口后自动改读 rocksdb，不必重载）".to_owned()
                });
        }
        Some(format!(
            "；其中 {} 由 API 从内存供数（初始化收口后自动改读 rocksdb，不必重载）",
            memory
                .iter()
                .map(|dbnum| format!("db{dbnum}"))
                .collect::<Vec<_>>()
                .join("、")
        ))
    }
}

/// Read generated geometry through gen-model's records endpoint.  Which store a
/// root is read from is the **server's** call, per dbnum (spec §4.12): an
/// initialized dbnum answers from rocksdb, one still initializing answers from the
/// process projection.  There is intentionally no SurrealDB fallback here, and a
/// single dbnum is never assembled from two sources: mixing the two epochs would
/// make a single viewport load internally inconsistent.
pub async fn model_records(
    base: &str,
    roots: &[aios_core::RefU64],
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<ModelRecords> {
    let mut out = model_record_union::ModelRecordUnion::default();
    for root in roots {
        let mut scope_records = Vec::new();
        let mut scope = None;
        let mut cursor = None;
        loop {
            let body = serde_json::json!({
                "project": project,
                "mdb": mdb,
                "namespace": namespace,
                "generation_root": root.to_slash_string(),
                "limit": 5000,
                "cursor": cursor,
            });
            let page: ModelRecordsPage = post(
                base,
                "/api/v1/model/records",
                body.to_string(),
                Duration::from_secs(30),
            )
            .await?;
            // 同一根分页中途翻面（第一页内存、第二页 rocksdb）：两页算的未必是同一版，
            // 与跨根混源同一条纪律，整根报错让调用点重来。
            let this = page.scope();
            match scope {
                None => scope = Some(this),
                Some(first) if first != this && this.source.is_some() && first.source.is_some() => {
                    anyhow::bail!(
                        "生成根 {} 的记录分页中途换了取数源（{} → {}），这一趟不可用，请重查",
                        root.to_slash_string(),
                        first.source.map(|s| s.as_str()).unwrap_or("?"),
                        this.source.map(|s| s.as_str()).unwrap_or("?"),
                    )
                }
                Some(_) => {}
            }
            scope_records.extend(page.items);
            let Some(next) = page.next_cursor else { break };
            cursor = Some(next);
        }
        out.extend_scope(
            scope.unwrap_or_else(model_record_union::RecordScope::legacy),
            scope_records,
        )?;
    }
    Ok(ModelRecords {
        sources: out.sources(),
        records: out.finish(),
    })
}

fn generate_status(raw: &str) -> GenerateStatus {
    match raw {
        "generated" | "Generated" => GenerateStatus::Generated,
        "already_available" | "AlreadyAvailable" => GenerateStatus::AlreadyAvailable,
        "no_renderable_geometry" | "NoRenderableGeometry" => GenerateStatus::NoRenderableGeometry,
        _ => GenerateStatus::Unknown,
    }
}

/// query 串里的 refno 带 `/`，项目名与 MDB 带 `/` 也带空格。
/// 只做百分号转义，不引第三方依赖——这几个字段的字符集很窄。
fn urlencode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

async fn get<T: DeserializeOwned>(base: &str, path: &str) -> anyhow::Result<T> {
    let mut req = ehttp::Request::get(format!("{base}{path}"));
    req.timeout = Some(Duration::from_secs(15));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(transport)
        .context("请求模型服务失败")?;
    request(response)
}

async fn post<T: DeserializeOwned>(
    base: &str,
    path: &str,
    body: String,
    timeout: Duration,
) -> anyhow::Result<T> {
    let mut req = ehttp::Request::post(format!("{base}{path}"), body.into_bytes());
    // ehttp 的 `post()` 默认带 `Content-Type: text/plain`，而 `Headers::insert`
    // 是**追加**不是替换（同名键允许重复）——insert 会造出两个 Content-Type，
    // axum 只看第一个，Json 提取器直接 415。整体覆盖 headers 才是替换。
    req.headers = ehttp::Headers::new(&[
        ("Accept", "*/*"),
        ("Content-Type", "application/json; charset=utf-8"),
    ]);
    req.timeout = Some(timeout);
    let response = ehttp::fetch_async(req)
        .await
        .map_err(transport)
        .context("请求模型更新服务失败")?;
    request(response)
}

/// 无 body 的 DELETE。`ehttp` 没有现成的构造器，拿 `get()` 的壳换方法名——其余
/// （url、无 body、默认头）与 GET 完全一样。
async fn delete<T: DeserializeOwned>(
    base: &str,
    path: &str,
    timeout: Duration,
) -> anyhow::Result<T> {
    let mut req = ehttp::Request::get(format!("{base}{path}"));
    req.method = ehttp::Method::DELETE;
    req.timeout = Some(timeout);
    let response = ehttp::fetch_async(req)
        .await
        .map_err(transport)
        .context("请求模型服务失败")?;
    request(response)
}

/// 传输层失败：连不上、超时、握手不成。对用的人来说这些是同一件事——服务够不着，
/// 没有任何数据被改动，直接重试即可，所以统一归 `timeout`。
fn transport(error: String) -> anyhow::Error {
    anyhow::Error::new(ApiError(Failure::new("timeout", error.to_string())))
}

fn request<T: DeserializeOwned>(response: ehttp::Response) -> anyhow::Result<T> {
    let status = response.status;
    let body = response.text().context("模型更新服务响应不是 UTF-8")?;
    if !response.ok {
        return Err(anyhow::Error::new(ApiError(error_packet(status, body))));
    }
    serde_json::from_str(&body).context("解析模型更新服务响应失败")
}

/// 解错误包封。`code` 缺失时按状态码兜底——422 是前置条件不满足、409 是任务冲突、
/// 504 是超时，其余一律 internal。
fn error_packet(status: u16, body: &str) -> Failure {
    let packet = serde_json::from_str::<ErrorResponse>(body).unwrap_or_default();
    let code = packet.code.unwrap_or_else(|| {
        match status {
            422 => "precondition",
            409 => "conflict",
            504 => "timeout",
            _ => "internal",
        }
        .to_owned()
    });
    Failure {
        code,
        message: packet
            .message
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| format!("HTTP {status}: {body}")),
        detail: packet.detail,
    }
}

#[derive(Default, serde::Deserialize)]
struct ErrorResponse {
    code: Option<String>,
    message: Option<String>,
    detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_ui::model_update::FailForm;

    #[test]
    fn element_attribute_request_carries_the_full_service_identity() {
        let refno = aios_core::RefU64::from(42);
        let body: serde_json::Value = serde_json::from_str(&element_attributes_body(
            refno,
            "AvevaMarineSample",
            "/ALL",
            "ams",
        ))
        .unwrap();
        assert_eq!(body["project"], "AvevaMarineSample");
        assert_eq!(body["mdb"], "/ALL");
        assert_eq!(body["namespace"], "ams");
        assert_eq!(body["refno"], refno.to_slash_string());
    }

    #[test]
    fn file_side_uda_marker_survives_the_http_mapping() {
        let reply: ElementAttributesReply = serde_json::from_str(
            r#"{
                "source":"e3d-io",
                "complete":true,
                "attributes":[{
                    "name":"MYUDA",
                    "value_type":"text",
                    "display":"value",
                    "is_uda":true,
                    "is_unset":false,
                    "editable":true
                }]
            }"#,
        )
        .unwrap();
        let rows = render_element_attributes(reply).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "MYUDA");
        assert!(rows[0].is_uda);
        assert_eq!(rows[0].kind, AttrKind::Text);
    }

    /// ADR-020 勾选子集的客户端半边：`None` 整个不发键（老服务端兼容 + 全范围），
    /// `Some` 原样上送——**包括空表**，那是「全不勾」而不是「没选择」。
    #[test]
    fn the_execute_body_only_carries_dbnums_when_a_subset_was_chosen() {
        let full: serde_json::Value =
            serde_json::from_str(&execute_body("ProjAMS", "/ALL", "hd", None)).unwrap();
        assert!(full.get("dbnums").is_none(), "None = 不发键，不是 null");
        assert_eq!(full["project"], "ProjAMS");

        let subset: serde_json::Value =
            serde_json::from_str(&execute_body("ProjAMS", "/ALL", "hd", Some(&[8000, 8021])))
                .unwrap();
        assert_eq!(subset["dbnums"], serde_json::json!([8000, 8021]));

        let none_checked: serde_json::Value =
            serde_json::from_str(&execute_body("ProjAMS", "/ALL", "hd", Some(&[]))).unwrap();
        assert_eq!(none_checked["dbnums"], serde_json::json!([]));
    }

    /// 错误包封按 `code` 分型；`code` 缺席时才按状态码兜底。
    ///
    /// 422 的兜底给的是已退役的 `precondition`，它落到 `Internal`——**这是有意的**。
    /// 别把它「顺手修」成 `identity_mismatch`：一个说不清来路的拒绝，把原始 message
    /// 摊进详情比替它编一个「范围对不上」诚实（S2-D），后者会让人去查一处并不存在
    /// 的配置错位。真的范围不符时服务端是带着 code 来的，走的是上面第一条。
    #[test]
    fn error_packets_fall_back_by_status_only_when_the_code_is_missing() {
        let tagged = error_packet(
            422,
            r#"{"code":"identity_mismatch","message":"mdb=/SAMPLE 与服务 mdb=/ALL 不一致"}"#,
        );
        assert_eq!(tagged.form(), FailForm::IdentityMismatch);

        assert_eq!(error_packet(504, "{}").form(), FailForm::Timeout);
        assert_eq!(error_packet(422, "{}").form(), FailForm::Internal);

        // 解不动的响应体不许把详情页留成一片空白：状态码与原文都要带上。
        let bare = error_packet(500, "not json at all");
        assert_eq!(bare.form(), FailForm::Internal);
        assert!(
            bare.message.contains("500") && bare.message.contains("not json at all"),
            "{}",
            bare.message
        );
    }

    /// query 串里的 refno 带 `/`、MDB 带 `/`，百分号转义要对——服务端按原文比对。
    #[test]
    fn urlencode_escapes_slashes_and_keeps_plain_names() {
        assert_eq!(urlencode("24381/100677"), "24381%2F100677");
        assert_eq!(urlencode("/ALL"), "%2FALL");
        assert_eq!(urlencode("ProjAMS"), "ProjAMS");
    }

    /// 清理一发的请求行（spec §4.5.1）：`confirm` 必须与 `refno` 原文一致——确认在界面上做过了，
    /// 这里替调用方填同一个值；斜杠形 refno 与 MDB 都要转义；身份三件套随行。
    #[test]
    fn cleanup_request_confirms_the_same_refno_and_encodes_it() {
        let path = cleanup_path("24381/100677", "ProjAMS", "/ALL", "aios");
        assert_eq!(
            path,
            "/api/v1/model/subtree?refno=24381%2F100677&confirm=24381%2F100677&project=ProjAMS&mdb=%2FALL&namespace=aios"
        );
        // 清理只有这一条路由，不像 generate 还有旧名可退：字符串常量只许出现一次。
        // 文档注释可以点名同一条路径，不计入。
        let source = include_str!("model_update_api.rs")
            .split_once("#[cfg(test)]")
            .expect("tests follow")
            .0;
        assert_eq!(
            source.matches("\"/api/v1/model/subtree\"").count(),
            1,
            "{source}"
        );
    }

    /// 整库清理的请求行：`confirm` 等于路径 dbnum，身份三件套转义。`ref0s` 缺席与空数组不是一回事。
    #[test]
    fn dbnum_model_cleanup_confirms_the_path_dbnum_and_keeps_absent_ref0s() {
        assert_eq!(
            dbnum_model_path(8000, "ProjAMS", "/ALL", "aios"),
            "/api/v1/dbnums/8000/model?confirm=8000&project=ProjAMS&mdb=%2FALL&namespace=aios"
        );
        let absent: DbnumModelCleanupReceipt = serde_json::from_str(
            r#"{"dbnum":8000,"scope":"model_face","status":"deleted","range_statements":3,"elapsed_ms":12}"#,
        )
        .unwrap();
        assert!(absent.ref0s.is_none(), "缺席不能当成空数组");
        let present: DbnumModelCleanupReceipt =
            serde_json::from_str(r#"{"dbnum":8000,"ref0s":[24381,31909]}"#).unwrap();
        assert_eq!(present.ref0s, Some(vec![24381, 31909]));
        let empty: DbnumModelCleanupReceipt =
            serde_json::from_str(r#"{"dbnum":8000,"ref0s":[]}"#).unwrap();
        assert_eq!(empty.ref0s, Some(vec![]));
    }

    /// generate 的四档状态对界面是四件不同的事，压成布尔就分不出「重做过了」与
    /// 「凭证当前直接命中」。
    #[test]
    fn generate_status_keeps_the_four_outcomes_apart() {
        assert_eq!(generate_status("generated"), GenerateStatus::Generated);
        assert_eq!(
            generate_status("already_available"),
            GenerateStatus::AlreadyAvailable
        );
        assert_eq!(
            generate_status("no_renderable_geometry"),
            GenerateStatus::NoRenderableGeometry
        );
        // 服务端换名字不许静默当成成功的那一档：它要能在日志里被点名。
        assert_eq!(generate_status("brand_new_state"), GenerateStatus::Unknown);
    }

    /// ADR-0030 实施约束 6：源码里不再有 ensure——`ensure_model` / `EnsureReply` / `EnsureStatus` 一个不留，
    /// `/api/v1/model/ensure` 只允许出现在 [`generate_model`] 的 404 回退分支（那一个常量）里。
    /// 新名 `/api/v1/model/generate` 也只有那一个常量在打，显示路径（`data.rs` / `main.rs`）谁都不打。
    #[test]
    fn no_ensure_left_outside_the_fallback() {
        // 只看非测试部分：测试自己要写这些字面值当禁单。第一处 `mod tests {` 就是测试模块的头
        // （`rfind` 会撞上本测试里的这个字面值）。
        fn non_test(source: &str) -> &str {
            source
                .find("mod tests {")
                .map_or(source, |at| &source[..at])
        }
        let api_body = non_test(include_str!("model_update_api.rs"));
        assert_eq!(
            api_body.matches("\"/api/v1/model/ensure\"").count(),
            1,
            "旧路径只许在 LEGACY_GENERATE_PATH 那一个常量里出现"
        );
        assert_eq!(api_body.matches("\"/api/v1/model/generate\"").count(), 1);
        let fallback = api_body
            .split_once("match post(base, GENERATE_PATH, body.clone(), timeout).await {")
            .expect("先打新名")
            .1;
        assert!(
            fallback.contains("Err(error) if endpoint_missing(&error) =>")
                && fallback.contains("LEGACY_GENERATE_PATH"),
            "404 / 405 才退回旧名，同一请求体"
        );
        for (name, source) in [
            ("model_update_api.rs", api_body),
            ("data.rs", non_test(include_str!("data.rs"))),
            ("main.rs", non_test(include_str!("main.rs"))),
            ("sim.rs", non_test(include_str!("sim.rs"))),
            (
                "read_face/mod.rs",
                non_test(include_str!("read_face/mod.rs")),
            ),
            (
                "read_face/service.rs",
                non_test(include_str!("read_face/service.rs")),
            ),
            (
                "read_face/store.rs",
                non_test(include_str!("read_face/store.rs")),
            ),
        ] {
            for forbidden in [
                "ensure_model",
                "EnsureReply",
                "EnsureStatus",
                "ensure_status",
            ] {
                assert!(
                    !source.contains(forbidden),
                    "{name} 还在说 ensure：{forbidden}"
                );
            }
            if name != "model_update_api.rs" {
                assert!(
                    !source.contains("/api/v1/model/ensure")
                        && !source.contains("/api/v1/model/generate"),
                    "{name} 不许直接打生成接口的路径"
                );
            }
        }
    }

    /// 欠账端点的三种答案要分得开：取到了、**服务端没有这一档**、暂时取不到。
    ///
    /// 只有 404 / 405 / `not_found` 算第二种——那是 gen-model 2026-09-08 起模型工作
    /// 内联进 dbnum 任务、欠账表退役之后的常态，答案是可信的空，界面整段不画。
    /// 超时与 500 是第三种：沿用上一份快照，不许冒充「欠账清零」。
    #[test]
    fn only_a_missing_endpoint_counts_as_the_owed_table_being_gone() {
        let api =
            |code: &str, message: &str| anyhow::Error::new(ApiError(Failure::new(code, message)));
        assert!(pending_endpoint_retired(&api("not_found", "no such route")));
        assert!(pending_endpoint_retired(&api(
            "internal",
            "HTTP 404: unknown path"
        )));
        assert!(pending_endpoint_retired(&api(
            "internal",
            "HTTP 405: method not allowed"
        )));
        assert!(!pending_endpoint_retired(&api(
            "timeout",
            "io: Connection refused"
        )));
        assert!(!pending_endpoint_retired(&api(
            "internal",
            "HTTP 500: boom"
        )));
        assert!(!pending_endpoint_retired(&anyhow::anyhow!("解析响应失败")));
    }

    /// 连不上、超时、握手不成对用的人是同一件事：服务够不着，没有任何数据被改动，
    /// 直接重试即可。统一归 `timeout`，界面才给得出「可以直接重试」那句话。
    #[test]
    fn transport_failures_are_all_timeouts() {
        let failure = failure_of(&transport("Connection refused (os error 10061)".into()));
        assert_eq!(failure.form(), FailForm::Timeout);
        assert!(
            failure.message.contains("Connection refused"),
            "{}",
            failure.message
        );
    }

    #[test]
    fn decodes_preview_payload_and_enqueue_receipt() {
        let preview: Preview = serde_json::from_str(
            r#"{
                "project":"ProjAMS",
                "mdb":"/ALL",
                "dbnums":[{
                    "dbnum":7997,
                    "db_type":"DESI",
                    "file_name":"des000.db",
                    "file_path":"C:/project/des000.db",
                    "applied_sesno":41,
                    "file_latest_sesno":42,
                    "sessions":[{"sesno":42,"added":1,"modified":2,"deleted":0}],
                    "net_added":1,
                    "net_modified":2,
                    "net_deleted":0,
                    "model_affecting":3,
                    "units":[],
                    "zones":[{"zone_refno":"24384/1","name":"/ZONE","units":[{
                        "root_refno":"24384/12","noun":"BRAN","name":"/PIPE"
                    }]}],
                    "no_generation":0,
                    "blocked":false,
                    "anomaly":null,
                    "initialization_required":false
                },{
                    "dbnum":7015,
                    "db_type":"DESI",
                    "file_name":"",
                    "file_path":"",
                    "applied_sesno":0,
                    "file_latest_sesno":0,
                    "sessions":[],
                    "net_added":0,
                    "net_modified":0,
                    "net_deleted":0,
                    "model_affecting":0,
                    "units":[],
                    "no_generation":0,
                    "blocked":false,
                    "anomaly":null,
                    "initialization_required":true,
                    "not_in_project":true
                }],
                "pending_model_retries":[],
                "warnings":[],
                "up_to_date":false
            }"#,
        )
        .unwrap();
        assert_eq!(preview.dbnums[0].zones[0].units[0].noun, "BRAN");
        // 范围口径那两个字段：它们都带 `serde(default)`，服务端换个名字不会报错、
        // 只会静默给假值，所以这里断的是「真解出来了」而不是「解得动」。
        assert_eq!(preview.mdb, "/ALL");
        assert!(preview.dbnums[1].not_in_project);
        // 够不着的库带着 `initialization_required`——`not_in_project` 一旦没解出来，
        // 它立刻变成一个「需初始化 · 会执行」的批次。这条断言守的就是那一步。
        assert!(!preview.dbnums[1].will_run());

        assert!(serde_json::from_str::<Preview>("{}").is_err());
        // 入队回执与预览不同：**空回执是合法的**——一次扫描完全可能一行都不用排
        // （都并进了既有排队行，或者水位本来就最新），那不是契约缺字段。
        let idle = serde_json::from_str::<Enqueued>("{}").expect("空回执合法");
        assert!(idle.enqueued.is_empty());
    }

    /// 2026-09-20 之前的模型服务给无名元素的 `name` 是 noun 本身、不带 `short_name`，
    /// 树上那一行就只剩一个 `SITE`。客户端照 E3D 的规矩补短名：序号按属主分开数、只数
    /// 同名词、有名字的兄弟也占号；名词印全词（`ELBO` → `ELBOW`）。
    #[test]
    fn a_legacy_service_gets_e3d_short_names_for_unnamed_nodes() {
        let mut roots: TreeNodesReply = serde_json::from_str(
            r#"{"source":"direct","nodes":[
                {"refno":"24381_2","noun":"SITE","name":"/1RB-CIVI","owner":"24381_1","children_count":3,"dbnum":7997},
                {"refno":"24381_5","noun":"SITE","name":"SITE","owner":"24381_1","children_count":1,"dbnum":7997},
                {"refno":"14195_2","noun":"SITE","name":"SITE","owner":"14195_1","children_count":1,"dbnum":6003},
                {"refno":"24381_9","noun":"SITE","name":"SITE","owner":"24381_1","children_count":0,"dbnum":7997}
            ]}"#,
        )
        .unwrap();
        fill_legacy_default_names(&mut roots.nodes);
        let names: Vec<(&str, Option<&str>)> = roots
            .nodes
            .iter()
            .map(|node| (node.name.as_str(), node.short_name.as_deref()))
            .collect();
        // 一个 WORLD 一套序号：6003 那只是它自己 WORLD 下的第一个，不接着 7997 往下数。
        // 根层短名就是全名（WORLD 不进名字），两格相同。
        assert_eq!(
            names,
            [
                ("/1RB-CIVI", None),
                ("SITE 2", Some("SITE 2")),
                ("SITE 1", Some("SITE 1")),
                ("SITE 3", Some("SITE 3")),
            ]
        );

        let mut children: TreeNodesReply = serde_json::from_str(
            r#"{"source":"direct","parent":"24383_73930","nodes":[
                {"refno":"24383_73931","noun":"ATTA","name":"/1RCV-ATTA-1","owner":"24383_73930","children_count":0},
                {"refno":"24383_73932","noun":"ELBO","name":"ELBO","owner":"24383_73930","children_count":0},
                {"refno":"24383_73933","noun":"ATTA","name":"ATTA","owner":"24383_73930","children_count":0},
                {"refno":"24383_73934","noun":"WELD","name":"","owner":"24383_73930","children_count":0},
                {"refno":"24383_73935","noun":"ELBO","name":"ELBO","owner":"24383_73930","children_count":0}
            ]}"#,
        )
        .unwrap();
        fill_legacy_default_names(&mut children.nodes);
        let names: Vec<&str> = children
            .nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "/1RCV-ATTA-1",
                "ELBOW 1",
                "ATTACHMENT 2",
                "WELD 1",
                "ELBOW 2"
            ]
        );
    }

    /// 新服务端每个节点都带 `short_name`（有名的与 `name` 相同）：一格都不动，
    /// 整条 `FLNM` 原样留给「显示全称」和悬停。
    #[test]
    fn a_current_service_keeps_its_own_names() {
        let mut reply: TreeNodesReply = serde_json::from_str(
            r#"{"source":"direct","nodes":[
                {"refno":"24381_7","noun":"ZONE","name":"ZONE 4 of SITE 2","short_name":"ZONE 4","stored_name":null,"owner":"24381_5","children_count":2,"dbnum":7997},
                {"refno":"24381_8","noun":"ZONE","name":"/Z-A","short_name":"/Z-A","stored_name":"/Z-A","owner":"24381_5","children_count":0,"dbnum":7997}
            ]}"#,
        )
        .unwrap();
        fill_legacy_default_names(&mut reply.nodes);
        assert_eq!(reply.nodes[0].name, "ZONE 4 of SITE 2");
        assert_eq!(reply.nodes[0].short_name.as_deref(), Some("ZONE 4"));
        assert_eq!(reply.nodes[1].name, "/Z-A");
        assert_eq!(reply.nodes[1].short_name.as_deref(), Some("/Z-A"));
    }

    /// 模型更新面板的 SITE 名与树上那一行过同一道补名。说不出库号的（老服务端不注、
    /// 新服务端给 `null`）只是那一行不收——`null` 不许让整份根层回执解不动。
    #[test]
    fn root_dbs_share_the_tree_names_and_skip_unknown_databases() {
        let reply: TreeRootDbReply = serde_json::from_str(
            r#"{"source":"direct","nodes":[
                {"refno":"24381_2","noun":"SITE","name":"/1RB-CIVI","owner":"24381_1","children_count":3,"dbnum":7997},
                {"refno":"24381_5","noun":"SITE","name":"SITE","owner":"24381_1","children_count":1,"dbnum":7997},
                {"refno":"9304_2","noun":"SITE","name":"/1RS-CIVI","owner":"9304_1","children_count":1,"dbnum":null},
                {"refno":"15201_535","noun":"SITE","name":"SITE","owner":"15201_1","children_count":1}
            ]}"#,
        )
        .unwrap();
        assert_eq!(
            root_dbs(reply.nodes),
            [
                (7997, "/1RB-CIVI".to_string()),
                (7997, "SITE 2".to_string())
            ]
        );
    }
}
