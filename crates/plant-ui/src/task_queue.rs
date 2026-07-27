//! 任务队列 dock 视图（画板 S12 常态 / S12-B 暂停·重建·断线，行由组件 `C/QueueRow` 铺）。
//!
//! 一行是一个数据批次。手动与自动两条来源合流于同一条队列、先进先出，执行进度
//! 整体从模型更新向导的第三步迁到这里（`docs/adr/0011`）：向导是人主动发起、用完
//! 就关，队列是一直在跑、你希望余光能扫到的东西。
//!
//! **界面上每个数字都要指得到契约字段或本地算得出的量，指不到就不摆。**
//! 逐项出处见 `design/QUEUE-FIELD-MAP.md`，那张表是本视图的验收基准。
//!
//! 本模块放在顶层而不是 `workbench/` 下，与 `model_update` 同源：那一层按模块注释
//! 是「输入 &WorkbenchVm、输出 Vec<Cmd>」的纯绘制，而这里要连契约类型一起管。

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, Local};
use egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Ui, pos2, vec2,
};
use egui_phosphor::regular as ph;
use serde::Deserialize;

use crate::Cmd;
use crate::model_update::{
    Feed, FileAnomaly, PendingModelUnit, ProgressEvent, RowState, UnitStatus,
};
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius};
use crate::style::widgets;

/// 数据批次任务。`manual_update` 那个 kind 随心脏改造退役，不再产生新行。
pub const KIND_DATA_BATCH: &str = "data_batch";
/// 房间归属重算轮。它与 dbnum 列表平级，不挂在任何 dbnum 行下（ADR-0011）。
pub const KIND_ROOM_RECALC: &str = "room_recalc";

// ================================================================ 契约

/// `GET /api/v1/queue`。行按队列序返回、运行中在前——FIFO 意味着**列表按入队序
/// 返回就够了**，「排在第几位」不需要新字段。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct QueueSnapshot {
    pub paused: bool,
    #[serde(default)]
    pub rows: Vec<QueueRow>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QueueRow {
    pub task_id: String,
    pub dbnum: u32,
    #[serde(default)]
    pub db_type: String,
    /// `queued` | `running`。终态行不在队列里，它们从任务表来。
    pub state: String,
    pub start_sesno: i32,
    pub end_sesno: i32,
}

/// `GET /api/v1/tasks`。服务端按创建时间倒序给最近若干条，且**有上限**
/// （`limit` 钳到 200），所以它只当「计时与历史」的来源；排队行以队列快照为准，
/// 那一份不封顶。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskList {
    #[serde(default)]
    pub tasks: Vec<TaskEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TaskEntry {
    pub task_id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub project: String,
    /// **入队时刻**，不是开跑时刻——「已排」与「已用」是两个起点。
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    pub dbnum: Option<u32>,
    #[serde(default)]
    pub db_type: Option<String>,
    #[serde(default)]
    pub start_sesno: Option<i32>,
    #[serde(default)]
    pub end_sesno: Option<i32>,
    #[serde(default)]
    pub units_done: Option<u32>,
    #[serde(default)]
    pub total_units: Option<u32>,
    #[serde(default)]
    pub events_seen: u64,
    /// 房间轮的 `{panels, elements, dead_letters}`；数据批次没有这一格。
    #[serde(default)]
    pub detail: Option<RoomCounts>,
    /// 终态结果；queued / running 时缺席。
    #[serde(default)]
    pub result: Option<BatchOutcome>,
}

impl TaskEntry {
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "partial" | "failed")
    }

    /// 本批次里没能生成出来的交付单元。终态摘要给得出，不必等持久表。
    fn failed_units(&self) -> Vec<&crate::model_update::UnitResult> {
        self.result
            .iter()
            .flat_map(|o| o.units.iter())
            .filter(|u| u.status == UnitStatus::Failed)
            .collect()
    }
}

/// 房间轮任务详情。按 action 分开计数，死信是已达重试上限、自动路径不会再碰的那些。
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct RoomCounts {
    #[serde(default)]
    pub panels: usize,
    #[serde(default)]
    pub elements: usize,
    #[serde(default)]
    pub dead_letters: usize,
}

/// 数据批次的终态摘要，与 gen-model 的 `DataBatchTaskResult` 同形。
///
/// **`batch` 是单数**：合流之后一个任务就是一个数据批次，「一次运行装着几个批次」
/// 那种形状随「运行」一并退役。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchOutcome {
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub status: crate::model_update::RunStatus,
    #[serde(default)]
    pub batch: Option<crate::model_update::BatchResult>,
    #[serde(default)]
    pub units: Vec<crate::model_update::UnitResult>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// `GET /api/v1/health`。三个字段各撑一句界面上的话：`started_at` 撑「队列是重建的」，
/// `gen_spatial_tree` 撑「房间增量没开」，`queue_paused` 是暂停横幅的兜底来源。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Health {
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub sync_live: bool,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub gen_spatial_tree: bool,
    #[serde(default)]
    pub queue_paused: bool,
}

/// `GET /api/v1/update/pending-units`。走持久表，**不依赖任务历史**——一个库
/// 欠着几个单元这件事，跨重启也答得出来。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PendingUnits {
    #[serde(default)]
    pub units: Vec<PendingModelUnit>,
}

/// `GET /api/v1/dbnums` 的一行。
///
/// 阻断与排除的库**压根不入队**，队列里没有它们的行——而阻断恰恰是「这个库的水位
/// 为什么一直不动」的唯一解释。自动同步常开时更要命：人可能从不点预览，一个库
/// 可以默默阻断好几周。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DbnumStatus {
    pub dbnum: u32,
    #[serde(default)]
    pub db_type: String,
    #[serde(default)]
    pub anomaly: Option<FileAnomaly>,
    /// 不入队、不应用，水位不动。五种异常里**只有路径迁移不阻断**。
    #[serde(default)]
    pub blocked: bool,
    /// 压根不在本期执行范围（`manual_db_nums` / 类型门控）。与阻断**不是一回事**，
    /// 界面上不许合成一行：混起来会把「出事了」讲成「本来就不跑」。
    #[serde(default)]
    pub excluded: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DbnumReport {
    #[serde(default)]
    pub dbnums: Vec<DbnumStatus>,
}

/// 一次轮询取回来的全部四份。四个请求打包成一次事件，免得界面上四份数据各更新各的、
/// 中间态互相矛盾。
#[derive(Debug, Clone, Default)]
pub struct Poll {
    pub queue: QueueSnapshot,
    pub tasks: Vec<TaskEntry>,
    pub health: Option<Health>,
    pub pending: Vec<PendingModelUnit>,
    pub dbnums: Vec<DbnumStatus>,
}

// ================================================================ 视图模型

/// 某个数据批次的逐单元明细。**行数只用来铺行，不当分母**——权威计数在任务行的
/// `units_done` / `total_units` 上（ADR-0007）。
#[derive(Debug, Clone, Default)]
pub struct Detail {
    pub units: Vec<UnitLine>,
    /// 本端实际收到的事件条数。与任务行的 `events_seen` 相减就是断线期间落后的条数。
    pub received: u64,
}

#[derive(Debug, Clone)]
pub struct UnitLine {
    pub root_refno: String,
    pub noun: String,
    pub state: RowState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Vm {
    /// 当前项目。快照里 `project` 与它不一致的条目直接不显示，**但过滤要出声**。
    pub project: String,
    pub queue: QueueSnapshot,
    pub tasks: Vec<TaskEntry>,
    pub health: Option<Health>,
    pub pending: Vec<PendingModelUnit>,
    /// 已登记的库及其异常 / 阻断 / 排除标志。队列里没有这些行，它们进「本期不执行」。
    pub dbnums: Vec<DbnumStatus>,
    /// 逐单元明细，按 task_id 分桶。
    pub details: HashMap<String, Detail>,
    /// 明细长连接的状态。它与队列状态是两件事：连接断了队列照跑。
    pub feed: Feed,
    /// 轮询失败的原因。进度与行都停在上一份快照上，界面要说出来。
    pub error: Option<String>,
    /// 有没有成功取到过一次快照。没有的话画「还没连上」而不是画一个空队列。
    pub loaded: bool,
}

impl Vm {
    pub fn adopt(&mut self, poll: Poll) {
        self.queue = poll.queue;
        self.tasks = poll.tasks;
        self.health = poll.health;
        self.pending = poll.pending;
        self.dbnums = poll.dbnums;
        self.error = None;
        self.loaded = true;
        // 数据源还没连上时本端不知道项目名，退回服务端说的那个：它是 `resolve_project`
        // 的缺省，与队列里这些条目同源，比拿空串把所有条目都当成本项目要准。
        if self.project.is_empty()
            && let Some(health) = &self.health
        {
            self.project = health.project.clone();
        }
        // 已经不在队列、也不在任务窗口里的明细留着只会越积越多。
        self.details
            .retain(|task_id, _| self.tasks.iter().any(|t| &t.task_id == task_id));
    }

    pub fn apply(&mut self, task_id: &str, event: ProgressEvent) {
        let detail = self.details.entry(task_id.to_owned()).or_default();
        detail.received += 1;
        match event {
            ProgressEvent::ModelUnitStarted {
                root_refno, noun, ..
            } => detail.units.push(UnitLine {
                root_refno,
                noun,
                state: RowState::Running,
                message: None,
            }),
            ProgressEvent::ModelUnitFinished {
                root_refno,
                success,
                message,
                ..
            } => {
                if let Some(line) = detail
                    .units
                    .iter_mut()
                    .rev()
                    .find(|u| u.root_refno == root_refno)
                {
                    line.state = if success {
                        RowState::Done
                    } else {
                        RowState::Failed
                    };
                    line.message = message;
                }
            }
            // 阶段一的起讫由任务行的 state 自己说得清，明细区不重复铺一遍。
            ProgressEvent::DataBatchStarted { .. } | ProgressEvent::DataBatchFinished { .. } => {}
        }
    }

    /// 断线时把还没收到收尾事件的明细行降级。它们可能已经做完了，只是消息没到，
    /// 继续显示「生成中」是在撒谎。
    pub fn mark_unknown(&mut self) {
        for detail in self.details.values_mut() {
            for line in &mut detail.units {
                if line.state == RowState::Running {
                    line.state = RowState::Unknown;
                }
            }
        }
    }

    /// 队列是不是暂停。快照与 `/health` 说的是同一件事，取快照那一份。
    pub fn paused(&self) -> bool {
        self.queue.paused
    }

    /// 房间轮：最近那条 `room_recalc` 任务。泳道本身是客户端第 4 项，这里只取
    /// 顶栏摘要要用的那个数。
    fn room_task(&self) -> Option<&TaskEntry> {
        self.tasks.iter().find(|t| t.kind == KIND_ROOM_RECALC)
    }

    /// 顶栏那格「房间待收敛 N」。`None` = 这一格整个不画。
    ///
    /// 房间增量没开时一个字都不提：当前项目的 `gen_spatial_tree` 是关的，PANE /
    /// CWALL / CFLOOR 三张表全空，画出来必然是个永远不动的 0，比不画更糟。
    pub fn rooms_pending(&self) -> Option<usize> {
        if !self.health.as_ref().is_some_and(|h| h.gen_spatial_tree) {
            return None;
        }
        let task = self.room_task()?;
        match task.detail {
            Some(counts) => Some(counts.panels + counts.elements),
            // 详情缺席时退回任务自己的分母，它至少是这一轮的目标数。
            None => task.total_units.map(|t| t as usize),
        }
    }

    fn task(&self, task_id: &str) -> Option<&TaskEntry> {
        self.tasks.iter().find(|t| t.task_id == task_id)
    }

    /// 这个 dbnum 欠着几个交付单元（持久表口径，跨重启仍然算数）。
    fn owed(&self, dbnum: u32) -> usize {
        self.pending.iter().filter(|u| u.dbnum == dbnum).count()
    }

    /// 属于本项目吗。任务行没进 `/tasks` 那 200 条窗口时项目未知——**未知按显示处理**：
    /// 一个 gen-model 进程只服务一个项目，把一条说不清出身的活动行藏掉比混排更糟。
    fn mine(&self, task_id: &str) -> bool {
        match self.task(task_id) {
            Some(entry) if !entry.project.is_empty() && !self.project.is_empty() => {
                entry.project == self.project
            }
            _ => true,
        }
    }
}

/// 一行的进行状态。取值来自契约 `state` 加两阶段事件，`Unknown` 专给断线
/// ——**不许拿「排队中」当兜底文案**，那现在是一个有契约来源的真实状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Queued,
    /// 出队冻结之后、第一个交付单元开始生成之前。
    Applying,
    Generating,
    Succeeded,
    Partial,
    Failed,
    Unknown,
}

impl Phase {
    fn tone(self, t: &Tokens) -> Color32 {
        match self {
            Self::Queued => t.text_muted,
            Self::Applying | Self::Generating => t.accent,
            Self::Succeeded => t.success,
            Self::Partial => t.warn,
            Self::Failed => t.danger,
            Self::Unknown => t.text_muted,
        }
    }

    fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Applying | Self::Generating)
    }

    fn running(self) -> bool {
        matches!(self, Self::Applying | Self::Generating)
    }
}

/// 计时那一列。三种起点互不相通：排队从入队时刻起算、运行从开跑时刻起算、
/// 终态摆的是结束的钟点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timer {
    Queued(Option<Duration>),
    Running(Option<Duration>),
    Finished(Option<String>),
}

/// 铺一行要的全部东西。由 [`rows`] 从快照 + 任务表 + 持久表算出来，绘制层只读。
#[derive(Debug, Clone)]
pub struct RowVm {
    pub task_id: String,
    pub dbnum: u32,
    pub db_type: String,
    pub phase: Phase,
    pub window: String,
    /// 排队第几位。只有排队行有，且**数的是队列里还排着几个库**，不掺历史行。
    pub position: Option<usize>,
    pub units_done: Option<u32>,
    pub total_units: Option<u32>,
    pub timer: Timer,
    /// 「进度 / 说明」那一列的文字。有进度条可画时它让位给条。
    pub note: String,
    pub note_tone: Status,
    /// 同一个 dbnum 的上一批已经冻结，这一行是之后新存的会话。
    pub behind_frozen: bool,
    /// 这个 dbnum 在持久表上欠着的交付单元数。
    pub owed: usize,
    /// 明细缺了多少条（服务端已发 − 本端已收）。
    pub behind_events: u64,
}

impl RowVm {
    /// 「有变化」= 还有活没干完：运行中、排队中，以及水位虽已推进、但仍欠着
    /// 交付单元的部分完成行。纯粹已完成的收起来。
    fn has_work(&self) -> bool {
        self.phase.active() || self.owed > 0 || self.phase == Phase::Failed
    }

    fn state_text(&self, paused: bool) -> String {
        match self.phase {
            Phase::Queued => {
                let head = if paused { "已暂停" } else { "排队中" };
                match self.position {
                    Some(n) => format!("{head} · 第 {n} 位"),
                    None => head.to_owned(),
                }
            }
            Phase::Applying => "应用中".to_owned(),
            Phase::Generating => match (self.units_done, self.total_units) {
                (Some(done), Some(total)) => format!("生成中 · {done} / {total} 个单元"),
                // 分母缺席时只报已完成数，绝不拿别处的数字凑一个（ADR-0007）。
                (Some(done), None) => format!("生成中 · 已完成 {done} 个单元"),
                _ => "生成中".to_owned(),
            },
            Phase::Succeeded => "已完成".to_owned(),
            Phase::Partial => match self.owed {
                0 => "部分完成".to_owned(),
                n => format!("部分完成 · 欠 {n} 个单元"),
            },
            Phase::Failed => "失败".to_owned(),
            Phase::Unknown => "状态未知".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    /// 默认只看有变化的库。287 行是积压态不是稳态，但面板得扛得住那一天。
    #[default]
    Changed,
    All,
    Running,
    Failed,
}

impl Filter {
    const ALL: [Filter; 4] = [Filter::Changed, Filter::All, Filter::Running, Filter::Failed];

    fn label(self) -> &'static str {
        match self {
            Self::Changed => "有变化",
            Self::All => "全部",
            Self::Running => "运行中",
            Self::Failed => "失败",
        }
    }

    fn keeps(self, row: &RowVm) -> bool {
        match self {
            Self::Changed => row.has_work(),
            Self::All => true,
            Self::Running => row.phase.running(),
            Self::Failed => matches!(row.phase, Phase::Failed | Phase::Partial),
        }
    }
}

/// 把快照、任务表与持久表拼成界面上的行。
///
/// 两个来源分工固定：**排队与运行中的行以队列快照为准**（那一份不封顶，287 行也全在），
/// 任务表只补计时、单元计数与终态历史。同一个 dbnum 至多两行——一行运行中、一行排队中，
/// 下面那行要说清自己是「上一批冻结之后新存的会话」，不能让人以为是重复项。
pub fn rows(vm: &Vm) -> Vec<RowVm> {
    let mut out = Vec::new();
    let mut seen = Vec::new();
    let mut position = 0usize;

    for row in &vm.queue.rows {
        if !vm.mine(&row.task_id) {
            continue;
        }
        let entry = vm.task(&row.task_id);
        let detail = vm.details.get(&row.task_id);
        let units = detail.map(|d| d.units.as_slice()).unwrap_or_default();
        let total_units = entry.and_then(|e| e.total_units);
        // 认不出的 state 串归「状态未知」。**不许兜底成「排队中」**——那现在是一个
        // 有契约来源的真实状态（`state == queued`），拿它当兜底会把两件事说成一件。
        //
        // 「应用中」与「生成中」的分界：画板口径是收到第一条 `ModelUnitStarted`，
        // 轮询侧的等价观测是 `total_units` 出现（worker 交给单元循环那一刻才写它）。
        // 两条通道都在时口径一致，不会一个说应用中一个说生成中。
        let phase = match row.state.as_str() {
            "queued" => Phase::Queued,
            "running" if total_units.is_some() || !units.is_empty() => Phase::Generating,
            "running" => Phase::Applying,
            _ => Phase::Unknown,
        };
        let queued = phase == Phase::Queued;
        if queued {
            position += 1;
        }
        let behind_frozen = queued
            && vm
                .queue
                .rows
                .iter()
                .any(|other| other.dbnum == row.dbnum && other.state == "running");
        seen.push(row.task_id.clone());
        out.push(RowVm {
            task_id: row.task_id.clone(),
            dbnum: row.dbnum,
            db_type: row.db_type.clone(),
            phase,
            window: window(row.start_sesno, row.end_sesno),
            position: queued.then_some(position),
            units_done: entry.and_then(|e| e.units_done),
            total_units,
            timer: if queued {
                Timer::Queued(entry.and_then(|e| since(&e.created_at)))
            } else {
                Timer::Running(entry.and_then(|e| e.started_at.as_deref()).and_then(since))
            },
            note: if behind_frozen {
                "上一批已冻结，这是之后新存的会话".to_owned()
            } else {
                String::new()
            },
            note_tone: Status::Neutral,
            behind_frozen,
            owed: vm.owed(row.dbnum),
            behind_events: entry
                .map(|e| e.events_seen)
                .unwrap_or_default()
                .saturating_sub(detail.map(|d| d.received).unwrap_or_default()),
        });
    }

    for entry in &vm.tasks {
        if entry.kind != KIND_DATA_BATCH || !entry.terminal() || seen.contains(&entry.task_id) {
            continue;
        }
        if !entry.project.is_empty() && !vm.project.is_empty() && entry.project != vm.project {
            continue;
        }
        let Some(dbnum) = entry.dbnum else {
            continue;
        };
        // 同上：只认契约里有的那三个终态串，认不出的不许猜成「失败」。
        let phase = match entry.state.as_str() {
            "succeeded" => Phase::Succeeded,
            "partial" => Phase::Partial,
            "failed" => Phase::Failed,
            _ => Phase::Unknown,
        };
        let failed = entry.failed_units();
        let note = match failed.first() {
            Some(unit) if unit.attempts > 0 => format!(
                "{} {} 第 {} 次尝试失败",
                unit.noun, unit.root_refno, unit.attempts
            ),
            Some(unit) => format!("{} {} 未能生成", unit.noun, unit.root_refno),
            None => entry
                .result
                .as_ref()
                .and_then(|o| o.batch.as_ref())
                .and_then(|b| b.message.clone())
                .unwrap_or_default(),
        };
        out.push(RowVm {
            task_id: entry.task_id.clone(),
            dbnum,
            db_type: entry.db_type.clone().unwrap_or_default(),
            phase,
            window: window(
                entry.start_sesno.unwrap_or_default(),
                entry.end_sesno.unwrap_or_default(),
            ),
            position: None,
            units_done: entry.units_done,
            total_units: entry.total_units,
            timer: Timer::Finished(entry.finished_at.as_deref().and_then(hhmm)),
            note,
            note_tone: if phase == Phase::Succeeded {
                Status::Neutral
            } else {
                Status::Warn
            },
            behind_frozen: false,
            owed: vm.owed(dbnum),
            behind_events: 0,
        });
    }

    out
}

/// 快照里属于别的项目、因此没有画上去的条目数。**过滤不许无声**——不然人会
/// 对着一块空面板怀疑服务没连上。
pub fn filtered_out(vm: &Vm) -> usize {
    let active = vm
        .queue
        .rows
        .iter()
        .filter(|row| !vm.mine(&row.task_id))
        .count();
    let history = vm
        .tasks
        .iter()
        .filter(|t| {
            t.kind == KIND_DATA_BATCH
                && t.terminal()
                && !t.project.is_empty()
                && !vm.project.is_empty()
                && t.project != vm.project
        })
        .count();
    active + history
}

/// 状态栏那一格要的几个数。与面板共用同一份行推导——同一组数字两处各数各的，
/// 迟早会对不上。
pub fn status(vm: &Vm) -> crate::vm::QueueStatusVm {
    let all = rows(vm);
    crate::vm::QueueStatusVm {
        active: all.iter().filter(|row| row.phase.active()).count(),
        paused: vm.paused(),
        filtered_out: filtered_out(vm),
        known: vm.loaded,
    }
}

// ================================================================ 房间泳道

/// 房间归属重算泳道的形态。它与 dbnum 列表**平级**，不挂在任何 dbnum 行下——
/// 房间任务的行 id 刻意不带 dbnum，那个字段是「最后一次触发来源」而非归属。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneState {
    /// `gen_spatial_tree` 关着。**只说这一句**，不摆一条永远是 0 的空泳道。
    Off,
    Converging,
    Waiting,
    Converged,
}

/// 等这么久还排不上就算饿着了。画板上 14 分钟是常态、47 分钟是饥饿态，
/// 阈值取在两者之间——它是一条界面提示线，不是服务端的什么承诺。
const STARVING_AFTER: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone)]
pub struct Lane {
    pub state: LaneState,
    /// 有没有过房间轮记录。没有的话下面那几个计数**一个都不许摆**——
    /// 「0 块面板待重算」和「从来没收过一轮」是两回事，前者是个没有出处的 0。
    pub seen: bool,
    pub panels: usize,
    pub elements: usize,
    pub dead_letters: usize,
    pub done: Option<u32>,
    pub total: Option<u32>,
    /// 现在 − 上一轮 `room_recalc` 的 `finished_at`。持久表没有时间戳，
    /// 「已等待多久」只能这么算。
    pub waited: Option<Duration>,
    pub starving: bool,
}

/// 房间泳道。`None` = 还没读到 `/health`，说不出开没开，那就一个字不提。
pub fn lane(vm: &Vm) -> Option<Lane> {
    let health = vm.health.as_ref()?;
    if !health.gen_spatial_tree {
        return Some(Lane {
            state: LaneState::Off,
            seen: false,
            panels: 0,
            elements: 0,
            dead_letters: 0,
            done: None,
            total: None,
            waited: None,
            starving: false,
        });
    }
    let task = vm.room_task();
    let counts = task.and_then(|t| t.detail).unwrap_or_default();
    let running = task.is_some_and(|t| t.state == "running");
    let waited = task
        .and_then(|t| t.finished_at.as_deref())
        .and_then(since);
    let live = counts.panels + counts.elements;
    let state = if running {
        LaneState::Converging
    } else if live > 0 {
        LaneState::Waiting
    } else {
        LaneState::Converged
    };
    Some(Lane {
        state,
        seen: task.is_some(),
        panels: counts.panels,
        elements: counts.elements,
        dead_letters: counts.dead_letters,
        done: task.and_then(|t| t.units_done),
        total: task.and_then(|t| t.total_units),
        // 饥饿只在「有活却排不上」时成立：已经收敛干净的泳道等再久也不算饿着。
        starving: state == LaneState::Waiting
            && waited.is_some_and(|elapsed| elapsed >= STARVING_AFTER),
        waited,
    })
}

/// 「本期不执行」那一格：阻断与排除的库。两者**不许合成一行**——阻断是出事了、
/// 排除是本来就不跑，混起来会把前者讲成后者。阻断排在前面。
pub fn not_running(vm: &Vm) -> Vec<&DbnumStatus> {
    let mut rows: Vec<&DbnumStatus> = vm
        .dbnums
        .iter()
        .filter(|db| db.blocked || db.excluded)
        .collect();
    rows.sort_by_key(|db| (!db.blocked, db.dbnum));
    rows
}

/// 筛选与搜索。**搜索时不受筛选芯片限制**——搜一个已完成的库却搜不到，
/// 会让人以为它不在队列里。
pub fn visible(all: &[RowVm], filter: Filter, search: &str) -> Vec<usize> {
    let needle = search.trim();
    let mut hits: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            if needle.is_empty() {
                filter.keeps(row)
            } else {
                row.dbnum.to_string().contains(needle)
            }
        })
        .map(|(i, _)| i)
        .collect();
    // 运行中置顶：队列再长也要一眼看得见现在在跑什么。
    hits.sort_by_key(|i| !all[*i].phase.running());
    hits
}

// ================================================================ 绘制状态

#[derive(Default)]
pub struct State {
    pub filter: Filter,
    pub search: String,
    /// 显式点过的展开箭头。默认展开与否按行的形态定，这里只记「与默认相反」的那些。
    toggled: std::collections::HashSet<String>,
}

impl State {
    /// 行内明细默认折叠，只有运行中那一行默认展开——与变化树已有的口径一致
    /// （会跑的摊开、不跑的收起）；队列是常驻视图，全部展开会把命令行和日志挤没。
    fn open(&self, row: &RowVm) -> bool {
        self.toggled.contains(&row.task_id) != row.phase.running()
    }

    fn toggle(&mut self, task_id: &str) {
        if !self.toggled.remove(task_id) {
            self.toggled.insert(task_id.to_owned());
        }
    }
}

// ================================================================ 视图

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm, state: &mut State, cmds: &mut Vec<Cmd>) {
    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;

        if !vm.loaded {
            return not_connected(ui, t, d, vm);
        }

        if vm.paused() {
            paused_banner(ui, t, d, cmds);
        }
        if rebuilt(vm) {
            rebuilt_banner(ui, t, d, vm);
        }
        if let Some(error) = vm.error.as_deref() {
            hint_banner(
                ui,
                t,
                d,
                Status::Warn,
                ph::WARNING,
                &format!("队列状态查询暂时失败：{error}。下面这份是上一次取到的快照。"),
            );
        }

        let all = rows(vm);
        summary_bar(ui, t, d, vm, &all, cmds);
        toolbar(ui, t, d, &all, state);
        // 泳道与「本期不执行」贴底：它们与 dbnum 列表平级，不该跟着列表一起滚走
        // ——一个库默默阻断好几周的时候，滚出视野就等于没说。
        footer(ui, t, d, vm);
        header(ui, t, d);

        let hits = visible(&all, state.filter, &state.search);
        ScrollArea::vertical()
            .id_salt("task-queue-rows")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if hits.is_empty() {
                    return empty_note(ui, t, d, vm, state);
                }
                for i in hits {
                    let row = &all[i];
                    let open = state.open(row);
                    let (resp, note_fits) = queue_row(ui, t, d, row, vm.paused(), open);
                    if resp.clicked() {
                        state.toggle(&row.task_id);
                    }
                    if !note_fits {
                        sub_line(ui, t, d, &row.note, row.note_tone == Status::Error);
                    }
                    if open {
                        row_detail(ui, t, d, vm, row, cmds);
                    }
                }
                ui.add_space(d.px(8.0));
            });
    });
}

/// 还没取到过一次快照。**不画一个空队列**——空队列和连不上服务是两件事。
fn not_connected(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - d.px(48.0)).max(d.px(32.0)));
        ui.spacing_mut().item_spacing.y = d.px(8.0);
        match vm.error.as_deref() {
            Some(error) => {
                ui.label(
                    RichText::new(ph::PLUGS)
                        .font(FontId::new(d.px(24.0), FontFamily::Proportional))
                        .color(t.warn),
                );
                ui.label(
                    RichText::new("读不到任务队列")
                        .font(Font::strong(d))
                        .color(t.text_secondary),
                );
                ui.label(
                    RichText::new(error)
                        .font(Font::meta(d))
                        .color(t.text_muted),
                );
            }
            None => {
                ui.add(egui::Spinner::new().size(d.px(22.0)).color(t.accent));
                ui.label(
                    RichText::new("正在读取任务队列…")
                        .font(Font::strong(d))
                        .color(t.text_secondary),
                );
            }
        }
    });
}

fn empty_note(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm, state: &State) {
    let text = if !state.search.trim().is_empty() {
        format!("没有 dbnum 含「{}」的批次。", state.search.trim())
    } else if state.filter == Filter::Changed && !vm.queue.rows.is_empty() {
        "本项目没有还没干完的批次。切到「全部」看历史。".to_owned()
    } else {
        "队列是空的：没有待应用的会话，也没有排队中的数据批次。".to_owned()
    };
    ui.add_space(d.px(24.0));
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(text).font(Font::meta(d)).color(t.text_muted));
    });
}

/// 暂停横幅。**文案只能说「不再出队」**——正在跑的那条停不了，服务端没有中止接口。
fn paused_banner(ui: &mut Ui, t: &Tokens, d: Density, cmds: &mut Vec<Cmd>) {
    let (fg, bg) = t.status(Status::Warn);
    let rect = strip_rect(ui, t, d.px(40.0), bg);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(14.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(15.0), d.px(15.0)), Sense::hover());
    glyph(&ui, mark.center(), ph::PAUSE_CIRCLE, d.px(15.0), fg);
    ui.label(
        RichText::new("队列已暂停 · 不再出队。正在跑的这一批会跑完为止——服务端没有中止接口。")
            .font(Font::strong(d))
            .color(fg),
    );
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if ui
            .add(widgets::button(t, d, "恢复队列").icon(ph::PLAY))
            .clicked()
        {
            cmds.push(Cmd::SetQueuePaused(false));
        }
    });
}

/// 服务重启过、这条队列是按水位重建的。
///
/// 判据：最早那条活动行的入队时刻落在进程启动之后 2 分钟内——那批行是启动扫描
/// 按水位重新发现出来的，不是运行期间新存的会话。
fn rebuilt(vm: &Vm) -> bool {
    let Some(health) = vm.health.as_ref() else {
        return false;
    };
    let Some(started) = parse(&health.started_at) else {
        return false;
    };
    vm.queue.rows.iter().any(|row| {
        vm.task(&row.task_id)
            .and_then(|entry| parse(&entry.created_at))
            .is_some_and(|at| {
                let gap = at.signed_duration_since(started);
                gap.num_seconds() >= 0 && gap.num_seconds() <= 120
            })
    })
}

fn rebuilt_banner(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm) {
    let at = vm
        .health
        .as_ref()
        .and_then(|h| hhmm(&h.started_at))
        .unwrap_or_else(|| "不明时刻".into());
    // 后半句不许省：省了就等于把重建的队列装成一直在那儿。
    hint_banner(
        ui,
        t,
        d,
        Status::Neutral,
        ph::ARROW_COUNTER_CLOCKWISE,
        &format!(
            "服务 {at} 重启过，这条队列是按水位重建的；排队时长从重启起算，重启前排了多久已无从得知。"
        ),
    );
}

fn hint_banner(ui: &mut Ui, t: &Tokens, d: Density, tone: Status, icon: &str, text: &str) {
    let (fg, _) = t.status(tone);
    let rect = strip_rect(ui, t, d.px(28.0), t.bg_header);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(14.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(13.0), d.px(13.0)), Sense::hover());
    glyph(&ui, mark.center(), icon, d.px(13.0), fg);
    ui.label(
        RichText::new(text)
            .font(Font::meta(d))
            .color(t.text_secondary),
    );
}

/// 顶栏：三个计数 + 断线提示 + 两枚控制按钮。
fn summary_bar(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &Vm,
    all: &[RowVm],
    cmds: &mut Vec<Cmd>,
) {
    let running = all.iter().filter(|r| r.phase.running()).count();
    let queued = all.iter().filter(|r| r.phase == Phase::Queued).count();
    let rect = strip_rect(ui, t, d.px(38.0), t.bg_panel);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(14.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(6.0);

    count_dot(&mut ui, t, d, t.accent, &format!("运行 {running}"));
    ui.add_space(d.px(8.0));
    let (queued_color, queued_label) = if vm.paused() {
        (t.warn, format!("已暂停 {queued}"))
    } else {
        (t.text_muted, format!("排队 {queued}"))
    };
    count_dot(&mut ui, t, d, queued_color, &queued_label);
    if let Some(rooms) = vm.rooms_pending() {
        ui.add_space(d.px(8.0));
        count_dot(&mut ui, t, d, t.warn, &format!("房间待收敛 {rooms}"));
    }

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if vm.paused() {
            if ui
                .add(widgets::button(t, d, "恢复队列").icon(ph::PLAY))
                .clicked()
            {
                cmds.push(Cmd::SetQueuePaused(false));
            }
        } else if ui
            .add(widgets::button(t, d, "暂停队列").icon(ph::PAUSE))
            .on_hover_text("只挡出队；正在跑的那一批会跑完为止")
            .clicked()
        {
            cmds.push(Cmd::SetQueuePaused(true));
        }
        if ui
            .add(widgets::button(t, d, "立刻扫一遍").icon(ph::ARROWS_CLOCKWISE))
            .on_hover_text("不插队，只是别等下一个 30 秒轮询")
            .clicked()
        {
            cmds.push(Cmd::ScanNow);
        }
        // 断线降级的是明细区，不是进度：计数走轮询，连接断了照样准（ADR-0007）。
        if let Feed::Down(reason) = &vm.feed {
            let behind: u64 = all.iter().map(|r| r.behind_events).sum();
            if ui
                .add(widgets::button(t, d, "重连明细").icon(ph::PLUGS))
                .on_hover_text(reason.as_str())
                .clicked()
            {
                cmds.push(Cmd::ReconnectQueueFeed);
            }
            ui.label(
                RichText::new(format!("实时通道断开 · 本端落后 {behind} 条明细"))
                    .font(Font::meta(d))
                    .color(t.warn),
            );
        }
    });
}

fn count_dot(ui: &mut Ui, t: &Tokens, d: Density, color: Color32, text: &str) {
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(8.0), d.px(8.0)), Sense::hover());
    ui.painter()
        .circle_filled(mark.center(), d.px(3.5), color);
    ui.label(
        RichText::new(text)
            .font(Font::meta(d))
            .color(t.text_secondary),
    );
}

/// 工具行：搜索框 + 筛选芯片 + 右侧「显示 N / 共 M」。
fn toolbar(ui: &mut Ui, t: &Tokens, d: Density, all: &[RowVm], state: &mut State) {
    let rect = strip_rect(ui, t, d.px(34.0), t.bg_panel);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(14.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(4.0);

    ui.add(
        egui::TextEdit::singleline(&mut state.search)
            .desired_width(d.px(190.0))
            .font(Font::mono_meta(d))
            .hint_text("搜 dbnum"),
    );
    ui.add_space(d.px(6.0));
    for filter in Filter::ALL {
        if ui
            .add(widgets::chip(
                t,
                d,
                filter.label(),
                state.filter == filter && state.search.trim().is_empty(),
            ))
            .clicked()
        {
            state.filter = filter;
            state.search.clear();
        }
    }

    let shown = visible(all, state.filter, &state.search).len();
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.label(
            RichText::new(format!("显示 {shown} / 队列与历史共 {} 条", all.len()))
                .font(Font::mono_micro(d))
                .color(t.text_muted),
        );
    });
}

/// 贴底的两格：房间归属重算泳道 + 本期不执行。都与 dbnum 列表平级。
fn footer(ui: &mut Ui, t: &Tokens, d: Density, vm: &Vm) {
    let lane = lane(vm);
    let excluded = not_running(vm);
    if lane.is_none() && excluded.is_empty() {
        return;
    }
    egui::Panel::bottom("task-queue-footer")
        .frame(egui::Frame::new().fill(t.bg_panel))
        .show_separator_line(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            if let Some(lane) = lane {
                room_lane(ui, t, d, &lane);
            }
            if !excluded.is_empty() {
                excluded_block(ui, t, d, &excluded);
            }
        });
}

fn room_lane(ui: &mut Ui, t: &Tokens, d: Density, lane: &Lane) {
    let fill = if lane.starving { t.danger_bg } else { t.bg_header };
    let head = strip_rect(ui, t, d.px(26.0), fill);
    let mut row = child(ui, head.shrink2(vec2(d.px(14.0), 0.0)), Align::Center);
    row.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = row.allocate_exact_size(vec2(d.px(13.0), d.px(13.0)), Sense::hover());
    glyph(&row, mark.center(), ph::SELECTION_ALL, d.px(13.0), t.text_muted);
    row.label(
        RichText::new("房间归属重算")
            .font(Font::strong(d))
            .color(t.text_primary),
    );
    // 行 id 不带 dbnum 是刻意的：拿「最后一次触发来源」当归属会误导。
    row.label(
        RichText::new("跨库 · 不属于任何 dbnum")
            .font(Font::micro(d))
            .color(t.text_muted),
    );
    row.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if let Some(waited) = lane.waited {
            ui.label(
                RichText::new(format!("已等待 {}", minutes(waited)))
                    .font(Font::mono_meta(d))
                    .color(if lane.starving { t.danger } else { t.text_muted }),
            );
        }
        if let (Some(done), Some(total)) = (lane.done, lane.total)
            && lane.state == LaneState::Converging
        {
            ui.label(
                RichText::new(format!("{done} / {total}"))
                    .font(Font::mono_meta(d))
                    .color(t.accent),
            );
        }
    });

    let body = strip_rect(ui, t, d.px(48.0), fill);
    let mut ui = child(ui, body.shrink2(vec2(d.px(14.0), d.px(5.0))), Align::Min);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = d.px(4.0);
        if lane.state == LaneState::Off {
            // 房间增量没开时**只说这一句**：本项目 PANE / CWALL / CFLOOR 三张表都是空的，
            // 画一条永远 0 的泳道比不画更糟。
            ui.label(
                RichText::new("房间增量没开（gen_spatial_tree = false），本项目当前不做房间归属重算。")
                    .font(Font::meta(d))
                    .color(t.text_muted),
            );
            return;
        }
        if !lane.seen {
            ui.label(
                RichText::new("房间增量开着，但还没有过房间轮记录——待重算的数量随第一轮任务带出来。")
                    .font(Font::meta(d))
                    .color(t.text_muted),
            );
            return;
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = d.px(14.0);
            ui.label(
                RichText::new(format!("{} 块面板待重算", lane.panels))
                    .font(Font::meta(d))
                    .color(t.text_secondary),
            );
            ui.label(
                RichText::new(format!("{} 个构件待重算", lane.elements))
                    .font(Font::meta(d))
                    .color(t.text_secondary),
            );
            // 死信到顶之后自动路径不会再碰它们，界面是唯一能把它们暴露出来的地方。
            if lane.dead_letters > 0 {
                ui.label(
                    RichText::new(format!("{} 个已达重试上限", lane.dead_letters))
                        .font(Font::meta(d))
                        .color(t.danger),
                );
            }
        });
        let (text, color) = if lane.starving {
            (
                "队列一直不空，房间收敛排不上。这段窗口里材料表读到的房间号可能是旧的，且不会有任何报错。",
                t.danger,
            )
        } else {
            (
                "队列跑空时才收一轮。队列持续繁忙，材料表的房间号可能滞后。",
                t.text_muted,
            )
        };
        ui.label(RichText::new(text).font(Font::micro(d)).color(color));
    });
}

fn excluded_block(ui: &mut Ui, t: &Tokens, d: Density, rows: &[&DbnumStatus]) {
    let head = strip_rect(ui, t, d.px(24.0), t.bg_panel);
    let mut row = child(ui, head.shrink2(vec2(d.px(14.0), 0.0)), Align::Center);
    row.spacing_mut().item_spacing.x = d.px(8.0);
    row.label(
        RichText::new("本期不执行")
            .font(Font::strong(d))
            .color(t.text_primary),
    );
    row.label(
        RichText::new("这些库不入队，水位不动")
            .font(Font::micro(d))
            .color(t.text_muted),
    );

    ScrollArea::vertical()
        .id_salt("task-queue-excluded")
        .max_height(d.px(84.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for db in rows {
                excluded_row(ui, t, d, db);
            }
        });
}

fn excluded_row(ui: &mut Ui, t: &Tokens, d: Density, db: &DbnumStatus) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(24.0)), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, t.bg_hover);
    }
    let blocked = db.blocked;
    let tone = if blocked { t.danger } else { t.text_muted };
    let mid = rect.center().y;
    let x = rect.left() + d.px(14.0);
    ui.painter().circle_filled(pos2(x, mid), d.px(4.0), tone);
    text_at(
        ui,
        pos2(rect.left() + d.px(28.0), mid),
        &format!("db{}", db.dbnum),
        Font::mono(d),
        t.text_primary,
    );
    // 排除有两种来源：类型不对，或者 DESI 但不在 `manual_db_nums` 里。一律写「非 DESI」
    // 的话，后一种就是假话——那个库明明是 DESI，只是这一期没排它。
    let reason = match (&db.anomaly, blocked) {
        (Some(anomaly), _) => anomaly_brief(anomaly),
        (None, true) => "阻断，原因未随契约给出".to_owned(),
        (None, false) if db.db_type != "DESI" => {
            format!("非 DESI（{}），不在本期范围", db.db_type)
        }
        (None, false) => "DESI，但不在本期执行范围内".to_owned(),
    };
    text_at(
        ui,
        pos2(rect.left() + d.px(108.0), mid),
        &reason,
        Font::meta(d),
        if blocked { t.danger } else { t.text_muted },
    );
    // 「阻断」与「排除」两个词分开摆：一个是出事了，一个是本来就不跑。
    let tag = if blocked { "阻断" } else { "排除" };
    let g = layout(ui, tag, Font::mono_micro(d));
    let pad = d.px(6.0);
    let box_rect = Rect::from_min_size(
        pos2(rect.right() - d.px(14.0) - g.size().x - pad * 2.0, mid - d.px(9.0)),
        vec2(g.size().x + pad * 2.0, d.px(18.0)),
    );
    let (fg, bg) = t.status(if blocked { Status::Error } else { Status::Neutral });
    ui.painter()
        .rect_filled(box_rect, CornerRadius::same(radius::SM), bg);
    ui.painter().galley(
        pos2(box_rect.left() + pad, mid - g.size().y / 2.0),
        g,
        fg,
    );
    // 出路（同号重复要列出 paths[] 交给人挑、文件缺失是补回文件或注销登记）
    // 与预览那边 S2-E 用的是同一段文案，两处不许各说各话。
    if let Some(anomaly) = &db.anomaly {
        resp.on_hover_text(anomaly.to_string());
    }
}

/// 阻断原因的短说明。完整的出路在悬停里，与 S2-E 同一段文案。
fn anomaly_brief(anomaly: &FileAnomaly) -> String {
    match anomaly {
        FileAnomaly::Rollback {
            file_latest_sesno,
            applied_sesno,
        } => format!(
            "文件回退 {} < 已应用 {}",
            group(*file_latest_sesno as i64),
            group(*applied_sesno as i64)
        ),
        FileAnomaly::TypeChanged {
            stored_db_type,
            observed_db_type,
        } => format!("类型变化 {stored_db_type} → {observed_db_type}"),
        FileAnomaly::Duplicate { paths } => format!("同号重复 · {} 个同号文件", paths.len()),
        FileAnomaly::Missing { .. } => "文件缺失".to_owned(),
        // 五种异常里只有它照常入队，所以只会出现在排除行上（如果真出现的话）。
        FileAnomaly::PathMigrated { .. } => "路径迁移".to_owned(),
    }
}

/// 表头。七个列位与 [`queue_row`] 共用 [`cols`] 算出来的那一组 x，两处不许各排各的。
fn header(ui: &mut Ui, t: &Tokens, d: Density) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(26.0)), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.bg_panel);
    hairline(ui.painter(), rect, t.border);
    let c = cols(rect, d);
    for (x, name) in [
        (c.db, "设计库"),
        (c.ty, "类型"),
        (c.window, "会话区间"),
        (c.state, "状态"),
        (c.note, "进度 / 说明"),
    ] {
        let g = layout(ui, name, Font::micro(d));
        ui.painter().galley(
            pos2(x, rect.center().y - g.size().y / 2.0),
            g,
            t.text_muted,
        );
    }
    let g = layout(ui, "计时 / 结果", Font::micro(d));
    ui.painter().galley(
        pos2(c.right - g.size().x, rect.center().y - g.size().y / 2.0),
        g,
        t.text_muted,
    );
}

struct Cols {
    dot: f32,
    db: f32,
    ty: f32,
    window: f32,
    state: f32,
    note: f32,
    /// 「进度 / 说明」写到哪为止；再往右是计时那一列的保留位。
    note_end: f32,
    right: f32,
}

/// 七个列位。
///
/// 左边四列按内容定宽（库号、类型、会话区间都是定长的），「状态」与
/// 「进度 / 说明」分掉剩下的宽度。**不能照画板那 1024 写死列位**：dock 里这块面板
/// 可宽可窄，默认布局下它只有六百点上下，写死的话说明列会整个被挤掉——而
/// 「上一批已冻结」那句恰恰是「同一个 dbnum 两行不是重复项」的全部依据。
///
/// 窄到连状态列都摆不下时，说明列让位给状态：一行先要说清它在干什么。
/// 挤掉的说明由调用点在行下面另起一行补上，不会丢。
fn cols(rect: Rect, d: Density) -> Cols {
    let pad = d.px(14.0);
    let right = rect.right() - pad;
    let window = rect.left() + d.px(168.0);
    let after_window = window + d.px(146.0);
    let note_end = right - d.px(80.0);
    let room = (note_end - after_window).max(0.0);
    let state_w = d.px(168.0).min(room);
    Cols {
        dot: rect.left() + pad,
        db: rect.left() + d.px(28.0),
        ty: rect.left() + d.px(108.0),
        window,
        state: after_window,
        note: after_window + state_w,
        note_end,
        right,
    }
}

/// 组件 `C/QueueRow`：状态点 / 设计库 / 类型 / 会话区间 / 状态 / 进度与说明 / 计时。
/// 返回行的响应，以及「进度 / 说明」那一格有没有装下——装不下的由调用点
/// 在行下面补一行，不许截没。
fn queue_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    row: &RowVm,
    paused: bool,
    open: bool,
) -> (egui::Response, bool) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(34.0)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return (resp, true);
    }
    let tone = row.phase.tone(t);
    if row.phase.running() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, t.bg_header);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, t.bg_hover);
    }
    let c = cols(rect, d);
    let mid = rect.center().y;

    ui.painter().circle_filled(pos2(c.dot, mid), d.px(4.0), tone);
    // 每一行都能展开：终态行摊开来看的是并入会话与欠着的单元，不比运行中那行少。
    let caret = if open { ph::CARET_DOWN } else { ph::CARET_RIGHT };
    glyph(
        ui,
        pos2(c.dot - d.px(10.0), mid),
        caret,
        d.px(11.0),
        t.text_muted,
    );

    text_at(ui, pos2(c.db, mid), &format!("db{}", row.dbnum), Font::mono(d), t.text_primary);
    if !row.db_type.is_empty() {
        let g = layout(ui, &row.db_type, Font::mono_micro(d));
        let tag = Rect::from_min_size(
            pos2(c.ty, mid - d.px(9.0)),
            vec2(g.size().x + d.px(12.0), d.px(18.0)),
        );
        ui.painter()
            .rect_filled(tag, CornerRadius::same(radius::SM), t.bg_input);
        ui.painter().galley(
            pos2(tag.left() + d.px(6.0), mid - g.size().y / 2.0),
            g,
            t.text_secondary,
        );
    }
    text_at(ui, pos2(c.window, mid), &row.window, Font::mono_meta(d), t.text_muted);
    {
        let g = layout(ui, &row.state_text(paused), Font::body(d));
        ui.painter()
            .with_clip_rect(Rect::from_min_max(
                pos2(c.state, rect.top()),
                pos2(c.note.max(c.state), rect.bottom()),
            ))
            .galley(pos2(c.state, mid - g.size().y / 2.0), g, tone);
    }

    let timer_text = match &row.timer {
        Timer::Queued(Some(elapsed)) => format!("已排 {}", clock(*elapsed)),
        Timer::Running(Some(elapsed)) => format!("已用 {}", clock(*elapsed)),
        Timer::Finished(Some(at)) => format!("{at} 结束"),
        // 计时缺席就整格不画：一个从 0 开始爬的秒表比没有秒表更能骗人。
        _ => String::new(),
    };
    if !timer_text.is_empty() {
        let color = match &row.timer {
            Timer::Running(_) => t.text_secondary,
            _ => t.text_muted,
        };
        let g = layout(ui, &timer_text, Font::mono_meta(d));
        ui.painter()
            .galley(pos2(c.right - g.size().x, mid - g.size().y / 2.0), g, color);
    }

    let note_w = (c.note_end - c.note).max(0.0);
    if row.phase == Phase::Generating
        && let (Some(done), Some(total)) = (row.units_done, row.total_units)
        && total > 0
        && note_w > d.px(40.0)
    {
        let h = d.px(8.0);
        let bar = Rect::from_min_size(pos2(c.note, mid - h / 2.0), vec2(note_w, h));
        let cr = CornerRadius::same((h / 2.0) as u8);
        ui.painter().rect_filled(bar, cr, t.bg_input);
        let ratio = (done as f32 / total as f32).clamp(0.0, 1.0);
        if ratio > 0.0 {
            ui.painter().rect_filled(
                Rect::from_min_size(bar.left_top(), vec2(bar.width() * ratio, h)),
                cr,
                t.accent,
            );
        }
        return (resp, true);
    }
    if row.note.is_empty() {
        return (resp, true);
    }
    // 装不下就交回给调用点，让它在行下面另起一行——**宁可多占一行，也不许把
    // 「上一批已冻结」截没**：截掉之后同一个 dbnum 那两行看着就是重复项。
    let g = layout(ui, &row.note, Font::meta(d));
    if g.size().x > note_w {
        return (resp, false);
    }
    let (fg, _) = t.status(row.note_tone);
    let color = if row.note_tone == Status::Neutral {
        t.text_muted
    } else {
        fg
    };
    ui.painter()
        .galley(pos2(c.note, mid - g.size().y / 2.0), g, color);
    (resp, true)
}

/// 行内明细：逐单元事件、并入会话、欠着的单元、断线时缺了多少条。
fn row_detail(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &Vm,
    row: &RowVm,
    cmds: &mut Vec<Cmd>,
) {
    let indent = d.px(28.0);
    egui::Frame::new()
        .inner_margin(Margin {
            left: indent as i8,
            right: d.px(14.0) as i8,
            top: d.px(2.0) as i8,
            bottom: d.px(8.0) as i8,
        })
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = d.px(2.0);

            if let Some(entry) = vm.task(&row.task_id)
                && let Some(merged) = entry
                    .result
                    .as_ref()
                    .and_then(|o| o.batch.as_ref())
                    .map(|b| b.merged_sesnos.as_slice())
                && !merged.is_empty()
            {
                let listed = merged
                    .iter()
                    .map(|s| group(*s as i64))
                    .collect::<Vec<_>>()
                    .join(" – ");
                detail_line(
                    ui,
                    t,
                    d,
                    DetailLine {
                        icon: ph::GIT_MERGE,
                        icon_color: t.accent,
                        label: &format!("{listed} 已并入"),
                        note: "预览之后新存的会话",
                        note_color: t.text_muted,
                    },
                );
            }

            let detail = vm.details.get(&row.task_id);
            if let Feed::Down(_) = vm.feed
                && row.phase.running()
            {
                detail_line(
                    ui,
                    t,
                    d,
                    DetailLine {
                        icon: ph::PLUGS,
                        icon_color: t.warn,
                        label: "断线期间没收到事件，不知道它开没开始",
                        note: &format!("明细缺 {} 条", row.behind_events),
                        note_color: t.warn,
                    },
                );
            }
            for unit in detail.map(|x| x.units.as_slice()).unwrap_or_default() {
                let (icon, color, note) = match unit.state {
                    RowState::Done => (ph::CHECK_CIRCLE, t.success, "生成成功"),
                    RowState::Failed => (ph::X_CIRCLE, t.danger, "生成失败 · 待重试"),
                    RowState::Unknown => (ph::CIRCLE, t.text_muted, "状态未知"),
                    RowState::Running => (ph::ARROWS_CLOCKWISE, t.accent, "生成中"),
                };
                detail_line(
                    ui,
                    t,
                    d,
                    DetailLine {
                        icon,
                        icon_color: color,
                        label: &format!("{} {}", unit.noun, unit.root_refno),
                        note,
                        note_color: color,
                    },
                );
                if let Some(message) = unit.message.as_deref() {
                    sub_line(ui, t, d, message, unit.state == RowState::Failed);
                }
            }

            // 欠着的单元走持久表，跨重启仍在；它与上面那批本次事件不是一回事。
            let owed: Vec<&PendingModelUnit> = vm
                .pending
                .iter()
                .filter(|u| u.dbnum == row.dbnum)
                .collect();
            for unit in &owed {
                if retry_line(ui, t, d, unit) {
                    cmds.push(Cmd::RetryModelUnit {
                        root_refno: unit.root_refno.clone(),
                    });
                }
            }

            if detail.is_none_or(|x| x.units.is_empty()) && owed.is_empty() {
                let text = match row.phase {
                    Phase::Queued => "还没开始：出队冻结之后才会有逐单元明细。",
                    Phase::Applying => "正在写数据，模型生成还没开始。",
                    _ => "本批次没有逐单元明细。",
                };
                ui.label(RichText::new(text).font(Font::micro(d)).color(t.text_muted));
            }
        });
}

/// 明细区的一条行。图标色与行尾说明色分开取：同一条行上「这是什么」与
/// 「它怎么了」未必是一个语气。
struct DetailLine<'a> {
    icon: &'a str,
    icon_color: Color32,
    label: &'a str,
    note: &'a str,
    note_color: Color32,
}

fn detail_line(ui: &mut Ui, t: &Tokens, d: Density, line: DetailLine<'_>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(22.0)), Sense::hover());
    let mut ui = child(ui, rect, Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(13.0), d.px(13.0)), Sense::hover());
    glyph(&ui, mark.center(), line.icon, d.px(12.0), line.icon_color);
    ui.label(
        RichText::new(line.label)
            .font(Font::mono_meta(d))
            .color(t.text_secondary),
    );
    if !line.note.is_empty() {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(line.note)
                    .font(Font::micro(d))
                    .color(line.note_color),
            );
        });
    }
}

/// 欠着的一个交付单元，行尾给「重试」。幂等，但每按一次服务端就真的重跑一遍生成。
fn retry_line(ui: &mut Ui, t: &Tokens, d: Density, unit: &PendingModelUnit) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(30.0)), Sense::hover());
    let mut ui = child(ui, rect, Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(13.0), d.px(13.0)), Sense::hover());
    glyph(&ui, mark.center(), ph::ARROW_COUNTER_CLOCKWISE, d.px(12.0), t.warn);
    ui.label(
        RichText::new(format!("{} {}", unit.noun, unit.root_refno))
            .font(Font::mono_meta(d))
            .color(t.text_secondary),
    );
    ui.label(
        RichText::new(format!("欠着 · 已尝试 {} 次", unit.attempts))
            .font(Font::micro(d))
            .color(t.warn),
    );
    let mut clicked = false;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        clicked = ui
            .add(widgets::button(t, d, "重试"))
            .on_hover_text("重新生成这个交付单元；幂等，可以反复按")
            .clicked();
    });
    clicked
}

fn sub_line(ui: &mut Ui, t: &Tokens, d: Density, text: &str, danger: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(18.0)), Sense::hover());
    let mut ui = child(ui, rect.shrink2(vec2(d.px(21.0), 0.0)), Align::Center);
    ui.label(
        RichText::new(text)
            .font(Font::micro(d))
            .color(if danger { t.danger } else { t.text_muted }),
    );
}

// ---------------------------------------------------------------- 绘制小工具

fn strip_rect(ui: &mut Ui, t: &Tokens, h: f32, fill: Color32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
    hairline(ui.painter(), rect, t.border);
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

fn layout(ui: &Ui, text: &str, font: FontId) -> std::sync::Arc<egui::Galley> {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
}

fn glyph(ui: &Ui, center: egui::Pos2, text: &str, size: f32, color: Color32) {
    let g = layout(ui, text, FontId::new(size, FontFamily::Proportional));
    ui.painter().galley(center - g.size() / 2.0, g, color);
}

/// 左对齐、垂直居中地写一段文字。
fn text_at(ui: &Ui, at: egui::Pos2, text: &str, font: FontId, color: Color32) {
    let g = layout(ui, text, font);
    ui.painter()
        .galley(pos2(at.x, at.y - g.size().y / 2.0), g, color);
}

// ---------------------------------------------------------------- 数与时间

/// 会话区间。画板上千位是分开写的（`sesno 1 024 → 1 038`），四位数的会话号连着写
/// 一眼数不清。
fn window(start: i32, end: i32) -> String {
    format!("sesno {} → {}", group(start as i64), group(end as i64))
}

fn group(value: i64) -> String {
    let negative = value < 0;
    let digits = value.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(' ');
        }
        out.push(ch);
    }
    if negative { format!("-{out}") } else { out }
}

fn clock(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    if s < 3600 {
        format!("{:02}:{:02}", s / 60, s % 60)
    } else {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    }
}

fn parse(stamp: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(stamp)
        .ok()
        .map(|at| at.with_timezone(&Local))
}

/// 服务端时刻到此刻的时长。解不出来、或者时钟倒着走时给 `None`——宁可少一格计时，
/// 也不摆一个从 0 开始爬的假秒表。
fn since(stamp: &str) -> Option<Duration> {
    Local::now()
        .signed_duration_since(parse(stamp)?)
        .to_std()
        .ok()
}

fn hhmm(stamp: &str) -> Option<String> {
    Some(parse(stamp)?.format("%H:%M").to_string())
}

/// 房间泳道的「已等待」按分钟报。秒级精度在这里没有意义——它衡量的是
/// 「队列忙了多久没腾出空来」。
fn minutes(elapsed: Duration) -> String {
    let m = elapsed.as_secs() / 60;
    if m < 60 {
        format!("{m} 分钟")
    } else {
        format!("{} 小时 {} 分钟", m / 60, m % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued(task_id: &str, dbnum: u32, start: i32, end: i32) -> QueueRow {
        QueueRow {
            task_id: task_id.into(),
            dbnum,
            db_type: "DESI".into(),
            state: "queued".into(),
            start_sesno: start,
            end_sesno: end,
        }
    }

    fn entry(task_id: &str, dbnum: u32, state: &str) -> TaskEntry {
        TaskEntry {
            task_id: task_id.into(),
            kind: KIND_DATA_BATCH.into(),
            state: state.into(),
            project: "ProjAMS".into(),
            dbnum: Some(dbnum),
            db_type: Some("DESI".into()),
            ..Default::default()
        }
    }

    fn vm(queue: Vec<QueueRow>, tasks: Vec<TaskEntry>) -> Vm {
        Vm {
            project: "ProjAMS".into(),
            queue: QueueSnapshot {
                paused: false,
                rows: queue,
            },
            tasks,
            loaded: true,
            ..Default::default()
        }
    }

    /// 契约形状对不上是最贵的一类错：终态摘要的 `batch` 是**单数**，
    /// 解成数组的话会静默空掉，界面上看不出任何异样。
    #[test]
    fn terminal_result_decodes_the_singular_batch() {
        let list: TaskList = serde_json::from_str(
            r#"{"tasks":[{
                "task_id":"db-7997-1","kind":"data_batch","state":"partial","project":"ProjAMS",
                "created_at":"2026-07-27T10:00:00+08:00","started_at":"2026-07-27T10:00:05+08:00",
                "finished_at":"2026-07-27T10:04:00+08:00","dbnum":7997,"db_type":"DESI",
                "start_sesno":1024,"end_sesno":1038,"units_done":22,"total_units":24,
                "events_seen":48,
                "result":{"project":"ProjAMS","status":"partial",
                    "batch":{"dbnum":7997,"db_type":"DESI","file_path":"d.db",
                             "start_sesno":1024,"end_sesno":1038,"status":"applied",
                             "merged_sesnos":[1032,1034],"changed_elements":512},
                    "units":[{"dbnum":7997,"root_refno":"24384/12","noun":"EQUI",
                              "status":"failed","attempts":3}],
                    "warnings":[]}
            }]}"#,
        )
        .expect("终态任务行应当能解出来");
        let task = &list.tasks[0];
        let batch = task.result.as_ref().unwrap().batch.as_ref().unwrap();
        assert_eq!(batch.merged_sesnos, vec![1032, 1034]);
        assert_eq!(task.failed_units()[0].attempts, 3);
    }

    /// 房间轮的 `detail` 与数据批次的 `result` 是两种形状，不能互相踩。
    #[test]
    fn room_round_detail_feeds_the_summary_count() {
        let list: TaskList = serde_json::from_str(
            r#"{"tasks":[{"task_id":"room-1","kind":"room_recalc","state":"running",
                "project":"ProjAMS","created_at":"2026-07-27T10:00:00+08:00",
                "events_seen":0,"units_done":18,"total_units":30,
                "detail":{"panels":3,"elements":27,"dead_letters":2}}]}"#,
        )
        .expect("房间轮任务行应当能解出来");
        let mut model = vm(Vec::new(), list.tasks);
        model.health = Some(Health {
            gen_spatial_tree: true,
            ..Default::default()
        });
        assert_eq!(model.rooms_pending(), Some(30));
        model.tasks[0].detail = None;
        // 详情缺席就退回任务分母，而不是把这一格算成 0。
        assert_eq!(model.rooms_pending(), Some(30));
    }

    /// 同一个 dbnum 至多两行：一行运行中、一行排队中。下面那行必须说清自己是
    /// 冻结之后新存的会话，否则看着就是重复项。
    #[test]
    fn a_second_row_for_the_same_dbnum_explains_itself() {
        let mut running = queued("db-7997-1", 7997, 1024, 1038);
        running.state = "running".into();
        let model = vm(
            vec![running, queued("db-7997-2", 7997, 1039, 1041)],
            vec![entry("db-7997-1", 7997, "running"), entry("db-7997-2", 7997, "queued")],
        );
        let out = rows(&model);
        assert_eq!(out.len(), 2);
        assert!(!out[0].behind_frozen);
        assert!(out[1].behind_frozen);
        assert_eq!(out[1].note, "上一批已冻结，这是之后新存的会话");
    }

    /// 「排在第几位」数的是队列里还排着几个库——运行中的行不占位置。
    #[test]
    fn positions_count_queued_rows_only() {
        let mut running = queued("db-1", 1, 0, 5);
        running.state = "running".into();
        let model = vm(
            vec![running, queued("db-2", 2, 0, 5), queued("db-3", 3, 0, 5)],
            vec![
                entry("db-1", 1, "running"),
                entry("db-2", 2, "queued"),
                entry("db-3", 3, "queued"),
            ],
        );
        let out = rows(&model);
        assert_eq!(out[0].position, None);
        assert_eq!(out[1].position, Some(1));
        assert_eq!(out[2].position, Some(2));
    }

    /// 「应用中」与「生成中」的分界由两阶段给：有了单元分母或收到第一条单元事件
    /// 才算进入阶段二。
    #[test]
    fn phase_two_starts_at_the_first_unit_not_at_dequeue() {
        let mut running = queued("db-7997-1", 7997, 1024, 1038);
        running.state = "running".into();
        let mut model = vm(vec![running], vec![entry("db-7997-1", 7997, "running")]);
        assert_eq!(rows(&model)[0].phase, Phase::Applying);

        model.apply(
            "db-7997-1",
            serde_json::from_str(
                r#"{"kind":"model_unit_started","dbnum":7997,"root_refno":"24384/1","noun":"BRAN"}"#,
            )
            .unwrap(),
        );
        assert_eq!(rows(&model)[0].phase, Phase::Generating);
    }

    /// 搜索不受筛选芯片限制：默认视图收起了已完成行，但搜一个已完成的库要搜得到。
    #[test]
    fn search_reaches_rows_the_filter_hides() {
        let mut done = entry("db-8004-9", 8004, "succeeded");
        done.finished_at = Some("2026-07-27T10:04:00+08:00".into());
        let model = vm(vec![queued("db-7997-1", 7997, 1024, 1038)], vec![done]);
        let all = rows(&model);
        assert_eq!(visible(&all, Filter::Changed, "").len(), 1);
        assert_eq!(visible(&all, Filter::Changed, "8004").len(), 1);
    }

    /// 运行中置顶：队列再长也要一眼看得见现在在跑什么。
    #[test]
    fn the_running_row_floats_to_the_top() {
        let mut running = queued("db-9-1", 9, 0, 5);
        running.state = "running".into();
        let model = vm(
            vec![queued("db-2", 2, 0, 5), queued("db-3", 3, 0, 5), running],
            vec![
                entry("db-2", 2, "queued"),
                entry("db-3", 3, "queued"),
                entry("db-9-1", 9, "running"),
            ],
        );
        let all = rows(&model);
        let order = visible(&all, Filter::All, "");
        assert_eq!(all[order[0]].dbnum, 9);
    }

    /// 只画当前项目，且**过滤要出声**：别的项目那几条不显示，但计数说得出来。
    #[test]
    fn other_projects_are_dropped_but_counted() {
        let mut theirs = entry("db-5-1", 5, "queued");
        theirs.project = "OtherProj".into();
        let model = vm(
            vec![queued("db-2", 2, 0, 5), queued("db-5-1", 5, 0, 5)],
            vec![entry("db-2", 2, "queued"), theirs],
        );
        assert_eq!(rows(&model).len(), 1);
        assert_eq!(filtered_out(&model), 1);
    }

    /// 「有变化」= 还有活没干完。已完成且不欠单元的收起来，欠着单元的部分完成留下。
    #[test]
    fn changed_keeps_partial_rows_that_still_owe_units() {
        let mut ok = entry("db-1-9", 1, "succeeded");
        ok.finished_at = Some("2026-07-27T10:04:00+08:00".into());
        let mut partial = entry("db-2-9", 2, "partial");
        partial.finished_at = Some("2026-07-27T10:05:00+08:00".into());
        let mut model = vm(Vec::new(), vec![ok, partial]);
        model.pending.push(PendingModelUnit {
            dbnum: 2,
            root_refno: "24384/12".into(),
            noun: "EQUI".into(),
            attempts: 3,
            ..Default::default()
        });
        let all = rows(&model);
        let shown = visible(&all, Filter::Changed, "");
        assert_eq!(shown.len(), 1);
        assert_eq!(all[shown[0]].dbnum, 2);
        assert_eq!(all[shown[0]].state_text(false), "部分完成 · 欠 1 个单元");
    }

    /// 断线时明细行降级成「状态未知」，而落后条数只用来解释缺了多少，
    /// **不许拿它反推进度**。
    #[test]
    fn a_dropped_feed_degrades_rows_and_reports_the_gap() {
        let mut running = queued("db-7997-1", 7997, 1024, 1038);
        running.state = "running".into();
        let mut task = entry("db-7997-1", 7997, "running");
        task.events_seen = 31;
        let mut model = vm(vec![running], vec![task]);
        model.apply(
            "db-7997-1",
            serde_json::from_str(
                r#"{"kind":"model_unit_started","dbnum":7997,"root_refno":"24384/1","noun":"BRAN"}"#,
            )
            .unwrap(),
        );
        model.mark_unknown();
        assert_eq!(
            model.details["db-7997-1"].units[0].state,
            RowState::Unknown
        );
        assert_eq!(rows(&model)[0].behind_events, 30);
    }

    /// 认不出的 `state` 串归「状态未知」。**不许兜底成「排队中」**——那是一个有
    /// 契约来源的真实状态，拿它当兜底会把「没排上」和「不知道」说成同一件事。
    #[test]
    fn an_unknown_state_string_never_degrades_to_queued() {
        let mut odd = queued("db-7-1", 7, 0, 5);
        odd.state = "paused_forever".into();
        let model = vm(vec![odd], vec![entry("db-7-1", 7, "paused_forever")]);
        let out = rows(&model);
        assert_eq!(out[0].phase, Phase::Unknown);
        assert_eq!(out[0].state_text(false), "状态未知");
        // 认不出的行也不该占掉一个排队位置。
        assert_eq!(out[0].position, None);
    }

    /// 房间增量没开时顶栏那一格整个不画——画出来必然是个永远不动的 0。
    #[test]
    fn the_room_count_stays_hidden_while_the_feature_is_off() {
        let mut model = vm(Vec::new(), vec![TaskEntry {
            task_id: "room-1".into(),
            kind: KIND_ROOM_RECALC.into(),
            state: "running".into(),
            total_units: Some(30),
            ..Default::default()
        }]);
        model.health = Some(Health {
            gen_spatial_tree: false,
            ..Default::default()
        });
        assert_eq!(model.rooms_pending(), None);
        model.health = Some(Health {
            gen_spatial_tree: true,
            ..Default::default()
        });
        assert_eq!(model.rooms_pending(), Some(30));
    }

    fn room(state: &str, counts: RoomCounts, finished_at: Option<&str>) -> TaskEntry {
        TaskEntry {
            task_id: "room-1".into(),
            kind: KIND_ROOM_RECALC.into(),
            state: state.into(),
            detail: Some(counts),
            finished_at: finished_at.map(str::to_owned),
            ..Default::default()
        }
    }

    fn health(gen_spatial_tree: bool) -> Health {
        Health {
            gen_spatial_tree,
            ..Default::default()
        }
    }

    /// 房间增量没开时泳道只说「没开」这一句。当前项目 PANE / CWALL / CFLOOR 三张表
    /// 全空，画一条永远是 0 的泳道比不画更糟。
    #[test]
    fn the_lane_says_only_that_the_feature_is_off() {
        let mut model = vm(Vec::new(), vec![room("running", RoomCounts { panels: 3, elements: 27, dead_letters: 2 }, None)]);
        model.health = Some(health(false));
        let off = lane(&model).expect("读到 health 就说得出开没开");
        assert_eq!(off.state, LaneState::Off);
        assert_eq!((off.panels, off.elements, off.dead_letters), (0, 0, 0));

        // 连 health 都没读到就一个字不提——说不出开没开的时候不许猜。
        model.health = None;
        assert!(lane(&model).is_none());

        // 开着、但一轮都没收过：那几个计数一个都不许摆，「0 块面板待重算」
        // 与「从来没收过一轮」是两回事。
        let mut fresh = vm(Vec::new(), Vec::new());
        fresh.health = Some(health(true));
        let never = lane(&fresh).unwrap();
        assert!(!never.seen);
        assert_eq!(never.state, LaneState::Converged);
    }

    /// 饥饿只在「有活却排不上」时成立：已经收敛干净的泳道等再久也不算饿着，
    /// 那条 `$danger-bg` 的横幅不该为它亮起来。
    #[test]
    fn only_a_lane_with_work_left_can_starve() {
        let long_ago = (Local::now() - chrono::Duration::minutes(47)).to_rfc3339();
        let mut model = vm(
            Vec::new(),
            vec![room("succeeded", RoomCounts { panels: 5, elements: 63, dead_letters: 2 }, Some(&long_ago))],
        );
        model.health = Some(health(true));
        let waiting = lane(&model).unwrap();
        assert_eq!(waiting.state, LaneState::Waiting);
        assert!(waiting.starving);

        model.tasks[0].detail = Some(RoomCounts::default());
        let converged = lane(&model).unwrap();
        assert_eq!(converged.state, LaneState::Converged);
        assert!(!converged.starving, "没活可干的泳道等再久也不是饿着");
    }

    /// 阻断与排除**不许合成一行**：一个是出事了、一个是本来就不跑，出路完全不同。
    /// 阻断排在前面——它才是「这个库的水位为什么一直不动」的答案。
    #[test]
    fn blocked_and_excluded_stay_two_different_things() {
        let mut model = vm(Vec::new(), Vec::new());
        model.dbnums = vec![
            DbnumStatus { dbnum: 7995, db_type: "CATA".into(), excluded: true, ..Default::default() },
            DbnumStatus {
                dbnum: 8003,
                db_type: "DESI".into(),
                blocked: true,
                anomaly: Some(FileAnomaly::Rollback { file_latest_sesno: 812, applied_sesno: 1005 }),
                ..Default::default()
            },
            // 正常入队的库不进这一格。
            DbnumStatus { dbnum: 7997, db_type: "DESI".into(), ..Default::default() },
        ];
        let rows = not_running(&model);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].dbnum, 8003, "阻断排在前面");
        assert!(rows[0].blocked && !rows[0].excluded);
        assert!(rows[1].excluded && !rows[1].blocked);
        assert_eq!(
            anomaly_brief(rows[0].anomaly.as_ref().unwrap()),
            "文件回退 812 < 已应用 1 005"
        );
    }

    /// 计时解不出来就整格不画，绝不从 0 起算。
    #[test]
    fn an_unparseable_timestamp_yields_no_stopwatch() {
        assert!(since("").is_none());
        assert!(hhmm("not-a-time").is_none());
        assert_eq!(window(1024, 1038), "sesno 1 024 → 1 038");
        assert_eq!(clock(Duration::from_secs(3725)), "1:02:05");
    }
}
