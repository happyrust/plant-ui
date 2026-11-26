//! 提资前检查：契约、判据与行形态（纯函数；窗口在 `data_handover.rs`）。
//!
//! 输入三样：每个提资元素向模型服务问回来的事实（`GET /api/v1/node/model-status`，gen-model
//! 接口文档 §4.5.3）、队列轮询的读数（[`task_queue::Vm`]）、本窗刚排上、队列快照还没跟上的库。
//! 输出是按库一行的 [`LibRow`] 与一句整体判决 [`Verdict`]。
//!
//! 判据只在 [`judge_lib`] 一处，顺序写死（做法同 `task_queue::early_run`）：范围（够不着 / 排除）→
//! 阻断 → 已停自动重跑 → 按单元放行 → 队列里已有它的活 → 服务端说排不了 → 需初始化 → 落后 →
//! 已是最新却缺行 → 节点判不出 → 模型追赶中 / 没追平 → 不挡提交。
//!
//! 放行分两档（ADR-0036）：**按库档**是今天的服务端——库的数据水位追平文件、该库没有在排或在跑的
//! 任务、每个提资元素的节点判定是「最新」或「没有生成单元」；**按单元档**要服务端按根回答就绪
//! （[`NodeModelStatus::publish_ready`]），给了才用。数据新不新只认库的数据水位：节点判定的「落后」只看
//! 几何变化，改名、改描述不算，而提资交的正是属性。
//!
//! 界面上不出现会话号与契约字段名（ADR-0019、DESIGN-SYSTEM 校验清单）。形态逐项见画板 S15-C。

use std::collections::{BTreeMap, BTreeSet, HashSet};
use web_time::Duration;

use serde::Deserialize;

use crate::RefU64;
use crate::model_update::FileAnomaly;
use crate::style::group_number;
use crate::style::tokens::Status;
use crate::task_queue::{self, DataProgress, DbnumStatus, Phase, Timer};

// ================================================================ 契约

/// `GET /api/v1/node/model-status` 的应答里检查用得上的那几格。字段全部宽容：服务端不给的格
/// 按「没给」处理，不让一格解不动拖垮整次检查。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeModelStatus {
    #[serde(default)]
    pub dbnum: u32,
    /// 节点自己的模型判定，字面见 [`NodeVerdict::parse`]。
    #[serde(default)]
    pub verdict: String,
    #[serde(default)]
    pub verdict_reason: Option<String>,
    #[serde(default)]
    pub library: NodeLibrary,
    /// Spec 041 的「数据」一块。老服务端没有。
    #[serde(default, deserialize_with = "lenient")]
    pub data_ingest: Option<NodeIngestBlock>,
    /// 按单元档：这个节点的单元已经追平（模型已重新生成、房间归属已重算）。今天的服务端不给，
    /// 缺席就停在按库档。字段名是暂定的，随 gen-model 那一侧的规格定稿（计划阶段四 G3）。
    #[serde(default)]
    pub publish_ready: Option<bool>,
}

/// 所属库的事实，与 `/dbnums` 那一行同源同义。
#[derive(Debug, Clone, Deserialize)]
pub struct NodeLibrary {
    #[serde(default)]
    pub file_latest_sesno: i64,
    /// 文件最新那次保存的 E3D 写入时刻（RFC3339）。
    #[serde(default)]
    pub file_latest_time: Option<String>,
    #[serde(default)]
    pub applied_sesno: i64,
    /// 数据水位那次保存的写入时刻。今天的服务端不给（计划阶段四 G4）；缺席时只有追平了才说得出这一格。
    #[serde(default)]
    pub applied_sesno_time: Option<String>,
    #[serde(default)]
    pub initialized: bool,
    /// 库级判决 `in_sync` / `lagging` / `not_judged`，判据只在服务端一处。
    #[serde(default)]
    pub model_verdict: Option<String>,
    #[serde(default)]
    pub data_batch_running: bool,
    #[serde(default)]
    pub model_task_running: bool,
    /// 缺席按「在范围里」：老服务端不给这一格，猜成不在会把每个库都说成更新不了。
    #[serde(default = "yes")]
    pub in_watch_scope: bool,
}

impl Default for NodeLibrary {
    fn default() -> Self {
        Self {
            file_latest_sesno: 0,
            file_latest_time: None,
            applied_sesno: 0,
            applied_sesno_time: None,
            initialized: false,
            model_verdict: None,
            data_batch_running: false,
            model_task_running: false,
            in_watch_scope: true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeIngestBlock {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub libraries: Vec<NodeIngestLib>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeIngestLib {
    #[serde(default)]
    pub dbnum: u32,
    /// `present` / `missing` / `unknown` / `not_applicable` / `disabled`。
    #[serde(default)]
    pub verdict: String,
    /// `would_enqueue` / `already_queued` / `already_running` / `not_enqueued` / `none`。
    #[serde(default)]
    pub action: String,
    /// 缺数据却排不了的原因（`out_of_scope`、`up_to_date_rows_missing`、`room_init_loading` ……）。
    #[serde(default)]
    pub reason: Option<String>,
    /// 服务端给人看的那一句，原样用。
    #[serde(default)]
    pub message: String,
    #[serde(default, deserialize_with = "lenient")]
    pub last_failure: Option<NodeIngestFailure>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct NodeIngestFailure {
    #[serde(default)]
    pub reason: String,
    /// 按巡检的规矩此刻已停自动重跑。
    #[serde(default)]
    pub parked: bool,
}

fn yes() -> bool {
    true
}

fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| serde_json::from_value(value).ok()))
}

impl NodeModelStatus {
    /// 「数据」一块里说这个节点所属库的那一条。
    fn ingest(&self) -> Option<&NodeIngestLib> {
        let block = self.data_ingest.as_ref()?;
        block
            .libraries
            .iter()
            .find(|lib| lib.dbnum == self.dbnum)
            .or_else(|| block.libraries.first())
    }
}

/// 节点的模型判定。`unknown` 与认不出的字面归一档：都是「判不出」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeVerdict {
    Latest,
    Empty,
    Lagging,
    Partial,
    NotGenerated,
    Updating,
    Unknown,
}

impl NodeVerdict {
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "latest" => Self::Latest,
            "empty" => Self::Empty,
            "lagging" => Self::Lagging,
            "partial" => Self::Partial,
            "not_generated" => Self::NotGenerated,
            "updating" => Self::Updating,
            _ => Self::Unknown,
        }
    }

    /// 模型这一侧不用再等：全部生成过且没被改到，或者这个节点下本来就没有会产出模型的东西。
    fn settled(self) -> bool {
        matches!(self, Self::Latest | Self::Empty)
    }
}

// ================================================================ 检查结果

/// 一个提资元素查回来的结局。
#[derive(Debug, Clone)]
pub enum ElementFact {
    Checked(Box<NodeModelStatus>),
    /// 文件最新保存里没有这个元素（或它的库不在当前 MDB、不是设计库）。
    Gone,
    /// 没查成：连不上、超时、服务端内部错。不当成最新，也不当成落后。
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ElementCheck {
    pub refno: RefU64,
    pub fact: ElementFact,
}

impl ElementCheck {
    pub fn checked(refno: RefU64, status: NodeModelStatus) -> Self {
        Self {
            refno,
            fact: ElementFact::Checked(Box::new(status)),
        }
    }
}

/// 一次检查的结果。
#[derive(Debug, Clone)]
pub enum Outcome {
    /// 这台模型服务判不了（没有这条路由，或本形态不入库）：检查整块不画，提资照旧、不拦。
    Unsupported(String),
    Checked {
        elements: Vec<ElementCheck>,
        /// 本地钟点 `HH:MM:SS`，由宿主在收齐回包时填。
        at: String,
    },
}

/// 这台模型服务没有检查用的接口时的那一句。
pub const UNSUPPORTED_NO_ROUTE: &str = "这台模型服务没有提资前检查用的接口（版本较老）";
const UNSUPPORTED_NO_INGEST: &str = "模型服务这一形态不入库（内存 / 读透），数据水位按设计不建立";

/// 把逐个元素查回来的事实收成一次检查的结果。
///
/// 任何一个元素所在的库「本形态不入库」就整块不适用：那一形态数据水位按设计不建立，照着判只会把
/// 每个库都说成需初始化。
pub fn settle(elements: Vec<ElementCheck>, at: String) -> Outcome {
    let no_ingest = elements.iter().any(|element| match &element.fact {
        ElementFact::Checked(status) => status
            .ingest()
            .is_some_and(|lib| lib.verdict == "not_applicable"),
        _ => false,
    });
    if no_ingest {
        return Outcome::Unsupported(UNSUPPORTED_NO_INGEST.to_owned());
    }
    Outcome::Checked { elements, at }
}

/// 队列轮询已经说得出「这台模型服务判不了」时的理由（读透形态）；`None` = 要去问了才知道。
pub fn not_applicable(queue: &task_queue::Vm) -> Option<String> {
    queue
        .health
        .as_ref()
        .is_some_and(|health| health.data_face.trim().eq_ignore_ascii_case("read-through"))
        .then(|| UNSUPPORTED_NO_INGEST.to_owned())
}

// ================================================================ 行形态

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStage {
    Applying,
    Generating,
}

/// 一个库在检查面板上的形态（画板 S15-C 右栏）。
#[derive(Debug, Clone, PartialEq)]
pub enum LibForm {
    /// 数据与模型都追平文件最新保存。
    UpToDate,
    /// 按单元档：提资元素所在的单元已追平，其余单元还在后台。`rest` = 整批（已完成, 总数）。
    ScopeCaughtUp {
        rest: Option<(u32, u32)>,
    },
    /// 数据水位落后于文件。
    Lagging {
        saves: i64,
    },
    /// 尚无数据水位。检查只报不跑；点「更新到最新」时一并首次导入。
    NeedsInit,
    /// 数据已应用，模型还在算。
    ModelCatchingUp,
    Queued {
        first_import: bool,
        position: Option<usize>,
        waited: Option<Duration>,
    },
    Running {
        first_import: bool,
        stage: RunStage,
        /// 交付单元（已完成, 总数）。分母缺席就整格不画（ADR-0007）。
        units: Option<(u32, u32)>,
        /// 解析保存进度。只有整库解析保存那一路有（Spec 041 FR-060）。
        data: Option<DataProgress>,
        elapsed: Option<Duration>,
    },
    /// 文件异常。`label` 是短名（「文件回退」），`detail` 是含出路的原文。
    Blocked {
        label: String,
        detail: String,
    },
    /// 当前 MDB 声明了它，当前项目目录里没有它的文件。
    Unreachable,
    OutOfScope(String),
    /// 这个库的入库此前连续失败、已停自动重跑。这里不替它重试。
    Parked(String),
    /// 数据水位已是文件最新，库里却缺这些元素的行：要由人发起重新解析。
    RowsMissing,
    /// 服务端说这个库此刻排不了（房间参照模型没就绪、增量消费关着……），原话照登。
    Refused(String),
    /// 数据已是最新，模型没追平，也没有任务在跑。
    ModelStale,
    Unknown(String),
}

/// 行形态归成五类，整体判决只看类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// 不挡提交。
    Clear,
    /// 等人点「更新到最新」。
    NeedsClick,
    /// 在排或在跑，等它落定。
    InProgress,
    /// 更新不了：先处理它，或把它的元素移出本次提资。
    Stuck,
    Unknown,
}

impl Class {
    /// 行表里的先后：要人处理的在前。
    fn rank(self) -> u8 {
        match self {
            Self::Stuck => 0,
            Self::Unknown => 1,
            Self::NeedsClick => 2,
            Self::InProgress => 3,
            Self::Clear => 4,
        }
    }
}

impl LibForm {
    pub fn class(&self) -> Class {
        match self {
            Self::UpToDate | Self::ScopeCaughtUp { .. } => Class::Clear,
            Self::Lagging { .. } | Self::NeedsInit => Class::NeedsClick,
            Self::ModelCatchingUp | Self::Queued { .. } | Self::Running { .. } => Class::InProgress,
            Self::Blocked { .. }
            | Self::Unreachable
            | Self::OutOfScope(_)
            | Self::Parked(_)
            | Self::RowsMissing
            | Self::Refused(_)
            | Self::ModelStale => Class::Stuck,
            Self::Unknown(_) => Class::Unknown,
        }
    }

    /// 圆点与状态词的颜色。元素列表的圆点与它同色。
    pub fn tone(&self) -> Status {
        match self {
            Self::Unreachable => Status::Neutral,
            other => match other.class() {
                Class::Clear => Status::Success,
                Class::NeedsClick | Class::Unknown => Status::Warn,
                Class::InProgress => Status::Info,
                Class::Stuck => Status::Error,
            },
        }
    }

    /// 状态列的那一句。
    pub fn state_text(&self) -> String {
        match self {
            Self::UpToDate => "已是最新".to_owned(),
            Self::ScopeCaughtUp { .. } => "提资范围已追平".to_owned(),
            Self::Lagging { saves } => format!("落后 {} 次保存", group_number(*saves)),
            Self::NeedsInit => "需初始化".to_owned(),
            Self::ModelCatchingUp => "模型追赶中".to_owned(),
            Self::Queued { first_import, .. } => format!("{} · 排队中", kind_text(*first_import)),
            Self::Running {
                first_import: true, ..
            } => "首次导入 · 运行中".to_owned(),
            Self::Running {
                stage: RunStage::Applying,
                ..
            } => "增量更新 · 应用中".to_owned(),
            Self::Running { .. } => "增量更新 · 模型重算中".to_owned(),
            Self::Blocked { label, .. } => format!("已阻断 · {label}"),
            Self::Unreachable => "够不着".to_owned(),
            Self::OutOfScope(_) => "不在本期执行范围".to_owned(),
            Self::Parked(_) => "上次更新失败".to_owned(),
            Self::RowsMissing => "已是最新却缺行".to_owned(),
            Self::Refused(_) => "此刻排不了".to_owned(),
            Self::ModelStale => "模型没追平".to_owned(),
            Self::Unknown(_) => "没查成".to_owned(),
        }
    }

    /// 第二行左边：这一形态是怎么回事。
    pub fn detail(&self) -> String {
        match self {
            Self::UpToDate => "数据与模型都追平文件最新保存".to_owned(),
            Self::ScopeCaughtUp { .. } => {
                "提资元素所在的交付单元已重新生成（含房间归属）".to_owned()
            }
            Self::Lagging { .. } => "数据水位落后于文件".to_owned(),
            Self::NeedsInit => "尚无数据水位；更新时一并首次导入，不用另去初始化".to_owned(),
            Self::ModelCatchingUp => "数据已应用，模型还在算".to_owned(),
            Self::Queued {
                first_import: true, ..
            }
            | Self::Running {
                first_import: true, ..
            } => "整库解析保存并生成模型，做完才能提交".to_owned(),
            Self::Queued { .. } => "等前面的任务跑完".to_owned(),
            Self::Running { .. } => "这个库的任务跑完才能提交".to_owned(),
            Self::Blocked { detail, .. } => first_line(detail),
            Self::Unreachable => "当前 MDB 声明了它，当前项目目录里没有它的文件".to_owned(),
            Self::OutOfScope(reason) | Self::Refused(reason) => first_line(reason),
            Self::Parked(reason) if reason.trim().is_empty() => {
                "连续失败已停，这里不替它重试".to_owned()
            }
            Self::Parked(reason) => {
                format!("连续失败已停，这里不替它重试：{}", first_line(reason))
            }
            Self::RowsMissing => "数据水位已是文件最新，库里却缺这些元素的行".to_owned(),
            Self::ModelStale => "数据已是最新，模型没追平，也没有任务在跑".to_owned(),
            Self::Unknown(_) => "不当成最新，也不当成落后".to_owned(),
        }
    }

    /// 第二行右边：人接下来该做什么，或者等到什么时候。
    pub fn way_out(&self) -> String {
        match self {
            Self::UpToDate | Self::ScopeCaughtUp { .. } => "不挡提交".to_owned(),
            Self::Lagging { .. } | Self::NeedsInit => "等人点「更新到最新」".to_owned(),
            Self::ModelCatchingUp => "不用点，算完自动重新检查".to_owned(),
            Self::Queued {
                position, waited, ..
            } => {
                let mut parts = Vec::new();
                if let Some(position) = position {
                    parts.push(format!("队列第 {position} 位"));
                }
                if let Some(waited) = waited {
                    parts.push(format!("已排 {}", clock(*waited)));
                }
                if parts.is_empty() {
                    "已排上".to_owned()
                } else {
                    parts.join(" · ")
                }
            }
            Self::Running { elapsed, .. } => match elapsed {
                Some(elapsed) => format!("已用 {}", clock(*elapsed)),
                None => "正在执行".to_owned(),
            },
            Self::Blocked { .. } => "处理文件，或移出它的元素".to_owned(),
            Self::Unreachable | Self::OutOfScope(_) => "移出它的元素".to_owned(),
            Self::Parked(_) => "去任务队列看原因并重试".to_owned(),
            Self::RowsMissing => "要对这个库重新解析".to_owned(),
            Self::Refused(_) => "按这句处理后重新检查".to_owned(),
            Self::ModelStale => "在模型树上对它「重新生成模型」".to_owned(),
            Self::Unknown(_) => "重新检查".to_owned(),
        }
    }

    /// 悬停时给的原文（阻断原因、失败原因、服务端的话、查询错误）。没有可补的就 `None`。
    pub fn hover(&self) -> Option<&str> {
        match self {
            Self::Blocked { detail, .. } => Some(detail.as_str()),
            Self::Parked(reason)
            | Self::Unknown(reason)
            | Self::OutOfScope(reason)
            | Self::Refused(reason)
                if !reason.trim().is_empty() =>
            {
                Some(reason.as_str())
            }
            _ => None,
        }
    }
}

fn kind_text(first_import: bool) -> &'static str {
    if first_import {
        "首次导入"
    } else {
        "增量更新"
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_owned()
}

/// 检查面板上的一行：一个库。
#[derive(Debug, Clone, PartialEq)]
pub struct LibRow {
    pub dbnum: u32,
    /// 库类型（`/dbnums` 行上的）。取不到就不画那枚标签。
    pub db_type: Option<String>,
    /// 库文件名（`/dbnums` 行上的），悬停时给。
    pub file_name: Option<String>,
    /// 这个库里的提资元素。
    pub elements: Vec<RefU64>,
    pub file_latest_time: Option<String>,
    pub applied_time: Option<String>,
    pub initialized: bool,
    /// 数据水位正好停在文件最新那次保存上。
    pub data_caught_up: bool,
    pub form: LibForm,
}

impl LibRow {
    /// 「文件最新保存」列。时刻读不出就留空，不回落成序号（ADR-0019）。够不着的库在当前项目目录里
    /// 没有文件，这一格没有可说的。
    pub fn file_cell(&self) -> String {
        if self.form == LibForm::Unreachable {
            return "—".to_owned();
        }
        self.file_latest_time
            .as_deref()
            .map(e3d_time)
            .unwrap_or_else(|| "—".to_owned())
    }

    /// 「数据水位」列。服务端不给这一枚时刻时，只有两句说得出来：尚无数据水位，或已追平
    /// （那就是文件最新那一刻）；落后时停在哪一次保存说不出，留空。
    pub fn mark_cell(&self) -> String {
        if self.form == LibForm::Unreachable {
            return "—".to_owned();
        }
        if !self.initialized {
            return "尚无数据水位".to_owned();
        }
        match self.applied_time.as_deref() {
            Some(at) => e3d_time(at),
            None if self.data_caught_up => self.file_cell(),
            None => "—".to_owned(),
        }
    }

    /// 数据水位那一格要不要提醒色：还没建立，或落后于文件。更新不了的行不用它催人——催也没用，
    /// 状态列已经说了为什么。
    pub fn mark_behind(&self) -> bool {
        self.form.class() != Class::Stuck
            && (!self.initialized || matches!(self.form, LibForm::Lagging { .. }))
    }

    /// 第二行左边：几个提资元素，外加这一形态是怎么回事。
    pub fn detail_line(&self) -> String {
        format!(
            "{} 个提资元素 · {}",
            self.elements.len(),
            self.form.detail()
        )
    }
}

// ================================================================ 整体判决

/// 右栏顶部的整体判决（画板 S15-C 左栏）。「正在检查」与「不支持」是请求的状态，不在这里。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ready,
    NeedsUpdate { libs: usize },
    Updating,
    Stuck,
    Unknown,
}

/// 一次判定的全部产出。
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub rows: Vec<LibRow>,
    /// 文件最新保存里已经没有的元素。
    pub gone: Vec<RefU64>,
    /// 没查成的元素与原因。它们属于哪个库都不知道，所以不进行表。
    pub failed: Vec<(RefU64, String)>,
    pub verdict: Verdict,
    /// 两份执行名单，按库号升序。先发落后的，回执到了再发需初始化的：队列先进先出，增量更新
    /// 就排在首次导入前面。
    pub lagging: Vec<u32>,
    pub needs_init: Vec<u32>,
    /// 此刻为什么点不了「更新到最新」；`None` = 可以点。
    pub update_blocked: Option<String>,
    /// 房间泳道里还有多少待重算。只提醒、不拦：它跨库，饥饿态下会长期不空。
    pub rooms_pending: Option<usize>,
    /// 提资元素之外还有几个库落后或需初始化。它们不在这次更新里。
    pub others_behind: usize,
}

impl Report {
    pub fn allows_submit(&self) -> bool {
        self.verdict == Verdict::Ready
    }

    /// 某个元素在列表里的圆点颜色：跟它所在库的行同色；已删除的是红，没查成的是黄。
    pub fn element_tone(&self, refno: RefU64) -> Option<Status> {
        if self.gone.contains(&refno) {
            return Some(Status::Error);
        }
        if self.failed.iter().any(|(failed, _)| *failed == refno) {
            return Some(Status::Warn);
        }
        self.row_of(refno).map(|row| row.form.tone())
    }

    /// 某个元素所在库的那一行。没查到就没有。
    pub fn row_of(&self, refno: RefU64) -> Option<&LibRow> {
        self.rows.iter().find(|row| row.elements.contains(&refno))
    }

    /// 某个元素为什么是这个颜色，悬停时说。
    pub fn element_note(&self, refno: RefU64) -> Option<String> {
        if self.gone.contains(&refno) {
            return Some("文件最新保存里已经没有这个元素，请把它移出本次提资".to_owned());
        }
        if let Some((_, reason)) = self.failed.iter().find(|(failed, _)| *failed == refno) {
            return Some(format!("没查成：{reason}"));
        }
        self.row_of(refno)
            .map(|row| format!("db{} · {}", row.dbnum, row.form.state_text()))
    }

    /// 更新不了的元素：所在库卡住的，加上文件里已删除的。「移出这些元素」移的就是它们。
    pub fn stuck_elements(&self) -> Vec<RefU64> {
        let mut out = self.gone.clone();
        for row in &self.rows {
            if row.form.class() == Class::Stuck {
                out.extend(row.elements.iter().copied());
            }
        }
        out
    }

    /// 横幅：颜色、标题、一句说明。
    pub fn banner(&self) -> (Status, String, String) {
        let count = |class: Class| {
            self.rows
                .iter()
                .filter(|row| row.form.class() == class)
                .count()
        };
        match self.verdict {
            Verdict::Ready => {
                let background = self
                    .rows
                    .iter()
                    .any(|row| matches!(row.form, LibForm::ScopeCaughtUp { .. }));
                let detail = if background {
                    "提资元素所在的交付单元都已追平文件最新保存；其余单元还在后台生成，不影响这次提资。提交时会再检查一次。"
                } else {
                    "提资元素所在的库，数据与模型都已追平文件最新保存。提交时会再检查一次。"
                };
                (
                    Status::Success,
                    "要提资的数据已是最新".to_owned(),
                    detail.to_owned(),
                )
            }
            Verdict::NeedsUpdate { .. } => {
                let mut parts = Vec::new();
                if !self.lagging.is_empty() {
                    parts.push(format!("{} 个库的数据水位落后于文件", self.lagging.len()));
                }
                if !self.needs_init.is_empty() {
                    parts.push(format!("{} 个库尚未初始化", self.needs_init.len()));
                }
                let in_progress = count(Class::InProgress);
                if in_progress > 0 {
                    parts.push(format!("{in_progress} 个库已在更新"));
                }
                (
                    Status::Warn,
                    "要提资的数据不是最新".to_owned(),
                    format!(
                        "{}。点「更新到最新」一并处理，追平之前不能提交。",
                        parts.join("，")
                    ),
                )
            }
            Verdict::Updating => (
                Status::Info,
                "正在把要提资的数据更新到最新".to_owned(),
                "追平后自动重新检查，届时可以提交。关闭这扇窗不会中断，进度也在任务队列里。"
                    .to_owned(),
            ),
            Verdict::Stuck => {
                let mut parts = Vec::new();
                let stuck = count(Class::Stuck);
                if stuck > 0 {
                    parts.push(format!("{stuck} 个库更新不了"));
                }
                if !self.gone.is_empty() {
                    parts.push(format!(
                        "{} 个元素在文件最新保存里已经没有了",
                        self.gone.len()
                    ));
                }
                (
                    Status::Error,
                    "有库更新不了".to_owned(),
                    format!(
                        "{}。先处理它，或把这些元素移出本次提资；不提供绕过。",
                        parts.join("，")
                    ),
                )
            }
            Verdict::Unknown => (
                Status::Warn,
                "没查成".to_owned(),
                "模型服务连不上或查询失败。不当成最新，也不当成落后；重新检查之前不能提交。"
                    .to_owned(),
            ),
        }
    }
}

/// 把一次检查的元素事实与队列读数判成行表与整体判决。队列每换代一次就重算一遍，不重发请求。
///
/// `claimed` 是本窗刚排上、队列快照里还没出现的库：按「已排上」说，免得回执与下一拍轮询之间
/// 它退回「落后」、按钮又亮一下。
pub fn judge(elements: &[ElementCheck], queue: &task_queue::Vm, claimed: &BTreeSet<u32>) -> Report {
    let mut by_lib: BTreeMap<u32, Vec<(RefU64, &NodeModelStatus)>> = BTreeMap::new();
    let mut gone = Vec::new();
    let mut failed = Vec::new();
    for element in elements {
        match &element.fact {
            ElementFact::Checked(status) => by_lib
                .entry(status.dbnum)
                .or_default()
                .push((element.refno, &**status)),
            ElementFact::Gone => gone.push(element.refno),
            ElementFact::Failed(reason) => failed.push((element.refno, reason.clone())),
        }
    }

    let active = active_rows(queue);
    let mut rows: Vec<LibRow> = by_lib
        .into_iter()
        .map(|(dbnum, members)| {
            let statuses: Vec<&NodeModelStatus> = members.iter().map(|(_, s)| *s).collect();
            let registry = queue.dbnums.iter().find(|db| db.dbnum == dbnum);
            let library = &statuses[0].library;
            let waiting = Waiting {
                active: active.get(&dbnum),
                claimed: claimed.contains(&dbnum),
            };
            LibRow {
                dbnum,
                db_type: registry
                    .map(|db| db.db_type.trim().to_owned())
                    .filter(|db_type| !db_type.is_empty()),
                file_name: registry
                    .map(|db| db.file_name.trim().to_owned())
                    .filter(|name| !name.is_empty()),
                elements: members.iter().map(|(refno, _)| *refno).collect(),
                file_latest_time: library.file_latest_time.clone(),
                applied_time: library.applied_sesno_time.clone(),
                initialized: library.initialized,
                data_caught_up: library.initialized
                    && library.applied_sesno == library.file_latest_sesno,
                form: judge_lib(&statuses, registry, waiting),
            }
        })
        .collect();

    // 要人处理的排在前面，不挡提交的沉底；同一类里按库号。
    rows.sort_by_key(|row| (row.form.class().rank(), row.dbnum));

    let has = |class: Class| rows.iter().any(|row| row.form.class() == class);
    let verdict = if has(Class::Stuck) || !gone.is_empty() {
        Verdict::Stuck
    } else if has(Class::Unknown) || !failed.is_empty() {
        Verdict::Unknown
    } else if has(Class::NeedsClick) {
        Verdict::NeedsUpdate {
            libs: rows
                .iter()
                .filter(|row| row.form.class() == Class::NeedsClick)
                .count(),
        }
    } else if has(Class::InProgress) {
        Verdict::Updating
    } else {
        Verdict::Ready
    };
    let pick = |want: fn(&LibForm) -> bool| -> Vec<u32> {
        rows.iter()
            .filter(|row| want(&row.form))
            .map(|row| row.dbnum)
            .collect()
    };
    let mine: BTreeSet<u32> = rows.iter().map(|row| row.dbnum).collect();
    Report {
        lagging: pick(|form| matches!(form, LibForm::Lagging { .. })),
        needs_init: pick(|form| matches!(form, LibForm::NeedsInit)),
        update_blocked: update_blocked(queue),
        rooms_pending: queue.rooms_pending().filter(|pending| *pending > 0),
        others_behind: queue
            .dbnums
            .iter()
            .filter(|db| !mine.contains(&db.dbnum) && behind_and_runnable(db))
            .count(),
        rows,
        gone,
        failed,
        verdict,
    }
}

/// 一个范围内、跑得动的设计库此刻落后或需初始化（与取回工作旁那行提示同一个范围口径）。
fn behind_and_runnable(db: &DbnumStatus) -> bool {
    db.db_type.trim().eq_ignore_ascii_case("DESI")
        && !db.excluded
        && !db.blocked
        && !db.not_in_project
        && (!db.initialized || db.file_latest_sesno > db.applied_sesno)
}

/// 「更新到最新」此刻为什么会白点。队列暂停时不替人恢复：恢复会放行队列里所有库。
fn update_blocked(queue: &task_queue::Vm) -> Option<String> {
    if !queue.can_mutate() {
        return Some("还没连上数据源，或当前项目与模型服务的身份对不上：不能发起更新".to_owned());
    }
    if let Some(reason) = queue.execution_blocked_reason() {
        return Some(reason.to_owned());
    }
    queue
        .paused()
        .then(|| "队列已暂停：先到任务队列恢复出队".to_owned())
}

/// 一个库此刻在队列里的活。
struct ActiveRow {
    running: Option<RunStage>,
    position: Option<usize>,
    units: Option<(u32, u32)>,
    data: Option<DataProgress>,
    timer: Option<Duration>,
}

#[derive(Clone, Copy)]
struct Waiting<'a> {
    active: Option<&'a ActiveRow>,
    claimed: bool,
}

/// 每个库在队列快照里的活动行。同一个库至多两行（一行运行中、一行排队中），说运行中的那一行。
/// 认不出状态的活动行按排着算：不知道它开没开始，就当它还占着队列。
fn active_rows(queue: &task_queue::Vm) -> BTreeMap<u32, ActiveRow> {
    let live: HashSet<&str> = queue
        .queue
        .rows
        .iter()
        .map(|row| row.task_id.as_str())
        .collect();
    let mut out: BTreeMap<u32, ActiveRow> = BTreeMap::new();
    for row in task_queue::rows(queue) {
        if !live.contains(row.task_id.as_str()) {
            continue;
        }
        let running = match row.phase {
            Phase::Applying => Some(RunStage::Applying),
            Phase::Generating => Some(RunStage::Generating),
            _ => None,
        };
        if out
            .get(&row.dbnum)
            .is_some_and(|kept| kept.running.is_some() || running.is_none())
        {
            continue;
        }
        let data = queue
            .tasks
            .iter()
            .find(|task| task.task_id == row.task_id)
            .and_then(|task| task.data_progress);
        let timer = match row.timer {
            Timer::Queued(elapsed) | Timer::Running(elapsed) => elapsed,
            Timer::Finished(_) => None,
        };
        out.insert(
            row.dbnum,
            ActiveRow {
                running,
                position: row.position,
                units: row.units_done.zip(row.total_units),
                data,
                timer,
            },
        );
    }
    out
}

/// 某些库此刻的状态签名。窗口拿它判「手上的事实旧了」：别处排的任务开始或结束，或者文件又来了新保存、
/// 数据水位动了——变了就重新检查。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct QueueSignature {
    /// 队列里这几个库的活：库号, 是否在跑。
    pub active: Vec<(u32, bool)>,
    /// 库登记表上这几个库的两枚水位：库号, 文件最新保存, 数据水位。检查回来的事实是问的那一刻的；
    /// 登记表跟着队列一起轮询，它动了就是该重问了——开着窗等的时候文件又存了一次，不必等人点
    /// 「提交」才发现。
    pub marks: Vec<(u32, i32, i32)>,
}

impl QueueSignature {
    /// 上一份签名里还在队列里的库，这一份里不在了：它的活落定了，落定之前问来的事实整份作废。
    pub fn settled_since(&self, earlier: &Self) -> bool {
        earlier
            .active
            .iter()
            .any(|(dbnum, _)| self.active.iter().all(|(now, _)| now != dbnum))
    }
}

pub fn queue_signature(queue: &task_queue::Vm, dbnums: &BTreeSet<u32>) -> QueueSignature {
    let mut marks: Vec<(u32, i32, i32)> = queue
        .dbnums
        .iter()
        .filter(|db| dbnums.contains(&db.dbnum))
        .map(|db| (db.dbnum, db.file_latest_sesno, db.applied_sesno))
        .collect();
    // 登记表的行序由服务端定，换了序不算变。
    marks.sort_unstable();
    QueueSignature {
        active: active_rows(queue)
            .into_iter()
            .filter(|(dbnum, _)| dbnums.contains(dbnum))
            .map(|(dbnum, row)| (dbnum, row.running.is_some()))
            .collect(),
        marks,
    }
}

fn anomaly_label(anomaly: &FileAnomaly) -> &'static str {
    match anomaly {
        FileAnomaly::Rollback { .. } => "文件回退",
        FileAnomaly::PathMigrated { .. } => "路径迁移",
        FileAnomaly::TypeChanged { .. } => "类型变化",
        FileAnomaly::Duplicate { .. } => "同号重复",
        FileAnomaly::Missing { .. } => "文件缺失",
    }
}

fn judge_lib(
    statuses: &[&NodeModelStatus],
    registry: Option<&DbnumStatus>,
    waiting: Waiting<'_>,
) -> LibForm {
    let library = &statuses[0].library;
    let ingests: Vec<&NodeIngestLib> = statuses
        .iter()
        .filter_map(|status| status.ingest())
        .collect();
    let refused = |reasons: &[&str]| {
        ingests.iter().copied().find(|lib| {
            lib.action == "not_enqueued"
                && lib
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reasons.contains(&reason))
        })
    };

    // 一、范围。够不着与排除是两回事，分开说（CONTEXT.md）。
    if registry.is_some_and(|db| db.not_in_project) {
        return LibForm::Unreachable;
    }
    if registry.is_some_and(|db| db.excluded) {
        return LibForm::OutOfScope("不在当前 MDB 的本期执行范围里".to_owned());
    }
    if !library.in_watch_scope {
        return LibForm::OutOfScope("不在模型服务的监听范围里，更新请求不会排它".to_owned());
    }
    if let Some(lib) = refused(&["out_of_scope", "scope_unresolved"]) {
        return LibForm::OutOfScope(lib.message.clone());
    }

    // 二、阻断。文件回退不管服务端标没标阻断都算：人工执行遇到它会清库整库重建（gen-model ADR-021），
    // 那不是「更新到最新」该替人做的事。
    if let Some(db) = registry {
        let rolled_back = matches!(db.anomaly, Some(FileAnomaly::Rollback { .. }));
        if db.blocked || rolled_back {
            return match &db.anomaly {
                Some(anomaly) => LibForm::Blocked {
                    label: anomaly_label(anomaly).to_owned(),
                    detail: anomaly.to_string(),
                },
                None => LibForm::Blocked {
                    label: "文件异常".to_owned(),
                    detail: "这个库被阻断，原因没有随状态给出；到「模型更新」的库状态页看"
                        .to_owned(),
                },
            };
        }
    }

    // 三、已停自动重跑。
    if let Some(failure) = ingests
        .iter()
        .filter_map(|lib| lib.last_failure.as_ref())
        .find(|failure| failure.parked)
    {
        return LibForm::Parked(failure.reason.clone());
    }
    if let Some(lib) = refused(&["parked_after_failures"]) {
        return LibForm::Parked(lib.message.clone());
    }

    let caught_up = library.initialized && library.applied_sesno >= library.file_latest_sesno;

    // 四、按单元放行（服务端按根答了就绪才走得到）。首次导入没有提前放行：它还没有数据水位。
    if caught_up
        && statuses
            .iter()
            .all(|status| status.publish_ready == Some(true))
    {
        return match waiting.active {
            Some(row) => LibForm::ScopeCaughtUp { rest: row.units },
            None => LibForm::UpToDate,
        };
    }

    // 五、队列里已有它的活。
    let first_import = !library.initialized;
    if let Some(row) = waiting.active {
        return match row.running {
            Some(stage) => LibForm::Running {
                first_import,
                stage,
                units: row.units,
                data: row.data,
                elapsed: row.timer,
            },
            None => LibForm::Queued {
                first_import,
                position: row.position,
                waited: row.timer,
            },
        };
    }
    // 队列快照还没跟上：服务端已经说这个库有数据批次在跑，或本窗刚把它排上。
    if library.data_batch_running {
        return LibForm::Running {
            first_import,
            stage: RunStage::Applying,
            units: None,
            data: None,
            elapsed: None,
        };
    }
    if waiting.claimed && !caught_up {
        return LibForm::Queued {
            first_import,
            position: None,
            waited: None,
        };
    }

    // 六、服务端说这个库缺数据却排不了。原话照登，不许诺一次排了也不会跑的更新。
    if caught_up && refused(&["up_to_date_rows_missing"]).is_some() {
        return LibForm::RowsMissing;
    }
    if !caught_up
        && let Some(lib) = ingests
            .iter()
            .copied()
            .find(|lib| lib.action == "not_enqueued" && lib.reason.is_some())
    {
        return LibForm::Refused(lib.message.clone());
    }

    // 七、需初始化；八、落后。
    if !library.initialized {
        return LibForm::NeedsInit;
    }
    if library.file_latest_sesno > library.applied_sesno {
        return LibForm::Lagging {
            saves: library.file_latest_sesno - library.applied_sesno,
        };
    }

    // 九、数据水位已是文件最新，库里却缺行。这里不自动清库重建。
    if ingests.iter().any(|lib| lib.verdict == "missing") {
        return LibForm::RowsMissing;
    }

    // 十、节点判不出。
    let verdicts: Vec<NodeVerdict> = statuses
        .iter()
        .map(|status| NodeVerdict::parse(&status.verdict))
        .collect();
    if let Some(status) = statuses
        .iter()
        .find(|status| NodeVerdict::parse(&status.verdict) == NodeVerdict::Unknown)
    {
        return LibForm::Unknown(match status.verdict_reason.as_deref() {
            Some(reason) if !reason.trim().is_empty() => {
                format!("模型服务判不出这个节点的模型状态（{}）", reason.trim())
            }
            _ => "模型服务判不出这个节点的模型状态".to_owned(),
        });
    }

    // 十一、模型。库级判决只认服务端的字面，不拿两枚水位自己比。
    let lib_lagging = library.model_verdict.as_deref().map(str::trim) == Some("lagging");
    let chasing = lib_lagging || library.model_task_running;
    if verdicts.iter().all(|verdict| verdict.settled()) && !chasing {
        return LibForm::UpToDate;
    }
    if chasing || verdicts.contains(&NodeVerdict::Updating) {
        return LibForm::ModelCatchingUp;
    }
    LibForm::ModelStale
}

// ================================================================ 数与时间

/// RFC3339 → 「MM-DD HH:MM」（本地时区），与 `task_queue` 里那一份同一口径：语义是 E3D 会话写入时刻，
/// 解不动就原样返回。
pub(crate) fn e3d_time(rfc3339: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| rfc3339.to_owned())
}

pub(crate) fn clock(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    if s < 3600 {
        format!("{:02}:{:02}", s / 60, s % 60)
    } else {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_queue::{QueueRow, TaskEntry};

    fn refno(n: u64) -> RefU64 {
        RefU64::from(n)
    }

    fn status(dbnum: u32, verdict: &str, applied: i64, latest: i64) -> NodeModelStatus {
        NodeModelStatus {
            dbnum,
            verdict: verdict.to_owned(),
            library: NodeLibrary {
                file_latest_sesno: latest,
                applied_sesno: applied,
                initialized: applied > 0,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn checked(n: u64, status: NodeModelStatus) -> ElementCheck {
        ElementCheck::checked(refno(n), status)
    }

    fn ingest(dbnum: u32, verdict: &str, action: &str, reason: Option<&str>) -> NodeIngestBlock {
        NodeIngestBlock {
            enabled: true,
            libraries: vec![NodeIngestLib {
                dbnum,
                verdict: verdict.into(),
                action: action.into(),
                reason: reason.map(str::to_owned),
                message: "服务端的那一句".into(),
                last_failure: None,
            }],
        }
    }

    /// 身份齐全、没有 health 的队列：可执行，没暂停。
    fn queue() -> task_queue::Vm {
        let mut queue = task_queue::Vm::new(Default::default());
        queue.project = "P".into();
        queue.mdb = "/ALL".into();
        queue.namespace = "ns".into();
        queue
    }

    fn registry(dbnum: u32) -> DbnumStatus {
        DbnumStatus {
            dbnum,
            db_type: "DESI".into(),
            file_name: format!("des{dbnum}"),
            ..Default::default()
        }
    }

    fn queue_row(task_id: &str, dbnum: u32, state: &str) -> QueueRow {
        QueueRow {
            task_id: task_id.into(),
            dbnum,
            db_type: "DESI".into(),
            state: state.into(),
            start_sesno: 1,
            end_sesno: 2,
        }
    }

    fn none() -> BTreeSet<u32> {
        BTreeSet::new()
    }

    fn form(elements: &[ElementCheck], queue: &task_queue::Vm) -> LibForm {
        let report = judge(elements, queue, &none());
        assert_eq!(report.rows.len(), 1, "{report:?}");
        report.rows[0].form.clone()
    }

    #[test]
    fn caught_up_library_with_settled_nodes_clears_the_gate() {
        let elements = [
            checked(1, status(8000, "latest", 92, 92)),
            checked(2, status(8000, "empty", 92, 92)),
        ];
        let report = judge(&elements, &queue(), &none());
        assert_eq!(report.rows[0].form, LibForm::UpToDate);
        assert_eq!(report.verdict, Verdict::Ready);
        assert!(report.allows_submit());
        assert_eq!(report.rows[0].elements, vec![refno(1), refno(2)]);
        assert_eq!(
            report.rows[0].detail_line(),
            "2 个提资元素 · 数据与模型都追平文件最新保存"
        );
    }

    #[test]
    fn lagging_and_uninitialised_libraries_wait_for_one_click_in_two_lists() {
        let elements = [
            checked(1, status(8021, "not_generated", 0, 12)),
            checked(2, status(8000, "latest", 89, 92)),
            checked(3, status(7999, "latest", 40, 40)),
        ];
        let report = judge(&elements, &queue(), &none());
        assert_eq!(report.verdict, Verdict::NeedsUpdate { libs: 2 });
        assert!(!report.allows_submit());
        // 增量在前、首次导入在后：两份名单分开，宿主按这个顺序发两次执行请求。
        assert_eq!(report.lagging, vec![8000]);
        assert_eq!(report.needs_init, vec![8021]);
        assert_eq!(report.update_blocked, None);
        let lagging = report.rows.iter().find(|row| row.dbnum == 8000).unwrap();
        assert_eq!(lagging.form, LibForm::Lagging { saves: 3 });
        assert_eq!(lagging.form.state_text(), "落后 3 次保存");
        // 落后时数据水位停在哪一次保存，今天的服务端说不出：留空，不拿别的时刻顶。
        assert_eq!(lagging.mark_cell(), "—");
        assert!(lagging.mark_behind());
        let init = report.rows.iter().find(|row| row.dbnum == 8021).unwrap();
        assert_eq!(init.form, LibForm::NeedsInit);
        assert_eq!(init.mark_cell(), "尚无数据水位");
    }

    /// 行表把要人处理的排在前面：更新不了的、等人点的、在排在跑的、不挡提交的；同一类里按库号。
    #[test]
    fn rows_that_need_attention_come_first() {
        let mut q = queue();
        let mut unreachable = registry(9500);
        unreachable.not_in_project = true;
        q.dbnums = vec![unreachable];
        q.queue.rows = vec![queue_row("t-run", 7000, "running")];
        let elements = [
            checked(1, status(7999, "latest", 40, 40)),
            checked(2, status(8021, "not_generated", 0, 12)),
            checked(3, status(7000, "updating", 5, 6)),
            checked(4, status(8000, "latest", 89, 92)),
            checked(5, status(9500, "not_generated", 0, 5)),
        ];
        let order: Vec<u32> = judge(&elements, &q, &none())
            .rows
            .iter()
            .map(|row| row.dbnum)
            .collect();
        assert_eq!(order, vec![9500, 8000, 8021, 7000, 7999]);
    }

    #[test]
    fn the_watermark_cell_borrows_the_file_time_only_when_caught_up() {
        let mut caught = status(7999, "latest", 40, 40);
        caught.library.file_latest_time = Some("2026-11-24T09:15:00+08:00".into());
        let report = judge(&[checked(1, caught.clone())], &queue(), &none());
        assert_eq!(report.rows[0].mark_cell(), report.rows[0].file_cell());
        assert_ne!(report.rows[0].file_cell(), "—");

        // 服务端给了数据水位的时刻就用它的。
        let mut lagging = status(8000, "latest", 89, 92);
        lagging.library.file_latest_time = Some("2026-11-26T10:42:00+08:00".into());
        lagging.library.applied_sesno_time = Some("2026-11-25T17:30:00+08:00".into());
        let report = judge(&[checked(1, lagging)], &queue(), &none());
        assert_eq!(
            report.rows[0].mark_cell(),
            e3d_time("2026-11-25T17:30:00+08:00")
        );
    }

    #[test]
    fn blocked_unreachable_and_excluded_libraries_never_enter_the_execute_lists() {
        let mut q = queue();
        let mut blocked = registry(8003);
        blocked.blocked = true;
        blocked.anomaly = Some(FileAnomaly::Missing { path: "a".into() });
        let mut unreachable = registry(7015);
        unreachable.not_in_project = true;
        let mut excluded = registry(9000);
        excluded.excluded = true;
        q.dbnums = vec![blocked, unreachable, excluded];
        let elements = [
            checked(1, status(8003, "lagging", 80, 82)),
            checked(2, status(7015, "not_generated", 0, 5)),
            checked(3, status(9000, "latest", 3, 9)),
        ];
        let report = judge(&elements, &q, &none());
        assert_eq!(report.verdict, Verdict::Stuck);
        assert!(report.lagging.is_empty() && report.needs_init.is_empty());
        let classes: Vec<Class> = report.rows.iter().map(|row| row.form.class()).collect();
        assert_eq!(classes, vec![Class::Stuck; 3]);
        assert_eq!(
            report
                .rows
                .iter()
                .find(|row| row.dbnum == 7015)
                .unwrap()
                .form,
            LibForm::Unreachable
        );
        assert_eq!(report.stuck_elements().len(), 3);
        // 够不着的库两列都没有可说的；更新不了的行不用提醒色催人。
        let unreachable = report.rows.iter().find(|row| row.dbnum == 7015).unwrap();
        assert_eq!(
            (unreachable.file_cell(), unreachable.mark_cell()),
            ("—".to_owned(), "—".to_owned())
        );
        assert!(report.rows.iter().all(|row| !row.mark_behind()));
    }

    /// 文件回退在新服务端上不标阻断（人工执行会清库整库重建）。提资窗不替人做这件事。
    #[test]
    fn a_rolled_back_file_is_stuck_even_when_the_server_does_not_block_it() {
        let mut q = queue();
        let mut rolled = registry(8003);
        rolled.anomaly = Some(FileAnomaly::Rollback {
            file_latest_sesno: 812,
            applied_sesno: 1005,
            file_latest_sesno_time: None,
            applied_sesno_time: None,
        });
        q.dbnums = vec![rolled];
        let report = judge(
            &[checked(1, status(8003, "latest", 1005, 812))],
            &q,
            &none(),
        );
        let found = &report.rows[0].form;
        assert!(
            matches!(found, LibForm::Blocked { label, .. } if label == "文件回退"),
            "{found:?}"
        );
        assert_eq!(found.state_text(), "已阻断 · 文件回退");
        assert!(report.lagging.is_empty() && report.needs_init.is_empty());
        // 数据水位比文件新：不算追平，数据水位那一格不借文件的时刻。
        assert!(!report.rows[0].data_caught_up);
    }

    #[test]
    fn a_parked_library_is_not_retried_from_here() {
        let mut parked = status(8004, "latest", 70, 74);
        let mut block = ingest(8004, "present", "none", None);
        block.libraries[0].last_failure = Some(NodeIngestFailure {
            reason: "文件读不开".into(),
            parked: true,
        });
        parked.data_ingest = Some(block);
        let report = judge(&[checked(1, parked)], &queue(), &none());
        assert_eq!(report.rows[0].form, LibForm::Parked("文件读不开".into()));
        assert!(report.lagging.is_empty());
        assert_eq!(report.verdict, Verdict::Stuck);
    }

    /// 服务端说缺数据却排不了（房间参照模型没就绪、增量消费关着）：不许诺一次排了也不会跑的更新。
    #[test]
    fn a_refusal_from_the_server_is_shown_verbatim_and_is_not_executed() {
        let mut refused = status(8007, "not_generated", 0, 9);
        refused.data_ingest = Some(ingest(
            8007,
            "missing",
            "not_enqueued",
            Some("data_incremental_off"),
        ));
        let report = judge(&[checked(1, refused)], &queue(), &none());
        assert_eq!(
            report.rows[0].form,
            LibForm::Refused("服务端的那一句".into())
        );
        assert_eq!(report.rows[0].form.detail(), "服务端的那一句");
        assert!(report.needs_init.is_empty());

        let mut out = status(8008, "not_generated", 0, 9);
        out.data_ingest = Some(ingest(
            8008,
            "missing",
            "not_enqueued",
            Some("out_of_scope"),
        ));
        assert!(matches!(
            form(&[checked(1, out)], &queue()),
            LibForm::OutOfScope(_)
        ));
    }

    #[test]
    fn queued_and_running_tasks_show_progress_and_keep_the_gate_shut() {
        let mut q = queue();
        q.queue.rows = vec![
            queue_row("t-run", 8000, "running"),
            queue_row("t-wait", 8021, "queued"),
        ];
        q.tasks = vec![TaskEntry {
            task_id: "t-run".into(),
            kind: task_queue::KIND_DATA_BATCH.into(),
            state: "running".into(),
            dbnum: Some(8000),
            units_done: Some(14),
            total_units: Some(24),
            ..Default::default()
        }];
        let elements = [
            checked(1, status(8000, "updating", 92, 92)),
            checked(2, status(8021, "not_generated", 0, 12)),
        ];
        let report = judge(&elements, &q, &none());
        assert_eq!(report.verdict, Verdict::Updating);
        assert!(!report.allows_submit());
        let running = &report
            .rows
            .iter()
            .find(|row| row.dbnum == 8000)
            .unwrap()
            .form;
        assert!(
            matches!(
                running,
                LibForm::Running {
                    first_import: false,
                    stage: RunStage::Generating,
                    units: Some((14, 24)),
                    ..
                }
            ),
            "{running:?}"
        );
        let waiting = &report
            .rows
            .iter()
            .find(|row| row.dbnum == 8021)
            .unwrap()
            .form;
        assert!(
            matches!(
                waiting,
                LibForm::Queued {
                    first_import: true,
                    position: Some(1),
                    ..
                }
            ),
            "{waiting:?}"
        );
        assert_eq!(waiting.state_text(), "首次导入 · 排队中");
        // 已在排的库不再进名单：再点也不会重复入队。
        assert!(report.lagging.is_empty() && report.needs_init.is_empty());
        assert_eq!(
            queue_signature(&q, &BTreeSet::from([8000, 8021, 7999])).active,
            vec![(8000, true), (8021, false)]
        );
    }

    /// 库登记表上的两枚水位进签名：文件又来了新保存，签名就变；但那不是任务落定。
    #[test]
    fn the_signature_follows_the_registry_marks() {
        let mut q = queue();
        let mut lib = registry(8000);
        lib.file_latest_sesno = 92;
        lib.applied_sesno = 92;
        q.dbnums = vec![lib, registry(7999)];
        let libs = BTreeSet::from([8000]);
        let before = queue_signature(&q, &libs);
        assert_eq!(before.marks, vec![(8000, 92, 92)]);

        // 行序换了不算变。
        q.dbnums.reverse();
        assert_eq!(queue_signature(&q, &libs), before);

        q.dbnums[1].file_latest_sesno = 93;
        let after = queue_signature(&q, &libs);
        assert_ne!(after, before);
        assert!(!after.settled_since(&before), "水位动了不是任务落定");

        q.queue.rows = vec![queue_row("t-1", 8000, "queued")];
        let queued = queue_signature(&q, &libs);
        q.queue.rows.clear();
        assert!(queue_signature(&q, &libs).settled_since(&queued));
    }

    /// 解析保存的进度跟着任务行走；认不出状态的活动行按排着算。
    #[test]
    fn first_import_progress_and_unrecognised_queue_states() {
        let mut q = queue();
        q.queue.rows = vec![
            queue_row("t-init", 8021, "running"),
            queue_row("t-odd", 8000, "waiting_prerequisites"),
        ];
        q.tasks = vec![TaskEntry {
            task_id: "t-init".into(),
            kind: task_queue::KIND_DATA_BATCH.into(),
            state: "running".into(),
            dbnum: Some(8021),
            data_progress: Some(DataProgress {
                done: 123_456,
                total: Some(250_000),
            }),
            ..Default::default()
        }];
        let elements = [
            checked(1, status(8021, "not_generated", 0, 12)),
            checked(2, status(8000, "latest", 89, 92)),
        ];
        let report = judge(&elements, &q, &none());
        let importing = &report
            .rows
            .iter()
            .find(|row| row.dbnum == 8021)
            .unwrap()
            .form;
        assert!(
            matches!(
                importing,
                LibForm::Running {
                    first_import: true,
                    stage: RunStage::Applying,
                    data: Some(DataProgress {
                        done: 123_456,
                        total: Some(250_000)
                    }),
                    ..
                }
            ),
            "{importing:?}"
        );
        assert_eq!(importing.state_text(), "首次导入 · 运行中");
        let odd = &report
            .rows
            .iter()
            .find(|row| row.dbnum == 8000)
            .unwrap()
            .form;
        assert!(
            matches!(
                odd,
                LibForm::Queued {
                    first_import: false,
                    ..
                }
            ),
            "{odd:?}"
        );
    }

    /// 回执到了、队列快照还没跟上：刚排上的库按「已排上」说，不退回「落后」让按钮再亮一下。
    #[test]
    fn a_library_just_enqueued_from_here_reads_as_queued_until_the_snapshot_catches_up() {
        let elements = [checked(1, status(8000, "latest", 89, 92))];
        let report = judge(&elements, &queue(), &BTreeSet::from([8000]));
        assert!(
            matches!(report.rows[0].form, LibForm::Queued { position: None, .. }),
            "{:?}",
            report.rows[0].form
        );
        assert_eq!(report.rows[0].form.way_out(), "已排上");
        assert_eq!(report.verdict, Verdict::Updating);
        assert!(report.lagging.is_empty());
        // 已经追平的库不认这枚标记：事实比标记新。
        let caught = [checked(1, status(8000, "latest", 92, 92))];
        assert_eq!(
            judge(&caught, &queue(), &BTreeSet::from([8000])).rows[0].form,
            LibForm::UpToDate
        );
    }

    #[test]
    fn caught_up_data_with_unsettled_model_waits_or_points_at_regenerate() {
        // 库级判决还在追赶：不用点，等。
        let mut chasing = status(8005, "lagging", 60, 60);
        chasing.library.model_verdict = Some("lagging".into());
        assert_eq!(
            form(&[checked(1, chasing)], &queue()),
            LibForm::ModelCatchingUp
        );
        // 节点都已最新，但库级模型任务还在跑：同样等。
        let mut draining = status(8005, "latest", 60, 60);
        draining.library.model_task_running = true;
        assert_eq!(
            form(&[checked(1, draining)], &queue()),
            LibForm::ModelCatchingUp
        );
        // 没有任何东西在跑，节点却没生成：卡住，指向右键重新生成。
        let mut stale = status(8005, "not_generated", 60, 60);
        stale.library.model_verdict = Some("in_sync".into());
        let found = form(&[checked(1, stale)], &queue());
        assert_eq!(found, LibForm::ModelStale);
        assert_eq!(found.class(), Class::Stuck);
    }

    #[test]
    fn up_to_date_watermark_with_missing_rows_asks_for_a_reparse() {
        let mut missing = status(8006, "latest", 30, 30);
        missing.data_ingest = Some(ingest(
            8006,
            "missing",
            "not_enqueued",
            Some("up_to_date_rows_missing"),
        ));
        let report = judge(&[checked(1, missing)], &queue(), &none());
        assert_eq!(report.rows[0].form, LibForm::RowsMissing);
        assert!(report.lagging.is_empty() && report.needs_init.is_empty());
    }

    /// 没查成既不算最新也不算落后；文件里已删除的元素要人移出。
    #[test]
    fn failed_queries_are_unknown_and_deleted_elements_are_stuck() {
        let failed = ElementCheck {
            refno: refno(9),
            fact: ElementFact::Failed("timeout".into()),
        };
        let ok = checked(1, status(8000, "latest", 92, 92));
        let report = judge(&[ok.clone(), failed], &queue(), &none());
        assert_eq!(report.verdict, Verdict::Unknown);
        assert!(!report.allows_submit());
        assert_eq!(report.element_tone(refno(9)), Some(Status::Warn));
        assert_eq!(report.element_tone(refno(1)), Some(Status::Success));
        assert_eq!(report.element_tone(refno(2)), None);

        let gone = ElementCheck {
            refno: refno(7),
            fact: ElementFact::Gone,
        };
        let report = judge(&[ok, gone], &queue(), &none());
        assert_eq!(report.verdict, Verdict::Stuck);
        assert_eq!(report.stuck_elements(), vec![refno(7)]);
        assert_eq!(report.element_tone(refno(7)), Some(Status::Error));

        let unknown = status(8000, "unknown", 92, 92);
        assert!(matches!(
            form(&[checked(1, unknown)], &queue()),
            LibForm::Unknown(_)
        ));
    }

    /// 按单元档：服务端按根答了就绪，库的任务还在跑也不挡提交；不答就停在按库档。
    #[test]
    fn per_unit_readiness_releases_early_only_when_the_server_reports_it() {
        let mut q = queue();
        q.queue.rows = vec![queue_row("t-run", 8000, "running")];
        q.tasks = vec![TaskEntry {
            task_id: "t-run".into(),
            kind: task_queue::KIND_DATA_BATCH.into(),
            state: "running".into(),
            dbnum: Some(8000),
            units_done: Some(9),
            total_units: Some(21),
            ..Default::default()
        }];
        let by_library = status(8000, "updating", 92, 92);
        assert_eq!(
            form(&[checked(1, by_library.clone())], &q).class(),
            Class::InProgress
        );

        let mut ready = by_library;
        ready.publish_ready = Some(true);
        let report = judge(&[checked(1, ready.clone())], &q, &none());
        assert_eq!(
            report.rows[0].form,
            LibForm::ScopeCaughtUp {
                rest: Some((9, 21))
            }
        );
        assert_eq!(report.verdict, Verdict::Ready);

        // 首次导入没有提前放行：还没有数据水位，就绪与否都不算。
        let mut importing = status(8000, "updating", 0, 12);
        importing.publish_ready = Some(true);
        assert_eq!(
            form(&[checked(1, importing)], &q).class(),
            Class::InProgress
        );
    }

    /// 队列暂停时不替人恢复：按钮灰，理由说出来。
    #[test]
    fn a_paused_queue_blocks_the_update_button_with_a_reason() {
        let mut q = queue();
        q.queue.paused = true;
        let report = judge(&[checked(1, status(8000, "latest", 89, 92))], &q, &none());
        assert_eq!(report.verdict, Verdict::NeedsUpdate { libs: 1 });
        assert!(
            report
                .update_blocked
                .as_deref()
                .is_some_and(|reason| reason.contains("暂停"))
        );
        // 身份没就绪时说的是另一句。
        let report = judge(
            &[checked(1, status(8000, "latest", 89, 92))],
            &task_queue::Vm::default(),
            &none(),
        );
        assert!(
            report
                .update_blocked
                .as_deref()
                .is_some_and(|reason| reason.contains("数据源"))
        );
    }

    #[test]
    fn other_lagging_libraries_are_counted_but_not_updated() {
        let mut q = queue();
        let mut behind = registry(8100);
        behind.initialized = true;
        behind.applied_sesno = 3;
        behind.file_latest_sesno = 5;
        let uninitialised = registry(8101);
        let mut blocked = registry(8102);
        blocked.blocked = true;
        let mut mine = registry(8000);
        mine.initialized = true;
        mine.applied_sesno = 89;
        mine.file_latest_sesno = 92;
        q.dbnums = vec![behind, uninitialised, blocked, mine];
        let report = judge(&[checked(1, status(8000, "latest", 89, 92))], &q, &none());
        assert_eq!(report.others_behind, 2);
        assert_eq!(report.lagging, vec![8000]);
        assert_eq!(report.rows[0].db_type.as_deref(), Some("DESI"));
        assert_eq!(report.rows[0].file_name.as_deref(), Some("des8000"));
    }

    #[test]
    fn modes_that_do_not_ingest_are_not_applicable() {
        let mut q = queue();
        assert_eq!(not_applicable(&q), None);
        q.health = Some(task_queue::Health {
            data_face: "read-through".into(),
            ..Default::default()
        });
        assert!(not_applicable(&q).is_some());

        let mut memory = status(8000, "latest", 0, 9);
        memory.data_ingest = Some(ingest(8000, "not_applicable", "none", None));
        assert!(matches!(
            settle(vec![checked(1, memory)], "12:00:00".into()),
            Outcome::Unsupported(_)
        ));
        assert!(matches!(
            settle(
                vec![checked(1, status(8000, "latest", 9, 9))],
                "12:00:00".into()
            ),
            Outcome::Checked { .. }
        ));
    }

    /// 接口文档 §4.5.3 的样例应答：用得上的格都解得出，多出来的格不碍事。
    #[test]
    fn the_documented_node_model_status_reply_parses() {
        let body = r#"{ "refno": "24381/535", "noun": "SITE", "dbnum": 7997, "unit_root": null,
          "verdict": "partial", "verdict_reason": null,
          "library": {
            "file_latest_sesno": 447, "file_latest_time": "2026-11-09T10:12:00+08:00",
            "applied_sesno": 0, "model_sesno": 0, "model_sesno_time": null, "initialized": false,
            "model_verdict": "not_judged", "model_verdict_reason": "data_watermark_unestablished",
            "model_source": "memory", "model_source_reason": "data_watermark_unestablished",
            "data_batch_running": false, "model_task_running": false, "in_watch_scope": true },
          "units": { "total": 12, "generated": 5, "missing": 7, "failed": 0, "lagging": 0, "exact": true },
          "changes": null,
          "precheck": { "units_to_generate": 7, "needs_confirm": false, "confirm_threshold_units": 20 },
          "data_ingest": { "enabled": true, "libraries": [ {
            "dbnum": 7997, "verdict": "missing", "missing_reason": "never_ingested", "checked": 0, "missing": 0, "truncated": false,
            "action": "would_enqueue", "intent": "first_import", "task_id": null, "prerequisites": [ { "dbnum": 7001, "task_id": "t-1" } ],
            "released_under_pause": false, "reason": null,
            "message": "这个库没有入库数据，可以先解析保存（首次导入，整库）",
            "applied_sesno": 0, "file_latest_sesno": 447, "last_failure": null } ] },
          "cached": false, "elapsed_ms": 41 }"#;
        let status: NodeModelStatus = serde_json::from_str(body).unwrap();
        assert_eq!(status.dbnum, 7997);
        assert_eq!(NodeVerdict::parse(&status.verdict), NodeVerdict::Partial);
        assert_eq!(status.library.file_latest_sesno, 447);
        assert_eq!(
            status.library.file_latest_time.as_deref(),
            Some("2026-11-09T10:12:00+08:00")
        );
        assert!(!status.library.initialized);
        assert!(status.library.in_watch_scope);
        assert_eq!(status.publish_ready, None);
        let lib = status.ingest().unwrap();
        assert_eq!(lib.verdict, "missing");
        assert_eq!(lib.action, "would_enqueue");
        assert!(lib.last_failure.is_none());
        // 没初始化的库：检查只报「需初始化」，不排任何任务。
        assert_eq!(form(&[checked(1, status)], &queue()), LibForm::NeedsInit);
        // 老服务端的应答缺格也解得动。
        let sparse: NodeModelStatus = serde_json::from_str(r#"{"dbnum": 1}"#).unwrap();
        assert!(sparse.library.in_watch_scope);
        assert!(sparse.data_ingest.is_none());
        // 「数据」一块形状不对只丢这一块，不连坐整份应答。
        let odd: NodeModelStatus =
            serde_json::from_str(r#"{"dbnum": 2, "verdict": "latest", "data_ingest": 7}"#).unwrap();
        assert_eq!(odd.dbnum, 2);
        assert!(odd.data_ingest.is_none());
    }

    #[test]
    fn banner_says_what_blocks_and_never_shows_contract_names() {
        let elements = [
            checked(1, status(8021, "not_generated", 0, 12)),
            checked(2, status(8000, "latest", 89, 92)),
        ];
        let report = judge(&elements, &queue(), &none());
        let (tone, title, detail) = report.banner();
        assert_eq!(tone, Status::Warn);
        assert_eq!(title, "要提资的数据不是最新");
        assert!(detail.contains("1 个库的数据水位落后于文件"), "{detail}");
        assert!(detail.contains("1 个库尚未初始化"), "{detail}");
        let mut said = vec![title, detail];
        for row in &report.rows {
            said.push(row.form.state_text());
            said.push(row.detail_line());
            said.push(row.form.way_out());
        }
        for word in ["sesno", "verdict", "applied", "pending", "dbnum"] {
            assert!(
                said.iter().all(|line| !line.contains(word)),
                "{word}: {said:?}"
            );
        }
    }

    #[test]
    fn clock_rolls_over_to_hours() {
        assert_eq!(clock(Duration::from_secs(72)), "01:12");
        assert_eq!(clock(Duration::from_secs(3_600 + 125)), "1:02:05");
    }
}
