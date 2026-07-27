use anyhow::Context;
use serde::de::DeserializeOwned;
use std::time::Duration;

use plant_ui::model_update::{Accepted, Failure, Preview, Run};

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
    std::env::var("PLANT_MODEL_API_URL")
        .unwrap_or_else(|_| plant_ui::settings::DEFAULT_MODEL_API_URL.into())
        .trim_end_matches('/')
        .to_owned()
}

pub fn preview(base: &str, project: &str) -> anyhow::Result<Preview> {
    post(
        base,
        "/api/v1/update/preview",
        serde_json::json!({ "project": project }).to_string(),
        Duration::from_secs(600),
    )
}

pub fn execute(base: &str, project: &str) -> anyhow::Result<Accepted> {
    post(
        base,
        "/api/v1/update/execute",
        serde_json::json!({ "project": project }).to_string(),
        Duration::from_secs(60),
    )
}

pub fn task(base: &str, run_id: &str) -> anyhow::Result<Run> {
    let response = agent(Duration::from_secs(15))
        .get(format!("{base}/api/v1/tasks/{run_id}"))
        .call()
        .map_err(transport)
        .context("请求模型更新任务状态失败")?;
    request(response)
}

/// `POST /api/v1/model/ensure` 的回执里本端用得上的那两个数。
///
/// 不收 `status`：带 `force` 发出去的请求不会回 `AlreadyAvailable`，剩下的两种终态
/// 「画得出来」与「画不出来」这两个计数就分得干净，没必要再跟服务端的枚举名对齐。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Ensured {
    /// 画得出来的实例数。
    #[serde(default)]
    pub model_instance_count: usize,
    /// 生成写出的实例数，含画不出来的。两者不等就是生成跑过但缺几何。
    #[serde(default)]
    pub generated_instance_count: usize,
}

/// 重新生成一个交付单元——S4-C 上那枚「重试」就是它。
///
/// 带 `force`：人按下重试就是要它重跑一遍。不带的话，服务端对「已经生成过、
/// 只是画不出来」的生成根会直接回状态，那正是这枚按钮不该有的行为。
pub fn ensure_model(base: &str, root_refno: &str) -> anyhow::Result<Ensured> {
    post(
        base,
        "/api/v1/model/ensure",
        serde_json::json!({ "refno": root_refno, "force": true }).to_string(),
        Duration::from_secs(120),
    )
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(timeout))
        .build()
        .new_agent()
}

fn post<T: DeserializeOwned>(
    base: &str,
    path: &str,
    body: String,
    timeout: Duration,
) -> anyhow::Result<T> {
    let response = agent(timeout)
        .post(format!("{base}{path}"))
        .header("content-type", "application/json")
        .send(body)
        .map_err(transport)
        .context("请求模型更新服务失败")?;
    request(response)
}

/// 传输层失败：连不上、超时、握手不成。对用的人来说这些是同一件事——服务够不着，
/// 没有任何数据被改动，直接重试即可，所以统一归 `timeout`。
fn transport(error: ureq::Error) -> anyhow::Error {
    anyhow::Error::new(ApiError(Failure::new("timeout", error.to_string())))
}

fn request<T: DeserializeOwned>(
    mut response: ureq::http::Response<ureq::Body>,
) -> anyhow::Result<T> {
    let status = response.status();
    let body = response
        .body_mut()
        .read_to_string()
        .context("读取模型更新服务响应失败")?;
    if !status.is_success() {
        return Err(anyhow::Error::new(ApiError(error_packet(status, &body))));
    }
    serde_json::from_str(&body).context("解析模型更新服务响应失败")
}

/// 解错误包封。`code` 缺失时按状态码兜底——422 是前置条件不满足、409 是任务冲突、
/// 504 是超时，其余一律 internal。
fn error_packet(status: ureq::http::StatusCode, body: &str) -> Failure {
    let packet = serde_json::from_str::<ErrorResponse>(body).unwrap_or_default();
    let code = packet.code.unwrap_or_else(|| {
        match status.as_u16() {
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
    fn decodes_preview_payload_and_terminal_task_contract() {
        let preview: Preview = serde_json::from_str(
            r#"{
                "project":"ProjAMS",
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
                }],
                "pending_model_retries":[],
                "warnings":[],
                "up_to_date":false
            }"#,
        )
        .unwrap();
        assert_eq!(preview.dbnums[0].zones[0].units[0].noun, "BRAN");

        let run: Run = serde_json::from_str(
            r#"{
                "task_id":"mu-1",
                "kind":"manual_update",
                "project":"ProjAMS",
                "state":"partial",
                "created_at":"2026-07-27T10:00:00+08:00",
                "finished_at":"2026-07-27T10:01:00+08:00",
                "events_seen":3,
                "result":{"batches":[],"units":[],"warnings":[]}
            }"#,
        )
        .unwrap();
        assert!(run.terminal());
        assert!(serde_json::from_str::<Preview>("{}").is_err());
        assert!(serde_json::from_str::<Accepted>("{}").is_err());
        assert!(serde_json::from_str::<Run>("{}").is_err());
    }
}
