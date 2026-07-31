//! 手动提资接口窗口。

use egui::{Align, CornerRadius, Layout, ScrollArea, Sense, Stroke};

use crate::Cmd;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, space};
use crate::style::widgets;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RoomCodeEntry {
    pub name: String,
    pub room_code: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RoomCodePublishRequest {
    #[serde(default = "default_room_code_publish_title")]
    pub title: String,
    pub room_codes: Vec<RoomCodeEntry>,
}

pub fn default_room_code_publish_title() -> String {
    "手动提资".to_owned()
}

#[derive(Debug)]
pub struct State {
    pub open: bool,
    pub submitting: bool,
    title: String,
    cells: Vec<[String; 2]>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            submitting: false,
            title: default_room_code_publish_title(),
            cells: vec![[String::new(), String::new()]; 4],
        }
    }
}

pub fn show(ctx: &egui::Context, tokens: &Tokens, density: Density, state: &mut State) -> Vec<Cmd> {
    if !state.open {
        return Vec::new();
    }

    let mut commands = Vec::new();
    let mut window_open = state.open;
    egui::Window::new("手动提资接口")
        .id(egui::Id::new("manual-data-publish-window"))
        .open(&mut window_open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([density.px(620.0), density.px(280.0)])
        .show(ctx, |ui| {
            let cell_height = density.px(36.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("标题:")
                        .font(Font::meta(density))
                        .color(tokens.text_muted),
                );
                ui.add_sized(
                    [density.px(360.0), density.btn_h()],
                    egui::TextEdit::singleline(&mut state.title).hint_text("请输入提资标题"),
                );
            });
            ui.add_space(space::S2);

            ScrollArea::vertical()
                .id_salt("manual-data-publish-table-scroll")
                .auto_shrink([false, false])
                .max_height(cell_height * 5.0)
                .show(ui, |ui| {
                    let cell_width = ui.available_width() / 2.0;
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    ui.horizontal(|ui| {
                        table_header(ui, tokens, density, "设备名称", cell_width, cell_height);
                        table_header(ui, tokens, density, "房间号", cell_width, cell_height);
                    });

                    for row in &mut state.cells {
                        ui.horizontal(|ui| {
                            for value in row {
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(cell_width, cell_height),
                                    Sense::click(),
                                );
                                ui.painter()
                                    .rect_filled(rect, CornerRadius::ZERO, tokens.bg_input);
                                ui.painter().rect_stroke(
                                    rect,
                                    CornerRadius::ZERO,
                                    Stroke::new(1.0, tokens.border),
                                    egui::StrokeKind::Inside,
                                );
                                ui.put(
                                    rect.shrink(1.0),
                                    egui::TextEdit::singleline(value)
                                        .frame(egui::Frame::NONE)
                                        .horizontal_align(Align::Center)
                                        .vertical_align(Align::Center),
                                );
                            }
                        });
                    }
                });

            if state
                .cells
                .iter()
                .all(|row| row.iter().all(|value| !value.trim().is_empty()))
            {
                state.cells.push([String::new(), String::new()]);
            }

            ui.add_space(space::S2);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let submit = ui.add_enabled(
                    !state.submitting && state.has_entries(),
                    widgets::button(tokens, density, "提交")
                        .primary()
                        .loading(state.submitting),
                );
                if submit.clicked() {
                    state.submitting = true;
                    commands.push(Cmd::SubmitRoomCodePublish(state.request()));
                }
            });
        });

    state.open = window_open;
    commands
}

impl State {
    fn has_entries(&self) -> bool {
        self.cells
            .iter()
            .any(|row| row.iter().all(|value| !value.trim().is_empty()))
    }

    fn request(&self) -> RoomCodePublishRequest {
        RoomCodePublishRequest {
            title: self.title.trim().to_owned(),
            room_codes: self
                .cells
                .iter()
                .filter_map(|row| {
                    let equipment_name = row[0].trim();
                    let room_code = row[1].trim();
                    (!equipment_name.is_empty() && !room_code.is_empty()).then(|| RoomCodeEntry {
                        name: equipment_name.to_owned(),
                        room_code: room_code.to_owned(),
                    })
                })
                .collect(),
        }
    }
}

fn table_header(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    label: &str,
    width: f32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, tokens.bg_elevated);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(1.0, tokens.border),
        egui::StrokeKind::Inside,
    );
    let text =
        ui.painter()
            .layout_no_wrap(label.to_owned(), Font::strong(density), tokens.text_primary);
    ui.painter()
        .galley(rect.center() - text.size() / 2.0, text, tokens.text_primary);
}
