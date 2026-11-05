//! 「Spooling」页签：卷管图 / 管段 / 制造管段 / 暖通管段 / 现场段 / 管段件六类平铺。
//!
//! 为什么它不是模型树里的一支：spool 的 owner 链（`SPOOL -> SPLDRG -> ISODEP`、
//! `PSPOOL -> PSLIST -> PIPE`）**不经过 `SITE`**，从 SITE 根出发的树永远走不到。
//! 这不是缺一个分支，是另一条层级，所以单立一个页签（与房间树同一侧 dock）。
//!
//! 布局是「一行分组芯片 + 当前分组的行列表」：六组同时展开就要拉六次查询，
//! 而这一层在 AMS 上六组全是 0（带 spool 的 `ams251095` / `ams251137` 不在
//! `/ALL` 的 `CURD` 名单里，gen-model 从不解析）。所以默认选第一个非空组，
//! 全空时如实说「这一层是通的、只是没有这类数据」，而不是画成故障。
//!
//! 点一行展开它的成员：成员来自两路——`pe_owner` 结构边与 spool 自己的引用属性
//! （`SPLMEM` / `PSARFA` / `AELRFA` …），后者是 E3D 内核算出来的断点归属。

use egui::{Id, Margin, RichText, ScrollArea, Ui, UiBuilder};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::spooling::{EMPTY_HINT, SpoolingAction, group_count_text};
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, space};
use crate::style::widgets::{self, Eye, PaneNote, PaneState, RowIcon, TreeRow};
use crate::vm::{
    RowVisibility, SpoolingDataVm, SpoolingMemberVm, SpoolingRowVm, SpoolingVm, WorkbenchVm,
};

pub fn show(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    match &vm.spooling {
        // 第一次画出来才拉统计（与房间树同一条规矩：页签不在前台不拉数据）。
        SpoolingVm::Idle => {
            cmds.push(Cmd::Spooling(SpoolingAction::Load));
            note(
                ui,
                t,
                d,
                PaneState::Loading,
                ph::SPINNER,
                "正在统计管段数据…",
                None,
                false,
            );
        }
        SpoolingVm::Loading => {
            note(
                ui,
                t,
                d,
                PaneState::Loading,
                ph::SPINNER,
                "正在统计管段数据…",
                None,
                false,
            );
        }
        SpoolingVm::Failed(reason) => {
            if note(
                ui,
                t,
                d,
                PaneState::Error,
                ph::WARNING,
                "管段统计取数失败",
                Some(reason),
                true,
            ) {
                cmds.push(Cmd::Spooling(SpoolingAction::Load));
            }
        }
        SpoolingVm::Ready(data) => {
            // 眼睛是显隐开关，得有真的渲染器才按得动：独立壳里整列不出现
            // （同模型树 `tree.rs` 与房间树 `room_tree.rs` 的门禁）。
            let live = vm.view3d.as_ref().is_some_and(|v| v.live);
            egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.spacing_mut().item_spacing.y = 0.0;
                header(ui, t, d, data, cmds);
                body(ui, t, d, data, live, cmds);
            });
        }
    }
}

/// 六枚分组芯片 + 状态行。芯片常画：全 0 时也要让人看见「六类都在、计数都是 0」，
/// 这是数据现状而不是面板坏了。
fn header(ui: &mut Ui, t: &Tokens, d: Density, data: &SpoolingDataVm, cmds: &mut Vec<Cmd>) {
    egui::Frame::new()
        .fill(t.bg_header)
        .inner_margin(Margin::symmetric(space::S2 as i8, space::S1 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(d.px(4.0), space::S1);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(4.0);
                for group in &data.groups {
                    let label = format!("{} {}", group.label, group_count_text(group.count));
                    let active = data.active.as_deref() == Some(group.noun.as_str());
                    if ui.add(widgets::chip(t, d, &label, active)).clicked() && !active {
                        cmds.push(Cmd::Spooling(SpoolingAction::SelectGroup(
                            group.noun.clone(),
                        )));
                    }
                }
                if ui
                    .add(widgets::tool_btn(t, d, ph::ARROWS_CLOCKWISE, false))
                    .on_hover_text("重新统计并重拉当前分组")
                    .clicked()
                {
                    cmds.push(Cmd::Spooling(SpoolingAction::Refresh));
                }
            });
            let total: usize = data.groups.iter().map(|g| g.count).sum();
            let mut status = if total == 0 {
                "本 MDB 范围内没有管段数据".to_string()
            } else {
                format!("合计 {total} 条")
            };
            if let Some((label, count)) = data.active.as_ref().and_then(|noun| {
                data.groups
                    .iter()
                    .find(|g| &g.noun == noun)
                    .map(|g| (g.label.as_str(), g.count))
            }) {
                status = format!("{label} {count} 条 · {status}");
            }
            if let Some(limit) = data.rows_truncated_at {
                status.push_str(&format!(" · 仅显示前 {limit} 条"));
            }
            ui.label(
                RichText::new(status)
                    .font(Font::micro(d))
                    .color(t.text_muted),
            );
            if let Some(error) = &data.rows_error {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("取行失败：{error}"))
                            .font(Font::micro(d))
                            .color(t.danger),
                    )
                    .truncate(),
                )
                .on_hover_text(error);
            }
        });
}

fn body(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    data: &SpoolingDataVm,
    live: bool,
    cmds: &mut Vec<Cmd>,
) {
    // 全 0：这一层通了，但项目里没有这类数据。写清「为什么」——`/ALL` 没声明带
    // spool 的库是最常见的成因，用户看到这句就知道该去改 MDB，而不是怀疑面板。
    let total: usize = data.groups.iter().map(|g| g.count).sum();
    if total == 0 {
        note(
            ui,
            t,
            d,
            PaneState::Empty,
            ph::PACKAGE,
            "本 MDB 范围内没有管段数据",
            Some(EMPTY_HINT),
            false,
        );
        return;
    }
    if data.rows_loading {
        note(
            ui,
            t,
            d,
            PaneState::Loading,
            ph::SPINNER,
            "正在拉取…",
            None,
            false,
        );
        return;
    }
    if data.rows.is_empty() {
        note(
            ui,
            t,
            d,
            PaneState::Empty,
            ph::PACKAGE,
            "这一类没有可列出的元素",
            None,
            false,
        );
        return;
    }
    egui::Frame::new()
        .fill(t.bg_panel)
        .inner_margin(Margin {
            left: space::S1 as i8,
            right: space::S1 as i8,
            top: space::S1 as i8,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            // 同模型树：`show_rows` 拿外层 ui 的 item_spacing.y 折算每行占位，
            // 设在闭包里就晚了，滚动条会比内容长出近四分之一。
            ui.spacing_mut().item_spacing.y = 0.0;
            ScrollArea::vertical()
                .id_salt("spooling-rows")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for row in &data.rows {
                        row_ui(ui, t, d, row, data, live, cmds);
                    }
                });
        });
}

fn row_ui(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    row: &SpoolingRowVm,
    data: &SpoolingDataVm,
    live: bool,
    cmds: &mut Vec<Cmd>,
) {
    // 行 id 按 refno 定死：虚拟滚动下自动 id 是「第几行」，行序一变右键菜单就串行。
    let id = Id::new(("wb-spooling-row", row.refno));
    let out = with_locate(ui, t, d, row.refno, &row.label, cmds, |ui| {
        ui.scope_builder(UiBuilder::new().id(id), |ui| {
            widgets::tree_row_ui(
                ui,
                t,
                d,
                TreeRow {
                    depth: 0,
                    icon: RowIcon::Glyph(super::noun_icon(&row.noun)),
                    label: &row.label,
                    meta: &row.meta,
                    selected: row.selected,
                    primary: row.primary,
                    // 成员是按需拉的，所以这一行永远「可展开」。
                    expandable: Some(true),
                    // 没有实时渲染器时整列不出现（同模型树与房间树）：按不动的开关
                    // 摆在那儿就是假控件。有渲染器时四态各画各的。
                    eye: live.then(|| eye_of(row.visibility)),
                    tone: Status::Neutral,
                    tags: &[],
                },
            )
        })
        .inner
    });
    // 自动化只认真正的树行：AccessKit 角色负责排除命令历史、属性面板与状态栏，
    // 标签里的 refno 则给出跨重命名的稳定身份——探针按 refno 找 spool 行靠的就是它。
    // 展开态按成员列表此刻挂在哪一行上算：这一行永远可展开，但不一定正展开。
    let open = data
        .members
        .as_ref()
        .is_some_and(|m| m.of == Some(row.refno));
    let accessible_label = tree_item_accessible_label(row.refno, &row.label);
    ui.ctx().accesskit_node_builder(out.response.id, |node| {
        node.set_role(egui::accesskit::Role::TreeItem);
        node.set_label(accessible_label);
        node.set_expanded(open);
    });
    // 点眼睛只切可见性：不选中也不展开（同模型树）。命中区在 `tree_row_ui` 里给，
    // 这里只判指针落没落在那一格。
    let hit = |r: Option<egui::Rect>| {
        r.is_some_and(|r| {
            out.response
                .interact_pointer_pos()
                .is_some_and(|p| r.contains(p))
        })
    };
    if hit(out.eye_rect) {
        if out.response.clicked() {
            // 方向由宿主算好挂在行上：图标跟的是实际渲染结果，指令还在路上时它不动，
            // 这里不能拿它反推。
            cmds.push(eye_cmd(row.refno, row.next_visible));
        }
    } else if out.response.clicked() {
        cmds.push(Cmd::Spooling(SpoolingAction::SelectRow(row.refno)));
    }
    if open {
        members_ui(ui, t, d, data, cmds);
    }
}

/// 展开行的成员列表。两路来源用行尾文字区分：结构边（`pe_owner`）与引用属性。
fn members_ui(ui: &mut Ui, t: &Tokens, d: Density, data: &SpoolingDataVm, cmds: &mut Vec<Cmd>) {
    let Some(members) = &data.members else {
        return;
    };
    if members.loading {
        ui.add_space(space::S1);
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(d.meta()));
            ui.label(
                RichText::new("正在取成员…")
                    .font(Font::micro(d))
                    .color(t.text_muted),
            );
        });
        return;
    }
    if let Some(error) = &members.error {
        ui.add(
            egui::Label::new(
                RichText::new(format!("成员取数失败：{error}"))
                    .font(Font::micro(d))
                    .color(t.danger),
            )
            .truncate(),
        )
        .on_hover_text(error);
        return;
    }
    if members.rows.is_empty() {
        ui.label(
            RichText::new("这一段没有成员")
                .font(Font::micro(d))
                .color(t.text_muted),
        );
        return;
    }
    for member in &members.rows {
        member_row(ui, t, d, member, cmds);
    }
}

fn member_row(ui: &mut Ui, t: &Tokens, d: Density, member: &SpoolingMemberVm, cmds: &mut Vec<Cmd>) {
    let id = Id::new(("wb-spooling-member", member.refno));
    let out = with_locate(ui, t, d, member.refno, &member.label, cmds, |ui| {
        ui.scope_builder(UiBuilder::new().id(id), |ui| {
            widgets::tree_row_ui(
                ui,
                t,
                d,
                TreeRow {
                    depth: 1,
                    icon: RowIcon::Glyph(super::noun_icon(&member.noun)),
                    label: &member.label,
                    meta: member.via,
                    selected: false,
                    primary: false,
                    expandable: None,
                    // 成员行的眼睛是 Milestone 2 的范围：无独立几何的件（GASK / SILE…）
                    // 要先有「停在未加载 + 说明」那套语义，单独摆一只没接线的眼睛只会骗人。
                    eye: None,
                    tone: Status::Neutral,
                    tags: &[],
                },
            )
        })
        .inner
    });
    // 成员行同样要能被探针按 refno 找到（Milestone 2 的成员行验收会用到）。
    let accessible_label = tree_item_accessible_label(member.refno, &member.label);
    ui.ctx().accesskit_node_builder(out.response.id, |node| {
        node.set_role(egui::accesskit::Role::TreeItem);
        node.set_label(accessible_label);
    });
    if out.response.clicked() {
        cmds.push(Cmd::Spooling(SpoolingAction::SelectRow(member.refno)));
    }
}

/// AccessKit 标签：`refno {e3d}; name={name}`，与模型树 `tree_item_accessible_label` 同格式。
///
/// E3D 形式自带等号，前缀里就不再补一个，否则读屏念出来是「refno 等于 等于」。
/// 探针按 refno 找 spool 行的唯一途径就是这条标签，不能省。
fn tree_item_accessible_label(refno: aios_core::RefU64, name: &str) -> String {
    format!("refno {}; name={name}", refno.to_e3d_id())
}

/// 点眼睛发出去的命令。
///
/// `visible` 一律取宿主算好的方向（`next_visible`），不从图标反推：图标跟的是
/// 三维实际渲染结果，指令下出去到画面变过来之间隔着查询与网格装载。
fn eye_cmd(refno: aios_core::RefU64, next_visible: bool) -> Cmd {
    Cmd::Spooling(SpoolingAction::SetVisible {
        refno,
        visible: next_visible,
    })
}

/// Vm 的四态可见性翻成组件层的画法（同 `tree::eye_of` / `room_tree::eye_of`）。
///
/// 组件层不认识 `vm`，两个枚举是同一件事在两层的说法。这已是第三份副本；
/// 提取到 `widgets` 属于重构，不在本 feature 范围。
fn eye_of(vis: RowVisibility) -> Eye {
    match vis {
        RowVisibility::Unloaded => Eye::Unloaded,
        RowVisibility::Shown => Eye::Shown,
        RowVisibility::Hidden => Eye::Hidden,
        RowVisibility::Partial => Eye::Partial,
    }
}

/// 行尾挂一枚「定位」按钮，行本体占剩下的宽度。
///
/// 按钮是独立的 egui 控件（不是在行上划一块热区）：Tab 能聚焦、回车 / 空格能按，
/// 读屏拿得到它的 tooltip 名字。
fn with_locate<R>(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    refno: aios_core::RefU64,
    label: &str,
    cmds: &mut Vec<Cmd>,
    row: impl FnOnce(&mut Ui) -> R,
) -> R {
    let h = d.row_h();
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let button = egui::Button::new(
                RichText::new(ph::CROSSHAIR)
                    .size(d.meta())
                    .color(t.text_secondary),
            )
            .frame(false)
            .min_size(egui::vec2(h, h));
            let hint = format!("在模型树中定位 {label} 并对准相机");
            if ui.add(button).on_hover_text(&hint).clicked() {
                cmds.push(Cmd::Spooling(SpoolingAction::Locate(refno)));
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), row)
                .inner
        },
    )
    .inner
}

#[allow(clippy::too_many_arguments)]
fn note(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    state: PaneState,
    icon: &str,
    text: &str,
    detail: Option<&str>,
    retry: bool,
) -> bool {
    widgets::pane_note(
        ui,
        t,
        d,
        PaneNote {
            state,
            icon,
            text,
            detail,
            retry,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aios_core::RefU64;

    fn r(n: u32) -> RefU64 {
        RefU64::from_two_nums(2013287639, n)
    }

    /// 四态必须一一对上，不能有两档画成同一个图标——半可见与全隐藏混在一起，
    /// 用户就看不出「还有一部分没出来」。
    #[test]
    fn eye_of_maps_all_four_states() {
        assert!(matches!(eye_of(RowVisibility::Unloaded), Eye::Unloaded));
        assert!(matches!(eye_of(RowVisibility::Shown), Eye::Shown));
        assert!(matches!(eye_of(RowVisibility::Hidden), Eye::Hidden));
        assert!(matches!(eye_of(RowVisibility::Partial), Eye::Partial));
    }

    /// 三档画法互不相同：Shown / Hidden / Partial 落在三个不同的 `Eye` 上。
    #[test]
    fn eye_of_keeps_the_three_visible_states_distinct() {
        let shown = eye_of(RowVisibility::Shown);
        let hidden = eye_of(RowVisibility::Hidden);
        let partial = eye_of(RowVisibility::Partial);
        assert!(!matches!(shown, Eye::Hidden | Eye::Partial));
        assert!(!matches!(hidden, Eye::Shown | Eye::Partial));
        assert!(!matches!(partial, Eye::Shown | Eye::Hidden));
    }

    /// AccessKit 标签是探针按 refno 找行的唯一途径：必须含 e3d 形式的 refno 与 name。
    #[test]
    fn spool_row_accessible_label_has_refno_and_name() {
        let label = tree_item_accessible_label(r(1000), "/PIPE_4DK_FWD");
        assert!(label.starts_with("refno "), "{label}");
        assert!(label.contains(r(1000).to_e3d_id().as_str()), "{label}");
        assert!(label.contains("name=/PIPE_4DK_FWD"), "{label}");
        // 两种 refno 得到不同标签，探针才能区分行。
        assert_ne!(
            tree_item_accessible_label(r(1000), "a"),
            tree_item_accessible_label(r(1001), "a")
        );
    }

    /// 命中眼睛发出去的方向必须是宿主算好的 `next_visible`，不是从图标反推。
    #[test]
    fn eye_hit_emits_set_visible_with_next_visible() {
        let refno = r(1001);
        assert_eq!(
            eye_cmd(refno, true),
            Cmd::Spooling(SpoolingAction::SetVisible {
                refno,
                visible: true
            })
        );
        assert_eq!(
            eye_cmd(refno, false),
            Cmd::Spooling(SpoolingAction::SetVisible {
                refno,
                visible: false
            })
        );
    }

    /// 在途态：`visibility` 还停在旧态（Shown），而 `next_visible` 已经是 true。
    /// 命令必须跟着 `next_visible` 走——若从图标反推会得到 false，方向就丢了。
    #[test]
    fn eye_command_follows_next_visible_even_when_the_icon_lags() {
        let row = SpoolingRowVm {
            refno: r(1001),
            visibility: RowVisibility::Shown,
            next_visible: true,
            ..SpoolingRowVm::default()
        };
        assert_ne!(
            row.next_visible,
            !matches!(row.visibility, RowVisibility::Shown),
            "这一行的两值必须不同，否则测不出方向来源"
        );
        assert_eq!(
            eye_cmd(row.refno, row.next_visible),
            Cmd::Spooling(SpoolingAction::SetVisible {
                refno: r(1001),
                visible: true
            })
        );
    }

    /// 每一档的四态都要能发出命令，不许有「点了没反应」的档。
    #[test]
    fn every_visibility_state_has_a_direction_to_send() {
        for vis in [
            RowVisibility::Unloaded,
            RowVisibility::Shown,
            RowVisibility::Hidden,
            RowVisibility::Partial,
        ] {
            let visible = vis.on_click();
            let cmd = eye_cmd(r(7), visible);
            assert_eq!(
                cmd,
                Cmd::Spooling(SpoolingAction::SetVisible {
                    refno: r(7),
                    visible
                }),
                "{vis:?} 没有发出命令"
            );
        }
    }
}
