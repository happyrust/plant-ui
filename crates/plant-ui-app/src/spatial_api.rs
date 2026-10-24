//! 房间树取数（plant3d-web「房间」页签移植，ADR 0068；计划 `docs/plans/2026-10-25-room-tree-port-plannotator.md`
//! 阶段一）：gen-model `GET /api/v1/spatial/rooms` 与 `GET /api/v1/spatial/rooms/{a_b}/tree?tubes=1`。
//!
//! 契约类型在 [`plant_ui::room_tree`]——绘制层要拿它展平成行，所以契约放那边；这里只做取数与错误分型。
//! 服务就是设置窗那一格「模型服务」（`model_update_api::base_url()`），身份三格随 query 带上（与
//! `dbnum_model_roots` 同法，服务端按 `resolve_identity` 校对）；`tubes=1` 恒带，老服务端不认就没有
//! `total_tube_count`，绘制层按响应里有没有那些键画。
//!
//! 不走读面：树是服务端聚合的（单元根派生、跨房去重、直段补全），库供数没有第二条路（计划 D2）；
//! 库供数下宿主根本不调这里，面板显一句注释。

use aios_core::RefU64;
use plant_ui::room_tree::{LeafSelector, RoomsResponse, TreeResponse};

use crate::model_update_api;

/// 一次取数失败的分型。界面按型给出路，不解析 message 字符串（S2-D）。
#[derive(Debug, Clone, PartialEq)]
pub enum SpatialError {
    /// 连不上、超时、504：服务够不着，直接重试即可。
    Unreachable(String),
    /// 老服务端没有这条路由（404 / 405）：整面板注释「当前服务端没有房间接口」，不重试。
    Unsupported(String),
    /// 这间房从没生成过面板模型（树路由 404）。
    RoomNotFound(String),
    /// 其余服务端错误，`code` 原样带出（400 refno 写法 / 不在册、422 房间体制未就绪、500…）。
    Service { code: String, message: String },
}

impl std::fmt::Display for SpatialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(message) => write!(f, "模型服务够不着（可直接重试）：{message}"),
            Self::Unsupported(_) => f.write_str("当前服务端没有房间接口（/api/v1/spatial/rooms）"),
            Self::RoomNotFound(_) => f.write_str("这间房还没生成过面板模型，答不出它的树"),
            Self::Service { code, message } => write!(f, "{code}：{message}"),
        }
    }
}

impl std::error::Error for SpatialError {}

/// 服务身份三格，随 query 带上。空串不发（服务端按 `Option` 收，没给就不校）。
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub project: String,
    pub mdb: String,
    pub namespace: String,
}

impl Identity {
    fn query(&self) -> Vec<(&'static str, String)> {
        [
            ("project", &self.project),
            ("mdb", &self.mdb),
            ("namespace", &self.namespace),
        ]
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, value)| (key, model_update_api::urlencode(value)))
        .collect()
    }
}

fn query_string(pairs: &[(&str, String)]) -> String {
    let body = pairs
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    if body.is_empty() {
        String::new()
    } else {
        format!("?{body}")
    }
}

/// `GET /api/v1/spatial/rooms` 的路径。
pub fn rooms_path(identity: &Identity) -> String {
    format!("/api/v1/spatial/rooms{}", query_string(&identity.query()))
}

/// `GET /api/v1/spatial/rooms/{a_b}/tree?tubes=1[&unit=a_b | &other_noun=NOUN]` 的路径。
/// 路径里的房间 refno 用 `a_b`（下划线形态，URL 安全），选择器里的单元 refno 同。
pub fn room_tree_path(identity: &Identity, room: RefU64, only: Option<&LeafSelector>) -> String {
    let mut pairs = identity.query();
    pairs.push(("tubes", "1".to_owned()));
    match only {
        Some(LeafSelector::Unit(unit)) => pairs.push(("unit", unit.to_string())),
        Some(LeafSelector::OtherNoun(noun)) => {
            pairs.push(("other_noun", model_update_api::urlencode(noun)))
        }
        None => {}
    }
    format!(
        "/api/v1/spatial/rooms/{}/tree{}",
        room,
        query_string(&pairs)
    )
}

/// 在册房间清单 + 房间体制状态。
pub async fn rooms(base: &str, identity: &Identity) -> Result<RoomsResponse, SpatialError> {
    model_update_api::get::<RoomsResponse>(base, &rooms_path(identity))
        .await
        .map_err(|error| classify(&error, true))
}

/// 一间房的层级树；`only` 给了就是补一组叶子（响应同形，`inlined` 说是哪组）。
pub async fn room_tree(
    base: &str,
    identity: &Identity,
    room: RefU64,
    only: Option<&LeafSelector>,
) -> Result<TreeResponse, SpatialError> {
    model_update_api::get::<TreeResponse>(base, &room_tree_path(identity, room, only))
        .await
        .map_err(|error| classify(&error, false))
}

/// 把 `model_update_api` 的包封分型。`listing` = 是清单那条路由：它的 404 是「老服务端没有这条路由」，
/// 而树路由的 404 是「这间房没生成过面板模型」（路由在、房间不在）。
fn classify(error: &anyhow::Error, listing: bool) -> SpatialError {
    let failure = model_update_api::failure_of(error);
    if failure.code == "timeout" {
        return SpatialError::Unreachable(failure.message);
    }
    if model_update_api::endpoint_missing(error) {
        return if listing {
            SpatialError::Unsupported(failure.message)
        } else {
            SpatialError::RoomNotFound(failure.message)
        };
    }
    SpatialError::Service {
        code: failure.code,
        message: failure.message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plant_ui::model_update::Failure;

    fn identity() -> Identity {
        Identity {
            project: "AMS".into(),
            mdb: "/ALL".into(),
            namespace: "ams".into(),
        }
    }

    fn failure(code: &str, message: &str) -> anyhow::Error {
        anyhow::Error::new(model_update_api::ApiError(Failure::new(code, message)))
    }

    #[test]
    fn the_paths_carry_identity_and_always_ask_for_tubes() {
        assert_eq!(
            rooms_path(&identity()),
            "/api/v1/spatial/rooms?project=AMS&mdb=%2FALL&namespace=ams"
        );
        let room = RefU64::from_two_nums(24384, 12);
        assert_eq!(
            room_tree_path(&identity(), room, None),
            "/api/v1/spatial/rooms/24384_12/tree?project=AMS&mdb=%2FALL&namespace=ams&tubes=1"
        );
        assert_eq!(
            room_tree_path(
                &Identity::default(),
                room,
                Some(&LeafSelector::Unit(RefU64::from_two_nums(24384, 99)))
            ),
            "/api/v1/spatial/rooms/24384_12/tree?tubes=1&unit=24384_99"
        );
        assert_eq!(
            room_tree_path(
                &Identity::default(),
                room,
                Some(&LeafSelector::OtherNoun("PANE".into()))
            ),
            "/api/v1/spatial/rooms/24384_12/tree?tubes=1&other_noun=PANE"
        );
    }

    #[test]
    fn the_two_kinds_of_404_stay_apart_and_timeouts_are_unreachable() {
        let missing = failure("not_found", "no such route");
        assert!(matches!(
            classify(&missing, true),
            SpatialError::Unsupported(_)
        ));
        assert!(matches!(
            classify(&missing, false),
            SpatialError::RoomNotFound(_)
        ));
        let timeout = failure("timeout", "connection refused");
        assert!(matches!(
            classify(&timeout, false),
            SpatialError::Unreachable(_)
        ));
        let precondition = failure("precondition", "rooms unavailable");
        assert_eq!(
            classify(&precondition, false),
            SpatialError::Service {
                code: "precondition".into(),
                message: "rooms unavailable".into()
            }
        );
        assert_eq!(
            SpatialError::RoomNotFound(String::new()).to_string(),
            "这间房还没生成过面板模型，答不出它的树"
        );
    }
}
