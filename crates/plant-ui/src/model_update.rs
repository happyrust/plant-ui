//! 模型增量更新任务窗：更新预览 → 确认执行 → 执行进度（S2 / S2-B / S4 三步向导）。
//!
//! 另外五张画板都是这三步的形态：S2-A 是第一步的等待态、S2-D 收拢五种不可用与失败态、
//! S2-E 收拢七种设计库行状态、S4-B 是第三步的断线降级、S4-C 是 `partial` 终态。
//!
//! 窗口自绘外壳（窗口头 52 / 步骤条 48 / 主体 / 底部操作 56）。不用 egui 的默认标题栏：
//! 步骤条要贴在标题底下，默认标题栏塞不进去，两条横栏也对不上一套配色。
//!
//! **界面上每个数字都要指得到契约字段或本地算得出的量，指不到就不摆。**
//! 逐项出处见 `design/MODEL-UPDATE-FIELD-MAP.md`。分组一律按 dbnum——它既是执行边界，
//! 也是任务队列的键（`docs/adr/0011`）。

use std::collections::HashSet;
use std::str::FromStr;
use std::time::{Duration, Instant};

use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use egui_phosphor::regular as ph;
use serde::Deserialize;

use crate::{Cmd, RefU64};
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius};
use crate::style::widgets;

// ================================================================ 预览契约

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Preview {
    pub project: String,
    /// 本期执行范围是照哪个 MDB 解的（服务端回显，带前导 `/`）。
    ///
    /// 范围由 MDB 定，界面就得说得出自己看的是哪个 MDB 的范围。服务端与客户端
    /// 各有一份 `mdb_name` 配置，不回显的话两边错开是静默的。
    #[serde(default)]
    pub mdb: String,
    pub dbnums: Vec<DbPreview>,
    #[serde(default)]
    pub pending_model_retries: Vec<PendingModelUnit>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub up_to_date: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DbPreview {
    pub dbnum: u32,
    pub db_type: String,
    pub file_name: String,
    pub file_path: String,
    pub applied_sesno: i32,
    pub file_latest_sesno: i32,
    pub sessions: Vec<SessionPreview>,
    pub net_added: u32,
    pub net_modified: u32,
    pub net_deleted: u32,
    pub model_affecting: u32,
    pub units: Vec<UnitPreview>,
    #[serde(default)]
    pub zones: Vec<ZonePreview>,
    pub no_generation: u32,
    pub blocked: bool,
    pub anomaly: Option<FileAnomaly>,
    #[serde(default)]
    pub initialization_required: bool,
    /// 当前 MDB 声明了这个库，但当前项目目录里没有它的文件。
    ///
    /// 与「文件缺失」隔开：那是登记过、文件后来找不到了，阻断，等人补文件或注销
    /// 登记；这一条是从没登记过，文件多半在别的项目目录里——`/ALL` 声明的 29 个
    /// DESI 里有 9 个是 AvevaCatalogue 的模板与标准库，而扫描按契约只走当前项目。
    /// 它不阻断也不执行，摆出来只为回答「MDB 说 29 个，范围表怎么只有 20 个」。
    #[serde(default)]
    pub not_in_project: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionPreview {
    pub sesno: u32,
    pub added: u32,
    pub modified: u32,
    pub deleted: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ZonePreview {
    pub zone_refno: String,
    pub name: String,
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub modified: u32,
    #[serde(default)]
    pub deleted: u32,
    #[serde(default)]
    pub model_affecting: u32,
    pub units: Vec<UnitPreview>,
}

impl ZonePreview {
    /// `zone_refno` 为空的那桶就是「归属未知」，调用点先判空再取名。
    pub fn name_or_refno(&self) -> &str {
        if self.name.is_empty() {
            &self.zone_refno
        } else {
            &self.name
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnitPreview {
    pub root_refno: String,
    pub noun: String,
    pub name: String,
    #[serde(default)]
    pub added: u32,
    #[serde(default)]
    pub modified: u32,
    #[serde(default)]
    pub deleted: u32,
    /// 元素跨交付单元迁入 / 迁出的计数。最容易让人困惑的一类变化，单独说。
    #[serde(default)]
    pub moved_in: u32,
    #[serde(default)]
    pub moved_out: u32,
    #[serde(default)]
    pub model_affecting: u32,
    /// 执行时会不会重新生成它。契约默认给 true 的那批老 payload 里没有这个字段，
    /// 缺省当作会生成——少标一行「不生成」比凭空标一行「会生成」安全。
    #[serde(default = "yes")]
    pub will_generate: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PendingModelUnit {
    pub dbnum: u32,
    pub root_refno: String,
    #[serde(default)]
    pub noun: String,
    pub source_end_sesno: i32,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileAnomaly {
    Rollback {
        file_latest_sesno: i32,
        applied_sesno: i32,
    },
    PathMigrated {
        old_path: String,
        new_path: String,
    },
    TypeChanged {
        stored_db_type: String,
        observed_db_type: String,
    },
    Duplicate {
        paths: Vec<String>,
    },
    Missing {
        path: String,
    },
}

impl std::fmt::Display for FileAnomaly {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rollback {
                file_latest_sesno,
                applied_sesno,
            } => write!(
                f,
                "文件最新会话号 {file_latest_sesno} 低于已应用 {applied_sesno}。水位只前进不后退，其他批次不受影响。出路：换回正确文件。"
            ),
            Self::PathMigrated { old_path, new_path } => write!(
                f,
                "同号同类型、水位没变，只是文件被移动了：{old_path} → {new_path}。登记路径可以自动跟新。"
            ),
            Self::TypeChanged {
                stored_db_type,
                observed_db_type,
            } => write!(
                f,
                "同一个 dbnum 这次读到的是 {observed_db_type}，登记的是 {stored_db_type}。身份永不自动覆盖，必须人工确认到底哪个对。"
            ),
            Self::Duplicate { paths } => write!(
                f,
                "项目内发现 {} 个同号文件，界面不替人挑：{}。",
                paths.len(),
                paths.join("、")
            ),
            Self::Missing { path } => write!(
                f,
                "已登记的路径上找不到文件了：{path}。出路二选一：把文件放回去，或注销这条登记。"
            ),
        }
    }
}

/// S2-E 的七种设计库行形态，外加两种「不在范围内」与「无变化」。
///
/// `blocked` 不是随 `anomaly` 一起来的，是算出来的：`preview_dbnum` 里只把
/// `Rollback | TypeChanged` 判为阻断，项目扫描器再把 `Duplicate` / `Missing`
/// 直接置 `blocked = true`。**五种异常里只有 `PathMigrated` 照常执行。**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbForm {
    /// 非 DESI：压根不进本期扫描，与「阻断」来自契约的不同位置，不许合成一行。
    Excluded,
    /// 当前 MDB 声明了它，但当前项目目录里没有它的文件——多半在别的项目里
    /// （AvevaCatalogue 的模板与标准库就是这样）。它与「文件缺失」的分别是
    /// **有没有人登记过**：缺失是登记过的文件不见了，得阻断；这个从没登记过，
    /// 本期扫描只是够不着它。
    NotInProject,
    /// 从未导入过，还没有权威水位。契约不为它解会话区间，`net_*` 全是 0，
    /// **但它确实会跑**，绝不能显示成「无变化」。
    Initialize,
    PathMigrated,
    Pending,
    UpToDate,
    Rollback,
    TypeChanged,
    Duplicate,
    Missing,
}

impl DbForm {
    pub fn blocks(self) -> bool {
        matches!(
            self,
            Self::Rollback | Self::TypeChanged | Self::Duplicate | Self::Missing
        )
    }

    /// 行上那枚状态词。空串表示这一形态不值得单独标一句。
    pub fn label(self) -> &'static str {
        match self {
            Self::Excluded => "",
            Self::NotInProject => "MDB 声明",
            Self::Initialize => "需初始化",
            Self::PathMigrated => "路径迁移",
            Self::Pending => "正常待更新",
            Self::UpToDate => "",
            Self::Rollback => "文件回退",
            Self::TypeChanged => "类型变化",
            Self::Duplicate => "同号重复",
            Self::Missing => "文件缺失",
        }
    }

    /// 行尾那句「会不会执行」。它与 label 分两处摆：一句说这库怎么了，
    /// 一句说这次拿它怎么办。
    pub fn note(self) -> &'static str {
        match self {
            Self::Excluded => "非 DESI · 不在本期执行范围",
            Self::NotInProject => "不在当前项目目录 · 本期够不着",
            Self::Initialize => "首次导入 · 会执行",
            Self::PathMigrated => "登记路径自动跟新 · 会执行",
            Self::Pending => "",
            Self::UpToDate => "无变化",
            _ => "已阻断 · 水位不变",
        }
    }

    pub fn tone(self) -> Status {
        match self {
            Self::Excluded | Self::NotInProject | Self::UpToDate => Status::Neutral,
            Self::Initialize | Self::PathMigrated => Status::Warn,
            Self::Pending => Status::Success,
            _ => Status::Error,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Excluded => ph::PROHIBIT,
            Self::NotInProject => ph::FILE_DASHED,
            Self::Initialize => ph::SEAL_WARNING,
            Self::PathMigrated => ph::GIT_MERGE,
            Self::Pending | Self::UpToDate => ph::FILE,
            Self::Rollback => ph::CLOCK_COUNTER_CLOCKWISE,
            Self::TypeChanged => ph::SHAPES,
            Self::Duplicate => ph::STACK,
            Self::Missing => ph::FILE_X,
        }
    }
}

impl DbPreview {
    pub fn is_desi(&self) -> bool {
        self.db_type == "DESI"
    }

    /// 会不会产生数据批次。阻断的库在执行范围之内、只是这次不跑；
    /// 非 DESI 压根不在范围内——两回事，但都落在「不产生批次」这一边。
    ///
    /// 「够不着」也在这一边，而且它是唯一会**看起来该跑**的一种：MDB 声明了、
    /// 从没导入过，于是契约给的是 `initialization_required = true`。少了这道判断，
    /// 同一行会同时落进确认页的「会执行」与「不会执行」两张卡。
    pub fn will_run(&self) -> bool {
        self.is_desi()
            && !self.not_in_project
            && !self.blocked
            && (!self.sessions.is_empty() || self.initialization_required)
    }

    pub fn changes(&self) -> u32 {
        self.net_added + self.net_modified + self.net_deleted
    }

    pub fn form(&self) -> DbForm {
        if !self.is_desi() {
            return DbForm::Excluded;
        }
        if self.not_in_project {
            return DbForm::NotInProject;
        }
        match &self.anomaly {
            Some(FileAnomaly::Rollback { .. }) => DbForm::Rollback,
            Some(FileAnomaly::TypeChanged { .. }) => DbForm::TypeChanged,
            Some(FileAnomaly::Duplicate { .. }) => DbForm::Duplicate,
            Some(FileAnomaly::Missing { .. }) => DbForm::Missing,
            Some(FileAnomaly::PathMigrated { .. }) => DbForm::PathMigrated,
            None if self.initialization_required => DbForm::Initialize,
            None if self.sessions.is_empty() => DbForm::UpToDate,
            None => DbForm::Pending,
        }
    }

    /// 会话区间。需初始化的库契约不为它解区间，那一行不摆区间而摆一句实话。
    pub fn window(&self) -> String {
        if self.initialization_required {
            return "首次导入 · 尚无权威水位".into();
        }
        format!(
            "sesno {} → {}",
            self.applied_sesno + 1,
            self.file_latest_sesno
        )
    }
}

/// 一次预览能算出来的全部汇总量。分子分母都在这一处算，免得几张画板各算各的。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub added: u32,
    pub modified: u32,
    pub deleted: u32,
    pub model_affecting: u32,
    pub no_generation: u32,
    /// 受影响交付单元：`DeliveryUnitSummary` 已按 refno 去重，直接相加。
    pub units: usize,
    /// 会产生数据批次的 dbnum 数。
    pub batches: usize,
    /// 在本期执行范围之内的 DESI 库数（当前 MDB 声明、且当前项目目录里有文件）。
    pub desi: usize,
    /// 非 DESI 而被排除的库数。
    pub excluded: usize,
    /// MDB 声明了、但当前项目目录里没有文件的库数。它不算在 `desi` 里——
    /// 两个数摆在一起才回答得了「MDB 说 29 个，这里怎么只有 20 个」。
    pub not_in_project: usize,
    pub blocked: usize,
}

impl Preview {
    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for db in &self.dbnums {
            t.added += db.net_added;
            t.modified += db.net_modified;
            t.deleted += db.net_deleted;
            t.model_affecting += db.model_affecting;
            t.no_generation += db.no_generation;
            t.units += db.units.len();
            t.batches += usize::from(db.will_run());
            if !db.is_desi() {
                t.excluded += 1;
            } else if db.not_in_project {
                t.not_in_project += 1;
            } else {
                t.desi += 1;
                t.blocked += usize::from(db.blocked);
            }
        }
        t
    }

    /// 有没有值得按下「开始更新」的东西。待重试单元自己就够成一次运行——
    /// 契约明写没有新 sesno 也要显示并允许重试。
    pub fn executable(&self) -> bool {
        self.dbnums.iter().any(DbPreview::will_run) || !self.pending_model_retries.is_empty()
    }
}

// ================================================================ 执行契约

/// `POST /api/v1/update/execute` 的 202 回执，与 gen-model 的 `ManualEnqueueReceipt` 同形。
///
/// 合流之后执行请求**一律入队**，回的不再是单个 task_id：一次扫描可能排下好几个库，
/// 也可能一行都没排（都并进了既有排队行，或者水位本来就最新）。「已有任务在跑 · 409」
/// 那一格随之退役——执行请求不会再被拒。进度去「任务队列」视图看。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Enqueued {
    #[serde(default)]
    pub project: String,
    /// 本次扫描到的候选 dbnum 数（含已是最新的与被阻断的）。
    #[serde(default)]
    pub scanned: usize,
    /// 新排的行，含接在运行中批次之后的那种。
    #[serde(default)]
    pub enqueued: Vec<EnqueuedBatch>,
    /// 并入既有排队行的：目标会话号被推高，不另开一行。
    #[serde(default)]
    pub merged: Vec<EnqueuedBatch>,
    /// 已被既有排队行覆盖、无需动作的 dbnum。
    #[serde(default)]
    pub already_covered: Vec<u32>,
    /// 阻断的库压根不入队，队列里不会有它们的行——这里是唯一说得出
    /// 「它为什么没排上」的地方。
    #[serde(default)]
    pub blocked: Vec<BlockedDbnum>,
    #[serde(default)]
    pub up_to_date: usize,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct EnqueuedBatch {
    pub task_id: String,
    pub dbnum: u32,
    #[serde(default)]
    pub db_type: String,
    /// 排队中的位置（1 起；运行中的行不占位置）。
    #[serde(default)]
    pub position: usize,
    #[serde(default)]
    pub start_sesno: i32,
    #[serde(default)]
    pub end_sesno: i32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BlockedDbnum {
    pub dbnum: u32,
    #[serde(default)]
    pub reason: String,
}

impl Enqueued {
    /// 一句能写进日志的回执。入队、并入、阻断三件事分开说——它们的出路不一样：
    /// 排上的等着跑，并入的已经在别人那一行里，阻断的要人去处理文件。
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.enqueued.is_empty() {
            let list = self
                .enqueued
                .iter()
                .map(|b| format!("db{} 排第 {} 位", b.dbnum, b.position))
                .collect::<Vec<_>>()
                .join("、");
            parts.push(format!("已入队 {} 个数据批次（{list}）", self.enqueued.len()));
        }
        if !self.merged.is_empty() {
            let list = self
                .merged
                .iter()
                .map(|b| format!("db{}", b.dbnum))
                .collect::<Vec<_>>()
                .join("、");
            parts.push(format!("{} 个并入已排队的批次（{list}）", self.merged.len()));
        }
        if !self.blocked.is_empty() {
            let list = self
                .blocked
                .iter()
                .map(|b| format!("db{}：{}", b.dbnum, b.reason))
                .collect::<Vec<_>>()
                .join("；");
            parts.push(format!("{} 个阻断未入队（{list}）", self.blocked.len()));
        }
        if parts.is_empty() {
            return format!(
                "扫描 {} 个库，没有需要入队的批次：{} 个已是最新，{} 个已被排队中的批次覆盖",
                self.scanned,
                self.up_to_date,
                self.already_covered.len()
            );
        }
        format!("扫描 {} 个库；{}", self.scanned, parts.join("；"))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Run {
    pub task_id: String,
    pub kind: String,
    pub state: String,
    pub project: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    pub events_seen: u64,
    pub result: Option<Outcome>,
    /// ADR-0007 的四个权威计数。服务端补上之前一个都取不到，那时进度只报已完成数、
    /// 不画确定态的条——**分母绝不许拿预览值相加**，那个加法两个方向都会偏。
    #[serde(default)]
    pub total_batches: Option<u32>,
    #[serde(default)]
    pub batches_done: Option<u32>,
    #[serde(default)]
    pub total_units: Option<u32>,
    #[serde(default)]
    pub units_done: Option<u32>,
}

impl Run {
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "partial" | "failed")
    }
}

/// 一个数据批次的结果摘要，与 gen-model 的 `DataBatchTaskResult` 同形。
///
/// **`batch` 是单数**：合流之后一个任务就是一个数据批次，「一次运行装着几个批次」
/// 那种形状随「运行」一并退役（ADR-0011）。解成数组的话字段名对不上、又带
/// `serde(default)`，会静默解成空——界面上看不出任何异样，摘要却整块空掉。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Outcome {
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub status: RunStatus,
    #[serde(default)]
    pub batch: Option<BatchResult>,
    #[serde(default)]
    pub units: Vec<UnitResult>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// 一个成功生成的交付单元给出的刷新线索。
///
/// 更新之后可能变的有三处：单元自己（几何换了）、它更新**前**的 OWNER（元素可能
/// 从那儿移走或被删掉）、更新**后**的 OWNER（元素可能是新增或移进去的）。三者
/// 常常是同一个，两个 OWNER 也都可能取不到——契约里它们是可选字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshUnit {
    pub root: RefU64,
    pub old_owner: Option<RefU64>,
    pub new_owner: Option<RefU64>,
}

impl Outcome {
    /// 本次真正生成成功的交付单元及其原 / 新 OWNER。
    ///
    /// 失败单元不进来：它们的模型压根没换，旧显示仍然是对的。解不出参考号的也
    /// 跳过——契约给的是 PDMS 串，宁可少刷一个分支也不去猜它想说什么。
    pub fn refresh_units(&self) -> Vec<RefreshUnit> {
        fn parse(text: &str) -> Option<RefU64> {
            RefU64::from_str(text).ok().filter(|r| r.is_valid())
        }
        self.units
            .iter()
            .filter(|unit| unit.status == UnitStatus::Generated)
            .filter_map(|unit| {
                Some(RefreshUnit {
                    root: parse(&unit.root_refno)?,
                    old_owner: unit.old_owner.as_deref().and_then(parse),
                    new_owner: unit.new_owner.as_deref().and_then(parse),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Success,
    /// 既有成功也有失败。数据批次已推进的水位不因模型生成失败而回退。
    Partial,
    Failed,
    #[default]
    UpToDate,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchResult {
    pub dbnum: u32,
    #[serde(default)]
    pub db_type: String,
    #[serde(default)]
    pub file_path: String,
    pub start_sesno: i32,
    pub end_sesno: i32,
    #[serde(default)]
    pub status: BatchStatus,
    #[serde(default)]
    pub message: Option<String>,
    /// 预览扫描之后才并入本批次的会话。契约明写结果摘要必须逐条列出。
    #[serde(default)]
    pub merged_sesnos: Vec<u32>,
    #[serde(default)]
    pub changed_elements: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Applied,
    Failed,
    #[default]
    Skipped,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnitResult {
    pub dbnum: u32,
    pub root_refno: String,
    #[serde(default)]
    pub noun: String,
    #[serde(default)]
    pub status: UnitStatus,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub old_owner: Option<String>,
    #[serde(default)]
    pub new_owner: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitStatus {
    #[default]
    Generated,
    Failed,
}

/// 执行期的两阶段进度事件，与 gen-model 的 `ManualUpdateEvent` 同形。
/// 只在 `/api/v1/ws` 的 tasks 主题上发，轮询取不到（ADR-0005）。
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProgressEvent {
    DataBatchStarted {
        dbnum: u32,
        start_sesno: i32,
        end_sesno: i32,
    },
    DataBatchFinished {
        dbnum: u32,
        success: bool,
        message: Option<String>,
    },
    ModelUnitStarted {
        dbnum: u32,
        root_refno: String,
        noun: String,
    },
    ModelUnitFinished {
        dbnum: u32,
        root_refno: String,
        success: bool,
        message: Option<String>,
    },
}

/// 一行的进行状态。`Unknown` 专给断线：没收到事件就不知道它开没开始，
/// 不许说成「排队中」（ADR-0005）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowState {
    Running,
    Done,
    Failed,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct BatchRow {
    pub dbnum: u32,
    pub start_sesno: i32,
    pub end_sesno: i32,
    pub state: RowState,
    pub message: Option<String>,
    pub started: Instant,
}

#[derive(Debug, Clone)]
pub struct UnitRow {
    pub dbnum: u32,
    pub root_refno: String,
    pub noun: String,
    pub state: RowState,
    pub message: Option<String>,
    pub started: Instant,
}

/// WebSocket 那条通道自己的状态。它与运行状态是两件事：连接断了运行照跑，
/// 所以要分开显示，不能拿连接状态去暗示任务出了问题。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Feed {
    #[default]
    Connecting,
    Live,
    /// 断开，附一句能说给人听的原因。
    Down(String),
}

/// 逐行明细。这里的行数只用来铺行，**不当分母**——权威计数在任务详情里（ADR-0007）。
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub feed: Feed,
    pub batches: Vec<BatchRow>,
    pub units: Vec<UnitRow>,
    /// 本端实际收到的事件条数。与任务详情的 `events_seen` 相减就是断线期间落后的条数。
    pub received: u64,
    /// 阶段一首尾两条事件的时刻，用来算「已完成 · 用时 48 秒」。
    first_batch_at: Option<Instant>,
    last_batch_at: Option<Instant>,
}

impl Progress {
    /// 明细通道从一开始就不存在的那种进度。
    ///
    /// 宿主还没接 WebSocket，给它 `Progress::default()` 的话列表区会停在
    /// 「连接中」——一个永远不会变的假状态。这里让它一开口就说清没接，
    /// 人看得懂，也知道少的是什么。计时字段不对外，所以宿主构不出这个形状，
    /// 得由这一侧给。
    pub fn feed_down(reason: impl Into<String>) -> Self {
        Self {
            feed: Feed::Down(reason.into()),
            ..Default::default()
        }
    }

    pub fn apply(&mut self, event: ProgressEvent) {
        self.received += 1;
        let now = Instant::now();
        match event {
            ProgressEvent::DataBatchStarted {
                dbnum,
                start_sesno,
                end_sesno,
            } => {
                self.first_batch_at.get_or_insert(now);
                self.batches.push(BatchRow {
                    dbnum,
                    start_sesno,
                    end_sesno,
                    state: RowState::Running,
                    message: None,
                    started: now,
                });
            }
            ProgressEvent::DataBatchFinished {
                dbnum,
                success,
                message,
            } => {
                self.last_batch_at = Some(now);
                if let Some(row) = self.batches.iter_mut().rev().find(|r| r.dbnum == dbnum) {
                    row.state = if success {
                        RowState::Done
                    } else {
                        RowState::Failed
                    };
                    row.message = message;
                }
            }
            ProgressEvent::ModelUnitStarted {
                dbnum,
                root_refno,
                noun,
            } => self.units.push(UnitRow {
                dbnum,
                root_refno,
                noun,
                state: RowState::Running,
                message: None,
                started: now,
            }),
            ProgressEvent::ModelUnitFinished {
                dbnum,
                root_refno,
                success,
                message,
            } => {
                if let Some(row) = self
                    .units
                    .iter_mut()
                    .rev()
                    .find(|r| r.dbnum == dbnum && r.root_refno == root_refno)
                {
                    row.state = if success {
                        RowState::Done
                    } else {
                        RowState::Failed
                    };
                    row.message = message;
                }
            }
        }
    }

    /// 断线时把还没收到收尾事件的行降级。它们可能已经做完了，只是消息没到，
    /// 继续显示「进行中」是在撒谎。
    pub fn mark_unknown(&mut self) {
        for row in &mut self.batches {
            if row.state == RowState::Running {
                row.state = RowState::Unknown;
            }
        }
        for row in &mut self.units {
            if row.state == RowState::Running {
                row.state = RowState::Unknown;
            }
        }
    }

    /// 服务端已发多少条、本端少收了多少条。断线补不回来，只能把差额说出来。
    /// **只用来解释明细缺了多少，不许拿它反推进度**（ADR-0007）。
    pub fn behind(&self, events_seen: u64) -> u64 {
        events_seen.saturating_sub(self.received)
    }

    fn units_done(&self) -> usize {
        self.units
            .iter()
            .filter(|u| u.state == RowState::Done)
            .count()
    }

    fn batch_phase_done(&self) -> bool {
        !self.batches.is_empty() && self.batches.iter().all(|b| b.state != RowState::Running)
    }

    fn batch_elapsed(&self) -> Option<Duration> {
        Some(self.last_batch_at?.duration_since(self.first_batch_at?))
    }
}

// ================================================================ 不可用与失败态

/// 服务端错误包封 `{ code, message, detail }`。**按 `code` 分型，不解析 message 字符串，
/// 更不把 anyhow 错误链摊给人看**（S2-D）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Failure {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
}

/// S2-D 的五种形态，减去「已是最新」（那是 200，走 `up_to_date`）与
/// 「数据源未就绪」（那一态连请求都不发，入口按钮已经禁着）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailForm {
    /// 422 · precondition：自动同步正在接管，两者互斥。
    SyncLive,
    /// 409 · conflict：同项目已有任务在跑。
    Conflict,
    /// 504 · timeout：连不上或超时。
    Timeout,
    /// 其余一律 500 · internal——**只有这一类**才需要把原始 message 摊出来。
    Internal,
}

impl Failure {
    pub fn form(&self) -> FailForm {
        match self.code.as_str() {
            "precondition" => FailForm::SyncLive,
            "conflict" => FailForm::Conflict,
            "timeout" => FailForm::Timeout,
            _ => FailForm::Internal,
        }
    }

    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: None,
        }
    }
}

impl FailForm {
    fn title(self) -> &'static str {
        match self {
            Self::SyncLive => "自动同步正在接管",
            Self::Conflict => "本项目已有一个更新在跑",
            Self::Timeout => "连不上模型服务",
            Self::Internal => "模型服务出错",
        }
    }

    fn body(self) -> &'static str {
        match self {
            Self::SyncLive => "服务端开着自动追新，与手动增量更新互斥。要手动更新，得先在服务端关掉它。",
            Self::Conflict => "同一项目同时只允许一个手动增量更新。等它跑完，或者切过去看它的进度。",
            Self::Timeout => "模型服务没有响应。没有任何数据被改动，可以直接重试。",
            Self::Internal => "服务端未能完成这次请求。原始信息收在下面的详情里。",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Self::SyncLive => ph::ARROWS_CLOCKWISE,
            Self::Conflict => ph::CLOCK_COUNTER_CLOCKWISE,
            Self::Timeout => ph::WARNING,
            Self::Internal => ph::WARNING_CIRCLE,
        }
    }

    fn tone(self) -> Status {
        match self {
            Self::SyncLive | Self::Conflict => Status::Warn,
            _ => Status::Error,
        }
    }
}

// ================================================================ 视图状态

#[derive(Debug, Clone, Default)]
pub enum Vm {
    /// 还没预览，或者用户放弃了等待。
    #[default]
    Idle,
    Loading,
    Ready(Preview),
    Starting,
    /// 装箱是因为它比别的变体大出一截：`Run` 的几个串加上逐行明细，不装箱整个
    /// `Vm` 都得按它的尺寸走。
    Running(Box<RunVm>),
    Failed(Failure),
}

impl Vm {
    /// 相位标识。计时器靠它判断该不该重新起表；同一相位内数据怎么变都不重置。
    fn phase(&self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Loading => 1,
            Self::Ready(_) => 2,
            Self::Starting => 3,
            Self::Running(_) => 4,
            Self::Failed(_) => 5,
        }
    }
}

/// 一次运行实例在界面上的全部状态：终态靠轮询（`run` / `poll_error`），
/// 明细靠 WebSocket（`progress`），两条通道并存（ADR-0005）。
#[derive(Debug, Clone, Default)]
pub struct RunVm {
    pub run: Run,
    pub poll_error: Option<String>,
    pub progress: Progress,
}

/// 向导走到第几步。第三步由 `Vm` 决定（一提交就进执行进度），前两步是纯界面状态：
/// 「确认执行」不发任何请求，它存在的唯一理由是**按下去的是一个不可取消的按钮**。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Step {
    #[default]
    Preview,
    Confirm,
    Running,
}

impl Step {
    fn index(self) -> usize {
        match self {
            Self::Preview => 0,
            Self::Confirm => 1,
            Self::Running => 2,
        }
    }
}

/// 缓存的上一份预览。S2-A 摆它的过期摘要，S4 靠它补出阻断 / 排除的库与
/// no_generation，S4-C 靠它算出「实际 512 项，预览时 487 项」的差额。
struct Cached {
    preview: Preview,
    at: Instant,
}

/// 相位计时器。「已用 01:12」「耗时 04:38」都是本地量，从相位切换那一刻起算。
#[derive(Default)]
struct Clock {
    phase: u8,
    since: Option<Instant>,
    frozen: Option<Duration>,
}

impl Clock {
    fn sync(&mut self, phase: u8) {
        if self.phase != phase || self.since.is_none() {
            self.phase = phase;
            self.since = Some(Instant::now());
            self.frozen = None;
        }
    }

    fn elapsed(&self) -> Duration {
        self.frozen
            .or_else(|| self.since.map(|s| s.elapsed()))
            .unwrap_or_default()
    }

    /// 终态到达时把表停住，之后不再往前走。
    fn freeze(&mut self) {
        if self.frozen.is_none() {
            self.frozen = Some(self.since.map(|s| s.elapsed()).unwrap_or_default());
        }
    }
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    step: Step,
    clock: Clock,
    cache: Option<Cached>,
    /// 显式点过的展开箭头。默认展开与否按行的种类定，这里只记「与默认相反」的那些。
    toggled: HashSet<String>,
    only_model_affecting: bool,
    /// internal 那一类失败的原始 message，默认折起。
    detail_open: bool,
}

impl State {
    fn sync(&mut self, vm: &Vm) {
        let phase = vm.phase();
        let switched = self.clock.phase != phase;
        self.clock.sync(phase);
        match vm {
            Vm::Ready(preview) => {
                if switched || self.cache.is_none() {
                    self.cache = Some(Cached {
                        preview: preview.clone(),
                        at: Instant::now(),
                    });
                }
                if switched {
                    // 预览一刷新就退回第一步：确认页上摆的是刚才那份的数字。
                    self.step = Step::Preview;
                    self.detail_open = false;
                }
            }
            Vm::Running(session) => {
                self.step = Step::Running;
                if session.run.terminal() {
                    self.clock.freeze();
                }
            }
            Vm::Failed(_) if switched => self.detail_open = false,
            _ => {}
        }
    }

    fn open_row(&self, key: &str, default_open: bool) -> bool {
        self.toggled.contains(key) != default_open
    }

    fn toggle_row(&mut self, key: &str) {
        if !self.toggled.remove(key) {
            self.toggled.insert(key.to_owned());
        }
    }

    fn cached(&self) -> Option<&Preview> {
        self.cache.as_ref().map(|c| &c.preview)
    }
}

// ================================================================ 窗口外壳

pub fn show(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    api_url: &str,
    vm: &Vm,
    state: &mut State,
) -> Vec<Cmd> {
    if !state.open {
        return Vec::new();
    }
    state.sync(vm);

    let mut cmds = Vec::new();
    egui::Window::new("模型增量更新")
        .id(egui::Id::new("model-update-window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .default_size([d.px(1040.0), d.px(720.0)])
        .min_size([d.px(760.0), d.px(520.0)])
        .frame(
            egui::Frame::new()
                .fill(t.bg_panel)
                .corner_radius(CornerRadius::same(radius::LG))
                .stroke(Stroke::new(1.0_f32, t.border_strong)),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            header(ui, t, d, vm, state);
            steps(ui, t, d, state.step);

            let footer_h = d.px(56.0);
            let body_h = (ui.available_height() - footer_h).max(d.px(200.0));
            let width = ui.available_width();
            ui.allocate_ui(vec2(width, body_h), |ui| {
                ui.set_min_size(vec2(width, body_h));
                match vm {
                    Vm::Idle => idle_body(ui, t, d),
                    Vm::Loading => previewing_body(ui, t, d, state),
                    Vm::Starting => center_note(
                        ui,
                        t,
                        d,
                        None,
                        "正在提交增量更新任务…",
                        "服务端接下这次运行之后，这里会换成逐批次、逐单元的执行进度。",
                    ),
                    Vm::Failed(failure) => failure_body(ui, t, d, failure, state),
                    Vm::Ready(preview) if preview.up_to_date => up_to_date_body(ui, t, d, state),
                    Vm::Ready(preview) => match state.step {
                        Step::Confirm => confirm_body(ui, t, d, preview, state),
                        _ => preview_body(ui, t, d, preview, state),
                    },
                    Vm::Running(session) => running_body(ui, t, d, session, state, &mut cmds),
                }
            });

            footer(ui, t, d, footer_h, api_url, vm, state, &mut cmds);
        });
    cmds
}

fn header(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm, state: &mut State) {
    let (title, sub, tone, icon) = header_text(vm, state);
    let rect = bar(ui, d.px(52.0), t.bg_chrome, Corner::Top);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(12.0);

    let (fg, bg) = t.status(tone);
    let (chip, _) = ui.allocate_exact_size(vec2(d.px(28.0), d.px(28.0)), Sense::hover());
    fill(&ui, chip, radius::MD, bg, None);
    glyph(&ui, chip.center(), icon, d.px(15.0), fg);

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            RichText::new(title)
                .font(Font::title(d))
                .color(t.text_primary),
        );
        ui.label(RichText::new(sub).font(Font::meta(d)).color(t.text_muted));
    });

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui.add(widgets::tool_btn(t, d, ph::X, false)).clicked() {
            state.open = false;
        }
        // 「已用 / 耗时」只在有表可读的两个相位摆；预览结果页摆一句用不着计时的提示。
        match vm {
            Vm::Loading => stopwatch(ui, t, d, "已用", state.clock.elapsed()),
            Vm::Running(session) => {
                let label = if session.run.terminal() {
                    "耗时"
                } else {
                    "已用"
                };
                stopwatch(ui, t, d, label, state.clock.elapsed());
            }
            _ => {
                ui.label(
                    RichText::new("关闭窗口不会中断任务")
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
            }
        }
    });
}

fn header_text(vm: &Vm, state: &State) -> (String, String, Status, &'static str) {
    let project = project_of(vm, state);
    match vm {
        Vm::Idle => (
            "模型更新".into(),
            format!("{project} · 尚未预览"),
            Status::Neutral,
            ph::ARROWS_CLOCKWISE,
        ),
        Vm::Loading => (
            "模型更新 · 正在预览".into(),
            format!("{project} · 只读扫描"),
            Status::Info,
            ph::ARROWS_CLOCKWISE,
        ),
        Vm::Starting => (
            "模型更新 · 正在提交".into(),
            format!("{project} · 等待服务端接下任务"),
            Status::Info,
            ph::ARROWS_CLOCKWISE,
        ),
        Vm::Failed(failure) => (
            format!("模型更新 · {}", failure.form().title()),
            format!("{project} · {}", failure.code),
            failure.form().tone(),
            failure.form().icon(),
        ),
        Vm::Ready(preview) if preview.up_to_date => (
            "模型更新 · 已是最新".into(),
            format!("{project} · 没有可更新的内容"),
            Status::Success,
            ph::CHECK_CIRCLE,
        ),
        Vm::Ready(_) if state.step == Step::Confirm => (
            "模型更新 · 确认执行".into(),
            format!("{project} · 开始后不可中途取消"),
            Status::Warn,
            ph::SEAL_WARNING,
        ),
        // 范围由 MDB 定，副标题就得把 MDB 说出来——服务端与客户端各有一份
        // `mdb_name`，只说「仅扫描当前项目」的话，两边错开时界面一声不响。
        Vm::Ready(preview) => (
            "模型更新".into(),
            match preview.mdb.as_str() {
                "" => format!("{project} · 仅扫描当前项目"),
                mdb => format!("{project} · MDB {mdb} · 仅扫描当前项目"),
            },
            Status::Info,
            ph::ARROWS_CLOCKWISE,
        ),
        Vm::Running(session) => {
            let run = &session.run;
            if !run.terminal() {
                return (
                    "模型更新 · 正在执行".into(),
                    format!("{project} · 任务开始后不可中途取消"),
                    Status::Info,
                    ph::ARROWS_CLOCKWISE,
                );
            }
            let outcome = run.result.as_ref();
            match outcome.map(|o| o.status).unwrap_or_default() {
                RunStatus::Success => (
                    "模型更新 · 已完成".into(),
                    format!("{project} · 数据与模型都已跟上"),
                    Status::Success,
                    ph::CHECK_CIRCLE,
                ),
                RunStatus::Partial => (
                    "模型更新 · 部分完成".into(),
                    format!(
                        "{project} · 数据已全部写入，{} 个交付单元未能生成",
                        outcome.map_or(0, |o| o
                            .units
                            .iter()
                            .filter(|u| u.status == UnitStatus::Failed)
                            .count())
                    ),
                    Status::Warn,
                    ph::WARNING,
                ),
                RunStatus::Failed => (
                    "模型更新 · 未完成".into(),
                    format!("{project} · 没有任何一件事做成"),
                    Status::Error,
                    ph::WARNING_CIRCLE,
                ),
                RunStatus::UpToDate => (
                    "模型更新 · 无事可做".into(),
                    format!("{project} · 这次运行没有可执行的工作"),
                    Status::Neutral,
                    ph::CHECK_CIRCLE,
                ),
            }
        }
    }
}

fn project_of(vm: &Vm, state: &State) -> String {
    match vm {
        Vm::Ready(preview) if !preview.project.is_empty() => preview.project.clone(),
        Vm::Running(session) if !session.run.project.is_empty() => session.run.project.clone(),
        _ => state
            .cached()
            .map(|p| p.project.clone())
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| "当前项目".into()),
    }
}

/// 步骤条。三步是本仓库定义的流程，没有契约来源；六张画板上都有它。
fn steps(ui: &mut Ui, t: &Tokens, d: Density, step: Step) {
    const NAMES: [&str; 3] = ["更新预览", "确认执行", "执行进度"];
    let rect = bar(ui, d.px(48.0), t.bg_panel, Corner::None);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(20.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);

    let current = step.index();
    // 连接线各占剩下的一半。先量总宽再分，边量边分的话第二条会比第一条短一截。
    let labels: f32 = NAMES
        .iter()
        .map(|n| layout(&ui, n, Font::strong(d)).size().x + d.px(28.0))
        .sum();
    let connector = ((ui.available_width() - labels) / 2.0 - d.px(24.0)).max(d.px(24.0));

    for (i, name) in NAMES.into_iter().enumerate() {
        if i > 0 {
            let (line, _) = ui.allocate_exact_size(vec2(connector, 1.0), Sense::hover());
            ui.painter().rect_filled(
                line,
                CornerRadius::ZERO,
                if i <= current { t.success } else { t.border },
            );
        }
        let (num, _) = ui.allocate_exact_size(vec2(d.px(20.0), d.px(20.0)), Sense::hover());
        if i < current {
            glyph(&ui, num.center(), ph::CHECK, d.px(14.0), t.success);
        } else if i == current {
            ui.painter()
                .circle_filled(num.center(), d.px(10.0), t.accent);
            mono_at(&ui, num.center(), &(i + 1).to_string(), d, t.accent_ink);
        } else {
            ui.painter().circle_stroke(
                num.center(),
                d.px(10.0) - 0.5,
                Stroke::new(1.0_f32, t.border_strong),
            );
            mono_at(&ui, num.center(), &(i + 1).to_string(), d, t.text_muted);
        }
        let (font, color) = if i == current {
            (Font::strong(d), t.text_primary)
        } else if i < current {
            (Font::body(d), t.text_secondary)
        } else {
            (Font::body(d), t.text_muted)
        };
        ui.label(RichText::new(name).font(font).color(color));
    }
    hairline(ui.painter(), rect, t.border);
}

#[allow(clippy::too_many_arguments)]
fn footer(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    h: f32,
    api_url: &str,
    vm: &Vm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    let rect = bar(ui, h, t.bg_chrome, Corner::Bottom);
    ui.painter().rect_filled(
        Rect::from_min_size(rect.left_top(), vec2(rect.width(), 1.0)),
        CornerRadius::ZERO,
        t.border,
    );
    let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);

    let (icon, hint, tone) = footer_hint(vm, state, api_url);
    let (fg, _) = t.status(tone);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(14.0), d.px(14.0)), Sense::hover());
    glyph(&ui, mark.center(), icon, d.px(13.0), fg);
    ui.label(
        RichText::new(hint)
            .font(Font::meta(d))
            .color(if tone == Status::Neutral {
                t.text_muted
            } else {
                fg
            }),
    );

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        footer_actions(ui, t, d, vm, state, cmds);
    });
}

fn footer_hint(vm: &Vm, state: &State, api_url: &str) -> (&'static str, String, Status) {
    match vm {
        Vm::Loading => (
            ph::LOCK_SIMPLE,
            "预览为只读：不修改元素数据、模型或已应用会话号；关闭窗口不影响后台扫描".into(),
            Status::Neutral,
        ),
        Vm::Ready(preview) if !preview.up_to_date && state.step == Step::Confirm => (
            ph::WARNING,
            "确认后立即开始，任务不可中途取消".into(),
            Status::Warn,
        ),
        Vm::Ready(preview) if !preview.up_to_date => (
            ph::LOCK_SIMPLE,
            "预览为只读：不修改元素数据、模型或已应用会话号".into(),
            Status::Neutral,
        ),
        Vm::Running(session) if session.run.terminal() => (
            ph::INFO,
            "失败单元已登记为待重试，下次更新自动并入；已写入的数据不受影响".into(),
            Status::Neutral,
        ),
        Vm::Running(_) => (
            ph::INFO,
            "关闭窗口只隐藏进度，任务继续运行；再次打开入口恢复本窗口".into(),
            Status::Neutral,
        ),
        _ => (ph::PLUGS, format!("服务 {api_url}"), Status::Neutral),
    }
}

fn footer_actions(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &Vm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    match vm {
        Vm::Idle | Vm::Failed(_) => {
            if ui
                .add(widgets::button(t, d, "重新预览").icon(ph::ARROWS_CLOCKWISE))
                .clicked()
            {
                cmds.push(Cmd::RefreshModelUpdate);
            }
            if ui.add(widgets::button(t, d, "关闭")).clicked() {
                state.open = false;
            }
        }
        Vm::Loading => {
            // 「取消预览」只放弃客户端等待。服务端没有 cancel 接口，文案不许暗示它会停。
            if ui.add(widgets::button(t, d, "取消预览")).clicked() {
                cmds.push(Cmd::CancelModelUpdatePreview);
            }
        }
        Vm::Starting => {}
        Vm::Ready(preview) if preview.up_to_date => {
            if ui
                .add(widgets::button(t, d, "重新预览").icon(ph::ARROWS_CLOCKWISE))
                .clicked()
            {
                cmds.push(Cmd::RefreshModelUpdate);
            }
            if ui.add(widgets::button(t, d, "关闭")).clicked() {
                state.open = false;
            }
        }
        Vm::Ready(preview) => {
            let totals = preview.totals();
            if state.step == Step::Confirm {
                if ui
                    .add(
                        widgets::button(
                            t,
                            d,
                            &format!("确认并开始 · {} 个数据批次", totals.batches),
                        )
                        .icon(ph::PLAY)
                        .primary(),
                    )
                    .clicked()
                {
                    cmds.push(Cmd::ExecuteModelUpdate);
                }
                if ui.add(widgets::button(t, d, "返回预览")).clicked() {
                    state.step = Step::Preview;
                }
            } else {
                if ui
                    .add_enabled(
                        preview.executable(),
                        widgets::button(t, d, &format!("开始更新 · {} 个批次", totals.batches))
                            .icon(ph::PLAY)
                            .primary(),
                    )
                    .on_disabled_hover_text("没有可执行的数据批次，也没有待重试单元")
                    .clicked()
                {
                    state.step = Step::Confirm;
                }
                if ui.add(widgets::button(t, d, "取消")).clicked() {
                    state.open = false;
                }
            }
        }
        Vm::Running(session) => {
            let done = session.run.terminal();
            if done {
                if ui
                    .add(widgets::button(t, d, "重新预览").icon(ph::ARROWS_CLOCKWISE))
                    .clicked()
                {
                    cmds.push(Cmd::RefreshModelUpdate);
                }
                if ui.add(widgets::button(t, d, "关闭")).clicked() {
                    state.open = false;
                }
            } else {
                if ui.add_enabled(false, widgets::button(t, d, "完成")).clicked() {
                    state.open = false;
                }
                if ui
                    .add(widgets::button(t, d, "转入后台").icon(ph::ARROWS_IN_SIMPLE))
                    .clicked()
                {
                    state.open = false;
                }
            }
        }
    }
}

// ================================================================ 第一步：预览

fn idle_body(ui: &mut Ui, t: &Tokens, d: Density) {
    center_note(
        ui,
        t,
        d,
        Some(ph::ARROWS_CLOCKWISE),
        "还没有预览结果",
        "预览只读扫描设计库，不改任何数据。按下方「重新预览」开始。",
    );
}

/// S2-A 正在预览。逐库推进要等 ADR-0006 的 `PreviewDbStarted / Finished` 事件，
/// 现在一条都没有，所以这里不摆库计数、不摆进度条——**造一条走不满的条比不摆更糟**。
fn previewing_body(ui: &mut Ui, t: &Tokens, d: Density, state: &State) {
    pad(ui, d, |ui| {
        ui.spacing_mut().item_spacing.y = d.px(12.0);
        card(ui, t, d, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(d.px(15.0)).color(t.accent));
                ui.label(
                    RichText::new("正在扫描设计库")
                        .font(Font::strong(d))
                        .color(t.text_primary),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(clock(state.clock.elapsed()))
                            .font(Font::mono_meta(d))
                            .color(t.text_muted),
                    );
                });
            });
            ui.add_space(d.px(6.0));
            ui.label(
                RichText::new(
                    "逐库比对会话号，单库可达数分钟。扫描期间不写入任何东西，超时重试即可。",
                )
                .font(Font::meta(d))
                .color(t.text_secondary),
            );
        });

        if let Some(cached) = &state.cache {
            let preview = &cached.preview;
            let totals = preview.totals();
            card(ui, t, d, |ui| {
                ui.horizontal(|ui| {
                    icon_label(
                        ui,
                        t,
                        d,
                        ph::CLOCK_COUNTER_CLOCKWISE,
                        t.text_muted,
                        "上次预览结果（已过期）",
                        t.text_secondary,
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(ago(cached.at.elapsed()))
                                .font(Font::mono_meta(d))
                                .color(t.text_muted),
                        );
                    });
                });
                ui.add_space(d.px(6.0));
                ui.label(
                    RichText::new(format!(
                        "{} 个数据批次 · {} 个交付单元 · {} 个待重试单元",
                        totals.batches,
                        totals.units,
                        preview.pending_model_retries.len()
                    ))
                    .font(Font::mono_meta(d))
                    .color(t.text_muted),
                );
            });
        }
    });
}

fn up_to_date_body(ui: &mut Ui, t: &Tokens, d: Density, state: &State) {
    let since = state
        .cache
        .as_ref()
        .map(|c| format!("上次扫描于 {}", ago(c.at.elapsed())))
        .unwrap_or_default();
    pad(ui, d, |ui| {
        note_card(
            ui,
            t,
            d,
            Status::Success,
            ph::CHECK_CIRCLE,
            "已是最新，没有可更新的内容",
            &format!(
                "没有待应用的会话，也没有待重试的交付单元。这是正常结果，不是错误。{since}"
            ),
        );
    });
}

/// S2 预览结果：左边变化树，右边 360 宽的汇总列。
fn preview_body(ui: &mut Ui, t: &Tokens, d: Density, preview: &Preview, state: &mut State) {
    let totals = preview.totals();
    let rail_w = d.px(360.0);
    let full = ui.available_rect_before_wrap();
    let split = full.right() - rail_w;

    // 左：工具行 + 变化树
    let mut left = child(
        ui,
        Rect::from_min_max(full.left_top(), pos2(split, full.bottom())),
        Align::Min,
    );
    left.set_min_size(vec2(split - full.left(), full.height()));
    left.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        tree_toolbar(ui, t, d, &totals, state);
        ScrollArea::vertical()
            .id_salt("model-update-change-tree")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(d.px(2.0));
                for db in &preview.dbnums {
                    db_rows(ui, t, d, db, state);
                }
                ui.add_space(d.px(8.0));
            });
    });

    // 右：本次范围 + 阻断与提示
    let rail = Rect::from_min_max(pos2(split, full.top()), full.right_bottom());
    ui.painter().rect_filled(rail, CornerRadius::ZERO, t.bg_header);
    ui.painter().rect_filled(
        Rect::from_min_size(rail.left_top(), vec2(1.0, rail.height())),
        CornerRadius::ZERO,
        t.border,
    );
    let mut ui = child(ui, rail.shrink(d.px(16.0)), Align::Min);
    ui.set_min_size(rail.size() - Vec2::splat(d.px(32.0)));
    ScrollArea::vertical()
        .id_salt("model-update-summary-rail")
        .auto_shrink([false, false])
        .show(&mut ui, |ui| {
            summary_block(ui, t, d, &totals);
            ui.add_space(d.px(20.0));
            hints_block(ui, t, d, preview, &totals);
        });
}

fn tree_toolbar(ui: &mut Ui, t: &Tokens, d: Density, totals: &Totals, state: &mut State) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(38.0)), Sense::hover());
    let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    ui.label(
        RichText::new("待更新范围")
            .font(Font::strong(d))
            .color(t.text_primary),
    );
    // 排除的理由现在有三条，得分开数：非 DESI、MDB 声明但项目里没有文件，
    // 以及压根不在 MDB 里（那些库连行都没有，靠 desi 这个数与 MDB 一对就知道）。
    // 合成一个数的话，人看到自己知道的某个设计库不见了只会以为是 bug。
    let mut scope = format!("{} 个 DESI 设计库在范围内", totals.desi);
    if totals.not_in_project > 0 {
        scope.push_str(&format!(
            " · {} 个 MDB 声明但项目内无文件",
            totals.not_in_project
        ));
    }
    if totals.excluded > 0 {
        scope.push_str(&format!(" · {} 个非 DESI 已排除", totals.excluded));
    }
    ui.label(
        RichText::new(scope)
            .font(Font::mono_meta(d))
            .color(t.text_muted),
    );
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui
            .add(widgets::chip(
                t,
                d,
                "仅看影响模型",
                state.only_model_affecting,
            ))
            .on_hover_text("只列出会触发模型重新生成的交付单元")
            .clicked()
        {
            state.only_model_affecting = !state.only_model_affecting;
        }
    });
}

fn db_rows(ui: &mut Ui, t: &Tokens, d: Density, db: &DbPreview, state: &mut State) {
    let form = db.form();
    let (tone_fg, tone_bg) = t.status(form.tone());
    let key = format!("db{}", db.dbnum);
    // 会跑的库默认摊开——用户打开这个窗口就是来看它们的；不跑的默认收起。
    let expandable = form == DbForm::Pending && !db.units.is_empty();
    let open = expandable && state.open_row(&key, true);
    let head_label = if db.file_name.is_empty() {
        format!("db{} · {}", db.dbnum, db.db_type)
    } else {
        format!("{} · db{} · {}", db.file_name, db.dbnum, db.db_type)
    };

    let head = Row {
        depth: 0,
        caret: expandable.then_some(open),
        icon: form.icon(),
        icon_color: tone_fg,
        label: &head_label,
        label_color: if form.blocks() {
            t.danger
        } else if form == DbForm::Excluded || form == DbForm::UpToDate {
            t.text_muted
        } else {
            t.text_primary
        },
        strong: true,
        tag: form.label(),
        tag_color: tone_fg,
        // 不在范围内的库没有会话区间可说。`NotInProject` 尤其不能摆：它连文件都
        // 不在这个项目里，`window()` 要么算出 `sesno 1 → 0`，要么说「首次导入 ·
        // 尚无权威水位」——后者与同一行的「本期够不着」直接打架。
        meta: if form == DbForm::Excluded || form == DbForm::NotInProject {
            String::new()
        } else {
            db.window()
        },
        note: form.note(),
        note_color: if form.blocks() { t.danger } else { t.text_muted },
        fill: if form.blocks() {
            tone_bg
        } else {
            t.bg_header
        },
    };
    if row(ui, t, d, head).clicked() && expandable {
        state.toggle_row(&key);
    }

    if !open {
        return;
    }
    for unit in db
        .units
        .iter()
        .filter(|u| !state.only_model_affecting || u.will_generate)
    {
        let moved = match (unit.moved_in, unit.moved_out) {
            (0, 0) => String::new(),
            (i, 0) => format!("迁入 {i}"),
            (0, o) => format!("迁出 {o}"),
            (i, o) => format!("迁入 {i} · 迁出 {o}"),
        };
        let ulabel = format!("{} {}", unit.noun, unit.root_refno);
        let urow = Row {
            depth: 1,
            caret: None,
            icon: crate::workbench::noun_icon(&unit.noun),
            icon_color: if unit.will_generate {
                t.warn
            } else {
                t.text_muted
            },
            label: &ulabel,
            label_color: t.text_primary,
            strong: false,
            tag: unit.name.as_str(),
            tag_color: t.text_muted,
            meta: moved,
            note: if unit.will_generate { "待生成" } else { "不生成" },
            note_color: if unit.will_generate {
                t.warn
            } else {
                t.text_muted
            },
            fill: Color32::TRANSPARENT,
        };
        let _ = row(ui, t, d, urow);
    }
}

fn summary_block(ui: &mut Ui, t: &Tokens, d: Density, totals: &Totals) {
    section(ui, t, d, "本次范围");
    ui.add_space(d.px(10.0));
    ui.horizontal(|ui| {
        let w = ui.available_width();
        let each = (w - d.px(2.0) * 2.0) / 3.0;
        for (i, (value, label, color)) in [
            (totals.added, "新增", t.success),
            (totals.modified, "修改", t.warn),
            (totals.deleted, "删除", t.danger),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                let (line, _) = ui.allocate_exact_size(vec2(1.0, d.px(30.0)), Sense::hover());
                ui.painter().rect_filled(line, CornerRadius::ZERO, t.border);
            }
            ui.allocate_ui(vec2(each - d.px(8.0), d.px(44.0)), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        RichText::new(value.to_string())
                            .font(FontId::new(d.px(22.0), FontFamily::Monospace))
                            .color(color),
                    );
                    ui.label(RichText::new(label).font(Font::meta(d)).color(t.text_muted));
                });
            });
        }
    });
    ui.add_space(d.px(10.0));
    ratio_bar(
        ui,
        t,
        d,
        &[
            (totals.added, t.success),
            (totals.modified, t.warn),
            (totals.deleted, t.danger),
        ],
    );
    ui.add_space(d.px(10.0));
    stat_row(
        ui,
        t,
        d,
        ph::STACK,
        t.accent,
        "受影响交付单元",
        &totals.units.to_string(),
        true,
    );
    ui.add_space(d.px(8.0));
    stat_row(
        ui,
        t,
        d,
        ph::SHAPES,
        t.text_muted,
        "其中影响模型的变化",
        &totals.model_affecting.to_string(),
        false,
    );
}

fn hints_block(ui: &mut Ui, t: &Tokens, d: Density, preview: &Preview, totals: &Totals) {
    section(ui, t, d, "阻断与提示");
    ui.add_space(d.px(10.0));
    let mut any = false;

    for db in preview.dbnums.iter().filter(|db| db.form().blocks()) {
        any = true;
        let form = db.form();
        note_card(
            ui,
            t,
            d,
            Status::Error,
            ph::WARNING_CIRCLE,
            &format!("db{} {}，本次跳过", db.dbnum, form.label()),
            &db.anomaly
                .as_ref()
                .map(FileAnomaly::to_string)
                .unwrap_or_else(|| "文件异常，本次不执行这个 dbnum。水位不变。".into()),
        );
        ui.add_space(d.px(10.0));
    }

    for db in preview
        .dbnums
        .iter()
        .filter(|db| db.form() == DbForm::PathMigrated)
    {
        any = true;
        note_card(
            ui,
            t,
            d,
            Status::Warn,
            ph::GIT_MERGE,
            &format!("db{} 路径迁移，照常执行", db.dbnum),
            &db.anomaly
                .as_ref()
                .map(FileAnomaly::to_string)
                .unwrap_or_default(),
        );
        ui.add_space(d.px(10.0));
    }

    if totals.no_generation > 0 {
        any = true;
        note_card(
            ui,
            t,
            d,
            Status::Warn,
            ph::WARNING,
            &format!("{} 项变化找不到可生成的交付单元", totals.no_generation),
            "已解析的最小交付单元照常生成，这些跳过并告警（no_generation）。",
        );
        ui.add_space(d.px(10.0));
    }

    if !preview.pending_model_retries.is_empty() {
        any = true;
        let list = preview
            .pending_model_retries
            .iter()
            .map(|u| {
                format!(
                    "{} {}（已尝试 {} 次）",
                    u.noun, u.root_refno, u.attempts
                )
            })
            .collect::<Vec<_>>()
            .join("；");
        note_card(
            ui,
            t,
            d,
            Status::Info,
            ph::ARROW_COUNTER_CLOCKWISE,
            &format!(
                "{} 个待重试单元将合并",
                preview.pending_model_retries.len()
            ),
            &format!("{list}。上次生成失败，本次按最新状态只生成一次。"),
        );
        ui.add_space(d.px(10.0));
    }

    for warning in &preview.warnings {
        any = true;
        note_card(ui, t, d, Status::Warn, ph::WARNING, "预览期告警", warning);
        ui.add_space(d.px(10.0));
    }

    if !any {
        ui.label(
            RichText::new("没有阻断，也没有需要交代的例外。")
                .font(Font::meta(d))
                .color(t.text_muted),
        );
    }
}

// ================================================================ 第二步：确认执行

/// S2-B。它不发任何请求，存在的唯一理由是**执行范围不等于预览列表**：
/// 阻断的库不跑、非 DESI 压根不在范围内、需初始化的库会跑、上次失败的单元会并入。
fn confirm_body(ui: &mut Ui, t: &Tokens, d: Density, preview: &Preview, state: &State) {
    let totals = preview.totals();
    let previewed_at = state
        .cache
        .as_ref()
        .map(|c| format!("预览完成于 {}；", ago(c.at.elapsed())))
        .unwrap_or_default();

    scroll_pad(ui, d, "model-update-confirm", |ui| {
        ui.spacing_mut().item_spacing.y = d.px(12.0);

        card_titled(
            ui,
            t,
            d,
            ph::PLAY,
            t.accent,
            "本次会执行",
            &format!("{} 个数据批次 · {} 个交付单元", totals.batches, totals.units),
            |ui| {
                for db in preview.dbnums.iter().filter(|db| db.will_run()) {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Tone(t.accent),
                        &format!("db{} · {}", db.dbnum, db.db_type),
                        &db.window(),
                        &if db.initialization_required {
                            "首次导入 · 全量建立水位".to_owned()
                        } else {
                            format!(
                                "{} 个会话 · {} 项变化",
                                db.sessions.len(),
                                db.changes()
                            )
                        },
                        t.text_secondary,
                    );
                }
                line(
                    ui,
                    t,
                    d,
                    Dot::Glyph(ph::STACK, t.accent),
                    &format!("{} 个最小交付单元将重新生成", totals.units),
                    "",
                    "本期执行范围：当前 MDB 声明的 DESI 库 + sesno",
                    t.text_muted,
                );
                ui.add_space(d.px(6.0));
                strip(
                    ui,
                    t,
                    d,
                    Status::Info,
                    ph::GIT_MERGE,
                    &format!(
                        "{previewed_at}开始时会重新扫描，期间新产生的会话自动并入本批次，结果摘要里会逐条列出"
                    ),
                );
            },
        );

        let not_running = totals.blocked
            + totals.excluded
            + totals.not_in_project
            + usize::from(totals.no_generation > 0);
        card_titled(
            ui,
            t,
            d,
            ph::PROHIBIT,
            t.text_muted,
            "本次不会执行",
            &format!("{not_running} 项"),
            |ui| {
                for db in preview.dbnums.iter().filter(|db| db.form().blocks()) {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Tone(t.danger),
                        &format!("db{} · {}", db.dbnum, db.db_type),
                        db.form().label(),
                        "已阻断 · 水位不变",
                        t.danger,
                    );
                }
                for db in preview
                    .dbnums
                    .iter()
                    .filter(|db| db.form() == DbForm::Excluded)
                {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Tone(t.text_muted),
                        &format!("db{} · {}", db.dbnum, db.db_type),
                        "非 DESI，本期不处理",
                        "不在执行范围",
                        t.text_muted,
                    );
                }
                for db in preview
                    .dbnums
                    .iter()
                    .filter(|db| db.form() == DbForm::NotInProject)
                {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Tone(t.text_muted),
                        &format!("db{} · {}", db.dbnum, db.db_type),
                        "MDB 声明了它，当前项目目录里没有这个文件",
                        "本期扫描够不着",
                        t.text_muted,
                    );
                }
                if totals.no_generation > 0 {
                    line(
                        ui,
                        t,
                        d,
                        Dot::Glyph(ph::WARNING, t.warn),
                        &format!("{} 项变化找不到可生成的交付单元", totals.no_generation),
                        "",
                        "计入 no_generation 并告警",
                        t.warn,
                    );
                }
                if not_running == 0 {
                    ui.label(
                        RichText::new("没有阻断，也没有被排除的库。")
                            .font(Font::meta(d))
                            .color(t.text_muted),
                    );
                }
            },
        );

        if !preview.pending_model_retries.is_empty() {
            card_titled(
                ui,
                t,
                d,
                ph::ARROW_COUNTER_CLOCKWISE,
                t.accent,
                "会一并处理",
                &format!("{} 个待重试单元", preview.pending_model_retries.len()),
                |ui| {
                    for unit in &preview.pending_model_retries {
                        line(
                            ui,
                            t,
                            d,
                            Dot::Tone(t.accent),
                            &format!("{} {}", unit.noun, unit.root_refno),
                            &format!(
                                "上次生成失败 · 已尝试 {} 次 · 来源会话 {}",
                                unit.attempts, unit.source_end_sesno
                            ),
                            "按最新状态只生成一次",
                            t.text_secondary,
                        );
                    }
                },
            );
        }

        note_card(
            ui,
            t,
            d,
            Status::Warn,
            ph::WARNING,
            "开始后不能中途取消",
            "数据批次一旦成功，已应用会话号立即推进且不回滚；模型生成失败的单元转入待重试，不影响已写入的数据。关闭窗口不会停任务。",
        );
    });
}

// ================================================================ 第三步：执行进度

fn running_body(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    session: &RunVm,
    state: &State,
    cmds: &mut Vec<Cmd>,
) {
    if session.run.terminal() {
        return outcome_body(ui, t, d, session, state, cmds);
    }
    let progress = &session.progress;

    // S4-B：连接状态与运行状态分开说。连接断了运行照跑，混成一句会让人以为任务出了问题。
    if let Feed::Down(reason) = &progress.feed {
        let behind = progress.behind(session.run.events_seen);
        feed_banner(
            ui,
            t,
            d,
            &format!(
                "服务端已发 {} 条进度事件，本端落后 {behind} 条（{reason}）· 断开期间的逐单元事件不会补发",
                session.run.events_seen
            ),
            cmds,
        );
    }
    if let Some(error) = session.poll_error.as_deref() {
        pad_h(ui, d, |ui| {
            strip(
                ui,
                t,
                d,
                Status::Warn,
                ph::WARNING,
                &format!("任务状态查询暂时失败：{error}"),
            );
        });
    }

    scroll_pad(ui, d, "model-update-progress", |ui| {
        ui.spacing_mut().item_spacing.y = d.px(12.0);
        let cached = state.cached();

        let phase1_note = if progress.batch_phase_done() {
            match progress.batch_elapsed() {
                Some(elapsed) => format!("已完成 · 用时 {}", secs(elapsed)),
                None => "已完成".into(),
            }
        } else {
            counted(session.run.batches_done, session.run.total_batches, "数据批次")
                .unwrap_or_else(|| format!("已完成 {} 个批次", done_batches(progress)))
        };
        card_titled(
            ui,
            t,
            d,
            ph::CHECK_CIRCLE,
            if progress.batch_phase_done() {
                t.success
            } else {
                t.accent
            },
            "阶段一 · 数据更新",
            &phase1_note,
            |ui| {
                if progress.batches.is_empty() {
                    ui.label(
                        RichText::new("还没有收到数据批次事件。")
                            .font(Font::meta(d))
                            .color(t.text_muted),
                    );
                }
                for b in &progress.batches {
                    let (note, color) = match b.state {
                        RowState::Done => ("已应用，水位已推进", t.success),
                        RowState::Failed => ("失败 · 水位不变", t.danger),
                        RowState::Unknown => ("状态未知", t.text_muted),
                        RowState::Running => ("进行中", t.accent),
                    };
                    line(
                        ui,
                        t,
                        d,
                        Dot::state(b.state, t),
                        &format!("db{}", b.dbnum),
                        &format!("sesno {} → {}", b.start_sesno, b.end_sesno),
                        note,
                        color,
                    );
                    if let Some(message) = b.message.as_deref() {
                        sub_line(ui, t, d, message, b.state == RowState::Failed);
                    }
                }
                // 阻断与排除的库不发事件，它们的去向要从预览那份里补出来，
                // 否则「这库怎么没动静」只能靠猜。
                if let Some(preview) = cached {
                    for db in preview
                        .dbnums
                        .iter()
                        .filter(|db| db.form().blocks() || db.form() == DbForm::Excluded)
                    {
                        let blocked = db.form().blocks();
                        line(
                            ui,
                            t,
                            d,
                            Dot::Glyph(
                                if blocked { ph::WARNING_CIRCLE } else { ph::PROHIBIT },
                                if blocked { t.danger } else { t.text_muted },
                            ),
                            &format!("db{} · {}", db.dbnum, db.db_type),
                            if blocked {
                                db.form().label()
                            } else {
                                "非 DESI，不产生数据批次"
                            },
                            db.form().note(),
                            if blocked { t.danger } else { t.text_muted },
                        );
                    }
                }
            },
        );

        // 断线降级的是列表区，不是这个计数——它走轮询通道，连接断了照样准
        //（ADR-0007 推翻了 ADR-0005 里「条冻结在最后已知值」的设想）。
        let units_note = counted(session.run.units_done, session.run.total_units, "交付单元")
            .unwrap_or_else(|| format!("本端已见 {} 个交付单元生成完成", progress.units_done()));
        card_titled(
            ui,
            t,
            d,
            ph::SHAPES,
            t.accent,
            "阶段二 · 模型生成",
            &units_note,
            |ui| {
                // 有权威分母才画确定态的条；没有就只报已完成数（ADR-0007）。
                if let (Some(done), Some(total)) =
                    (session.run.units_done, session.run.total_units)
                    && total > 0
                {
                    let failed = progress
                        .units
                        .iter()
                        .filter(|u| u.state == RowState::Failed)
                        .count() as u32;
                    ratio_bar(
                        ui,
                        t,
                        d,
                        &[
                            (done.saturating_sub(failed), t.accent),
                            (failed.min(done), t.danger),
                            (total.saturating_sub(done), Color32::TRANSPARENT),
                        ],
                    );
                    ui.add_space(d.px(6.0));
                    ui.label(
                        RichText::new("这是单元数进度，不是时间进度——一个 BRAN 和一个 EQUI 的生成耗时差一个数量级。")
                            .font(Font::micro(d))
                            .color(t.text_muted),
                    );
                    ui.add_space(d.px(6.0));
                }
                if let Some(preview) = cached {
                    let no_gen = preview.totals().no_generation;
                    if no_gen > 0 {
                        strip(
                            ui,
                            t,
                            d,
                            Status::Neutral,
                            ph::INFO,
                            &format!(
                                "预览时另有 {no_gen} 处变化找不到可生成的交付单元（no_generation），不会出现在下方列表"
                            ),
                        );
                        ui.add_space(d.px(6.0));
                    }
                }
                if progress.units.is_empty() {
                    ui.label(
                        RichText::new("还没有交付单元开始生成。")
                            .font(Font::meta(d))
                            .color(t.text_muted),
                    );
                }
                for u in &progress.units {
                    let (note, color) = match u.state {
                        RowState::Done => ("生成成功".to_owned(), t.success),
                        RowState::Failed => ("生成失败 · 待重试".to_owned(), t.danger),
                        RowState::Unknown => ("状态未知".to_owned(), t.text_muted),
                        // 跑满 10 秒才报用时：短单元上那个数字每帧都在跳，只会晃眼。
                        RowState::Running if u.started.elapsed() >= Duration::from_secs(10) => {
                            (format!("生成中 · 已用 {}", secs(u.started.elapsed())), t.accent)
                        }
                        RowState::Running => ("生成中".to_owned(), t.accent),
                    };
                    line(
                        ui,
                        t,
                        d,
                        Dot::state(u.state, t),
                        &format!("{} {}", u.noun, u.root_refno),
                        &format!("db{}", u.dbnum),
                        &note,
                        color,
                    );
                    if let Some(message) = u.message.as_deref() {
                        sub_line(ui, t, d, message, u.state == RowState::Failed);
                    }
                }
            },
        );
    });
}

/// S4-C 终态。`partial` 是常态：数据批次一旦 `Applied`，水位不因后面的模型生成失败而回退。
fn outcome_body(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    session: &RunVm,
    state: &State,
    cmds: &mut Vec<Cmd>,
) {
    let run = &session.run;
    let Some(outcome) = run.result.as_ref() else {
        return pad(ui, d, |ui| {
            note_card(
                ui,
                t,
                d,
                Status::Warn,
                ph::WARNING,
                &format!("任务已结束（{}），但还没拿到结果摘要", run.state),
                "任务详情里没有 result。稍后重新预览可以看到最新水位。",
            );
        });
    };
    let applied = outcome
        .batch
        .iter()
        .filter(|b| b.status == BatchStatus::Applied)
        .count();
    let failed_batches = outcome
        .batch
        .iter()
        .filter(|b| b.status == BatchStatus::Failed)
        .count();
    let skipped = outcome.batch.iter().count() - applied - failed_batches;
    let failed_units: Vec<&UnitResult> = outcome
        .units
        .iter()
        .filter(|u| u.status == UnitStatus::Failed)
        .collect();
    let ok_units = outcome.units.len() - failed_units.len();

    scroll_pad(ui, d, "model-update-outcome", |ui| {
        ui.spacing_mut().item_spacing.y = d.px(12.0);

        let (tone, icon, title, body) = match outcome.status {
            RunStatus::Partial => (
                Status::Warn,
                ph::WARNING,
                format!("部分完成：数据全部落库，模型少了 {} 个交付单元", failed_units.len()),
                "服务端按「有成功也有失败」判定为 partial。已应用的水位不因模型失败而回退，未生成的单元单独登记为待重试。".to_owned(),
            ),
            RunStatus::Success => (
                Status::Success,
                ph::CHECK_CIRCLE,
                "全部完成".to_owned(),
                "数据批次与模型交付单元都成功了，水位已推进。".to_owned(),
            ),
            RunStatus::Failed => (
                Status::Error,
                ph::WARNING_CIRCLE,
                "未完成：没有任何一件事做成".to_owned(),
                "有可执行的工作，但一件都没成功。水位没有推进。".to_owned(),
            ),
            RunStatus::UpToDate => (
                Status::Neutral,
                ph::CHECK_CIRCLE,
                "无事可做".to_owned(),
                "这次运行开始时已经没有可执行的数据批次与待重试单元。".to_owned(),
            ),
        };
        note_card(ui, t, d, tone, icon, &title, &body);

        card_titled(
            ui,
            t,
            d,
            ph::CHECK_CIRCLE,
            if failed_batches > 0 { t.danger } else { t.success },
            "阶段一 · 数据批次",
            &format!("{applied} 已应用 · {skipped} 阻断 · {failed_batches} 失败"),
            |ui| {
                if outcome.batch.is_none() {
                    ui.label(
                        RichText::new("这次没有数据批次（只跑了模型重试）。")
                            .font(Font::meta(d))
                            .color(t.text_muted),
                    );
                }
                if let Some(b) = &outcome.batch {
                    let (dot, note, color) = match b.status {
                        BatchStatus::Applied => (
                            Dot::Tone(t.success),
                            format!("已应用 · 水位推进至 {}", b.end_sesno),
                            t.success,
                        ),
                        BatchStatus::Failed => (
                            Dot::Tone(t.danger),
                            "失败 · 水位不变".to_owned(),
                            t.danger,
                        ),
                        BatchStatus::Skipped => (
                            Dot::Glyph(ph::PROHIBIT, t.text_muted),
                            "未执行 · 水位不变".to_owned(),
                            t.text_muted,
                        ),
                    };
                    let window = if b.status == BatchStatus::Skipped {
                        b.message.clone().unwrap_or_else(|| "本期不执行".into())
                    } else {
                        format!("sesno {} → {}", b.start_sesno, b.end_sesno)
                    };
                    line(
                        ui,
                        t,
                        d,
                        dot,
                        &format!("db{} · {}", b.dbnum, b.db_type),
                        &window,
                        &note,
                        color,
                    );
                    // 契约明写结果摘要必须逐条列出并入的会话——这是「执行范围 ≠ 预览列表」
                    // 在终态上的最后一次交代。
                    if !b.merged_sesnos.is_empty() {
                        let listed = b
                            .merged_sesnos
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(" / ");
                        let diff = state
                            .cached()
                            .and_then(|p| p.dbnums.iter().find(|db| db.dbnum == b.dbnum))
                            .map(|db| {
                                format!(
                                    " —— 实际执行 {} 项变化，预览时为 {} 项",
                                    b.changed_elements,
                                    db.changes()
                                )
                            })
                            .unwrap_or_default();
                        strip(
                            ui,
                            t,
                            d,
                            Status::Info,
                            ph::GIT_MERGE,
                            &format!(
                                "预览后并入 {} 个会话：{listed}{diff}",
                                b.merged_sesnos.len()
                            ),
                        );
                    }
                    if let Some(message) = b.message.as_deref()
                        && b.status == BatchStatus::Failed
                    {
                        sub_line(ui, t, d, message, true);
                    }
                }
            },
        );

        card_titled(
            ui,
            t,
            d,
            if failed_units.is_empty() {
                ph::CHECK_CIRCLE
            } else {
                ph::WARNING
            },
            if failed_units.is_empty() {
                t.success
            } else {
                t.warn
            },
            "阶段二 · 模型交付单元",
            &format!("{ok_units} 成功 · {} 失败", failed_units.len()),
            |ui| {
                for u in &failed_units {
                    let attempts = format!("第 {} 次尝试", u.attempts);
                    if line_with_retry(
                        ui,
                        t,
                        d,
                        &format!("{} {}", u.noun, u.root_refno),
                        &format!(
                            "db{}  {}",
                            u.dbnum,
                            u.message.as_deref().unwrap_or("生成失败")
                        ),
                        &attempts,
                    ) {
                        cmds.push(Cmd::RetryModelUnit {
                            root_refno: u.root_refno.clone(),
                        });
                    }
                }
                if ok_units > 0 {
                    let owners = outcome
                        .units
                        .iter()
                        .filter(|u| u.status == UnitStatus::Generated)
                        .filter(|u| u.old_owner.is_some() || u.new_owner.is_some())
                        .count();
                    line(
                        ui,
                        t,
                        d,
                        Dot::Glyph(ph::CHECK_CIRCLE, t.success),
                        &format!("其余 {ok_units} 个单元已生成"),
                        "",
                        &if owners > 0 {
                            format!("{owners} 个单元换了归属，模型树需要刷新")
                        } else {
                            String::new()
                        },
                        t.text_muted,
                    );
                }
                if outcome.units.is_empty() {
                    ui.label(
                        RichText::new("这次没有交付单元需要生成。")
                            .font(Font::meta(d))
                            .color(t.text_muted),
                    );
                }
            },
        );

        for warning in &outcome.warnings {
            strip(ui, t, d, Status::Warn, ph::WARNING, warning);
        }
    });
}

// ================================================================ 失败态

/// S2-D。按 `code` 分型给出「原因 / 影响 / 一个出路」；
/// **只有 internal 那一类**才把原始 message 摊出来，且收进「详情」默认折起。
fn failure_body(ui: &mut Ui, t: &Tokens, d: Density, failure: &Failure, state: &mut State) {
    let form = failure.form();
    pad(ui, d, |ui| {
        ui.spacing_mut().item_spacing.y = d.px(12.0);
        note_card(ui, t, d, form.tone(), form.icon(), form.title(), form.body());

        if form == FailForm::Internal {
            let label = if state.detail_open {
                "收起详情"
            } else {
                "详情"
            };
            if ui.add(widgets::chip(t, d, label, state.detail_open)).clicked() {
                state.detail_open = !state.detail_open;
            }
            if state.detail_open {
                ui.add_space(d.px(8.0));
                card(ui, t, d, |ui| {
                    ui.label(
                        RichText::new(&failure.message)
                            .font(Font::mono_meta(d))
                            .color(t.text_secondary),
                    );
                    if let Some(detail) = failure.detail.as_deref() {
                        ui.add_space(d.px(6.0));
                        ui.label(
                            RichText::new(detail)
                                .font(Font::mono_micro(d))
                                .color(t.text_muted),
                        );
                    }
                });
            }
        } else if form == FailForm::Conflict && !failure.message.is_empty() {
            // conflict 的 message 里带着那个 task_id，它是人接下来唯一用得上的东西。
            ui.label(
                RichText::new(&failure.message)
                    .font(Font::mono_meta(d))
                    .color(t.text_muted),
            );
        }
    });
}

// ================================================================ 零件

enum Corner {
    Top,
    Bottom,
    None,
}

/// 一条通栏横条。返回它的矩形，调用点在里面摆内容。
fn bar(ui: &mut Ui, h: f32, fill: Color32, corner: Corner) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let r = radius::LG;
    let cr = match corner {
        Corner::Top => CornerRadius {
            nw: r,
            ne: r,
            sw: 0,
            se: 0,
        },
        Corner::Bottom => CornerRadius {
            nw: 0,
            ne: 0,
            sw: r,
            se: r,
        },
        Corner::None => CornerRadius::ZERO,
    };
    ui.painter().rect_filled(rect, cr, fill);
    rect
}

fn hairline(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_filled(
        Rect::from_min_size(pos2(rect.left(), rect.bottom() - 1.0), vec2(rect.width(), 1.0)),
        CornerRadius::ZERO,
        color,
    );
}

fn child(ui: &mut Ui, rect: Rect, align: Align) -> Ui {
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(align)),
    )
}

fn pad<R>(ui: &mut Ui, d: Density, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(Margin::same(d.px(16.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn pad_h<R>(ui: &mut Ui, d: Density, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(d.px(16.0) as i8, d.px(8.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn scroll_pad(ui: &mut Ui, d: Density, salt: &str, add: impl FnOnce(&mut Ui)) {
    ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .show(ui, |ui| pad(ui, d, add));
}

fn center_note(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    icon: Option<&str>,
    title: &str,
    detail: &str,
) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - d.px(56.0)).max(d.px(40.0)));
        ui.spacing_mut().item_spacing.y = d.px(8.0);
        match icon {
            Some(icon) => ui.label(
                RichText::new(icon)
                    .font(FontId::new(d.px(26.0), FontFamily::Proportional))
                    .color(t.text_muted),
            ),
            None => ui.add(egui::Spinner::new().size(d.px(26.0)).color(t.accent)),
        };
        ui.label(
            RichText::new(title)
                .font(Font::strong(d))
                .color(t.text_secondary),
        );
        ui.label(RichText::new(detail).font(Font::meta(d)).color(t.text_muted));
    });
}

fn card<R>(ui: &mut Ui, t: &Tokens, d: Density, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(t.bg_elevated)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(d.px(14.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

#[allow(clippy::too_many_arguments)]
fn card_titled(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    icon: &str,
    icon_color: Color32,
    title: &str,
    note: &str,
    add: impl FnOnce(&mut Ui),
) {
    card(ui, t, d, |ui| {
        ui.horizontal(|ui| {
            icon_label(ui, t, d, icon, icon_color, title, t.text_primary);
            if !note.is_empty() {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(note)
                            .font(Font::mono_meta(d))
                            .color(t.text_secondary),
                    );
                });
            }
        });
        ui.add_space(d.px(8.0));
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui);
    });
}

fn icon_label(
    ui: &mut Ui,
    _t: &Tokens,
    d: Density,
    icon: &str,
    icon_color: Color32,
    text: &str,
    text_color: Color32,
) {
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(d.px(15.0), d.px(15.0)), Sense::hover());
    glyph(ui, rect.center(), icon, d.px(14.0), icon_color);
    ui.label(RichText::new(text).font(Font::strong(d)).color(text_color));
}

fn note_card(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    tone: Status,
    icon: &str,
    title: &str,
    detail: &str,
) {
    let (fg, bg) = t.status(tone);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(d.px(10.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(10.0);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(d.px(14.0), d.px(18.0)), Sense::hover());
                glyph(ui, rect.center(), icon, d.px(14.0), fg);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = d.px(4.0);
                    ui.label(RichText::new(title).font(Font::strong(d)).color(fg));
                    if !detail.is_empty() {
                        ui.label(
                            RichText::new(detail)
                                .font(Font::meta(d))
                                .color(t.text_secondary),
                        );
                    }
                });
            });
        });
}

/// 卡片内的一条浅底提示。比 `note_card` 轻，用来解释上一行是怎么回事。
fn strip(ui: &mut Ui, t: &Tokens, d: Density, tone: Status, icon: &str, text: &str) {
    let (fg, bg) = t.status(tone);
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(radius::SM))
        .inner_margin(Margin::symmetric(d.px(10.0) as i8, d.px(7.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(8.0);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(d.px(13.0), d.px(15.0)), Sense::hover());
                glyph(ui, rect.center(), icon, d.px(12.0), fg);
                ui.label(
                    RichText::new(text)
                        .font(Font::meta(d))
                        .color(t.text_secondary),
                );
            });
        });
}

fn section(ui: &mut Ui, t: &Tokens, d: Density, title: &str) {
    ui.label(
        RichText::new(title)
            .font(Font::strong(d))
            .color(t.text_muted),
    );
}

fn stat_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    icon: &str,
    icon_color: Color32,
    label: &str,
    value: &str,
    strong: bool,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = d.px(8.0);
        let (rect, _) = ui.allocate_exact_size(vec2(d.px(13.0), d.px(13.0)), Sense::hover());
        glyph(ui, rect.center(), icon, d.px(13.0), icon_color);
        ui.label(
            RichText::new(label)
                .font(Font::body(d))
                .color(t.text_secondary),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .font(if strong {
                        Font::mono(d)
                    } else {
                        Font::mono_meta(d)
                    })
                    .color(if strong {
                        t.text_primary
                    } else {
                        t.text_secondary
                    }),
            );
        });
    });
}

/// 按比例铺开的横条。分布条与进度条是同一个零件：都只是「几段各占多少」。
fn ratio_bar(ui: &mut Ui, t: &Tokens, d: Density, segments: &[(u32, Color32)]) {
    let h = d.px(8.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let cr = CornerRadius::same((h / 2.0) as u8);
    ui.painter().rect_filled(rect, cr, t.bg_input);
    let total: u32 = segments.iter().map(|(v, _)| *v).sum();
    if total == 0 {
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let mut x = rect.left();
    for (value, color) in segments {
        let w = rect.width() * (*value as f32) / (total as f32);
        if *color != Color32::TRANSPARENT && w > 0.0 {
            painter.rect_filled(
                Rect::from_min_size(pos2(x, rect.top()), vec2(w, h)),
                cr,
                *color,
            );
        }
        x += w;
    }
}

/// 行首那枚状态标记。
enum Dot {
    Tone(Color32),
    Glyph(&'static str, Color32),
}

impl Dot {
    fn state(state: RowState, t: &Tokens) -> Self {
        match state {
            RowState::Done => Self::Glyph(ph::CHECK_CIRCLE, t.success),
            RowState::Failed => Self::Glyph(ph::X_CIRCLE, t.danger),
            // 没收到事件就不知道它开没开始，画一个空圈，不许画成「排队中」。
            RowState::Unknown => Self::Glyph(ph::CIRCLE, t.text_muted),
            RowState::Running => Self::Glyph(ph::ARROWS_CLOCKWISE, t.accent),
        }
    }
}

/// 卡片里的一条明细行：标记 + 等宽标识 + 次要说明 + 右对齐的状态词。
fn line(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    dot: Dot,
    label: &str,
    detail: &str,
    note: &str,
    note_color: Color32,
) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(28.0)), Sense::hover());
    let mut ui = child(ui, rect, Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(14.0), d.px(14.0)), Sense::hover());
    match dot {
        Dot::Tone(color) => {
            ui.painter().circle_filled(mark.center(), d.px(4.0), color);
        }
        Dot::Glyph(icon, color) => glyph(&ui, mark.center(), icon, d.px(13.0), color),
    }
    ui.label(
        RichText::new(label)
            .font(Font::mono(d))
            .color(t.text_primary),
    );
    if !detail.is_empty() {
        ui.label(
            RichText::new(detail)
                .font(Font::mono_meta(d))
                .color(t.text_muted),
        );
    }
    if !note.is_empty() {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(note).font(Font::meta(d)).color(note_color));
        });
    }
}

/// 带「重试」按钮的失败行。返回按钮是否被按下。
fn line_with_retry(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    label: &str,
    detail: &str,
    attempts: &str,
) -> bool {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(34.0)), Sense::hover());
    let mut ui = child(ui, rect, Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(14.0), d.px(14.0)), Sense::hover());
    glyph(&ui, mark.center(), ph::X_CIRCLE, d.px(13.0), t.danger);
    ui.label(
        RichText::new(label)
            .font(Font::mono(d))
            .color(t.text_primary),
    );
    ui.label(
        RichText::new(detail)
            .font(Font::mono_meta(d))
            .color(t.text_muted),
    );
    let mut clicked = false;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        clicked = ui
            .add(widgets::button(t, d, "重试").icon(ph::ARROW_COUNTER_CLOCKWISE))
            .on_hover_text("重新生成这个交付单元；幂等，可以反复按")
            .clicked();
        ui.label(
            RichText::new(attempts)
                .font(Font::mono_meta(d))
                .color(t.danger),
        );
    });
    clicked
}

fn sub_line(ui: &mut Ui, t: &Tokens, d: Density, text: &str, danger: bool) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(20.0)), Sense::hover());
    let mut ui = child(ui, rect.shrink2(vec2(d.px(22.0), 0.0)), Align::Center);
    ui.label(
        RichText::new(text)
            .font(Font::micro(d))
            .color(if danger { t.danger } else { t.text_muted }),
    );
}

/// 变化树的一行。设计稿上它比通用 `tree_row` 多一段「行尾状态词」，
/// 而状态词恰恰是这棵树最要紧的一列，所以单独画。
struct Row<'a> {
    depth: usize,
    caret: Option<bool>,
    icon: &'a str,
    icon_color: Color32,
    label: &'a str,
    label_color: Color32,
    strong: bool,
    tag: &'a str,
    tag_color: Color32,
    meta: String,
    note: &'a str,
    note_color: Color32,
    fill: Color32,
}

fn row(ui: &mut Ui, t: &Tokens, d: Density, r: Row<'_>) -> egui::Response {
    let h = d.px(30.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let bg = if r.fill != Color32::TRANSPARENT {
        r.fill
    } else if resp.hovered() {
        t.bg_hover
    } else {
        Color32::TRANSPARENT
    };
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    }

    let gap = d.px(6.0);
    let mut x = rect.left() + d.px(10.0) + r.depth as f32 * d.px(16.0);
    if let Some(open) = r.caret {
        let icon = if open { ph::CARET_DOWN } else { ph::CARET_RIGHT };
        glyph(
            ui,
            pos2(x + d.px(6.0), rect.center().y),
            icon,
            d.px(12.0),
            t.text_muted,
        );
    }
    x += d.px(12.0) + gap;
    glyph(
        ui,
        pos2(x + d.px(7.0), rect.center().y),
        r.icon,
        d.px(14.0),
        r.icon_color,
    );
    x += d.px(14.0) + gap;

    // 右侧先量后画：状态词与 meta 占了多少，决定了标识能写到哪。
    let mut right = rect.right() - d.px(12.0);
    if !r.note.is_empty() {
        let g = layout(ui, r.note, Font::meta(d));
        right -= g.size().x;
        ui.painter().galley(
            pos2(right, rect.center().y - g.size().y / 2.0),
            g,
            r.note_color,
        );
        right -= gap * 2.0;
    }
    if !r.meta.is_empty() {
        let g = layout(ui, &r.meta, Font::mono_meta(d));
        right -= g.size().x;
        ui.painter().galley(
            pos2(right, rect.center().y - g.size().y / 2.0),
            g,
            t.text_muted,
        );
        right -= gap * 2.0;
    }

    let font = if r.strong {
        Font::strong(d)
    } else {
        Font::body(d)
    };
    let g = layout(ui, r.label, font);
    let clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right.max(x), rect.bottom()));
    let label_w = g.size().x;
    ui.painter().with_clip_rect(clip).galley(
        pos2(x, rect.center().y - g.size().y / 2.0),
        g,
        r.label_color,
    );
    if !r.tag.is_empty() {
        let tg = layout(ui, r.tag, Font::meta(d));
        let tx = x + label_w + gap * 2.0;
        ui.painter().with_clip_rect(clip).galley(
            pos2(tx, rect.center().y - tg.size().y / 2.0),
            tg,
            r.tag_color,
        );
    }
    resp
}

fn feed_banner(ui: &mut Ui, t: &Tokens, d: Density, detail: &str, cmds: &mut Vec<Cmd>) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(40.0)), Sense::hover());
    let (fg, bg) = t.status(Status::Warn);
    ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    hairline(ui.painter(), rect, t.border);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(14.0), d.px(14.0)), Sense::hover());
    glyph(&ui, mark.center(), ph::PLUGS, d.px(14.0), fg);
    ui.label(
        RichText::new("实时进度已断开，已改为每秒轮询任务状态")
            .font(Font::strong(d))
            .color(fg),
    );
    ui.label(
        RichText::new(detail)
            .font(Font::meta(d))
            .color(t.text_secondary),
    );
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui
            .add(widgets::button(t, d, "重连").icon(ph::ARROW_COUNTER_CLOCKWISE))
            .clicked()
        {
            cmds.push(Cmd::ReconnectModelUpdateFeed);
        }
    });
}

fn stopwatch(ui: &mut Ui, t: &Tokens, d: Density, label: &str, elapsed: Duration) {
    ui.label(
        RichText::new(format!("{label} {}", clock(elapsed)))
            .font(Font::mono_meta(d))
            .color(t.text_secondary),
    );
}

// ---------------------------------------------------------------- 绘制小工具

fn layout(ui: &Ui, text: &str, font: FontId) -> std::sync::Arc<egui::Galley> {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
}

fn glyph(ui: &Ui, center: egui::Pos2, text: &str, size: f32, color: Color32) {
    let g = layout(ui, text, FontId::new(size, FontFamily::Proportional));
    ui.painter().galley(center - g.size() / 2.0, g, color);
}

fn mono_at(ui: &Ui, center: egui::Pos2, text: &str, d: Density, color: Color32) {
    let g = layout(ui, text, Font::mono_meta(d));
    ui.painter().galley(center - g.size() / 2.0, g, color);
}

fn fill(ui: &Ui, rect: Rect, r: u8, bg: Color32, stroke: Option<Color32>) {
    ui.painter().rect_filled(rect, CornerRadius::same(r), bg);
    if let Some(s) = stroke {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(r),
            Stroke::new(1.0_f32, s),
            StrokeKind::Inside,
        );
    }
}

// ---------------------------------------------------------------- 数与时间

fn clock(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    format!("{:02}:{:02}", s / 60, s % 60)
}

fn secs(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    if s < 60 {
        format!("{s} 秒")
    } else {
        format!("{} 分 {} 秒", s / 60, s % 60)
    }
}

fn ago(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    match s {
        0..=59 => format!("{s} 秒前"),
        60..=3599 => format!("{} 分钟前", s / 60),
        _ => format!("{} 小时前", s / 3600),
    }
}

fn done_batches(progress: &Progress) -> usize {
    progress
        .batches
        .iter()
        .filter(|b| b.state == RowState::Done)
        .count()
}

/// 有权威分子分母才给「14 / 24」。缺任何一个就返回 None，让调用点退回只报已完成数
/// ——分母不许自己算（ADR-0007）。
fn counted(done: Option<u32>, total: Option<u32>, unit: &str) -> Option<String> {
    Some(format!("{} / {} 个{unit}", done?, total?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(json: &str) -> ProgressEvent {
        serde_json::from_str(json).expect("契约 payload 应当能解出来")
    }

    fn db(dbnum: u32, db_type: &str) -> DbPreview {
        DbPreview {
            dbnum,
            db_type: db_type.into(),
            ..Default::default()
        }
    }

    /// 一个「够不着」的库，**带 `initialization_required`**。
    ///
    /// 这个标志是这条 fixture 的全部意义所在：够不着的库正是从没导入过的那些
    /// （`/ALL` 的 29 个里 10 个没导入，除去磁盘上有文件的 1112，剩下 9 个就是它们），
    /// 没有权威水位，契约给的就是 `initialization_required = true`。少了它，
    /// 一个默认 `DbPreview` 本来就 `!will_run()`——用它去断言「够不着的不跑」，
    /// 断言会通过，但通过的理由与 `not_in_project` 毫无关系。
    fn unreachable_db(dbnum: u32) -> DbPreview {
        DbPreview {
            not_in_project: true,
            initialization_required: true,
            ..db(dbnum, "DESI")
        }
    }

    #[test]
    fn events_pair_up_into_rows() {
        let mut progress = Progress::default();
        progress.apply(event(
            r#"{"kind":"data_batch_started","dbnum":7997,"start_sesno":41,"end_sesno":42}"#,
        ));
        progress.apply(event(
            r#"{"kind":"model_unit_started","dbnum":7997,"root_refno":"24381/1","noun":"BRAN"}"#,
        ));
        progress.apply(event(
            r#"{"kind":"model_unit_finished","dbnum":7997,"root_refno":"24381/1",
                "success":false,"message":"生成失败"}"#,
        ));

        assert_eq!(progress.batches[0].state, RowState::Running);
        assert_eq!(progress.units[0].state, RowState::Failed);
        assert_eq!(progress.units[0].message.as_deref(), Some("生成失败"));
        assert_eq!(progress.received, 3);
    }

    /// 收尾事件按 `(dbnum, root_refno)` 回填。同一个 dbnum 下多个单元同时在跑时，
    /// 只按 dbnum 找会把状态写到错误的那一行上。
    #[test]
    fn finish_lands_on_the_matching_unit_not_the_latest_one() {
        let mut progress = Progress::default();
        for refno in ["24381/1", "24381/2"] {
            progress.apply(event(&format!(
                r#"{{"kind":"model_unit_started","dbnum":7997,"root_refno":"{refno}","noun":"BRAN"}}"#
            )));
        }
        progress.apply(event(
            r#"{"kind":"model_unit_finished","dbnum":7997,"root_refno":"24381/1",
                "success":true,"message":null}"#,
        ));

        assert_eq!(progress.units[0].state, RowState::Done);
        assert_eq!(progress.units[1].state, RowState::Running);
    }

    /// 断线之后没收尾的行只能说「状态未知」，不许继续显示「进行中」；
    /// 已经收过尾的行不受影响（ADR-0005）。
    #[test]
    fn disconnect_degrades_only_the_unfinished_rows() {
        let mut progress = Progress::default();
        progress.apply(event(
            r#"{"kind":"model_unit_started","dbnum":7997,"root_refno":"24381/1","noun":"BRAN"}"#,
        ));
        progress.apply(event(
            r#"{"kind":"model_unit_finished","dbnum":7997,"root_refno":"24381/1",
                "success":true,"message":null}"#,
        ));
        progress.apply(event(
            r#"{"kind":"model_unit_started","dbnum":7997,"root_refno":"24381/2","noun":"EQUI"}"#,
        ));
        progress.mark_unknown();

        assert_eq!(progress.units[0].state, RowState::Done);
        assert_eq!(progress.units[1].state, RowState::Unknown);
    }

    #[test]
    fn behind_never_goes_negative_when_the_client_is_ahead() {
        let mut progress = Progress::default();
        progress.apply(event(
            r#"{"kind":"data_batch_started","dbnum":1,"start_sesno":0,"end_sesno":1}"#,
        ));
        // 轮询与 WS 是两条通道，任务详情有可能比本端收到的还旧。
        assert_eq!(progress.behind(0), 0);
        assert_eq!(progress.behind(9), 8);
    }

    /// S2-E 的七种行形态。最要紧的两条：`PathMigrated` 是五种异常里唯一不阻断的，
    /// `initialization_required` 的库 `net_*` 全是 0 却确实会跑。
    #[test]
    fn seven_row_forms_split_by_whether_the_batch_runs() {
        let mut pending = db(8000, "DESI");
        pending.sessions.push(SessionPreview::default());
        assert_eq!(pending.form(), DbForm::Pending);
        assert!(pending.will_run());

        let mut init = db(8021, "DESI");
        init.initialization_required = true;
        assert_eq!(init.form(), DbForm::Initialize);
        assert!(init.will_run(), "需初始化的库没有会话区间，但它确实会跑");
        assert_ne!(init.form(), DbForm::UpToDate);

        let mut migrated = db(8044, "DESI");
        migrated.sessions.push(SessionPreview::default());
        migrated.anomaly = Some(FileAnomaly::PathMigrated {
            old_path: "a".into(),
            new_path: "b".into(),
        });
        assert_eq!(migrated.form(), DbForm::PathMigrated);
        assert!(!migrated.form().blocks());
        assert!(migrated.will_run());

        for (dbnum, anomaly, want) in [
            (
                8003,
                FileAnomaly::Rollback {
                    file_latest_sesno: 812,
                    applied_sesno: 1005,
                },
                DbForm::Rollback,
            ),
            (
                8107,
                FileAnomaly::TypeChanged {
                    stored_db_type: "DESI".into(),
                    observed_db_type: "CATA".into(),
                },
                DbForm::TypeChanged,
            ),
            (
                8150,
                FileAnomaly::Duplicate {
                    paths: vec!["a".into(), "b".into()],
                },
                DbForm::Duplicate,
            ),
            (8163, FileAnomaly::Missing { path: "a".into() }, DbForm::Missing),
        ] {
            let mut blocked = db(dbnum, "DESI");
            blocked.anomaly = Some(anomaly);
            blocked.blocked = true;
            assert_eq!(blocked.form(), want);
            assert!(want.blocks());
            assert!(!blocked.will_run());
        }

        let excluded = db(8191, "CATA");
        assert_eq!(excluded.form(), DbForm::Excluded);
        assert!(!excluded.will_run());
    }

    /// 「够不着」与「文件缺失」是两回事，最要紧的是**它不阻断**：MDB 声明了
    /// 一个库、当前项目目录里没有它的文件，这不是异常，只是本期扫描碰不到它。
    /// 合成一行的话，AvevaCatalogue 那 9 个模板库会顶着「已阻断」出现在界面上，
    /// 人会去找根本不存在的故障。
    #[test]
    fn declared_by_mdb_but_absent_from_the_project_is_not_a_missing_file() {
        let unreachable = unreachable_db(7015);

        assert_eq!(unreachable.form(), DbForm::NotInProject);
        assert!(!DbForm::NotInProject.blocks(), "够不着不是阻断");
        assert!(!unreachable.will_run());

        let mut missing = db(7016, "DESI");
        missing.anomaly = Some(FileAnomaly::Missing { path: "a".into() });
        missing.blocked = true;
        assert_ne!(missing.form(), unreachable.form());
    }

    /// 「够不着」的行每次预览都在，算进待办的话「已是最新」永远到不了。
    /// 它也不许并进 `desi`——那个数要跟 MDB 声明数一对才说得清差在哪儿。
    #[test]
    fn unreachable_rows_are_counted_apart_and_never_block_up_to_date() {
        let preview = Preview {
            dbnums: vec![db(8000, "DESI"), unreachable_db(7015), db(8191, "CATA")],
            ..Default::default()
        };

        let totals = preview.totals();
        assert_eq!(totals.desi, 1, "够不着的库不算在范围内的设计库里");
        assert_eq!(totals.not_in_project, 1);
        assert_eq!(totals.excluded, 1);
        assert_eq!(totals.batches, 0);
        assert!(!preview.executable());
    }

    /// 「排除」与「阻断」来自契约的不同位置，汇总里也得分开数。
    #[test]
    fn totals_count_batches_excluded_and_blocked_separately() {
        let mut pending = db(8000, "DESI");
        pending.sessions.push(SessionPreview::default());
        pending.net_added = 128;
        pending.net_modified = 342;
        pending.net_deleted = 17;
        pending.model_affecting = 361;
        pending.no_generation = 14;
        pending.units = vec![UnitPreview::default(); 23];

        let mut blocked = db(8003, "DESI");
        blocked.blocked = true;
        blocked.anomaly = Some(FileAnomaly::Rollback {
            file_latest_sesno: 812,
            applied_sesno: 1005,
        });

        let preview = Preview {
            project: "AMS-8009".into(),
            dbnums: vec![pending, blocked, db(8191, "CATA")],
            ..Default::default()
        };
        let totals = preview.totals();

        assert_eq!((totals.added, totals.modified, totals.deleted), (128, 342, 17));
        assert_eq!(totals.units, 23);
        assert_eq!(totals.model_affecting, 361);
        assert_eq!(totals.no_generation, 14);
        assert_eq!(totals.batches, 1);
        assert_eq!(totals.desi, 2);
        assert_eq!(totals.excluded, 1);
        assert_eq!(totals.blocked, 1);
        assert!(preview.executable());
    }

    /// 待重试单元自己就够成一次运行——契约明写没有新 sesno 也要允许重试。
    #[test]
    fn pending_retries_alone_make_a_run_executable() {
        let preview = Preview {
            dbnums: vec![db(8191, "CATA")],
            pending_model_retries: vec![PendingModelUnit {
                dbnum: 8000,
                root_refno: "24381/1".into(),
                noun: "EQUI".into(),
                attempts: 2,
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(preview.totals().batches, 0);
        assert!(preview.executable());
    }

    /// 分母缺一个就整条不给。**不许拿预览值相加凑一个**——那个加法两个方向都会偏。
    #[test]
    fn progress_counter_needs_both_ends_from_the_task_record() {
        assert_eq!(
            counted(Some(14), Some(24), "交付单元").as_deref(),
            Some("14 / 24 个交付单元")
        );
        assert_eq!(counted(Some(14), None, "交付单元"), None);
        assert_eq!(counted(None, Some(24), "交付单元"), None);
    }

    /// 失败按 `code` 分型，不解析 message 字符串。
    #[test]
    fn failures_are_classified_by_code_only() {
        assert_eq!(
            Failure::new("precondition", "sync_live=true 时不允许手动更新").form(),
            FailForm::SyncLive
        );
        assert_eq!(
            Failure::new("conflict", "项目 AMS-8009 已有手动更新任务正在执行: mu-3c81").form(),
            FailForm::Conflict
        );
        assert_eq!(Failure::new("timeout", "").form(), FailForm::Timeout);
        assert_eq!(Failure::new("internal", "").form(), FailForm::Internal);
        // message 里带 sync_live 但 code 是 internal 的，仍然归 internal。
        assert_eq!(
            Failure::new("internal", "sync_live").form(),
            FailForm::Internal
        );
    }

    /// 结果摘要按契约字段名解。`status` 是 snake_case 的串，不是数字；
    /// **`batch` 是单数**——合流之后一个任务就是一个数据批次。
    #[test]
    fn outcome_decodes_the_partial_terminal_contract() {
        let run: Run = serde_json::from_str(
            r#"{
                "task_id":"db-8000-7","kind":"data_batch","state":"partial",
                "project":"AMS-8009","created_at":"2026-07-27T10:00:00+08:00",
                "finished_at":"2026-07-27T10:04:38+08:00","events_seen":31,
                "result":{
                    "project":"AMS-8009","status":"partial",
                    "batch":{"dbnum":8000,"db_type":"DESI","file_path":"a",
                        "start_sesno":1024,"end_sesno":1034,"status":"applied",
                        "message":null,"merged_sesnos":[1032,1033,1034],
                        "changed_elements":512},
                    "units":[{"dbnum":8000,"root_refno":"24381/2","noun":"EQUI",
                        "status":"failed","attempts":3,"message":"写入生成结果失败"}],
                    "warnings":["14 个变更无法解析合法生成根，跳过模型生成"]
                }
            }"#,
        )
        .expect("契约 payload 应当能解出来");

        assert!(run.terminal());
        let outcome = run.result.expect("终态必须带结果摘要");
        assert_eq!(outcome.status, RunStatus::Partial);
        let batch = outcome.batch.expect("数据批次摘要必须解得出来");
        assert_eq!(batch.status, BatchStatus::Applied);
        assert_eq!(batch.merged_sesnos, vec![1032, 1033, 1034]);
        assert_eq!(outcome.units[0].status, UnitStatus::Failed);
        assert_eq!(outcome.units[0].attempts, 3);
        // ADR-0007 的四个计数还没有，缺了不该让整个任务详情解不出来。
        assert_eq!(run.total_units, None);
    }

    /// 入队回执是数组不是单个 task_id：一次扫描可能排下好几个库，也可能一行都没排。
    /// 解成旧的 `{task_id, kind, state}` 会**直接反序列化失败**，界面落到一个
    /// 看不懂的 internal 错误页——这正是合流之后向导那枚按钮坏掉的形态。
    #[test]
    fn execute_returns_an_enqueue_receipt_not_a_single_task() {
        let receipt: Enqueued = serde_json::from_str(
            r#"{"project":"AMS-8009","scanned":3,
                "enqueued":[{"task_id":"db-7997-1","dbnum":7997,"db_type":"DESI",
                    "position":1,"start_sesno":1024,"end_sesno":1038}],
                "merged":[{"task_id":"db-8000-2","dbnum":8000,"db_type":"DESI",
                    "position":2,"start_sesno":812,"end_sesno":830}],
                "already_covered":[],
                "blocked":[{"dbnum":8003,"reason":"文件回退 812 < 已应用 1005"}],
                "up_to_date":1,"warnings":[]}"#,
        )
        .expect("入队回执应当能解出来");
        let summary = receipt.summary();
        assert!(summary.contains("db7997 排第 1 位"), "{summary}");
        assert!(summary.contains("1 个并入已排队的批次"), "{summary}");
        assert!(summary.contains("db8003：文件回退"), "{summary}");

        // 一行都没排也要说得清是为什么——不然人只会以为按钮没反应。
        let idle: Enqueued =
            serde_json::from_str(r#"{"scanned":3,"up_to_date":3}"#).expect("空回执也要解得出来");
        assert!(idle.summary().contains("没有需要入队的批次"));
    }
}
