use anyhow::{Context, bail};
use serde::de::DeserializeOwned;
use std::time::Duration;

use plant_ui::model_update::{Accepted, Preview, Run};

pub fn base_url() -> String {
    std::env::var("PLANT_MODEL_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8020".into())
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
    request(
        agent(Duration::from_secs(15))
            .get(format!("{base}/api/v1/tasks/{run_id}"))
            .call()
            .context("请求模型更新任务状态失败")?,
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
        .context("请求模型更新服务失败")?;
    request(response)
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
        let message = serde_json::from_str::<ErrorResponse>(&body)
            .ok()
            .and_then(|value| value.message)
            .unwrap_or(body);
        bail!("HTTP {status}: {message}");
    }
    serde_json::from_str(&body).context("解析模型更新服务响应失败")
}

#[derive(serde::Deserialize)]
struct ErrorResponse {
    message: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_zone_bucket_and_terminal_task_contract() {
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
