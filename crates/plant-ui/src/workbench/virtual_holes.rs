//! 「虚拟孔洞」页签（ADR-0031 阶段一 / 阶段二）：AMS 提资表导入的开孔记录列表。
//!
//! 数据节奏分三层：导入（`Cmd::ImportVirtualHoles`，一次拿到整表）→ 映射
//! （每条记录的 `RelyItemREF` 去库里确认目标存在，宿主经数据层查祖先链）→ 布尔
//! 预览（宿主编排，回执进会话再同步到这里）。本页签只读 `vm.virtual_holes`，
//! 任何动作都发命令，不自己碰会话。
//!
//! 手动映射的入口是「映射到选中」：先在模型树上选中目标元素，再点这枚按钮。
//! 没有弹窗挑元素——弹窗要一套自己的元素挑选器，阶段一不造第二套树。
//!
//! 布尔结果只改三维画面（内存里的网格替换），不写回正式模型；清掉预览
//! （`Cmd::ClearVirtualHolePreview`）时原始网格整层恢复。

use egui::{Align2, Rect, RichText, ScrollArea, Sense, Ui, vec2};
use egui_phosphor::regular as ph;

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius, space};
use crate::style::widgets::{self, PaneNote, PaneState, button, status_tag, tool_btn};
use crate::vm::{VirtualHoleRowVm, VirtualHoleTargetVm, WorkbenchVm};
use crate::virtual_hole::VirtualHoleStatus;
use crate::Cmd;

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    let vh = &vm.virtual_holes;
    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.spacing_mut().item_spacing.y = 0.0;
        toolbar(ui, t, d, vm, cmds);
        ui.add_space(space::S2);
        if vh.holes.is_empty() && vh.row_errors.is_empty() {
            note(
                ui,
                t,
                d,
                PaneState::Uninit,
                ph::FILE_XLS,
                "导入 AMS 虚拟孔洞表（xlsx / csv）开始预览",
            );
            return;
        }
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if !vh.row_errors.is_empty() {
                    section_header(ui, t, d, "解析错误", &vh.row_errors.len().to_string());
                    for (line, message) in &vh.row_errors {
                        error_row(ui, t, d, *line, message);
                    }
                    ui.add_space(space::S2);
                }
                section_header(ui, t, d, "孔洞记录", &vh.holes.len().to_string());
                for (index, row) in vh.holes.iter().enumerate() {
                    hole_row(ui, t, d, vm, index, row, cmds);
                }
            });
    });
}

/// 工具栏：导入 / 全部预览 / 清除，以及「看原始」「X-Ray」两枚开关。
///
/// 开关用 `tool_btn` 的 active 态画、点一下发命令——Vm 是只读的，
/// 开关真值归宿主会话，回执落地后再同步回来翻 active。
fn toolbar(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    let vh = &vm.virtual_holes;
    let mapped = vh
        .holes
        .iter()
        .filter(|row| matches!(row.target, VirtualHoleTargetVm::Mapped(_)))
        .count();
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), d.px(36.0)));
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(vec2(0.0, d.px(3.0))))
            .id(egui::Id::new("vh-toolbar")),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            if ui
                .add(button(t, d, "导入").icon(ph::FILE_XLS).primary())
                .on_hover_text("选择 xlsx / csv 提资表")
                .clicked()
            {
                cmds.push(Cmd::ImportVirtualHoles);
            }
            let preview = ui.add_enabled(
                mapped > 0 && vh.calculating == 0,
                button(t, d, "全部预览").icon(ph::PLAY),
            );
            if preview.clicked() {
                cmds.push(Cmd::PreviewVirtualHoles);
            } else if preview.hovered() && mapped == 0 {
                preview.on_hover_text(
                    "先映射出目标：RelyItemREF 要能解析成参考号、且库里有这个元素",
                );
            }
            let clear = ui.add_enabled(
                vh.calculating > 0 || vh.done > 0 || !vh.holes.is_empty(),
                button(t, d, "清除").icon(ph::TRASH),
            );
            if clear.clicked() {
                cmds.push(Cmd::ClearVirtualHolePreview);
            } else if clear.hovered() {
                clear.on_hover_text("恢复所有原始网格，撤掉切削体；孔洞列表保留");
            }
            ui.add_space(d.px(space::S2));
            let original = ui.add(tool_btn(t, d, ph::EYE, vh.show_original));
            if original.clicked() {
                cmds.push(Cmd::ShowVirtualHoleOriginal(!vh.show_original));
            }
            original.on_hover_text(if vh.show_original {
                "此刻看原始网格；点切回布尔预览"
            } else {
                "看原始网格（布尔结果只改画面，不写回模型）"
            });
            let xray = ui.add(tool_btn(t, d, ph::SUN, vh.xray));
            if xray.clicked() {
                cmds.push(Cmd::SetVirtualHoleXRay(!vh.xray));
            }
            xray.on_hover_text("X-Ray：被开孔的实体半透明，看清孔洞在实体内部的样子");
            if vh.calculating > 0 {
                ui.add_space(d.px(space::S2));
                ui.label(
                    RichText::new(format!(
                        "计算中 {}/{}",
                        vh.done + vh.calculating,
                        vh.holes.len()
                    ))
                    .font(Font::meta(d))
                    .color(t.text_muted),
                );
                // 布尔回执是异步的：不催这一帧，界面会停在旧数字上不动。
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(200));
            }
        },
    );
    ui.painter()
        .hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, t.border));
}

/// 一行孔洞：主行（编号 + 尺寸 + 状态）+ 次行（映射目标 + 动作）。
///
/// 整行可点选中；行上铺子 Ui 的 `max_rect` 是行内的两段，按钮的点击不会
/// 冒泡成行选中（`scope_builder` 拿到的是独立子 Ui）。
fn hole_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    index: usize,
    row: &VirtualHoleRowVm,
    cmds: &mut Vec<Cmd>,
) {
    let selected = vm.virtual_holes.selected.as_deref() == Some(row.id.as_str());
    let h = d.px(52.0);
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), h));
    let row_resp = ui.interact(rect, egui::Id::new(("vh-row", index)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let (bg, border) = if selected {
        (t.accent_bg, Some(t.accent))
    } else if row_resp.hovered() {
        (t.bg_hover, None)
    } else {
        (egui::Color32::TRANSPARENT, None)
    };
    let p = ui.painter();
    p.rect_filled(rect, radius::MD, bg);
    if let Some(border) = border {
        p.rect_stroke(
            rect,
            radius::MD,
            egui::Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
    }
    if row_resp.clicked() {
        cmds.push(Cmd::SelectVirtualHole(row.id.clone()));
    }
    let pad = d.px(space::S3);
    let inner = rect.shrink2(vec2(pad, d.px(5.0)));
    // 主行：编号 + 尺寸标签 + 右侧状态芯片。
    let top = Rect::from_min_size(inner.min, vec2(inner.width(), d.px(20.0)));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(top)
            .id(egui::Id::new(("vh-row-top", index))),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            let id = if row.id.is_empty() {
                "（未命名）".to_string()
            } else {
                row.id.clone()
            };
            ui.label(RichText::new(id).font(Font::strong(d)).color(t.text_primary));
            ui.label(
                RichText::new(row.size_label.clone())
                    .font(Font::mono_micro(d))
                    .color(t.text_muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, status) = status_chip(row.status);
                ui.add(status_tag(t, d, text, status));
            });
        },
    );
    // 次行：映射目标芯片 + 右侧动作。
    let bottom = Rect::from_min_size(
        inner.min + vec2(0.0, d.px(24.0)),
        vec2(inner.width(), d.px(22.0)),
    );
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(bottom)
            .id(egui::Id::new(("vh-row-bottom", index))),
        |ui| {
            ui.spacing_mut().item_spacing.x = d.px(space::S2);
            target_chip(ui, t, d, row);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                row_actions(ui, t, d, vm, row, cmds);
            });
        },
    );
    ui.painter()
        .hline(rect.x_range(), rect.bottom(), egui::Stroke::new(1.0, t.border));
}

/// 映射目标芯片：未解析 / 确认中 / 已映射（带 refno）/ 目标不存在。
fn target_chip(ui: &mut Ui, t: &Tokens, d: Density, row: &VirtualHoleRowVm) {
    let (text, status) = match row.target {
        VirtualHoleTargetVm::Unparsed => ("未解析 RelyItemREF".to_string(), Status::Warn),
        VirtualHoleTargetVm::Resolving => ("确认目标中…".to_string(), Status::Info),
        VirtualHoleTargetVm::Mapped(refno) => (refno.to_string(), Status::Success),
        VirtualHoleTargetVm::Missing => ("目标不存在".to_string(), Status::Error),
    };
    ui.add(status_tag(t, d, text.as_str(), status))
        .on_hover_text(match row.target {
            VirtualHoleTargetVm::Unparsed => {
                "RelyItemREF 只认两段数字（24381_145018 / 24381/145018）"
            }
            VirtualHoleTargetVm::Resolving => "正在库里确认这个参考号",
            VirtualHoleTargetVm::Mapped(_) => "RelyItemREF 映射到的实体",
            VirtualHoleTargetVm::Missing => "库里没有这个参考号；可手动映射到选中元素",
        });
}

/// 行动作：定位孔洞 / 定位实体 / 映射到选中。从右往左排，与工具栏的 tool_btn 同款。
fn row_actions(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    row: &VirtualHoleRowVm,
    cmds: &mut Vec<Cmd>,
) {
    let mapped = matches!(row.target, VirtualHoleTargetVm::Mapped(_));
    let focus_target = ui.add_enabled(mapped, tool_btn(t, d, ph::TARGET, false));
    if focus_target.clicked() {
        cmds.push(Cmd::FocusVirtualHoleTarget(row.id.clone()));
    }
    focus_target.on_hover_text("定位到映射的实体（没加载时先显示它）");
    let focus_hole = ui.add(tool_btn(t, d, ph::CROSSHAIR, false));
    if focus_hole.clicked() {
        cmds.push(Cmd::FocusVirtualHole(row.id.clone()));
    }
    focus_hole.on_hover_text("定位到孔洞切削体");
    if !mapped
        && let Some(refno) = vm.selection.primary()
    {
        let map = ui.add(tool_btn(t, d, ph::FUNNEL, false));
        if map.clicked() {
            cmds.push(Cmd::MapVirtualHole {
                hole_id: row.id.clone(),
                refno,
            });
        }
        map.on_hover_text("把这条记录映射到模型树上当前选中的元素");
    }
}

/// 状态芯片的文案与颜色。`Failed` 的具体原因在日志里，行上只摆态。
fn status_chip(status: VirtualHoleStatus) -> (&'static str, Status) {
    match status {
        VirtualHoleStatus::Ready => ("已就绪", Status::Neutral),
        VirtualHoleStatus::Calculating => ("计算中", Status::Info),
        VirtualHoleStatus::NoIntersection => ("无相交", Status::Warn),
        VirtualHoleStatus::Changed => ("已切割", Status::Success),
        VirtualHoleStatus::Failed => ("失败", Status::Error),
    }
}

fn error_row(ui: &mut Ui, t: &Tokens, d: Density, line: usize, message: &str) {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), d.px(26.0)));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    let pad = d.px(space::S3);
    p.text(
        rect.left_center() + vec2(pad, 0.0),
        Align2::LEFT_CENTER,
        format!("第 {line} 行"),
        Font::mono_micro(d),
        t.warn,
    );
    p.text(
        rect.left_center() + vec2(d.px(64.0), 0.0),
        Align2::LEFT_CENTER,
        message,
        Font::meta(d),
        t.text_secondary,
    );
}

fn section_header(ui: &mut Ui, t: &Tokens, d: Density, title: &str, count: &str) {
    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), d.px(30.0)));
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    let pad = d.px(space::S3);
    p.text(
        rect.left_center() + vec2(pad, 0.0),
        Align2::LEFT_CENTER,
        title,
        Font::meta_strong(d),
        t.text_secondary,
    );
    p.text(
        rect.right_center() - vec2(pad, 0.0),
        Align2::RIGHT_CENTER,
        count,
        Font::mono_micro(d),
        t.text_muted,
    );
}

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
            retry: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::CornerRadius;

    /// 状态芯片的文案钉死：五态一一对应，面板与日志里的状态名要一致。
    #[test]
    fn status_chip_covers_every_status() {
        assert_eq!(status_chip(VirtualHoleStatus::Ready).0, "已就绪");
        assert_eq!(status_chip(VirtualHoleStatus::Calculating).0, "计算中");
        assert_eq!(status_chip(VirtualHoleStatus::NoIntersection).0, "无相交");
        assert_eq!(status_chip(VirtualHoleStatus::Changed).0, "已切割");
        assert_eq!(status_chip(VirtualHoleStatus::Failed).0, "失败");
    }

    /// `CornerRadius` 的 u8 形参与 painter 直接认——别让这里的圆角半径悄悄漂移。
    #[test]
    fn row_radius_matches_panel_language() {
        let r: CornerRadius = radius::MD.into();
        assert_eq!(r, CornerRadius::same(radius::MD));
    }
}
