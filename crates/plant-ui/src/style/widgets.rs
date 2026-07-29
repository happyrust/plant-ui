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
    loading: bool,
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

    /// 保持按钮尺寸不变，并用旋转加载动画替换文字。
    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
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
        loading: false,
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
        let (bg, fg, border) = if !ui.is_enabled() {
            (t.bg_input, t.text_muted, Some(t.border))
        } else if self.primary {
            let bg = if resp.hovered() {
                t.accent_strong
            } else {
                t.accent
            };
            (bg, t.accent_ink, None)
        } else if resp.hovered() {
            (t.bg_hover, t.text_primary, Some(t.border_strong))
        } else {
            (t.bg_elevated, t.text_primary, Some(t.border))
        };

        fill_rect(ui, rect, radius::MD, bg, border);

        if self.loading {
            let spinner_size = d.px(16.0);
            let spinner_rect =
                Rect::from_center_size(rect.center(), vec2(spinner_size, spinner_size));
            ui.put(
                spinner_rect,
                egui::Spinner::new().size(spinner_size).color(fg),
            );
            ui.ctx().request_repaint();
            return resp;
        }

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
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
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
        let g = lay(
            ui,
            label,
            if active {
                Font::strong(d)
            } else {
                Font::label(d)
            },
        );
        let pos = pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0);
        draw_galley(ui, pos, g, fg);

        let b = lay(ui, badge, Font::mono_micro(d));
        let bpos = pos2(
            rect.right() - pad - b.size().x,
            rect.center().y - b.size().y / 2.0,
        );
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
        let g = lay(
            ui,
            label,
            if active {
                Font::strong(d)
            } else {
                Font::label(d)
            },
        );
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
            draw_galley(
                ui,
                pos2(x, y),
                i,
                if active { t.accent } else { t.text_muted },
            );
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
        // 禁用态必须先判：调用点用 `add_enabled` 表达「这枚现在按不了」，组件不读
        // `is_enabled` 的话画出来和常态一模一样，看着就是一枚点了没反应的按钮。
        // 不加底色也不加描边——它悬在视口工具栏上，给填充反而比可用的那几枚还重。
        let (bg, fg) = if !ui.is_enabled() {
            (Color32::TRANSPARENT, t.text_muted)
        } else if active {
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

/// 行内可见性眼睛的画法。
///
/// 组件层不认识 `vm::RowVisibility`（这里只依赖 egui 与令牌），所以另立一个说法，
/// 由调用点映射。三档而不是开关的理由在 ADR-0010。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Eye {
    /// 从没显示过。槽位照占，但只有整行悬停时才浮出来。
    Unloaded,
    Shown,
    Hidden,
}

pub struct TreeRow<'a> {
    pub depth: usize,
    pub icon: RowIcon<'a>,
    pub label: &'a str,
    pub meta: &'a str,
    pub selected: bool,
    /// 选择集里的**主选中**：属性面板显示的是它。多选时整片都是 accent-bg，
    /// 不额外标一下的话，用户看不出属性栏在说这几行里的哪一行。
    pub primary: bool,
    pub expandable: Option<bool>,
    /// 行内可见性眼睛。`None` = 这一列**压根不存在**，一格宽度都不占——
    /// 没有实时渲染器时按不动，摆着就是假控件。
    pub eye: Option<Eye>,
    pub tone: Status,
    pub tags: &'a [RowTag<'a>],
}

pub struct TreeRowOut {
    pub response: Response,
    /// 展开箭头的位置。整行只有一个 Response，调用点靠它把「点箭头折叠」从
    /// 「点整行选中」里区分出来；不可展开的行是 `None`。
    pub caret_rect: Option<Rect>,
    /// 眼睛的位置，用途同 `caret_rect`：把「点眼睛切可见性」从「点整行选中」
    /// 里分出来。没有这一列时是 `None`。
    pub eye_rect: Option<Rect>,
}

pub fn tree_row_ui(ui: &mut Ui, t: &Tokens, d: Density, row: TreeRow<'_>) -> TreeRowOut {
    let h = d.row_h();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
    if !ui.is_rect_visible(rect) {
        return TreeRowOut {
            response: resp,
            caret_rect: None,
            eye_rect: None,
        };
    }
    let bg = if row.selected {
        t.accent_bg
    } else if resp.hovered() {
        t.bg_hover
    } else {
        Color32::TRANSPARENT
    };
    // 主选中多一圈 1px 描边而不是换底色：底色一换，多选时「选中的一片」就断成
    // 深浅两段，看着像两组而不是一组。
    let border = (row.selected && row.primary).then_some(t.accent);
    fill_rect(ui, rect, radius::SM, bg, border);

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

    // 眼睛的宽度**恒定**参与这轮丈量，未加载时只是不画。让位置跟着悬停走的话，
    // 长 PDMS 名的裁剪点会随鼠标进出而变，鼠标扫过一列就是一列抖动。
    let mut eye_rect = None;
    if let Some(eye) = row.eye {
        let sz = d.px(13.0);
        right -= sz;
        let tint = match eye {
            // 开机时整棵树都是未加载。这一档常驻画出来，满屏都是「默认就是这样」的图标。
            Eye::Unloaded if !resp.hovered() => None,
            Eye::Unloaded => Some(t.text_muted),
            Eye::Shown | Eye::Hidden => Some(t.text_secondary),
        };
        if let Some(tint) = tint {
            let glyph = if eye == Eye::Hidden {
                egui_phosphor::regular::EYE_SLASH
            } else {
                egui_phosphor::regular::EYE
            };
            let g = lay(ui, glyph, FontId::new(sz, FontFamily::Proportional));
            let pos = pos2(
                right + (sz - g.size().x) / 2.0,
                rect.center().y - g.size().y / 2.0,
            );
            draw_galley(ui, pos, g, tint);
        }
        // 命中区同箭头，按整行高给足：13px 的字形本身很难点中。
        eye_rect = Some(Rect::from_min_size(
            pos2(right - gap / 2.0, rect.top()),
            vec2(sz + gap, h),
        ));
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
    // 已隐藏的行整行弱化。一枚 13px 的图标在万行列表里扫不出来，而「哪些被我藏了」
    // 恰恰是开了这一列之后最想一眼看到的事。选中优先于弱化：选中是更急的信号，
    // 而那一行的眼睛仍然是斜杠的，说得清楚。
    let fg = if row.selected {
        t.accent_strong
    } else if row.eye == Some(Eye::Hidden) {
        t.text_muted
    } else {
        t.text_primary
    };
    let label_clip = Rect::from_min_max(pos2(x, rect.top()), pos2(right.max(x), rect.bottom()));
    ui.painter().with_clip_rect(label_clip).galley(
        pos2(x, rect.center().y - lg.size().y / 2.0),
        lg,
        fg,
    );

    TreeRowOut {
        response: resp,
        caret_rect,
        eye_rect,
    }
}

pub struct PropRow<'a> {
    pub key: &'a str,
    pub value: &'a str,
    /// 值列走等宽字体。数值、方位串、refno 靠它上下对齐。
    pub mono: bool,
    /// 偶数行斑马底。
    pub zebra: bool,
    /// unset / 空值弱化，与有值的行一眼可分。
    pub muted: bool,
}

/// 只读属性行。两列各自裁剪：PDMS 的键名和值都可能超长，不裁的话键名会顶穿
/// 值列、值会溢出面板右缘。
pub fn prop_row_ui(ui: &mut Ui, t: &Tokens, d: Density, row: PropRow<'_>) -> Response {
    let h = d.row_h();
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    if row.zebra {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, t.bg_header);
    }
    let pad = d.px(12.0);
    let key_x = rect.left() + pad;
    let value_x = rect.left() + prop_value_x(d);

    let kg = lay(ui, row.key, Font::meta(d));
    let key_clip = Rect::from_min_max(
        pos2(key_x, rect.top()),
        pos2(value_x - d.px(4.0), rect.bottom()),
    );
    ui.painter().with_clip_rect(key_clip).galley(
        pos2(key_x, rect.center().y - kg.size().y / 2.0),
        kg,
        t.text_muted,
    );

    let vg = lay(
        ui,
        row.value,
        if row.mono {
            Font::mono(d)
        } else {
            Font::label(d)
        },
    );
    let value_clip = Rect::from_min_max(
        pos2(value_x, rect.top()),
        pos2(rect.right() - pad, rect.bottom()),
    );
    let fg = if row.muted {
        t.text_muted
    } else {
        t.text_primary
    };
    ui.painter().with_clip_rect(value_clip).galley(
        pos2(value_x, rect.center().y - vg.size().y / 2.0),
        vg,
        fg,
    );
    resp
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
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, t.bg_header);
    }
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, t.bg_hover);
    }
    // 只有键名跟着层级缩进，值列固定不动——值列一旦跟着缩进，同一屏里的数字就
    // 对不成一列了。
    let value_x = rect.left() + pad + prop_key_w(d);
    let kg = lay(ui, key, Font::meta(d));
    // 与只读行同样裁在值列前 4px：长 UDA 名（`:MechanicalRep` 一类）不裁就会顶进
    // 输入框，而输入框有底色，压上去的键名比只读行顶穿值列更难看。
    let key_clip = Rect::from_min_max(
        pos2(rect.left() + pad, rect.top()),
        pos2(value_x - d.px(4.0), rect.bottom()),
    );
    ui.painter().with_clip_rect(key_clip).galley(
        pos2(
            rect.left() + pad + depth as f32 * d.px(16.0),
            rect.center().y - kg.size().y / 2.0,
        ),
        kg,
        t.text_muted,
    );

    let value_rect = Rect::from_min_max(pos2(value_x, rect.top()), rect.right_bottom());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(value_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    value_ui(&mut child)
}

pub struct LogRow<'a> {
    pub time: &'a str,
    pub level: Status,
    pub level_text: &'a str,
    /// 该行指向的元素名。Some 时正文前多一段 accent 色的可点文字。
    pub element: Option<&'a str>,
    pub message: &'a str,
}

pub struct LogRowOut<R> {
    pub response: Response,
    /// 元素名那一段的响应；`LogRow::element` 为 None 时也是 None。
    pub element: Option<Response>,
    /// 正文那一段的响应。正文被截断时，调用点拿它挂完整文本的 tooltip。
    pub message: Response,
    pub trailing: R,
}

/// 日志行（C/LogRow：高 24、时间等宽 `text-muted` + 52pt 分级标签 + 正文；
/// ERROR 行整行 `danger-bg`）。
///
/// 时间 / 分级 / 正文都走 selectable 的 `Label`，不用 painter 直接落字：控制台的
/// 头等需求是把出错的那几行原样抄出去。egui 的跨 Label 选中会把同一行的几段用空格
/// 拼、跨行用换行拼（行距大于半个字高时多一个空行），拖选几行 Ctrl+C 就能贴走——
/// 不必像旧壳那样为了可选中复制而整块退回 `TextEdit`，把分级配色一起弄丢。
/// 想要单行不带空行的话走行尾的「复制」。
///
/// `trailing` 在行尾从右往左画处置入口（重试 / 复制），正文按剩下的宽度截断。
/// `element` 给出时在分级标签与正文之间插一段 accent 色的元素名，调用点拿
/// `LogRowOut::element` 的点击回指该元素（M1-6 的命令行定位）。
pub fn log_row_ui<R>(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    row: LogRow<'_>,
    trailing: impl FnOnce(&mut Ui) -> R,
) -> LogRowOut<R> {
    let h = d.px(24.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::hover());
    let (fg, bg) = t.status(row.level);

    // 行上盖着几个 Label，它们会挡掉整行 Response 的 hover；底色按几何命中判，
    // 不看 `response.hovered()`。
    if row.level == Status::Error {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, bg);
    } else if ui.rect_contains_pointer(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::ZERO, t.bg_hover);
    }

    let pad = d.px(14.0);
    let gap = d.px(12.0);

    // 行尾先摆：正文能写到哪由处置入口占了多少宽度决定，不先摆的话长消息会
    // 直接压在按钮底下。
    let mut tail = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(
                pos2(rect.left() + pad, rect.top()),
                pos2(rect.right() - pad, rect.bottom()),
            ))
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let trailing = trailing(&mut tail);
    let tail_w = tail.min_rect().width();
    let right = rect.right() - pad - if tail_w > 0.0 { tail_w + gap } else { 0.0 };

    let mut x = rect.left() + pad;
    let time_w = lay(ui, row.time, Font::mono_meta(d)).size().x;
    selectable_at(
        ui,
        Rect::from_min_max(pos2(x, rect.top()), pos2(x + time_w, rect.bottom())),
        egui::RichText::new(row.time)
            .font(Font::mono_meta(d))
            .color(t.text_muted),
        false,
    );
    x += time_w + gap;

    let tag_w = d.px(52.0);
    let tag = Rect::from_min_size(
        pos2(x, rect.center().y - d.px(16.0) / 2.0),
        vec2(tag_w, d.px(16.0)),
    );
    fill_rect(ui, tag, radius::SM, bg, None);
    selectable_at(
        ui,
        tag,
        egui::RichText::new(row.level_text)
            .font(Font::mono_micro(d))
            .color(fg),
        false,
    );
    x += tag_w + gap;

    // 元素名：accent 色 + 悬停下划线，指认它是可点的。仍走 selectable 的 Label，
    // 拖选整行时它跟着一起被抄走——egui 的可选中 Label 本身就 sense 点击，
    // 不必为了可点而换成 Link 把这段从选区里挖掉。
    let element = row.element.map(|name| {
        // 长 PDMS 名不许把正文挤没，超过四成行宽就截断。
        let want = lay(ui, name, Font::mono_meta(d)).size().x;
        let w = want.min((right - x) * 0.4).max(0.0);
        let elem_rect = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
        let hovered = ui.rect_contains_pointer(elem_rect);
        let mut text = egui::RichText::new(name)
            .font(Font::mono_meta(d))
            .color(t.accent);
        if hovered {
            text = text.underline();
        }
        let out = selectable_at(ui, elem_rect, text, want > w);
        x += w + gap;
        out
    });

    // INFO 用次级色，让 WARN / ERROR 在一屏里跳出来。
    let color = if row.level == Status::Info {
        t.text_secondary
    } else {
        fg
    };
    let message = selectable_at(
        ui,
        Rect::from_min_max(pos2(x, rect.top()), pos2(right.max(x), rect.bottom())),
        egui::RichText::new(row.message)
            .font(Font::meta(d))
            .color(color),
        true,
    );

    LogRowOut {
        response,
        element,
        message,
        trailing,
    }
}

/// 在指定矩形里左对齐地放一段可选中文本。`truncate` 为真时按矩形宽度截断并加省略号
/// ——整段被选中时 egui 复制的是未截断原文，所以截断不影响抄走完整内容。
fn selectable_at(ui: &mut Ui, rect: Rect, text: egui::RichText, truncate: bool) -> Response {
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            let label = egui::Label::new(text).selectable(true);
            ui.add(if truncate { label.truncate() } else { label })
        },
    )
    .inner
}

// ---------------------------------------------------------------- 面板四态

/// 数据面板「这会儿没有内容可画」的四种由来。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PaneState {
    /// 还没轮到它加载——用户尚未做出让它有内容的动作。文案该是一句指引。
    Uninit,
    /// 查询在途。
    Loading,
    /// 查完了，确实没有数据。
    Empty,
    /// 失败。
    Error,
}

pub struct PaneNote<'a> {
    pub state: PaneState,
    /// 状态图标。`Loading` 忽略它——那一态画的是会转的 spinner，静态的转圈字形
    /// 比不画还糟，它看着就像界面卡死了。
    pub icon: &'a str,
    pub text: &'a str,
    /// 次要说明（错误链、下一步提示），比正文低一号且更弱。
    pub detail: Option<&'a str>,
    /// 给一枚「重试」，返回值即它是否被点。只有调用点还原得出可重做的操作时才该给。
    pub retry: bool,
}

/// 面板四态的居中提示，四个视图共用一份。
///
/// 收敛之前模型树 / 属性 / 命令行 / 三维视图各写了一份 `note`：图标 24 与 28、
/// 顶部间距 40 与 28 与垂直居中、三处只有一行字而三维视图是标题加副文——同一个
/// 应用里四块面板的「没东西可看」长四个样。
///
/// 设计稿没有对应画板可抄：`DESIGN-SYSTEM.md` 把「S8 状态词汇」列进了画板表，但
/// .pen 里 S5 往后一张都没画（原仓库那份也一样）。所以分寸是按设计令牌定的——
/// 前三态说的都是「还没有内容」，共用 text-muted 图标加 text-secondary 文字，靠
/// 图标与措辞区分；只有错误态换 danger 图标并给出处置入口，出了事才值得跳出来。
pub fn pane_note(ui: &mut Ui, t: &Tokens, d: Density, note: PaneNote<'_>) -> bool {
    let mut retried = false;
    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        let icon_h = d.px(26.0);
        // 先量出这一块多高，才好把它摆在正中。面板高度从命令行那样的窄条到整根
        // 属性栏都有，固定顶距会在矮面板上贴边、在高面板上浮到上三分之一。
        let mut block = icon_h + space::S2 + d.meta();
        if note.detail.is_some() {
            block += space::S1 + d.micro();
        }
        if note.retry {
            block += space::S2 + d.px(22.0);
        }
        ui.add_space(((ui.available_height() - block) / 2.0).max(d.px(20.0)));
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = space::S2;
            let icon_fg = if note.state == PaneState::Error {
                t.danger
            } else {
                t.text_muted
            };
            if note.state == PaneState::Loading {
                ui.add(egui::Spinner::new().size(icon_h).color(icon_fg));
            } else {
                ui.label(
                    egui::RichText::new(note.icon)
                        .font(FontId::new(icon_h, FontFamily::Proportional))
                        .color(icon_fg),
                );
            }
            ui.label(
                egui::RichText::new(note.text)
                    .font(Font::meta(d))
                    .color(t.text_secondary),
            );
            if let Some(detail) = note.detail {
                ui.spacing_mut().item_spacing.y = space::S1;
                ui.label(
                    egui::RichText::new(detail)
                        .font(Font::mono_micro(d))
                        .color(t.text_muted),
                );
            }
            if note.retry {
                ui.spacing_mut().item_spacing.y = space::S2;
                retried = ui.add(chip(t, d, "重试", false)).clicked();
            }
        });
    });
    retried
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
