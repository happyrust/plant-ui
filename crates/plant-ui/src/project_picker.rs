//! S5 项目选择。条目由宿主从真实项目配置生成。

use egui::{RichText, ScrollArea};

use crate::Cmd;
use crate::style::tokens::{Density, Tokens};
use crate::style::widgets;

#[derive(Debug, Clone)]
pub struct Entry {
    pub asset_path: String,
    pub project: String,
    pub code: String,
    pub mdb: String,
}

/// 开机直接进的那份项目配置。
pub const DEFAULT_ASSET_PATH: &str = "config/e3d.project.ron";

/// 默认项目在条目表里的位置：认 [`DEFAULT_ASSET_PATH`]，找不着就取第一个。
///
/// 宿主开机加载谁、选择框默认选中哪一行，都得问它。两处各挑各的，就会出现
/// 「打开选择框看到的选中项不是正在用的项目」。
pub fn default_index(entries: &[Entry]) -> usize {
    entries
        .iter()
        .position(|entry| entry.asset_path == DEFAULT_ASSET_PATH)
        .unwrap_or(0)
}

pub struct State {
    pub open: bool,
    pub entries: Vec<Entry>,
    selected: usize,
}

impl State {
    pub fn new(entries: Vec<Entry>, open: bool) -> Self {
        let selected = default_index(&entries);
        Self {
            open,
            entries,
            selected,
        }
    }
}

pub fn show(ctx: &egui::Context, t: &Tokens, d: Density, state: &mut State) -> Vec<Cmd> {
    if !state.open {
        return Vec::new();
    }

    let mut cmds = Vec::new();
    egui::Window::new("选择项目")
        .id(egui::Id::new("plant-project-picker"))
        .collapsible(false)
        .resizable(true)
        .default_size([860.0, 620.0])
        .min_size([620.0, 440.0])
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!("{} 个可用项目配置", state.entries.len()))
                    .color(t.text_muted),
            );
            ui.separator();
            if state.entries.is_empty() {
                ui.colored_label(t.warn, "未找到可加载的项目配置。");
            }
            ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height(ui.available_height() - 54.0)
                .show(ui, |ui| {
                    for (index, entry) in state.entries.iter().enumerate() {
                        let selected = index == state.selected;
                        let response = egui::Frame::group(ui.style())
                            .fill(if selected { t.accent_bg } else { t.bg_elevated })
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(&entry.project)
                                                .strong()
                                                .color(t.text_primary),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "项目号 {} · MDB {}",
                                                entry.code, entry.mdb
                                            ))
                                            .monospace()
                                            .color(t.text_muted),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new(&entry.asset_path)
                                                    .monospace()
                                                    .small()
                                                    .color(t.text_muted),
                                            );
                                        },
                                    );
                                });
                            })
                            .response
                            .interact(egui::Sense::click());
                        if response.clicked() {
                            state.selected = index;
                        }
                        ui.add_space(6.0);
                    }
                });
            ui.separator();
            ui.horizontal(|ui| {
                let continue_clicked = ui
                    .add_enabled(
                        state.entries.get(state.selected).is_some(),
                        widgets::button(t, d, "继续进入项目").primary(),
                    )
                    .clicked();
                if (continue_clicked || ctx.input(|input| input.key_pressed(egui::Key::Enter)))
                    && let Some(entry) = state.entries.get(state.selected)
                {
                    cmds.push(Cmd::LoadProject(entry.asset_path.clone()));
                }
            });
        });
    cmds
}
