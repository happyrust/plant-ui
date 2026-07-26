//! 三维视口（M1-5：占位纹理）。
//!
//! 底是 `viewport-top` -> `viewport-bottom` 的竖直渐变，上面按 S1 那块「模型场景」
//! 矩形等比裁切铺一张占位纹理（0.92 不透明）。等比裁切而不是拉伸：视口的宽高比
//! 跟着 dock 分隔条走，拉伸的话管子会随手一拖就变成椭圆。
//!
//! S1 视口里另外几件浮层——FPS 与已加载计数、视图立方体、左侧工具栏、坐标读数、
//! 比例尺——这一版都没画。它们每一个字都要相机和渲染器才有真值，现在摆上去就是
//! 四个假数字加一排点不动的按钮。按外壳一贯的「宁可少一格」，只留一块讲清楚这是
//! 占位的 HUD，其余等 M3 接回 Bevy 有了真数据再补。

use egui::{Color32, Mesh, Rect, RichText, Sense, Shape, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as ph;

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, radius, space};
use crate::vm::WorkbenchVm;

/// 占位纹理的不透明度，取自 S1 的「模型场景」矩形。压这么一点让渐变底透上来，
/// 图与视口连成一片，不像贴上去的一张照片。
const TEXTURE_ALPHA: u8 = 235;

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm) {
    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
    ui.painter()
        .add(gradient(rect, t.viewport_top, t.viewport_bottom));

    let Some(view) = vm.view3d else {
        note(
            ui,
            t,
            d,
            rect,
            "占位纹理未能载入",
            "assets/textures 下的图缺了或读不了",
        );
        return;
    };

    ui.painter().image(
        view.texture,
        rect,
        cover_uv(rect.size(), view.size),
        Color32::from_white_alpha(TEXTURE_ALPHA),
    );
    hud(ui, t, d, rect);
}

/// 竖直渐变。egui 没有渐变笔刷，四个顶点各自带色的三角网格就是标准做法。
fn gradient(rect: Rect, top: Color32, bottom: Color32) -> Shape {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(2, 1, 3);
    Shape::mesh(mesh)
}

/// 等比裁切铺满（CSS 的 `object-fit: cover`）：算出该取纹理中间多大一块。
/// 两个方向上取到的比例都 ≤ 1，其中一个正好等于 1。
fn cover_uv(view: egui::Vec2, texture: egui::Vec2) -> Rect {
    if texture.x <= 0.0 || texture.y <= 0.0 || view.x <= 0.0 || view.y <= 0.0 {
        return Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    }
    let scale = (view.x / texture.x).max(view.y / texture.y);
    let visible = vec2(view.x / (texture.x * scale), view.y / (texture.y * scale));
    Rect::from_center_size(pos2(0.5, 0.5), visible)
}

/// 左上角的 HUD（S1 的「HUD状态」位置与规格：距边 14、高 26、bg-elevated + 1px 边）。
/// 内容换成实话：这是占位纹理，不是渲染出来的画面。
fn hud(ui: &mut Ui, t: &Tokens, d: Density, view: Rect) {
    let inset = d.px(14.0);
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                view.min + vec2(inset, inset),
                view.max - vec2(inset, inset),
            ))
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
    );
    egui::Frame::new()
        .fill(t.bg_elevated)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(radius::MD)
        .inner_margin(egui::Margin::symmetric(d.px(10.0) as i8, 0))
        .show(&mut child, |ui| {
            ui.set_height(d.px(26.0));
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(10.0);
                let (dot, _) = ui.allocate_exact_size(egui::Vec2::splat(d.px(6.0)), Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), d.px(3.0), t.text_muted);
                ui.label(
                    RichText::new("占位纹理 · 未接渲染器")
                        .font(Font::mono_meta(d))
                        .color(t.text_secondary),
                );
                let (div, _) = ui.allocate_exact_size(vec2(1.0, d.px(12.0)), Sense::hover());
                ui.painter().rect_filled(div, 0, t.border);
                ui.label(
                    RichText::new("实时视口在 M3 接回 Bevy")
                        .font(Font::mono_meta(d))
                        .color(t.text_muted),
                );
            });
        });
}

/// 纹理都没有时的居中提示。四态统一视觉 M1-7 收口。
fn note(ui: &mut Ui, t: &Tokens, d: Density, view: Rect, title: &str, hint: &str) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(view));
    child.vertical_centered(|ui| {
        ui.add_space(view.height() / 2.0 - d.px(40.0));
        ui.label(
            RichText::new(ph::IMAGE_BROKEN)
                .font(egui::FontId::proportional(d.px(28.0)))
                .color(t.text_muted),
        );
        ui.add_space(space::S2);
        ui.label(
            RichText::new(title)
                .font(Font::label(d))
                .color(t.text_secondary),
        );
        ui.label(RichText::new(hint).font(Font::meta(d)).color(t.text_muted));
    });
}
