//! 房间树（plant3d-web 模型树「房间」页签的移植，ADR 0068；计划
//! `docs/plans/2026-10-25-room-tree-port-plannotator.md`）：两条只读路由的契约、节点模型、
//! 标签与收集器，外加一台不做 IO 的状态机。
//!
//! 层次是服务端聚合好的：在册房间平铺一层，展开一间房是 房间 → 专业 → 最小交付单元类型 →
//! 单元 → 构件（BRAN 单元的构件行之后再列它的直段），不属任何单元的构件归专业下的
//! 「其他构件」按 noun 分。这里**不**重新设计：节点身份、标签、计数尾巴、距离格式与来源
//! `src/composables/roomTreeNodes.ts` / `useRoomTree.ts` 逐项对应，两端看到的是同一棵树。
//!
//! 取数归宿主（`plant-ui-app/src/spatial_api.rs`，只走 gen-model 服务供数，决策 D2）；显隐 /
//! 聚焦 / 隔离落到既有的 [`crate::ModelAction`]；眼睛读 View3d 的实际可见性（决策 D4，与模型树
//! 同一条房规），所以本模块不另记勾选表——行的 eye 由宿主给的闭包按 refno 算，父节点按子节点聚合。
//! 直段行的眼睛是逐段的（阶段四）：按 (所属 BRAN, [`TubeSegment`]) 读 View3d 的逐段实际可见性，点它落到
//! [`crate::ModelAction::SetTubeSegmentsVisible`]；没带身份进场景的直段（老服务端 / 库供数）停在「未加载」。

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use aios_core::RefU64;
use serde::Deserialize;

use crate::TubeSegment;
use crate::style::tokens::Status;
use crate::vm::{RoomTreeErrorVm, RoomTreeRowVm, RoomTreeTagVm, RowVisibility};

// ---------------------------------------------------------------- 契约
//
// 照 plant3d-web `src/api/genModelSpatialApi.ts` / `genModelV1Api.ts`「房间层级树」一节，
// 一字不增减；服务端多给的键（`center` / `radius` / `room_status`…）serde 直接略过。

/// `GET /api/v1/spatial/rooms` 的 `status` 字面值里，清单非空的两档。
pub const STATUS_READY: &str = "ready";
pub const STATUS_DEGRADED: &str = "degraded";

/// 在册房间清单里的一间。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoomListItem {
    /// `a_b`
    pub refno: String,
    pub room_num: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub dbnum: Option<u32>,
    #[serde(default)]
    pub panel_count: usize,
    /// 在册边界面板的 `a_b` 列表；旧服务端可能未下发。
    #[serde(default)]
    pub panel_refnos: Vec<String>,
}

/// `GET /api/v1/spatial/rooms`：在册房间清单 + 房间体制此刻的状态。
/// `status` 不是 `ready` / `degraded` 时 `rooms` 为空、`reason` 说为什么。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RoomsResponse {
    pub status: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub definition_version: Option<String>,
    #[serde(default)]
    pub rooms: Vec<RoomListItem>,
}

/// 树里的一个构件（叶子）；`shared_rooms` 只在它属于 ≥ 2 间在册房间时出现。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeLeaf {
    /// `a_b`
    pub refno: String,
    pub noun: String,
    /// 到房间盒的最小表面距离（mm）
    pub distance: f64,
    #[serde(default)]
    pub shared_rooms: Option<u32>,
}

/// 世界盒（mm），与 `nearby` 结果的 `aabb` 同形。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Aabb {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

/// BRAN 单元下的一段直管（隐式管身；`tubes=1`）。身份是 `(单元 refno, from, to, ordinal)`：
/// 直管没有自己的 refno、不计入任何 `count`。`from` / `to` 是两端管件的 `a_b`——容器头 / 尾
/// 那一段的一端是 BRAN 自己（`*_noun` 为 `BRAN`）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeTube {
    pub ordinal: u32,
    pub from: String,
    pub to: String,
    pub from_noun: String,
    pub to_noun: String,
    pub distance: f64,
    /// 直管长度（mm）
    pub length: f64,
    pub aabb: Aabb,
    #[serde(default)]
    pub invalid: bool,
    #[serde(default)]
    pub shared_rooms: Option<u32>,
}

/// 一个最小交付单元（BRAN / EQUI…）；`elements` 缺 = 叶子超上限未内联（按 `unit=` 另取）。
/// `tube_count` / `tubes` 只在 `tubes=1` 且服务端认这个参数时出现；`tubes` 与 `elements`
/// 一起来、一起缺。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeUnit {
    pub refno: String,
    pub noun: String,
    #[serde(default)]
    pub name: Option<String>,
    pub count: usize,
    #[serde(default)]
    pub tube_count: Option<usize>,
    pub min_distance: f64,
    #[serde(default)]
    pub elements: Option<Vec<TreeLeaf>>,
    #[serde(default)]
    pub tubes: Option<Vec<TreeTube>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeUnitType {
    pub noun: String,
    pub count: usize,
    #[serde(default)]
    pub tube_count: Option<usize>,
    pub units: Vec<TreeUnit>,
}

/// 「其他构件」里按 noun 的一组；`elements` 缺同上（按 `other_noun=` 另取）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeOtherNoun {
    pub noun: String,
    pub count: usize,
    pub min_distance: f64,
    #[serde(default)]
    pub elements: Option<Vec<TreeLeaf>>,
}

#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
pub struct TreeOthers {
    #[serde(default)]
    pub count: usize,
    #[serde(default)]
    pub by_noun: Vec<TreeOtherNoun>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeSpec {
    pub spec_value: i64,
    pub count: usize,
    #[serde(default)]
    pub tube_count: Option<usize>,
    /// 按 `delivery_unit_types` 配置表顺序
    #[serde(default)]
    pub unit_types: Vec<TreeUnitType>,
    /// 不属任何最小交付单元的构件，按 noun 分
    #[serde(default)]
    pub others: TreeOthers,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeRoom {
    pub refno: String,
    pub room_num: String,
    #[serde(default)]
    pub name: Option<String>,
    /// 该房间下按 refno 去重的构件数
    pub count: usize,
    /// 该房间下的直段放置数（`tubes=1` 才有；不计入 `count`）
    #[serde(default)]
    pub tube_count: Option<usize>,
    /// `spec_value` 升序，0「其他」在前
    #[serde(default)]
    pub specs: Vec<TreeSpec>,
}

fn default_true() -> bool {
    true
}

/// `GET /api/v1/spatial/rooms/{a_b}/tree?tubes=1`（与 `nearby/tree` 同形，`rooms[]` 恒一条）。
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TreeResponse {
    /// 全树按 refno 去重的构件数
    #[serde(default)]
    pub total_count: usize,
    /// 老服务端不认 `tubes=1` 时缺 = 树里没有直段
    #[serde(default)]
    pub total_tube_count: Option<usize>,
    /// BRAN 单元数超过读根预算，只为最近的那些补了直段
    #[serde(default)]
    pub tubes_truncated: bool,
    /// false = 叶子省略了（超上限，或只按 `unit=` / `other_noun=` 内联了一组）
    #[serde(default = "default_true")]
    pub leaves_inline: bool,
    #[serde(default)]
    pub leaf_cap: usize,
    /// 叶子放置数（跨房构件每间房各算一次）
    #[serde(default)]
    pub leaf_count: usize,
    /// 只内联了一组时是哪一组：`unit:a_b` / `other_noun:PANE`
    #[serde(default)]
    pub inlined: Option<String>,
    #[serde(default)]
    pub delivery_unit_types: Vec<String>,
    #[serde(default)]
    pub rooms: Vec<TreeRoom>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// 叶子超上限后只取一组：单元 refno 或「其他构件」里的一个 noun，二选一。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LeafSelector {
    Unit(RefU64),
    OtherNoun(String),
}

// ---------------------------------------------------------------- 节点

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoomNodeKind {
    Room,
    Spec,
    UnitType,
    Unit,
    Others,
    OtherNoun,
    Element,
    Tube,
}

/// 节点身份。照来源的字符串 id（`room:<a_b>` / `spec:<房间>:<spec>` / `utype:…` / `others:…` /
/// `unit:<房间>:<a_b>` / `onoun:…` / `elem:<房间>:<a_b>` / `tube:<房间>:<单元>#<from>-<to>#<ordinal>`）
/// 改成枚举：同一构件在两间房下各有一个节点（id 带房间），与来源一致。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RoomNodeId {
    Room(RefU64),
    Spec {
        room: RefU64,
        spec: i64,
    },
    UnitType {
        room: RefU64,
        spec: i64,
        noun: String,
    },
    Others {
        room: RefU64,
        spec: i64,
    },
    Unit {
        room: RefU64,
        unit: RefU64,
    },
    OtherNoun {
        room: RefU64,
        spec: i64,
        noun: String,
    },
    Element {
        room: RefU64,
        refno: RefU64,
    },
    /// 直段行：`from` / `to` 保留服务端的 `a_b` 原文——身份就是那串字，不替它改写。
    Tube {
        room: RefU64,
        unit: RefU64,
        from: String,
        to: String,
        ordinal: u32,
    },
}

impl RoomNodeId {
    /// 所属房间。
    pub fn room(&self) -> RefU64 {
        match self {
            Self::Room(room)
            | Self::Spec { room, .. }
            | Self::UnitType { room, .. }
            | Self::Others { room, .. }
            | Self::Unit { room, .. }
            | Self::OtherNoun { room, .. }
            | Self::Element { room, .. }
            | Self::Tube { room, .. } => *room,
        }
    }

    pub fn kind(&self) -> RoomNodeKind {
        match self {
            Self::Room(_) => RoomNodeKind::Room,
            Self::Spec { .. } => RoomNodeKind::Spec,
            Self::UnitType { .. } => RoomNodeKind::UnitType,
            Self::Others { .. } => RoomNodeKind::Others,
            Self::Unit { .. } => RoomNodeKind::Unit,
            Self::OtherNoun { .. } => RoomNodeKind::OtherNoun,
            Self::Element { .. } => RoomNodeKind::Element,
            Self::Tube { .. } => RoomNodeKind::Tube,
        }
    }

    /// 直段行的场景键：所属 BRAN + 直段身份（`ModelAction::SetTubeSegmentsVisible` 用）；其他行回 `None`。
    pub fn tube_segment(&self) -> Option<(RefU64, TubeSegment)> {
        match self {
            Self::Tube {
                unit,
                from,
                to,
                ordinal,
                ..
            } => Some((
                *unit,
                TubeSegment {
                    ordinal: *ordinal,
                    from: from.clone(),
                    to: to.clone(),
                },
            )),
            _ => None,
        }
    }
}

/// 直段行带的那段直管：所属 BRAN（点行 = 选中它）与服务端给的世界盒（聚焦用）。
#[derive(Debug, Clone, PartialEq)]
pub struct TubeInfo {
    pub unit: RefU64,
    pub aabb: Aabb,
}

/// 树里的一个节点（展平前）。标签拆成 `label` + `meta` + `tags` 三格喂给 `tree_row_ui`：
/// 来源把计数拼在名字尾巴上，这里按 D3 把计数尾巴放 `meta`、「跨 N 房」「无效」走 `tags`。
#[derive(Debug, Clone, PartialEq)]
pub struct RoomTreeNode {
    pub id: RoomNodeId,
    /// 行上的主文字
    pub label: String,
    /// 行尾：计数尾巴 / noun · 距离
    pub meta: String,
    /// 悬停全文
    pub hover: String,
    /// 图标用的类型：房间 `ROOM`、单元 / 构件 / noun 组用 noun、直段 `TUBI`、专业与「其他构件」为空
    pub noun: String,
    /// 场景对象键：只有单元与构件有
    pub refno: Option<RefU64>,
    /// 该节点下按 refno 去重的构件数（房间根在树取回前为 `None`）
    pub count: Option<usize>,
    pub tube_count: Option<usize>,
    /// 该节点的叶子是否都已内联；未内联的单元 / noun 组第一次展开或动作前先按 `leaf_selector` 补
    pub leaves_inline: bool,
    pub leaf_selector: Option<LeafSelector>,
    /// 构件 / 单元到房间盒的距离（mm）；单元是最近成员距离
    pub distance: Option<f64>,
    /// 构件属于 ≥ 2 间在册房间时服务端给的房间数
    pub shared_rooms: Option<u32>,
    pub tags: Vec<RoomTreeTagVm>,
    pub tube: Option<TubeInfo>,
    pub children: Vec<RoomTreeNode>,
}

impl RoomTreeNode {
    pub fn kind(&self) -> RoomNodeKind {
        self.id.kind()
    }

    fn bare(id: RoomNodeId, label: String, noun: &str) -> Self {
        Self {
            id,
            label,
            meta: String::new(),
            hover: String::new(),
            noun: noun.to_owned(),
            refno: None,
            count: None,
            tube_count: None,
            leaves_inline: true,
            leaf_selector: None,
            distance: None,
            shared_rooms: None,
            tags: Vec::new(),
            tube: None,
            children: Vec::new(),
        }
    }

    /// 有没有可展开的一层（照来源 `hasChildren`）：构件 / 直段没有；房间根在树取回前当作有；
    /// 叶子未内联、计数又大于零的单元 / noun 组也当作有（展开时再补）。
    pub fn has_children(&self) -> bool {
        match self.kind() {
            RoomNodeKind::Element | RoomNodeKind::Tube => false,
            RoomNodeKind::Room => {
                self.count.is_none_or(|count| count > 0) || !self.children.is_empty()
            }
            _ => !self.children.is_empty() || (!self.leaves_inline && self.count.unwrap_or(0) > 0),
        }
    }
}

// ---------------------------------------------------------------- 标签（照搬来源文案）

/// 专业值 → 名字（`plant3d-web/src/types/spec.ts::getSpecValueName`）。
pub fn spec_name(value: i64) -> String {
    match value {
        0 => "未知或其他".to_owned(),
        1 => "管道系统".to_owned(),
        2 => "电气系统".to_owned(),
        3 => "仪表系统".to_owned(),
        4 => "暖通空调系统".to_owned(),
        5 => "土建系统".to_owned(),
        6 => "结构系统".to_owned(),
        other => format!("未知专业({other})"),
    }
}

fn trim_zeros(text: String) -> String {
    if !text.contains('.') {
        return text;
    }
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_owned()
}

/// 到房间盒的距离：整米写 `3 m`，否则三位小数去尾零（`1.25 m`）。
pub fn format_distance(mm: f64) -> String {
    let meters = mm / 1000.0;
    if (meters - meters.round()).abs() < 0.0001 {
        return format!("{} m", meters.round() as i64);
    }
    format!("{} m", trim_zeros(format!("{meters:.3}")))
}

/// 直管长度：< 1 m 用整 mm，≥ 1 m 两位小数的 m（去尾零）。
pub fn format_tube_length(mm: f64) -> String {
    if !mm.is_finite() {
        return String::new();
    }
    if mm < 1000.0 {
        return format!("{} mm", mm.round() as i64);
    }
    format!("{} m", trim_zeros(format!("{:.2}", mm / 1000.0)))
}

/// 计数尾巴：`N 个构件（· M 条直段）`。来源写「M 段直管」；这边界面上的词是「直段」
/// （`CONTEXT.md`，TUBI / 直管段 / 管身都不上界面），计数和层次不变，只换这一个词。
pub fn count_phrase(count: usize, tube_count: Option<usize>) -> String {
    match tube_count {
        Some(tubes) => format!("{count} 个构件 · {tubes} 条直段"),
        None => format!("{count} 个构件"),
    }
}

/// 直段行的主标识：两端 noun「REDU → BEND」；读不出的一端画 `?`。
pub fn tube_label(from_noun: &str, to_noun: &str) -> String {
    let from = if from_noun.is_empty() { "?" } else { from_noun };
    let to = if to_noun.is_empty() { "?" } else { to_noun };
    format!("{from} → {to}")
}

fn join(parts: &[Option<String>]) -> String {
    parts
        .iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn name_or_none(name: Option<&String>) -> Option<String> {
    name.filter(|name| !name.trim().is_empty()).cloned()
}

fn parse_refno(raw: &str) -> Option<RefU64> {
    raw.trim().parse::<RefU64>().ok().filter(RefU64::is_valid)
}

fn shared_tag(shared: u32) -> RoomTreeTagVm {
    RoomTreeTagVm {
        text: format!("跨 {shared} 房"),
        tone: Status::Info,
    }
}

/// 在册房间清单里的一间 → 页签根节点（树未取回：`count` None、没有子层、`leaves_inline` false
/// 表示还要取）。`panel_count == 0` 的在册房在源模型里就没有边界面板（空房壳）：成员恒空、
/// `rooms/{refno}/tree` 恒答不出，直接标成叶子（`count: 0`），不给展开钮、不发请求。
pub fn room_root_node(room: &RoomListItem) -> Option<RoomTreeNode> {
    let refno = parse_refno(&room.refno)?;
    let empty = room.panel_count == 0;
    let name = name_or_none(room.name.as_ref());
    let mut node = RoomTreeNode::bare(
        RoomNodeId::Room(refno),
        join(&[Some(room.room_num.clone()), name.clone()]),
        "ROOM",
    );
    node.refno = Some(refno);
    node.count = empty.then_some(0);
    node.leaves_inline = empty;
    node.meta = if empty {
        "空房".to_owned()
    } else {
        String::new()
    };
    node.hover = join(&[
        Some(room.room_num.clone()),
        name,
        Some(room.refno.clone()),
        room.dbnum.map(|dbnum| format!("库 {dbnum}")),
        Some(if empty {
            "空房间（无边界面板）".to_owned()
        } else {
            format!("{} 块面板", room.panel_count)
        }),
    ]);
    Some(node)
}

fn leaf_node(room: RefU64, leaf: &TreeLeaf) -> Option<RoomTreeNode> {
    let refno = parse_refno(&leaf.refno)?;
    let mut node = RoomTreeNode::bare(
        RoomNodeId::Element { room, refno },
        leaf.refno.clone(),
        &leaf.noun,
    );
    node.refno = Some(refno);
    node.count = Some(1);
    node.distance = Some(leaf.distance);
    node.shared_rooms = leaf.shared_rooms;
    node.meta = format!("{} · {}", leaf.noun, format_distance(leaf.distance));
    node.tags = leaf.shared_rooms.map(shared_tag).into_iter().collect();
    node.hover = join(&[
        Some(leaf.refno.clone()),
        Some(leaf.noun.clone()),
        Some(format_distance(leaf.distance)),
        leaf.shared_rooms.map(|shared| format!("跨 {shared} 房")),
    ]);
    Some(node)
}

fn unit_node(room: RefU64, unit: &TreeUnit) -> Option<RoomTreeNode> {
    let refno = parse_refno(&unit.refno)?;
    let inline = unit.elements.is_some();
    let name = name_or_none(unit.name.as_ref());
    let mut node = RoomTreeNode::bare(
        RoomNodeId::Unit { room, unit: refno },
        name.clone().unwrap_or_else(|| unit.refno.clone()),
        &unit.noun,
    );
    node.refno = Some(refno);
    node.count = Some(unit.count);
    node.tube_count = unit.tube_count;
    node.leaves_inline = inline;
    node.leaf_selector = (!inline).then_some(LeafSelector::Unit(refno));
    node.distance = Some(unit.min_distance);
    node.meta = match unit.tube_count.filter(|tubes| *tubes > 0) {
        Some(tubes) => format!("{} · {} 条直段", unit.count, tubes),
        None => unit.count.to_string(),
    };
    node.hover = join(&[
        name,
        Some(format!("{} {}", unit.noun, unit.refno)),
        Some(format!("最近 {}", format_distance(unit.min_distance))),
        Some(count_phrase(unit.count, unit.tube_count)),
    ]);
    for leaf in unit.elements.iter().flatten() {
        node.children.extend(leaf_node(room, leaf));
    }
    // 直段行：构件行之后；`tubes` 与 `elements` 一起来、一起缺。
    for tube in unit.tubes.iter().flatten() {
        node.children
            .push(tube_node(room, refno, &unit.refno, tube));
    }
    Some(node)
}

/// 直段行：主文字「REDU → BEND」，尾巴「直段 · 141 mm」，「无效」「跨 N 房」走标签片。
/// 没有 refno（不是场景对象、不进任何 refno 集），`count: 0`（不是构件）。`noun` 留 `TUBI` 只给图标查表用，
/// 不上界面。
fn tube_node(room: RefU64, unit: RefU64, unit_raw: &str, tube: &TreeTube) -> RoomTreeNode {
    let label = tube_label(&tube.from_noun, &tube.to_noun);
    let mut node = RoomTreeNode::bare(
        RoomNodeId::Tube {
            room,
            unit,
            from: tube.from.clone(),
            to: tube.to.clone(),
            ordinal: tube.ordinal,
        },
        label.clone(),
        "TUBI",
    );
    node.count = Some(0);
    node.distance = Some(tube.distance);
    node.shared_rooms = tube.shared_rooms;
    node.meta = format!("直段 · {}", format_tube_length(tube.length));
    if tube.invalid {
        node.tags.push(RoomTreeTagVm {
            text: "无效".to_owned(),
            tone: Status::Error,
        });
    }
    if let Some(shared) = tube.shared_rooms.filter(|shared| *shared > 1) {
        node.tags.push(shared_tag(shared));
    }
    node.hover = join(&[
        Some(format!("直段 {label}")),
        Some(format_tube_length(tube.length)),
        Some(format!("距 {}", format_distance(tube.distance))),
        Some(format!("{} → {}", tube.from, tube.to)),
        (tube.ordinal > 0).then(|| format!("第 {} 段", tube.ordinal + 1)),
        tube.invalid.then(|| "无效直段".to_owned()),
        tube.shared_rooms
            .filter(|shared| *shared > 1)
            .map(|shared| format!("跨 {shared} 房")),
        Some(format!("属 BRAN {unit_raw}")),
    ]);
    node.tube = Some(TubeInfo {
        unit,
        aabb: tube.aabb,
    });
    node
}

fn unit_type_inline(group: &TreeUnitType) -> bool {
    group.units.iter().all(|unit| unit.elements.is_some())
}

fn others_inline(others: &TreeOthers) -> bool {
    others.by_noun.iter().all(|group| group.elements.is_some())
}

fn spec_inline(spec: &TreeSpec) -> bool {
    spec.unit_types.iter().all(unit_type_inline) && others_inline(&spec.others)
}

/// 把一间房的 `rooms/{refno}/tree` 响应展成节点树：根是房间（尾巴带构件数），下面 专业 → 单元类型
/// → 单元 → 构件 与 专业 → 其他构件 → noun 组 → 构件。响应里 `rooms[]` 不止一条时只取 refno 相同的
/// 那条（契约恒一条）。回 `None` 表示响应里没有这间房。
pub fn flatten_room_tree(
    room: RefU64,
    tree: &TreeResponse,
    root: Option<&RoomListItem>,
) -> Option<RoomTreeNode> {
    let entry = tree
        .rooms
        .iter()
        .find(|entry| parse_refno(&entry.refno) == Some(room))
        .or_else(|| tree.rooms.first())?;
    let room_num = if entry.room_num.is_empty() {
        root.map(|root| root.room_num.clone())
            .unwrap_or_else(|| room.to_string())
    } else {
        entry.room_num.clone()
    };
    let name = name_or_none(entry.name.as_ref())
        .or_else(|| root.and_then(|root| name_or_none(root.name.as_ref())));
    let mut node = RoomTreeNode::bare(
        RoomNodeId::Room(room),
        join(&[Some(room_num.clone()), name.clone()]),
        "ROOM",
    );
    node.refno = Some(room);
    node.count = Some(entry.count);
    node.tube_count = entry.tube_count;
    node.leaves_inline = entry.specs.iter().all(spec_inline);
    node.meta = count_phrase(entry.count, entry.tube_count);
    node.hover = join(&[
        Some(room_num),
        name,
        Some(room.to_string()),
        Some(count_phrase(entry.count, entry.tube_count)),
        (!tree.leaves_inline).then(|| {
            format!(
                "构件 {} 个放置 > 上限 {}，按单元展开时再取",
                tree.leaf_count, tree.leaf_cap
            )
        }),
    ]);

    for spec in &entry.specs {
        let mut spec_node = RoomTreeNode::bare(
            RoomNodeId::Spec {
                room,
                spec: spec.spec_value,
            },
            spec_name(spec.spec_value),
            "",
        );
        spec_node.count = Some(spec.count);
        spec_node.tube_count = spec.tube_count;
        spec_node.leaves_inline = spec_inline(spec);
        spec_node.meta = spec.count.to_string();
        spec_node.hover = format!(
            "{} · 专业 {} · {}",
            spec_name(spec.spec_value),
            spec.spec_value,
            count_phrase(spec.count, spec.tube_count)
        );

        for group in &spec.unit_types {
            let mut group_node = RoomTreeNode::bare(
                RoomNodeId::UnitType {
                    room,
                    spec: spec.spec_value,
                    noun: group.noun.clone(),
                },
                group.noun.clone(),
                &group.noun,
            );
            group_node.count = Some(group.count);
            group_node.tube_count = group.tube_count;
            group_node.leaves_inline = unit_type_inline(group);
            group_node.meta = format!("{} 个单元 · {}", group.units.len(), group.count);
            group_node.hover = format!(
                "{} · {} 个最小交付单元 · {}",
                group.noun,
                group.units.len(),
                count_phrase(group.count, group.tube_count)
            );
            for unit in &group.units {
                group_node.children.extend(unit_node(room, unit));
            }
            spec_node.children.push(group_node);
        }

        if spec.others.count > 0 || !spec.others.by_noun.is_empty() {
            let mut others_node = RoomTreeNode::bare(
                RoomNodeId::Others {
                    room,
                    spec: spec.spec_value,
                },
                "其他构件".to_owned(),
                "",
            );
            others_node.count = Some(spec.others.count);
            others_node.leaves_inline = others_inline(&spec.others);
            others_node.meta = spec.others.count.to_string();
            others_node.hover = format!(
                "其他构件 · 不属任何最小交付单元的构件，按 noun 分 · {} 个",
                spec.others.count
            );
            for group in &spec.others.by_noun {
                let inline = group.elements.is_some();
                let mut group_node = RoomTreeNode::bare(
                    RoomNodeId::OtherNoun {
                        room,
                        spec: spec.spec_value,
                        noun: group.noun.clone(),
                    },
                    group.noun.clone(),
                    &group.noun,
                );
                group_node.count = Some(group.count);
                group_node.leaves_inline = inline;
                group_node.leaf_selector =
                    (!inline).then(|| LeafSelector::OtherNoun(group.noun.clone()));
                group_node.distance = Some(group.min_distance);
                group_node.meta = group.count.to_string();
                group_node.hover = format!(
                    "{} · {} 个构件 · 最近 {}",
                    group.noun,
                    group.count,
                    format_distance(group.min_distance)
                );
                for leaf in group.elements.iter().flatten() {
                    group_node.children.extend(leaf_node(room, leaf));
                }
                others_node.children.push(group_node);
            }
            spec_node.children.push(others_node);
        }
        node.children.push(spec_node);
    }
    Some(node)
}

/// 把按 `unit=` / `other_noun=` 补回来的叶子合进已有的树：同一单元 refno / 同一 noun 组的
/// `elements` 用新响应里的填上（单元的 `tubes` 跟 `elements` 一起来、一起填），别的节点不动。
pub fn merge_tree_leaves(tree: &TreeResponse, partial: &TreeResponse) -> TreeResponse {
    /// (房间 refno, 单元 refno) → 补回来的构件与直段。
    type UnitLeaves = HashMap<(String, String), (Vec<TreeLeaf>, Option<Vec<TreeTube>>)>;
    /// (房间 refno, 专业, noun) → 补回来的构件。
    type OtherLeaves = HashMap<(String, i64, String), Vec<TreeLeaf>>;
    let mut unit_leaves: UnitLeaves = HashMap::new();
    let mut other_leaves: OtherLeaves = HashMap::new();
    for room in &partial.rooms {
        for spec in &room.specs {
            for group in &spec.unit_types {
                for unit in &group.units {
                    if let Some(elements) = &unit.elements {
                        unit_leaves.insert(
                            (room.refno.clone(), unit.refno.clone()),
                            (elements.clone(), unit.tubes.clone()),
                        );
                    }
                }
            }
            for group in &spec.others.by_noun {
                if let Some(elements) = &group.elements {
                    other_leaves.insert(
                        (room.refno.clone(), spec.spec_value, group.noun.clone()),
                        elements.clone(),
                    );
                }
            }
        }
    }
    let mut merged = tree.clone();
    for room in &mut merged.rooms {
        for spec in &mut room.specs {
            for group in &mut spec.unit_types {
                for unit in &mut group.units {
                    if let Some((elements, tubes)) =
                        unit_leaves.get(&(room.refno.clone(), unit.refno.clone()))
                    {
                        unit.elements = Some(elements.clone());
                        if tubes.is_some() {
                            unit.tubes = tubes.clone();
                        }
                    }
                }
            }
            for group in &mut spec.others.by_noun {
                if let Some(elements) =
                    other_leaves.get(&(room.refno.clone(), spec.spec_value, group.noun.clone()))
                {
                    group.elements = Some(elements.clone());
                }
            }
        }
    }
    merged
}

// ---------------------------------------------------------------- 收集器

fn visit<'a>(node: &'a RoomTreeNode, f: &mut impl FnMut(&'a RoomTreeNode) -> bool) {
    if !f(node) {
        return;
    }
    for child in &node.children {
        visit(child, f);
    }
}

/// 节点下已知的全部构件 refno（去重、树序）；叶子未内联的部分不在里面——调用方先补叶子。
/// 直段行不进来。
pub fn refnos_under(node: &RoomTreeNode) -> Vec<RefU64> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    visit(node, &mut |n| {
        if n.kind() == RoomNodeKind::Element
            && let Some(refno) = n.refno
            && seen.insert(refno)
        {
            out.push(refno);
        }
        true
    });
    out
}

/// 节点下（含自身）的 BRAN 单元 refno（去重）。直管挂在 BRAN 自己的 refno 上，单元级及以上的
/// 显隐 / 隔离要把它们一起带上（方案 A）；构件行不经这里。HANG / EQUI 单元没有直管，不给。
pub fn bran_units_under(node: &RoomTreeNode) -> Vec<RefU64> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    visit(node, &mut |n| {
        if n.kind() == RoomNodeKind::Unit && n.noun.eq_ignore_ascii_case("BRAN") {
            if let Some(refno) = n.refno
                && seen.insert(refno)
            {
                out.push(refno);
            }
            return false;
        }
        true
    });
    out
}

/// 节点下还没内联叶子的单元 / noun 组（要先按各自的选择器补一次才有全部 refno）。
pub fn pending_leaf_nodes_under(node: &RoomTreeNode) -> Vec<(RoomNodeId, LeafSelector)> {
    let mut out = Vec::new();
    visit(node, &mut |n| {
        if matches!(n.kind(), RoomNodeKind::Unit | RoomNodeKind::OtherNoun)
            && !n.leaves_inline
            && let Some(selector) = &n.leaf_selector
        {
            out.push((n.id.clone(), selector.clone()));
        }
        true
    });
    out
}

/// 房间号自然序（`R43` < `R432`，字母段不分大小写），再按名字、refno。
pub fn compare_rooms(a: &RoomListItem, b: &RoomListItem) -> Ordering {
    natural_cmp(&a.room_num, &b.room_num)
        .then_with(|| {
            a.name
                .as_deref()
                .unwrap_or("")
                .cmp(b.name.as_deref().unwrap_or(""))
        })
        .then_with(|| a.refno.cmp(&b.refno))
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut xa = a.chars().peekable();
    let mut xb = b.chars().peekable();
    loop {
        match (xa.peek().copied(), xb.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(ca), Some(cb)) if ca.is_ascii_digit() && cb.is_ascii_digit() => {
                let mut na = 0u128;
                let mut nb = 0u128;
                while let Some(c) = xa.peek().copied().filter(char::is_ascii_digit) {
                    na = na
                        .saturating_mul(10)
                        .saturating_add(c as u128 - '0' as u128);
                    xa.next();
                }
                while let Some(c) = xb.peek().copied().filter(char::is_ascii_digit) {
                    nb = nb
                        .saturating_mul(10)
                        .saturating_add(c as u128 - '0' as u128);
                    xb.next();
                }
                match na.cmp(&nb) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(ca), Some(cb)) => {
                let la = ca.to_lowercase().next().unwrap_or(ca);
                let lb = cb.to_lowercase().next().unwrap_or(cb);
                match la.cmp(&lb) {
                    Ordering::Equal => {
                        xa.next();
                        xb.next();
                    }
                    other => return other,
                }
            }
        }
    }
}

/// `status` 非 ready / degraded 且服务端没给 `reason` 时的兜底一句。
pub fn rooms_status_fallback(status: &str) -> String {
    match status {
        "unsupported" => "当前服务端没有房间接口".to_owned(),
        "disabled" => "服务端未开启房间归属计算（room_membership=false）".to_owned(),
        "initializing" => "房间模型正在就绪，稍后再试".to_owned(),
        "failed" => "房间模型构建失败".to_owned(),
        other => format!("房间体制状态：{other}"),
    }
}

// ---------------------------------------------------------------- 动作（绘制层 → 宿主）

/// 点选时按着的修饰键；与模型树同手势：Shift 区间、Ctrl（mac 为 Cmd）加减。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SelectModifiers {
    pub shift: bool,
    pub command: bool,
}

/// 「房间树」面板发出的动作，宿主取数、缓存、再转成既有的 [`crate::ModelAction`]。
#[derive(Debug, Clone, PartialEq)]
pub enum RoomTreeAction {
    /// 拉在册房间清单：面板第一次画出来（`RoomTreeVm::Idle`）或失败态上的「重试」。
    Load,
    /// 刷新钮：丢掉全部缓存（根清单与各房的树）重拉。
    Refresh,
    /// 顶部搜索框：按房间号 / 名称 / refno 过滤根清单。
    SetFilter(String),
    /// 点箭头 / 双击行：房间根第一次展开去取它的树，叶子未内联的单元 / noun 组展开时补叶子。
    ToggleExpand(RoomNodeId),
    /// 点行：树内选中，并把能落到 refno 上的那部分写进全局选择集（直段行 = 所属 BRAN）。
    Select {
        id: RoomNodeId,
        modifiers: SelectModifiers,
    },
    /// 眼睛 / 右键「显示 / 隐藏」：节点下全部构件（单元级及以上连所属 BRAN）→ `SetVisible`。
    SetVisible { id: RoomNodeId, visible: bool },
    /// 右键「聚焦」：构件 / 分组按已加载几何的并集盒（`FocusGroup`），直段按它自己的 `aabb`（`FocusBounds`）。
    Focus(RoomNodeId),
    /// 右键「隔离」：节点下构件 + 所属 BRAN 整条留实体，其余隐藏（`Isolate`）。
    Isolate(RoomNodeId),
    /// 右键「加载模型」= 显示节点下全部构件；超过 200 个先弹确认，`confirmed` 是确认框按下来的那一回。
    LoadModels { id: RoomNodeId, confirmed: bool },
    /// 确认框上的「取消」/ 关窗。
    CancelLoadModels,
}

/// 「加载模型」超过这个数先弹确认（与 plant3d-web 同阈值，用户 2026-09-14 拍板）。
pub const LARGE_LOAD_CONFIRM_THRESHOLD: usize = 200;

// ---------------------------------------------------------------- 状态机（不做 IO）

/// 根清单的状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub enum RootsStatus {
    #[default]
    Idle,
    Loading,
    /// `note` = `degraded` 时服务端给的原因（面板状态行上跟一句）。
    Ready {
        note: Option<String>,
    },
    /// 房间体制未就绪 / 老服务端 / 库供数：整面板一句注释。
    Unavailable(String),
    Failed(String),
}

/// 展开一个节点之后还要去取什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandNeed {
    /// 房间根第一次展开：取整棵树。
    Tree(RefU64),
    /// 叶子未内联的单元 / noun 组：按选择器补一组。
    Leaves(RoomNodeId, LeafSelector),
}

/// 一行的 eye 从哪来：宿主按 refno 给实际可见性与待执行方向（模型树的同一份账）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EyeState {
    pub visibility: RowVisibility,
    /// 点过眼睛、指令还没落到画面上的方向；`Some` 时下一次点击反转它而不是反转图标。
    pub pending: Option<bool>,
}

/// 行眼睛的两路来源（都由宿主按 View3d 的实际状态给）：`model` 按 refno（构件 / 单元 / BRAN 自己），
/// `tube` 按 (所属 BRAN, 直段身份)（直段行，阶段四）。
pub struct EyeSource<'a> {
    pub model: &'a dyn Fn(RefU64) -> EyeState,
    pub tube: &'a dyn Fn(RefU64, &TubeSegment) -> EyeState,
}

/// 「房间树」的全部状态：根清单、各房取回的树、展开 / 在途 / 失败、过滤词、树内选中。
/// 不做 IO：宿主拿 [`ExpandNeed`] / 收集器去取数、去发 `ModelAction`，取回来再喂进来。
#[derive(Debug, Default)]
pub struct RoomTreeModel {
    pub status: RootsStatus,
    /// 在册房间，按房间号自然序。
    roots: Vec<RoomListItem>,
    /// 每间房的根节点；取过树的整棵都在它底下。
    nodes: HashMap<RefU64, RoomTreeNode>,
    /// 每间房取回的整树（补叶子时合并后再展一遍）。
    trees: HashMap<RefU64, TreeResponse>,
    fetched: HashSet<RefU64>,
    pub expanded: HashSet<RoomNodeId>,
    /// 正在取树 / 补叶子的节点。
    pub loading: HashSet<RoomNodeId>,
    errors: Vec<(RoomNodeId, String)>,
    pub filter: String,
    /// 树内选中的行（点选顺序）；最后一次单点 / Ctrl 点的那一行是锚（Shift 区间的另一端）。
    selected: Vec<RoomNodeId>,
    anchor: Option<RoomNodeId>,
}

impl RoomTreeModel {
    /// 全部清掉（刷新 / 换项目 / 重连）。过滤词留着：那是用户敲的，不是数据。
    pub fn clear(&mut self) {
        let filter = std::mem::take(&mut self.filter);
        *self = Self {
            filter,
            ..Self::default()
        };
    }

    pub fn begin_roots(&mut self) {
        self.status = RootsStatus::Loading;
    }

    /// 在册房间清单到了。非 ready / degraded 的房间体制状态折成 `Unavailable` + 原因，不当错误。
    /// 清单里不再有的房连同它的树一起丢；还在的房保留已取的树。
    pub fn set_roots(&mut self, response: RoomsResponse) {
        if response.status != STATUS_READY && response.status != STATUS_DEGRADED {
            self.status = RootsStatus::Unavailable(
                response
                    .reason
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or_else(|| rooms_status_fallback(&response.status)),
            );
            self.roots.clear();
            self.nodes.clear();
            self.trees.clear();
            self.fetched.clear();
            return;
        }
        let mut rooms = response.rooms;
        rooms.sort_by(compare_rooms);
        let keep: HashSet<RefU64> = rooms.iter().filter_map(|r| parse_refno(&r.refno)).collect();
        self.nodes.retain(|room, _| keep.contains(room));
        self.trees.retain(|room, _| keep.contains(room));
        self.fetched.retain(|room| keep.contains(room));
        for room in &rooms {
            let Some(refno) = parse_refno(&room.refno) else {
                continue;
            };
            if !self.nodes.contains_key(&refno)
                && let Some(node) = room_root_node(room)
            {
                self.nodes.insert(refno, node);
            }
        }
        self.roots = rooms;
        self.status = RootsStatus::Ready {
            note: (response.status == STATUS_DEGRADED).then(|| {
                response
                    .reason
                    .filter(|reason| !reason.is_empty())
                    .unwrap_or_else(|| "房间体制降级运行".to_owned())
            }),
        };
    }

    pub fn fail_roots(&mut self, reason: String) {
        self.status = RootsStatus::Failed(reason);
    }

    pub fn set_unavailable(&mut self, reason: String) {
        self.status = RootsStatus::Unavailable(reason);
    }

    pub fn total(&self) -> usize {
        self.roots.len()
    }

    pub fn root(&self, room: RefU64) -> Option<&RoomListItem> {
        self.roots
            .iter()
            .find(|item| parse_refno(&item.refno) == Some(room))
    }

    pub fn is_fetched(&self, room: RefU64) -> bool {
        self.fetched.contains(&room)
    }

    /// `panel_count == 0` 的在册房：源模型里没有边界面板，树恒答不出，不发请求。
    pub fn is_empty_room(&self, room: RefU64) -> bool {
        self.root(room).is_some_and(|item| item.panel_count == 0)
    }

    /// 这间房的树还要去取（没取过、不是空房、也不在途）。
    pub fn needs_tree(&self, room: RefU64) -> bool {
        !self.fetched.contains(&room)
            && !self.is_empty_room(room)
            && !self.loading.contains(&RoomNodeId::Room(room))
    }

    /// 一间房的整树到了。
    pub fn set_tree(&mut self, room: RefU64, tree: TreeResponse) -> Result<(), String> {
        let node = flatten_room_tree(room, &tree, self.root(room))
            .ok_or_else(|| "响应里没有这间房".to_owned())?;
        self.nodes.insert(room, node);
        self.trees.insert(room, tree);
        self.fetched.insert(room);
        self.clear_error(&RoomNodeId::Room(room));
        Ok(())
    }

    /// 按 `unit=` / `other_noun=` 补回来的一组叶子到了：合进这间房的树再展一遍。
    pub fn merge_leaves(&mut self, room: RefU64, partial: TreeResponse) -> Result<(), String> {
        let tree = self
            .trees
            .get(&room)
            .ok_or_else(|| "这间房的树还没取回".to_owned())?;
        let merged = merge_tree_leaves(tree, &partial);
        let node = flatten_room_tree(room, &merged, self.root(room))
            .ok_or_else(|| "响应里没有这间房".to_owned())?;
        self.nodes.insert(room, node);
        self.trees.insert(room, merged);
        Ok(())
    }

    pub fn set_error(&mut self, id: RoomNodeId, message: String) {
        self.clear_error(&id);
        self.errors.push((id, message));
    }

    pub fn clear_error(&mut self, id: &RoomNodeId) {
        self.errors.retain(|(other, _)| other != id);
    }

    pub fn error_of(&self, id: &RoomNodeId) -> Option<&str> {
        self.errors
            .iter()
            .find(|(other, _)| other == id)
            .map(|(_, message)| message.as_str())
    }

    /// 一条取数回包是哪个节点在等：整树 = 房间根；`unit=` = 那个单元；`other_noun=` 的 id 里还有专业值，
    /// 回包里没有，从在途集里按（房间、noun）找回来。
    pub fn loading_node_for(
        &self,
        room: RefU64,
        only: Option<&LeafSelector>,
    ) -> Option<RoomNodeId> {
        match only {
            None => Some(RoomNodeId::Room(room)),
            Some(LeafSelector::Unit(unit)) => Some(RoomNodeId::Unit { room, unit: *unit }),
            Some(LeafSelector::OtherNoun(noun)) => self
                .loading
                .iter()
                .find(|id| {
                    matches!(id, RoomNodeId::OtherNoun { room: r, noun: n, .. } if *r == room && n == noun)
                })
                .cloned(),
        }
    }

    /// 展开过、取树 / 补叶子失败的节点：一句原因列在树上方。
    pub fn errors_vm(&self) -> Vec<RoomTreeErrorVm> {
        self.errors
            .iter()
            .map(|(id, message)| RoomTreeErrorVm {
                id: id.clone(),
                label: self
                    .node(id)
                    .map(|node| node.label.clone())
                    .unwrap_or_else(|| format!("{id:?}")),
                message: message.clone(),
            })
            .collect()
    }

    /// 按 id 找节点（在它所属房间的树里走一遍）。
    pub fn node(&self, id: &RoomNodeId) -> Option<&RoomTreeNode> {
        fn find<'a>(node: &'a RoomTreeNode, id: &RoomNodeId) -> Option<&'a RoomTreeNode> {
            if &node.id == id {
                return Some(node);
            }
            node.children.iter().find_map(|child| find(child, id))
        }
        find(self.nodes.get(&id.room())?, id)
    }

    /// 从根到 `id` 的父链（不含自身）；节点不在树里回空。
    pub fn ancestors(&self, id: &RoomNodeId) -> Vec<RoomNodeId> {
        fn path(node: &RoomTreeNode, id: &RoomNodeId, trail: &mut Vec<RoomNodeId>) -> bool {
            if &node.id == id {
                return true;
            }
            trail.push(node.id.clone());
            for child in &node.children {
                if path(child, id, trail) {
                    return true;
                }
            }
            trail.pop();
            false
        }
        let mut trail = Vec::new();
        match self.nodes.get(&id.room()) {
            Some(root) if path(root, id, &mut trail) => trail,
            _ => Vec::new(),
        }
    }

    /// 展开 / 折叠。展开时回「还要去取什么」。
    pub fn toggle_expand(&mut self, id: &RoomNodeId) -> Option<ExpandNeed> {
        if self.expanded.remove(id) {
            return None;
        }
        self.expanded.insert(id.clone());
        self.expand_need(id)
    }

    /// 展开到这个节点时要补的数据（房间根要树、未内联的组要叶子）。
    pub fn expand_need(&self, id: &RoomNodeId) -> Option<ExpandNeed> {
        if let RoomNodeId::Room(room) = id {
            return self.needs_tree(*room).then_some(ExpandNeed::Tree(*room));
        }
        let node = self.node(id)?;
        if matches!(node.kind(), RoomNodeKind::Unit | RoomNodeKind::OtherNoun)
            && !node.leaves_inline
            && !self.loading.contains(id)
        {
            let selector = node.leaf_selector.clone()?;
            return Some(ExpandNeed::Leaves(id.clone(), selector));
        }
        None
    }

    pub fn refnos_under(&self, id: &RoomNodeId) -> Vec<RefU64> {
        self.node(id).map(refnos_under).unwrap_or_default()
    }

    pub fn bran_units_under(&self, id: &RoomNodeId) -> Vec<RefU64> {
        self.node(id).map(bran_units_under).unwrap_or_default()
    }

    pub fn pending_leaf_nodes_under(&self, id: &RoomNodeId) -> Vec<(RoomNodeId, LeafSelector)> {
        self.node(id)
            .map(pending_leaf_nodes_under)
            .unwrap_or_default()
    }

    /// 显隐 / 隔离作用的场景 refno：节点下的构件 + 它们所属 BRAN 的整体（直管挂在 BRAN 自己的
    /// refno 上）。构件行不扩（一个管件就是一个管件，E3D 口径）。
    pub fn scene_refnos_under(&self, id: &RoomNodeId) -> Vec<RefU64> {
        let Some(node) = self.node(id) else {
            return Vec::new();
        };
        let mut out = refnos_under(node);
        if node.kind() == RoomNodeKind::Element {
            return out;
        }
        let mut seen: HashSet<RefU64> = out.iter().copied().collect();
        for bran in bran_units_under(node) {
            if seen.insert(bran) {
                out.push(bran);
            }
        }
        out
    }

    /// 过滤后的根清单（按房间号 / 名字 / refno 的子串，不分大小写；refno 认 `a_b` 与 `a/b`）。
    pub fn filtered_roots(&self) -> Vec<&RoomListItem> {
        let q = self.filter.trim().to_lowercase();
        self.roots
            .iter()
            .filter(|room| {
                q.is_empty()
                    || room.room_num.to_lowercase().contains(&q)
                    || room
                        .name
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&q)
                    || room.refno.contains(&q)
                    || room.refno.replace('_', "/").contains(&q)
            })
            .collect()
    }

    /// 按展开状态展平成行。`eyes.model` 按 refno 给实际可见性；父节点按子节点聚合（全 / 半 / 无），
    /// BRAN 单元另算上它自己（直管挂在它名下）；直段行按 `eyes.tube` 逐段给（阶段四），已装进场景的
    /// 直段也计入所属 BRAN 单元的聚合，没装的不计（否则老服务端的整根已显示会被说成半可见）。
    pub fn rows(&self, eyes: &EyeSource<'_>) -> Vec<RoomTreeRowVm> {
        let mut rows = Vec::new();
        let primary = self.anchor.as_ref();
        for room in self.filtered_roots() {
            let Some(refno) = parse_refno(&room.refno) else {
                continue;
            };
            let Some(node) = self.nodes.get(&refno) else {
                continue;
            };
            self.walk(node, 0, eyes, primary, &mut rows);
        }
        rows
    }

    fn walk(
        &self,
        node: &RoomTreeNode,
        depth: u16,
        eyes: &EyeSource<'_>,
        primary: Option<&RoomNodeId>,
        rows: &mut Vec<RoomTreeRowVm>,
    ) {
        let state = eye_of(node, eyes);
        let expandable = node
            .has_children()
            .then(|| self.expanded.contains(&node.id));
        rows.push(RoomTreeRowVm {
            id: node.id.clone(),
            kind: node.kind(),
            depth,
            label: node.label.clone(),
            meta: node.meta.clone(),
            hover: node.hover.clone(),
            noun: node.noun.clone(),
            tags: node.tags.clone(),
            expandable,
            loading: self.loading.contains(&node.id),
            visibility: state.map(|s| s.visibility),
            next_visible: state
                .map(|s| s.pending.map_or_else(|| s.visibility.on_click(), |p| !p))
                .unwrap_or(true),
            selected: self.selected.contains(&node.id),
            primary: primary == Some(&node.id),
            refno: node.refno.or_else(|| node.tube.as_ref().map(|t| t.unit)),
            count: node.count,
        });
        if expandable == Some(true) {
            for child in &node.children {
                self.walk(child, depth + 1, eyes, primary, rows);
            }
        }
    }

    /// 行选中（单击 / Ctrl 多选 / Shift 连选，与模型树同手势；`order` 是此刻的可见行序）。
    /// 回这次选中里能落到场景键上的 refno（单元 / 构件行的 refno；直段行回所属 BRAN），
    /// 调用方拿它设全局选中；只选了分组行时回空，全局选中不动。
    pub fn select(
        &mut self,
        id: &RoomNodeId,
        modifiers: SelectModifiers,
        order: &[RoomNodeId],
    ) -> Vec<RefU64> {
        if modifiers.shift
            && let Some(anchor) = self.anchor.clone()
        {
            let a = order.iter().position(|row| row == &anchor);
            let b = order.iter().position(|row| row == id);
            match (a, b) {
                (Some(a), Some(b)) => {
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    for row in &order[lo..=hi] {
                        if !self.selected.contains(row) {
                            self.selected.push(row.clone());
                        }
                    }
                }
                _ => {
                    self.selected = vec![id.clone()];
                    self.anchor = Some(id.clone());
                }
            }
        } else if modifiers.command {
            if let Some(pos) = self.selected.iter().position(|row| row == id) {
                self.selected.remove(pos);
            } else {
                self.selected.push(id.clone());
            }
            self.anchor = Some(id.clone());
        } else {
            self.selected = vec![id.clone()];
            self.anchor = Some(id.clone());
        }
        self.selected_refnos()
    }

    /// 树内选中里能落到场景键上的 refno（去重、选中顺序）。
    pub fn selected_refnos(&self) -> Vec<RefU64> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for id in &self.selected {
            let Some(node) = self.node(id) else {
                continue;
            };
            let refno = match node.kind() {
                RoomNodeKind::Element | RoomNodeKind::Unit => node.refno,
                RoomNodeKind::Tube => node.tube.as_ref().map(|t| t.unit),
                _ => None,
            };
            if let Some(refno) = refno
                && seen.insert(refno)
            {
                out.push(refno);
            }
        }
        out
    }

    pub fn is_selected(&self, id: &RoomNodeId) -> bool {
        self.selected.contains(id)
    }

    pub fn selected(&self) -> &[RoomNodeId] {
        &self.selected
    }

    /// 外部选中联动：某个 refno（构件或单元）若已在**取过树的房**里，展开到它、单选它、回它的 id
    /// 供滚动；优先当前已展开的房。**不**为了它去拉没展开的房。找不到回 `None`。
    pub fn reveal_refno(&mut self, refno: RefU64) -> Option<RoomNodeId> {
        let mut hit: Option<RoomNodeId> = None;
        let mut rooms: Vec<RefU64> = self.fetched.iter().copied().collect();
        rooms.sort();
        for room in rooms {
            let Some(root) = self.nodes.get(&room) else {
                continue;
            };
            let mut found: Option<RoomNodeId> = None;
            visit(root, &mut |n| {
                if found.is_none()
                    && matches!(n.kind(), RoomNodeKind::Element | RoomNodeKind::Unit)
                    && n.refno == Some(refno)
                {
                    found = Some(n.id.clone());
                }
                found.is_none()
            });
            let Some(found) = found else {
                continue;
            };
            let expanded_here = self.expanded.contains(&RoomNodeId::Room(room));
            let replace = match &hit {
                None => true,
                Some(prev) => {
                    expanded_here && !self.expanded.contains(&RoomNodeId::Room(prev.room()))
                }
            };
            if replace {
                hit = Some(found);
            }
        }
        let hit = hit?;
        for ancestor in self.ancestors(&hit) {
            self.expanded.insert(ancestor);
        }
        self.selected = vec![hit.clone()];
        self.anchor = Some(hit.clone());
        Some(hit)
    }

    /// 「在房间树查看」一间房：过滤词把它挡住就清过滤，展开它、单选它。不在册回 `None`；
    /// 树还没取时由调用方按 [`Self::needs_tree`] 去取。
    pub fn reveal_room(&mut self, room: RefU64) -> Option<RoomNodeId> {
        self.root(room)?;
        if !self.filter.is_empty()
            && !self
                .filtered_roots()
                .iter()
                .any(|item| parse_refno(&item.refno) == Some(room))
        {
            self.filter.clear();
        }
        let id = RoomNodeId::Room(room);
        self.expanded.insert(id.clone());
        self.selected = vec![id.clone()];
        self.anchor = Some(id.clone());
        Some(id)
    }
}

/// 一个节点的 eye：构件按 refno；BRAN 单元 = 它自己（直管）+ 构件；其余分组按子节点聚合；
/// 直段行没有（`None`）。
fn eye_of(node: &RoomTreeNode, eyes: &EyeSource<'_>) -> Option<EyeState> {
    match node.kind() {
        RoomNodeKind::Tube => node
            .id
            .tube_segment()
            .map(|(bran, segment)| (eyes.tube)(bran, &segment)),
        RoomNodeKind::Element => node.refno.map(eyes.model),
        kind => {
            let mut states: Vec<EyeState> = node
                .children
                .iter()
                .filter_map(|child| {
                    let state = eye_of(child, eyes)?;
                    // 没装进场景的直段不参与聚合：整根已显示时它的「未加载」只说明没有逐段身份。
                    (child.kind() != RoomNodeKind::Tube
                        || state.visibility != RowVisibility::Unloaded)
                        .then_some(state)
                })
                .collect();
            if kind == RoomNodeKind::Unit
                && node.noun.eq_ignore_ascii_case("BRAN")
                && let Some(refno) = node.refno
            {
                states.push((eyes.model)(refno));
            }
            Some(aggregate(&states))
        }
    }
}

/// 子节点的 eye 聚合成父节点的：都没加载 = 未加载；有显示有隐藏、或有半可见、或有显示也有
/// 未加载 = 半可见；全显示 = 显示；其余 = 隐藏。与模型树容器行的 `row_visibility` 同一把尺。
pub fn aggregate(states: &[EyeState]) -> EyeState {
    let mut shown = 0usize;
    let mut hidden = 0usize;
    let mut partial = 0usize;
    let mut unloaded = 0usize;
    let mut pending = None;
    for state in states {
        match state.visibility {
            RowVisibility::Shown => shown += 1,
            RowVisibility::Hidden => hidden += 1,
            RowVisibility::Partial => partial += 1,
            RowVisibility::Unloaded => unloaded += 1,
        }
        if pending.is_none() {
            pending = state.pending;
        }
    }
    let visibility = if shown + hidden + partial == 0 {
        RowVisibility::Unloaded
    } else if partial > 0 || (shown > 0 && hidden > 0) || (shown > 0 && unloaded > 0) {
        RowVisibility::Partial
    } else if shown > 0 {
        RowVisibility::Shown
    } else {
        RowVisibility::Hidden
    };
    EyeState {
        visibility,
        pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(a: u32, b: u32) -> RefU64 {
        RefU64::from_two_nums(a, b)
    }

    fn leaf(refno: &str, noun: &str, distance: f64, shared: Option<u32>) -> TreeLeaf {
        TreeLeaf {
            refno: refno.into(),
            noun: noun.into(),
            distance,
            shared_rooms: shared,
        }
    }

    fn tube(
        ordinal: u32,
        from: &str,
        to: &str,
        from_noun: &str,
        to_noun: &str,
        length: f64,
        invalid: bool,
    ) -> TreeTube {
        TreeTube {
            ordinal,
            from: from.into(),
            to: to.into(),
            from_noun: from_noun.into(),
            to_noun: to_noun.into(),
            distance: 120.0,
            length,
            aabb: Aabb {
                min: [0.0, 0.0, 0.0],
                max: [1.0, 1.0, 1.0],
            },
            invalid,
            shared_rooms: None,
        }
    }

    fn room_item(refno: &str, room_num: &str, name: Option<&str>, panels: usize) -> RoomListItem {
        RoomListItem {
            refno: refno.into(),
            room_num: room_num.into(),
            name: name.map(str::to_owned),
            dbnum: Some(1),
            panel_count: panels,
            panel_refnos: Vec::new(),
        }
    }

    /// 与 plant3d-web `roomTreeNodes.test.ts` 的样本同形：一间房、管道 + 结构两个专业，
    /// BRAN 单元内联两个构件 + 两段直管，EQUI 单元叶子未内联，结构专业只有「其他构件」。
    fn sample_tree() -> TreeResponse {
        TreeResponse {
            total_count: 5,
            total_tube_count: Some(2),
            tubes_truncated: false,
            leaves_inline: false,
            leaf_cap: 5000,
            leaf_count: 6,
            inlined: None,
            delivery_unit_types: vec!["BRAN".into(), "EQUI".into()],
            rooms: vec![TreeRoom {
                refno: "1_100".into(),
                room_num: "R101".into(),
                name: Some("控制室".into()),
                count: 5,
                tube_count: Some(2),
                specs: vec![
                    TreeSpec {
                        spec_value: 1,
                        count: 4,
                        tube_count: Some(2),
                        unit_types: vec![
                            TreeUnitType {
                                noun: "BRAN".into(),
                                count: 2,
                                tube_count: Some(2),
                                units: vec![TreeUnit {
                                    refno: "1_10".into(),
                                    noun: "BRAN".into(),
                                    name: Some("/P1/B1".into()),
                                    count: 2,
                                    tube_count: Some(2),
                                    min_distance: 250.0,
                                    elements: Some(vec![
                                        leaf("1_11", "ELBO", 250.0, None),
                                        leaf("1_12", "VALV", 1000.0, Some(2)),
                                    ]),
                                    tubes: Some(vec![
                                        tube(0, "1_10", "1_11", "BRAN", "ELBO", 141.0, false),
                                        tube(1, "1_11", "1_12", "ELBO", "VALV", 2500.0, true),
                                    ]),
                                }],
                            },
                            TreeUnitType {
                                noun: "EQUI".into(),
                                count: 2,
                                tube_count: None,
                                units: vec![TreeUnit {
                                    refno: "1_20".into(),
                                    noun: "EQUI".into(),
                                    name: None,
                                    count: 2,
                                    tube_count: None,
                                    min_distance: 0.0,
                                    elements: None,
                                    tubes: None,
                                }],
                            },
                        ],
                        others: TreeOthers::default(),
                    },
                    TreeSpec {
                        spec_value: 6,
                        count: 1,
                        tube_count: None,
                        unit_types: Vec::new(),
                        others: TreeOthers {
                            count: 1,
                            by_noun: vec![TreeOtherNoun {
                                noun: "SCTN".into(),
                                count: 1,
                                min_distance: 1234.0,
                                elements: Some(vec![leaf("1_30", "SCTN", 1234.0, None)]),
                            }],
                        },
                    },
                ],
            }],
            warnings: Vec::new(),
        }
    }

    fn sample_roots() -> RoomsResponse {
        RoomsResponse {
            status: "ready".into(),
            reason: None,
            definition_version: Some("v1".into()),
            rooms: vec![
                room_item("1_200", "R432", Some("泵房"), 4),
                room_item("1_100", "R101", Some("控制室"), 6),
                room_item("1_300", "R43", None, 0),
            ],
        }
    }

    fn unloaded(_: RefU64) -> EyeState {
        EyeState::default()
    }

    fn no_tube(_: RefU64, _: &TubeSegment) -> EyeState {
        EyeState::default()
    }

    /// 只给 refno 那一路、直段一律没装的眼睛来源。
    fn eyes(model: &dyn Fn(RefU64) -> EyeState) -> EyeSource<'_> {
        EyeSource {
            model,
            tube: &no_tube,
        }
    }

    /// 阶段四：直段行按 (BRAN, 直段身份) 读逐段可见性；装进场景的直段计入所属 BRAN 单元的聚合，没装的不计。
    #[test]
    fn tube_rows_read_per_segment_eyes_and_loaded_tubes_join_the_bran_aggregate() {
        let mut model = RoomTreeModel::default();
        model.set_roots(sample_roots());
        let room = r(1, 100);
        model.set_tree(room, sample_tree()).unwrap();
        for id in [
            RoomNodeId::Room(room),
            RoomNodeId::Spec { room, spec: 1 },
            RoomNodeId::UnitType {
                room,
                spec: 1,
                noun: "BRAN".into(),
            },
            RoomNodeId::Unit {
                room,
                unit: r(1, 10),
            },
        ] {
            model.expanded.insert(id);
        }
        let shown = |_: RefU64| EyeState {
            visibility: RowVisibility::Shown,
            pending: None,
        };
        // 第 0 段显示、第 1 段藏了；身份要对得上（所属 BRAN + 序号 + 两端）。
        let tube = |bran: RefU64, segment: &TubeSegment| {
            assert_eq!(bran, r(1, 10));
            assert_eq!(
                segment.from,
                if segment.ordinal == 0 { "1_10" } else { "1_11" }
            );
            EyeState {
                visibility: if segment.ordinal == 0 {
                    RowVisibility::Shown
                } else {
                    RowVisibility::Hidden
                },
                pending: None,
            }
        };
        let rows = model.rows(&EyeSource {
            model: &shown,
            tube: &tube,
        });
        let tubes: Vec<_> = rows
            .iter()
            .filter(|row| row.kind == RoomNodeKind::Tube)
            .collect();
        assert_eq!(tubes.len(), 2);
        assert_eq!(tubes[0].visibility, Some(RowVisibility::Shown));
        assert!(!tubes[0].next_visible, "已显示的直段再点就是隐藏");
        assert_eq!(tubes[1].visibility, Some(RowVisibility::Hidden));
        assert!(tubes[1].next_visible);
        assert_eq!(
            tubes[1].id.tube_segment(),
            Some((
                r(1, 10),
                TubeSegment {
                    ordinal: 1,
                    from: "1_11".into(),
                    to: "1_12".into(),
                }
            ))
        );
        let bran = rows
            .iter()
            .find(|row| row.kind == RoomNodeKind::Unit)
            .unwrap();
        assert_eq!(
            bran.visibility,
            Some(RowVisibility::Partial),
            "构件与 BRAN 自己都显示、一段直段藏了 = 半可见"
        );
        // 直段没装进场景（老服务端没给身份）：不拖累整根的聚合。
        let rows = model.rows(&eyes(&shown));
        let bran = rows
            .iter()
            .find(|row| row.kind == RoomNodeKind::Unit)
            .unwrap();
        assert_eq!(bran.visibility, Some(RowVisibility::Shown));
        assert!(
            rows.iter()
                .filter(|row| row.kind == RoomNodeKind::Tube)
                .all(|row| row.visibility == Some(RowVisibility::Unloaded))
        );
    }

    #[test]
    fn labels_follow_the_source_wording() {
        assert_eq!(format_distance(3000.0), "3 m");
        assert_eq!(format_distance(1250.0), "1.25 m");
        assert_eq!(format_distance(1234.0), "1.234 m");
        assert_eq!(format_tube_length(141.0), "141 mm");
        assert_eq!(format_tube_length(2500.0), "2.5 m");
        assert_eq!(format_tube_length(1000.0), "1 m");
        assert_eq!(count_phrase(5, Some(2)), "5 个构件 · 2 条直段");
        assert_eq!(count_phrase(5, None), "5 个构件");
        assert_eq!(spec_name(1), "管道系统");
        assert_eq!(spec_name(9), "未知专业(9)");
        assert_eq!(tube_label("", "ELBO"), "? → ELBO");
    }

    #[test]
    fn the_hierarchy_is_room_spec_type_unit_leaf_with_tubes_after_leaves() {
        let root = flatten_room_tree(r(1, 100), &sample_tree(), None).unwrap();
        assert_eq!(root.label, "R101 · 控制室");
        assert_eq!(root.meta, "5 个构件 · 2 条直段");
        assert!(!root.leaves_inline, "EQUI 单元的叶子没内联，整房就不算内联");
        let specs: Vec<_> = root.children.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(specs, ["管道系统", "结构系统"]);
        let pipe = &root.children[0];
        assert_eq!(pipe.meta, "4");
        let bran_group = &pipe.children[0];
        assert_eq!(bran_group.label, "BRAN");
        assert_eq!(bran_group.meta, "1 个单元 · 2");
        let bran = &bran_group.children[0];
        assert_eq!(bran.label, "/P1/B1");
        assert_eq!(bran.meta, "2 · 2 条直段");
        assert_eq!(bran.refno, Some(r(1, 10)));
        let kinds: Vec<_> = bran.children.iter().map(|c| c.kind()).collect();
        assert_eq!(
            kinds,
            [
                RoomNodeKind::Element,
                RoomNodeKind::Element,
                RoomNodeKind::Tube,
                RoomNodeKind::Tube
            ]
        );
        let valve = &bran.children[1];
        assert_eq!(valve.label, "1_12");
        assert_eq!(valve.meta, "VALV · 1 m");
        assert_eq!(valve.tags.len(), 1);
        assert_eq!(valve.tags[0].text, "跨 2 房");
        let tube = &bran.children[3];
        assert_eq!(tube.label, "ELBO → VALV");
        assert_eq!(tube.meta, "直段 · 2.5 m");
        assert_eq!(tube.tags[0].text, "无效");
        assert_eq!(tube.tube.as_ref().unwrap().unit, r(1, 10));
        assert!(tube.hover.contains("第 2 段"));
        // EQUI 单元叶子未内联：名字缺省用 refno，带选择器，仍然可展开。
        let equi = &pipe.children[1].children[0];
        assert_eq!(equi.label, "1_20");
        assert_eq!(equi.leaf_selector, Some(LeafSelector::Unit(r(1, 20))));
        assert!(equi.has_children());
        // 结构专业只有「其他构件」。
        let stru = &root.children[1];
        let others = &stru.children[0];
        assert_eq!(others.label, "其他构件");
        assert_eq!(others.meta, "1");
        assert_eq!(others.children[0].label, "SCTN");
        assert_eq!(others.children[0].children[0].refno, Some(r(1, 30)));
    }

    #[test]
    fn collectors_skip_tubes_and_stop_at_bran_units() {
        let root = flatten_room_tree(r(1, 100), &sample_tree(), None).unwrap();
        assert_eq!(refnos_under(&root), [r(1, 11), r(1, 12), r(1, 30)]);
        assert_eq!(bran_units_under(&root), [r(1, 10)]);
        let pending = pending_leaf_nodes_under(&root);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].1, LeafSelector::Unit(r(1, 20)));
        let bran = &root.children[0].children[0].children[0];
        assert_eq!(bran_units_under(bran), [r(1, 10)], "单元自己也算");
        assert_eq!(
            refnos_under(&bran.children[2]),
            Vec::<RefU64>::new(),
            "直段行不进 refno 集"
        );
    }

    #[test]
    fn merging_leaves_fills_only_the_requested_group() {
        let tree = sample_tree();
        let mut partial = sample_tree();
        partial.rooms[0].specs[0].unit_types[0].units[0].elements = None;
        partial.rooms[0].specs[0].unit_types[0].units[0].tubes = None;
        partial.rooms[0].specs[0].unit_types[1].units[0].elements = Some(vec![
            leaf("1_21", "NOZZ", 10.0, None),
            leaf("1_22", "NOZZ", 20.0, None),
        ]);
        let merged = merge_tree_leaves(&tree, &partial);
        let equi = &merged.rooms[0].specs[0].unit_types[1].units[0];
        assert_eq!(equi.elements.as_ref().unwrap().len(), 2);
        let bran = &merged.rooms[0].specs[0].unit_types[0].units[0];
        assert_eq!(bran.elements.as_ref().unwrap().len(), 2, "别的单元不动");
        assert_eq!(bran.tubes.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn roots_sort_naturally_and_filter_by_num_name_or_refno() {
        let mut model = RoomTreeModel::default();
        model.set_roots(sample_roots());
        assert_eq!(model.status, RootsStatus::Ready { note: None });
        let nums: Vec<_> = model
            .filtered_roots()
            .iter()
            .map(|r| r.room_num.as_str())
            .collect();
        assert_eq!(
            nums,
            ["R43", "R101", "R432"],
            "R43 < R101 < R432：数字段按值比"
        );
        model.filter = "泵".into();
        assert_eq!(model.filtered_roots().len(), 1);
        model.filter = "1/100".into();
        assert_eq!(model.filtered_roots()[0].room_num, "R101");
        model.filter = "r4".into();
        assert_eq!(model.filtered_roots().len(), 2);
    }

    #[test]
    fn an_empty_room_is_a_leaf_and_never_asks_for_a_tree() {
        let mut model = RoomTreeModel::default();
        model.set_roots(sample_roots());
        let empty = RoomNodeId::Room(r(1, 300));
        let node = model.node(&empty).unwrap();
        assert_eq!(node.meta, "空房");
        assert!(!node.has_children());
        assert!(!model.needs_tree(r(1, 300)));
        assert_eq!(model.toggle_expand(&empty), None);
        assert_eq!(
            model.toggle_expand(&RoomNodeId::Room(r(1, 100))),
            Some(ExpandNeed::Tree(r(1, 100)))
        );
    }

    #[test]
    fn rows_follow_expansion_and_aggregate_eyes_from_leaves() {
        let mut model = RoomTreeModel::default();
        model.set_roots(sample_roots());
        model.filter = "R101".into();
        let room = r(1, 100);
        model.toggle_expand(&RoomNodeId::Room(room));
        model.set_tree(room, sample_tree()).unwrap();
        let rows = model.rows(&eyes(&unloaded));
        let labels: Vec<_> = rows.iter().map(|row| row.label.as_str()).collect();
        assert_eq!(labels, ["R101 · 控制室", "管道系统", "结构系统"]);
        assert_eq!(rows[0].expandable, Some(true));
        assert_eq!(rows[0].visibility, Some(RowVisibility::Unloaded));
        // 展开到 BRAN：构件行与直段行都出来；直段行的眼睛按逐段账给（这里没装 = 未加载）、refno 指向所属 BRAN。
        model.toggle_expand(&RoomNodeId::Spec { room, spec: 1 });
        model.toggle_expand(&RoomNodeId::UnitType {
            room,
            spec: 1,
            noun: "BRAN".into(),
        });
        model.toggle_expand(&RoomNodeId::Unit {
            room,
            unit: r(1, 10),
        });
        let eye = |refno: RefU64| EyeState {
            visibility: if refno == r(1, 11) {
                RowVisibility::Shown
            } else {
                RowVisibility::Unloaded
            },
            pending: None,
        };
        let rows = model.rows(&eyes(&eye));
        let tube_row = rows
            .iter()
            .find(|row| row.kind == RoomNodeKind::Tube)
            .unwrap();
        assert_eq!(tube_row.visibility, Some(RowVisibility::Unloaded));
        assert_eq!(tube_row.refno, Some(r(1, 10)));
        assert_eq!(tube_row.depth, 4);
        let elbo = rows
            .iter()
            .find(|row| row.refno == Some(r(1, 11)) && row.kind == RoomNodeKind::Element)
            .unwrap();
        assert_eq!(elbo.visibility, Some(RowVisibility::Shown));
        assert!(!elbo.next_visible, "已显示的再点就是隐藏");
        let bran = rows
            .iter()
            .find(|row| row.kind == RoomNodeKind::Unit)
            .unwrap();
        assert_eq!(
            bran.visibility,
            Some(RowVisibility::Partial),
            "一个显示、一个没加载、BRAN 自己没加载 = 半可见"
        );
        assert_eq!(rows[0].visibility, Some(RowVisibility::Partial));
    }

    #[test]
    fn aggregation_matches_the_model_tree_rules() {
        let s = |visibility| EyeState {
            visibility,
            pending: None,
        };
        assert_eq!(aggregate(&[]).visibility, RowVisibility::Unloaded);
        assert_eq!(
            aggregate(&[s(RowVisibility::Unloaded), s(RowVisibility::Unloaded)]).visibility,
            RowVisibility::Unloaded
        );
        assert_eq!(
            aggregate(&[s(RowVisibility::Shown), s(RowVisibility::Shown)]).visibility,
            RowVisibility::Shown
        );
        assert_eq!(
            aggregate(&[s(RowVisibility::Hidden), s(RowVisibility::Unloaded)]).visibility,
            RowVisibility::Hidden
        );
        assert_eq!(
            aggregate(&[s(RowVisibility::Shown), s(RowVisibility::Hidden)]).visibility,
            RowVisibility::Partial
        );
        let pending = aggregate(&[
            s(RowVisibility::Shown),
            EyeState {
                visibility: RowVisibility::Shown,
                pending: Some(false),
            },
        ]);
        assert_eq!(pending.pending, Some(false), "子节点在途的方向带给父节点");
    }

    #[test]
    fn selection_resolves_to_scene_refnos_and_tubes_select_their_bran() {
        let mut model = RoomTreeModel::default();
        model.set_roots(sample_roots());
        let room = r(1, 100);
        model.set_tree(room, sample_tree()).unwrap();
        for id in [
            RoomNodeId::Room(room),
            RoomNodeId::Spec { room, spec: 1 },
            RoomNodeId::UnitType {
                room,
                spec: 1,
                noun: "BRAN".into(),
            },
            RoomNodeId::Unit {
                room,
                unit: r(1, 10),
            },
        ] {
            model.expanded.insert(id);
        }
        let order: Vec<_> = model
            .rows(&eyes(&unloaded))
            .into_iter()
            .map(|row| row.id)
            .collect();
        let tube_id = order
            .iter()
            .find(|id| id.kind() == RoomNodeKind::Tube)
            .unwrap()
            .clone();
        assert_eq!(
            model.select(&tube_id, SelectModifiers::default(), &order),
            [r(1, 10)]
        );
        let spec_id = RoomNodeId::Spec { room, spec: 1 };
        assert!(
            model
                .select(&spec_id, SelectModifiers::default(), &order)
                .is_empty(),
            "分组行没有场景键"
        );
        // Shift 从专业行连到直段行：中间的单元与两个构件都进来，refno 去重后是 BRAN + 两个构件。
        let refnos = model.select(
            &tube_id,
            SelectModifiers {
                shift: true,
                command: false,
            },
            &order,
        );
        assert_eq!(refnos, [r(1, 10), r(1, 11), r(1, 12)]);
        assert!(model.is_selected(&spec_id));
        // Ctrl 再点一次构件行就把它去掉。
        let elbo_id = RoomNodeId::Element {
            room,
            refno: r(1, 11),
        };
        let refnos = model.select(
            &elbo_id,
            SelectModifiers {
                shift: false,
                command: true,
            },
            &order,
        );
        assert_eq!(refnos, [r(1, 10), r(1, 12)]);
    }

    #[test]
    fn reveal_only_looks_inside_fetched_rooms_and_expands_the_path() {
        let mut model = RoomTreeModel::default();
        model.set_roots(sample_roots());
        assert_eq!(model.reveal_refno(r(1, 12)), None, "树没取过就不替它去拉");
        let room = r(1, 100);
        model.set_tree(room, sample_tree()).unwrap();
        let hit = model.reveal_refno(r(1, 12)).unwrap();
        assert_eq!(
            hit,
            RoomNodeId::Element {
                room,
                refno: r(1, 12)
            }
        );
        for ancestor in model.ancestors(&hit) {
            assert!(model.expanded.contains(&ancestor), "{ancestor:?} 该展开");
        }
        assert_eq!(model.ancestors(&hit).len(), 4);
        let rows = model.rows(&eyes(&unloaded));
        assert!(
            rows.iter()
                .any(|row| row.id == hit && row.selected && row.primary)
        );
        // 过滤词挡住的房，「在房间树查看」先清过滤。
        model.filter = "泵房".into();
        assert_eq!(model.reveal_room(room), Some(RoomNodeId::Room(room)));
        assert!(model.filter.is_empty());
        assert_eq!(model.reveal_room(r(9, 9)), None, "不在册");
    }

    #[test]
    fn a_non_ready_status_becomes_an_unavailable_note() {
        let mut model = RoomTreeModel::default();
        model.set_roots(RoomsResponse {
            status: "initializing".into(),
            reason: None,
            definition_version: None,
            rooms: Vec::new(),
        });
        assert_eq!(
            model.status,
            RootsStatus::Unavailable("房间模型正在就绪，稍后再试".into())
        );
        model.set_roots(RoomsResponse {
            status: "degraded".into(),
            reason: Some("部分库未对齐".into()),
            definition_version: None,
            rooms: sample_roots().rooms,
        });
        assert_eq!(
            model.status,
            RootsStatus::Ready {
                note: Some("部分库未对齐".into())
            }
        );
    }

    #[test]
    fn the_wire_types_decode_a_server_shaped_payload() {
        let body = r#"{
            "status": "ready", "definition_version": "abc",
            "rooms": [{"refno": "24384_12", "room_num": "R1", "name": null, "dbnum": 24384, "panel_count": 6, "panel_refnos": ["24384_13"]}]
        }"#;
        let rooms: RoomsResponse = serde_json::from_str(body).unwrap();
        assert_eq!(rooms.rooms[0].panel_refnos, ["24384_13"]);
        let body = r#"{
            "center": {"x": 0, "y": 0, "z": 0, "source": "refno_aabb_center"}, "radius": 0, "shape": "box",
            "total_count": 1, "candidate_count": 1, "truncated_candidates": false, "candidate_cap": 10,
            "leaves_inline": true, "leaf_cap": 5000, "leaf_count": 1, "delivery_unit_types": ["BRAN"],
            "rooms": [{"refno": "24384_12", "room_num": "R1", "name": null, "count": 1, "specs": [
                {"spec_value": 1, "count": 1, "unit_types": [], "others": {"count": 1, "by_noun": [
                    {"noun": "PANE", "count": 1, "min_distance": 0.5, "elements": [{"refno": "24384_99", "noun": "PANE", "distance": 0.5}]}
                ]}}
            ]}],
            "room_status": {"rooms": [], "source": "memory", "matched": 1, "unresolved": 0, "definition_version": null, "library_alignment_current": null},
            "spatial_state": "ready", "coverage": "global-tree"
        }"#;
        let tree: TreeResponse = serde_json::from_str(body).unwrap();
        assert_eq!(tree.total_tube_count, None, "老服务端没有直段那一格");
        let root = flatten_room_tree(r(24384, 12), &tree, None).unwrap();
        assert_eq!(root.meta, "1 个构件");
        assert_eq!(root.children[0].children[0].label, "其他构件");
    }
}
