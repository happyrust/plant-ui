//! 房间树视图（plant3d-web 模型树「房间」页签的移植，ADR 0068）：顶部搜索框 + 刷新 + 在册数，
//! 取树失败的节点逐条列在树上方，下面是虚拟滚动的 `tree_row_ui` 行。
//!
//! 行的手势与模型树（`tree.rs`）完全一致：点箭头展开 / 折叠、点眼睛只切可见性、点行选中
//! （Ctrl / Shift）、双击展开、右键落在选择集外才重设选中。不同的只是行的身份是
//! [`RoomNodeId`] 而不是 refno：分组行（专业 / 单元类型 / 其他构件）没有 refno，所以这里发的是
//! `Cmd::RoomTree(..)`，由宿主收齐节点下的 refno 再转成既有的 `ModelAction`。
//!
//! 库供数 / 老服务端 / 房间体制未就绪都是一句明白的注释态（`RoomTreeVm::Unavailable`），不是灰页。

use egui::{Id, Margin, RichText, ScrollArea, TextEdit, Ui, UiBuilder};
use egui_phosphor::regular as ph;

use super::Pane;
use crate::room_tree::{
    LARGE_LOAD_CONFIRM_THRESHOLD, RoomNodeId, RoomNodeKind, RoomTreeAction, SelectModifiers,
};
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, space};
use crate::style::widgets::{self, Eye, PaneNote, PaneState, RowIcon, RowTag, TreeRow};
use crate::vm::{RoomTreeDataVm, RoomTreeRowVm, RoomTreeVm, RowVisibility, WorkbenchVm};
use crate::{Cmd, ModelAction};

/// 搜索框的绘制状态。过滤词的真值在宿主（它按过滤词展平行），这里只是输入框里的草稿：
/// 宿主那份变了（「在房间树查看」把挡住目标的过滤词清掉）就把输入框跟过去。
#[derive(Default)]
pub struct State {
    draft: String,
    /// 上一次与宿主对齐过的过滤词：只有宿主那份和它不同时才覆盖草稿，免得敲一半被上一帧的回执冲掉。
    synced: String,
}

pub fn show(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    match &vm.room_tree {
        // 第一次画出来才拉根清单（plant3d-web：页签不在前台不拉数据）。这一帧先画成加载态，
        // 宿主处理命令后下一帧就是 Loading，不会反复发。
        RoomTreeVm::Idle => {
            cmds.push(Cmd::RoomTree(RoomTreeAction::Load));
            note(
                ui,
                t,
                d,
                PaneState::Loading,
                ph::SPINNER,
                "正在拉取在册房间…",
                None,
                false,
            );
        }
        RoomTreeVm::Loading => {
            note(
                ui,
                t,
                d,
                PaneState::Loading,
                ph::SPINNER,
                "正在拉取在册房间…",
                None,
                false,
            );
        }
        // 不可用是一句注释而不是故障，但房间体制会从「正在就绪」走到就绪，所以照样给一枚「重试」。
        RoomTreeVm::Unavailable(reason) => {
            if note(
                ui,
                t,
                d,
                PaneState::Empty,
                ph::TREE_VIEW,
                reason,
                None,
                true,
            ) {
                cmds.push(Cmd::RoomTree(RoomTreeAction::Load));
            }
        }
        RoomTreeVm::Failed(reason) => {
            if note(
                ui,
                t,
                d,
                PaneState::Error,
                ph::WARNING,
                "在册房间清单取数失败",
                Some(reason),
                true,
            ) {
                cmds.push(Cmd::RoomTree(RoomTreeAction::Load));
            }
        }
        RoomTreeVm::Ready(data) => {
            egui::Frame::new().fill(t.bg_panel).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.spacing_mut().item_spacing.y = 0.0;
                header(ui, t, d, data, state, cmds);
                body(ui, t, d, vm, data, cmds);
            });
        }
    }
}

/// 搜索框 + 刷新 + 状态行 + 失败条。
fn header(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    data: &RoomTreeDataVm,
    state: &mut State,
    cmds: &mut Vec<Cmd>,
) {
    if data.filter != state.synced {
        state.draft = data.filter.clone();
        state.synced = data.filter.clone();
    }
    egui::Frame::new()
        .fill(t.bg_header)
        .inner_margin(Margin::symmetric(space::S2 as i8, space::S1 as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(d.px(4.0), space::S1);
            ui.horizontal(|ui| {
                let tool_w = d.px(26.0);
                let tools = if state.draft.is_empty() { 1.0 } else { 2.0 };
                let hint = format!("搜索房间号 / 名称（在册 {} 间）", data.total);
                let resp = ui.add(
                    TextEdit::singleline(&mut state.draft)
                        .id(Id::new("room-tree-search"))
                        .desired_width((ui.available_width() - tool_w * tools).max(d.px(80.0)))
                        .font(Font::meta(d))
                        .background_color(t.bg_input)
                        .text_color(t.text_primary)
                        .hint_text(RichText::new(hint).color(t.text_muted)),
                );
                if resp.changed() {
                    state.synced = state.draft.clone();
                    cmds.push(Cmd::RoomTree(RoomTreeAction::SetFilter(
                        state.draft.clone(),
                    )));
                }
                if !state.draft.is_empty()
                    && ui
                        .add(widgets::tool_btn(t, d, ph::X, false))
                        .on_hover_text("清空搜索")
                        .clicked()
                {
                    state.draft.clear();
                    state.synced.clear();
                    cmds.push(Cmd::RoomTree(RoomTreeAction::SetFilter(String::new())));
                }
                if ui
                    .add(widgets::tool_btn(t, d, ph::ARROWS_CLOCKWISE, false))
                    .on_hover_text("重新拉取在册房间（丢掉已取的树）")
                    .clicked()
                {
                    cmds.push(Cmd::RoomTree(RoomTreeAction::Refresh));
                }
            });
            let status = if data.matched == data.total {
                format!("在册 {} 间房", data.total)
            } else {
                format!("匹配 {} / {} 间房", data.matched, data.total)
            };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = d.px(4.0);
                ui.label(
                    RichText::new(status)
                        .font(Font::micro(d))
                        .color(t.text_muted),
                );
                if let Some(note) = &data.note {
                    ui.label(
                        RichText::new(format!("· {note}"))
                            .font(Font::micro(d))
                            .color(t.warn),
                    );
                }
                ui.label(
                    RichText::new("· 展开一间房看 专业 → 最小交付单元 → 构件")
                        .font(Font::micro(d))
                        .color(t.text_muted),
                );
            });
            for error in &data.errors {
                let line = format!("{}：{}", error.label, error.message);
                ui.add(
                    egui::Label::new(RichText::new(&line).font(Font::micro(d)).color(t.danger))
                        .truncate(),
                )
                .on_hover_text(line);
            }
        });
}

fn body(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    data: &RoomTreeDataVm,
    cmds: &mut Vec<Cmd>,
) {
    let rows = &data.rows;
    if rows.is_empty() {
        let text = if data.total == 0 {
            "服务端没有在册房间"
        } else {
            "没有匹配的房间"
        };
        note(ui, t, d, PaneState::Empty, ph::TREE_VIEW, text, None, false);
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
            // 同模型树：`show_rows` 拿外层 ui 的 item_spacing.y 折算每行占位，设在闭包里就晚了。
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut area = ScrollArea::vertical()
                .id_salt("room-tree-rows")
                .auto_shrink([false, false]);
            if let Some(y) = reveal_offset(ui, d, rows, data.reveal.as_ref()) {
                area = area.vertical_scroll_offset(y);
            }
            // 模型动作要有真的渲染器才做得成，独立壳里不摆这几项（与模型树同一条房规）。
            let live = vm.view3d.as_ref().is_some_and(|v| v.live);
            area.show_rows(ui, d.row_h(), rows.len(), |ui, range| {
                for row in &rows[range] {
                    row_ui(ui, t, d, row, live, cmds);
                }
            });
        });
}

fn row_icon(row: &RoomTreeRowVm) -> &'static str {
    match row.kind {
        RoomNodeKind::Room => ph::DOOR_OPEN,
        RoomNodeKind::Spec => ph::FOLDER,
        RoomNodeKind::UnitType => ph::SQUARES_FOUR,
        RoomNodeKind::Others => ph::DOTS_THREE_OUTLINE,
        RoomNodeKind::Tube => ph::LINE_SEGMENT,
        RoomNodeKind::Unit | RoomNodeKind::OtherNoun | RoomNodeKind::Element => {
            super::noun_icon(&row.noun)
        }
    }
}

fn row_ui(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    row: &RoomTreeRowVm,
    live: bool,
    cmds: &mut Vec<Cmd>,
) {
    let meta = if row.loading {
        "加载中…"
    } else {
        row.meta.as_str()
    };
    let tags: Vec<RowTag<'_>> = row
        .tags
        .iter()
        .map(|tag| RowTag {
            icon: None,
            text: &tag.text,
            tone: tag.tone,
        })
        .collect();
    // 行 id 按节点身份定死（理由同模型树：虚拟滚动下自动 id 是「第几行」，行序一变右键菜单就串行）。
    let id = Id::new(("wb-room-tree-row", &row.id));
    let out = ui
        .scope_builder(UiBuilder::new().id(id), |ui| {
            widgets::tree_row_ui(
                ui,
                t,
                d,
                TreeRow {
                    depth: row.depth as usize,
                    icon: RowIcon::Glyph(row_icon(row)),
                    label: &row.label,
                    meta,
                    selected: row.selected,
                    primary: row.primary,
                    expandable: row.expandable,
                    // 直段行的眼睛是逐段的（按所属 BRAN + 直段身份读场景，阶段四）；没有实时渲染器时整列不出现。
                    eye: if live {
                        row.visibility.map(eye_of)
                    } else {
                        None
                    },
                    tone: Status::Neutral,
                    tags: &tags,
                },
            )
        })
        .inner;
    if !row.hover.is_empty() && row.hover != row.label {
        out.response.clone().on_hover_text(&row.hover);
    }
    let accessible_label = format!("room-tree {:?}; label={}", row.kind, row.label);
    ui.ctx().accesskit_node_builder(out.response.id, |node| {
        node.set_role(egui::accesskit::Role::TreeItem);
        node.set_label(accessible_label);
        if let Some(expanded) = row.expandable {
            node.set_expanded(expanded);
        }
    });
    let hit = |r: Option<egui::Rect>| {
        r.is_some_and(|r| {
            out.response
                .interact_pointer_pos()
                .is_some_and(|p| r.contains(p))
        })
    };
    let on_caret = hit(out.caret_rect);
    if hit(out.eye_rect) {
        if out.response.clicked() {
            cmds.push(Cmd::RoomTree(RoomTreeAction::SetVisible {
                id: row.id.clone(),
                visible: row.next_visible,
            }));
        }
    } else if out.response.double_clicked() && row.expandable.is_some() && !on_caret {
        cmds.push(Cmd::RoomTree(RoomTreeAction::ToggleExpand(row.id.clone())));
    } else if out.response.clicked() {
        if on_caret && !out.response.double_clicked() {
            cmds.push(Cmd::RoomTree(RoomTreeAction::ToggleExpand(row.id.clone())));
        } else if !out.response.double_clicked() {
            let mods = ui.input(|i| i.modifiers);
            cmds.push(Cmd::RoomTree(RoomTreeAction::Select {
                id: row.id.clone(),
                modifiers: SelectModifiers {
                    shift: mods.shift,
                    command: mods.command,
                },
            }));
        }
    }
    // 右键落在选择集外才重设选中，落在集内保持整批（同模型树）。
    if out.response.secondary_clicked() && !row.selected {
        cmds.push(Cmd::RoomTree(RoomTreeAction::Select {
            id: row.id.clone(),
            modifiers: SelectModifiers::default(),
        }));
    }
    out.response
        .context_menu(|ui| row_menu(ui, d, row, live, cmds));
}

/// Vm 的四态可见性翻成组件层的画法（同 `tree::eye_of`）。
fn eye_of(vis: RowVisibility) -> Eye {
    match vis {
        RowVisibility::Unloaded => Eye::Unloaded,
        RowVisibility::Shown => Eye::Shown,
        RowVisibility::Hidden => Eye::Hidden,
        RowVisibility::Partial => Eye::Partial,
    }
}

/// 行的右键菜单（照 plant3d-web `RoomTreePanel.vue`，计划 D5）：聚焦 · 隔离 · 取消隔离 · 显示 / 隐藏 ·
/// 加载模型 · 查看关联房间 · 查看属性；房间行多「聚焦房间」「显示房间模型」；直段行是聚焦 · 显示 / 隐藏
/// （只动这一段，阶段四）· 查看关联房间 / 属性（看所属 BRAN），不摆隔离与加载。模型动作吃 `live` 门禁（没有
/// 渲染器就不摆），数据类的两项不吃。
fn row_menu(ui: &mut Ui, d: Density, row: &RoomTreeRowVm, live: bool, cmds: &mut Vec<Cmd>) {
    ui.set_min_width(d.px(200.0));
    let id = row.id.clone();
    let is_tube = row.kind == RoomNodeKind::Tube;
    let mut grouped = false;
    if let Some(open) = row.expandable {
        let (icon, label) = if open {
            (ph::CARET_DOWN, "折叠")
        } else {
            (ph::CARET_RIGHT, "展开")
        };
        if ui.button(format!("{icon}  {label}")).clicked() {
            cmds.push(Cmd::RoomTree(RoomTreeAction::ToggleExpand(id.clone())));
            ui.close();
        }
        grouped = true;
    }
    if live {
        if grouped {
            ui.separator();
        }
        let focus = ui.button(format!("{}  聚焦", ph::CROSSHAIR_SIMPLE));
        if focus.clicked() {
            cmds.push(Cmd::RoomTree(RoomTreeAction::Focus(id.clone())));
            ui.close();
        }
        if is_tube {
            focus.on_hover_text("按这段直管自己的世界盒取景，不要求已加载");
        } else {
            focus.on_hover_text(
                "对准这一支下已加载几何的并集盒；一个都没加载时相机不动，先「加载模型」",
            );
        }
        if row.kind == RoomNodeKind::Room
            && let Some(room) = row.refno
        {
            if ui
                .button(format!("{}  聚焦房间（房间视图）", ph::DOOR_OPEN))
                .on_hover_text("隔离这间房的成员、半透明房间体、相机对准房间，详情进「房间」页签")
                .clicked()
            {
                cmds.push(Cmd::FocusRoom(room));
                ui.close();
            }
            if ui
                .button(format!("{}  显示房间模型", ph::CUBE))
                .on_hover_text("把这间房自己的墙面板几何取回来显示，再取景到它")
                .clicked()
            {
                cmds.push(Cmd::ShowRoomModel(room));
                ui.close();
            }
        }
        if !is_tube {
            if ui
                .button(format!("{}  隔离", ph::SELECTION))
                .on_hover_text("这一支下的构件（单元级及以上连所属 BRAN 整条）留实体，其余隐藏")
                .clicked()
            {
                cmds.push(Cmd::RoomTree(RoomTreeAction::Isolate(id.clone())));
                ui.close();
            }
            if ui
                .button(format!("{}  取消隔离", ph::SELECTION_SLASH))
                .clicked()
            {
                cmds.push(Cmd::Model(ModelAction::ExitIsolate));
                ui.close();
            }
        }
        ui.separator();
        let show = ui.button(format!("{}  显示", ph::EYE));
        if show.clicked() {
            cmds.push(Cmd::RoomTree(RoomTreeAction::SetVisible {
                id: id.clone(),
                visible: true,
            }));
            ui.close();
        }
        let hide = ui.button(format!("{}  隐藏", ph::EYE_SLASH));
        if hide.clicked() {
            cmds.push(Cmd::RoomTree(RoomTreeAction::SetVisible {
                id: id.clone(),
                visible: false,
            }));
            ui.close();
        }
        if is_tube {
            const SEGMENT_HINT: &str =
                "只动这一段直段（要先显示所属 BRAN）；之后对整根 BRAN 的显示 / 隐藏会把它一并盖掉";
            show.on_hover_text(SEGMENT_HINT);
            hide.on_hover_text(SEGMENT_HINT);
        }
        if !is_tube {
            let count = row
                .count
                .map(|count| format!("（{count} 个构件）"))
                .unwrap_or_default();
            if ui
                .button(format!("{}  加载模型{count}", ph::DOWNLOAD_SIMPLE))
                .on_hover_text(format!(
                    "显示这一支下全部构件的模型并取景；超过 {LARGE_LOAD_CONFIRM_THRESHOLD} 个先确认"
                ))
                .clicked()
            {
                cmds.push(Cmd::RoomTree(RoomTreeAction::LoadModels {
                    id: id.clone(),
                    confirmed: false,
                }));
                ui.close();
            }
        }
        grouped = true;
    }
    if let Some(refno) = row.refno
        && matches!(
            row.kind,
            RoomNodeKind::Element | RoomNodeKind::Unit | RoomNodeKind::Tube
        )
    {
        if grouped {
            ui.separator();
        }
        let scope_hint = is_tube.then_some("直段没有自己的 refno：作用于所属 BRAN");
        let rooms = ui.button(format!("{}  查看关联房间", ph::DOOR_OPEN));
        if rooms.clicked() {
            cmds.push(Cmd::ShowRooms(refno));
            ui.close();
        }
        if let Some(hint) = scope_hint {
            rooms.on_hover_text(hint);
        }
        let props = ui.button(format!("{}  查看属性", ph::SLIDERS_HORIZONTAL));
        if props.clicked() {
            cmds.push(Cmd::SelectElement(refno));
            cmds.push(Cmd::FocusPane(Pane::Properties));
            ui.close();
        }
        if let Some(hint) = scope_hint {
            props.on_hover_text(hint);
        }
        if ui
            .button(format!("{}  复制 REFNO", ph::COPY_SIMPLE))
            .clicked()
        {
            ui.ctx().copy_text(refno.to_string());
            ui.close();
        }
    }
}

/// 「加载模型」超过阈值的确认框（与 plant3d-web 同阈值、同文案）。窗口级：页签切走了它也在。
/// 关窗等于取消——右上角那个叉与「取消」必须是同一件事。
pub fn confirm_dialog(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    cmds: &mut Vec<Cmd>,
) {
    let RoomTreeVm::Ready(data) = &vm.room_tree else {
        return;
    };
    let Some(confirm) = &data.confirm else {
        return;
    };
    let mut open = true;
    let accept_label = format!("加载 {} 个", confirm.count);
    egui::Window::new("加载数量较多")
        .id(Id::new("plant-room-tree-load-confirm"))
        .collapsible(false)
        .resizable(false)
        .default_width(440.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.label(
                RichText::new(format!(
                    "「加载模型」（{}）将加载 {} 个模型（超过 {} 个），可能要等一会儿。继续吗？",
                    confirm.label, confirm.count, LARGE_LOAD_CONFIRM_THRESHOLD
                ))
                .color(t.text_primary),
            );
            ui.add_space(d.px(6.0));
            ui.label(
                RichText::new("进度在状态栏「加载模型网格」那一格；装完自动取景到这一支。")
                    .small()
                    .color(t.text_muted),
            );
            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add(
                        widgets::button(t, d, &accept_label)
                            .icon(ph::DOWNLOAD_SIMPLE)
                            .primary(),
                    )
                    .clicked()
                {
                    cmds.push(Cmd::RoomTree(RoomTreeAction::LoadModels {
                        id: confirm.id.clone(),
                        confirmed: true,
                    }));
                }
                if ui.add(widgets::button(t, d, "取消")).clicked() {
                    cmds.push(Cmd::RoomTree(RoomTreeAction::CancelLoadModels));
                }
            });
        });
    if !open {
        cmds.push(Cmd::RoomTree(RoomTreeAction::CancelLoadModels));
    }
}

/// 把目标行摆到视野正中所需的滚动偏移（同 `tree::reveal_offset`，行按节点身份找）。
fn reveal_offset(
    ui: &Ui,
    d: Density,
    rows: &[RoomTreeRowVm],
    reveal: Option<&RoomNodeId>,
) -> Option<f32> {
    let id = reveal?;
    let idx = rows.iter().position(|r| &r.id == id)?;
    let row_h = d.row_h();
    let view_h = ui.available_height();
    let max = (rows.len() as f32 * row_h - view_h).max(0.0);
    Some((idx as f32 * row_h + row_h / 2.0 - view_h / 2.0).clamp(0.0, max))
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
