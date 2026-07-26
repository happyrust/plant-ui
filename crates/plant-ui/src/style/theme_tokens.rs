//! 基于设计令牌的 egui 样式装配。
//!
//! 这是全局唯一装配 `Visuals` 的地方。业务代码不要再用 `ui.style_mut()` 做局部覆盖——
//! 现在散落各处的覆盖正是视觉不一致的来源，改到这里来。
//!
//! 设计源文件：`design/rs-plant3d-ui.pen`，落地说明：`design/DESIGN-SYSTEM.md`。

use std::sync::atomic::{AtomicBool, Ordering};

use egui::style::{ScrollStyle, Selection, WidgetVisuals, Widgets};
use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Margin, Stroke, Style, TextStyle, Vec2,
    Visuals, epaint::Shadow,
};

use crate::style::tokens::{Density, Tokens, radius};

/// 字重通过独立的字体家族表达；egui 没有字重轴。
pub const FAMILY_MEDIUM: &str = "puhui-medium";
pub const FAMILY_SEMIBOLD: &str = "puhui-semibold";

pub fn medium() -> FontFamily {
    FontFamily::Name(FAMILY_MEDIUM.into())
}

pub fn semibold() -> FontFamily {
    FontFamily::Name(FAMILY_SEMIBOLD.into())
}

/// 最近一次装配的主题。
///
/// 大量调用点（`RSStyle::default()`、各种 `set_*_style`）拿不到 `Context`，
/// 却仍要取色。让它们读这里，才能和全局装配保持一致，而不是各自写死一份。
static DARK: AtomicBool = AtomicBool::new(true);

/// 字重家族是否已经注册进 egui。
///
/// egui 对未绑定字体的命名家族是**直接 panic**，而界面在 Bevy 资源就绪之前就已经
/// 开始绘制——`setup_egui_fonts` 挂在资源加载完成上，比首帧晚。所以取字重字体前
/// 必须先问一句，没就绪就退回常规字重，宁可少一层字重层次，也不能崩。
static WEIGHTS_READY: AtomicBool = AtomicBool::new(false);

/// 由字体装配完成的那一刻调用，见 `egui_loader::egui_fonts::setup_egui_fonts`。
pub fn set_weight_families_ready(ready: bool) {
    WEIGHTS_READY.store(ready, Ordering::Relaxed);
}

fn weighted(size: f32, family: FontFamily) -> FontId {
    if WEIGHTS_READY.load(Ordering::Relaxed) {
        FontId::new(size, family)
    } else {
        FontId::new(size, FontFamily::Proportional)
    }
}

/// 当前生效的令牌。没有 `Context` 的调用点用它取色。
pub fn current() -> Tokens {
    Tokens::of(DARK.load(Ordering::Relaxed))
}

pub fn apply(ctx: &Context, t: &Tokens, d: Density) {
    DARK.store(t.dark, Ordering::Relaxed);
    let mut style = Style::default();
    style.visuals = visuals(t);
    style.text_styles = text_styles(d);
    style.spacing.item_spacing = Vec2::new(d.px(8.0), d.px(6.0));
    style.spacing.button_padding = Vec2::new(d.px(10.0), d.px(4.0));
    style.spacing.menu_margin = Margin::same(d.px(6.0) as i8);
    style.spacing.indent = d.px(14.0);
    style.spacing.interact_size = Vec2::new(d.px(28.0), d.px(24.0));
    style.spacing.scroll = ScrollStyle::thin();
    style.animation_time = 0.08;
    // egui 0.32+ 默认跟随系统主题，存在 dark/light 两个样式槽；全部覆盖，
    // 避免系统亮色主题时 egui 自身部件与令牌配色错位。
    ctx.all_styles_mut(|s| *s = style.clone());
}

fn widget(bg: Color32, weak_bg: Color32, stroke: Color32, fg: Color32, r: u8) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: weak_bg,
        bg_stroke: Stroke::new(1.0_f32, stroke),
        fg_stroke: Stroke::new(1.0_f32, fg),
        corner_radius: CornerRadius::same(r),
        expansion: 0.0,
    }
}

fn visuals(t: &Tokens) -> Visuals {
    let mut v = if t.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };

    v.dark_mode = t.dark;
    v.panel_fill = t.bg_panel;
    v.window_fill = t.bg_elevated;
    v.faint_bg_color = t.bg_header;
    v.extreme_bg_color = t.bg_input;
    v.code_bg_color = t.bg_input;

    v.window_stroke = Stroke::new(1.0_f32, t.border_strong);
    v.window_corner_radius = CornerRadius::same(radius::LG);
    v.menu_corner_radius = CornerRadius::same(radius::MD);

    v.override_text_color = None;
    v.warn_fg_color = t.warn;
    v.error_fg_color = t.danger;
    v.hyperlink_color = t.accent;

    v.selection = Selection {
        bg_fill: t.accent_bg,
        stroke: Stroke::new(1.0_f32, t.accent_strong),
    };

    v.widgets = Widgets {
        noninteractive: widget(
            t.bg_panel,
            t.bg_panel,
            t.border,
            t.text_secondary,
            radius::MD,
        ),
        inactive: widget(
            t.bg_elevated,
            t.bg_elevated,
            t.border,
            t.text_primary,
            radius::MD,
        ),
        hovered: widget(
            t.bg_hover,
            t.bg_hover,
            t.border_strong,
            t.text_primary,
            radius::MD,
        ),
        active: widget(
            t.accent_bg,
            t.accent_bg,
            t.accent,
            t.accent_strong,
            radius::MD,
        ),
        open: widget(
            t.bg_header,
            t.bg_header,
            t.border_strong,
            t.text_primary,
            radius::MD,
        ),
    };

    let shadow_color = if t.dark {
        Color32::from_black_alpha(140)
    } else {
        Color32::from_black_alpha(36)
    };
    v.window_shadow = Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 0,
        color: shadow_color,
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 16,
        spread: 0,
        color: shadow_color,
    };

    v
}

pub fn text_styles(d: Density) -> std::collections::BTreeMap<TextStyle, FontId> {
    use FontFamily::{Monospace, Proportional};
    let mut m = std::collections::BTreeMap::new();
    m.insert(TextStyle::Small, FontId::new(d.meta(), Proportional));
    m.insert(TextStyle::Body, FontId::new(d.label_size(), Proportional));
    m.insert(TextStyle::Button, FontId::new(d.label_size(), Proportional));
    m.insert(TextStyle::Heading, FontId::new(d.title(), Proportional));
    m.insert(TextStyle::Monospace, FontId::new(d.body(), Monospace));
    m
}

/// 常用字体规格，避免调用点到处手写 FontId。
pub struct Font;

impl Font {
    pub fn micro(d: Density) -> FontId {
        FontId::new(d.micro(), FontFamily::Proportional)
    }
    pub fn meta(d: Density) -> FontId {
        FontId::new(d.meta(), FontFamily::Proportional)
    }
    pub fn meta_strong(d: Density) -> FontId {
        weighted(d.meta(), semibold())
    }
    pub fn body(d: Density) -> FontId {
        FontId::new(d.body(), FontFamily::Proportional)
    }
    pub fn label(d: Density) -> FontId {
        FontId::new(d.label_size(), FontFamily::Proportional)
    }
    pub fn title(d: Density) -> FontId {
        weighted(d.title(), semibold())
    }
    pub fn page(d: Density) -> FontId {
        weighted(d.page(), semibold())
    }
    pub fn strong(d: Density) -> FontId {
        weighted(d.label_size(), medium())
    }
    pub fn mono_micro(d: Density) -> FontId {
        FontId::new(d.micro(), FontFamily::Monospace)
    }
    pub fn mono_meta(d: Density) -> FontId {
        FontId::new(d.meta(), FontFamily::Monospace)
    }
    pub fn mono(d: Density) -> FontId {
        FontId::new(d.body(), FontFamily::Monospace)
    }
}
