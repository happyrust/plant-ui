//! 模型树视图（M1-2）：虚拟化滚动 + 懒加载展开。
//!
//! 万行级靠两层配合：App 侧只在结构变化时展平可见行（Vm 逐帧只读），
//! 这里用 `show_rows` 只画视口内的行；行内文字裁剪由 `tree_row_ui` 保证，
//! 长 PDMS 名不会撑破行。

use egui::text::{LayoutJob, TextFormat};
use egui::{Id, ScrollArea, Ui, UiBuilder};
use egui_phosphor::regular as ph;

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, space};
use crate::style::widgets::{self, Eye, PaneNote, PaneState, RowIcon, TreeRow};
use crate::vm::{RoomVm, RowVisibility, Selection, TreeRowVm, TreeVm, WorkbenchVm};
use crate::{Cmd, ModelAction};

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
                    // 模型动作要有真的渲染器才做得成，独立壳里不摆这几项。
                    let live = vm.view3d.is_some_and(|v| v.live);
                    let primary = vm.selection.primary();
                    area.show_rows(ui, d.row_h(), rows.len(), |ui, range| {
                        for row in &rows[range] {
                            let meta = if row.loading {
                                "加载中…"
                            } else {
                                row.noun.as_str()
                            };
                            // 行 id 按 refno 定死：虚拟滚动下自动 id 是「第几行」，
                            // 行序一变同一个 id 就落到另一个元素上，右键菜单会跟着串行。
                            //
                            // 这里要的是 `UiBuilder::id` 而不是 `push_id`：`push_id` 只
                            // 定住子 Ui 的 `id`，它内部控件仍从 `unique_id` 取号，而
                            // `unique_id = id.with(父 Ui 的自动计数)` 又把行序混了回来。
                            // 展开 / 折叠会让下方每一行都换号，egui 的
                            // `warn_if_rect_changes_id` 逐行描红框（调试构建默认开）。
                            let id = Id::new(("wb-tree-row", row.refno));
                            let out = ui
                                .scope_builder(UiBuilder::new().id(id), |ui| {
                                    widgets::tree_row_ui(
                                        ui,
                                        t,
                                        d,
                                        TreeRow {
                                            depth: row.depth as usize,
                                            icon: RowIcon::Glyph(super::noun_icon(&row.noun)),
                                            label: &row.name,
                                            meta,
                                            selected: vm.selection.contains(row.refno),
                                            primary: primary == Some(row.refno),
                                            expandable: row.expandable,
                                            // 没有实时渲染器时整列不出现，同右键菜单里那几项。
                                            eye: live.then(|| eye_of(row.visibility)),
                                            tone: Status::Neutral,
                                            tags: &[],
                                        },
                                    )
                                })
                                .inner;
                            // 自动化只认真正的树行：AccessKit 角色负责排除命令历史、
                            // 属性面板和状态栏，标签里的 refno 则提供跨重命名稳定身份。
                            // 当前名称仍随数据刷新，便于同时断言 rename 的新旧语义。
                            let accessible_label =
                                tree_item_accessible_label(row.refno, row.name.as_str());
                            ui.ctx().accesskit_node_builder(out.response.id, |node| {
                                node.set_role(egui::accesskit::Role::TreeItem);
                                node.set_label(accessible_label);
                                if let Some(expanded) = row.expandable {
                                    node.set_expanded(expanded);
                                }
                            });
                            // 点箭头只折叠 / 展开；点眼睛只切可见性；点行其他区域选中；
                            // 双击整行也展开。`clicked` / `double_clicked` 都只认主键，
                            // 右键落不进这几支——右键在下面单独处理。
                            let hit = |r: Option<egui::Rect>| {
                                r.is_some_and(|r| {
                                    out.response
                                        .interact_pointer_pos()
                                        .is_some_and(|p| r.contains(p))
                                })
                            };
                            let on_caret = hit(out.caret_rect);
                            if hit(out.eye_rect) {
                                // 眼睛这一格只切可见性：不选中也不展开，双击不例外
                                // （连点两下就是切两次，开关本来就该这样）。
                                // 只作用于这一行——整批走右键菜单与视口的 S / H。
                                if out.response.clicked() {
                                    // 方向由宿主算好挂在行上：图标跟的是实际渲染
                                    // 结果，指令还在路上时它不动，这里不能拿它反推。
                                    cmds.push(Cmd::Model(ModelAction::SetVisible {
                                        refnos: vec![row.refno],
                                        visible: row.next_visible,
                                    }));
                                }
                            } else if out.response.double_clicked()
                                && row.expandable.is_some()
                                && !on_caret
                            {
                                cmds.push(Cmd::ToggleExpand(row.refno));
                            } else if out.response.clicked() {
                                if on_caret && !out.response.double_clicked() {
                                    cmds.push(Cmd::ToggleExpand(row.refno));
                                } else if !out.response.double_clicked() {
                                    let mods = ui.input(|i| i.modifiers);
                                    cmds.push(click_selection(
                                        &vm.selection,
                                        rows,
                                        row.refno,
                                        mods,
                                    ));
                                }
                            }
                            // 右键落在选择集**外**才重设选中；落在集内保持整批，
                            // 否则「选中三个再右键其中一个」会把批量选择打散，
                            // 菜单上那句「隐藏所选 3 项」也就无从谈起。
                            if out.response.secondary_clicked() && !vm.selection.contains(row.refno)
                            {
                                cmds.push(Cmd::SelectElement(row.refno));
                            }
                            out.response.context_menu(|ui| {
                                row_menu(
                                    ui,
                                    t,
                                    d,
                                    row,
                                    &vm.selection,
                                    &vm.rooms,
                                    live,
                                    vm.regen_busy,
                                    cmds,
                                )
                            });
                        }
                    });
                });
        }
    }
}

fn tree_item_accessible_label(refno: aios_core::RefU64, name: &str) -> String {
    // E3D 形式自带等号，前缀里就不再补一个，否则读屏念出来是「refno 等于 等于」。
    format!("refno {}; name={name}", refno.to_e3d_id())
}

/// Vm 的四态可见性翻成组件层的画法。两个枚举是同一件事在两层的说法——
/// 组件层不认识 `vm`，见 `widgets::Eye`。
fn eye_of(vis: RowVisibility) -> Eye {
    match vis {
        RowVisibility::Unloaded => Eye::Unloaded,
        RowVisibility::Shown => Eye::Shown,
        RowVisibility::Hidden => Eye::Hidden,
        RowVisibility::Partial => Eye::Partial,
    }
}

/// 一次左键点击落到什么选择集上。
///
/// 修饰键在绘制层判、结果整体交出：Shift 区间要按**可见行序**取，那个序只有这里
/// 手上的 `rows` 才有；宿主拿到一个 refno 是复原不出用户跨过了哪几行的。
///
/// Ctrl+Shift（在已有选择上追加一段）不做，落到 Shift 一支：多一档语义就得多一个
/// 锚，而这一档在模型树上少见到不值得为它养第二个锚。
fn click_selection(
    current: &Selection,
    rows: &[TreeRowVm],
    refno: aios_core::RefU64,
    mods: egui::Modifiers,
) -> Cmd {
    let mut next = current.clone();
    if mods.shift {
        let order: Vec<_> = rows.iter().map(|r| r.refno).collect();
        next.range(&order, refno);
    } else if mods.command {
        next.toggle(refno);
    } else {
        next = Selection::single(refno);
    }
    Cmd::SetSelection(next)
}

/// 行的右键菜单。
///
/// 作用对象在这里就地算死，不回头读 `vm.selection`：右键落在选择集外时，重设选中的
/// `Cmd` 要下一帧才被宿主处理，而菜单这一帧就画出来了——读 Vm 会让首帧的菜单作用到
/// 上一批元素上。
///
/// 「隐藏全部」是全局动作，只在工具栏出现：每个节点的菜单里重复一遍，看着像
/// 「隐藏这一支的全部」，其实不是。
#[allow(clippy::too_many_arguments)]
fn row_menu(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    row: &TreeRowVm,
    selection: &Selection,
    rooms: &RoomVm,
    live: bool,
    regen_busy: bool,
    cmds: &mut Vec<Cmd>,
) {
    // 右键落在集内 = 对整批操作；落在集外 = 只对这一行。
    let targets: Vec<_> = if selection.contains(row.refno) {
        selection.to_vec()
    } else {
        vec![row.refno]
    };

    let mut grouped = false;
    if let Some(open) = row.expandable {
        let (icon, label) = if open {
            (ph::CARET_DOWN, "折叠")
        } else {
            (ph::CARET_RIGHT, "展开")
        };
        if ui.button(format!("{icon}  {label}")).clicked() {
            cmds.push(Cmd::ToggleExpand(row.refno));
            ui.close();
        }
        grouped = true;
    }
    element_menu(
        ui, t, d, row.refno, &targets, rooms, live, regen_busy, cmds, grouped,
    );
}

/// 元素菜单的公共部分：模型动作 + 房间归属 + 复制 REFNO。模型树的行菜单与
/// 三维视口的右键菜单是同一件事的两个入口，内容必须一致，所以收在一处。
///
/// `refno` 是右键落点那一个元素（定位与房间归属对它），`targets` 是批量动作的
/// 作用集（显示 / 隐藏 / 复制对它们）。
#[allow(clippy::too_many_arguments)]
pub(super) fn element_menu(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    refno: aios_core::RefU64,
    targets: &[aios_core::RefU64],
    rooms: &RoomVm,
    live: bool,
    regen_busy: bool,
    cmds: &mut Vec<Cmd>,
    mut grouped: bool,
) {
    // 「1 项」是废话，只有真成批时才报数。
    let count = (targets.len() > 1).then(|| format!(" {} 项", targets.len()));
    let suffix = count.as_deref().unwrap_or_default();

    if live {
        if grouped {
            ui.separator();
        }
        for (icon, label, action) in [
            (
                ph::EYE,
                format!("显示模型{suffix}"),
                ModelAction::SetVisible {
                    refnos: targets.to_vec(),
                    visible: true,
                },
            ),
            (
                ph::EYE_SLASH,
                format!("隐藏模型{suffix}"),
                ModelAction::SetVisible {
                    refnos: targets.to_vec(),
                    visible: false,
                },
            ),
            // 定位不带计数：它只把相机带到右键的这一行跟前，见 `ModelAction::Focus`。
            (
                ph::CROSSHAIR_SIMPLE,
                "定位模型".to_owned(),
                ModelAction::Focus(refno),
            ),
        ] {
            if ui.button(format!("{icon}  {label}")).clicked() {
                cmds.push(Cmd::Model(action));
                ui.close();
            }
        }
        grouped = true;
    }
    // 房间归属是数据查询，不吃 live 门禁——独立壳里照样能看（计划目标 7）；
    // 点击后的隔离 / 取景由宿主按 live 自行降级。
    if grouped {
        ui.separator();
    }
    room_menu_section(ui, t, d, refno, rooms, live, cmds);
    // 自成一组。上面那几项改的是「我看不看得见」，这一项改的是模型库里的产物
    // ——删掉的几何只能重新算回来。挤进同一组里，手滑的代价差得太远。
    //
    // 吃 `live` 门禁：跑完要把结果显示出来，没有渲染器的壳里点了只能改库、
    // 看不见结果，而现有约定是「不接线就不显示」，不摆灰色占位。
    if live {
        ui.separator();
        let label = format!("{}  重新生成模型{suffix}", ph::ARROWS_CLOCKWISE);
        if ui
            .add_enabled(!regen_busy, egui::Button::new(label))
            .on_disabled_hover_text("已经有一趟重新生成在跑了")
            .clicked()
        {
            cmds.push(Cmd::RegenerateModels {
                targets: targets.to_vec(),
            });
            ui.close();
        }
    }
    ui.separator();
    if ui
        .button(format!("{}  复制 REFNO{suffix}", ph::COPY_SIMPLE))
        .clicked()
    {
        // 一行一个，方便直接贴进命令行的 `=<参考号>`；`RefU64` 的 FromStr
        // 认 `_` 也认 `/`，还会剃掉前导 `=`，所以贴回来一定认得。
        let text = targets
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        ui.ctx().copy_text(text);
        ui.close();
    }
}

/// 「查看所属房间 ▸」子菜单（S13 稿）。
///
/// 归属数据随选中预取（计划决定 5），这里只读现成的 `vm.rooms`；数据还没
/// 追上这一行时（右键刚把选中切过来、或右键落在多选集里非主选中的行上）
/// 老老实实说「查询中…」，不拿别的元素的归属顶数。
fn room_menu_section(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    refno: aios_core::RefU64,
    rooms: &RoomVm,
    live: bool,
    cmds: &mut Vec<Cmd>,
) {
    ui.menu_button(format!("{}  查看所属房间", ph::DOOR_OPEN), |ui| {
        ui.set_min_width(d.px(220.0));
        room_menu_items(ui, t, d, refno, rooms, live, cmds);
        ui.separator();
        // 浏览器看的是全部房间，不依赖这个元素的归属，所以哪个分支里都有它。
        if ui
            .button(format!("{}  打开房间浏览器…", ph::LAYOUT))
            .clicked()
        {
            cmds.push(Cmd::OpenRoomBrowser);
            ui.close();
        }
    });
}

/// 子菜单里随元素归属而变的那部分（房间行 + 显示房间模型 + 页签入口 / 三种占位态）。
fn room_menu_items(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    refno: aios_core::RefU64,
    rooms: &RoomVm,
    live: bool,
    cmds: &mut Vec<Cmd>,
) {
    let data = match rooms {
        RoomVm::Ready(data) | RoomVm::Loading(Some(data)) if data.refno == refno => data,
        RoomVm::Failed(_) => {
            ui.add_enabled(false, egui::Button::new("归属查询失败，详见日志"));
            return;
        }
        _ => {
            ui.add_enabled(false, egui::Button::new("查询中…"));
            return;
        }
    };
    if data.relations.is_empty() {
        ui.add_enabled(false, egui::Button::new("无所属房间"));
        return;
    }
    for (i, rel) in data.relations.iter().enumerate() {
        let mut job = LayoutJob::default();
        job.append(
            &rel.room_num,
            0.0,
            TextFormat {
                font_id: Font::strong(d),
                color: t.text_primary,
                ..Default::default()
            },
        );
        // 首条即最强（数据层已按 ADR-010 §5 排好），只有它标「主归属」。
        if i == 0 {
            job.append(
                "  主归属",
                0.0,
                TextFormat {
                    font_id: Font::meta(d),
                    color: t.success,
                    ..Default::default()
                },
            );
        }
        if rel.via_member {
            job.append(
                "  来自成员",
                0.0,
                TextFormat {
                    font_id: Font::meta(d),
                    color: t.text_muted,
                    ..Default::default()
                },
            );
        }
        job.append(
            &format!(
                "\n{} · 顶点 {}/8 · 距中心 {}",
                rel.room_code.as_deref().unwrap_or("—"),
                rel.inside_count,
                super::room::format_dist(rel.center_dist),
            ),
            0.0,
            TextFormat {
                font_id: Font::mono_micro(d),
                color: t.text_muted,
                ..Default::default()
            },
        );
        let room = rel.room;
        let resp = ui.add_enabled(room.is_some(), egui::Button::new(job));
        if resp.clicked()
            && let Some(room) = room
        {
            cmds.push(Cmd::FocusRoom(room));
            cmds.push(Cmd::ShowRooms(refno));
            ui.close();
        }
        resp.on_disabled_hover_text("面板不在册（陈旧归属边），无法聚焦");
    }
    ui.separator();
    // 只给主归属那一间：右键菜单答的是「这个构件在哪」，一键直达最强那间房就够了。
    // 跨房构件要看第二间，去「房间」页签点中它再按同名芯片——那里本来就摆着
    // 全部归属，不必把这份列表在菜单里再展开一遍。
    if live && let Some(primary) = data.relations.first() {
        let resp = ui.add_enabled(
            primary.room.is_some(),
            egui::Button::new(format!(
                "{}  显示房间模型（{}）",
                ph::CUBE,
                primary.room_num
            )),
        );
        if resp.clicked()
            && let Some(room) = primary.room
        {
            cmds.push(Cmd::ShowRoomModel(room));
            ui.close();
        }
        resp.on_disabled_hover_text("面板不在册（陈旧归属边），取不到房间模型");
    }
    if ui
        .button(format!("{}  在「房间」页签中查看", ph::SIDEBAR_SIMPLE))
        .clicked()
    {
        cmds.push(Cmd::ShowRooms(refno));
        ui.close();
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

#[cfg(test)]
mod tests {
    use super::tree_item_accessible_label;
    use aios_core::RefU64;

    #[test]
    fn accessible_tree_identity_survives_rename_without_name_matching() {
        let refno = RefU64(0x1234);
        let before = tree_item_accessible_label(refno, "OLD-EQUI");
        let after = tree_item_accessible_label(refno, "NEW-EQUI");
        assert!(before.contains("refno =0/4660"));
        assert!(after.contains("refno =0/4660"));
        assert!(!after.contains("0_4660"));
        assert!(!after.contains("OLD-EQUI"));
        assert!(after.contains("name=NEW-EQUI"));
    }
}
