//! MBD 尺寸标注取数（`gen-model/.planning/2026-09-09-plant-ui-mbd-dimensions` Phase B1）。
//!
//! `GET {模型服务 base}/api/mbd/v2/pipe/{w0_w1}`。路径是 plant3d-web 先钉死的契约路径，
//! 故意不在 `/api/v1` 下（服务端 `web_service/mbd.rs` 的说法）；服务就是设置窗那一格
//! 「模型服务」（`model_update_api::base_url()`），同一进程同一端口，不新增连接配置。
//! 契约类型 path 依赖 `plant-mbd` 直接用（计划 D1：单一契约源；纯库，无 DB / HTTP /
//! e3d 依赖）；本模块只做取数与错误分型，不解释几何。
//!
//! 不复用 `model_update_api` 的错误解包：那条路把包封 `detail` 按字符串收，而本端点
//! 422（RootError）的 `detail.issues` 是结构化四级定位（BRAN / 直段 / 对象 / 规则），
//! 压成字符串界面就再也定位不回来。分型纪律与那边同一把尺子（S2-D）：`code` 优先、
//! 缺 `code` 才按状态码兜底、连不上与超时同型、不解析 message 字符串。
//!
//! 超时与取消跟既有取数同口径：超时 30 秒（服务端每次请求新开 `DbSet`、在阻塞线程上
//! 跑求解，大 BRAN 要几秒，与 `element_attributes` / `model/records` 同档）；取数在数据
//! 线程上跑（`data::Req::PipeDimensions`），取消由宿主按 `epoch` 丢弃过时结果
//! （`data::Evt::PipeDimensions`），这里不做第二套取消机制。

use std::time::Duration;

use plant_mbd::{GeometrySpace, MbdV2Issue, MbdV2PipeData};

const TIMEOUT: Duration = Duration::from_secs(30);

/// 一次取数失败的分型。界面按型给出路，**不解析 message 字符串**（S2-D）。
#[derive(Debug, Clone, PartialEq)]
pub enum MbdError {
    /// 连不上、超时、服务端 504：服务够不着，没有任何数据被改动，直接重试即可。
    Unreachable { message: String },
    /// 404（带包封）：refno 在服务所钉的会话里不存在——本地树与服务时点不同步的形态。
    NotFound { message: String },
    /// 422：这个元素解不出尺寸标注——不是派生隐式管身的路由容器（HVAC 支管、PIPE、
    /// ZONE…），或整条 BRAN 建不起成员结点视图。后者带 `issues`（四级定位）。
    NotDimensionable {
        message: String,
        issues: Vec<MbdV2Issue>,
    },
    /// 其余服务端错误（400 refno 写法、500 读库、路由缺席的 `endpoint_missing`…）。
    /// `code` 原样带出。
    Service { code: String, message: String },
    /// 2xx 但响应体解不动，或几何不在约定空间：两边契约版本漂移，对齐版本再来。
    Contract { message: String },
}

impl std::fmt::Display for MbdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable { message } => write!(f, "模型服务够不着（可直接重试）：{message}"),
            Self::NotFound { message } | Self::Contract { message } => f.write_str(message),
            Self::NotDimensionable { message, issues } => {
                f.write_str(message)?;
                if !issues.is_empty() {
                    write!(f, "（{} 条定位随详情）", issues.len())?;
                }
                Ok(())
            }
            Self::Service { code, message } => write!(f, "{code}：{message}"),
        }
    }
}

impl std::error::Error for MbdError {}

/// 取一条 BRAN 的尺寸标注。
pub async fn pipe_dimensions(
    base: &str,
    refno: aios_core::RefU64,
) -> Result<MbdV2PipeData, MbdError> {
    let mut req = ehttp::Request::get(endpoint_url(base, refno));
    req.timeout = Some(TIMEOUT);
    let response = ehttp::fetch_async(req)
        .await
        .map_err(|message| MbdError::Unreachable { message })?;
    classify(
        response.ok,
        response.status,
        response.text().unwrap_or_default(),
    )
}

/// refno 统一用下划线形（计划 R5）：免转义，日志与端点路径同形。
fn endpoint_url(base: &str, refno: aios_core::RefU64) -> String {
    format!("{}/api/mbd/v2/pipe/{refno}", base.trim_end_matches('/'))
}

fn classify(ok: bool, status: u16, body: &str) -> Result<MbdV2PipeData, MbdError> {
    if ok {
        let data: MbdV2PipeData =
            serde_json::from_str(body).map_err(|error| MbdError::Contract {
                message: format!("解析尺寸标注响应失败（两边契约版本对不上？）：{error}"),
            })?;
        // B3 把这批几何直接当世界毫米画；服务端哪天换口径必须死在这儿（fail-closed，
        // 与 plant3d-web 解析层同纪律），不能把设计米当毫米画出一幅错一千倍的图。
        if data.meta.geometry_space != GeometrySpace::SourceMm {
            return Err(MbdError::Contract {
                message: format!(
                    "几何空间是 {:?}，不是约定的 source_mm——请对齐 gen-model 版本",
                    data.meta.geometry_space
                ),
            });
        }
        return Ok(data);
    }
    let envelope: ErrorEnvelope = serde_json::from_str(body).unwrap_or_default();
    let coded = envelope.code.is_some();
    let code = envelope
        .code
        .unwrap_or_else(|| fallback_code(status).to_owned());
    let message = envelope
        .message
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| format!("HTTP {status}: {body}"));
    Err(match code.as_str() {
        // 裸 404（没有错误包封）不是「refno 不存在」，是路由缺席：现网还常见没有
        // /api/mbd/v2 的旧构建（计划 R1）。这句话要说人话，不然像时点不同步。
        "not_found" if !coded => MbdError::Service {
            code: "endpoint_missing".to_owned(),
            message:
                "模型服务没有尺寸标注端点（/api/mbd/v2）——多半是旧构建，请换新构建重启模型服务"
                    .to_owned(),
        },
        "not_found" => MbdError::NotFound { message },
        "precondition" => MbdError::NotDimensionable {
            message,
            issues: issues_of(&envelope.detail),
        },
        "timeout" => MbdError::Unreachable { message },
        _ => MbdError::Service { code, message },
    })
}

/// 缺 `code` 时按状态码兜底（`model_update_api::error_packet` 同一张表 + 404）。
fn fallback_code(status: u16) -> &'static str {
    match status {
        404 => "not_found",
        409 => "conflict",
        422 => "precondition",
        504 => "timeout",
        _ => "internal",
    }
}

/// RootError 的 `detail.issues`（四级定位）。解不动不整锅端：message 仍是权威话术，
/// 定位丢了只是详情面板少一层。
fn issues_of(detail: &serde_json::Value) -> Vec<MbdV2Issue> {
    detail
        .get("issues")
        .and_then(|issues| serde_json::from_value(issues.clone()).ok())
        .unwrap_or_default()
}

/// 服务端统一错误包封 `{ code, message, detail }`。`detail` 平时是 null / 字符串；
/// 本端点 422（RootError）给的是 `{ branch_refno, issues }`，所以按 Value 收。
#[derive(Debug, Default, serde::Deserialize)]
struct ErrorEnvelope {
    code: Option<String>,
    message: Option<String>,
    #[serde(default)]
    detail: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use plant_mbd::{IssueSeverity, MbdPrimitive};

    use super::*;

    fn success_body(space: &str) -> String {
        format!(
            r#"{{
                "version": "v2",
                "input_refno": "24383_99996",
                "branch_refno": "24383/99996",
                "primitives": [{{
                    "kind": "linear_dim",
                    "id": "dim-0",
                    "start": [0.0, 0.0, 0.0],
                    "end": [1000.0, 0.0, 0.0],
                    "text": "1000",
                    "extension_lines": [{{"from": [0, 0, 0], "to": [0, 0, 120]}}],
                    "arrow_lines": [],
                    "label_anchor": [500.0, 0.0, 140.0]
                }}],
                "meta": {{
                    "geometry_space": "{space}",
                    "cheight_mm": 30.0,
                    "layout_mode": "isodim_main",
                    "notes": []
                }},
                "issues": [{{
                    "id": "issue-0",
                    "severity": "warning",
                    "category": "suppress",
                    "message": "断口对面的尾点已删",
                    "rule_id": "linear.point_off_axis"
                }}]
            }}"#
        )
    }

    /// 200 解进冻结契约：图元、meta、issues 的定位字段一个不少地穿过 HTTP 边界。
    #[test]
    fn a_success_body_decodes_into_the_frozen_contract() {
        let data = classify(true, 200, &success_body("source_mm")).unwrap();
        assert_eq!(data.branch_refno, "24383/99996");
        assert_eq!(data.primitives.len(), 1);
        match &data.primitives[0] {
            MbdPrimitive::LinearDim {
                text,
                extension_lines,
                ..
            } => {
                assert_eq!(text, "1000");
                assert_eq!(extension_lines.len(), 1);
            }
            other => panic!("expected linear_dim, got {other:?}"),
        }
        assert_eq!(data.meta.layout_mode.as_deref(), Some("isodim_main"));
        assert_eq!(data.issues[0].severity, IssueSeverity::Warning);
        assert_eq!(
            data.issues[0].rule_id.as_deref(),
            Some("linear.point_off_axis")
        );
    }

    /// B3 拿这批几何直接当世界毫米画：空间不是 source_mm 必须拦在解析层，
    /// 不能把设计米当毫米画出一幅错一千倍的图。
    #[test]
    fn geometry_outside_source_mm_is_refused_at_the_boundary() {
        let error = classify(true, 200, &success_body("design_m")).unwrap_err();
        assert!(matches!(error, MbdError::Contract { .. }), "{error:?}");
    }

    /// 422 RootError 的四级定位要活着穿过来，界面才说得清「为什么整条没出图」。
    #[test]
    fn a_root_error_422_carries_its_issues_out() {
        let body = r#"{
            "code": "precondition",
            "message": "BRAN 24383/1 建不起可信的成员结点视图：owner 链不可读",
            "detail": {
                "branch_refno": "24383_1",
                "issues": [{
                    "id": "root-0",
                    "severity": "error",
                    "category": "data",
                    "message": "owner 链不可读",
                    "refno": "24383/1",
                    "rule_id": "root.member_view"
                }]
            }
        }"#;
        match classify(false, 422, body).unwrap_err() {
            MbdError::NotDimensionable { message, issues } => {
                assert!(message.contains("成员结点视图"), "{message}");
                assert_eq!(issues.len(), 1);
                assert_eq!(issues[0].rule_id.as_deref(), Some("root.member_view"));
                assert_eq!(issues[0].severity, IssueSeverity::Error);
            }
            other => panic!("expected NotDimensionable, got {other:?}"),
        }
    }

    /// NotRouteContainer / Solver 的 422 没有 detail.issues，也归同一型（空定位）。
    #[test]
    fn a_non_route_container_422_reads_as_not_dimensionable_too() {
        let body =
            r#"{"code":"precondition","message":"ZONE =24383/2 不是路由容器","detail":null}"#;
        match classify(false, 422, body).unwrap_err() {
            MbdError::NotDimensionable { issues, .. } => assert!(issues.is_empty()),
            other => panic!("expected NotDimensionable, got {other:?}"),
        }
    }

    /// 两种 404 是两件事：带包封的是「refno 不存在」（服务真查过）；裸 404 是路由缺席
    /// ——现网旧构建没有 /api/mbd/v2（计划 R1），错话会把人支去查时点同步。
    #[test]
    fn the_two_kinds_of_404_stay_apart() {
        let real = classify(
            false,
            404,
            r#"{"code":"not_found","message":"refno 24383/9 在所钉会话里不存在","detail":null}"#,
        )
        .unwrap_err();
        assert!(matches!(real, MbdError::NotFound { .. }), "{real:?}");

        match classify(false, 404, "").unwrap_err() {
            MbdError::Service { code, message } => {
                assert_eq!(code, "endpoint_missing");
                assert!(message.contains("旧构建"), "{message}");
            }
            other => panic!("expected endpoint_missing, got {other:?}"),
        }
    }

    /// 连不上、超时、服务端 504 对用的人是同一件事：够不着、什么都没改、直接重试。
    #[test]
    fn a_504_reads_as_unreachable_like_a_transport_failure() {
        assert!(matches!(
            classify(false, 504, "{}").unwrap_err(),
            MbdError::Unreachable { .. }
        ));
    }

    /// 带 code 的失败原样出去，界面按型给出路，不解析 message。
    #[test]
    fn coded_failures_keep_their_code() {
        let body = r#"{"code":"internal","message":"读库出错","detail":null}"#;
        match classify(false, 500, body).unwrap_err() {
            MbdError::Service { code, .. } => assert_eq!(code, "internal"),
            other => panic!("expected Service, got {other:?}"),
        }
    }

    /// 2xx 但解不动 = 契约漂移，不是可重试失败；原始解析错误要留在 message 里。
    #[test]
    fn garbage_success_bodies_are_contract_errors() {
        assert!(matches!(
            classify(true, 200, "not json").unwrap_err(),
            MbdError::Contract { .. }
        ));
    }

    /// 路径用下划线形 refno（R5）：免转义，日志与端点同形；base 尾斜杠不产生双斜杠。
    #[test]
    fn the_request_path_uses_the_underscore_refno_form() {
        let refno: aios_core::RefU64 = "24383/99996".parse().unwrap();
        assert_eq!(
            endpoint_url("http://127.0.0.1:8022/", refno),
            "http://127.0.0.1:8022/api/mbd/v2/pipe/24383_99996"
        );
    }
}
