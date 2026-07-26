//! 主工作台 S1：外壳（标题栏 / 命令栏 / 状态栏）+ 四个 dock 常驻视图。
//!
//! 纯绘制：输入 &WorkbenchVm 与外壳状态，输出 Vec<Cmd>；
//! 不认识数据库、不认识 Bevy。三维视口对本层只是一个 TextureId（M1-5 前用占位）。

pub mod chrome;
pub mod panes;
pub mod props;
pub mod tree;

pub use panes::Pane;

use egui_dock::{DockArea, DockState, NodeIndex};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::style::tokens::{Density, Tokens};
use crate::vm::WorkbenchVm;

/// PDMS 类型 -> 图标。模型树行首与属性面板头共用一套，同一个元素在两处
/// 不能是两个形状。挑常见层级给可辨识的形状，其余统一立方体。
pub fn noun_icon(noun: &str) -> &'static str {
    match noun {
        "SITE" => ph::FACTORY,
        "ZONE" => ph::BOUNDING_BOX,
        "PIPE" => ph::PIPE,
        "BRAN" => ph::GIT_BRANCH,
        "EQUI" => ph::ENGINE,
        "NOZZ" => ph::PLUGS_CONNECTED,
        "STRU" | "FRMW" => ph::WALL,
        "HANG" | "SUPPO" => ph::ANCHOR,
        _ => ph::CUBE,
    }
}

/// 外壳持久状态（dock 布局）。属于界面本身而非数据，所以不进 Vm。
pub struct WorkbenchState {
    dock: DockState<Pane>,
}

impl Default for WorkbenchState {
    fn default() -> Self {
        // S1 布局：中央三维视图，左 19% 模型树，右侧属性，中下命令行。
        let mut dock = DockState::new(vec![Pane::View3d]);
        {
            let surface = dock.main_surface_mut();
            let [center, _left] =
                surface.split_left(NodeIndex::root(), 0.19, vec![Pane::ModelTree]);
            let [center, _right] = surface.split_right(center, 0.76, vec![Pane::Properties]);
            let [_center, _bottom] = surface.split_below(center, 0.7, vec![Pane::Console]);
        }
        Self { dock }
    }
}

/// 画一帧主工作台。用嵌套面板而不是手算高度：dock 会把分配给它的空间吃满，
/// 手算的话状态栏会被挤没。
pub fn show(
    ui: &mut egui::Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    state: &mut WorkbenchState,
) -> Vec<Cmd> {
    let mut cmds = Vec::new();

    // 外壳高度都 +1，给栏边的 1px hairline 留位。
    egui::Panel::top("wb-title")
        .exact_size(d.title_bar_h() + 1.0)
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show(ui, |ui| chrome::title_bar(ui, t, d, vm));
    egui::Panel::top("wb-command")
        .exact_size(d.command_bar_h() + 1.0)
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show(ui, |ui| chrome::command_bar(ui, t, d, vm));
    egui::Panel::bottom("wb-status")
        .exact_size(d.status_bar_h() + 1.0)
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show(ui, |ui| chrome::status_bar(ui, t, d, vm));
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            let style = panes::dock_style(ui, t, d);
            let mut viewer = panes::Viewer {
                t,
                d,
                vm,
                cmds: &mut cmds,
            };
            DockArea::new(&mut state.dock)
                .style(style)
                .show_close_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_leaf_collapse_buttons(false)
                .show_inside(ui, &mut viewer);
        });

    cmds
}
