//! 组件层。
//!
//! 硬约束：这里只依赖 egui 与令牌，不认识 Bevy、不认识任何数据源。
//! 所有尺寸都从 `Density` 换算，不写死像素。

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Galley, Pos2, Rect, Response, Sense, Stroke,
    StrokeKind, Ui, Widget, pos2, vec2,
};

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius, space};

fn lay(ui: &Ui, text: &str, font: FontId) -> Arc<Galley> {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
}

fn icon_font(d: Density) -> FontId {
    FontId::new(d.px(14.0), FontFamily::Proportional)
}

fn draw_galley(ui: &Ui, pos: Pos2, galley: Arc<Galley>, color: Color32) {
    ui.painter().galley(pos, galley, color);
}

fn fill_rect(ui: &Ui, rect: Rect, r: u8, bg: Color32, stroke: Option<Color32>) {
    let cr = CornerRadius::same(r);
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, cr, bg);
    }
    if let Some(s) = stroke {
        ui.painter()
            .rect_stroke(rect, cr, Stroke::new(1.0_f32, s), StrokeKind::Inside);
    }
}

// ---------------------------------------------------------------- 按钮

pub struct Btn<'a> {
    t: &'a Tokens,
    d: Density,
    label: &'a str,
    icon: Option<&'a str>,
    primary: bool,
    width: Option<f32>,
}

impl<'a> Btn<'a> {
    pub fn icon(mut self, icon: &'a str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn full_width(mut self) -> Self {
        self.width = Some(f32::INFINITY);
        self
    }
}

pub fn button<'a>(t: &'a Tokens, d: Density, label: &'a str) -> Btn<'a> {
    Btn {
        t,
        d,
        label,
        icon: None,
        primary: false,
        width: None,
    }
}

impl Widget for Btn<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let d = self.d;
        let pad = d.px(12.0);
        let gap = d.px(6.0);

        let label = lay(ui, self.label, Font::strong(d));
        let icon = self.icon.map(|i| lay(ui, i, icon_font(d)));

        let mut w = pad * 2.0 + label.size().x;
        if let Some(g) = &icon {
            w += g.size().x + gap;
        }
        if self.width == Some(f32::INFINITY) {
            w = ui.available_width();
        }

        let (rect, resp) = ui.allocate_exact_size(vec2(w, d.btn_h()), Sense::click());
        if !ui.is_rect_visible(rect) {
            return resp;
        }

        let t = self.t;
        let (bg, fg, border) = if self.primary {
            let bg = if resp.hovered() { t.accent_strong } else { t.accent };
            (bg, t.accent_ink, None)
        } else if resp.hovered() {
            (t.bg_hover, t.text_primary, Some(t.border_strong))
        } else {
            (t.bg_elevated, t.text_primary, Some(t.border))
        };

        fill_rect(ui, rect, radius::MD, bg, border);

        let content_w = label.size().x + icon.as_ref().map_or(0.0, |g| g.size().x + gap);
        let mut x = rect.center().x - content_w / 2.0;
        if let Some(g) = icon {
            let y = rect.center().y - g.size().y / 2.0;
            let size = g.size();
            draw_galley(ui, pos2(x, y), g, fg);
            x += size.x + gap;
        }
        let y = rect.center().y - label.size().y / 2.0;
        draw_galley(ui, pos2(x, y), label, fg);

        resp
    }
}

// ---------------------------------------------------------------- 芯片

pub fn chip<'a>(t: &'a Tokens, d: Density, label: &'a str, active: bool) -> impl Widget + 'a {
    move |ui: &mut Ui| {
        let pad = d.px(9.0);
        let g = lay(ui, label, Font::meta(d));
        let h = d.px(22.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + pad * 2.0, h), Sense::click());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let (bg, fg, border) = if active {
            (t.accent_bg, t.accent, None)
        } else if resp.hovered() {
            (t.bg_hover, t.text_secondary, Some(t.border_strong))
        } else {
            (Color32::TRANSPARENT, t.text_muted, Some(t.border))
        };
        fill_rect(ui, rect, (h / 2.0) as u8, bg, border);
        let pos = pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0);
        draw_galley(ui, pos, g, fg);
        resp
    }
}

/// 状态标签：颜色与底色成对取，调用点不用自己配。
pub fn status_tag<'a>(t: &'a Tokens, d: Density, text: &'a str, s: Status) -> impl Widget + 'a {
    move |ui: &mut Ui| {
        let (fg, bg) = t.status(s);
        let pad = d.px(7.0);
        let g = lay(ui, text, Font::mono_micro(d));
        let h = d.px(18.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + pad * 2.0, h), Sense::hover());
        if ui.is_rect_visible(rect) {
            fill_rect(ui, rect, radius::SM, bg, None);
            let pos = pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0);
            draw_galley(ui, pos, g, fg);
        }
        resp
    }
}

// ---------------------------------------------------------------- 导航项

pub fn nav_item<'a>(
    t: &'a Tokens,
    d: Density,
    label: &'a str,
    badge: &'a str,
    active: bool,
) -> impl Widget + 'a {
    move |ui: &mut Ui| {
        let h = d.px(32.0);
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let (bg, fg) = if active {
            (t.accent_bg, t.accent)
        } else if resp.hovered() {
            (t.bg_hover, t.text_secondary)
        } else {
            (Color32::TRANSPARENT, t.text_secondary)
        };
        fill_rect(ui, rect, radius::MD, bg, None);

        let pad = d.px(10.0);
        let g = lay(ui, label, if active { Font::strong(d) } else { Font::label(d) });
        let pos = pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0);
        draw_galley(ui, pos, g, fg);

        let b = lay(ui, badge, Font::mono_micro(d));
        let bpos = pos2(rect.right() - pad - b.size().x, rect.center().y - b.size().y / 2.0);
        draw_galley(ui, bpos, b, t.text_muted);

        resp
    }
}

// ---------------------------------------------------------------- 页签

pub fn tab<'a>(
    t: &'a Tokens,
    d: Density,
    label: &'a str,
    icon: Option<&'a str>,
    active: bool,
) -> impl Widget + 'a {
    move |ui: &mut Ui| {
        let pad = d.px(12.0);
        let gap = d.px(8.0);
        let g = lay(ui, label, if active { Font::strong(d) } else { Font::label(d) });
        let ig = icon.map(|i| lay(ui, i, icon_font(d)));
        let mut w = pad * 2.0 + g.size().x;
        if let Some(x) = &ig {
            w += x.size().x + gap;
        }
        let (rect, resp) = ui.allocate_exact_size(vec2(w, d.tab_h()), Sense::click());
        if !ui.is_rect_visible(rect) {
            return resp;
        }

        let (bg, fg) = if active {
            (t.bg_panel, t.text_primary)
        } else if resp.hovered() {
            (t.bg_hover, t.text_secondary)
        } else {
            (t.bg_header, t.text_secondary)
        };
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
        if active {
            // 顶部 2px 强调条，替代整块高亮，避免页签抢过内容。
            let bar = Rect::from_min_size(rect.left_top(), vec2(rect.width(), d.px(2.0)));
            ui.painter().rect_filled(bar, CornerRadius::ZERO, t.accent);
        }

        let mut x = rect.left() + pad;
        if let Some(i) = ig {
            let y = rect.center().y - i.size().y / 2.0;
            let sx = i.size().x;
            draw_galley(ui, pos2(x, y), i, if active { t.accent } else { t.text_muted });
            x += sx + gap;
        }
        let y = rect.center().y - g.size().y / 2.0;
        draw_galley(ui, pos2(x, y), g, fg);
        resp
    }
}

// ---------------------------------------------------------------- 工具按钮

pub fn tool_btn<'a>(t: &'a Tokens, d: Density, icon: &'a str, active: bool) -> impl Widget + 'a {
    move |ui: &mut Ui| {
        let s = d.px(30.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(s, s), Sense::click());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let (bg, fg) = if active {
            (t.accent_bg, t.accent)
        } else if resp.hovered() {
            (t.bg_hover, t.text_primary)
        } else {
            (Color32::TRANSPARENT, t.text_secondary)
        };
        fill_rect(ui, rect, radius::MD, bg, None);
        let g = lay(ui, icon, FontId::new(d.px(16.0), FontFamily::Proportional));
        let pos = rect.center() - g.size() / 2.0;
        draw_galley(ui, pos, g, fg);
        resp
    }
}

// ---------------------------------------------------------------- 开关

pub fn toggle<'a>(t: &'a Tokens, d: Density, on: &'a mut bool) -> impl Widget + 'a {
    move |ui: &mut Ui| {
        let w = d.px(36.0);
        let h = d.px(20.0);
        let (rect, mut resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
        if resp.clicked() {
            *on = !*on;
            resp.mark_changed();
        }
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let (bg, border) = if *on {
            (t.accent, None)
        } else {
            (t.bg_input, Some(t.border_strong))
        };
        fill_rect(ui, rect, (h / 2.0) as u8, bg, border);
        let knob_r = h / 2.0 - d.px(3.0);
        let cx = if *on {
            rect.right() - d.px(3.0) - knob_r
        } else {
            rect.left() + d.px(3.0) + knob_r
        };
        let knob = if *on { t.accent_ink } else { t.text_muted };
        ui.painter()
            .circle_filled(pos2(cx, rect.center().y), knob_r, knob);
        resp
    }
}

// ---------------------------------------------------------------- 分段控件

pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;
    let pad = 2.0;
    let inner_h = d.px(24.0);
    let frame = egui::Frame::new()
        .fill(t.bg_input)
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(egui::Margin::same(pad as i8));
    frame.show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = pad;
        // 显式指定从左到右：这个控件常被放进 right_to_left 的行里，不指定就会倒序。
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            for (value, label) in options {
                let on = *value == *current;
                let g = lay(ui, label, if on { Font::strong(d) } else { Font::meta(d) });
                let w = g.size().x + d.px(14.0) * 2.0;
                let (rect, resp) = ui.allocate_exact_size(vec2(w, inner_h), Sense::click());
                if ui.is_rect_visible(rect) {
                    let (bg, fg) = if on {
                        (t.accent, t.accent_ink)
                    } else if resp.hovered() {
                        (t.bg_hover, t.text_primary)
                    } else {
                        (Color32::TRANSPARENT, t.text_secondary)
                    };
                    fill_rect(ui, rect, radius::SM, bg, None);
                    let pos = rect.center() - g.size() / 2.0;
                    draw_galley(ui, pos, g, fg);
                }
                if resp.clicked() {
                    *current = *value;
                    changed = true;
                }
            }
        });
    });
    changed
}

// ---------------------------------------------------------------- 行

/// 行首图标。模型树的类型图标是 PDMS 导出的图片纹理而不是字形，只支持字形的话
/// 这些树根本用不了组件层。
pub enum RowIcon<'a> {
    None,
    Glyph(&'a str),
    Texture(egui::TextureId),
}

/// 行尾标签。`icon` 与 `text` 必须分开绘制：图标字形只并入了比例字体家族，
/// 跟着等宽正文一起排会变成豆腐块。
#[derive(Clone, Copy)]
pub struct RowTag<'a> {
    pub icon: Option<&'a str>,
    pub text: &'a str,
    pub tone: Status,
}

pub struct TreeRow<'a> {
    pub depth: usize,
    pub icon: RowIcon<'a>,
    pub label: &'a str,
    pub meta: &'a str,
    pub selected: bool,
    pub expandable: Option<bool>,
    pub tone: Status,
    pub tags: &'a [RowTag<'a>],
}

pub struct TreeRowOut {
    pub response: Response,
    /// 展开箭头的位置。整行只有一个 Response，调用点靠它把「点箭头折叠」从
    /// 「点整行选中」里区分出来；不可展开的行是 `None`。
    pub caret_rect: Option<Rect>,
}

pub fn tree_row_ui(ui: &mut Ui, t: &Tokens, d: Density, row: TreeRow<'_>) -> TreeRowOut {
    let h = d.row_h();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return TreeRowOut {
            response: resp,
            caret_rect: None,
        };
    }
    let bg = if row.selected {
        t.accent_bg
    } else if resp.hovered() {
        t.bg_hover
    } else {
        Color32::TRANSPARENT
    };
    fill_rect(ui, rect, radius::SM, bg, None);

    let gap = d.px(6.0);
    let mut x = rect.left() + d.px(10.0) + row.depth as f32 * d.px(13.0);
    let mut caret_rect = None;

    if let Some(open) = row.expandable {
        let glyph = if open {
            egui_phosphor::regular::CARET_DOWN
        } else {
            egui_phosphor::regular::CARET_RIGHT
        };
        let g = lay(ui, glyph, FontId::new(d.px(12.0), FontFamily::Proportional));
        let sx = g.size().x;
        draw_galley(
            ui,
            pos2(x, rect.center().y - g.size().y / 2.0),
            g,
            t.text_muted,
        );
        // 命中区按整行高给足，箭头字形本身只有十几像素宽，照字形取会很难点中。
        caret_rect = Some(Rect::from_min_size(
            pos2(x - gap / 2.0, rect.top()),
            vec2(sx + gap, h),
        ));
        x += sx + gap;
    } else {
        x += d.px(12.0) + gap;
    }

    let (tone, _) = t.status(row.tone);
    let icon_sz = d.px(14.0);
    match row.icon {
        RowIcon::None => {}
        RowIcon::Glyph(g) => {
            let ig = lay(ui, g, FontId::new(icon_sz, FontFamily::Proportional));
            let isx = ig.size().x;
            draw_galley(ui, pos2(x, rect.center().y - ig.size().y / 2.0), ig, tone);
            x += isx + gap;
        }
        RowIcon::Texture(id) => {
            let r = Rect::from_min_size(
                pos2(x, rect.center().y - icon_sz / 2.0),
                vec2(icon_sz, icon_sz),
            );
            ui.painter().image(
                id,
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            x += icon_sz + gap;
        }
    }

    // 右侧先量后画：标签与 meta 占多少宽度决定了标题能写到哪，不先量的话
    // 长节点名会直接压在标签底下。
    let mut right = rect.right() - d.px(10.0);
    if !row.meta.is_empty() {
        let mg = lay(ui, row.meta, Font::mono_micro(d));
        right -= mg.size().x;
        draw_galley(
            ui,
            pos2(right, rect.center().y - mg.size().y / 2.0),
            mg,
            t.text_muted,
        );
        right -= gap;
    }
    for tag in row.tags.iter().rev() {
        let pad = d.px(7.0);
        let tg = lay(ui, tag.text, Font::mono_micro(d));
        let ig = tag
            .icon
            .map(|i| lay(ui, i, FontId::new(d.px(12.0), FontFamily::Proportional)));
        let mut w = pad * 2.0 + tg.size().x;
        if let Some(g) = &ig {
            w += g.size().x + d.px(4.0);
        }
        right -= w;
        let th = d.px(18.0);
        let tr = Rect::from_min_size(pos2(right, rect.center().y - th / 2.0), vec2(w, th));
        let (fg, bg) = t.status(tag.tone);
        fill_rect(ui, tr, radius::SM, bg, None);
        let mut tx = tr.left() + pad;
        if let Some(g) = ig {
            let sx = g.size().x;
            draw_galley(ui, pos2(tx, tr.center().y - g.size().y / 2.0), g, fg);
            tx += sx + d.px(4.0);
        }
        draw_galley(ui, pos2(tx, tr.center().y - tg.size().y / 2.0), tg, fg);
        right -= gap;
    }

    let lg = lay(
        ui,
        row.label,
        if row.selected {
            Font::strong(d)
        } else {
            Font::label(d)
        },
    );
    let fg = if row.selected {
        t.accent_strong
    } else {
        t.text_primary
    };
    let label_clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right.max(x), rect.bottom()));
    ui.painter()
        .with_clip_rect(label_clip)
        .galley(pos2(x, rect.center().y - lg.size().y / 2.0), lg, fg);

    TreeRowOut {
        response: resp,
        caret_rect,
    }
}

pub fn prop_row<'a>(
    t: &'a Tokens,
    d: Density,
    key: &'a str,
    value: &'a str,
    mono: bool,
    zebra: bool,
) -> impl Widget + 'a {
    move |ui: &mut Ui| {
        let h = d.row_h();
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        if zebra {
            ui.painter()
                .rect_filled(rect, CornerRadius::ZERO, t.bg_header);
        }
        let pad = d.px(12.0);
        let kg = lay(ui, key, Font::meta(d));
        draw_galley(
            ui,
            pos2(rect.left() + pad, rect.center().y - kg.size().y / 2.0),
            kg,
            t.text_muted,
        );
        let vg = lay(
            ui,
            value,
            if mono { Font::mono(d) } else { Font::label(d) },
        );
        draw_galley(
            ui,
            pos2(
                rect.left() + pad + prop_key_w(d),
                rect.center().y - vg.size().y / 2.0,
            ),
            vg,
            t.text_primary,
        );
        resp
    }
}

/// 键名列宽。只读的 `prop_row` 与可编辑的 `prop_row_edit` 必须共用它，
/// 两种行混排时值列才在同一条竖线上。
fn prop_key_w(d: Density) -> f32 {
    d.px(88.0)
}

/// 属性行里值列相对行左边缘的偏移。分组标题栏的附加内容要按它摆，
/// 否则标题右半区和下方的值列会错开一截。
pub fn prop_value_x(d: Density) -> f32 {
    d.px(12.0) + prop_key_w(d)
}

/// 可编辑属性行。
///
/// `prop_row` 是绘制型只读行，而属性面板的值是 reflect 驱动出来的任意可编辑控件
/// （数值拖拽、下拉、勾选……），组件层穷举不了，硬换成只读行会把编辑能力弄丢。
/// 所以这里给的是容器：铺底、键名、列宽由组件负责，值区交回调用点自己画。
pub fn prop_row_edit<R>(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    key: &str,
    depth: usize,
    zebra: bool,
    value_ui: impl FnOnce(&mut Ui) -> R,
) -> R {
    let h = d.row_h();
    let pad = d.px(12.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());

    if zebra {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, t.bg_header);
    }
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, t.bg_hover);
    }
    // 只有键名跟着层级缩进，值列固定不动——值列一旦跟着缩进，同一屏里的数字就
    // 对不成一列了。
    let kg = lay(ui, key, Font::meta(d));
    draw_galley(
        ui,
        pos2(
            rect.left() + pad + depth as f32 * d.px(16.0),
            rect.center().y - kg.size().y / 2.0,
        ),
        kg,
        t.text_muted,
    );

    let value_rect = Rect::from_min_max(
        pos2(rect.left() + pad + prop_key_w(d), rect.top()),
        rect.right_bottom(),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(value_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    value_ui(&mut child)
}

pub fn log_row<'a>(
    t: &'a Tokens,
    d: Density,
    time: &'a str,
    level: Status,
    level_text: &'a str,
    message: &'a str,
) -> impl Widget + 'a {
    move |ui: &mut Ui| {
        let h = d.px(24.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
        if !ui.is_rect_visible(rect) {
            return resp;
        }
        let (fg, bg) = t.status(level);
        if level == Status::Error {
            ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
        }

        let pad = d.px(14.0);
        let gap = d.px(12.0);
        let mut x = rect.left() + pad;

        let tg = lay(ui, time, Font::mono_meta(d));
        let tsx = tg.size().x;
        draw_galley(
            ui,
            pos2(x, rect.center().y - tg.size().y / 2.0),
            tg,
            t.text_muted,
        );
        x += tsx + gap;

        let tag_w = d.px(52.0);
        let tag = Rect::from_min_size(
            pos2(x, rect.center().y - d.px(16.0) / 2.0),
            vec2(tag_w, d.px(16.0)),
        );
        fill_rect(ui, tag, radius::SM, bg, None);
        let lg = lay(ui, level_text, Font::mono_micro(d));
        draw_galley(ui, tag.center() - lg.size() / 2.0, lg, fg);
        x += tag_w + gap;

        let color = if level == Status::Info {
            t.text_secondary
        } else {
            fg
        };
        let mg = lay(ui, message, Font::meta(d));
        draw_galley(ui, pos2(x, rect.center().y - mg.size().y / 2.0), mg, color);
        resp
    }
}

// ---------------------------------------------------------------- 容器

pub fn section_header(ui: &mut Ui, t: &Tokens, d: Density, title: &str, note: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(Font::strong(d))
                .color(t.text_muted),
        );
        if !note.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(note)
                        .font(Font::mono_meta(d))
                        .color(t.text_muted),
                );
            });
        }
    });
    ui.add_space(space::S1);
}

pub fn swatch(ui: &mut Ui, t: &Tokens, d: Density, name: &str, color: Color32) {
    let h = d.px(30.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(d.px(46.0), h), Sense::hover());
        fill_rect(ui, rect, radius::SM, color, Some(t.border));
        ui.label(
            egui::RichText::new(name)
                .font(Font::meta(d))
                .color(t.text_secondary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "#{:02X}{:02X}{:02X}",
                    color.r(),
                    color.g(),
                    color.b()
                ))
                .font(Font::mono_micro(d))
                .color(t.text_muted),
            );
        });
    });
}
