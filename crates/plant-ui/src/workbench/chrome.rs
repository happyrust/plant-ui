//! 外壳三条横栏：标题栏 38 / 命令栏 40 / 状态栏 26（S1 画板规格）。
//!
//! 状态栏只摆应用确实持有的数据（数据源状态、项目 / 库、选中元素、元素计数）；
//! 设计稿上的已应用 sesno / 文件 sesno / 待更新批次 / 待重试单元属于 gen-model
//! 侧，等 M4-4 定下数据边界后再补，宁可少一格。

use egui::{Align, Color32, CornerRadius, Layout, Margin, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius, space};
use crate::style::widgets;
use crate::vm::{ModelLoadVm, WorkbenchVm};

pub fn title_bar(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm) {
    // 栏体(Frame)与 hairline 必须严丝合缝：全局 item_spacing.y=6 会把
    // hairline 推出面板裁剪区（Panel 只比栏体高 1px）。
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::Frame::new()
        .fill(t.bg_chrome)
        .inner_margin(Margin::symmetric(space::S3 as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(d.title_bar_h());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                logo(ui, t, d);
                ui.label(
                    RichText::new("布置平台")
                        .font(Font::strong(d))
                        .color(t.text_primary),
                );
                // 连接成功前没有工程标识，这一段整体不画（不摆空芯片）。
                if !vm.project.is_empty() {
                    divider(ui, t, d);
                    ui.add(widgets::status_tag(
                        t,
                        d,
                        &format!("项目名 {}", vm.project),
                        Status::Info,
                    ));
                    ui.label(
                        RichText::new(format!("项目代号 {}", vm.project_code))
                            .font(Font::mono_meta(d))
                            .color(t.text_muted),
                    );
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(&vm.user)
                            .font(Font::meta(d))
                            .color(t.text_secondary),
                    );
                    avatar(ui, t, d, &vm.user);
                    divider(ui, t, d);
                    search_box(ui, t, d, d.px(320.0), "搜索元素、REFNO 或命令", "Ctrl K");
                });
            });
        });
    hairline(ui, t);
}

pub fn command_bar(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm, cmds: &mut Vec<Cmd>) {
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::Frame::new()
        .fill(t.bg_panel)
        .inner_margin(Margin::symmetric(space::S2 as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(d.command_bar_h());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = space::S1;

                let project = ui.add(command_menu_button(d, "项目", true));
                let project_popup = egui::Popup::menu(&project);
                open_menu_marker(ui, t, d, &project, project_popup.is_open());
                project_popup.show(|ui| {
                    ui.set_min_width(d.px(236.0));
                    if ui
                        .add(command_menu_action(
                            d,
                            ph::FOLDER_OPEN,
                            "打开项目…",
                            "选择配置",
                        ))
                        .clicked()
                    {
                        cmds.push(Cmd::OpenProjectPicker);
                        ui.close();
                    }
                    ui.separator();
                    // 没连上库时取回工作无从谈起：没有树可重查，也没有
                    // 模型可重载。禁用而不是隐藏——菜单项换位置比灰着更难找。
                    let busy = vm.get_work_busy;
                    let label = if busy {
                        "正在取回工作…"
                    } else {
                        "取回工作"
                    };
                    let get_work = ui
                        .add_enabled(
                            vm.data_source_ok && !busy,
                            command_menu_action(
                                d,
                                ph::ARROW_CLOCKWISE,
                                label,
                                if busy { "请稍候" } else { "GET WORK" },
                            ),
                        )
                        .on_disabled_hover_text(if busy {
                            "取回工作正在进行"
                        } else {
                            "连接数据源后可用"
                        });
                    if get_work.clicked() {
                        cmds.push(Cmd::GetWork);
                        ui.close();
                    }
                    // 取回工作只取界面。设计库里还躺着没应用的会话时，
                    // 这一行是唯一告诉人「该去的是另一个入口」的地方。
                    if let Some(pending) = vm.pending_sessions.filter(|n| *n > 0) {
                        ui.label(
                            RichText::new(format!(
                                "设计库还有 {pending} 个会话未应用 · 去「模型更新」"
                            ))
                            .font(Font::micro(d))
                            .color(t.text_muted),
                        );
                    }
                });

                let create = ui.add(command_menu_button(d, "创建", true));
                let create_popup = egui::Popup::menu(&create);
                open_menu_marker(ui, t, d, &create, create_popup.is_open());
                create_popup.show(|ui| {
                    ui.set_min_width(d.px(236.0));
                    if ui
                        .add(command_menu_action(
                            d,
                            ph::SHARE_NETWORK,
                            "三维数据接口",
                            "创建提资任务",
                        ))
                        .clicked()
                    {
                        cmds.push(Cmd::OpenDataPublish);
                        ui.close();
                    }
                });

                for (name, hint) in [
                    ("元件库", "元件库尚未接入当前工作台"),
                    ("插件", "插件管理尚未接入当前工作台"),
                ] {
                    ui.add_enabled(false, command_menu_button(d, name, false))
                        .on_disabled_hover_text(hint);
                }
                if ui.add(command_menu_button(d, "设置", false)).clicked() {
                    cmds.push(Cmd::OpenSettings);
                }
                ui.add_enabled(false, command_menu_button(d, "帮助", false))
                    .on_disabled_hover_text("帮助中心尚未接入当前工作台");

                ui.add_space(space::S2);
                divider(ui, t, d);
                ui.add_space(space::S2);
                for (icon, hint) in [
                    (ph::ARROW_COUNTER_CLOCKWISE, "撤销"),
                    (ph::ARROW_CLOCKWISE, "重做"),
                    (ph::FLOPPY_DISK, "保存"),
                ] {
                    ui.add_enabled(false, widgets::tool_btn(t, d, icon, false))
                        .on_disabled_hover_text(format!("{hint}功能尚未接入"));
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            vm.data_source_ok,
                            widgets::button(t, d, "模型更新")
                                .icon(ph::ARROWS_CLOCKWISE)
                                .primary(),
                        )
                        .clicked()
                    {
                        cmds.push(Cmd::OpenModelUpdate);
                    }
                });
            });
        });
    hairline(ui, t);
}

pub fn status_bar(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm) {
    ui.spacing_mut().item_spacing.y = 0.0;
    hairline(ui, t);
    egui::Frame::new()
        .fill(t.bg_chrome)
        .inner_margin(Margin::symmetric(space::S3 as i8, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(d.status_bar_h());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = space::S2;
                let (source_color, source_text) = if vm.data_source_ok {
                    (t.success, "数据源 就绪")
                } else {
                    (t.warn, "数据源 未连接")
                };
                dot(ui, d, source_color);
                ui.label(
                    RichText::new(source_text)
                        .font(Font::micro(d))
                        .color(t.text_muted),
                );
                access_point_chip(ui, t, d, vm);
                divider(ui, t, d);
                meta_icon(ui, d, ph::CURSOR, t.text_secondary);
                // 多选时报主选中加余量：状态栏这一格只有一行，列不下整批，
                // 而「属性面板在说哪一个」比「一共选了几个」更常被问到。
                match vm.selection.primary() {
                    Some(refno) => {
                        let rest = vm.selection.len() - 1;
                        let text = if rest > 0 {
                            format!("{refno} +{rest}")
                        } else {
                            refno.to_string()
                        };
                        ui.label(
                            RichText::new(text)
                                .font(Font::mono_micro(d))
                                .color(t.text_secondary),
                        )
                    }
                    None => ui.label(
                        RichText::new("未选中")
                            .font(Font::micro(d))
                            .color(t.text_muted),
                    ),
                };

                queue_count(ui, t, d, vm);

                divider(ui, t, d);
                ui.label(
                    RichText::new(format!("刷新 {}", vm.refresh_generation))
                        .font(Font::mono_micro(d))
                        .color(t.text_muted),
                );

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(vm.element_count.to_string())
                            .font(Font::mono_micro(d))
                            .color(t.text_secondary),
                    );
                    ui.label(
                        RichText::new("元素")
                            .font(Font::micro(d))
                            .color(t.text_muted),
                    );
                    if vm.model_load.is_some() {
                        ui.add_space(space::S3);
                        model_load_progress(ui, t, d, vm);
                    }
                });
            });
        });
}

fn model_load_progress(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm) {
    let Some(state) = &vm.model_load else {
        return;
    };
    let fraction = state.fraction();
    let text = match state {
        ModelLoadVm::Loading {
            label, done, total, ..
        } if *total > 0 => {
            format!("{label}  {done}/{total}  {:.0}%", fraction.unwrap() * 100.0)
        }
        _ => state.label().to_owned(),
    };
    let fill = match state {
        ModelLoadVm::Success(_) => t.success,
        ModelLoadVm::Failed(_) => t.danger,
        _ => t.accent,
    };
    ui.add(
        egui::ProgressBar::new(fraction.unwrap_or(0.0))
            .desired_width(d.px(320.0))
            .desired_height(d.px(14.0))
            .fill(fill)
            .animate(fraction.is_none())
            .text(RichText::new(text).font(Font::micro(d))),
    );
}

/// 状态栏那枚数据库芯片。点开是当前接入点的完整交代。
///
/// **连上之前也画。** 「连不上」正是最需要知道自己冲着谁去的那一刻，而那时候摆的是
/// 配置里的库名而不是项目名——项目名要连上才算数，不许拿配置里的字面值冒充。
fn access_point_chip(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm) {
    let ap = &vm.access_point;
    let connected = !vm.project.is_empty();
    if !connected && ap.db_url.trim().is_empty() {
        return;
    }
    divider(ui, t, d);
    let icon = ui.label(
        RichText::new(ph::DATABASE)
            .font(egui::FontId::new(
                d.px(12.0),
                egui::FontFamily::Proportional,
            ))
            .color(if connected { t.accent } else { t.text_muted }),
    );
    let response = if connected {
        let name = ui.label(
            RichText::new(format!("项目名 {}", vm.project))
                .font(Font::micro(d))
                .color(t.text_secondary),
        );
        let code = ui.label(
            RichText::new(format!("项目代号 {}", vm.project_code))
                .font(Font::mono_micro(d))
                .color(t.text_muted),
        );
        icon.union(name).union(code)
    } else {
        let label = if ap.database.trim().is_empty() {
            "接入点".to_owned()
        } else {
            format!("接入点 {}", ap.database)
        };
        let name = ui.label(
            RichText::new(label)
                .font(Font::micro(d))
                .color(t.text_muted),
        );
        icon.union(name)
    };
    let response = response
        .interact(Sense::click())
        .on_hover_text("点击查看当前接入点");
    egui::Popup::menu(&response).show(|ui| access_point_detail(ui, t, d, ap));
}

fn access_point_detail(ui: &mut Ui, t: &Tokens, d: Density, ap: &crate::vm::AccessPointVm) {
    ui.set_min_width(d.px(420.0));
    for (label, value) in [
        ("模型本体库", ap.db_url.as_str()),
        ("命名空间", ap.namespace.as_str()),
        ("数据库", ap.database.as_str()),
        ("MDB", ap.mdb.as_str()),
        ("用户", ap.user.as_str()),
        ("模型服务", ap.model_api_url.as_str()),
        ("数据中心", ap.data_api_url.as_str()),
    ] {
        access_point_row(ui, t, d, label, value);
    }
    ui.separator();
    // 这一行是整块面板存在的理由：没有它，静默回落到工作目录 `DbOption.toml` 的那次
    // 启动与正常启动在界面上一模一样。
    access_point_row(ui, t, d, "配置来自", &ap.source);
}

fn access_point_row(ui: &mut Ui, t: &Tokens, d: Density, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(d.px(72.0));
            ui.label(
                RichText::new(label)
                    .font(Font::micro(d))
                    .color(t.text_muted),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (text, color) = if value.trim().is_empty() {
                ("未配置".to_owned(), t.text_muted)
            } else {
                (value.to_owned(), t.text_secondary)
            };
            ui.label(RichText::new(text).font(Font::mono_micro(d)).color(color));
        });
    });
}

/// 队列计数。队列视图被折起时由它叫住人，**不重复面板上的明细**——同一组数字
/// 两处渲染就是两处维护。
///
/// 还没读到过快照就整格不画：「队列 0」与「还没拿到第一份快照」是两句话，
/// 摆一个假的 0 会让人以为没活可干。
fn queue_count(ui: &mut Ui, t: &Tokens, d: Density, vm: &WorkbenchVm) {
    let queue = vm.queue;
    if !queue.known {
        return;
    }
    divider(ui, t, d);
    meta_icon(ui, d, ph::LIST_CHECKS, t.accent);
    let text = if queue.paused {
        format!("队列 {} · 已暂停", queue.active)
    } else {
        format!("队列 {}", queue.active)
    };
    ui.label(
        RichText::new(text)
            .font(Font::micro(d))
            .color(if queue.paused {
                t.warn
            } else {
                t.text_secondary
            }),
    );
    // 跨项目过滤不许无声：不然人会对着一块空面板怀疑服务没连上。
    if queue.filtered_out > 0 {
        ui.label(
            RichText::new(format!("已过滤 {} 条其它项目条目", queue.filtered_out))
                .font(Font::micro(d))
                .color(t.text_muted),
        );
    }
    // 契约破损也不许无声：它往往不是「别的项目」，而是服务端给的历史行缺了 dbnum。
    if queue.malformed > 0 {
        ui.label(
            RichText::new(format!("{} 条历史行缺 dbnum 未显示", queue.malformed))
                .font(Font::micro(d))
                .color(t.warn),
        );
    }
    // 死信不进「队列 N」：那一格数的是还会被干掉的活，而这些非人工不动。
    // 队列空成 0 的时候它更要在——那正是最容易以为「都干完了」的一刻。
    if queue.dead_letters > 0 {
        ui.label(
            RichText::new(format!("{} 个单元已放弃重试", queue.dead_letters))
                .font(Font::micro(d))
                .color(t.danger),
        );
    }
}

// ---------------------------------------------------------------- 小零件

fn logo(ui: &mut Ui, t: &Tokens, d: Density) {
    let s = d.px(24.0);
    let (rect, _) = ui.allocate_exact_size(vec2(s, s), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius::MD), t.accent);
    let g = ui.painter().layout_no_wrap(
        ph::CUBE_FOCUS.to_owned(),
        egui::FontId::new(d.px(14.0), egui::FontFamily::Proportional),
        t.accent_ink,
    );
    let pos = rect.center() - g.size() / 2.0;
    ui.painter().galley(pos, g, t.accent_ink);
}

fn avatar(ui: &mut Ui, t: &Tokens, d: Density, name: &str) {
    let s = d.px(22.0);
    let (rect, _) = ui.allocate_exact_size(vec2(s, s), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), s / 2.0, t.accent_bg);
    let ch: String = name.chars().take(1).collect();
    let g = ui.painter().layout_no_wrap(ch, Font::micro(d), t.accent);
    ui.painter()
        .galley(rect.center() - g.size() / 2.0, g, t.accent);
}

pub fn divider(ui: &mut Ui, t: &Tokens, d: Density) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, d.px(16.0)), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.border);
}

pub fn hairline(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, t.border);
}

fn dot(ui: &mut Ui, d: Density, color: Color32) {
    let s = d.px(6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(s, s), Sense::hover());
    ui.painter().circle_filled(rect.center(), s / 2.0, color);
}

fn meta_icon(ui: &mut Ui, d: Density, icon: &str, color: Color32) {
    ui.label(
        RichText::new(icon)
            .font(egui::FontId::new(
                d.px(12.0),
                egui::FontFamily::Proportional,
            ))
            .color(color),
    );
}

fn command_menu_button(d: Density, label: &str, has_popup: bool) -> egui::Button<'static> {
    let label = if has_popup {
        format!("{label}  {}", ph::CARET_DOWN)
    } else {
        label.to_owned()
    };
    egui::Button::new(RichText::new(label).font(Font::label(d)))
        .frame(true)
        .frame_when_inactive(false)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(radius::MD))
        .min_size(vec2(0.0, d.px(26.0)))
}

fn command_menu_action(d: Density, icon: &str, label: &str, detail: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(format!("{icon}  {label}")).font(Font::label(d)))
        .shortcut_text(RichText::new(detail).font(Font::micro(d)))
        .min_size(vec2(d.px(224.0), d.px(28.0)))
}

fn open_menu_marker(ui: &mut Ui, t: &Tokens, d: Density, response: &egui::Response, open: bool) {
    if open {
        let marker = egui::Rect::from_min_size(
            pos2(response.rect.left(), response.rect.bottom() - d.px(2.0)),
            vec2(response.rect.width(), d.px(2.0)),
        );
        ui.painter()
            .rect_filled(marker, CornerRadius::ZERO, t.accent);
    }
}

/// 纯视觉搜索框（M1-1）；真实的命令面板 / 搜索交互不在本里程碑。
fn search_box(ui: &mut Ui, t: &Tokens, d: Density, width: f32, placeholder: &str, key: &str) {
    let h = d.px(26.0);
    let (rect, _) = ui.allocate_exact_size(vec2(width, h), Sense::click());
    let cr = CornerRadius::same(radius::MD);
    ui.painter().rect_filled(rect, cr, t.bg_input);
    ui.painter().rect_stroke(
        rect,
        cr,
        Stroke::new(1.0_f32, t.border),
        egui::StrokeKind::Inside,
    );
    let pad = d.px(10.0);
    let ig = ui.painter().layout_no_wrap(
        ph::MAGNIFYING_GLASS.to_owned(),
        egui::FontId::new(d.px(13.0), egui::FontFamily::Proportional),
        t.text_muted,
    );
    let iw = ig.size().x;
    ui.painter().galley(
        pos2(rect.left() + pad, rect.center().y - ig.size().y / 2.0),
        ig,
        t.text_muted,
    );
    let pg = ui
        .painter()
        .layout_no_wrap(placeholder.to_owned(), Font::meta(d), t.text_muted);
    ui.painter().galley(
        pos2(
            rect.left() + pad + iw + d.px(8.0),
            rect.center().y - pg.size().y / 2.0,
        ),
        pg,
        t.text_muted,
    );
    let kg = ui
        .painter()
        .layout_no_wrap(key.to_owned(), Font::mono_micro(d), t.text_muted);
    ui.painter().galley(
        pos2(
            rect.right() - pad - kg.size().x,
            rect.center().y - kg.size().y / 2.0,
        ),
        kg,
        t.text_muted,
    );
}
