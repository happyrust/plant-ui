//! 外壳三条横栏：标题栏 38 / 命令栏 40 / 状态栏 26（S1 画板规格）。
//!
//! 状态栏只摆应用确实持有的数据（数据源状态、项目 / 库、选中元素、元素计数）；
//! 设计稿上的已应用 sesno / 文件 sesno / 待更新批次 / 待重试单元属于 gen-model
//! 侧，等 M4-4 定下数据边界后再补，宁可少一格。

use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as ph;

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius, space};
use crate::style::widgets;
use crate::vm::WorkbenchVm;

pub fn title_bar(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm) {
    // 栏体(Frame)与 hairline 必须严丝合缝：全局 item_spacing.y=6 会把
    // hairline 推出面板裁剪区（Panel 只比栏体高 1px）。
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::Frame::new()
        .fill(t.bg_chrome)
        .inner_margin(Margin::symmetric(space::S3 as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(d.title_bar_h());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                logo(ui, t, d);
                ui.label(
                    RichText::new("布置平台")
                        .font(Font::strong(d))
                        .color(t.text_primary),
                );
                // 连接成功前没有工程标识，这一段整体不画（不摆空芯片）。
                if !vm.project.is_empty() {
                    divider(ui, t, d);
                    ui.add(widgets::status_tag(t, d, &vm.project, Status::Info));
                    ui.label(
                        RichText::new(&vm.db)
                            .font(Font::mono_meta(d))
                            .color(t.text_muted),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(&vm.user)
                            .font(Font::meta(d))
                            .color(t.text_secondary),
                    );
                    avatar(ui, t, d, &vm.user);
                    divider(ui, t, d);
                    search_box(ui, t, d, d.px(320.0), "搜索元素、REFNO 或命令", "Ctrl K");
                });
            });
        });
    hairline(ui, t);
}

pub fn command_bar(ui: &mut Ui, t: &Tokens, d: Density, _vm: &WorkbenchVm) {
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::Frame::new()
        .fill(t.bg_panel)
        .inner_margin(Margin::symmetric(space::S2 as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(d.command_bar_h());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (i, name) in ["项目", "创建", "元件库", "插件", "设置", "帮助"]
                    .into_iter()
                    .enumerate()
                {
                    // M1-1 只有视觉：菜单项的下拉与任务窗在 M4 接。
                    let _ = ui.add(widgets::chip(t, d, name, i == 0));
                }
                ui.add_space(space::S2);
                divider(ui, t, d);
                ui.add_space(space::S2);
                for icon in [
                    ph::ARROW_COUNTER_CLOCKWISE,
                    ph::ARROW_CLOCKWISE,
                    ph::FLOPPY_DISK,
                ] {
                    let _ = ui.add(widgets::tool_btn(t, d, icon, false));
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // 同一屏只有一个 accent 主操作：模型更新（S2/S4 任务窗，M4 接线）。
                    let _ = ui.add(widgets::button(t, d, "生成模型").icon(ph::PLAY));
                    ui.add_space(space::S1);
                    let _ = ui.add(
                        widgets::button(t, d, "模型更新")
                            .icon(ph::ARROWS_CLOCKWISE)
                            .primary(),
                    );
                });
            });
        });
    hairline(ui, t);
}

pub fn status_bar(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm) {
    ui.spacing_mut().item_spacing.y = 0.0;
    hairline(ui, t);
    egui::Frame::new()
        .fill(t.bg_chrome)
        .inner_margin(Margin::symmetric(space::S3 as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(d.status_bar_h());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                let (source_color, source_text) = if vm.data_source_ok {
                    (t.success, "数据源 就绪")
                } else {
                    (t.warn, "数据源 未连接")
                };
                dot(ui, d, source_color);
                ui.label(
                    RichText::new(source_text)
                        .font(Font::micro(d))
                        .color(t.text_muted),
                );
                if !vm.project.is_empty() {
                    divider(ui, t, d);
                    meta_icon(ui, d, ph::DATABASE, t.accent);
                    ui.label(
                        RichText::new(&vm.project)
                            .font(Font::micro(d))
                            .color(t.text_secondary),
                    );
                    ui.label(
                        RichText::new(&vm.db)
                            .font(Font::mono_micro(d))
                            .color(t.text_muted),
                    );
                }
                divider(ui, t, d);
                meta_icon(ui, d, ph::CURSOR, t.text_secondary);
                match &vm.selected {
                    Some(refno) => ui.label(
                        RichText::new(format!("{refno:?}"))
                            .font(Font::mono_micro(d))
                            .color(t.text_secondary),
                    ),
                    None => ui.label(
                        RichText::new("未选中")
                            .font(Font::micro(d))
                            .color(t.text_muted),
                    ),
                };

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(vm.element_count.to_string())
                            .font(Font::mono_micro(d))
                            .color(t.text_secondary),
                    );
                    ui.label(
                        RichText::new("元素")
                            .font(Font::micro(d))
                            .color(t.text_muted),
                    );
                });
            });
        });
}

// ---------------------------------------------------------------- 小零件

fn logo(ui: &mut Ui, t: &Tokens, d: Density) {
    let s = d.px(24.0);
    let (rect, _) = ui.allocate_exact_size(vec2(s, s), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::MD), t.accent);
    let g = ui.painter().layout_no_wrap(
        ph::CUBE_FOCUS.to_owned(),
        egui::FontId::new(d.px(14.0), egui::FontFamily::Proportional),
        t.accent_ink,
    );
    let pos = rect.center() - g.size() / 2.0;
    ui.painter().galley(pos, g, t.accent_ink);
}

fn avatar(ui: &mut Ui, t: &Tokens, d: Density, name: &str) {
    let s = d.px(22.0);
    let (rect, _) = ui.allocate_exact_size(vec2(s, s), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), s / 2.0, t.accent_bg);
    let ch: String = name.chars().take(1).collect();
    let g = ui.painter().layout_no_wrap(ch, Font::micro(d), t.accent);
    ui.painter()
        .galley(rect.center() - g.size() / 2.0, g, t.accent);
}

pub fn divider(ui: &mut Ui, t: &Tokens, d: Density) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, d.px(16.0)), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.border);
}

pub fn hairline(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.border);
}

fn dot(ui: &mut Ui, d: Density, color: Color32) {
    let s = d.px(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(s, s), Sense::hover());
    ui.painter().circle_filled(rect.center(), s / 2.0, color);
}

fn meta_icon(ui: &mut Ui, d: Density, icon: &str, color: Color32) {
    ui.label(
        RichText::new(icon)
            .font(egui::FontId::new(
                d.px(12.0),
                egui::FontFamily::Proportional,
            ))
            .color(color),
    );
}

/// 纯视觉搜索框（M1-1）；真实的命令面板 / 搜索交互不在本里程碑。
fn search_box(ui: &mut Ui, t: &Tokens, d: Density, width: f32, placeholder: &str, key: &str) {
    let h = d.px(26.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    let cr = CornerRadius::same(radius::MD);
    ui.painter().rect_filled(rect, cr, t.bg_input);
    ui.painter().rect_stroke(
        rect,
        cr,
        Stroke::new(1.0_f32, t.border),
        egui::StrokeKind::Inside,
    );
    let pad = d.px(10.0);
    let ig = ui.painter().layout_no_wrap(
        ph::MAGNIFYING_GLASS.to_owned(),
        egui::FontId::new(d.px(13.0), egui::FontFamily::Proportional),
        t.text_muted,
    );
    let iw = ig.size().x;
    ui.painter().galley(
        pos2(rect.left() + pad, rect.center().y - ig.size().y / 2.0),
        ig,
        t.text_muted,
    );
    let pg = ui
        .painter()
        .layout_no_wrap(placeholder.to_owned(), Font::meta(d), t.text_muted);
    ui.painter().galley(
        pos2(
            rect.left() + pad + iw + d.px(8.0),
            rect.center().y - pg.size().y / 2.0,
        ),
        pg,
        t.text_muted,
    );
    let kg = ui
        .painter()
        .layout_no_wrap(key.to_owned(), Font::mono_micro(d), t.text_muted);
    ui.painter().galley(
        pos2(
            rect.right() - pad - kg.size().x,
            rect.center().y - kg.size().y / 2.0,
        ),
        kg,
        t.text_muted,
    );
}
