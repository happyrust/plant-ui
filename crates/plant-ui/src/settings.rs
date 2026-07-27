//! S6 设置任务窗。只暴露当前两端都能立即生效的选项。

use egui::{Align, Layout, RichText, TextEdit};

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens};
use crate::style::widgets;

/// 模型服务地址的出厂默认。
///
/// 8020 的回环侧被另一个 SurrealDB 实例占着，gen-model 因此让到了 8021
/// （见 gen-model/DbOption.toml 的注释）。指向 8020 不会干净地连不上，
/// 而是打进那个实例、拿回一个看着像模像样的 HTTP 错误。
pub const DEFAULT_MODEL_API_URL: &str = "http://127.0.0.1:8021";

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
    pub model_api_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Light,
            density: Density::Standard,
            model_api_url: DEFAULT_MODEL_API_URL.to_owned(),
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

    /// 用宿主解析出来的值顶掉出厂默认。环境变量这类来源只有宿主认得，
    /// 绘制层不去读它。
    pub fn adopt(&mut self, settings: Settings) {
        self.draft = settings.clone();
        self.saved = settings;
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

            ui.add_space(12.0);
            ui.label(RichText::new("服务").strong().color(t.text_secondary));
            setting_row(
                ui,
                t,
                "模型服务地址",
                "gen-model 的 REST 与 WebSocket 入口，保存后下一次预览生效",
                |ui| {
                    ui.add(
                        TextEdit::singleline(&mut state.draft.model_api_url)
                            .desired_width(300.0)
                            .font(Font::mono(d))
                            .hint_text(DEFAULT_MODEL_API_URL),
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
        state.draft.model_api_url = normalize_api_url(&state.draft.model_api_url);
        state.saved = state.draft.clone();
        state.open = false;
        return Some(state.saved.clone());
    }
    None
}

/// 地址拼接是 `{base}{path}`，尾斜杠会拼出 `//api/v1`；清空则退回出厂默认，
/// 免得存下一个连不上任何东西的空串。
fn normalize_api_url(raw: &str) -> String {
    let url = raw.trim().trim_end_matches('/');
    if url.is_empty() {
        DEFAULT_MODEL_API_URL.to_owned()
    } else {
        url.to_owned()
    }
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

    #[test]
    fn default_api_url_avoids_the_surreal_port() {
        assert_eq!(Settings::default().model_api_url, DEFAULT_MODEL_API_URL);
        assert!(!DEFAULT_MODEL_API_URL.ends_with(":8020"));
    }

    #[test]
    fn normalizing_strips_trailing_slash_and_refuses_empty() {
        assert_eq!(
            normalize_api_url("  http://10.0.0.9:8021/  "),
            "http://10.0.0.9:8021"
        );
        assert_eq!(normalize_api_url("   "), DEFAULT_MODEL_API_URL);
    }

    #[test]
    fn adopt_syncs_draft_so_the_dialog_opens_on_the_host_value() {
        let mut state = State::default();
        state.adopt(Settings {
            model_api_url: "http://10.0.0.9:8021".into(),
            ..Settings::default()
        });
        state.open();
        assert_eq!(state.saved.model_api_url, "http://10.0.0.9:8021");
    }
}
