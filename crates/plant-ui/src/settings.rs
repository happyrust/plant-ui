//! S6 设置任务窗。只暴露当前两端都能立即生效的选项。

use egui::{Align, Layout, RichText};

use crate::style::tokens::{Density, Tokens};
use crate::style::widgets;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

impl Theme {
    pub const fn tokens(self) -> Tokens {
        match self {
            Self::Light => Tokens::light(),
            Self::Dark => Tokens::dark(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub theme: Theme,
    pub density: Density,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Light,
            density: Density::Standard,
        }
    }
}

pub struct State {
    pub open: bool,
    pub saved: Settings,
    draft: Settings,
}

impl Default for State {
    fn default() -> Self {
        let saved = Settings::default();
        Self {
            open: false,
            draft: saved.clone(),
            saved,
        }
    }
}

impl State {
    pub fn open(&mut self) {
        self.draft = self.saved.clone();
        self.open = true;
    }
}

/// Returns the saved settings when the user confirms the dialog.
pub fn show(ctx: &egui::Context, t: &Tokens, d: Density, state: &mut State) -> Option<Settings> {
    if !state.open {
        return None;
    }

    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    egui::Window::new("设置")
        .id(egui::Id::new("plant-settings-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([880.0, 640.0])
        .min_size([620.0, 480.0])
        .show(ctx, |ui| {
            ui.label(RichText::new("外观").strong().color(t.text_secondary));
            setting_row(ui, t, "主题", "亮色为默认主题", |ui| {
                widgets::segmented(
                    ui,
                    t,
                    d,
                    &mut state.draft.theme,
                    &[(Theme::Light, "亮色"), (Theme::Dark, "深色")],
                );
            });
            setting_row(
                ui,
                t,
                "界面密度",
                "影响字号、行高和控件尺寸",
                |ui| {
                    widgets::segmented(
                        ui,
                        t,
                        d,
                        &mut state.draft.density,
                        &[
                            (Density::Compact, "紧凑 12"),
                            (Density::Standard, "标准 13"),
                            (Density::Relaxed, "宽松 15"),
                        ],
                    );
                },
            );

            ui.with_layout(Layout::bottom_up(Align::RIGHT), |ui| {
                ui.horizontal(|ui| {
                    if ui.add(widgets::button(t, d, "保存").primary()).clicked() {
                        save = true;
                    }
                    if ui.add(widgets::button(t, d, "取消")).clicked() {
                        cancel = true;
                    }
                    if ui.add(widgets::button(t, d, "恢复默认值")).clicked() {
                        state.draft = Settings::default();
                    }
                });
            });
        });

    if cancel || !open {
        state.open = false;
        return None;
    }
    if save {
        state.saved = state.draft.clone();
        state.open = false;
        return Some(state.saved.clone());
    }
    None
}

fn setting_row(
    ui: &mut egui::Ui,
    t: &Tokens,
    title: &str,
    detail: &str,
    trailing: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(title).color(t.text_primary));
            ui.label(RichText::new(detail).small().color(t.text_muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_light() {
        assert_eq!(Settings::default().theme, Theme::Light);
    }
}
