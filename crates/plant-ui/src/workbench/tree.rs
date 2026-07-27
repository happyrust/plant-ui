//! 模型树视图（M1-2）：虚拟化滚动 + 懒加载展开。
//!
//! 万行级靠两层配合：App 侧只在结构变化时展平可见行（Vm 逐帧只读），
//! 这里用 `show_rows` 只画视口内的行；行内文字裁剪由 `tree_row_ui` 保证，
//! 长 PDMS 名不会撑破行。

use egui::{ScrollArea, Ui};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::style::tokens::{Density, Status, Tokens, space};
use crate::style::widgets::{self, PaneNote, PaneState, RowIcon, TreeRow};
use crate::vm::{TreeRowVm, TreeVm, WorkbenchVm};

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    match &vm.tree {
        TreeVm::Loading => {
            note(ui, t, d, PaneState::Loading, ph::SPINNER, "正在连接数据源…");
        }
        // 树上的失败只有一种来源：连接或根层查询没成。重试就是重连，
        // 不必让用户跑去命令行找那一行的「重试」。
        TreeVm::Failed(reason) => {
            if note(ui, t, d, PaneState::Error, ph::WARNING, reason) {
                cmds.push(Cmd::Reconnect);
            }
        }
        TreeVm::Ready(rows) if rows.is_empty() => {
            note(
                ui,
                t,
                d,
                PaneState::Empty,
                ph::TREE_STRUCTURE,
                "当前库下没有 SITE",
            );
        }
        TreeVm::Ready(rows) => {
            egui::Frame::new()
                .fill(t.bg_panel)
                .inner_margin(egui::Margin {
                    left: space::S1 as i8,
                    right: space::S1 as i8,
                    top: space::S1 as i8,
                    bottom: 0,
                })
                .show(ui, |ui| {
                    ui.set_min_size(ui.available_size());
                    // show_rows 是拿**外层** ui 的 item_spacing.y 去折算每行占位的，
                    // 设在闭包里就晚了：行按 26 画、位子却按 26+6 留，滚动条会比
                    // 内容长出近四分之一，能一路滚进底下的空白里。
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let mut area = ScrollArea::vertical().auto_shrink([false, false]);
                    if let Some(y) = reveal_offset(ui, d, rows, vm.tree_reveal) {
                        area = area.vertical_scroll_offset(y);
                    }
                    area.show_rows(ui, d.row_h(), rows.len(), |ui, range| {
                        for row in &rows[range] {
                            let meta = if row.loading {
                                "加载中…"
                            } else {
                                row.noun.as_str()
                            };
                            let out = widgets::tree_row_ui(
                                ui,
                                t,
                                d,
                                TreeRow {
                                    depth: row.depth as usize,
                                    icon: RowIcon::Glyph(super::noun_icon(&row.noun)),
                                    label: &row.name,
                                    meta,
                                    selected: vm.selected == Some(row.refno),
                                    expandable: row.expandable,
                                    tone: Status::Neutral,
                                    tags: &[],
                                },
                            );
                            // 点箭头只折叠 / 展开；点行其他区域选中；双击整行也展开。
                            let on_caret = out.caret_rect.is_some_and(|r| {
                                out.response
                                    .interact_pointer_pos()
                                    .is_some_and(|p| r.contains(p))
                            });
                            if out.response.double_clicked()
                                && row.expandable.is_some()
                                && !on_caret
                            {
                                cmds.push(Cmd::ToggleExpand(row.refno));
                            } else if out.response.clicked() {
                                if on_caret && !out.response.double_clicked() {
                                    cmds.push(Cmd::ToggleExpand(row.refno));
                                } else if !out.response.double_clicked() {
                                    cmds.push(Cmd::SelectElement(row.refno));
                                }
                            }
                        }
                    });
                });
        }
    }
}

/// 命令行定位过来时，把目标行摆到视野正中所需的滚动偏移。
///
/// 一律居中，不做「已经看得见就不动」：`show_rows` 只创建视野内的行，目标行多半
/// 还不存在，`scroll_to_rect` 无从谈起；要判断在不在视野里就得先把上一帧的滚动
/// 位置捞出来，两帧一来一回的复杂度换一次「不跳」不值。定位本来就是「带我过去」，
/// 每次都停在正中反而好预期。
fn reveal_offset(
    ui: &Ui,
    d: Density,
    rows: &[TreeRowVm],
    reveal: Option<aios_core::RefU64>,
) -> Option<f32> {
    let refno = reveal?;
    let idx = rows.iter().position(|r| r.refno == refno)?;
    let row_h = d.row_h();
    let view_h = ui.available_height();
    let max = (rows.len() as f32 * row_h - view_h).max(0.0);
    Some((idx as f32 * row_h + row_h / 2.0 - view_h / 2.0).clamp(0.0, max))
}

/// 树区的四态提示（连接中 / 空库 / 失败）。返回值是错误态那枚「重试」是否被点。
///
/// 树没有「未初始化」态：应用一启动就发连接请求，`TreeVm` 的默认值直接是
/// `Loading`，没有中间那一帧。造一个永远不出现的态就是摆假状态。
fn note(ui: &mut Ui, t: &Tokens, d: Density, state: PaneState, icon: &str, text: &str) -> bool {
    widgets::pane_note(
        ui,
        t,
        d,
        PaneNote {
            state,
            icon,
            text,
            detail: None,
            retry: state == PaneState::Error,
        },
    )
}
