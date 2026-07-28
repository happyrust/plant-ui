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

/// rs-server 数据中心接口：专业发布复用 `ThreeDDatacenterRequest`；房间查询走服务端
/// 已有的 `PipeNameRequest[]` 契约。
pub async fn submit(base: &str, request: &PublishRequest) -> anyhow::Result<String> {
    let body = request_body(request)?.to_string();
    let mut req = ehttp::Request::post(
        format!("{base}{}", request.category.endpoint()),
        body.into_bytes(),
    );
    req.headers
        .insert("content-type", "application/json; charset=utf-8");
    req.timeout = Some(Duration::from_secs(60));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(anyhow::Error::msg)
        .context("请求数据服务失败")?;
    let status = response.status;
    let body = response.text().context("数据服务响应不是 UTF-8")?;
    if response.ok {
        Ok(body.to_owned())
    } else {
        anyhow::bail!("数据服务返回 HTTP {status}: {body}")
    }
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
        _ => serde_json::json!({
            "refnos": request.elements.iter().map(|element| element.refno.to_string()).collect::<Vec<_>>(),
            "title": request.title,
            "create_rvm_relations": true,
            "b_first_time_design": request.design_phase == DesignPhase::Preliminary,
        }),
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
}
