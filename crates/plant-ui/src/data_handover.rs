//! 「三维数据提资」任务窗（画板 S15 / S15-A / S15-B，形态见 S15-C）。
//!
//! 这是与旧入口「三维数据接口」（[`crate::data_publish`]）并排的**新入口**，旧窗一行不动。表单状态、
//! 预览子窗与确认删除子窗是从旧入口借来的（同一个 `data_publish::State`、同两段绘制）；本窗多出来的是
//! 右栏的提资前检查、「更新到最新」与底部的硬拦——检查没放行，「提交」就一直灰着，不提供绕过
//! （`docs/adr/0036`）。
//!
//! 判据不在这里：一行是什么形态、整体是什么判决，都由 [`crate::data_publish_check`] 给。这里只管三件事：
//! 什么时候去问（打开、提资元素增删、点「重新检查」、队列里这几个库的活变了、更新落定、点「提交」），
//! 一次「更新到最新」跑到哪了，以及把这些画出来。检查只读，不排任何任务；取回工作由宿主在任务落定时
//! 照旧自动做，这里不另发。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use web_time::{Duration, Instant};

use egui::{
    Align, Color32, ComboBox, CornerRadius, FontFamily, FontId, Layout, Margin, Rect, RichText,
    ScrollArea, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use egui_phosphor::regular as ph;

use crate::data_publish::{
    self, DeletionCandidate, DesignPhase, PreviewLayerChoice, PreviewStatus, PublishCategory,
    PublishElement, PublishPreviewData, PublishRequest,
};
use crate::data_publish_check::{self as check, Class, LibForm, LibRow, Outcome, Report, Verdict};
use crate::model_update::Enqueued;
use crate::style::group_number;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius};
use crate::style::widgets;
use crate::task_queue::{self, Phase};
use crate::vm::WorkbenchVm;
use crate::workbench::Pane;
use crate::{Cmd, RefU64};

/// 有库在排、在跑或模型在追赶时，隔这么久再问一次：模型追平那一刻没有队列行可看。
const RECHECK_EVERY: Duration = Duration::from_secs(3);
/// 回执说排上了、队列快照里却一直没见到它的活：等这么久就不等了，按落定去重新检查。
const SETTLE_GRACE: Duration = Duration::from_secs(20);

// ================================================================ 动作与状态

/// 本窗发给宿主的动作（`Cmd::DataHandover`）。
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// 菜单「创建 → 三维数据提资」。
    Open,
    /// 提资前检查：逐个元素问模型服务。只读，不排任何任务。`epoch` 原样带回，过期的回包丢弃。
    Check {
        epoch: u64,
        refnos: Vec<RefU64>,
    },
    /// 「更新到最新」：两份名单各发一次执行请求，落后的先发，回执到了再发需初始化的。
    Update {
        lagging: Vec<u32>,
        needs_init: Vec<u32>,
    },
    /// 下面三条与旧入口打的是同一组接口，回包落到本窗自己的状态上。
    Preview(PublishRequest),
    LookupDeletions(PublishRequest),
    Publish(PublishRequest),
}

#[derive(Debug, Default)]
pub struct State {
    pub open: bool,
    /// 收成一条：只留窗口头，好去模型树或三维视图里选元素。
    pub collapsed: bool,
    /// 表单、预览子窗与确认删除子窗的状态，与旧入口同一个类型。
    pub form: data_publish::State,
    check: CheckState,
    /// 从这扇窗发起、还没落定的那一次「更新到最新」。
    run: Option<Run>,
    /// 刚落定的那一次，留着说结果。
    last_run: Option<Run>,
    /// 点了「提交」、提交前那次检查还在途。
    submit_pending: bool,
    notice: Option<Notice>,
    /// 提资元素所在的库上一次的状态签名（队列里的活 + 库登记表上的两枚水位），以及它是对着第几份检查结果记的。
    signature: check::QueueSignature,
    signature_of: u64,
}

#[derive(Debug, Default)]
struct CheckState {
    epoch: u64,
    /// 收下过几份结果。
    adopted: u64,
    /// 在途那次检查问的元素。
    pending: Option<Vec<RefU64>>,
    outcome: Option<Outcome>,
    /// `outcome` 是为哪份元素清单查的。
    checked: Vec<RefU64>,
    asked_at: Option<Instant>,
}

#[derive(Debug, Clone, PartialEq)]
struct Notice {
    tone: Status,
    text: String,
}

/// 一次「更新到最新」。
#[derive(Debug, Clone, Default)]
struct Run {
    libs: BTreeMap<u32, LibRun>,
    /// 执行请求的回执还没到齐。
    sending: bool,
    queued_at: Option<Instant>,
}

#[derive(Debug, Clone, Default)]
struct LibRun {
    first_import: bool,
    /// 回执点到了它（新排、并入、或已被排队中的批次覆盖）。没点到的这次没有活。
    named: bool,
    task_id: Option<String>,
    /// 在队列快照里见过它的活动行。
    seen: bool,
    done: bool,
}

/// 这一帧右栏要画的东西。
#[derive(Debug, Clone, PartialEq)]
enum View {
    /// 还没有提资元素。
    Empty,
    /// 在问，手上没有可画的事实。
    Checking,
    /// 这台模型服务判不了：整块检查不画，提资照旧、不拦。
    Unsupported(String),
    Checked {
        report: Report,
        at: String,
        /// 手上这份事实照画，另有一次检查在途。
        rechecking: bool,
    },
}

/// 「提交」旁边那枚此刻的主操作。
#[derive(Debug, Clone, PartialEq)]
enum Side {
    Update {
        libs: usize,
        /// 此刻为什么点不了（队列暂停、服务不消化、身份没对上）。
        blocked: Option<String>,
    },
    Updating,
    RemoveStuck(Vec<RefU64>),
    Recheck,
}

/// 底部操作条要画的东西。
#[derive(Debug, Clone, PartialEq)]
struct Footer {
    /// 「提交」点不点得了；点不了时是悬停上的理由。
    submit: Result<(), String>,
    /// 「提交」画成加载中。
    busy: bool,
    /// `None` = 主操作就是「提交」。
    side: Option<Side>,
    /// 一次提交在途，旁边那枚也先别点。
    locked: bool,
    tone: Status,
    icon: &'static str,
    hint: String,
}

/// 表单、预览与确认删除是从旧入口借来的，它们发出的是旧入口的命令；换成本窗的，回包才落得回本窗。
fn own(cmd: Cmd) -> Cmd {
    match cmd {
        Cmd::PreviewDataPublish(request) => Cmd::DataHandover(Action::Preview(request)),
        Cmd::LookupDataPublishDeletions(request) => {
            Cmd::DataHandover(Action::LookupDeletions(request))
        }
        Cmd::SubmitDataPublish(request) => Cmd::DataHandover(Action::Publish(request)),
        other => other,
    }
}

/// 一份执行回执写进日志的那一句。只说这次点名的库：全范围两三百个库，照通用回执那样把「未勾选跳过」
/// 逐个列出来就是刷屏。`kind` 是「增量更新」或「首次导入」。
pub fn receipt_line(kind: &str, asked: &[u32], receipt: &Enqueued) -> String {
    let mut parts = Vec::new();
    for batch in &receipt.enqueued {
        parts.push(format!(
            "db{} 排第 {} 位{}",
            batch.dbnum,
            batch.position,
            if batch.rebuilds() {
                "（文件回退 · 清库整库重建）"
            } else {
                ""
            }
        ));
    }
    for batch in &receipt.merged {
        parts.push(format!("db{} 并入已排队的批次", batch.dbnum));
    }
    for dbnum in &receipt.already_covered {
        parts.push(format!("db{dbnum} 已被排队中的批次覆盖"));
    }
    for blocked in receipt
        .blocked
        .iter()
        .filter(|blocked| asked.contains(&blocked.dbnum))
    {
        parts.push(format!(
            "db{} 阻断未入队（{}）",
            blocked.dbnum, blocked.reason
        ));
    }
    if parts.is_empty() {
        format!("提资前更新 · {kind}：这次一行都没排上（这些库可能已经追平）")
    } else {
        format!("提资前更新 · {kind}：{}", parts.join("；"))
    }
}

impl State {
    /// 菜单点进来：打开并展开。
    pub fn request_open(&mut self) {
        self.open = true;
        self.collapsed = false;
    }

    /// 检查的回包。过期的（又发过一次、或窗已经关了）丢弃。
    pub fn adopt_check(&mut self, epoch: u64, outcome: Outcome) {
        if epoch != self.check.epoch {
            return;
        }
        let Some(asked) = self.check.pending.take() else {
            return;
        };
        self.check.checked = asked;
        self.check.outcome = Some(outcome);
        self.check.adopted += 1;
    }

    /// 一份执行回执：记下它点到了哪些库、各是哪条任务。
    pub fn update_receipt(&mut self, receipt: &Enqueued) {
        let Some(run) = self.run.as_mut() else {
            return;
        };
        for batch in receipt.enqueued.iter().chain(&receipt.merged) {
            if let Some(lib) = run.libs.get_mut(&batch.dbnum) {
                lib.named = true;
                lib.task_id = Some(batch.task_id.clone());
            }
        }
        for dbnum in &receipt.already_covered {
            if let Some(lib) = run.libs.get_mut(dbnum) {
                lib.named = true;
            }
        }
    }

    /// 回执到齐：从此按队列里的活说话。回执没点到的库（已追平、被阻断）这次没有活，不等它。
    pub fn update_queued(&mut self) {
        let Some(run) = self.run.as_mut() else {
            return;
        };
        run.sending = false;
        run.queued_at = Some(Instant::now());
        for lib in run.libs.values_mut() {
            lib.done |= !lib.named;
        }
    }

    /// 执行请求没发成（或只发成了前一份）：已排上的照常跟，没排上的说出来。
    pub fn update_failed(&mut self, reason: &str) {
        self.update_queued();
        self.notice = Some(Notice {
            tone: Status::Error,
            text: format!("更新没有全部提交成功：{reason}"),
        });
    }

    /// 预览的回包。
    pub fn adopt_preview(&mut self, result: Result<PublishPreviewData, String>) {
        match result {
            Ok(preview) => {
                self.form.preview_layer = PreviewLayerChoice::Datacenter;
                self.form.preview_selected_instance =
                    (!preview.datacenter.instances.is_empty()).then_some(0);
                self.form.preview_status = PreviewStatus::Ready(preview);
            }
            Err(reason) => self.form.preview_status = PreviewStatus::Failed(reason),
        }
    }

    /// 待删除提资的回包：查到了就开确认删除子窗。
    pub fn adopt_deletions(&mut self, result: Result<Vec<DeletionCandidate>, String>) {
        self.form.submitting = false;
        match result {
            Ok(candidates) => {
                self.form.deletion_candidates = candidates;
                self.form.delete_confirmation_open = true;
            }
            Err(reason) => {
                self.form.pending_request = None;
                self.notice = Some(Notice {
                    tone: Status::Error,
                    text: format!("获取待删除提资失败，这次没有提交：{reason}"),
                });
            }
        }
    }

    /// 提交的回包。`Ok` 里是接口回的那一句。
    pub fn adopt_published(&mut self, result: Result<String, String>) {
        self.form.submitting = false;
        self.form.delete_confirmation_open = false;
        self.form.pending_request = None;
        self.form.deletion_candidates.clear();
        self.notice = Some(match result {
            Ok(message) => Notice {
                tone: Status::Success,
                text: format!("已提交：{message}"),
            },
            Err(reason) => Notice {
                tone: Status::Error,
                text: format!("提交失败：{reason}"),
            },
        });
    }

    fn refnos(&self) -> Vec<RefU64> {
        self.form
            .request
            .elements
            .iter()
            .map(|element| element.refno)
            .collect()
    }

    fn name_of(&self, refno: RefU64) -> String {
        self.form
            .request
            .elements
            .iter()
            .find(|element| element.refno == refno)
            .map(display_name)
            .unwrap_or_else(|| refno.to_pdms_str())
    }

    fn add(&mut self, element: PublishElement) {
        if self
            .form
            .request
            .elements
            .iter()
            .all(|existing| existing.refno != element.refno)
        {
            self.form.request.elements.push(element);
            self.touched();
        }
    }

    fn remove(&mut self, refnos: &[RefU64]) {
        self.form
            .request
            .elements
            .retain(|element| !refnos.contains(&element.refno));
        if self
            .form
            .selected_refno
            .is_some_and(|selected| refnos.contains(&selected))
        {
            self.form.selected_refno = None;
        }
        self.touched();
    }

    fn clear(&mut self) {
        self.form.request.elements.clear();
        self.form.selected_refno = None;
        self.touched();
    }

    /// 提资元素变了：上一句提示与等着的提交都不再作数。
    fn touched(&mut self) {
        self.notice = None;
        self.submit_pending = false;
    }

    fn start_check(&mut self, refnos: Vec<RefU64>, cmds: &mut Vec<Cmd>) {
        self.check.epoch += 1;
        self.check.asked_at = Some(Instant::now());
        self.check.pending = Some(refnos.clone());
        cmds.push(Cmd::DataHandover(Action::Check {
            epoch: self.check.epoch,
            refnos,
        }));
    }

    fn recheck(&mut self, cmds: &mut Vec<Cmd>) {
        let refnos = self.refnos();
        if !refnos.is_empty() {
            self.notice = None;
            self.start_check(refnos, cmds);
        }
    }

    /// 发起一次「更新到最新」：记下这次更新，回给宿主要发的那条命令。按钮点下去走的就是它；组件画廊摆
    /// 「更新中」形态时也经它进来，所以是公开的。
    pub fn begin_update(&mut self, lagging: &[u32], needs_init: &[u32]) -> Cmd {
        self.update_started(lagging, needs_init);
        Cmd::DataHandover(Action::Update {
            lagging: lagging.to_vec(),
            needs_init: needs_init.to_vec(),
        })
    }

    /// 「更新到最新」按下的那一刻：回执到齐之前按钮不许再亮。
    fn update_started(&mut self, lagging: &[u32], needs_init: &[u32]) {
        let mut libs = BTreeMap::new();
        for (dbnums, first_import) in [(lagging, false), (needs_init, true)] {
            for dbnum in dbnums {
                libs.insert(
                    *dbnum,
                    LibRun {
                        first_import,
                        ..Default::default()
                    },
                );
            }
        }
        self.run = Some(Run {
            libs,
            sending: true,
            queued_at: None,
        });
        self.last_run = None;
        self.notice = None;
    }

    /// 点「提交」：先重新检查一次，放行了才去查待删除（见 [`Self::resolve_submit`]）。
    fn submit(&mut self, view: &View, cmds: &mut Vec<Cmd>) {
        self.notice = None;
        match view {
            // 判不了的服务端不拦：与旧入口同一条路。
            View::Unsupported(_) => cmds.push(own(self.form.start_lookup())),
            View::Checked { .. } => {
                self.submit_pending = true;
                let refnos = self.refnos();
                self.start_check(refnos, cmds);
            }
            View::Empty | View::Checking => {}
        }
    }

    /// 提交前那次检查回来了：放行就接着查待删除，不放行就说一声、不提交。
    fn resolve_submit(&mut self, view: &View, cmds: &mut Vec<Cmd>) {
        if !self.submit_pending {
            return;
        }
        let pass = match view {
            View::Unsupported(_) => true,
            View::Checking
            | View::Checked {
                rechecking: true, ..
            } => return,
            View::Checked { report, .. } => report.allows_submit() && self.run.is_none(),
            View::Empty => false,
        };
        self.submit_pending = false;
        if pass {
            cmds.push(own(self.form.start_lookup()));
        } else {
            self.notice = Some(Notice {
                tone: Status::Warn,
                text: "提交前重新检查：要提资的数据不是最新，这次没有提交".to_owned(),
            });
        }
    }

    /// 本窗刚排上、还没落定的库。队列快照跟上之前，这些库按「已排上」说。
    fn claimed(&self) -> BTreeSet<u32> {
        self.run
            .iter()
            .filter(|run| !run.sending)
            .flat_map(|run| run.libs.iter())
            .filter(|(_, lib)| lib.named && !lib.done)
            .map(|(dbnum, _)| *dbnum)
            .collect()
    }

    /// 跟一下这次更新。返回「刚落定」。
    ///
    /// 按库号跟而不按任务号：一个库可以有两行（一行在跑、一行排着），而「已被排队中的批次覆盖」的库
    /// 回执里根本没有任务号。一个库落定 = 见过它的活动行、现在一行都没有了；跑得太快、一拍都没赶上
    /// 的，认任务表里那条任务的终态；两样都等不到就到点不等。
    fn track_run(&mut self, queue: &task_queue::Vm) -> bool {
        let Some(run) = self.run.as_mut() else {
            return false;
        };
        if run.sending {
            return false;
        }
        let overdue = run.queued_at.is_none_or(|at| at.elapsed() >= SETTLE_GRACE);
        for (dbnum, lib) in run.libs.iter_mut().filter(|(_, lib)| !lib.done) {
            match queue.queue.rows.iter().find(|row| row.dbnum == *dbnum) {
                Some(row) => {
                    lib.seen = true;
                    if lib.task_id.is_none() {
                        lib.task_id = Some(row.task_id.clone());
                    }
                }
                None => {
                    let finished = lib.task_id.as_deref().is_some_and(|task_id| {
                        queue
                            .tasks
                            .iter()
                            .any(|task| task.task_id == task_id && task.terminal())
                    });
                    lib.done = lib.seen || finished || overdue;
                }
            }
        }
        if run.libs.values().all(|lib| lib.done) {
            self.last_run = self.run.take();
            true
        } else {
            false
        }
    }

    /// 每帧一次：跟一下这次更新、决定要不要重新检查、把提交前那次检查接上。返回这一帧要画的东西。
    fn tick(&mut self, queue: &task_queue::Vm, cmds: &mut Vec<Cmd>) -> View {
        let refnos = self.refnos();
        if refnos.is_empty() {
            self.check.pending = None;
            self.check.outcome = None;
            self.check.checked.clear();
            self.submit_pending = false;
            self.run = None;
            self.last_run = None;
            return View::Empty;
        }
        // 队列轮询已经说得出这台模型服务判不了（读透形态）：不问，也不拦。
        if let Some(reason) = check::not_applicable(queue) {
            self.check.pending = None;
            self.check.checked = refnos;
            self.check.outcome = Some(Outcome::Unsupported(reason.clone()));
            self.run = None;
            let view = View::Unsupported(reason);
            self.resolve_submit(&view, cmds);
            return view;
        }

        // 这次更新刚落定：手上的事实是更新之前的，照着画会退回「落后」、按钮又亮一下——整份丢掉重问。
        let mut ask = self.track_run(queue);
        if ask {
            self.check.outcome = None;
        }
        // 查过之后又移出列表的元素不再算数；新加进来还没查的没有事实，等这一次检查回来。
        if let Some(Outcome::Checked { elements, .. }) = self.check.outcome.as_mut() {
            elements.retain(|element| refnos.contains(&element.refno));
        }
        ask |= self.check.checked != refnos && self.check.pending.as_ref() != Some(&refnos);

        let claimed = self.claimed();
        let mut report = match &self.check.outcome {
            Some(Outcome::Checked { elements, .. }) => {
                Some(check::judge(elements, queue, &claimed))
            }
            _ => None,
        };

        // 队列里这几个库的活变了（别处排的任务开始或结束），或者库登记表上它们的水位动了（文件又来了新保存）：
        // 手上的事实旧了。只有任务落定才把旧事实整份丢掉；水位动了照旧画着，等新的一份回来。
        if let Some(current) = &report {
            let libs: BTreeSet<u32> = current.rows.iter().map(|row| row.dbnum).collect();
            let signature = check::queue_signature(queue, &libs);
            if self.signature_of != self.check.adopted {
                // 新一份事实刚落地：以此刻为基线，不为它再问一次。
                self.signature_of = self.check.adopted;
                self.signature = signature;
            } else if signature != self.signature {
                let finished = signature.settled_since(&self.signature);
                self.signature = signature;
                ask = true;
                if finished {
                    self.check.outcome = None;
                    report = None;
                }
            }
        }

        let in_progress = report.as_ref().is_some_and(|report| {
            report
                .rows
                .iter()
                .any(|row| row.form.class() == Class::InProgress)
        });
        if in_progress
            && self.check.pending.is_none()
            && self
                .check
                .asked_at
                .is_none_or(|at| at.elapsed() >= RECHECK_EVERY)
        {
            ask = true;
        }
        if ask {
            self.start_check(refnos, cmds);
        }

        let view = match (&self.check.outcome, report) {
            (Some(Outcome::Unsupported(reason)), _) => View::Unsupported(reason.clone()),
            (Some(Outcome::Checked { at, .. }), Some(report)) => View::Checked {
                report,
                at: at.clone(),
                rechecking: self.check.pending.is_some(),
            },
            _ => View::Checking,
        };
        self.resolve_submit(&view, cmds);
        view
    }

    /// 这一帧之后还有没有东西在变（在问、在更新、在追赶）。有就让界面每秒醒一次。
    fn busy(&self, view: &View) -> bool {
        self.check.pending.is_some()
            || self.run.is_some()
            || matches!(
                view,
                View::Checked { report, .. }
                    if report.rows.iter().any(|row| row.form.class() == Class::InProgress)
            )
    }

    fn footer(&self, view: &View) -> Footer {
        let plain =
            |submit: Result<(), String>, tone: Status, icon: &'static str, hint: &str| Footer {
                submit,
                busy: false,
                side: None,
                locked: false,
                tone,
                icon,
                hint: hint.to_owned(),
            };
        let updating = "更新中 · 追平后自动重新检查；关闭这扇窗不会中断，进度也在任务队列里";
        let mut footer = match view {
            View::Empty => plain(
                Err("先添加提资元素".to_owned()),
                Status::Neutral,
                ph::INFO,
                "先在模型树或三维视图里选中元素，再点「添加」",
            ),
            View::Checking => plain(
                Err("正在检查，查完才知道能不能提交".to_owned()),
                Status::Info,
                ph::HOURGLASS,
                "正在检查提资元素所在的库 · 只读，不排任何任务",
            ),
            View::Unsupported(_) => plain(
                Ok(()),
                Status::Neutral,
                ph::INFO,
                "这台模型服务不支持提资前检查，提资照旧",
            ),
            View::Checked { .. } if self.run.is_some() => Footer {
                side: Some(Side::Updating),
                ..plain(
                    Err("正在更新，追平后才能提交".to_owned()),
                    Status::Info,
                    ph::ARROWS_CLOCKWISE,
                    updating,
                )
            },
            View::Checked { report, .. } => match report.verdict {
                Verdict::Ready => plain(
                    Ok(()),
                    Status::Success,
                    ph::CHECK_CIRCLE,
                    "可以提交 · 提交时会再检查一次",
                ),
                Verdict::NeedsUpdate { libs } => {
                    let (tone, icon, hint) = match &report.update_blocked {
                        Some(reason) => (Status::Warn, ph::PAUSE_CIRCLE, reason.as_str()),
                        None => (
                            Status::Neutral,
                            ph::INFO,
                            "提资元素所在的库追平后才能提交；提交时会再检查一次",
                        ),
                    };
                    Footer {
                        side: Some(Side::Update {
                            libs,
                            blocked: report.update_blocked.clone(),
                        }),
                        ..plain(
                            Err("要提资的数据不是最新：先点「更新到最新」".to_owned()),
                            tone,
                            icon,
                            hint,
                        )
                    }
                }
                Verdict::Updating => Footer {
                    side: Some(Side::Updating),
                    ..plain(
                        Err("正在更新，追平后才能提交".to_owned()),
                        Status::Info,
                        ph::ARROWS_CLOCKWISE,
                        updating,
                    )
                },
                Verdict::Stuck => {
                    let hint = "有库更新不了：先处理它，或把它的元素移出本次提资";
                    Footer {
                        side: Some(Side::RemoveStuck(report.stuck_elements())),
                        ..plain(
                            Err(hint.to_owned()),
                            Status::Error,
                            ph::WARNING_CIRCLE,
                            hint,
                        )
                    }
                }
                Verdict::Unknown => {
                    let hint = "没查成：重新检查之前不能提交";
                    Footer {
                        side: Some(Side::Recheck),
                        ..plain(Err(hint.to_owned()), Status::Warn, ph::QUESTION, hint)
                    }
                }
            },
        };
        // 表单这一侧的事压在检查之上：一次提交在途时别的都先不让点。
        let in_flight = if self.form.submitting {
            Some("正在提交，请稍候")
        } else if self.form.delete_confirmation_open {
            Some("请先处理「确认删除提资」窗口")
        } else if self.submit_pending {
            Some("提交前正在重新检查")
        } else {
            None
        };
        if let Some(reason) = in_flight {
            footer.submit = Err(reason.to_owned());
            footer.busy = self.form.submitting || self.submit_pending;
            footer.locked = true;
        }
        if let Some(notice) = &self.notice {
            footer.tone = notice.tone;
            footer.icon = match notice.tone {
                Status::Success => ph::CHECK_CIRCLE,
                Status::Error => ph::WARNING_CIRCLE,
                Status::Warn => ph::WARNING,
                Status::Info | Status::Neutral => ph::INFO,
            };
            footer.hint = notice.text.clone();
        }
        footer
    }

    /// 收成一条时副标题上的那一句。
    fn summary(&self, view: &View) -> String {
        let count = self.form.request.elements.len();
        let state = match view {
            View::Empty => {
                return "还没有提资元素 · 在模型树或三维视图里选中后点「添加」".to_owned();
            }
            View::Checking => "正在检查".to_owned(),
            View::Unsupported(_) => "不检查，提资照旧".to_owned(),
            View::Checked { .. } if self.run.is_some() => "正在更新到最新".to_owned(),
            View::Checked { report, .. } => report.banner().1,
        };
        format!("{count} 个提资元素 · {state}")
    }

    fn close(&mut self) {
        self.form.preview_open = false;
        self.form.delete_confirmation_open = false;
        self.form.pending_request = None;
        self.check.pending = None;
        self.check.outcome = None;
        self.check.checked.clear();
        self.submit_pending = false;
        self.run = None;
        self.notice = None;
    }
}

// ================================================================ 窗口

pub fn show(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    workbench: &WorkbenchVm,
    queue: &task_queue::Vm,
    state: &mut State,
) -> Vec<Cmd> {
    if !state.open {
        state.close();
        return Vec::new();
    }

    let mut cmds = Vec::new();
    let view = state.tick(queue, &mut cmds);
    if state.busy(&view) {
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    let mut keep_open = true;
    if state.collapsed {
        show_bar(ctx, t, d, workbench, &view, state, &mut keep_open);
    } else {
        show_window(
            ctx,
            t,
            d,
            workbench,
            queue,
            &view,
            state,
            &mut keep_open,
            &mut cmds,
        );
    }

    // 与旧入口同一条规矩：确认删除那一步的提交在途时，窗不跟着关。
    state.open = keep_open || (state.form.submitting && state.form.delete_confirmation_open);
    if !state.open {
        state.close();
        return cmds;
    }
    if state.form.preview_open
        && let Some(cmd) = data_publish::show_preview(ctx, t, d, &mut state.form)
    {
        cmds.push(own(cmd));
    }
    if state.form.delete_confirmation_open
        && let Some(cmd) = data_publish::show_delete_confirmation(ctx, t, d, &mut state.form)
    {
        cmds.push(own(cmd));
    }
    cmds
}

fn window_frame(t: &Tokens) -> egui::Frame {
    egui::Frame::new()
        .fill(t.bg_panel)
        .corner_radius(CornerRadius::same(radius::LG))
        .stroke(Stroke::new(1.0_f32, t.border_strong))
}

#[allow(clippy::too_many_arguments)]
fn show_window(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    workbench: &WorkbenchVm,
    queue: &task_queue::Vm,
    view: &View,
    state: &mut State,
    keep_open: &mut bool,
    cmds: &mut Vec<Cmd>,
) {
    // 不锚在正中：添加元素要去模型树或三维视图里选，这扇窗得挪得开（拖窗口头），也收得起来。
    let viewport = ctx.input(|i| i.viewport_rect());
    let margin = d.px(24.0);
    let max = vec2(
        (viewport.width() - margin).max(d.px(640.0)),
        (viewport.height() - margin).max(d.px(420.0)),
    );
    // 矮视口（1536×864 这一档的屏很常见）上默认高度收一点，上下各让出一条：首次打开不压住顶上的工具条和
    // 底下的状态条。右栏自己会滚，左栏的列表跟着短。
    let roomy_h = (viewport.height() - d.px(160.0)).max(d.px(520.0));
    let size = vec2(d.px(1040.0).min(max.x), d.px(720.0).min(roomy_h).min(max.y));
    let pos = pos2(
        viewport.left() + ((viewport.width() - size.x) * 0.5).max(0.0),
        viewport.top() + ((viewport.height() - size.y) * 0.5).max(0.0),
    );
    egui::Window::new("三维数据提资")
        .id(egui::Id::new("data-handover-window"))
        .title_bar(false)
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_pos(pos)
        .default_size(size)
        .min_size([d.px(880.0).min(max.x), d.px(520.0).min(max.y)])
        .max_size(max)
        .frame(window_frame(t))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            // 标签不接选字：窗口头上的字要让得出拖拽。
            ui.style_mut().interaction.selectable_labels = false;
            header(ui, t, d, queue, state, keep_open);

            let footer_h = d.px(56.0);
            let body_h = (ui.available_height() - footer_h).max(d.px(200.0));
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), body_h), Sense::hover());
            let left_w = d.px(400.0).min(rect.width() * 0.46);
            let left = Rect::from_min_max(rect.min, pos2(rect.left() + left_w, rect.bottom()));
            let right = Rect::from_min_max(pos2(left.right(), rect.top()), rect.max);
            ui.painter().rect_filled(
                Rect::from_min_size(
                    pos2(left.right() - 1.0, rect.top()),
                    vec2(1.0, rect.height()),
                ),
                CornerRadius::ZERO,
                t.border,
            );
            let mut left_ui = column(ui, left, d.px(16.0), "data-handover-left");
            left_column(&mut left_ui, t, d, workbench, view, state);
            let mut right_ui = column(ui, right, d.px(16.0), "data-handover-right");
            right_column(&mut right_ui, t, d, queue, view, state, cmds);

            footer(ui, t, d, footer_h, view, state, cmds);
        });
}

/// 收起来的那一条：只有窗口头，外加一枚「添加当前选中」——收起来就是为了去选元素。
fn show_bar(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    workbench: &WorkbenchVm,
    view: &View,
    state: &mut State,
    keep_open: &mut bool,
) {
    let viewport = ctx.input(|i| i.viewport_rect());
    let w = d
        .px(640.0)
        .min((viewport.width() - d.px(24.0)).max(d.px(320.0)));
    egui::Window::new("三维数据提资（收起）")
        .id(egui::Id::new("data-handover-bar"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .constrain(true)
        .default_pos(pos2(
            viewport.center().x - w / 2.0,
            viewport.top() + d.px(88.0),
        ))
        .fixed_size([w, d.px(52.0)])
        .frame(window_frame(t))
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.style_mut().interaction.selectable_labels = false;
            let rect = bar(ui, d.px(52.0), t.bg_chrome, Corner::All);
            let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
            ui.spacing_mut().item_spacing.x = d.px(12.0);
            title_block(&mut ui, t, d, &state.summary(view));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = d.px(4.0);
                if ui
                    .add(widgets::tool_btn(t, d, ph::X, false))
                    .on_hover_text("关闭")
                    .clicked()
                {
                    *keep_open = false;
                }
                if ui
                    .add(widgets::tool_btn(t, d, ph::CARET_DOWN, false))
                    .on_hover_text("展开")
                    .clicked()
                {
                    state.collapsed = false;
                }
                ui.add_space(d.px(8.0));
                add_button(ui, t, d, workbench, state, "添加当前选中");
            });
        });
}

fn header(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    queue: &task_queue::Vm,
    state: &mut State,
    keep_open: &mut bool,
) {
    let rect = bar(ui, d.px(52.0), t.bg_chrome, Corner::Top);
    hairline(ui.painter(), rect, t.border);
    let mut ui = child(ui, rect.shrink2(vec2(d.px(16.0), 0.0)), Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(12.0);
    title_block(&mut ui, t, d, &subtitle(queue));
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = d.px(4.0);
        if ui
            .add(widgets::tool_btn(t, d, ph::X, false))
            .on_hover_text("关闭")
            .clicked()
        {
            *keep_open = false;
        }
        if ui
            .add(widgets::tool_btn(t, d, ph::CARET_UP, false))
            .on_hover_text("收成一条，好去模型树或三维视图里选元素")
            .clicked()
        {
            state.collapsed = true;
        }
        ui.add_space(d.px(8.0));
        ui.label(
            RichText::new("关闭窗口不会中断已排入的任务")
                .font(Font::meta(d))
                .color(t.text_muted),
        );
    });
}

/// 窗口头左边：图标块 + 标题 + 副标题。
fn title_block(ui: &mut Ui, t: &Tokens, d: Density, sub: &str) {
    let (chip, _) = ui.allocate_exact_size(vec2(d.px(28.0), d.px(28.0)), Sense::hover());
    fill(ui, chip, radius::MD, t.accent_bg, None);
    glyph(ui, chip.center(), ph::SHARE_NETWORK, d.px(15.0), t.accent);
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.label(
            RichText::new("三维数据提资")
                .font(Font::title(d))
                .color(t.text_primary),
        );
        ui.label(RichText::new(sub).font(Font::meta(d)).color(t.text_muted));
    });
}

fn subtitle(queue: &task_queue::Vm) -> String {
    let mut parts: Vec<&str> = [queue.project.trim(), queue.mdb.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect();
    parts.push("经 rs-server 交数据中心");
    parts.join(" · ")
}

// ================================================================ 左栏：提资内容与提资元素

/// 提资专业在界面与日志里的说法（说「提资」，不说「数据发布」）。
pub fn category_label(category: PublishCategory) -> &'static str {
    match category {
        PublishCategory::Process => "工艺专业",
        PublishCategory::Electrical => "电气专业",
        PublishCategory::Instrumentation => "仪控专业",
        PublishCategory::Ventilation => "通风专业",
        PublishCategory::Room => "设备房间位置",
    }
}

/// 「添加」：把工作台此刻的主选中加进提资元素。
fn add_button(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    workbench: &WorkbenchVm,
    state: &mut State,
    label: &str,
) {
    let current = data_publish::selected_workbench_element(workbench);
    let listed = current.as_ref().is_some_and(|candidate| {
        state
            .form
            .request
            .elements
            .iter()
            .any(|element| element.refno == candidate.refno)
    });
    let hint = if current.is_none() {
        "先在模型树或三维视图里选中元素"
    } else {
        "当前选中的元素已经在列表里"
    };
    let response = ui
        .add_enabled(
            current.is_some() && !listed,
            widgets::button(t, d, label).icon(ph::PLUS),
        )
        .on_disabled_hover_text(hint);
    if response.clicked()
        && let Some(element) = current
    {
        state.add(element);
    }
}

fn left_column(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    workbench: &WorkbenchVm,
    view: &View,
    state: &mut State,
) {
    ui.spacing_mut().item_spacing = vec2(d.px(8.0), d.px(8.0));
    ui.label(
        RichText::new("提资内容")
            .font(Font::strong(d))
            .color(t.text_primary),
    );
    let field_w = (ui.available_width() - d.px(80.0)).max(d.px(120.0));
    egui::Grid::new("data-handover-form")
        .num_columns(2)
        .spacing([d.px(12.0), d.px(8.0)])
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = d.input_h();
            form_label(ui, t, d, "标题");
            widgets::text_input(
                ui,
                t,
                d,
                egui::Id::new("data-handover-title"),
                &mut state.form.request.title,
                "请输入提资标题",
                field_w,
            );
            ui.end_row();

            form_label(ui, t, d, "提资专业");
            ComboBox::from_id_salt("data-handover-category")
                .selected_text(category_label(state.form.request.category))
                .width(field_w)
                .show_ui(ui, |ui| {
                    for category in PublishCategory::ALL {
                        ui.selectable_value(
                            &mut state.form.request.category,
                            category,
                            category_label(category),
                        );
                    }
                });
            ui.end_row();

            form_label(ui, t, d, "设计阶段");
            widgets::segmented(
                ui,
                t,
                d,
                &mut state.form.request.design_phase,
                &[
                    (DesignPhase::Preliminary, "初设"),
                    (DesignPhase::Detailed, "施设"),
                ],
            );
            ui.end_row();
        });

    ui.add_space(d.px(4.0));
    let report = match view {
        View::Checked { report, .. } => Some(report),
        _ => None,
    };
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("提资元素")
                .font(Font::strong(d))
                .color(t.text_primary),
        );
        let count = state.form.request.elements.len();
        let libs = report.map_or(0, |report| report.rows.len());
        let tally = match (count, libs) {
            (0, _) => "尚未添加".to_owned(),
            (count, 0) => format!("{count} 个"),
            (count, libs) => format!("{count} 个 · 分属 {libs} 个库"),
        };
        ui.label(RichText::new(tally).font(Font::meta(d)).color(t.text_muted));
    });

    ui.horizontal(|ui| {
        add_button(ui, t, d, workbench, state, "添加");
        if ui
            .add_enabled(
                state.form.selected_refno.is_some(),
                widgets::button(t, d, "移除"),
            )
            .on_disabled_hover_text("先在下面的列表里点选一个元素")
            .clicked()
            && let Some(selected) = state.form.selected_refno
        {
            state.remove(&[selected]);
        }
        if ui
            .add_enabled(
                !state.form.request.elements.is_empty(),
                widgets::button(t, d, "清空"),
            )
            .clicked()
        {
            state.clear();
        }
    });

    // 当前选中：添加的是谁，先说出来。
    let current = data_publish::selected_workbench_element(workbench);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(18.0)), Sense::hover());
    let y = rect.center().y;
    glyph(
        ui,
        pos2(rect.left() + d.px(7.0), y),
        ph::CURSOR,
        d.px(12.0),
        t.text_muted,
    );
    let (line, note) = match &current {
        Some(element) => {
            let listed = state
                .form
                .request
                .elements
                .iter()
                .any(|existing| existing.refno == element.refno);
            (
                format!("当前选中 {}", display_name(element)),
                if listed {
                    "已在列表中"
                } else {
                    "未在列表中"
                },
            )
        }
        None => ("当前没有选中元素".to_owned(), ""),
    };
    let note_w = text_right(
        ui,
        rect.right(),
        y,
        d.px(96.0),
        note,
        Font::meta(d),
        t.text_muted,
    );
    let x = rect.left() + d.px(20.0);
    text_left(
        ui,
        x,
        y,
        (rect.right() - note_w - d.px(8.0) - x).max(0.0),
        &line,
        Font::meta(d),
        t.text_secondary,
    );

    let legend_h = d.px(34.0);
    let inner_h = (ui.available_height() - legend_h - d.px(8.0) - d.px(10.0)).max(d.px(96.0));
    let mut clicked = None;
    egui::Frame::new()
        .fill(t.bg_input)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(d.px(4.0) as i8))
        .show(ui, |ui| {
            ui.set_min_size(vec2(ui.available_width(), inner_h));
            ScrollArea::vertical()
                .id_salt("data-handover-elements")
                .auto_shrink([false, false])
                .max_height(inner_h)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if state.form.request.elements.is_empty() {
                        ui.add_space(d.px(28.0));
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("尚未添加提资元素")
                                    .font(Font::meta(d))
                                    .color(t.text_muted),
                            );
                        });
                        return;
                    }
                    for element in &state.form.request.elements {
                        let selected = state.form.selected_refno == Some(element.refno);
                        let tone = report.and_then(|report| report.element_tone(element.refno));
                        let tag = report
                            .and_then(|report| report.row_of(element.refno))
                            .map(|row| format!("db{}", row.dbnum));
                        let note = report.and_then(|report| report.element_note(element.refno));
                        if element_row(ui, t, d, element, selected, tone, tag.as_deref(), note) {
                            clicked = Some(element.refno);
                        }
                    }
                });
        });
    if let Some(refno) = clicked {
        state.form.selected_refno = Some(refno);
    }

    ui.label(
        RichText::new(
            "圆点是该元素所在库的检查结论：绿 不挡提交 · 黄 需要更新 · 蓝 正在更新 · 红 更新不了。",
        )
        .font(Font::micro(d))
        .color(t.text_muted),
    );
}

fn form_label(ui: &mut Ui, t: &Tokens, d: Density, text: &str) {
    ui.allocate_ui_with_layout(
        vec2(d.px(68.0), d.input_h()),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.label(
                RichText::new(text)
                    .font(Font::label(d))
                    .color(t.text_secondary),
            );
        },
    );
}

/// 元素的显示名。没有名字的元素，表单那边拿下划线写法的记录号垫着；这扇窗里记录号一律按界面上的写法
/// （`24384/68343`）出现，名字那一格也跟着，不让同一个号在一行里写成两个样。
fn display_name(element: &PublishElement) -> String {
    if element.name.trim().is_empty() || element.name == element.refno.to_string() {
        element.refno.to_pdms_str()
    } else {
        element.name.clone()
    }
}

/// 提资元素列表的一行：圆点 + 名字 + 记录号 + 所在库。返回「被点了」。
#[allow(clippy::too_many_arguments)]
fn element_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    element: &PublishElement,
    selected: bool,
    tone: Option<Status>,
    tag: Option<&str>,
    note: Option<String>,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(28.0)), Sense::click());
    if ui.is_rect_visible(rect) {
        if selected {
            fill(ui, rect, radius::SM, t.accent_bg, None);
        } else if response.hovered() {
            fill(ui, rect, radius::SM, t.bg_hover, None);
        }
        let y = rect.center().y;
        let dot = match tone {
            Some(Status::Neutral) | None => t.border_strong,
            Some(tone) => t.status(tone).0,
        };
        ui.painter()
            .circle_filled(pos2(rect.left() + d.px(12.0), y), d.px(3.5), dot);
        glyph(
            ui,
            pos2(rect.left() + d.px(30.0), y),
            ph::CUBE,
            d.px(13.0),
            t.text_muted,
        );
        let mut right = rect.right() - d.px(8.0);
        if let Some(tag) = tag {
            right -= pill(ui, d, right, y, true, tag, t.text_secondary, t.bg_hover) + d.px(8.0);
        }
        let refno = element.refno.to_pdms_str();
        right -= text_right(
            ui,
            right,
            y,
            d.px(120.0),
            &refno,
            Font::mono_meta(d),
            t.text_muted,
        ) + d.px(10.0);
        let x = rect.left() + d.px(44.0);
        text_left(
            ui,
            x,
            y,
            (right - x).max(0.0),
            &display_name(element),
            Font::body(d),
            t.text_primary,
        );
    }
    let clicked = response.clicked();
    if let Some(note) = note {
        response.on_hover_text(note);
    }
    clicked
}

// ================================================================ 右栏：提资前检查

fn right_column(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    queue: &task_queue::Vm,
    view: &View,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    ui.spacing_mut().item_spacing = vec2(d.px(8.0), d.px(10.0));
    check_header(ui, t, d, view, state, cmds);
    match view {
        View::Empty => center_note(
            ui,
            t,
            d,
            Some(ph::LIST_CHECKS),
            "还没有提资元素",
            "添加之后，这里会检查它们所在的库是不是最新",
        ),
        View::Checking => center_note(
            ui,
            t,
            d,
            None,
            "正在检查…",
            "只查提资元素所在的库；检查只读，不排任何任务",
        ),
        View::Unsupported(reason) => note_card(
            ui,
            t,
            d,
            Status::Neutral,
            ph::PLUGS,
            "这台模型服务不支持提资前检查",
            &format!("{reason}。这里查不了数据是不是最新，提资照旧、不拦。"),
        ),
        View::Checked { report, .. } => {
            ScrollArea::vertical()
                .id_salt("data-handover-check")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    checked_body(ui, t, d, queue, report, state);
                });
        }
    }
}

fn check_header(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    view: &View,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(30.0)), Sense::hover());
    let mut ui = child(ui, rect, Align::Center);
    ui.spacing_mut().item_spacing.x = d.px(8.0);
    let (mark, _) = ui.allocate_exact_size(vec2(d.px(16.0), d.px(16.0)), Sense::hover());
    glyph(
        &ui,
        mark.center(),
        ph::SHIELD_CHECK,
        d.px(15.0),
        t.text_secondary,
    );
    ui.label(
        RichText::new("提资前检查")
            .font(Font::strong(d))
            .color(t.text_primary),
    );
    ui.label(
        RichText::new("只查提资元素所在的库")
            .font(Font::meta(d))
            .color(t.text_muted),
    );
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let checking = state.check.pending.is_some();
        if ui
            .add_enabled(
                !checking && !matches!(view, View::Empty),
                widgets::button(t, d, "重新检查").icon(ph::ARROWS_CLOCKWISE),
            )
            .clicked()
        {
            state.recheck(cmds);
        }
        if checking {
            ui.label(
                RichText::new("正在检查…")
                    .font(Font::meta(d))
                    .color(t.text_muted),
            );
            ui.add(egui::Spinner::new().size(d.px(13.0)).color(t.accent));
        } else if let View::Checked { at, .. } = view {
            ui.label(
                RichText::new(format!("检查于 {at}"))
                    .font(Font::mono_meta(d))
                    .color(t.text_muted),
            );
        }
    });
}

fn verdict_icon(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Ready => ph::CHECK_CIRCLE,
        Verdict::NeedsUpdate { .. } => ph::WARNING,
        Verdict::Updating => ph::ARROWS_CLOCKWISE,
        Verdict::Stuck => ph::WARNING_CIRCLE,
        Verdict::Unknown => ph::QUESTION,
    }
}

fn checked_body(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    queue: &task_queue::Vm,
    report: &Report,
    state: &State,
) {
    ui.spacing_mut().item_spacing.y = d.px(10.0);
    let (tone, title, detail) = report.banner();
    note_card(
        ui,
        t,
        d,
        tone,
        verdict_icon(report.verdict),
        &title,
        &detail,
    );
    if !report.rows.is_empty() {
        lib_table(ui, t, d, report);
    }
    if !report.gone.is_empty() {
        strip(
            ui,
            t,
            d,
            Status::Error,
            ph::WARNING_CIRCLE,
            &format!(
                "{} 个元素在文件最新保存里已经没有了（{}）：把它们移出本次提资。",
                report.gone.len(),
                names(state, &report.gone)
            ),
        );
    }
    if let Some((_, reason)) = report.failed.first() {
        let refnos: Vec<RefU64> = report.failed.iter().map(|(refno, _)| *refno).collect();
        strip(
            ui,
            t,
            d,
            Status::Warn,
            ph::QUESTION,
            &format!(
                "{} 个元素没查成（{}）：{reason}",
                refnos.len(),
                names(state, &refnos)
            ),
        );
    }
    plan_card(ui, t, d, queue, report, state);

    let scope = if report.others_behind > 0 {
        format!(
            "只碰提资元素所在的库；另有 {} 个落后或需初始化的库不在这次更新里，整体追新用「模型更新」。",
            report.others_behind
        )
    } else {
        "只碰提资元素所在的库；整体追新用「模型更新」。".to_owned()
    };
    foot(ui, t, d, ph::INFO, t.text_muted, &scope);
    if let Some(pending) = report.rooms_pending {
        foot(
            ui,
            t,
            d,
            ph::WARNING,
            t.warn,
            &format!(
                "房间归属还有 {pending} 项待重算：房间号可能还不是最终的。这一项只提醒，不拦提交。"
            ),
        );
    }
}

/// 几个元素的名字，多了只列前三个。
fn names(state: &State, refnos: &[RefU64]) -> String {
    let mut listed: Vec<String> = refnos
        .iter()
        .take(3)
        .map(|refno| state.name_of(*refno))
        .collect();
    if refnos.len() > 3 {
        listed.push("…".to_owned());
    }
    listed.join("、")
}

/// 几个库号连成一句，多了只列前六个。
fn dbs(dbnums: &[u32]) -> String {
    let mut listed: Vec<String> = dbnums
        .iter()
        .take(6)
        .map(|dbnum| format!("db{dbnum}"))
        .collect();
    if dbnums.len() > 6 {
        listed.push(format!("… 共 {} 个库", dbnums.len()));
    }
    listed.join("、")
}

// ---------------------------------------------------------------- 库行表

/// 库行表四列的起点（画板 S15：设计库 | 文件最新保存 | 数据水位 | 状态）。
struct Cols {
    name: f32,
    file: f32,
    mark: f32,
    state: f32,
    right: f32,
}

fn cols(rect: Rect, d: Density) -> Cols {
    let w = rect.width();
    Cols {
        name: rect.left() + d.px(30.0),
        file: rect.left() + w * 0.31,
        mark: rect.left() + w * 0.51,
        state: rect.left() + w * 0.70,
        right: rect.right() - d.px(14.0),
    }
}

fn lib_table(ui: &mut Ui, t: &Tokens, d: Density, report: &Report) {
    egui::Frame::new()
        .fill(t.bg_elevated)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), d.px(30.0)), Sense::hover());
            let c = cols(rect, d);
            for (x, label) in [
                (c.name, "设计库"),
                (c.file, "文件最新保存"),
                (c.mark, "数据水位"),
                (c.state, "状态"),
            ] {
                text_left(
                    ui,
                    x,
                    rect.center().y,
                    f32::INFINITY,
                    label,
                    Font::meta(d),
                    t.text_muted,
                );
            }
            for row in &report.rows {
                lib_row(ui, t, d, row);
            }
        });
}

/// 库行底下的一条进度。`fraction` 缺席 = 分母还没有，只报数、不画条。
struct Meter {
    label: &'static str,
    fraction: Option<f32>,
    count: String,
    note: &'static str,
}

fn meters(form: &LibForm) -> Vec<Meter> {
    match form {
        LibForm::Running { units, data, .. } => {
            let mut out = Vec::new();
            if let Some(data) = data {
                out.push(match data.total.filter(|total| *total > 0) {
                    Some(total) => Meter {
                        label: "解析保存",
                        fraction: Some(data.done as f32 / total as f32),
                        count: format!(
                            "{} / {} 元素",
                            group_number(data.done as i64),
                            group_number(total as i64)
                        ),
                        note: "",
                    },
                    None => Meter {
                        label: "解析保存",
                        fraction: None,
                        count: format!("已存 {} 元素", group_number(data.done as i64)),
                        note: "总数还没定下来",
                    },
                });
            }
            if let Some((done, total)) = units
                && *total > 0
            {
                out.push(Meter {
                    label: "交付单元",
                    fraction: Some(*done as f32 / *total as f32),
                    count: format!("{done} / {total} 个"),
                    note: "",
                });
            }
            out
        }
        LibForm::ScopeCaughtUp {
            rest: Some((done, total)),
        } if *total > 0 => vec![Meter {
            label: "整批单元",
            fraction: Some(*done as f32 / *total as f32),
            count: format!("{done} / {total} 个"),
            note: "后台继续，不挡提交",
        }],
        _ => Vec::new(),
    }
}

fn lib_row(ui: &mut Ui, t: &Tokens, d: Density, row: &LibRow) {
    let meters = meters(&row.form);
    let meters_h = if meters.is_empty() {
        0.0
    } else {
        meters.len() as f32 * d.px(20.0) + d.px(6.0)
    };
    let (rect, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), d.px(56.0) + meters_h),
        Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.left_top(), vec2(rect.width(), 1.0)),
            CornerRadius::ZERO,
            t.border,
        );
        let c = cols(rect, d);
        let tone = row.form.tone();
        let tone_fg = if tone == Status::Neutral {
            t.text_muted
        } else {
            t.status(tone).0
        };

        // 第一行：圆点 + 库号 + 类型 | 文件最新保存 | 数据水位 | 状态。
        let y = rect.top() + d.px(19.0);
        ui.painter()
            .circle_filled(pos2(rect.left() + d.px(16.0), y), d.px(4.0), tone_fg);
        let name_w = text_left(
            ui,
            c.name,
            y,
            c.file - c.name,
            &format!("db{}", row.dbnum),
            Font::mono(d),
            if tone == Status::Neutral {
                t.text_muted
            } else {
                t.text_primary
            },
        );
        if let Some(db_type) = &row.db_type {
            pill(
                ui,
                d,
                c.name + name_w + d.px(8.0),
                y,
                false,
                db_type,
                t.text_muted,
                t.bg_input,
            );
        }
        text_left(
            ui,
            c.file,
            y,
            c.mark - c.file - d.px(8.0),
            &row.file_cell(),
            Font::mono_meta(d),
            if matches!(row.form, LibForm::Blocked { .. }) {
                t.danger
            } else {
                t.text_secondary
            },
        );
        text_left(
            ui,
            c.mark,
            y,
            c.state - c.mark - d.px(8.0),
            &row.mark_cell(),
            Font::mono_meta(d),
            if row.mark_behind() {
                t.warn
            } else {
                t.text_secondary
            },
        );
        text_left(
            ui,
            c.state,
            y,
            c.right - c.state,
            &row.form.state_text(),
            Font::strong(d),
            tone_fg,
        );

        // 第二行：左边是怎么回事，右边是接下来该做什么。
        let y = rect.top() + d.px(41.0);
        let way_w = text_right(
            ui,
            c.right,
            y,
            rect.width() * 0.42,
            &row.form.way_out(),
            Font::meta(d),
            t.text_secondary,
        );
        text_left(
            ui,
            c.name,
            y,
            (c.right - way_w - d.px(16.0) - c.name).max(0.0),
            &row.detail_line(),
            Font::meta(d),
            t.text_muted,
        );

        let mut y = rect.top() + d.px(56.0) + d.px(8.0);
        for meter in &meters {
            let label_w = text_left(
                ui,
                c.name,
                y,
                d.px(72.0),
                meter.label,
                Font::meta(d),
                t.text_muted,
            );
            let mut x = c.name + label_w.max(d.px(56.0)) + d.px(12.0);
            if let Some(fraction) = meter.fraction {
                let track = Rect::from_min_size(
                    pos2(x, y - d.px(3.0)),
                    vec2((c.state - x - d.px(12.0)).max(d.px(40.0)), d.px(6.0)),
                );
                let cr = CornerRadius::same(3);
                ui.painter().rect_filled(track, cr, t.bg_input);
                let done = Rect::from_min_size(
                    track.min,
                    vec2(track.width() * fraction.clamp(0.0, 1.0), track.height()),
                );
                ui.painter().rect_filled(done, cr, tone_fg);
                x = track.right() + d.px(12.0);
            }
            let note_w = text_right(
                ui,
                c.right,
                y,
                d.px(160.0),
                meter.note,
                Font::meta(d),
                t.text_muted,
            );
            text_left(
                ui,
                x,
                y,
                (c.right - note_w - d.px(12.0) - x).max(0.0),
                &meter.count,
                Font::mono_meta(d),
                t.text_secondary,
            );
            y += d.px(20.0);
        }
    }

    let mut hover = Vec::new();
    if let Some(file_name) = &row.file_name {
        hover.push(format!("库文件 {file_name}"));
    }
    if let Some(detail) = row.form.hover() {
        hover.push(detail.to_owned());
    }
    if !hover.is_empty() {
        response.on_hover_text(hover.join("\n"));
    }
}

// ---------------------------------------------------------------- 这次更新怎么排 / 排成了什么样

fn plan_card(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    queue: &task_queue::Vm,
    report: &Report,
    state: &State,
) {
    let split = |run: &Run| -> (Vec<u32>, Vec<u32>) {
        let pick = |first_import: bool| {
            run.libs
                .iter()
                .filter(|(_, lib)| lib.first_import == first_import)
                .map(|(dbnum, _)| *dbnum)
                .collect::<Vec<u32>>()
        };
        (pick(false), pick(true))
    };

    if let Some(run) = &state.run {
        let (lagging, needs_init) = split(run);
        card(
            ui,
            t,
            d,
            ph::LIST_CHECKS,
            "这次更新怎么排的",
            "先增量、后首次导入",
            |ui| {
                order_lines(ui, t, d, &lagging, &needs_init);
                strip(
                    ui,
                    t,
                    d,
                    Status::Info,
                    ph::INFO,
                    "已排进任务队列，开始后不能中途取消。这扇窗可以关，进度也在任务队列里。",
                );
            },
        );
        return;
    }
    if matches!(report.verdict, Verdict::NeedsUpdate { .. }) {
        let libs = report.lagging.len() + report.needs_init.len();
        card(
            ui,
            t,
            d,
            ph::PLAY,
            "点「更新到最新」之后",
            &format!("{libs} 个库"),
            |ui| {
                order_lines(ui, t, d, &report.lagging, &report.needs_init);
                strip(
                    ui,
                    t,
                    d,
                    Status::Info,
                    ph::INFO,
                    "排进任务队列，不打断正在跑的任务；开始后不能中途取消。这扇窗可以关，进度也在任务队列里。",
                );
            },
        );
        return;
    }
    if report.verdict == Verdict::Updating {
        strip(
            ui,
            t,
            d,
            Status::Info,
            ph::INFO,
            "这些库的任务不是从这扇窗发起的（自动同步或别处排的），跑完同样会自动重新检查。",
        );
        return;
    }
    let Some(run) = &state.last_run else {
        return;
    };
    let rows = task_queue::rows(queue);
    card(
        ui,
        t,
        d,
        ph::CHECK_CIRCLE,
        "本次更新结果",
        &format!("{} 个库", run.libs.len()),
        |ui| {
            for (dbnum, lib) in &run.libs {
                let finished = lib
                    .task_id
                    .as_deref()
                    .and_then(|task_id| rows.iter().find(|row| row.task_id == task_id));
                let (tone, result) = match finished.map(|row| row.phase) {
                    Some(Phase::Succeeded) => (t.success, "已完成"),
                    Some(Phase::Partial) => (t.warn, "部分完成"),
                    Some(Phase::Failed) => (t.danger, "失败"),
                    _ if !lib.named => (t.text_muted, "这次没有排它"),
                    _ => (t.text_muted, "已结束"),
                };
                let note = finished
                    .map(|row| row.note.as_str())
                    .filter(|note| !note.is_empty());
                let tail = match note {
                    Some(note) => format!("{result} · {note}"),
                    None => result.to_owned(),
                };
                order_line(
                    ui,
                    t,
                    d,
                    None,
                    tone,
                    &format!("db{dbnum}"),
                    if lib.first_import {
                        "首次导入"
                    } else {
                        "增量更新"
                    },
                    &tail,
                );
            }
        },
    );
}

/// 「先 → 接着 → 最后」三行。哪一类库没有，那一行就不画，序词顺延。
fn order_lines(ui: &mut Ui, t: &Tokens, d: Density, lagging: &[u32], needs_init: &[u32]) {
    let mut badges = ["先", "接着"].into_iter();
    if !lagging.is_empty() {
        order_line(
            ui,
            t,
            d,
            badges.next(),
            t.accent,
            &dbs(lagging),
            "增量更新",
            "应用落后的保存，重算受影响的模型与房间归属",
        );
    }
    if !needs_init.is_empty() {
        order_line(
            ui,
            t,
            d,
            badges.next(),
            t.accent,
            &dbs(needs_init),
            "首次导入",
            "整库解析保存并生成模型，做完才能提交",
        );
    }
    order_line(
        ui,
        t,
        d,
        Some("最后"),
        t.accent,
        "自动重新检查",
        "",
        "追平后「提交」亮起；取回工作照常自动做",
    );
}

/// 卡片里的一行：序词（或圆点）+ 等宽标识 + 次要说明 + 右对齐的说明。
#[allow(clippy::too_many_arguments)]
fn order_line(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    badge: Option<&str>,
    tone: Color32,
    head: &str,
    kind: &str,
    tail: &str,
) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(28.0)), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let y = rect.center().y;
    let mut x = rect.left();
    match badge {
        Some(badge) => {
            pill(ui, d, x, y, false, badge, t.accent, t.accent_bg);
            x += d.px(46.0);
        }
        None => {
            ui.painter()
                .circle_filled(pos2(x + d.px(6.0), y), d.px(4.0), tone);
            x += d.px(22.0);
        }
    }
    let tail_w = text_right(
        ui,
        rect.right(),
        y,
        rect.width() * 0.55,
        tail,
        Font::meta(d),
        t.text_secondary,
    );
    let room = (rect.right() - tail_w - d.px(16.0) - x).max(0.0);
    let head_w = text_left(ui, x, y, room, head, Font::mono(d), t.text_primary);
    if !kind.is_empty() {
        text_left(
            ui,
            x + head_w + d.px(10.0),
            y,
            (room - head_w - d.px(10.0)).max(0.0),
            kind,
            Font::meta(d),
            t.text_muted,
        );
    }
}

// ================================================================ 底部操作

fn footer(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    h: f32,
    view: &View,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    let model = state.footer(view);
    let rect = bar(ui, h, t.bg_chrome, Corner::Bottom);
    ui.painter().rect_filled(
        Rect::from_min_size(rect.left_top(), vec2(rect.width(), 1.0)),
        CornerRadius::ZERO,
        t.border,
    );
    let inner = rect.shrink2(vec2(d.px(16.0), 0.0));

    // 按钮先摆（靠右），提示句占剩下的宽度：理由有长有短，不许把按钮挤出去。
    let mut actions = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("data-handover-actions")
            .max_rect(inner)
            .layout(Layout::right_to_left(Align::Center)),
    );
    actions.spacing_mut().item_spacing.x = d.px(8.0);
    footer_actions(&mut actions, t, d, view, &model, state, cmds);
    let hint_right = actions.min_rect().left() - d.px(16.0);

    let color = if model.tone == Status::Neutral {
        t.text_muted
    } else {
        t.status(model.tone).0
    };
    let y = inner.center().y;
    glyph(
        ui,
        pos2(inner.left() + d.px(7.0), y),
        model.icon,
        d.px(13.0),
        color,
    );
    let x = inner.left() + d.px(22.0);
    let room = (hint_right - x).max(0.0);
    let hint_w = text_left(ui, x, y, room, &model.hint, Font::meta(d), color);
    // 截断过的提示，全文挂在悬停上。
    let hint_rect = Rect::from_min_max(pos2(x, inner.top()), pos2(x + hint_w, inner.bottom()));
    ui.interact(
        hint_rect,
        ui.id().with("data-handover-hint"),
        Sense::hover(),
    )
    .on_hover_text(model.hint.as_str());
}

fn footer_actions(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    view: &View,
    model: &Footer,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    // 从右往左摆：主操作、提交、预览。一屏只有一枚强调色按钮。
    match &model.side {
        Some(Side::Update { libs, blocked }) => {
            let label = format!("更新到最新 · {libs} 个库");
            let response = ui.add_enabled(
                !model.locked && blocked.is_none(),
                widgets::button(t, d, &label)
                    .icon(ph::ARROWS_CLOCKWISE)
                    .primary(),
            );
            let response = match blocked {
                Some(reason) => response.on_disabled_hover_text(reason.as_str()),
                None => response.on_hover_text(
                    "落后的库先增量更新，需初始化的库随后首次导入；排进任务队列，不打断正在跑的任务",
                ),
            };
            if response.clicked()
                && let View::Checked { report, .. } = view
            {
                cmds.push(state.begin_update(&report.lagging, &report.needs_init));
            }
        }
        Some(Side::Updating) => {
            ui.add_enabled(
                false,
                widgets::button(t, d, "更新中…")
                    .icon(ph::ARROWS_CLOCKWISE)
                    .primary(),
            );
        }
        Some(Side::RemoveStuck(elements)) => {
            let label = format!("移出这 {} 个元素", elements.len());
            if ui
                .add_enabled(
                    !model.locked,
                    widgets::button(t, d, &label).icon(ph::MINUS_CIRCLE),
                )
                .on_hover_text("把更新不了的库里的元素、以及文件里已经没有的元素移出本次提资")
                .clicked()
            {
                state.remove(elements);
            }
        }
        Some(Side::Recheck) => {
            let response = ui.add_enabled(
                !model.locked,
                widgets::button(t, d, "重新检查")
                    .icon(ph::ARROWS_CLOCKWISE)
                    .primary(),
            );
            if response.clicked() {
                state.recheck(cmds);
            }
        }
        None => {}
    }

    let mut submit = widgets::button(t, d, "提交")
        .icon(ph::PAPER_PLANE_TILT)
        .loading(model.busy);
    if model.side.is_none() {
        submit = submit.primary();
    }
    let response = ui.add_enabled(model.submit.is_ok(), submit);
    let response = match &model.submit {
        Err(reason) => response.on_disabled_hover_text(reason.as_str()),
        Ok(()) => response,
    };
    if response.clicked() {
        state.submit(view, cmds);
    }

    if ui
        .add(widgets::button(t, d, "预览"))
        .on_hover_text("只读：看一眼这次会交出去的数据，不提交")
        .clicked()
        && let Some(cmd) = state.form.start_preview()
    {
        cmds.push(own(cmd));
    }

    // 队列暂停或正在更新时，人要去的就是任务队列：给一枚直达，顺手把这扇窗收成一条、别挡着。
    let wants_queue = matches!(
        model.side,
        Some(
            Side::Update {
                blocked: Some(_),
                ..
            } | Side::Updating
        )
    );
    if wants_queue && ui.add(widgets::button(t, d, "打开任务队列")).clicked() {
        state.collapsed = true;
        cmds.push(Cmd::FocusPane(Pane::TaskQueue));
    }
}

// ================================================================ 零件

enum Corner {
    Top,
    Bottom,
    All,
}

/// 一条通栏横条。返回它的矩形，调用点在里面摆内容。
fn bar(ui: &mut Ui, h: f32, fill: Color32, corner: Corner) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let r = radius::LG;
    let (top, bottom) = match corner {
        Corner::Top => (r, 0),
        Corner::Bottom => (0, r),
        Corner::All => (r, r),
    };
    ui.painter().rect_filled(
        rect,
        CornerRadius {
            nw: top,
            ne: top,
            sw: bottom,
            se: bottom,
        },
        fill,
    );
    rect
}

fn hairline(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_filled(
        Rect::from_min_size(
            pos2(rect.left(), rect.bottom() - 1.0),
            vec2(rect.width(), 1.0),
        ),
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

/// 主体里的一栏：自上而下排，超出这一栏的内容裁掉。
fn column(ui: &mut Ui, rect: Rect, pad: f32, salt: &str) -> Ui {
    let mut column = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(salt)
            .max_rect(rect.shrink(pad))
            .layout(Layout::top_down(Align::Min)),
    );
    column.set_clip_rect(rect.intersect(ui.clip_rect()));
    column
}

fn center_note(ui: &mut Ui, t: &Tokens, d: Density, icon: Option<&str>, title: &str, detail: &str) {
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
        ui.label(
            RichText::new(detail)
                .font(Font::meta(d))
                .color(t.text_muted),
        );
    });
}

/// 整体判决的横幅：状态色底，图标 + 标题 + 一句说明。
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
    let fg = if tone == Status::Neutral {
        t.text_secondary
    } else {
        fg
    };
    egui::Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(d.px(12.0) as i8))
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

/// 一条浅底提示，比横幅轻。
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

/// 右栏底下的一句脚注。
fn foot(ui: &mut Ui, t: &Tokens, d: Density, icon: &str, color: Color32, text: &str) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = d.px(8.0);
        let (rect, _) = ui.allocate_exact_size(vec2(d.px(13.0), d.px(15.0)), Sense::hover());
        glyph(ui, rect.center(), icon, d.px(12.0), color);
        ui.label(RichText::new(text).font(Font::meta(d)).color(t.text_muted));
    });
}

/// 带标题行的卡片。
fn card(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    icon: &str,
    title: &str,
    note: &str,
    add: impl FnOnce(&mut Ui),
) {
    egui::Frame::new()
        .fill(t.bg_elevated)
        .stroke(Stroke::new(1.0_f32, t.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::same(d.px(14.0) as i8))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(8.0);
                let (rect, _) =
                    ui.allocate_exact_size(vec2(d.px(15.0), d.px(15.0)), Sense::hover());
                glyph(ui, rect.center(), icon, d.px(14.0), t.text_secondary);
                ui.label(
                    RichText::new(title)
                        .font(Font::strong(d))
                        .color(t.text_primary),
                );
                if !note.is_empty() {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(note)
                                .font(Font::meta(d))
                                .color(t.text_secondary),
                        );
                    });
                }
            });
            ui.add_space(d.px(6.0));
            ui.spacing_mut().item_spacing.y = d.px(2.0);
            add(ui);
        });
}

fn layout(ui: &Ui, text: &str, font: FontId) -> Arc<egui::Galley> {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
}

/// 量好的一段文字，宽度不超过 `max_w`；超了就按字符截断、省略号收尾。先按宽度比例猜一刀再逐字回退，
/// 免得从尾巴一个字一个字试。
fn fit(ui: &Ui, text: &str, font: FontId, max_w: f32) -> Arc<egui::Galley> {
    let full = layout(ui, text, font.clone());
    if full.size().x <= max_w {
        return full;
    }
    let total = text.chars().count();
    let mut keep = ((total as f32) * max_w / full.size().x.max(1.0)) as usize;
    keep = keep.min(total.saturating_sub(1));
    loop {
        let cut: String = text
            .chars()
            .take(keep)
            .chain(std::iter::once('…'))
            .collect();
        let galley = layout(ui, &cut, font.clone());
        if galley.size().x <= max_w || keep == 0 {
            return galley;
        }
        keep -= 1;
    }
}

/// 以 `x` 为左缘、`y` 为中线画一段文字，返回画出来的宽度。
fn text_left(ui: &Ui, x: f32, y: f32, max_w: f32, text: &str, font: FontId, color: Color32) -> f32 {
    if text.is_empty() || max_w <= 0.0 {
        return 0.0;
    }
    let galley = fit(ui, text, font, max_w);
    let size = galley.size();
    ui.painter()
        .galley(pos2(x, y - size.y / 2.0), galley, color);
    size.x
}

/// 以 `right` 为右缘、`y` 为中线画一段文字，返回画出来的宽度。
fn text_right(
    ui: &Ui,
    right: f32,
    y: f32,
    max_w: f32,
    text: &str,
    font: FontId,
    color: Color32,
) -> f32 {
    if text.is_empty() || max_w <= 0.0 {
        return 0.0;
    }
    let galley = fit(ui, text, font, max_w);
    let size = galley.size();
    ui.painter()
        .galley(pos2(right - size.x, y - size.y / 2.0), galley, color);
    size.x
}

/// 一枚小标签（库号、库类型、序词）。`from_right` 为真时 `x` 是它的右缘。返回它的宽度。
#[allow(clippy::too_many_arguments)]
fn pill(
    ui: &Ui,
    d: Density,
    x: f32,
    y: f32,
    from_right: bool,
    text: &str,
    fg: Color32,
    bg: Color32,
) -> f32 {
    let galley = layout(ui, text, Font::mono_micro(d));
    let pad = d.px(6.0);
    let size = vec2(galley.size().x + pad * 2.0, d.px(18.0));
    let left = if from_right { x - size.x } else { x };
    let rect = Rect::from_min_size(pos2(left, y - size.y / 2.0), size);
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::SM), bg);
    let text_y = y - galley.size().y / 2.0;
    ui.painter()
        .galley(pos2(rect.left() + pad, text_y), galley, fg);
    size.x
}

fn glyph(ui: &Ui, center: egui::Pos2, text: &str, size: f32, color: Color32) {
    let galley = layout(ui, text, FontId::new(size, FontFamily::Proportional));
    ui.painter()
        .galley(center - galley.size() / 2.0, galley, color);
}

fn fill(ui: &Ui, rect: Rect, r: u8, bg: Color32, stroke: Option<Color32>) {
    ui.painter().rect_filled(rect, CornerRadius::same(r), bg);
    if let Some(stroke) = stroke {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(r),
            Stroke::new(1.0_f32, stroke),
            StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data_publish_check::{ElementCheck, ElementFact, NodeLibrary, NodeModelStatus};
    use crate::model_update::EnqueuedBatch;
    use crate::task_queue::{DbnumStatus, QueueRow, TaskEntry};

    fn refno(n: u64) -> RefU64 {
        RefU64::from(n)
    }

    fn queue() -> task_queue::Vm {
        let mut queue = task_queue::Vm::new(Default::default());
        queue.project = "P".into();
        queue.mdb = "/ALL".into();
        queue.namespace = "ns".into();
        queue
    }

    /// 带着 `refnos` 这几个提资元素、已经打开的窗。
    fn opened(refnos: &[u64]) -> State {
        let mut state = State::default();
        state.request_open();
        for n in refnos {
            state.add(PublishElement {
                refno: refno(*n),
                name: format!("/E{n}"),
            });
        }
        state
    }

    fn fact(n: u64, dbnum: u32, verdict: &str, applied: i64, latest: i64) -> ElementCheck {
        ElementCheck::checked(
            refno(n),
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
            },
        )
    }

    fn checked(elements: Vec<ElementCheck>) -> Outcome {
        Outcome::Checked {
            elements,
            at: "12:31:07".into(),
        }
    }

    /// 这一帧发出的检查请求（纪元, 元素）。
    fn asked(cmds: &[Cmd]) -> Vec<(u64, Vec<RefU64>)> {
        cmds.iter()
            .filter_map(|cmd| match cmd {
                Cmd::DataHandover(Action::Check { epoch, refnos }) => {
                    Some((*epoch, refnos.clone()))
                }
                _ => None,
            })
            .collect()
    }

    fn tick(state: &mut State, queue: &task_queue::Vm) -> (View, Vec<Cmd>) {
        let mut cmds = Vec::new();
        let view = state.tick(queue, &mut cmds);
        (view, cmds)
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

    #[test]
    fn opening_with_elements_asks_once_and_keeps_submit_shut_until_the_reply() {
        let q = queue();
        let mut state = opened(&[1, 2]);
        let (view, cmds) = tick(&mut state, &q);
        assert_eq!(asked(&cmds), vec![(1, vec![refno(1), refno(2)])]);
        assert_eq!(view, View::Checking);
        assert!(state.footer(&view).submit.is_err());

        // 同一份清单不再问第二次。
        let (_, cmds) = tick(&mut state, &q);
        assert!(asked(&cmds).is_empty());

        state.adopt_check(
            1,
            checked(vec![
                fact(1, 8000, "latest", 92, 92),
                fact(2, 8000, "latest", 92, 92),
            ]),
        );
        let (view, cmds) = tick(&mut state, &q);
        assert!(asked(&cmds).is_empty(), "新一份事实落地不再为它问一次");
        let footer = state.footer(&view);
        assert_eq!(footer.submit, Ok(()));
        assert_eq!(footer.side, None, "主操作就是提交");
        assert!(matches!(
            &view,
            View::Checked { report, rechecking: false, .. } if report.allows_submit()
        ));
    }

    /// 开着窗、已经可以提交的时候文件又来了新保存：库登记表一说，这扇窗自己重新检查，不等人点「提交」才发现。
    #[test]
    fn a_new_save_seen_by_the_registry_asks_again_without_a_click() {
        let mut q = queue();
        q.dbnums = vec![DbnumStatus {
            dbnum: 8000,
            file_latest_sesno: 92,
            applied_sesno: 92,
            initialized: true,
            ..Default::default()
        }];
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.adopt_check(1, checked(vec![fact(1, 8000, "latest", 92, 92)]));
        let (view, cmds) = tick(&mut state, &q);
        assert!(asked(&cmds).is_empty());
        assert_eq!(state.footer(&view).submit, Ok(()));

        // 设计人员又存了一次：登记表先知道。旧事实照旧画着（没有任务落定，不必清空），同时去问。
        q.dbnums[0].file_latest_sesno = 93;
        let (view, cmds) = tick(&mut state, &q);
        assert_eq!(asked(&cmds), vec![(2, vec![refno(1)])]);
        assert!(matches!(
            &view,
            View::Checked {
                rechecking: true,
                ..
            }
        ));
        // 同一次变化不连着问。
        let (_, cmds) = tick(&mut state, &q);
        assert!(asked(&cmds).is_empty());

        state.adopt_check(2, checked(vec![fact(1, 8000, "lagging", 92, 93)]));
        let (view, cmds) = tick(&mut state, &q);
        assert!(asked(&cmds).is_empty(), "新一份事实落地，以此刻为基线");
        let footer = state.footer(&view);
        assert!(footer.submit.is_err());
        assert_eq!(
            footer.side,
            Some(Side::Update {
                libs: 1,
                blocked: None
            })
        );
    }

    #[test]
    fn changing_the_elements_asks_again_and_drops_the_stale_reply() {
        let q = queue();
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.add(PublishElement {
            refno: refno(2),
            name: "/E2".into(),
        });
        let (_, cmds) = tick(&mut state, &q);
        assert_eq!(asked(&cmds), vec![(2, vec![refno(1), refno(2)])]);

        // 第一次的回包晚到：丢掉，仍在等第二次。
        state.adopt_check(1, checked(vec![fact(1, 8000, "latest", 92, 92)]));
        let (view, _) = tick(&mut state, &q);
        assert_eq!(view, View::Checking);

        state.adopt_check(
            2,
            checked(vec![
                fact(1, 8000, "latest", 92, 92),
                fact(2, 8021, "not_generated", 0, 12),
            ]),
        );
        let (view, _) = tick(&mut state, &q);
        let footer = state.footer(&view);
        assert!(footer.submit.is_err());
        assert_eq!(
            footer.side,
            Some(Side::Update {
                libs: 1,
                blocked: None
            })
        );

        // 移出那个需初始化的元素：它的事实立刻不再算数，同时去问新的清单。
        state.remove(&[refno(2)]);
        let (view, cmds) = tick(&mut state, &q);
        assert_eq!(asked(&cmds), vec![(3, vec![refno(1)])]);
        assert!(matches!(
            &view,
            View::Checked { report, rechecking: true, .. } if report.rows.len() == 1
        ));
    }

    #[test]
    fn submit_checks_again_first_and_only_then_looks_up_deletions() {
        let q = queue();
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.adopt_check(1, checked(vec![fact(1, 8000, "latest", 92, 92)]));
        let (view, _) = tick(&mut state, &q);

        let mut cmds = Vec::new();
        state.submit(&view, &mut cmds);
        assert_eq!(asked(&cmds), vec![(2, vec![refno(1)])]);
        assert!(!state.form.submitting, "检查回来之前不碰提交接口");
        let (view, cmds) = tick(&mut state, &q);
        assert!(cmds.is_empty());
        let footer = state.footer(&view);
        assert!(footer.submit.is_err() && footer.busy && footer.locked);

        // 这期间文件又存了一次盘：不放行，说一声，不提交。
        state.adopt_check(2, checked(vec![fact(1, 8000, "latest", 92, 93)]));
        let (view, cmds) = tick(&mut state, &q);
        assert!(cmds.is_empty(), "{cmds:?}");
        assert!(!state.form.submitting);
        let footer = state.footer(&view);
        assert_eq!(footer.tone, Status::Warn);
        assert!(footer.hint.contains("没有提交"), "{}", footer.hint);

        // 追平之后再点：检查放行，接着查待删除。
        state.submit(&view, &mut Vec::new());
        state.adopt_check(3, checked(vec![fact(1, 8000, "latest", 93, 93)]));
        let (_, cmds) = tick(&mut state, &q);
        assert!(
            matches!(
                cmds.as_slice(),
                [Cmd::DataHandover(Action::LookupDeletions(request))] if request.elements.len() == 1
            ),
            "{cmds:?}"
        );
        assert!(state.form.submitting);

        // 查到待删除 → 确认删除子窗；提交回来 → 收尾并说结果。
        state.adopt_deletions(Ok(Vec::new()));
        assert!(state.form.delete_confirmation_open && !state.form.submitting);
        state.adopt_published(Ok("已受理".into()));
        assert!(!state.form.delete_confirmation_open && state.form.pending_request.is_none());
        let (view, _) = tick(&mut state, &q);
        let footer = state.footer(&view);
        assert_eq!(footer.tone, Status::Success);
        assert!(footer.hint.contains("已受理"));
    }

    #[test]
    fn an_update_is_followed_through_receipts_and_queue_until_it_settles() {
        let mut q = queue();
        let mut state = opened(&[1, 2]);
        tick(&mut state, &q);
        state.adopt_check(
            1,
            checked(vec![
                fact(1, 8000, "latest", 89, 92),
                fact(2, 8021, "not_generated", 0, 12),
            ]),
        );
        let (view, _) = tick(&mut state, &q);
        assert_eq!(
            state.footer(&view).side,
            Some(Side::Update {
                libs: 2,
                blocked: None
            })
        );

        // 按下：回执到齐之前按钮不再亮。
        state.update_started(&[8000], &[8021]);
        let (view, _) = tick(&mut state, &q);
        assert_eq!(state.footer(&view).side, Some(Side::Updating));

        let receipt = |task_id: &str, dbnum: u32| Enqueued {
            enqueued: vec![EnqueuedBatch {
                task_id: task_id.into(),
                dbnum,
                position: 1,
                ..Default::default()
            }],
            ..Default::default()
        };
        state.update_receipt(&receipt("t-inc", 8000));
        state.update_receipt(&receipt("t-init", 8021));
        state.update_queued();

        // 队列快照还没跟上：两个库按「已排上」说，不退回「落后」。
        let (view, _) = tick(&mut state, &q);
        let View::Checked { report, .. } = &view else {
            panic!("{view:?}");
        };
        assert!(
            report
                .rows
                .iter()
                .all(|row| matches!(row.form, LibForm::Queued { .. })),
            "{report:?}"
        );
        assert_eq!(state.footer(&view).side, Some(Side::Updating));

        // 快照跟上了：队列里这两个库的活变了，重新检查一次。
        q.queue.rows = vec![
            queue_row("t-inc", 8000, "running"),
            queue_row("t-init", 8021, "queued"),
        ];
        let (_, cmds) = tick(&mut state, &q);
        assert_eq!(asked(&cmds).len(), 1);
        state.adopt_check(
            2,
            checked(vec![
                fact(1, 8000, "updating", 89, 92),
                fact(2, 8021, "not_generated", 0, 12),
            ]),
        );
        let (view, _) = tick(&mut state, &q);
        assert_eq!(state.footer(&view).side, Some(Side::Updating));

        // 两条任务都跑完：落定，旧事实整份丢掉重问，不退回「落后」让按钮再亮一下。
        q.queue.rows.clear();
        q.tasks = vec![TaskEntry {
            task_id: "t-inc".into(),
            kind: task_queue::KIND_DATA_BATCH.into(),
            state: "succeeded".into(),
            dbnum: Some(8000),
            ..Default::default()
        }];
        let (view, cmds) = tick(&mut state, &q);
        assert_eq!(view, View::Checking);
        assert_eq!(asked(&cmds), vec![(3, vec![refno(1), refno(2)])]);
        assert!(state.run.is_none() && state.last_run.is_some());

        state.adopt_check(
            3,
            checked(vec![
                fact(1, 8000, "latest", 92, 92),
                fact(2, 8021, "latest", 12, 12),
            ]),
        );
        let (view, cmds) = tick(&mut state, &q);
        assert!(asked(&cmds).is_empty());
        let footer = state.footer(&view);
        assert_eq!((footer.submit, footer.side), (Ok(()), None));
    }

    /// 回执没点到的库（已被别的任务追平、或被阻断）这次没有活：不等它，直接落定重问。
    #[test]
    fn libraries_the_receipt_did_not_name_are_not_waited_for() {
        let q = queue();
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.adopt_check(1, checked(vec![fact(1, 8000, "latest", 89, 92)]));
        tick(&mut state, &q);
        state.update_started(&[8000], &[]);
        state.update_receipt(&Enqueued::default());
        state.update_queued();
        let (view, cmds) = tick(&mut state, &q);
        assert_eq!(view, View::Checking);
        assert_eq!(asked(&cmds).len(), 1);
        assert!(state.run.is_none());

        // 请求没发成也一样收尾，并把原因说出来。
        state.adopt_check(2, checked(vec![fact(1, 8000, "latest", 89, 92)]));
        tick(&mut state, &q);
        state.update_started(&[8000], &[]);
        state.update_failed("模型服务连不上");
        let (_, cmds) = tick(&mut state, &q);
        assert_eq!(asked(&cmds).len(), 1);
        state.adopt_check(3, checked(vec![fact(1, 8000, "latest", 89, 92)]));
        let (view, _) = tick(&mut state, &q);
        let footer = state.footer(&view);
        assert_eq!(footer.tone, Status::Error);
        assert!(footer.hint.contains("模型服务连不上"), "{}", footer.hint);
        assert!(
            matches!(footer.side, Some(Side::Update { .. })),
            "可以再点一次"
        );
    }

    /// 别处排的任务（自动同步）跑完了：队列里这个库的行没了，手上的事实旧了，整份重问。
    #[test]
    fn a_task_from_elsewhere_finishing_triggers_a_fresh_check() {
        let mut q = queue();
        q.queue.rows = vec![queue_row("t-auto", 8000, "running")];
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.adopt_check(1, checked(vec![fact(1, 8000, "updating", 89, 92)]));
        let (view, cmds) = tick(&mut state, &q);
        assert!(asked(&cmds).is_empty());
        let footer = state.footer(&view);
        assert_eq!(footer.side, Some(Side::Updating));
        assert!(state.busy(&view));

        q.queue.rows.clear();
        let (view, cmds) = tick(&mut state, &q);
        assert_eq!(asked(&cmds).len(), 1);
        assert_eq!(view, View::Checking);
    }

    #[test]
    fn a_service_that_cannot_judge_does_not_gate() {
        let mut q = queue();
        q.health = Some(task_queue::Health {
            data_face: "read-through".into(),
            ..Default::default()
        });
        let mut state = opened(&[1]);
        let (view, cmds) = tick(&mut state, &q);
        assert!(cmds.is_empty(), "读透形态不问：{cmds:?}");
        assert!(matches!(view, View::Unsupported(_)));
        assert_eq!(state.footer(&view).submit, Ok(()));
        let mut cmds = Vec::new();
        state.submit(&view, &mut cmds);
        assert!(
            matches!(
                cmds.as_slice(),
                [Cmd::DataHandover(Action::LookupDeletions(_))]
            ),
            "{cmds:?}"
        );

        // 老服务端没有检查接口：宿主回的是「不支持」，同样不拦、也不反复去问。
        let q = queue();
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.adopt_check(1, Outcome::Unsupported(check::UNSUPPORTED_NO_ROUTE.into()));
        let (view, cmds) = tick(&mut state, &q);
        assert!(cmds.is_empty());
        assert_eq!(state.footer(&view).submit, Ok(()));
    }

    #[test]
    fn stuck_elements_can_be_moved_out_in_one_go() {
        let q = queue();
        let mut state = opened(&[1, 2]);
        tick(&mut state, &q);
        state.adopt_check(
            1,
            checked(vec![
                fact(1, 8000, "latest", 92, 92),
                ElementCheck {
                    refno: refno(2),
                    fact: ElementFact::Gone,
                },
            ]),
        );
        let (view, _) = tick(&mut state, &q);
        let footer = state.footer(&view);
        assert!(footer.submit.is_err());
        assert_eq!(footer.side, Some(Side::RemoveStuck(vec![refno(2)])));
        state.remove(&[refno(2)]);
        assert_eq!(state.refnos(), vec![refno(1)]);

        // 没查成：主操作是重新检查，提交灰着。
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.adopt_check(
            1,
            checked(vec![ElementCheck {
                refno: refno(1),
                fact: ElementFact::Failed("timeout".into()),
            }]),
        );
        let (view, _) = tick(&mut state, &q);
        let footer = state.footer(&view);
        assert!(footer.submit.is_err());
        assert_eq!(footer.side, Some(Side::Recheck));
    }

    /// 队列暂停时不替人恢复：「更新到最新」灰着，理由在按钮与底部提示上都说。
    #[test]
    fn a_paused_queue_keeps_the_update_button_disabled() {
        let mut q = queue();
        q.queue.paused = true;
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.adopt_check(1, checked(vec![fact(1, 8000, "latest", 89, 92)]));
        let (view, _) = tick(&mut state, &q);
        let footer = state.footer(&view);
        let Some(Side::Update {
            blocked: Some(reason),
            ..
        }) = &footer.side
        else {
            panic!("{footer:?}");
        };
        assert!(reason.contains("暂停"));
        assert_eq!(footer.tone, Status::Warn);
        assert_eq!(&footer.hint, reason);
    }

    #[test]
    fn closing_forgets_the_check_and_emptying_the_list_stops_asking() {
        let q = queue();
        let mut state = opened(&[1]);
        tick(&mut state, &q);
        state.adopt_check(1, checked(vec![fact(1, 8000, "latest", 92, 92)]));
        state.close();
        // 再打开：事实可能已经旧了，重新问。
        let (view, cmds) = tick(&mut state, &q);
        assert_eq!(view, View::Checking);
        assert_eq!(asked(&cmds).len(), 1);
        // 关着的时候晚到的回包不收。
        state.close();
        state.adopt_check(2, checked(vec![fact(1, 8000, "latest", 92, 92)]));
        assert!(state.check.outcome.is_none());

        state.clear();
        let (view, cmds) = tick(&mut state, &q);
        assert_eq!(view, View::Empty);
        assert!(cmds.is_empty());
        assert!(state.footer(&view).submit.is_err());
    }

    #[test]
    fn borrowed_publish_commands_are_rerouted_to_this_window() {
        let request = PublishRequest::default();
        assert_eq!(
            own(Cmd::PreviewDataPublish(request.clone())),
            Cmd::DataHandover(Action::Preview(request.clone()))
        );
        assert_eq!(
            own(Cmd::LookupDataPublishDeletions(request.clone())),
            Cmd::DataHandover(Action::LookupDeletions(request.clone()))
        );
        assert_eq!(
            own(Cmd::SubmitDataPublish(request.clone())),
            Cmd::DataHandover(Action::Publish(request))
        );
        assert_eq!(own(Cmd::GetWork), Cmd::GetWork);
    }

    #[test]
    fn receipt_line_names_only_the_libraries_asked_for() {
        let receipt = Enqueued {
            enqueued: vec![EnqueuedBatch {
                task_id: "t1".into(),
                dbnum: 8000,
                position: 2,
                ..Default::default()
            }],
            already_covered: vec![8003],
            blocked: vec![
                crate::model_update::BlockedDbnum {
                    dbnum: 8004,
                    reason: "文件缺失".into(),
                },
                crate::model_update::BlockedDbnum {
                    dbnum: 9999,
                    reason: "与这次无关".into(),
                },
            ],
            unselected: vec![1, 2, 3],
            up_to_date: 40,
            ..Default::default()
        };
        let line = receipt_line("增量更新", &[8000, 8003, 8004], &receipt);
        assert_eq!(
            line,
            "提资前更新 · 增量更新：db8000 排第 2 位；db8003 已被排队中的批次覆盖；db8004 阻断未入队（文件缺失）"
        );
        assert!(receipt_line("首次导入", &[8021], &Enqueued::default()).contains("一行都没排上"));
    }

    #[test]
    fn subtitle_and_labels_use_the_vocabulary() {
        assert_eq!(subtitle(&queue()), "P · /ALL · 经 rs-server 交数据中心");
        assert_eq!(
            subtitle(&task_queue::Vm::default()),
            "经 rs-server 交数据中心"
        );
        for category in PublishCategory::ALL {
            let label = category_label(category);
            assert!(
                !label.contains("发布"),
                "界面上说提资，不说数据发布：{label}"
            );
        }
        assert_eq!(dbs(&[8000, 8021]), "db8000、db8021");
    }
}
