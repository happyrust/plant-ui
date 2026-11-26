use aios_core::data_center::{ThreeDDatacenterRequest, ThreeDDatacenterResponse};
use anyhow::Context;
use plant_ui::data_publish::{
    DeletionCandidate, DesignPhase, PreviewAttributeRow, PreviewInstanceRow, PreviewPayloadLayer,
    PreviewRelationRow, PreviewSkippedRow, PublishCategory, PublishPreviewData, PublishRequest,
};
use plant_ui::manual_data_publish::RoomCodePublishRequest;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

static BASE_URL: OnceLock<String> = OnceLock::new();
static AUTH_TOKEN: RwLock<Option<String>> = RwLock::new(None);
static INITIAL_USER: RwLock<Option<crate::startup::AuthUser>> = RwLock::new(None);

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

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn set_auth_token(token: Option<String>) {
    let cleaned = token
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    if let Ok(mut guard) = AUTH_TOKEN.write() {
        *guard = cleaned;
    }
}

pub fn auth_token() -> Option<String> {
    AUTH_TOKEN.read().ok().and_then(|guard| guard.clone())
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn set_initial_user(user: Option<crate::startup::AuthUser>) {
    let cleaned = user.filter(|u| !u.user_name.trim().is_empty() || !u.chinese_name.trim().is_empty());
    if let Ok(mut guard) = INITIAL_USER.write() {
        *guard = cleaned;
    }
}

pub fn initial_user() -> Option<crate::startup::AuthUser> {
    INITIAL_USER.read().ok().and_then(|guard| guard.clone())
}

/// 一次成功提交的服务端回执。
///
/// 数据中心成功响应中的 `LoginUrl`；未返回时保持为空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitResult {
    pub message: String,
    pub login_url: Option<String>,
}

/// rs-server 数据中心接口：所有发布类别均复用 `ThreeDDatacenterRequest`。
/// 响应按 `ThreeDDatacenterResponse` 判断业务成功，而不只依赖 HTTP 状态。
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

pub async fn lookup_deletions(
    base: &str,
    request: &PublishRequest,
) -> anyhow::Result<Vec<DeletionCandidate>> {
    let payload = deletion_lookup_body(request)?.to_string();
    let url = format!("{base}/get_datacenter_delete_by_refnos");
    eprintln!(
        "[data_publish_delete_lookup] POST {url}\n[data_publish_delete_lookup] request: {payload}"
    );
    let mut req = http_request(url.clone(), payload.clone());
    req.timeout = Some(Duration::from_secs(60));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("POST {url} 请求失败；请求体：{payload}"))?;
    let status = response.status;
    let body = response
        .text()
        .with_context(|| format!("POST {url} 返回 HTTP {status}，响应不是 UTF-8"))?;
    if !response.ok {
        anyhow::bail!("POST {url} 返回 HTTP {status}；请求体：{payload}；响应体：{body}")
    }
    serde_json::from_str(body).with_context(|| {
        format!("POST {url} 返回 HTTP {status}，响应不是 id/name 数组；响应体：{body}")
    })
}

fn deletion_lookup_body(request: &PublishRequest) -> anyhow::Result<serde_json::Value> {
    if request.elements.is_empty() {
        anyhow::bail!("请至少添加一个元素");
    }
    Ok(serde_json::json!({
        "refnos": request.elements.iter().map(|element| {
            element.refno.to_string().replace('_', "/")
        }).collect::<Vec<_>>(),
    }))
}

pub async fn submit_room_codes(
    base: &str,
    request: &RoomCodePublishRequest,
) -> anyhow::Result<SubmitResult> {
    if request.room_codes.is_empty() {
        anyhow::bail!("请至少填写一条设备名称和房间号");
    }
    let body = serde_json::to_string(request)?;
    let url = format!("{base}/send_room_code_to_data_center");
    eprintln!("[room_code_publish] POST {url}\n[room_code_publish] request: {body}");
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
    three_d_response_body(body)
}

fn http_request(url: String, body: String) -> ehttp::Request {
    http_request_with_token(url, body, auth_token().as_deref())
}

fn http_request_with_token(url: String, body: String, token: Option<&str>) -> ehttp::Request {
    let mut headers = ehttp::Headers::new(&[("content-type", "application/json; charset=utf-8")]);
    if let Some(token) = token.map(str::trim).filter(|t| !t.is_empty()) {
        headers.insert("authorization", format!("Bearer {token}"));
    }
    ehttp::Request::new(ehttp::Method::POST, url, headers).with_body(body.into_bytes())
}

pub async fn fetch_current_user(base: &str) -> anyhow::Result<Option<crate::startup::AuthUser>> {
    let url = format!("{}/get_user", base.trim_end_matches('/'));
    let mut headers = ehttp::Headers::default();
    if let Some(token) = auth_token() {
        headers.insert("authorization", format!("Bearer {token}"));
    }
    let mut req = ehttp::Request::new(ehttp::Method::GET, url.clone(), headers);
    req.timeout = Some(Duration::from_secs(15));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("GET {url} 请求失败"))?;
    if response.status == 401 || response.status == 403 {
        return Ok(None);
    }
    let body = response
        .text()
        .with_context(|| format!("GET {url} 返回 HTTP {}，响应不是 UTF-8", response.status))?;
    if !response.ok {
        anyhow::bail!("GET {url} 返回 HTTP {}: {body}", response.status);
    }
    let user: crate::startup::AuthUser = serde_json::from_str(body)
        .with_context(|| format!("解析 {url} 用户信息失败: {body}"))?;
    Ok(Some(user))
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct DataServiceMeta {
    #[serde(default)]
    pub version: String,
}

/// 查询 rs-server `/api/metadata/meta` 获取数据中心服务版本号。
pub async fn fetch_meta(base: &str) -> anyhow::Result<DataServiceMeta> {
    let url = format!("{}/api/metadata/meta", base.trim_end_matches('/'));
    let mut req = ehttp::Request::new(ehttp::Method::GET, url.clone(), ehttp::Headers::default());
    req.timeout = Some(Duration::from_secs(10));
    let response = ehttp::fetch_async(req)
        .await
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("GET {url} 请求失败"))?;
    let body = response
        .text()
        .with_context(|| format!("GET {url} 返回 HTTP {}，响应不是 UTF-8", response.status))?;
    if !response.ok {
        anyhow::bail!("GET {url} 返回 HTTP {}: {body}", response.status);
    }
    serde_json::from_str(body).with_context(|| format!("解析 {url} 服务元信息失败: {body}"))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewCreateResponse {
    preview_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPreviewView {
    status: String,
    elapsed_ms: Option<u64>,
    result: Option<RawPreviewResult>,
    error: Option<RawPreviewError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPreviewError {
    #[serde(default)]
    stage: String,
    message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPreviewResult {
    internal: serde_json::Value,
    datacenter: serde_json::Value,
    #[serde(default)]
    datacenter_layer: String,
    #[serde(default)]
    refno_map: Vec<RawRefnoMapping>,
    #[serde(default)]
    skipped: Vec<RawSkippedNode>,
    #[serde(default)]
    warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawRefnoMapping {
    refno: String,
    instance_code: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawSkippedNode {
    refno: String,
    reason: String,
}

pub async fn preview(base: &str, request: &PublishRequest) -> anyhow::Result<PublishPreviewData> {
    if request.elements.is_empty() {
        anyhow::bail!("请至少添加一个提资元素");
    }
    let Some((discipline, target)) = request.category.preview_route() else {
        anyhow::bail!("当前提资类型暂不支持后端元数据提取预览");
    };
    let clean_base = base.trim_end_matches('/');
    let payload = preview_request_body(request, discipline, target).to_string();
    let create_url = format!("{clean_base}/api/metadata/previews");
    let mut create_req = http_request(create_url.clone(), payload.clone());
    create_req.timeout = Some(Duration::from_secs(30));
    let create_resp = ehttp::fetch_async(create_req)
        .await
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("POST {create_url} 请求失败"))?;
    let create_body = create_resp
        .text()
        .with_context(|| format!("POST {create_url} 响应不是 UTF-8"))?;
    if !create_resp.ok {
        anyhow::bail!("创建预览失败 (HTTP {}): {create_body}", create_resp.status);
    }
    let created: PreviewCreateResponse = serde_json::from_str(create_body)
        .with_context(|| format!("解析预览创建响应失败: {create_body}"))?;

    let poll_url = format!("{clean_base}/api/metadata/previews/{}", created.preview_id);
    for _ in 0..200 {
        let mut headers = ehttp::Headers::default();
        if let Some(token) = auth_token() {
            headers.insert("authorization", format!("Bearer {token}"));
        }
        let mut poll_req = ehttp::Request::new(ehttp::Method::GET, poll_url.clone(), headers);
        poll_req.timeout = Some(Duration::from_secs(30));
        let poll_resp = ehttp::fetch_async(poll_req)
            .await
            .map_err(anyhow::Error::msg)
            .with_context(|| format!("GET {poll_url} 轮询失败"))?;
        let poll_body = poll_resp
            .text()
            .with_context(|| format!("GET {poll_url} 响应不是 UTF-8"))?;
        if !poll_resp.ok {
            anyhow::bail!("查询预览失败 (HTTP {}): {poll_body}", poll_resp.status);
        }
        if let Some(parsed) = parse_preview_poll_step(poll_body)? {
            return Ok(parsed);
        }
        sleep_ms(300).await;
    }
    anyhow::bail!("预览超时（超过 60 秒仍在运行）")
}

fn preview_request_body(
    request: &PublishRequest,
    discipline: &str,
    target: &str,
) -> serde_json::Value {
    serde_json::json!({
        "discipline": discipline,
        "target": target,
        "title": request.title,
        "refnos": request
            .elements
            .iter()
            .map(|element| element.refno.to_string())
            .collect::<Vec<_>>(),
        "createRvmRelations": true,
        "bFirstTimeDesign": matches!(request.design_phase, DesignPhase::Preliminary),
        "deleteRefnos": [],
    })
}

fn parse_preview_poll_step(body: &str) -> anyhow::Result<Option<PublishPreviewData>> {
    let view: RawPreviewView =
        serde_json::from_str(body).with_context(|| format!("解析预览结果失败: {body}"))?;
    match view.status.as_str() {
        "running" => Ok(None),
        "done" => Ok(Some(parse_preview_view(body)?)),
        "failed" => {
            if let Some(err) = view.error {
                if err.stage.trim().is_empty() {
                    anyhow::bail!("{}", err.message);
                } else {
                    anyhow::bail!("{}：{}", err.stage, err.message);
                }
            } else {
                anyhow::bail!("服务端预览任务失败");
            }
        }
        "cancelled" => anyhow::bail!("预览任务已取消"),
        other => anyhow::bail!("未知预览状态：{other}"),
    }
}

pub fn parse_preview_view(body: &str) -> anyhow::Result<PublishPreviewData> {
    let view: RawPreviewView =
        serde_json::from_str(body).with_context(|| format!("解析预览结果失败: {body}"))?;
    let Some(result) = view.result else {
        anyhow::bail!("预览已完成但缺少 result 字段");
    };
    let mut refno_by_code = HashMap::new();
    for item in result.refno_map {
        refno_by_code.insert(item.instance_code, item.refno);
    }
    let datacenter = parse_payload_layer(&result.datacenter, &refno_by_code);
    let internal = parse_payload_layer(&result.internal, &refno_by_code);
    let skipped = result
        .skipped
        .into_iter()
        .map(|item| PreviewSkippedRow {
            refno: item.refno,
            reason: item.reason,
        })
        .collect();
    Ok(PublishPreviewData {
        datacenter_layer: result.datacenter_layer,
        elapsed_ms: view.elapsed_ms,
        datacenter,
        internal,
        skipped,
        warnings: result.warnings,
    })
}

fn parse_payload_layer(
    val: &serde_json::Value,
    refno_by_code: &HashMap<String, String>,
) -> PreviewPayloadLayer {
    let project_code = json_str(val, "projectCode");
    let owner = json_str(val, "owner");
    let instances = val
        .get("instances")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|item| parse_instance_row(item, refno_by_code))
                .collect()
        })
        .unwrap_or_default();
    let relations = val
        .get("relations")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(parse_relation_row).collect())
        .unwrap_or_default();
    PreviewPayloadLayer {
        project_code,
        owner,
        instances,
        relations,
    }
}

fn parse_instance_row(
    val: &serde_json::Value,
    refno_by_code: &HashMap<String, String>,
) -> PreviewInstanceRow {
    let object_model_code = json_str(val, "objectModelCode");
    let operate = json_str(val, "operate");
    let version = json_str(val, "version");
    let attributes = parse_attributes(val);
    let top_code = json_str(val, "instanceCode");
    let instance_code = if !top_code.is_empty() {
        top_code
    } else {
        ["PBS_ID", "ID", "MOD_ENC"]
            .into_iter()
            .find_map(|key| find_attr(&attributes, key).map(str::to_owned))
            .unwrap_or_default()
    };
    let refno = refno_by_code.get(&instance_code).cloned();
    PreviewInstanceRow {
        object_model_code,
        instance_code,
        refno,
        operate,
        version,
        attributes,
    }
}

fn parse_relation_row(val: &serde_json::Value) -> PreviewRelationRow {
    let object_model_code = json_str(val, "objectModelCode");
    let operate = json_str(val, "operate");
    let version = json_str(val, "version");
    let attributes = parse_attributes(val);

    let from_summary = if let Some(start_inst) = find_attr(&attributes, "START_INST_CODE") {
        match find_attr(&attributes, "START_OBJ_CODE") {
            Some(obj) if !obj.is_empty() => format!("{obj}: {start_inst}"),
            _ => start_inst.to_owned(),
        }
    } else if let Some(pbs_id) = find_attr(&attributes, "PBS_ID") {
        pbs_id.to_owned()
    } else {
        "—".to_owned()
    };

    let end_inst = find_attr(&attributes, "END_INST_CODE");
    let end_obj = find_attr(&attributes, "EDN_OBJ_CODE").or_else(|| find_attr(&attributes, "END_OBJ_CODE"));
    let to_summary = if let Some(inst) = end_inst {
        match end_obj {
            Some(obj) if !obj.is_empty() => format!("{obj}: {inst}"),
            _ => inst.to_owned(),
        }
    } else if let Some(mod_id) = find_attr(&attributes, "THR_DIM_MOD_ID") {
        mod_id.to_owned()
    } else {
        "—".to_owned()
    };

    PreviewRelationRow {
        object_model_code,
        operate,
        version,
        from_summary,
        to_summary,
        attributes,
    }
}

fn parse_attributes(val: &serde_json::Value) -> Vec<PreviewAttributeRow> {
    val.get("attributes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|attr| PreviewAttributeRow {
                    code: json_str(attr, "attributeModelCode"),
                    value: json_value_string(attr.get("value")),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn find_attr<'a>(attrs: &'a [PreviewAttributeRow], code: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|attr| attr.code == code && !attr.value.is_empty())
        .map(|attr| attr.value.as_str())
}

fn json_str(val: &serde_json::Value, key: &str) -> String {
    val.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}

fn json_value_string(val: Option<&serde_json::Value>) -> String {
    match val {
        None | Some(serde_json::Value::Null) => String::new(),
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn sleep_ms(ms: u32) {
    tokio::time::sleep(Duration::from_millis(u64::from(ms))).await;
}

#[cfg(target_arch = "wasm32")]
async fn sleep_ms(ms: u32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        if let Some(window) = web_sys::window() {
            let _ = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
        } else {
            let _ = resolve.call0(&wasm_bindgen::JsValue::UNDEFINED);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

fn request_body(request: &PublishRequest) -> anyhow::Result<serde_json::Value> {
    if request.elements.is_empty() {
        anyhow::bail!("请至少添加一个元素");
    }
    let mut body = serde_json::to_value(ThreeDDatacenterRequest {
        refnos: request
            .elements
            .iter()
            .map(|element| element.refno.to_string())
            .collect(),
        title: request.title.clone(),
        create_rvm_relations: true,
        b_first_time_design: matches!(request.design_phase, DesignPhase::Preliminary),
    })?;
    if matches!(
        request.category,
        PublishCategory::Process
            | PublishCategory::Electrical
            | PublishCategory::Instrumentation
            | PublishCategory::Ventilation
    ) {
        body.as_object_mut()
            .expect("ThreeDDatacenterRequest 必须序列化为 JSON 对象")
            .insert(
                "delete_refnos".into(),
                serde_json::to_value(&request.delete_refnos)?,
            );
    }
    Ok(body)
}

fn response_body(_category: PublishCategory, body: &str) -> anyhow::Result<SubmitResult> {
    three_d_response_body(body)
}

fn summarize_response_body(body: &str) -> String {
    let compact: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return "空响应".to_string();
    }
    const MAX_CHARS: usize = 200;
    let mut out = String::new();
    for (idx, ch) in compact.chars().enumerate() {
        if idx >= MAX_CHARS {
            out.push_str("...");
            break;
        }
        out.push(ch);
    }
    out
}

fn three_d_response_body(body: &str) -> anyhow::Result<SubmitResult> {
    let response: ThreeDDatacenterResponse =
        serde_json::from_str(body).map_err(|error| {
            let preview = summarize_response_body(body);
            anyhow::anyhow!(
                "数据服务响应不符合 ThreeDDatacenterResponse 契约（{preview}）：{error}"
            )
        })?;
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
        //login_url: Some("http://pms.powerpms.net:1801/sysin.html".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_ui::{
        RefU64,
        data_publish::{DesignPhase, PublishElement},
    };

    fn request(category: PublishCategory) -> PublishRequest {
        PublishRequest {
            title: "发布测试".into(),
            category,
            design_phase: DesignPhase::Detailed,
            elements: vec![PublishElement {
                refno: RefU64::from(12_345_u64),
                name: "/PIPE-100".into(),
            }],
            delete_refnos: vec!["24383/66458".into(), "24383/66457".into()],
        }
    }

    #[test]
    fn four_professional_publish_endpoints_include_delete_refnos() {
        for category in [
            PublishCategory::Process,
            PublishCategory::Electrical,
            PublishCategory::Instrumentation,
            PublishCategory::Ventilation,
        ] {
            assert_eq!(
                request_body(&request(category)).unwrap(),
                serde_json::json!({
                "refnos": ["0_12345"],
                "title": "发布测试",
                "create_rvm_relations": true,
                "b_first_time_design": false,
                "delete_refnos": ["24383/66458", "24383/66457"],
                })
            );
        }
    }

    #[test]
    fn room_publish_uses_the_datacenter_contract() {
        assert_eq!(
            request_body(&request(PublishCategory::Room)).unwrap(),
            serde_json::json!({
                "refnos": ["0_12345"],
                "title": "发布测试",
                "create_rvm_relations": true,
                "b_first_time_design": false,
            })
        );
    }

    #[test]
    fn empty_publish_requests_are_rejected() {
        let mut request = request(PublishCategory::Process);
        request.elements.clear();
        assert!(request_body(&request).is_err());
    }

    #[test]
    fn deletion_lookup_uses_only_slash_separated_refnos() {
        let mut request = request(PublishCategory::Process);
        request.elements.push(PublishElement {
            refno: RefU64::from(12_346_u64),
            name: "/PIPE-101".into(),
        });
        assert_eq!(
            deletion_lookup_body(&request).unwrap(),
            serde_json::json!({"refnos": ["0/12345", "0/12346"]})
        );
    }

    #[test]
    fn deletion_lookup_response_selects_every_candidate() {
        let candidates: Vec<DeletionCandidate> =
            serde_json::from_str(r#"[{"id":"24383/66458","name":"/1WCC1135"}]"#).unwrap();
        assert_eq!(candidates[0].id, "24383/66458");
        assert_eq!(candidates[0].name, "/1WCC1135");
        assert!(candidates[0].selected);
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
    fn successful_responses_keep_the_server_login_url() {
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
    fn room_responses_use_the_datacenter_contract() {
        let result = response_body(
            PublishCategory::Room,
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

    #[test]
    fn publish_http_request_attaches_bearer_token_when_present() {
        let without = http_request_with_token(
            "http://127.0.0.1:9098/get_gy_bran_data".into(),
            "{}".into(),
            None,
        );
        assert_eq!(without.headers.get("authorization"), None);

        let with = http_request_with_token(
            "http://127.0.0.1:9098/get_gy_bran_data".into(),
            "{}".into(),
            Some("  tok-abc  "),
        );
        assert_eq!(with.headers.get("authorization"), Some("Bearer tok-abc"));
        assert_eq!(
            with.headers.get("content-type"),
            Some("application/json; charset=utf-8")
        );
    }

    #[test]
    fn auth_user_parses_get_user_payload_and_formats_display_name() {
        let user_only: crate::startup::AuthUser = serde_json::from_str(
            r#"{"user_name":"KANGWP","chinese_name":"","user_major":"test","b_designer":true}"#,
        )
        .unwrap();
        assert_eq!(user_only.display_name(), "KANGWP");
        assert_eq!(user_only.user_major, "test");
        assert!(user_only.b_designer);

        let with_cn: crate::startup::AuthUser = serde_json::from_str(
            r#"{"user_name":"KANGWP","chinese_name":"康文平","user_major":"GY","b_designer":true}"#,
        )
        .unwrap();
        assert_eq!(with_cn.display_name(), "康文平 (KANGWP)");
    }

    #[test]
    fn preview_request_body_maps_category_and_refnos() {
        let req = request(PublishCategory::Instrumentation);
        let body = preview_request_body(&req, "inst", "bran");
        assert_eq!(
            body,
            serde_json::json!({
                "discipline": "inst",
                "target": "bran",
                "title": "发布测试",
                "refnos": ["0_12345"],
                "createRvmRelations": true,
                "bFirstTimeDesign": false,
                "deleteRefnos": [],
            })
        );
    }

    #[test]
    fn parse_preview_view_extracts_instances_relations_and_refno_map() {
        let payload = serde_json::json!({
            "id": "pv_123",
            "status": "done",
            "elapsedMs": 918,
            "result": {
                "datacenterLayer": "datacenter",
                "internal": {
                    "projectCode": "1516",
                    "owner": "KY1801-208",
                    "instances": [
                        {
                            "objectModelCode": "INSTUB",
                            "instanceCode": "1WCC0302A/B1",
                            "operate": "draft",
                            "version": "A版",
                            "attributes": [
                                { "attributeModelCode": "ROO_COD", "value": "R142" }
                            ]
                        }
                    ],
                    "relations": []
                },
                "datacenter": {
                    "projectCode": "1516",
                    "owner": "KY1801-208",
                    "instances": [
                        {
                            "objectModelCode": "PE_SEG_INSTUB",
                            "operate": "draft",
                            "version": "A版",
                            "attributes": [
                                { "attributeModelCode": "PBS_ID", "value": "1WCC0302A/B1" },
                                { "attributeModelCode": "ROO_COD", "value": "R142" }
                            ]
                        }
                    ],
                    "relations": [
                        {
                            "objectModelCode": "REL_PBS_OBJ",
                            "version": "A版",
                            "operate": "draft",
                            "attributes": [
                                { "attributeModelCode": "START_OBJ_CODE", "value": "PE_EQP_DD_LOC" },
                                { "attributeModelCode": "START_INST_CODE", "value": "1WCC262VN" },
                                { "attributeModelCode": "EDN_OBJ_CODE", "value": "PE_SEG_INSTUB" },
                                { "attributeModelCode": "END_INST_CODE", "value": "1WCC0302A/B1" }
                            ]
                        }
                    ]
                },
                "refnoMap": [
                    { "refno": "24383/68341", "instanceCode": "1WCC0302A/B1" }
                ],
                "skipped": [
                    { "refno": "24383/99999", "reason": "该参考号下没有可发布的节点" }
                ],
                "warnings": ["示例警告"]
            }
        })
        .to_string();

        let parsed = parse_preview_view(&payload).expect("应成功解析预览视图");
        assert_eq!(parsed.elapsed_ms, Some(918));
        assert_eq!(parsed.datacenter_layer, "datacenter");
        assert_eq!(parsed.datacenter.instances.len(), 1);
        assert_eq!(parsed.datacenter.instances[0].object_model_code, "PE_SEG_INSTUB");
        assert_eq!(parsed.datacenter.instances[0].instance_code, "1WCC0302A/B1");
        assert_eq!(parsed.datacenter.instances[0].refno.as_deref(), Some("24383/68341"));
        assert_eq!(parsed.datacenter.instances[0].attributes.len(), 2);
        assert_eq!(parsed.datacenter.relations.len(), 1);
        assert_eq!(parsed.datacenter.relations[0].from_summary, "PE_EQP_DD_LOC: 1WCC262VN");
        assert_eq!(parsed.datacenter.relations[0].to_summary, "PE_SEG_INSTUB: 1WCC0302A/B1");
        assert_eq!(parsed.skipped.len(), 1);
        assert_eq!(parsed.warnings, vec!["示例警告"]);
    }

    #[test]
    fn parses_data_service_meta_version() {
        let raw = r#"{"project":{"code":"1516","name":"AvevaMarineSample"},"owner":"KY1801-208","version":"0.2"}"#;
        let meta: super::DataServiceMeta = serde_json::from_str(raw).unwrap();
        assert_eq!(meta.version, "0.2");
    }

    #[test]
    fn three_d_response_body_surfaces_structured_error_message() {
        let raw = r#"{"Result":"查询数据失败：未配置普华材料数据库地址（puhua_database_ip）","Success":false,"StatusCode":500}"#;
        let err = three_d_response_body(raw).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("查询数据失败："));
        assert!(message.contains("puhua_database_ip"));
    }

    #[test]
    fn three_d_response_body_includes_non_json_snippet_on_contract_error() {
        let err = three_d_response_body("ForeignErr(\"puhua UCML/ReceiveZYModel api\")").unwrap_err();
        let message = err.to_string();
        assert!(message.contains("ThreeDDatacenterResponse 契约"));
        assert!(message.contains("ForeignErr"));

        let empty_err = three_d_response_body("   ").unwrap_err();
        assert!(empty_err.to_string().contains("空响应"));
    }

    #[test]
    fn preliminary_phase_sets_b_first_time_design_for_both_submit_and_preview() {
        let mut req = request(PublishCategory::Process);
        assert_eq!(request_body(&req).unwrap()["b_first_time_design"], false);
        assert_eq!(preview_request_body(&req, "pipe", "bran")["bFirstTimeDesign"], false);

        req.design_phase = DesignPhase::Preliminary;
        assert_eq!(request_body(&req).unwrap()["b_first_time_design"], true);
        assert_eq!(preview_request_body(&req, "pipe", "bran")["bFirstTimeDesign"], true);
    }
}
