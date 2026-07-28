use anyhow::Context;
use serde::de::DeserializeOwned;
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
pub async fn execute(
    base: &str,
    project: &str,
    mdb: &str,
    namespace: &str,
) -> anyhow::Result<Enqueued> {
    post(
        base,
        "/api/v1/update/execute",
        serde_json::json!({ "project": project, "mdb": mdb, "namespace": namespace }).to_string(),
        Duration::from_secs(60),
    )
    .await
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
    let tasks: TaskList = get(base, "/api/v1/tasks?limit=200").await?;
    // 后两份取不到不该让整次轮询作废：队列行已经在手上了，缺的只是「队列已重建」
    // 横幅与「欠 N 个单元」，各自缺席时那一格不画就是了。
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
        tasks: tasks.tasks,
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
    req.headers
        .insert("content-type", "application/json; charset=utf-8");
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
