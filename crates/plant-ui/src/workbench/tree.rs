//! 模型树视图（M1-2）：虚拟化滚动 + 懒加载展开。
//!
//! 万行级靠两层配合：App 侧只在结构变化时展平可见行（Vm 逐帧只读），
//! 这里用 `show_rows` 只画视口内的行；行内文字裁剪由 `tree_row_ui` 保证，
//! 长 PDMS 名不会撑破行。

use egui::{RichText, ScrollArea, Ui};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, space};
use crate::style::widgets::{self, RowIcon, TreeRow};
use crate::vm::{TreeVm, WorkbenchVm};

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    match &vm.tree {
        TreeVm::Loading => note(ui, t, d, ph::SPINNER, "正在连接数据源…"),
        TreeVm::Failed(reason) => note(ui, t, d, ph::WARNING, reason),
        TreeVm::Ready(rows) if rows.is_empty() => {
            note(ui, t, d, ph::TREE_STRUCTURE, "当前库下没有 SITE")
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
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show_rows(ui, d.row_h(), rows.len(), |ui, range| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for row in &rows[range] {
                                let meta = if row.loading { "加载中…" } else { row.noun.as_str() };
                                let out = widgets::tree_row_ui(
                                    ui,
                                    t,
                                    d,
                                    TreeRow {
                                        depth: row.depth as usize,
                                        icon: RowIcon::Glyph(noun_icon(&row.noun)),
                                        label: &row.name,
                                        meta,
                                        selected: vm.selected == Some(row.refno),
                                        expandable: row.expandable,
                                        tone: Status::Neutral,
                                        tags: &[],
                                    },
                                );
                                // 点箭头只折叠 / 展开；点行其他区域选中；双击整行也展开。
                                if out.response.clicked() {
                                    let on_caret = out.caret_rect.is_some_and(|r| {
                                        out.response
                                            .interact_pointer_pos()
                                            .is_some_and(|p| r.contains(p))
                                    });
                                    if on_caret {
                                        cmds.push(Cmd::ToggleExpand(row.refno));
                                    } else {
                                        cmds.push(Cmd::SelectElement(row.refno));
                                    }
                                } else if out.response.double_clicked()
                                    && row.expandable.is_some()
                                {
                                    cmds.push(Cmd::ToggleExpand(row.refno));
                                }
                            }
                        });
                });
        }
    }
}

/// PDMS 类型 -> 行首图标。挑常见层级给个可辨识的形状，其余统一立方体。
fn noun_icon(noun: &str) -> &'static str {
    match noun {
        "SITE" => ph::FACTORY,
        "ZONE" => ph::BOUNDING_BOX,
        "PIPE" => ph::PIPE,
        "BRAN" => ph::GIT_BRANCH,
        "EQUI" => ph::ENGINE,
        "STRU" | "FRMW" => ph::WALL,
        "HANG" | "SUPPO" => ph::ANCHOR,
        _ => ph::CUBE,
    }
}

/// 树区的居中提示（连接中 / 失败 / 空库）。
fn note(ui: &mut Ui, t: &Tokens, d: Density, icon: &str, text: &str) {
    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.vertical_centered(|ui| {
            ui.add_space(d.px(40.0));
            ui.label(
                RichText::new(icon)
                    .font(egui::FontId::new(
                        d.px(24.0),
                        egui::FontFamily::Proportional,
                    ))
                    .color(t.text_muted),
            );
            ui.add_space(space::S2);
            ui.label(
                RichText::new(text)
                    .font(Font::meta(d))
                    .color(t.text_secondary),
            );
        });
    });
}
