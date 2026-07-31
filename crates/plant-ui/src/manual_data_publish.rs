//! 手动提资接口窗口。

use egui::{Align, CornerRadius, Layout, ScrollArea, Sense, Stroke};

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, space};
use crate::style::widgets;

#[derive(Debug)]
pub struct State {
    pub open: bool,
    cells: Vec<[String; 2]>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            cells: vec![[String::new(), String::new()]; 4],
        }
    }
}

pub fn show(ctx: &egui::Context, tokens: &Tokens, density: Density, state: &mut State) {
    if !state.open {
        return;
    }

    let mut window_open = state.open;
    egui::Window::new("手动提资接口")
        .id(egui::Id::new("manual-data-publish-window"))
        .open(&mut window_open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([density.px(560.0), density.px(232.0)])
        .show(ctx, |ui| {
            let cell_height = density.px(36.0);

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
                                ui.painter().rect_filled(
                                    rect,
                                    CornerRadius::ZERO,
                                    tokens.bg_input,
                                );
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
                ui.add(widgets::button(tokens, density, "提交").primary());
            });
        });

    state.open = window_open;
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
    let text = ui.painter().layout_no_wrap(
        label.to_owned(),
        Font::strong(density),
        tokens.text_primary,
    );
    ui.painter().galley(
        rect.center() - text.size() / 2.0,
        text,
        tokens.text_primary,
    );
}
