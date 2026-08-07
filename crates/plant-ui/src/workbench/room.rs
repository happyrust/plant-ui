//! 「房间」页签（S13-B 稿）：选中元素的全部房间归属 + 聚焦房间的详情。
//!
//! 两层数据两种节奏：归属列表跟着选中预取（`vm.rooms`，与属性同拍），详情要
//! 用户点了某间房才去查（`vm.room_detail`，`Cmd::FocusRoom` 触发）——详情带
//! 全量成员集，跟着选中自动查会把轻查询养成重查询。所以本页签的空态不是错：
//! 「还没点房间」就只画归属列表加一行提示。
//!
//! 设计稿里的视口压暗 + 亮框是二期（计划第八节）；「缩放到房间」「隔离成员」
//! 两枚动作按钮在独立壳（无实时渲染器）下不出现，与树行菜单的 live 门禁同一条
//! 规矩。归属数据本身不吃这个门禁。

use egui::{Align2, CursorIcon, RichText, ScrollArea, Sense, Ui, vec2};
use egui_phosphor::regular as ph;

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens, space};
use crate::style::widgets::{self, PaneNote, PaneState, PropRow, prop_row_ui};
use crate::vm::{RoomDetailDataVm, RoomDetailVm, RoomVm, RoomsDataVm, WorkbenchVm};
use crate::{Cmd, ModelAction};

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    match &vm.rooms {
        RoomVm::Uninit => {
            note(
                ui,
                t,
                d,
                PaneState::Uninit,
                ph::CURSOR_CLICK,
                "在模型树中选中元素查看房间归属",
            );
        }
        RoomVm::Loading(None) => {
            note(ui, t, d, PaneState::Loading, ph::SPINNER, "归属加载中…");
        }
        // 上一份数据撑住布局防闪烁；房间是全局对象，点击动作对着房间 refno 走，
        // 不会像属性编辑那样落错元素，所以命令不拦。
        RoomVm::Loading(Some(data)) | RoomVm::Ready(data) => ready(ui, t, d, vm, data, cmds),
        RoomVm::Failed(reason) => {
            if note(ui, t, d, PaneState::Error, ph::WARNING, reason)
                && let Some(refno) = vm.selection.primary()
            {
                cmds.push(Cmd::RetryRooms(refno));
            }
        }
    }
}

fn ready(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, data: &RoomsDataVm, cmds: &mut Vec<Cmd>) {
    if data.relations.is_empty() {
        note(
            ui,
            t,
            d,
            PaneState::Empty,
            ph::DOOR_OPEN,
            "该元素不属于任何房间",
        );
        return;
    }
    let live = vm.view3d.is_some_and(|v| v.live);
    let focused = detail_data(&vm.room_detail).map(|det| det.room);
    egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
        ui.set_min_size(ui.available_size());
        ui.spacing_mut().item_spacing.y = 0.0;
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                banner(ui, t, d, &vm.room_detail, cmds);
                summary(ui, t, d, &vm.room_detail, data, live, cmds);
                section_header(ui, t, d, "全部归属", &data.relations.len().to_string());
                for (i, rel) in data.relations.iter().enumerate() {
                    relation_row(ui, t, d, i == 0, focused == rel.room, rel, cmds);
                }
                if let Some(det) = detail_data(&vm.room_detail) {
                    composition(ui, t, d, det);
                    members(ui, t, d, det, cmds);
                }
            });
    });
}

/// 详情的当前可画数据（在途时是上一份，撑布局用）。
fn detail_data(detail: &RoomDetailVm) -> Option<&RoomDetailDataVm> {
    match detail {
        RoomDetailVm::Ready(data) | RoomDetailVm::Loading(Some(data)) => Some(data),
        _ => None,
    }
}

/// 待重算横幅：这间房还在 gen-model 的房间重算队列里，归属数据可能过时。
/// 点击跳任务队列——S12 的房间泳道与它同源（计划目标 5）。
fn banner(ui: &mut Ui, t: &Tokens, d: Density, detail: &RoomDetailVm, cmds: &mut Vec<Cmd>) {
    let Some(det) = detail_data(detail) else {
        return;
    };
    if !det.pending_recalc {
        return;
    }
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), d.px(32.0)), Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, 0, t.warn_bg);
        let pad = d.px(space::S3);
        p.text(
            rect.left_center() + vec2(pad, 0.0),
            Align2::LEFT_CENTER,
            ph::WARNING,
            Font::meta(d),
            t.warn,
        );
        p.text(
            rect.left_center() + vec2(pad + d.px(18.0), 0.0),
            Align2::LEFT_CENTER,
            format!("{} 在待重算队列中，归属可能过时", det.room_num),
            Font::meta(d),
            t.warn,
        );
        p.text(
            rect.right_center() - vec2(pad, 0.0),
            Align2::RIGHT_CENTER,
            "查看队列",
            Font::meta(d),
            t.warn,
        );
    }
    if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        cmds.push(Cmd::FocusPane(super::Pane::TaskQueue));
    }
}

/// 归属概要卡：聚焦房间的房间码、构成计数、强度与两枚模型动作。
/// 还没聚焦房间时只给一行提示——不摆空卡。
fn summary(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    detail: &RoomDetailVm,
    data: &RoomsDataVm,
    live: bool,
    cmds: &mut Vec<Cmd>,
) {
    match detail {
        RoomDetailVm::Uninit => hint_row(ui, t, d, "点击下方房间查看详情，双入口：右键菜单同款"),
        RoomDetailVm::Loading(None) => hint_row(ui, t, d, "房间详情加载中…"),
        RoomDetailVm::Failed(reason) => {
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), d.px(28.0)), Sense::hover());
            if ui.is_rect_visible(rect) {
                ui.painter().text(
                    rect.left_center() + vec2(d.px(space::S3), 0.0),
                    Align2::LEFT_CENTER,
                    format!("{}  {reason}", ph::WARNING),
                    Font::meta(d),
                    t.danger,
                );
            }
        }
        RoomDetailVm::Ready(det) | RoomDetailVm::Loading(Some(det)) => {
            egui::Frame::new()
                .fill(t.bg_header)
                .inner_margin(egui::Margin::same(d.px(space::S3) as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing = vec2(d.px(8.0), d.px(8.0));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(ph::DOOR_OPEN)
                                .font(Font::title(d))
                                .color(t.accent),
                        );
                        ui.label(
                            RichText::new(det.room_code.as_deref().unwrap_or(&det.room_num))
                                .font(Font::title(d))
                                .color(t.text_primary),
                        );
                        ui.label(
                            RichText::new(&det.name)
                                .font(Font::mono_meta(d))
                                .color(t.text_muted),
                        );
                    });
                    // 强度取自归属列表里这间房那条边；聚焦房间不在当前元素的
                    // 归属里（切换选中的过渡帧）就不硬凑。
                    let strength = data
                        .relations
                        .iter()
                        .find(|rel| rel.room == Some(det.room))
                        .map(|rel| format!(" · 强度 {}/8", rel.inside_count))
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(format!(
                            "面板 {} · 成员 {}{strength}",
                            det.panels.len(),
                            det.member_count,
                        ))
                        .font(Font::mono_meta(d))
                        .color(t.text_secondary),
                    );
                    if live {
                        ui.horizontal(|ui| {
                            // 取景要面板参与：房间的空间范围由墙面板围出来，
                            // 光取成员会把相机贴到设备堆上。与宿主 FocusRoom
                            // 展开的目标集同一口径。
                            let mut targets = det.panels.clone();
                            targets.extend(
                                det.member_refnos
                                    .iter()
                                    .filter(|refno| !det.panels.contains(refno)),
                            );
                            if ui.add(widgets::chip(t, d, "缩放到房间", false)).clicked() {
                                cmds.push(Cmd::Model(ModelAction::FocusGroup {
                                    refnos: targets.clone(),
                                }));
                            }
                            if ui.add(widgets::chip(t, d, "隔离成员", false)).clicked() {
                                cmds.push(Cmd::Model(ModelAction::Isolate { refnos: targets }));
                            }
                        });
                    }
                });
            super::chrome::hairline(ui, t);
        }
    }
}

/// 全部归属列表的一行（两行高）：房号 + 主归属 / 来自成员标记 + 房间码，
/// 右侧顶点数与距离。点击聚焦该房间。
fn relation_row(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    strongest: bool,
    focused: bool,
    rel: &crate::vm::RoomRelationVm,
    cmds: &mut Vec<Cmd>,
) {
    let clickable = rel.room.is_some();
    let (rect, resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), d.px(44.0)),
        if clickable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        if focused {
            p.rect_filled(rect, 0, t.accent_bg);
        } else if resp.hovered() && clickable {
            p.rect_filled(rect, 0, t.bg_hover);
        }
        let pad = d.px(space::S3);
        let icon_color = if focused { t.accent } else { t.text_muted };
        p.text(
            egui::pos2(rect.left() + pad, rect.top() + d.px(14.0)),
            Align2::LEFT_CENTER,
            ph::DOOR_OPEN,
            Font::meta(d),
            icon_color,
        );
        let mut x = rect.left() + pad + d.px(20.0);
        let title = p.layout_no_wrap(rel.room_num.clone(), Font::strong(d), t.text_primary);
        p.galley(
            egui::pos2(x, rect.top() + d.px(14.0) - title.size().y / 2.0),
            title.clone(),
            t.text_primary,
        );
        x += title.size().x + d.px(6.0);
        for (show, text, color) in [
            (strongest, "主归属", t.success),
            (rel.via_member, "来自成员", t.text_muted),
        ] {
            if !show {
                continue;
            }
            let g = p.layout_no_wrap(text.to_owned(), Font::micro(d), color);
            p.galley(
                egui::pos2(x, rect.top() + d.px(14.0) - g.size().y / 2.0),
                g.clone(),
                color,
            );
            x += g.size().x + d.px(6.0);
        }
        p.text(
            egui::pos2(rect.left() + pad + d.px(20.0), rect.top() + d.px(31.0)),
            Align2::LEFT_CENTER,
            rel.room_code.as_deref().unwrap_or("面板不在册（陈旧边）"),
            Font::mono_micro(d),
            t.text_muted,
        );
        p.text(
            egui::pos2(rect.right() - pad, rect.top() + d.px(14.0)),
            Align2::RIGHT_CENTER,
            format!("顶点 {}/8", rel.inside_count),
            Font::mono_micro(d),
            t.text_secondary,
        );
        p.text(
            egui::pos2(rect.right() - pad, rect.top() + d.px(31.0)),
            Align2::RIGHT_CENTER,
            format_dist(rel.center_dist),
            Font::mono_micro(d),
            t.text_muted,
        );
    }
    if clickable {
        let resp = resp.on_hover_cursor(CursorIcon::PointingHand);
        if resp.clicked()
            && let Some(room) = rel.room
        {
            cmds.push(Cmd::FocusRoom(room));
        }
    }
}

/// 房间构成属性组（S13-B 的属性块，行样式与属性面板同款）。
fn composition(ui: &mut Ui, t: &Tokens, d: Density, det: &RoomDetailDataVm) {
    section_header(ui, t, d, "房间构成", "");
    let refno = det.room.to_string();
    let panels = det.panels.len().to_string();
    let members = det.member_count.to_string();
    let rows = [
        ("名称", det.name.as_str()),
        ("参考号", refno.as_str()),
        ("房号", det.room_num.as_str()),
        ("房间码", det.room_code.as_deref().unwrap_or("—")),
        ("面板数", panels.as_str()),
        ("成员数", members.as_str()),
    ];
    for (i, (key, value)) in rows.into_iter().enumerate() {
        prop_row_ui(
            ui,
            t,
            d,
            PropRow {
                key,
                value,
                mono: true,
                zebra: i % 2 == 0,
                muted: value == "—",
            },
        );
    }
}

/// 成员预览：按归属强度排序的前若干个，点击在树中定位（选中 + 展开 + 滚动到）。
fn members(ui: &mut Ui, t: &Tokens, d: Density, det: &RoomDetailDataVm, cmds: &mut Vec<Cmd>) {
    section_header(
        ui,
        t,
        d,
        "成员预览",
        &format!("{}/{}", det.members.len(), det.member_count),
    );
    for member in &det.members {
        let (rect, resp) =
            ui.allocate_exact_size(vec2(ui.available_width(), d.px(26.0)), Sense::click());
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(rect, 0, t.bg_hover);
            }
            let pad = d.px(space::S3);
            p.text(
                rect.left_center() + vec2(pad, 0.0),
                Align2::LEFT_CENTER,
                super::noun_icon(&member.noun),
                Font::meta(d),
                t.accent,
            );
            p.text(
                rect.left_center() + vec2(pad + d.px(20.0), 0.0),
                Align2::LEFT_CENTER,
                &member.name,
                Font::meta(d),
                t.text_primary,
            );
            p.text(
                rect.right_center() - vec2(pad, 0.0),
                Align2::RIGHT_CENTER,
                format!("{} · {}/8", member.noun, member.inside_count),
                Font::mono_micro(d),
                t.text_muted,
            );
        }
        if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            cmds.push(Cmd::LocateElement(member.refno));
        }
    }
    if det.member_count > det.members.len() {
        hint_row(
            ui,
            t,
            d,
            &format!(
                "共 {} 个成员，此处预览最强 {} 个",
                det.member_count,
                det.members.len(),
            ),
        );
    }
}

/// 分组头，样式随属性面板（高 30，标题 11/600，右侧计数 10 等宽）；
/// 不做折叠——本页签内容量小，折叠是多余的一档状态。
fn section_header(ui: &mut Ui, t: &Tokens, d: Density, title: &str, count: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(30.0)), Sense::hover());
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

/// 一行弱化提示（空态 / 截断说明用）。
fn hint_row(ui: &mut Ui, t: &Tokens, d: Density, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), d.px(28.0)), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().text(
        rect.left_center() + vec2(d.px(space::S3), 0.0),
        Align2::LEFT_CENTER,
        text,
        Font::micro(d),
        t.text_muted,
    );
}

/// 距离显示：`room_relate.center_dist` 是 PDMS 毫米，米级以上换米报一位小数。
pub(super) fn format_dist(mm: f32) -> String {
    if mm >= 1000.0 {
        format!("{:.1} m", mm / 1000.0)
    } else {
        format!("{mm:.0} mm")
    }
}

/// 页签的四态提示。返回值是错误态那枚「重试」是否被点。
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 毫米进位到米的门槛与格式都钉住：菜单 meta 与页签列表两处共用，
    /// 错一处就是两处一起骗人。
    #[test]
    fn distances_read_in_the_right_unit() {
        assert_eq!(format_dist(0.0), "0 mm");
        assert_eq!(format_dist(999.4), "999 mm");
        assert_eq!(format_dist(1000.0), "1.0 m");
        assert_eq!(format_dist(5640.0), "5.6 m");
    }
}
