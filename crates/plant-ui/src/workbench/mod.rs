//! 主工作台 S1：外壳（标题栏 / 命令栏 / 状态栏）+ 四个 dock 常驻视图。
//!
//! 纯绘制：输入 &WorkbenchVm 与外壳状态，输出 Vec<Cmd>；
//! 不认识数据库、不认识 Bevy。三维视口对本层只是一个 TextureId（M1-5 前用占位）。

pub mod chrome;
pub mod command_line;
pub mod logs;
pub mod panes;
pub mod props;
pub mod tree;
pub mod view3d;
pub mod viewcube;

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

/// 外壳持久状态（dock 布局 + 各视图自己的绘制状态）。属于界面本身而非数据，
/// 所以不进 Vm。
pub struct WorkbenchState {
    dock: DockState<Pane>,
    command: command_line::State,
    queue: crate::task_queue::State,
}

impl Default for WorkbenchState {
    fn default() -> Self {
        // 中央三维视图，左 19% 模型树，右侧属性，底部命令行 40% + 日志 60%。
        // 任务队列与日志同一排页签：两者都是「余光扫一眼」的监控面，而命令行要
        // 和它们同时看得见，所以不并进同一格（画板 S12 的页签条同此分组）。
        let mut dock = DockState::new(vec![Pane::View3d]);
        {
            let surface = dock.main_surface_mut();
            let [center, _left] =
                surface.split_left(NodeIndex::root(), 0.19, vec![Pane::ModelTree]);
            let [center, _right] = surface.split_right(center, 0.76, vec![Pane::Properties]);
            let [_center, bottom] = surface.split_below(center, 0.7, vec![Pane::CommandLine]);
            let [_command, _logs] =
                surface.split_right(bottom, 0.4, vec![Pane::Logs, Pane::TaskQueue]);
        }
        Self {
            dock,
            command: command_line::State::default(),
            queue: crate::task_queue::State::default(),
        }
    }
}

impl WorkbenchState {
    /// 把某个常驻视图切到前台。
    ///
    /// 「确认执行」按下之后要能自己跳到任务队列——按完一个不可撤销的按钮还得自己
    /// 去找页签，等于没说清进度在哪看（ADR-0011）。找不到那个视图就什么也不做：
    /// 用户可以把页签拖走，那是他的布局。
    pub fn focus(&mut self, pane: Pane) {
        if let Some(path) = self.dock.find_tab(&pane) {
            let _ = self.dock.set_active_tab(path);
        }
    }
}

/// 画一帧主工作台。用嵌套面板而不是手算高度：dock 会把分配给它的空间吃满，
/// 手算的话状态栏会被挤没。
pub fn show(
    ui: &mut egui::Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    queue: &crate::task_queue::Vm,
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
        .show(ui, |ui| chrome::command_bar(ui, t, d, vm, &mut cmds));
    egui::Panel::bottom("wb-status")
        .exact_size(d.status_bar_h() + 1.0)
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show(ui, |ui| chrome::status_bar(ui, t, d, vm));
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |ui| {
            let style = panes::dock_style(ui, t, d);
            let WorkbenchState {
                dock,
                command,
                queue: queue_state,
            } = state;
            let mut viewer = panes::Viewer {
                t,
                d,
                vm,
                queue,
                command,
                queue_state,
                cmds: &mut cmds,
            };
            DockArea::new(dock)
                .style(style)
                .show_close_buttons(false)
                .show_leaf_close_all_buttons(false)
                .show_leaf_collapse_buttons(false)
                .show_inside(ui, &mut viewer);
        });

    cmds
}
