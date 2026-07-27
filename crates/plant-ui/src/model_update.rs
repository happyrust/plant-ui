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

#[derive(Debug, Clone, Default)]
pub enum Vm {
    #[default]
    Idle,
    Loading,
    Ready(Preview),
    Starting,
    Running {
        run: Run,
        poll_error: Option<String>,
    },
    Failed(String),
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
                Vm::Running { run, poll_error } => {
                    running_ui(ui, t, d, run, poll_error.as_deref(), &mut cmds)
                }
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
            format!(
                "{} 个模型单元等待重试",
                preview.pending_model_retries.len()
            ),
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

fn running_ui(
    ui: &mut egui::Ui,
    t: &Tokens,
    d: Density,
    run: &Run,
    poll_error: Option<&str>,
    cmds: &mut Vec<Cmd>,
) {
    let complete = run.terminal();
    ui.label(
        RichText::new(if complete {
            "本次更新已结束"
        } else {
            "正在按 dbnum 串行执行增量更新"
        })
        .strong(),
    );
    if let Some(error) = poll_error {
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
    ui.label(
        RichText::new(format!("已接收 {} 条进度事件", run.events_seen))
            .monospace()
            .color(t.text_muted),
    );
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
