//! 主工作台 S1：外壳（标题栏 / 命令栏 / 状态栏）+ 四个 dock 常驻视图。
//!
//! 纯绘制：输入 &WorkbenchVm 与外壳状态，输出 Vec<Cmd>；
//! 不认识数据库、不认识 Bevy。三维视口对本层只是一个 TextureId（M1-5 前用占位）。

pub mod chrome;
pub mod command_line;
pub mod logs;
pub mod panes;
pub mod props;
pub mod room;
pub mod search;
pub mod tree;
pub mod view3d;
pub mod viewcube;

pub use panes::Pane;

/// 工作台四周可收起的 dock 分区。中央三维视图恒在，不在此列。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockSide {
    Left,
    Bottom,
    Right,
}

/// 三侧 dock 的收 / 展状态。属于界面本身而非数据，所以不进 Vm。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DockVisibility {
    pub left: bool,
    pub bottom: bool,
    pub right: bool,
}

impl Default for DockVisibility {
    fn default() -> Self {
        Self {
            left: true,
            bottom: true,
            right: true,
        }
    }
}

impl DockVisibility {
    fn get(self, side: DockSide) -> bool {
        match side {
            DockSide::Left => self.left,
            DockSide::Bottom => self.bottom,
            DockSide::Right => self.right,
        }
    }

    fn set(&mut self, side: DockSide, on: bool) {
        match side {
            DockSide::Left => self.left = on,
            DockSide::Bottom => self.bottom = on,
            DockSide::Right => self.right = on,
        }
    }
}

/// 常驻视图归在哪一侧 dock；三维视图在中央，不归任何一侧。
fn dock_side_of(pane: Pane) -> Option<DockSide> {
    match pane {
        Pane::ModelTree => Some(DockSide::Left),
        Pane::Properties | Pane::Room => Some(DockSide::Right),
        Pane::CommandLine | Pane::Logs | Pane::TaskQueue => Some(DockSide::Bottom),
        Pane::View3d => None,
    }
}

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
    vis: DockVisibility,
    command: command_line::State,
    queue: crate::task_queue::State,
    search: search::State,
}

impl Default for WorkbenchState {
    fn default() -> Self {
        let vis = DockVisibility::default();
        Self {
            dock: build_dock(vis),
            vis,
            command: command_line::State::default(),
            queue: crate::task_queue::State::default(),
            search: search::State::default(),
        }
    }
}

/// 按三侧可见性搭出 dock 布局。全可见时与设计稿一致：中央三维视图，左 19% 模型树，
/// 右侧属性 / 房间，底部命令行 40% + 日志 / 任务队列 60%（画板 S12 的页签分组）。
///
/// 收起某侧就是跳过那一刀，其余各刀比例不变——**收 / 展一次会回到这套默认比例，
/// 不保留用户手动拖过的分隔条**：egui_dock 0.20 没有「藏起整棵子树再原样放回」的
/// 原语，硬存硬恢复比重建更容易在层级里出错，这里选了可预期的重建。
fn build_dock(vis: DockVisibility) -> DockState<Pane> {
    let mut dock = DockState::new(vec![Pane::View3d]);
    {
        let surface = dock.main_surface_mut();
        let mut center = NodeIndex::root();
        if vis.left {
            let [c, _left] = surface.split_left(center, 0.19, vec![Pane::ModelTree]);
            center = c;
        }
        if vis.right {
            // 房间与属性同一格页签（S13-B 的右侧面板分组）。
            let [c, _right] =
                surface.split_right(center, 0.76, vec![Pane::Properties, Pane::Room]);
            center = c;
        }
        if vis.bottom {
            // 任务队列与日志同一排页签：两者都是「余光扫一眼」的监控面，而命令行要
            // 和它们同时看得见，所以不并进同一格。
            let [_center, bottom] = surface.split_below(center, 0.7, vec![Pane::CommandLine]);
            let [_command, _logs] =
                surface.split_right(bottom, 0.4, vec![Pane::Logs, Pane::TaskQueue]);
        }
    }
    dock
}

impl WorkbenchState {
    /// 把某个常驻视图切到前台。
    ///
    /// 「确认执行」按下之后要能自己跳到任务队列——按完一个不可撤销的按钮还得自己
    /// 去找页签，等于没说清进度在哪看（ADR-0011）。找不到那个视图就什么也不做：
    /// 用户可以把页签拖走，那是他的布局。
    pub fn focus(&mut self, pane: Pane) {
        // 目标所在的 dock 被收起时先展开它：否则「确认后跳到任务队列」这类跳转
        // 在收起态下会静默落空（find_tab 找不到那一页）。
        if let Some(side) = dock_side_of(pane) {
            if !self.vis.get(side) {
                self.set_dock_visible(side, true);
            }
        }
        if let Some(path) = self.dock.find_tab(&pane) {
            let _ = self.dock.set_active_tab(path);
        }
    }

    /// 收 / 展某一侧 dock。dock 布局属于界面自身，宿主收到 `Cmd::ToggleDock` 后
    /// 转手到这里（与 `focus` 同一路）。
    pub fn toggle_dock(&mut self, side: DockSide) {
        self.set_dock_visible(side, !self.vis.get(side));
    }

    fn set_dock_visible(&mut self, side: DockSide, on: bool) {
        if self.vis.get(side) == on {
            return;
        }
        self.vis.set(side, on);
        self.dock = build_dock(self.vis);
    }

    /// 复活死信的请求没发出去。成功那一半队列面板自己按快照收口，只有失败要宿主
    /// 说一声——否则那枚按钮会一直灰着等一个永远不会来的变化。
    pub fn queue_retry_failed(&mut self, root_refno: &str) {
        self.queue.retry_failed(root_refno);
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
    let vis = state.vis;

    // 外壳高度都 +1，给栏边的 1px hairline 留位。
    egui::Panel::top("wb-title")
        .exact_size(d.title_bar_h() + 1.0)
        .frame(egui::Frame::NONE)
        .show_separator_line(false)
        .show(ui, |ui| {
            chrome::title_bar(ui, t, d, vm, vis, &mut state.search, &mut cmds)
        });
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
                vis: _,
                command,
                queue: queue_state,
                search: _,
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
