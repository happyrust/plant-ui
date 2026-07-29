use aios_core::data_center::{ThreeDDatacenterRequest, ThreeDDatacenterResponse};
use anyhow::Context;
use plant_ui::data_publish::{DesignPhase, PublishCategory, PublishRequest};
use std::sync::OnceLock;
use std::time::Duration;

static BASE_URL: OnceLock<String> = OnceLock::new();

pub fn base_url() -> String {
    BASE_URL
        .get()
        .cloned()
        .or_else(|| std::env::var("PLANT_DATA_API_URL").ok())
        .unwrap_or_else(|| plant_ui::settings::DEFAULT_DATA_API_URL.into())
        .trim()
        .trim_end_matches('/')
        .to_owned()
}

pub fn set_base_url(base: String) -> anyhow::Result<()> {
    BASE_URL
        .set(base.trim_end_matches('/').to_owned())
        .map_err(|_| anyhow::anyhow!("数据服务地址已初始化，不能重复覆盖"))
}

/// 一次成功提交的服务端回执。
///
/// 专业发布会返回数据中心的 `LoginUrl`；房间接口没有该字段时保持为空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitResult {
    pub message: String,
    pub login_url: Option<String>,
}

/// rs-server 数据中心接口：专业发布复用 `ThreeDDatacenterRequest`；房间查询走服务端
/// 已有的 `PipeNameRequest[]` 契约。专业发布的响应按
/// `ThreeDDatacenterResponse` 判断业务成功，而不只依赖 HTTP 状态。
pub async fn submit(base: &str, request: &PublishRequest) -> anyhow::Result<SubmitResult> {
    let body = request_body(request)?.to_string();
    let url = format!("{base}{}", request.category.endpoint());
    eprintln!("[data_publish] POST {url}\n[data_publish] request: {body}");
    let mut req = http_request(url, body);
    req.timeout = Some(Duration::from_secs(60));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(anyhow::Error::msg)
        .context("请求数据服务失败")?;
    let status = response.status;
    let body = response.text().context("数据服务响应不是 UTF-8")?;
    if !response.ok {
        anyhow::bail!("数据服务返回 HTTP {status}: {body}")
    }

    response_body(request.category, body)
}

fn http_request(url: String, body: String) -> ehttp::Request {
    ehttp::Request::new(
        ehttp::Method::POST,
        url,
        ehttp::Headers::new(&[("content-type", "application/json; charset=utf-8")]),
    )
    .with_body(body.into_bytes())
}

fn request_body(request: &PublishRequest) -> anyhow::Result<serde_json::Value> {
    if request.elements.is_empty() {
        anyhow::bail!("请至少添加一个元素");
    }
    Ok(match request.category {
        PublishCategory::Room => serde_json::json!(
            request
                .elements
                .iter()
                .map(|element| serde_json::json!({ "name": element.name, "position": [] }))
                .collect::<Vec<_>>()
        ),
        _ => serde_json::to_value(ThreeDDatacenterRequest {
            refnos: request
                .elements
                .iter()
                .map(|element| element.refno.to_string())
                .collect(),
            title: request.title.clone(),
            create_rvm_relations: true,
            b_first_time_design: request.design_phase == DesignPhase::Preliminary,
        })?,
    })
}

fn response_body(category: PublishCategory, body: &str) -> anyhow::Result<SubmitResult> {
    if category == PublishCategory::Room {
        return Ok(SubmitResult {
            message: body.to_owned(),
            login_url: None,
        });
    }

    let response: ThreeDDatacenterResponse =
        serde_json::from_str(body).context("数据服务响应不符合 ThreeDDatacenterResponse 契约")?;
    if !response.success {
        anyhow::bail!(
            "{}",
            if response.result.is_empty() {
                "数据中心发布失败"
            } else {
                &response.result
            }
        );
    }

    Ok(SubmitResult {
        message: response.result,
        login_url: (!response.login_url.trim().is_empty()).then_some(response.login_url),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_ui::{RefU64, data_publish::PublishElement};

    fn request(category: PublishCategory) -> PublishRequest {
        PublishRequest {
            title: "发布测试".into(),
            category,
            design_phase: DesignPhase::Detailed,
            elements: vec![PublishElement {
                refno: RefU64::from(12_345_u64),
                name: "/PIPE-100".into(),
            }],
        }
    }

    #[test]
    fn professional_publish_uses_the_datacenter_contract() {
        assert_eq!(
            request_body(&request(PublishCategory::Process)).unwrap(),
            serde_json::json!({
                "refnos": ["0_12345"],
                "title": "发布测试",
                "create_rvm_relations": true,
                "b_first_time_design": false,
            })
        );
    }

    #[test]
    fn room_query_uses_the_pipe_room_contract() {
        assert_eq!(
            request_body(&request(PublishCategory::Room)).unwrap(),
            serde_json::json!([{"name": "/PIPE-100", "position": []}])
        );
    }

    #[test]
    fn empty_publish_requests_are_rejected() {
        let mut request = request(PublishCategory::Process);
        request.elements.clear();
        assert!(request_body(&request).is_err());
    }

    #[test]
    fn publish_http_request_has_one_json_content_type() {
        let request = http_request("http://127.0.0.1:9099/get_gy_bran_data".into(), "{}".into());

        assert_eq!(request.method, ehttp::Method::POST);
        assert_eq!(
            request.headers.get_all("content-type").collect::<Vec<_>>(),
            vec!["application/json; charset=utf-8"]
        );
        assert_eq!(request.body, b"{}");
    }

    #[test]
    fn professional_response_uses_success_result_and_login_url() {
        let result = response_body(
            PublishCategory::Process,
            r#"{"Success":true,"Result":"已提交","KeyValue":"","LoginUrl":"https://example.test/login"}"#,
        )
        .unwrap();

        assert_eq!(result.message, "已提交");
        assert_eq!(
            result.login_url.as_deref(),
            Some("https://example.test/login")
        );
    }

    #[test]
    fn professional_response_rejects_business_failures() {
        let error = response_body(
            PublishCategory::Process,
            r#"{"Success":false,"Result":"发布被拒绝","KeyValue":"","LoginUrl":""}"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("发布被拒绝"));
    }
}
