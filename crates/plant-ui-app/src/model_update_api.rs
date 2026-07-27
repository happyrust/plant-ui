use anyhow::{Context, bail};
use serde::de::DeserializeOwned;
use std::time::Duration;

use plant_ui::model_update::{Accepted, Preview, Run};

pub fn base_url() -> String {
    std::env::var("PLANT_MODEL_API_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3100".into())
        .trim_end_matches('/')
        .to_owned()
}

pub fn preview(base: &str, project: &str) -> anyhow::Result<Preview> {
    post(
        base,
        "/api/v1/update/preview",
        serde_json::json!({ "project": project, "dbnums": [] }).to_string(),
        Duration::from_secs(600),
    )
}

pub fn execute(base: &str, project: &str, dbnums: &[u32]) -> anyhow::Result<Accepted> {
    post(
        base,
        "/api/v1/update/execute",
        serde_json::json!({ "project": project, "dbnums": dbnums }).to_string(),
        Duration::from_secs(60),
    )
}

pub fn task(base: &str, run_id: &str) -> anyhow::Result<Run> {
    let response: TaskResponse = request(
        agent(Duration::from_secs(15))
            .get(format!("{base}/api/v1/tasks/{run_id}"))
            .call()
            .context("请求模型更新任务状态失败")?,
    )?;
    Ok(response.run)
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
            .and_then(|value| value.error)
            .unwrap_or(body);
        bail!("HTTP {status}: {message}");
    }
    serde_json::from_str(&body).context("解析模型更新服务响应失败")
}

#[derive(serde::Deserialize)]
struct TaskResponse {
    run: Run,
}

#[derive(serde::Deserialize)]
struct ErrorResponse {
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_zone_bucket_and_terminal_task_contract() {
        let preview: Preview = serde_json::from_str(
            r#"{
                "project":"ProjAMS",
                "execution_scope":"dbnum+sesno",
                "dbnums":[{
                    "dbnum":7997,
                    "sessions":[{"sesno":42,"added":1,"modified":2,"deleted":0,"changed_count":3}],
                    "zones":[{"zone_refno":"24384/1","unit_count":1,"units":[{
                        "root_refno":"24384/12","noun":"BRAN","model_category":"BRAN"
                    }]}]
                }]
            }"#,
        )
        .unwrap();
        assert_eq!(preview.dbnums[0].zones[0].units[0].noun, "BRAN");

        let run: TaskResponse = serde_json::from_str(
            r#"{"run":{"run_id":"sync+generate-db7997","dbnum":7997,"state":"succeeded"}}"#,
        )
        .unwrap();
        assert!(run.run.terminal());
    }
}
