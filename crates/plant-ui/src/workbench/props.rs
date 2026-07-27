//! 属性视图（M1-3）：跟随选中元素，按「通用 / 元件 / UDA」三组展示真实属性。
//!
//! 规格来自设计稿的「当前元素」头（bg-header 底 + 下边框，类型图标 16 accent +
//! 名称 14/600 + refno 芯片）与 Sec-* 分组头（高 30 可折叠，标题 11/600，
//! 右侧行数 10 等宽），行沿用组件层的 `prop_row_ui`。
//!
//! 标量属性（数值 / 布尔 / 文本）走组件层的 `prop_row_edit` 给行内控件，回车或失焦
//! 提交成 `Cmd::EditAttr`；引用、方位、数组、未设值一律只读——它们要元素选择器或
//! 方位串解析器才能安全地改，给个自由文本框只会写进去非法值。
//!
//! 设计稿里这一屏还有三样东西没画，都不是遗漏：
//! - 会话号 / 修改人 / 修改时间、「待重新生成」状态芯片 —— 版本数据（his_pe），
//!   M4 接入前不摆假值；
//! - 「定位到模型」「在树中显示」两个按钮 —— 要等三维视图与四视图联动（M1-5/M1-6），
//!   现在画出来只能是死按钮。

use egui::{
    Align2, CornerRadius, CursorIcon, FontId, Margin, RichText, ScrollArea, Sense, Stroke,
    TextEdit, Ui, vec2,
};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, radius, space};
use crate::style::widgets::{
    self, PaneNote, PaneState, PropRow, prop_row_edit, prop_row_ui, toggle,
};
use crate::vm::{PropKind, PropRowVm, PropsDataVm, PropsVm, WorkbenchVm};

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    match &vm.props {
        PropsVm::Uninit => {
            note(
                ui,
                t,
                d,
                PaneState::Uninit,
                ph::CURSOR_CLICK,
                "在模型树中选中元素查看属性",
            );
        }
        PropsVm::Loading => {
            note(ui, t, d, PaneState::Loading, ph::SPINNER, "属性加载中…");
        }
        // 重查的是当前选中元素；选中已经挪走的话 App 侧会把这条丢掉。
        PropsVm::Failed(reason) => {
            if note(ui, t, d, PaneState::Error, ph::WARNING, reason)
                && let Some(refno) = vm.selection.primary()
            {
                cmds.push(Cmd::RetryProps(refno));
            }
        }
        PropsVm::Ready(data) => {
            egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                // 头部、下边框、分组行之间不能有间距，否则边框会被推离头部。
                ui.spacing_mut().item_spacing.y = 0.0;
                current_element(ui, t, d, data);
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        section(ui, t, d, data, "通用属性", &data.common, false, cmds);
                        section(ui, t, d, data, "元件属性", &data.attrs, false, cmds);
                        if !data.udas.is_empty() {
                            section(ui, t, d, data, "UDA 属性", &data.udas, true, cmds);
                        }
                    });
            });
        }
    }
}

/// 「当前元素」头：类型图标 + 「NOUN 名称」+ refno 芯片。
fn current_element(ui: &mut Ui, t: &Tokens, d: Density, data: &PropsDataVm) {
    egui::Frame::new()
        .fill(t.bg_header)
        .inner_margin(egui::Margin::same(space::S3 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(space::S2, d.px(10.0));
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(super::noun_icon(&data.noun))
                        .font(FontId::proportional(d.px(16.0)))
                        .color(t.accent),
                );
                let title = if data.noun.is_empty() {
                    data.name.clone()
                } else {
                    format!("{} {}", data.noun, data.name)
                };
                ui.label(
                    RichText::new(title)
                        .font(Font::title(d))
                        .color(t.text_primary),
                );
            });
            refno_chip(ui, t, d, &data.refno.to_string());
        });
    super::chrome::hairline(ui, t);
}

/// refno 芯片：高 20、bg-input 底、11 等宽次级色。设计稿把 refno 放在头部，
/// 所以通用属性组里不再重复一行。
fn refno_chip(ui: &mut Ui, t: &Tokens, d: Density, refno: &str) {
    let pad = d.px(8.0);
    let g = ui
        .painter()
        .layout_no_wrap(refno.to_owned(), Font::mono_meta(d), t.text_secondary);
    let (rect, _) =
        ui.allocate_exact_size(vec2(g.size().x + pad * 2.0, d.px(20.0)), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::SM), t.bg_input);
    ui.painter().galley(
        egui::pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0),
        g,
        t.text_secondary,
    );
}

/// 分组头（高 30：折叠箭头 13 + 标题 11/600 + 右侧行数 10 等宽）+ 组内行。
#[allow(clippy::too_many_arguments)]
fn section(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    data: &PropsDataVm,
    title: &str,
    rows: &[PropRowVm],
    collapsed_by_default: bool,
    cmds: &mut Vec<Cmd>,
) {
    let id = ui.id().with(("props-sec", title));
    let mut collapsed = ui.data_mut(|store| *store.get_temp_mut_or(id, collapsed_by_default));

    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(30.0)), Sense::click());
    if resp.clicked() {
        collapsed = !collapsed;
        ui.data_mut(|store| store.insert_temp(id, collapsed));
    }
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let pad = d.px(space::S3);
        let caret = if collapsed {
            ph::CARET_RIGHT
        } else {
            ph::CARET_DOWN
        };
        p.text(
            rect.left_center() + vec2(pad, 0.0),
            Align2::LEFT_CENTER,
            caret,
            FontId::proportional(d.px(13.0)),
            t.text_muted,
        );
        p.text(
            rect.left_center() + vec2(pad + d.px(13.0 + 6.0), 0.0),
            Align2::LEFT_CENTER,
            title,
            Font::meta_strong(d),
            t.text_secondary,
        );
        p.text(
            rect.right_center() - vec2(pad, 0.0),
            Align2::RIGHT_CENTER,
            rows.len().to_string(),
            FontId::monospace(d.px(10.0)),
            t.text_muted,
        );
    }
    resp.on_hover_cursor(CursorIcon::PointingHand);

    if !collapsed {
        for (i, row) in rows.iter().enumerate() {
            prop_row(ui, t, d, data, row, i % 2 == 0, cmds);
        }
    }
}

/// 一行属性：只读的走绘制型 `prop_row_ui`，可编辑的走容器型 `prop_row_edit`
/// 并在值区放控件。两者共用组件层的键列宽，值列落在同一条竖线上。
fn prop_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    data: &PropsDataVm,
    row: &PropRowVm,
    zebra: bool,
    cmds: &mut Vec<Cmd>,
) {
    if !row.kind.editable() {
        prop_row_ui(
            ui,
            t,
            d,
            PropRow {
                key: &row.key,
                value: &row.value,
                mono: true,
                zebra,
                muted: row.muted,
            },
        );
        return;
    }

    let id = ui.make_persistent_id(("prop-edit", data.refno.to_string(), &row.attr));
    let mut edited: Option<String> = None;
    prop_row_edit(ui, t, d, &row.key, 0, zebra, |ui| {
        if row.kind == PropKind::Bool {
            let mut on = row.value == "true";
            if ui.add(toggle(t, d, &mut on)).changed() {
                edited = Some(if on { "true" } else { "false" }.to_owned());
            }
            return;
        }

        let mut text = ui
            .data_mut(|m| m.get_temp::<String>(id))
            .unwrap_or_else(|| row.value.clone());
        let bad = !valid(row.kind, &text);
        // 输入框整体左移一个内边距，让框内文字而不是框的边缘落在值列竖线上：
        // 面板里只读行占多数，眼睛跟的是值本身，框边对齐反而看着是错开的。
        let inset = d.px(6.0);
        let vr = ui.max_rect();
        let h = d.px(20.0);
        let w = (vr.width() - d.px(12.0) + inset).max(d.px(40.0));
        let rect = egui::Rect::from_min_size(
            egui::pos2(vr.left() - inset, vr.center().y - h / 2.0),
            vec2(w, h),
        );
        let resp = ui.put(
            rect,
            TextEdit::singleline(&mut text)
                .id(id)
                .font(Font::mono(d))
                .margin(Margin::symmetric(inset as i8, d.px(2.0) as i8))
                .background_color(t.bg_input)
                .text_color(if bad { t.danger } else { t.text_primary }),
        );
        if bad {
            ui.painter().rect_stroke(
                resp.rect,
                CornerRadius::same(radius::SM),
                Stroke::new(1.0, t.danger),
                egui::StrokeKind::Inside,
            );
        }
        if resp.changed() {
            ui.data_mut(|m| m.insert_temp(id, text.clone()));
        }
        // 回车与点走别处都算提交；非法值不提交，缓冲一丢，下一帧回到 Vm 的值。
        if resp.lost_focus() {
            ui.data_mut(|m| m.remove_temp::<String>(id));
            if !bad && text != row.value {
                edited = Some(text);
            }
        }
    });

    if let Some(value) = edited {
        cmds.push(Cmd::EditAttr {
            refno: data.refno,
            attr: row.attr.clone(),
            value,
        });
    }
}

/// 数值列的输入校验。旧壳用 `DragValue` 天然进不去非法值，换成文本框后这道
/// 校验就是「异常值有明确显示」的落点：解析不了的当场标红且不提交。
fn valid(kind: PropKind, text: &str) -> bool {
    match kind {
        PropKind::Int => text.trim().parse::<i64>().is_ok(),
        PropKind::Real => text.trim().parse::<f64>().is_ok(),
        _ => true,
    }
}

/// 属性区的三态提示（未选中 / 加载中 / 失败）。返回值是「重试」是否被点。
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
