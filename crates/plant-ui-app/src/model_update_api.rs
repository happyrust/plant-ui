use anyhow::Context;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::sync::OnceLock;
use std::time::Duration;

use plant_ui::model_update::{Enqueued, Failure, Preview};
use plant_ui::task_queue::{DbnumReport, Health, PendingUnits, Poll, QueueSnapshot, TaskList};

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
    let pending = get::<PendingUnits>(base, "/api/v1/update/pending-units").await;
    let pending_known = pending.is_ok();
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
        dbnums,
    })
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

/// 删掉一个 refno **精确子树**下已经生成的模型数据。
///
/// `confirm` 服务端强制要求等于 `refno`，不等就是 400——这个接口不接受随手一点。
/// 它删的是产物不是本体：`pe` 一行不动，删完那片元素在三维里就是「未加载」。
///
/// 容器也删得动（`WORL / SITE / ZONE` 在这里不受限，那道门只挡生成根），
/// 所以「整片删一次」只需要对右键那一行调一次。
pub async fn delete_model_subtree(
    base: &str,
    refno: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<()> {
    let query = format!(
        "refno={}&confirm={}&project={}&mdb={}&namespace={}",
        urlencode(refno),
        urlencode(refno),
        urlencode(project),
        urlencode(mdb),
        urlencode(namespace),
    );
    let _: serde_json::Value = delete(
        base,
        &format!("/api/v1/model/subtree?{query}"),
        Duration::from_secs(300),
    )
    .await?;
    Ok(())
}

/// `POST /api/v1/model/ensure` 的回执状态。
///
/// 四档对界面是四件不同的事，别压成一个布尔：`Generated` 是真做了一趟，
/// `AlreadyAvailable` 是同根的活刚被别的元素触发过（这正是删除之后靠
/// `force:false` 拿到的免费去重），`NoRenderableGeometry` 是这一片本来就没有
/// 可画的东西——它不是失败，重试一百遍还是同一个结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnsureStatus {
    Generated,
    AlreadyAvailable,
    NoRenderableGeometry,
    /// 服务端换了新状态名。当成「做过了」计数，但要在日志里点名。
    Unknown,
}

#[derive(Debug, Clone)]
pub struct EnsureReply {
    pub status: EnsureStatus,
    /// 服务端解出来的生成根。客户端归根算错时，这一份是唯一的对照物。
    pub generation_root: String,
    pub model_available: bool,
}

#[derive(Deserialize)]
struct EnsureBody {
    #[serde(default)]
    status: String,
    #[serde(default)]
    generation_root: String,
    #[serde(default)]
    model_available: bool,
}

/// 让一个 refno 有可渲染模型。`force` 只在人明确要求「无论如何重跑一遍」时为真。
///
/// **重新生成走的是 `force = false`**：删除已经把这一片清空了，第一个元素触发
/// 真生成，同根后面的元素读到 `renderable > 0` 直接回 `AlreadyAvailable`——
/// 去重是服务端免费给的，客户端不必自己裁剪嵌套单元。
///
/// 超时给到 125 秒，比服务端那道 120 秒稍长：让服务端的超时语义先生效
/// （它回 504 并说明后台继续跑），而不是客户端先把连接掐掉、什么都不知道。
pub async fn ensure_model(
    base: &str,
    refno: &str,
    force: bool,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<EnsureReply> {
    let body = serde_json::json!({
        "refno": refno,
        "force": force,
        "project": project,
        "mdb": mdb,
        "namespace": namespace,
    });
    let reply: EnsureBody = post(
        base,
        "/api/v1/model/ensure",
        body.to_string(),
        Duration::from_secs(125),
    )
    .await?;
    Ok(EnsureReply {
        status: ensure_status(&reply.status),
        generation_root: reply.generation_root,
        model_available: reply.model_available,
    })
}

fn ensure_status(raw: &str) -> EnsureStatus {
    match raw {
        "generated" | "Generated" => EnsureStatus::Generated,
        "already_available" | "AlreadyAvailable" => EnsureStatus::AlreadyAvailable,
        "no_renderable_geometry" | "NoRenderableGeometry" => EnsureStatus::NoRenderableGeometry,
        _ => EnsureStatus::Unknown,
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

/// `ehttp` 只给了 get / post / head 三个构造器，DELETE 自己改 `method`。
/// 服务端那条路由把参数全放在 query 串里，所以不带请求体。
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

    /// `DELETE /model/subtree` 的 `confirm` 必须与 `refno` 逐字相同，且 `/`
    /// 要转义——服务端拿它当「你确实想删这一个」的凭据，不等就是 400。
    #[test]
    fn the_delete_confirm_matches_the_refno_after_escaping() {
        assert_eq!(urlencode("24381/100677"), "24381%2F100677");
        assert_eq!(urlencode("/ALL"), "%2FALL");
        assert_eq!(urlencode("ProjAMS"), "ProjAMS");

        let refno = "24381/100677";
        let query = format!("refno={}&confirm={}", urlencode(refno), urlencode(refno));
        assert_eq!(query, "refno=24381%2F100677&confirm=24381%2F100677");
    }

    /// ensure 的四档状态对界面是四件不同的事，压成布尔就分不出「重做过了」与
    /// 「同根刚被别人做过」——后者正是删除之后 `force:false` 拿到的免费去重。
    #[test]
    fn ensure_status_keeps_the_four_outcomes_apart() {
        assert_eq!(ensure_status("generated"), EnsureStatus::Generated);
        assert_eq!(
            ensure_status("already_available"),
            EnsureStatus::AlreadyAvailable
        );
        assert_eq!(
            ensure_status("no_renderable_geometry"),
            EnsureStatus::NoRenderableGeometry
        );
        // 服务端换名字不许静默当成成功的那一档：它要能在日志里被点名。
        assert_eq!(ensure_status("brand_new_state"), EnsureStatus::Unknown);
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
}
