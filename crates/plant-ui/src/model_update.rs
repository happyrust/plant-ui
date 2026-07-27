//! 模型增量更新任务窗。ZONE 只做统计分桶，执行范围仍是项目内 dbnum + sesno。

use egui::{RichText, ScrollArea};
use egui_phosphor::regular as ph;
use serde::Deserialize;

use crate::Cmd;
use crate::style::tokens::{Density, Tokens};
use crate::style::widgets;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Preview {
    pub project: String,
    pub dbnums: Vec<DbPreview>,
    pub pending_model_retries: Vec<PendingModelUnit>,
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
    pub zones: Vec<ZonePreview>,
    pub no_generation: u32,
    pub blocked: bool,
    pub anomaly: Option<FileAnomaly>,
    pub initialization_required: bool,
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
    pub units: Vec<UnitPreview>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UnitPreview {
    pub root_refno: String,
    pub noun: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PendingModelUnit {
    pub dbnum: u32,
    pub root_refno: String,
    pub noun: String,
    pub source_end_sesno: i32,
    pub attempts: u32,
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
                "文件会话号回退：{file_latest_sesno} < 已应用 {applied_sesno}"
            ),
            Self::PathMigrated { old_path, new_path } => {
                write!(f, "文件已移动：{old_path} → {new_path}")
            }
            Self::TypeChanged {
                stored_db_type,
                observed_db_type,
            } => write!(f, "数据库类型变化：{stored_db_type} → {observed_db_type}"),
            Self::Duplicate { paths } => write!(f, "发现 {} 个同 DBNUM 文件", paths.len()),
            Self::Missing { path } => write!(f, "文件缺失：{path}"),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Accepted {
    pub task_id: String,
    pub kind: String,
    pub state: String,
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
    pub result: Option<serde_json::Value>,
}

impl Run {
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "partial" | "failed")
    }
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
}

#[derive(Debug, Clone)]
pub struct UnitRow {
    pub dbnum: u32,
    pub root_refno: String,
    pub noun: String,
    pub state: RowState,
    pub message: Option<String>,
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

/// 逐行明细。分子分母暂时还只有本端数得到的这一份——权威计数要等 gen-model
/// 把 `total_units` / `units_done` 放进任务详情（ADR-0007），到时候这里的
/// `batches` / `units` 长度只用来铺行，不再当分母。
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub feed: Feed,
    pub batches: Vec<BatchRow>,
    pub units: Vec<UnitRow>,
    /// 本端实际收到的事件条数。与任务详情的 `events_seen` 相减就是断线期间落后的条数。
    pub received: u64,
}

impl Progress {
    pub fn apply(&mut self, event: ProgressEvent) {
        self.received += 1;
        match event {
            ProgressEvent::DataBatchStarted {
                dbnum,
                start_sesno,
                end_sesno,
            } => self.batches.push(BatchRow {
                dbnum,
                start_sesno,
                end_sesno,
                state: RowState::Running,
                message: None,
            }),
            ProgressEvent::DataBatchFinished {
                dbnum,
                success,
                message,
            } => {
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
    pub fn behind(&self, events_seen: u64) -> u64 {
        events_seen.saturating_sub(self.received)
    }
}

#[derive(Debug, Clone, Default)]
pub enum Vm {
    #[default]
    Idle,
    Loading,
    Ready(Preview),
    Starting,
    /// 装箱是因为它比别的变体大出一截：`Run` 的六个串加上逐行明细，不装箱整个
    /// `Vm` 都得按它的尺寸走。
    Running(Box<RunVm>),
    Failed(String),
}

/// 一次运行实例在界面上的全部状态：终态靠轮询（`run` / `poll_error`），
/// 明细靠 WebSocket（`progress`），两条通道并存（ADR-0005）。
#[derive(Debug, Clone, Default)]
pub struct RunVm {
    pub run: Run,
    pub poll_error: Option<String>,
    pub progress: Progress,
}

#[derive(Default)]
pub struct State {
    pub open: bool,
}

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

    let mut cmds = Vec::new();
    let mut open = state.open;
    egui::Window::new("模型增量更新")
        .id(egui::Id::new("model-update-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([1040.0, 720.0])
        .min_size([760.0, 520.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(ph::ARROWS_CLOCKWISE).color(t.accent));
                ui.label(RichText::new("按 dbnum + sesno 执行").strong());
                ui.label(RichText::new("ZONE 仅用于汇总受影响的生成单元").color(t.text_muted));
            });
            ui.label(
                RichText::new(format!("服务：{api_url}"))
                    .monospace()
                    .small()
                    .color(t.text_muted),
            );
            ui.separator();

            match vm {
                Vm::Idle | Vm::Loading => loading(ui, t, "正在统计发生变化的 sesno 与 ZONE…"),
                Vm::Starting => loading(ui, t, "正在提交增量更新任务…"),
                Vm::Failed(error) => {
                    ui.colored_label(t.danger, error);
                    if ui.add(widgets::button(t, d, "重试预览")).clicked() {
                        cmds.push(Cmd::RefreshModelUpdate);
                    }
                }
                Vm::Ready(preview) => preview_ui(ui, t, d, preview, &mut cmds),
                Vm::Running(session) => running_ui(ui, t, d, session, &mut cmds),
            }
        });
    state.open = open;
    cmds
}

fn loading(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(80.0);
        ui.add(egui::Spinner::new().color(t.accent));
        ui.label(RichText::new(text).color(t.text_secondary));
    });
}

fn preview_ui(ui: &mut egui::Ui, t: &Tokens, d: Density, preview: &Preview, cmds: &mut Vec<Cmd>) {
    let changed = preview
        .dbnums
        .iter()
        .filter(|db| !db.sessions.is_empty())
        .count();
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("项目 {}", preview.project)).strong());
        ui.separator();
        ui.label(format!("{} 个设计库发生变化", changed));
        if preview.up_to_date {
            ui.colored_label(t.success, "已是最新");
        }
    });
    for warning in &preview.warnings {
        ui.colored_label(t.warn, warning);
    }
    if !preview.pending_model_retries.is_empty() {
        ui.colored_label(
            t.warn,
            format!("{} 个模型单元等待重试", preview.pending_model_retries.len()),
        );
        for unit in &preview.pending_model_retries {
            ui.label(
                RichText::new(format!(
                    "DB {} · {} {} · 已尝试 {} 次{}",
                    unit.dbnum,
                    unit.noun,
                    unit.root_refno,
                    unit.attempts,
                    unit.last_error
                        .as_deref()
                        .map(|error| format!(" · {error}"))
                        .unwrap_or_default()
                ))
                .monospace()
                .color(t.text_secondary),
            );
        }
    }

    ui.separator();
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 48.0)
        .show(ui, |ui| {
            for db in &preview.dbnums {
                db_preview(ui, t, db);
                ui.add_space(6.0);
            }
        });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.add(widgets::button(t, d, "刷新")).clicked() {
            cmds.push(Cmd::RefreshModelUpdate);
        }
        let executable = preview
            .dbnums
            .iter()
            .any(|db| !db.blocked && (!db.sessions.is_empty() || db.initialization_required))
            || !preview.pending_model_retries.is_empty();
        if ui
            .add_enabled(
                executable,
                widgets::button(t, d, &format!("执行本次更新（{changed} 个库）")).primary(),
            )
            .clicked()
        {
            cmds.push(Cmd::ExecuteModelUpdate);
        }
    });
}

fn db_preview(ui: &mut egui::Ui, t: &Tokens, db: &DbPreview) {
    egui::Frame::group(ui.style())
        .fill(t.bg_elevated)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("DB {}", db.dbnum)).strong());
                ui.label(RichText::new(&db.file_name).monospace());
                ui.label(
                    RichText::new(format!(
                        "SESNO {} → {}",
                        db.applied_sesno, db.file_latest_sesno
                    ))
                    .monospace()
                    .color(t.text_secondary),
                );
                if db.blocked {
                    ui.colored_label(t.danger, "不可更新");
                } else if db.sessions.is_empty() {
                    ui.colored_label(t.text_muted, "无变化");
                }
            });
            if let Some(anomaly) = &db.anomaly {
                ui.colored_label(t.warn, anomaly.to_string());
            }
            if !db.sessions.is_empty() {
                let sessions = db
                    .sessions
                    .iter()
                    .map(|s| {
                        format!(
                            "{}: +{} ~{} -{}（{}）",
                            s.sesno,
                            s.added,
                            s.modified,
                            s.deleted,
                            s.added + s.modified + s.deleted
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
                ui.label(RichText::new(format!("变化会话  {sessions}")).monospace());
            }
            for zone in &db.zones {
                let zone_name = if zone.zone_refno.is_empty() {
                    "未归属 ZONE"
                } else {
                    &zone.zone_refno
                };
                ui.collapsing(
                    format!("ZONE {zone_name} · {} 个生成单元", zone.units.len()),
                    |ui| {
                        for unit in &zone.units {
                            ui.label(
                                RichText::new(format!(
                                    "{}  {}  {}",
                                    unit.noun, unit.root_refno, unit.name
                                ))
                                .monospace()
                                .color(t.text_secondary),
                            );
                        }
                    },
                );
            }
        });
}

fn running_ui(ui: &mut egui::Ui, t: &Tokens, d: Density, session: &RunVm, cmds: &mut Vec<Cmd>) {
    let RunVm {
        run,
        poll_error,
        progress,
    } = session;
    let complete = run.terminal();
    ui.label(
        RichText::new(if complete {
            "本次更新已结束"
        } else {
            "正在按 dbnum 串行执行增量更新"
        })
        .strong(),
    );
    if let Some(error) = poll_error.as_deref() {
        ui.colored_label(t.warn, format!("状态查询暂时失败：{error}"));
    }
    ui.add_space(8.0);
    let color = match run.state.as_str() {
        "succeeded" => t.success,
        "partial" | "failed" => t.danger,
        _ => t.accent,
    };
    ui.horizontal(|ui| {
        if !run.terminal() {
            ui.add(egui::Spinner::new().size(14.0).color(color));
        }
        ui.label(RichText::new(&run.project).strong());
        ui.colored_label(color, &run.state);
        ui.label(RichText::new(&run.task_id).monospace().color(t.text_muted));
    });
    feed_line(ui, t, run, progress);
    phase_rows(ui, t, progress);
    if let Some(result) = &run.result {
        let batches = result["batches"].as_array().map_or(0, Vec::len);
        let units = result["units"].as_array().map_or(0, Vec::len);
        ui.label(format!("数据批次 {batches} · 模型单元 {units}"));
        if let Some(warnings) = result["warnings"].as_array() {
            for warning in warnings.iter().filter_map(|value| value.as_str()) {
                ui.colored_label(t.warn, warning);
            }
        }
    }
    if complete && ui.add(widgets::button(t, d, "重新预览")).clicked() {
        cmds.push(Cmd::RefreshModelUpdate);
    }
}

/// 明细通道自己那一行。连接状态与运行状态分开说：连接断了运行照跑，
/// 把两者混成一句会让人以为任务出了问题。
fn feed_line(ui: &mut egui::Ui, t: &Tokens, run: &Run, progress: &Progress) {
    ui.horizontal(|ui| match &progress.feed {
        Feed::Connecting => {
            ui.colored_label(t.text_muted, "明细通道连接中…");
        }
        Feed::Live => {
            ui.colored_label(t.text_muted, format!("已接收 {} 条进度事件", progress.received));
        }
        Feed::Down(reason) => {
            let behind = progress.behind(run.events_seen);
            ui.colored_label(
                t.warn,
                format!(
                    "明细已断连：服务端已发 {} 条，本端落后 {behind} 条（{reason}）",
                    run.events_seen
                ),
            );
        }
    });
    // 断线期间事件不补发，下面的行只是本端见过的那些，说清楚免得当成全量。
    if matches!(progress.feed, Feed::Down(_)) {
        ui.label(
            RichText::new("断线期间的事件不会补发，下面只列出本端收到过的行。")
                .small()
                .color(t.text_muted),
        );
    }
}

fn phase_rows(ui: &mut egui::Ui, t: &Tokens, progress: &Progress) {
    if progress.batches.is_empty() && progress.units.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ScrollArea::vertical()
        .id_salt("model-update-progress-rows")
        .auto_shrink([false, false])
        .max_height((ui.available_height() - 96.0).max(120.0))
        .show(ui, |ui| {
            if !progress.batches.is_empty() {
                ui.label(RichText::new("阶段一 · 数据批次").strong());
                for row in &progress.batches {
                    progress_row(
                        ui,
                        t,
                        row.state,
                        &format!("DB {}", row.dbnum),
                        &format!("sesno {} → {}", row.start_sesno, row.end_sesno),
                        row.message.as_deref(),
                    );
                }
                ui.add_space(6.0);
            }
            if !progress.units.is_empty() {
                ui.label(RichText::new("阶段二 · 交付单元").strong());
                for row in &progress.units {
                    progress_row(
                        ui,
                        t,
                        row.state,
                        &format!("{} {}", row.noun, row.root_refno),
                        &format!("DB {}", row.dbnum),
                        row.message.as_deref(),
                    );
                }
            }
        });
}

fn progress_row(
    ui: &mut egui::Ui,
    t: &Tokens,
    state: RowState,
    title: &str,
    detail: &str,
    message: Option<&str>,
) {
    ui.horizontal(|ui| {
        match state {
            RowState::Running => {
                ui.add(egui::Spinner::new().size(12.0).color(t.accent));
            }
            RowState::Done => {
                ui.colored_label(t.success, ph::CHECK);
            }
            RowState::Failed => {
                ui.colored_label(t.danger, ph::X);
            }
            RowState::Unknown => {
                ui.colored_label(t.text_muted, ph::QUESTION);
            }
        }
        ui.label(RichText::new(title).monospace());
        ui.label(RichText::new(detail).monospace().color(t.text_secondary));
        if state == RowState::Unknown {
            ui.colored_label(t.text_muted, "状态未知");
        }
    });
    if let Some(message) = message {
        ui.label(
            RichText::new(message)
                .small()
                .color(if state == RowState::Failed {
                    t.danger
                } else {
                    t.text_muted
                }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(json: &str) -> ProgressEvent {
        serde_json::from_str(json).expect("契约 payload 应当能解出来")
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
}
