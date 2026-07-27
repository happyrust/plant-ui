//! 模型增量更新任务窗。ZONE 只做统计分桶，执行选择仍是 dbnum。

use std::collections::BTreeSet;

use egui::{RichText, ScrollArea};
use egui_phosphor::regular as ph;
use serde::Deserialize;

use crate::Cmd;
use crate::style::tokens::{Density, Tokens};
use crate::style::widgets;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Preview {
    pub project: String,
    pub dbnums: Vec<DbPreview>,
    pub warnings: Vec<String>,
    pub up_to_date: bool,
    pub execution_scope: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DbPreview {
    pub dbnum: u32,
    pub db_type: String,
    pub file_name: String,
    pub file_path: String,
    pub applied_sesno: u32,
    pub file_latest_sesno: u32,
    pub sessions: Vec<SessionPreview>,
    pub zones: Vec<ZonePreview>,
    pub blocked: bool,
    pub anomaly: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SessionPreview {
    pub sesno: u32,
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
    pub changed_count: usize,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ZonePreview {
    pub zone_refno: Option<String>,
    pub unit_count: usize,
    pub units: Vec<UnitPreview>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UnitPreview {
    pub root_refno: String,
    pub noun: String,
    pub model_category: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Accepted {
    pub run_ids: Vec<String>,
    pub dbnums: Vec<u32>,
    pub skipped: Vec<Skipped>,
    pub excluded: Vec<Skipped>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Skipped {
    pub dbnum: u32,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Run {
    pub run_id: String,
    pub dbnum: u32,
    pub state: String,
    pub from_sesno: u32,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error: Option<String>,
}

impl Run {
    pub fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed" | "contention")
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
        runs: Vec<Run>,
        warnings: Vec<String>,
        poll_error: Option<String>,
    },
    Failed(String),
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    selected: BTreeSet<u32>,
}

impl State {
    pub fn select_defaults(&mut self, preview: &Preview) {
        self.selected = preview
            .dbnums
            .iter()
            .filter(|db| !db.blocked && !db.sessions.is_empty())
            .map(|db| db.dbnum)
            .collect();
    }
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
                Vm::Ready(preview) => preview_ui(ui, t, d, preview, state, &mut cmds),
                Vm::Running {
                    runs,
                    warnings,
                    poll_error,
                } => running_ui(ui, t, d, runs, warnings, poll_error.as_deref(), &mut cmds),
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

fn preview_ui(
    ui: &mut egui::Ui,
    t: &Tokens,
    d: Density,
    preview: &Preview,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
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

    ui.separator();
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 48.0)
        .show(ui, |ui| {
            for db in &preview.dbnums {
                db_preview(ui, t, db, &mut state.selected);
                ui.add_space(6.0);
            }
        });
    ui.separator();
    ui.horizontal(|ui| {
        if ui.add(widgets::button(t, d, "刷新")).clicked() {
            cmds.push(Cmd::RefreshModelUpdate);
        }
        let selected: Vec<u32> = state.selected.iter().copied().collect();
        if ui
            .add_enabled(
                !selected.is_empty(),
                widgets::button(t, d, &format!("更新所选 {} 个库", selected.len())).primary(),
            )
            .clicked()
        {
            cmds.push(Cmd::ExecuteModelUpdate(selected));
        }
    });
}

fn db_preview(ui: &mut egui::Ui, t: &Tokens, db: &DbPreview, selected: &mut BTreeSet<u32>) {
    egui::Frame::group(ui.style())
        .fill(t.bg_elevated)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let selectable = !db.blocked && !db.sessions.is_empty();
            let mut checked = selected.contains(&db.dbnum);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(selectable, egui::Checkbox::new(&mut checked, ""))
                    .changed()
                {
                    if checked {
                        selected.insert(db.dbnum);
                    } else {
                        selected.remove(&db.dbnum);
                    }
                }
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
                ui.colored_label(t.warn, anomaly);
            }
            if !db.sessions.is_empty() {
                let sessions = db
                    .sessions
                    .iter()
                    .map(|s| {
                        format!(
                            "{}: +{} ~{} -{}（{}）",
                            s.sesno, s.added, s.modified, s.deleted, s.changed_count
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
                ui.label(RichText::new(format!("变化会话  {sessions}")).monospace());
            }
            for zone in &db.zones {
                let zone_name = zone.zone_refno.as_deref().unwrap_or("未归属 ZONE");
                ui.collapsing(
                    format!("ZONE {zone_name} · {} 个生成单元", zone.unit_count),
                    |ui| {
                        for unit in &zone.units {
                            ui.label(
                                RichText::new(format!(
                                    "{}  {}  {}",
                                    unit.noun, unit.root_refno, unit.model_category
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
    runs: &[Run],
    warnings: &[String],
    poll_error: Option<&str>,
    cmds: &mut Vec<Cmd>,
) {
    let complete = !runs.is_empty() && runs.iter().all(Run::terminal);
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
    for warning in warnings {
        ui.colored_label(t.warn, warning);
    }
    ui.add_space(8.0);
    for run in runs {
        let color = match run.state.as_str() {
            "succeeded" => t.success,
            "failed" | "contention" => t.danger,
            _ => t.accent,
        };
        ui.horizontal(|ui| {
            if !run.terminal() {
                ui.add(egui::Spinner::new().size(14.0).color(color));
            }
            ui.label(RichText::new(format!("DB {}", run.dbnum)).strong());
            ui.colored_label(color, &run.state);
            ui.label(
                RichText::new(format!("from SESNO {}", run.from_sesno))
                    .monospace()
                    .color(t.text_muted),
            );
        });
        if let Some(error) = &run.error {
            ui.colored_label(t.danger, error);
        }
    }
    if complete && ui.add(widgets::button(t, d, "重新预览")).clicked() {
        cmds.push(Cmd::RefreshModelUpdate);
    }
}
