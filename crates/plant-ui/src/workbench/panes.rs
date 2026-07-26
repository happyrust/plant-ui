//! dock 视图：四个常驻视图的枚举、egui_dock 样式与页签内容分发。
//!
//! 决定 4：dock 只有模型树 / 三维视图 / 命令行 / 属性四个常驻视图，
//! 一次性任务走任务窗（M4），不再和常驻视图挤同一排页签。
//!
//! 已知与设计稿的偏差：egui_dock 的 Style 表达不了「激活页签顶部 2px 强调条」，
//! 这里用「激活页签底色与面板底色合并」表达；自绘页签是后续单独的增强项。

use egui::{Color32, CornerRadius, Margin, Stroke, Ui};
use egui_dock::{Style as DockStyle, TabViewer};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::style::tokens::{Density, Tokens};
use crate::vm::WorkbenchVm;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    ModelTree,
    View3d,
    Console,
    Properties,
}

impl Pane {
    pub fn title(self) -> (&'static str, &'static str) {
        match self {
            Pane::ModelTree => (ph::TREE_STRUCTURE, "模型树"),
            Pane::View3d => (ph::CUBE, "三维视图"),
            Pane::Console => (ph::TERMINAL_WINDOW, "命令行"),
            Pane::Properties => (ph::SLIDERS_HORIZONTAL, "属性"),
        }
    }
}

/// 页签栏 32、激活页签 = 面板底色、未激活 = 页眉底色 + 次级文字（C/Tab 规格）。
pub fn dock_style(ui: &Ui, t: &Tokens, d: Density) -> DockStyle {
    let mut s = DockStyle::from_egui(ui.style());
    s.dock_area_padding = None;
    s.main_surface_border_stroke = Stroke::NONE;
    s.main_surface_border_rounding = CornerRadius::ZERO;

    s.tab_bar.bg_fill = t.bg_header;
    s.tab_bar.height = d.tab_h();
    s.tab_bar.hline_color = t.border;
    s.tab_bar.corner_radius = CornerRadius::ZERO;
    s.tab_bar.inner_margin = Margin::ZERO;

    s.tab.hline_below_active_tab_name = false;
    s.tab.spacing = 0.0;
    s.tab.tab_body.bg_fill = t.bg_panel;
    s.tab.tab_body.stroke = Stroke::NONE;
    s.tab.tab_body.inner_margin = Margin::ZERO;
    s.tab.tab_body.corner_radius = CornerRadius::ZERO;

    for style in [
        &mut s.tab.active,
        &mut s.tab.focused,
        &mut s.tab.active_with_kb_focus,
        &mut s.tab.focused_with_kb_focus,
    ] {
        style.bg_fill = t.bg_panel;
        style.text_color = t.text_primary;
        style.outline_color = Color32::TRANSPARENT;
        style.corner_radius = CornerRadius::ZERO;
    }
    for style in [&mut s.tab.inactive, &mut s.tab.inactive_with_kb_focus] {
        style.bg_fill = t.bg_header;
        style.text_color = t.text_secondary;
        style.outline_color = Color32::TRANSPARENT;
        style.corner_radius = CornerRadius::ZERO;
    }
    s.tab.hovered.bg_fill = t.bg_hover;
    s.tab.hovered.text_color = t.text_primary;
    s.tab.hovered.outline_color = Color32::TRANSPARENT;
    s.tab.hovered.corner_radius = CornerRadius::ZERO;

    s.separator.width = 1.0;
    s.separator.color_idle = t.border;
    s.separator.color_hovered = t.accent;
    s.separator.color_dragged = t.accent_strong;

    s.overlay.selection_color = t.accent.linear_multiply(0.35);
    s
}

pub struct Viewer<'a> {
    pub t: &'a Tokens,
    pub d: Density,
    pub vm: &'a WorkbenchVm,
    pub cmds: &'a mut Vec<Cmd>,
}

impl TabViewer for Viewer<'_> {
    type Tab = Pane;

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        let (icon, label) = tab.title();
        format!("{icon}  {label}").into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            Pane::ModelTree => super::tree::show(ui, self.t, self.d, self.vm, self.cmds),
            Pane::Properties => super::props::show(ui, self.t, self.d, self.vm, self.cmds),
            Pane::Console => super::console::show(ui, self.t, self.d, self.vm, self.cmds),
            Pane::View3d => super::view3d::show(ui, self.t, self.d, self.vm),
        }
    }

    fn closeable(&mut self, _tab: &mut Self::Tab) -> bool {
        false
    }
}
