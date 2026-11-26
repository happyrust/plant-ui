//! 手动提资接口窗口。

use egui::{Align, CornerRadius, Layout, Margin, Pos2, Rect, RichText, ScrollArea, Sense, Stroke};

use crate::Cmd;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, radius, space};
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
    selected_row: Option<usize>,
    selected_col: Option<usize>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            submitting: false,
            title: default_room_code_publish_title(),
            cells: vec![[String::new(), String::new()]; 4],
            selected_row: None,
            selected_col: None,
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
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .open(&mut window_open)
        .collapsible(false)
        .resizable(false)
        .fixed_size([density.px(620.0), density.px(320.0)])
        .show(ctx, |ui| {
            show_contents(ui, tokens, density, state, &mut commands);
        });

    state.open = window_open;
    commands
}

fn show_contents(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    state: &mut State,
    commands: &mut Vec<Cmd>,
) {
    ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);

    ui.horizontal(|ui| {
        let label_width = density.px(40.0);
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, density.input_h()),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.label(
                    RichText::new("标题")
                        .font(Font::label(density))
                        .color(tokens.text_secondary),
                );
            },
        );
        let field_width = ui.available_width().max(density.px(200.0));
        widgets::text_input(
            ui,
            tokens,
            density,
            egui::Id::new("manual-data-publish-title"),
            &mut state.title,
            "请输入提资标题",
            field_width,
        );
    });
    ui.add_space(space::S2);

    let header_h = density.px(30.0);
    let row_h = density.px(32.0);
    let pad_x = density.px(10.0);

    egui::Frame::new()
        .fill(tokens.bg_panel)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::ZERO)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let table_width = ui.available_width();
            let index_w = density.px(40.0);
            let remaining_w = (table_width - index_w).max(density.px(200.0));
            let name_w = (remaining_w * 0.5).floor();
            let room_w = remaining_w - name_w;

            draw_table_header(
                ui,
                tokens,
                density,
                table_width,
                header_h,
                index_w,
                name_w,
                room_w,
                pad_x,
            );

            ScrollArea::vertical()
                .id_salt("manual-data-publish-table-scroll")
                .auto_shrink([false, false])
                .max_height(row_h * 5.0)
                .show(ui, |ui| {
                    ui.set_width(table_width);
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;

                    let row_count = state.cells.len();
                    let mut focus_target: Option<egui::Id> = None;

                    for row_idx in 0..row_count {
                        let (row_rect, _) =
                            ui.allocate_exact_size(egui::vec2(table_width, row_h), Sense::hover());
                        let (idx_rect, col0_rect, col1_rect) =
                            split_row_rects(row_rect, index_w, name_w);

                        let idx_resp = ui
                            .interact(idx_rect, row_index_widget_id(row_idx), Sense::click())
                            .on_hover_cursor(egui::CursorIcon::PointingHand);

                        if idx_resp.clicked() {
                            state.selected_row = Some(row_idx);
                            state.selected_col = None;
                            if let Some(focused_id) = ui.memory(|m| m.focused()) {
                                ui.memory_mut(|m| m.surrender_focus(focused_id));
                            }
                        }

                        let cell0_id = cell_widget_id(row_idx, 0);
                        let cell1_id = cell_widget_id(row_idx, 1);
                        let cell0_focused = ui.memory(|m| m.has_focus(cell0_id));
                        let cell1_focused = ui.memory(|m| m.has_focus(cell1_id));
                        if !idx_resp.clicked() {
                            if cell0_focused {
                                state.selected_row = Some(row_idx);
                                state.selected_col = Some(0);
                            } else if cell1_focused {
                                state.selected_row = Some(row_idx);
                                state.selected_col = Some(1);
                            }
                        }

                        let is_row_selected = state.selected_row == Some(row_idx);
                        let row_hovered = ui.rect_contains_pointer(row_rect);
                        let row_bg = if is_row_selected {
                            tokens.accent_bg
                        } else if row_hovered {
                            tokens.bg_hover
                        } else if row_idx % 2 == 0 {
                            tokens.bg_panel
                        } else {
                            tokens.bg_input
                        };
                        ui.painter()
                            .rect_filled(row_rect, CornerRadius::ZERO, row_bg);

                        if is_row_selected {
                            ui.painter()
                                .rect_filled(idx_rect, CornerRadius::ZERO, tokens.accent);
                        }

                        let idx_text = format!("{}", row_idx + 1);
                        let (idx_font, idx_color) = if is_row_selected {
                            (Font::strong(density), tokens.accent_ink)
                        } else {
                            (Font::mono_meta(density), tokens.text_muted)
                        };
                        let idx_galley =
                            ui.painter().layout_no_wrap(idx_text, idx_font, idx_color);
                        ui.painter().galley(
                            idx_rect.center() - idx_galley.size() / 2.0,
                            idx_galley,
                            idx_color,
                        );

                        let border_stroke = Stroke::new(1.0, tokens.border);
                        ui.painter()
                            .vline(idx_rect.max.x, row_rect.y_range(), border_stroke);
                        ui.painter()
                            .vline(col0_rect.max.x, row_rect.y_range(), border_stroke);
                        if row_idx + 1 < row_count {
                            ui.painter()
                                .hline(row_rect.x_range(), row_rect.max.y, border_stroke);
                        }

                        for (col_idx, cell_rect, hint) in [
                            (0_usize, col0_rect, "输入设备名称"),
                            (1_usize, col1_rect, "输入房间号"),
                        ] {
                            let cell_id = cell_widget_id(row_idx, col_idx);
                            let cell_focused = ui.memory(|m| m.has_focus(cell_id));
                            let is_active_cell = state.selected_row == Some(row_idx)
                                && (state.selected_col == Some(col_idx) || cell_focused);

                            let cell_bg_resp = ui
                                .interact(
                                    cell_rect,
                                    cell_bg_widget_id(row_idx, col_idx),
                                    Sense::click(),
                                )
                                .on_hover_cursor(egui::CursorIcon::Text);

                            if is_active_cell {
                                ui.painter().rect_filled(
                                    cell_rect.shrink(1.0),
                                    CornerRadius::ZERO,
                                    tokens.bg_panel,
                                );
                                ui.painter().rect_stroke(
                                    cell_rect,
                                    CornerRadius::ZERO,
                                    Stroke::new(1.5, tokens.accent),
                                    egui::StrokeKind::Inside,
                                );
                            }

                            let text_rect = cell_rect.shrink2(egui::vec2(pad_x, 0.0));
                            let mut cell_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(text_rect)
                                    .layout(Layout::left_to_right(Align::Center)),
                            );
                            let show_hint = row_idx == 0 || is_active_cell;
                            let mut editor =
                                egui::TextEdit::singleline(&mut state.cells[row_idx][col_idx])
                                    .id(cell_id)
                                    .frame(egui::Frame::NONE)
                                    .background_color(egui::Color32::TRANSPARENT)
                                    .margin(egui::Margin::ZERO)
                                    .desired_width(text_rect.width())
                                    .font(Font::label(density))
                                    .text_color(tokens.text_primary);
                            if show_hint {
                                editor = editor.hint_text(
                                    RichText::new(hint)
                                        .font(Font::label(density))
                                        .color(tokens.text_muted),
                                );
                            }
                            let edit_resp = cell_ui.add(editor);

                            if cell_bg_resp.clicked() {
                                state.selected_row = Some(row_idx);
                                state.selected_col = Some(col_idx);
                                edit_resp.request_focus();
                            } else if edit_resp.clicked() || edit_resp.gained_focus() {
                                state.selected_row = Some(row_idx);
                                state.selected_col = Some(col_idx);
                            }

                            if cell_focused && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                if row_idx + 1 < row_count {
                                    state.selected_row = Some(row_idx + 1);
                                    state.selected_col = Some(col_idx);
                                    focus_target = Some(cell_widget_id(row_idx + 1, col_idx));
                                }
                            }
                        }
                    }

                    if let Some(target_id) = focus_target {
                        ui.memory_mut(|m| m.request_focus(target_id));
                    }
                });
        });

    if state
        .cells
        .iter()
        .all(|row| row.iter().all(|value| !value.trim().is_empty()))
    {
        state.cells.push([String::new(), String::new()]);
    }

    ui.add_space(space::S2);
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        if ui.add(widgets::button(tokens, density, "添加行")).clicked() {
            state.add_row();
            if let Some(new_row) = state.selected_row {
                ui.memory_mut(|m| m.request_focus(cell_widget_id(new_row, 0)));
            }
        }
        let delete_resp = ui.add_enabled(
            state.selected_row.is_some(),
            widgets::button(tokens, density, "删除选中行"),
        );
        if delete_resp.clicked() {
            state.delete_selected_row();
        }

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
}

fn row_index_widget_id(row_idx: usize) -> egui::Id {
    egui::Id::new(("manual-data-publish-row-idx", row_idx))
}

fn cell_bg_widget_id(row_idx: usize, col_idx: usize) -> egui::Id {
    egui::Id::new(("manual-data-publish-cell-bg", row_idx, col_idx))
}

fn cell_widget_id(row_idx: usize, col_idx: usize) -> egui::Id {
    egui::Id::new(("manual-data-publish-cell", row_idx, col_idx))
}

fn split_row_rects(row_rect: Rect, index_w: f32, name_w: f32) -> (Rect, Rect, Rect) {
    let idx_max_x = row_rect.min.x + index_w;
    let col0_max_x = idx_max_x + name_w;
    let idx_rect = Rect::from_min_max(row_rect.min, Pos2::new(idx_max_x, row_rect.max.y));
    let col0_rect = Rect::from_min_max(
        Pos2::new(idx_max_x, row_rect.min.y),
        Pos2::new(col0_max_x, row_rect.max.y),
    );
    let col1_rect = Rect::from_min_max(Pos2::new(col0_max_x, row_rect.min.y), row_rect.max);
    (idx_rect, col0_rect, col1_rect)
}

#[allow(clippy::too_many_arguments)]
fn draw_table_header(
    ui: &mut egui::Ui,
    tokens: &Tokens,
    density: Density,
    table_width: f32,
    header_h: f32,
    index_w: f32,
    name_w: f32,
    _room_w: f32,
    pad_x: f32,
) {
    let (header_rect, _) =
        ui.allocate_exact_size(egui::vec2(table_width, header_h), Sense::hover());
    ui.painter().rect_filled(
        header_rect,
        CornerRadius {
            nw: radius::MD,
            ne: radius::MD,
            sw: 0,
            se: 0,
        },
        tokens.bg_header,
    );

    let (idx_rect, col0_rect, col1_rect) = split_row_rects(header_rect, index_w, name_w);
    let border_stroke = Stroke::new(1.0, tokens.border);
    ui.painter()
        .hline(header_rect.x_range(), header_rect.max.y, border_stroke);
    ui.painter()
        .vline(idx_rect.max.x, header_rect.y_range(), border_stroke);
    ui.painter()
        .vline(col0_rect.max.x, header_rect.y_range(), border_stroke);

    let idx_galley =
        ui.painter()
            .layout_no_wrap("#".to_owned(), Font::strong(density), tokens.text_secondary);
    ui.painter().galley(
        idx_rect.center() - idx_galley.size() / 2.0,
        idx_galley,
        tokens.text_secondary,
    );

    for (rect, label) in [(col0_rect, "设备名称"), (col1_rect, "房间号")] {
        let galley = ui.painter().layout_no_wrap(
            label.to_owned(),
            Font::strong(density),
            tokens.text_primary,
        );
        let pos = Pos2::new(
            rect.min.x + pad_x,
            rect.center().y - galley.size().y * 0.5,
        );
        ui.painter().galley(pos, galley, tokens.text_primary);
    }
}

impl State {
    fn add_row(&mut self) {
        self.cells.push([String::new(), String::new()]);
        self.selected_row = Some(self.cells.len() - 1);
        self.selected_col = Some(0);
    }

    fn delete_selected_row(&mut self) {
        let Some(idx) = self.selected_row else {
            return;
        };
        if idx >= self.cells.len() {
            self.selected_row = (!self.cells.is_empty()).then_some(self.cells.len() - 1);
            return;
        }
        if self.cells.len() > 1 {
            self.cells.remove(idx);
            self.selected_row = Some(idx.min(self.cells.len() - 1));
        } else {
            self.cells[0] = [String::new(), String::new()];
            self.selected_row = Some(0);
        }
        self.selected_col = None;
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_trims_fields_and_omits_empty_or_incomplete_rows() {
        let state = State {
            title: "  手动提资测试  ".into(),
            cells: vec![
                ["  /PUMP-001  ".into(), "  R101  ".into()],
                ["/PUMP-002".into(), "  ".into()],
                ["".into(), "R102".into()],
                ["".into(), "".into()],
            ],
            ..State::default()
        };
        assert!(state.has_entries());
        assert_eq!(
            serde_json::to_value(state.request()).unwrap(),
            serde_json::json!({
                "title": "手动提资测试",
                "room_codes": [{"name": "/PUMP-001", "room_code": "R101"}]
            })
        );
        assert!(!State::default().has_entries());
    }

    #[test]
    fn add_and_delete_selected_row_maintains_valid_selection() {
        let mut state = State::default();
        assert_eq!(state.cells.len(), 4);
        assert_eq!(state.selected_row, None);

        state.add_row();
        assert_eq!(state.cells.len(), 5);
        assert_eq!(state.selected_row, Some(4));
        assert_eq!(state.selected_col, Some(0));

        state.delete_selected_row();
        assert_eq!(state.cells.len(), 4);
        assert_eq!(state.selected_row, Some(3));
        assert_eq!(state.selected_col, None);

        state.cells = vec![["/PUMP-1".into(), "R101".into()]];
        state.selected_row = Some(0);
        state.delete_selected_row();
        assert_eq!(state.cells.len(), 1);
        assert_eq!(state.cells[0], [String::new(), String::new()]);
        assert_eq!(state.selected_row, Some(0));
    }

    #[test]
    fn clicking_row_index_and_data_cell_selects_and_focuses_cleanly() {
        let ctx = egui::Context::default();
        let tokens = Tokens::light();
        let d = Density::Standard;
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(720.0, 420.0));
        let mut state = State {
            open: true,
            ..State::default()
        };

        let render = |state: &mut State, events: Vec<egui::Event>| -> (Option<Rect>, Option<Rect>) {
            let mut row1_idx_rect = None;
            let mut row2_col0_rect = None;
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            });
            egui::Area::new("test-manual-publish".into())
                .fixed_pos(screen.min)
                .show(&ctx, |ui| {
                    ui.set_min_size(screen.size());
                    ui.set_max_size(screen.size());
                    let mut cmds = Vec::new();
                    show_contents(ui, &tokens, d, state, &mut cmds);
                    row1_idx_rect = ctx.read_response(row_index_widget_id(1)).map(|r| r.rect);
                    row2_col0_rect = ctx.read_response(cell_bg_widget_id(2, 0)).map(|r| r.rect);
                });
            let _ = ctx.end_pass();
            (row1_idx_rect, row2_col0_rect)
        };

        let _ = render(&mut state, Vec::new());
        let (idx_rect, col0_rect) = render(&mut state, Vec::new());
        let idx_pos = idx_rect.expect("row 1 index rect").center();
        let col0_rect = col0_rect.expect("row 2 col 0 rect");
        // Click near top padding of the data cell so cell_bg_resp receives it directly too.
        let cell_pos = Pos2::new(col0_rect.min.x + 6.0, col0_rect.min.y + 4.0);

        let click_at = |state: &mut State, pos: Pos2| {
            let _ = render(state, vec![egui::Event::PointerMoved(pos)]);
            let _ = render(
                state,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            let _ = render(
                state,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        };

        click_at(&mut state, idx_pos);
        assert_eq!(state.selected_row, Some(1));
        assert_eq!(state.selected_col, None);

        click_at(&mut state, cell_pos);
        assert_eq!(state.selected_row, Some(2));
        assert_eq!(state.selected_col, Some(0));
        assert!(ctx.memory(|m| m.has_focus(cell_widget_id(2, 0))));

        click_at(&mut state, idx_pos);
        assert_eq!(state.selected_row, Some(1));
        assert_eq!(state.selected_col, None);
    }
}
