//! 帮助菜单与「关于 布置平台」弹窗。
//!
//! 展示前端 (`plant-ui`) 版本、模型后端 (`aios-database` `/api/v1/health`) 版本与构建信息，
//! 以及数据中心服务 (`rs-server` `/api/metadata/meta`) 版本，并提供常用操作与命令行指引。

use egui::{Align, CornerRadius, Key, Layout, Margin, RichText, Stroke, Ui, vec2};
use egui_phosphor::regular as ph;

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Status, Tokens, radius, space};
use crate::style::widgets;
use crate::vm::{ServiceVersionsVm, WorkbenchVm};
use crate::workbench::Pane;
use crate::Cmd;

/// 编译期注入的 `plant-ui` 前端版本号（与工作区 `Cargo.toml` 同步）。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 前端版本号显示标签（如 `v0.1.15`）。
pub fn app_version_tag() -> String {
    ServiceVersionsVm::display_version(APP_VERSION)
}

pub const fn runtime_platform_label() -> &'static str {
    #[cfg(target_arch = "wasm32")]
    {
        "WebAssembly (浏览器)"
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        "原生桌面端"
    }
}

#[derive(Debug, Default)]
pub struct State {
    pub help_open: bool,
    pub about_open: bool,
    copied_feedback: bool,
}

impl State {
    pub fn open_help(&mut self) {
        self.help_open = true;
    }

    pub fn open_about(&mut self) {
        self.about_open = true;
        self.copied_feedback = false;
    }
}

fn backend_version_display(versions: &ServiceVersionsVm) -> (String, Status) {
    if !versions.backend_version.trim().is_empty() {
        (
            ServiceVersionsVm::display_version(&versions.backend_version),
            Status::Success,
        )
    } else if versions.loading {
        ("获取中…".to_owned(), Status::Info)
    } else if versions.backend_error.is_some() {
        ("未连接 / 获取失败".to_owned(), Status::Error)
    } else {
        ("待查询".to_owned(), Status::Neutral)
    }
}

fn data_service_version_display(versions: &ServiceVersionsVm) -> (String, Status) {
    if !versions.data_service_version.trim().is_empty() {
        (
            ServiceVersionsVm::display_version(&versions.data_service_version),
            Status::Success,
        )
    } else if versions.loading {
        ("获取中…".to_owned(), Status::Info)
    } else if versions.data_service_error.is_some() {
        ("未连接 / 获取失败".to_owned(), Status::Error)
    } else {
        ("待查询".to_owned(), Status::Neutral)
    }
}

/// 组装适合复制到剪贴板的多行版本报告。
pub fn version_summary_text(vm: &WorkbenchVm) -> String {
    let v = &vm.versions;
    let ap = &vm.access_point;
    let mut lines = vec![
        format!("布置平台 (plant-ui): {}", app_version_tag()),
        format!("运行形态: {}", runtime_platform_label()),
    ];

    let backend_ver = if !v.backend_version.trim().is_empty() {
        ServiceVersionsVm::display_version(&v.backend_version)
    } else if let Some(err) = &v.backend_error {
        format!("未连接 ({err})")
    } else {
        "未获取".to_owned()
    };
    lines.push(format!("模型后端 (aios-database): {backend_ver}"));
    if !v.backend_build_id.is_empty() {
        lines.push(format!("  Build ID: {}", v.backend_build_id));
    }
    if !v.backend_read_mode.is_empty() || !v.backend_data_face.is_empty() {
        lines.push(format!(
            "  运行模式: read_mode={} data_face={}",
            if v.backend_read_mode.is_empty() {
                "—"
            } else {
                &v.backend_read_mode
            },
            if v.backend_data_face.is_empty() {
                "—"
            } else {
                &v.backend_data_face
            }
        ));
    }
    if !ap.model_api_url.trim().is_empty() {
        lines.push(format!("  模型服务地址: {}", ap.model_api_url.trim()));
    }

    let data_ver = if !v.data_service_version.trim().is_empty() {
        ServiceVersionsVm::display_version(&v.data_service_version)
    } else if let Some(err) = &v.data_service_error {
        format!("未连接 ({err})")
    } else {
        "未获取".to_owned()
    };
    lines.push(format!("数据服务 (rs-server): {data_ver}"));
    if !ap.data_api_url.trim().is_empty() {
        lines.push(format!("  数据服务地址: {}", ap.data_api_url.trim()));
    }

    if !v.backend_e3d_model_build_id.is_empty() {
        lines.push(format!("  e3d-model: {}", v.backend_e3d_model_build_id));
    }
    if !v.backend_e3d_io_build_id.is_empty() {
        lines.push(format!("  e3d-io: {}", v.backend_e3d_io_build_id));
    }
    if !v.backend_e3d_attlib_build_id.is_empty() {
        lines.push(format!("  e3d-attlib: {}", v.backend_e3d_attlib_build_id));
    }
    if !vm.project.trim().is_empty() {
        let code = vm.project_code.trim();
        let project_label = if code.is_empty() {
            vm.project.trim().to_owned()
        } else {
            format!("{} ({code})", vm.project.trim())
        };
        lines.push(format!("当前项目: {project_label}"));
    }
    if !ap.mdb.trim().is_empty() {
        lines.push(format!("MDB: {}", ap.mdb.trim()));
    }
    lines.join("\n")
}

/// 计算受视口约束的可拖动/可缩放窗口初始位置与尺寸（不使用 `.anchor(...)`，确保可拖动与关闭）。
fn clamped_window_rect(
    ctx: &egui::Context,
    d: Density,
    preferred_w: f32,
    preferred_h: f32,
    min_w: f32,
    min_h: f32,
) -> (egui::Pos2, [f32; 2], [f32; 2], [f32; 2]) {
    let viewport = ctx.input(|i| i.viewport_rect());
    let margin = d.px(24.0);
    let max_w = (viewport.width() - margin).max(d.px(min_w));
    let max_h = (viewport.height() - margin).max(d.px(min_h));
    let default_w = d.px(preferred_w).min(max_w);
    let default_h = d.px(preferred_h).min(max_h);
    let min_w = d.px(min_w).min(max_w);
    let min_h = d.px(min_h).min(max_h);
    let default_pos = egui::pos2(
        viewport.left() + ((viewport.width() - default_w) * 0.5).max(d.px(12.0)),
        viewport.top() + ((viewport.height() - default_h) * 0.5).max(d.px(12.0)),
    );
    (default_pos, [default_w, default_h], [min_w, min_h], [max_w, max_h])
}

/// 绘制「使用指引与快捷键」及「关于 布置平台」浮窗。
pub fn show(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    state: &mut State,
) -> Vec<Cmd> {
    let mut cmds = Vec::new();
    let esc_close = ctx.input(|i| i.key_pressed(Key::Escape))
        && ctx.memory(|m| m.focused().is_none());

    if state.help_open {
        show_help_window(ctx, t, d, state, esc_close && !state.about_open, &mut cmds);
    }
    if state.about_open {
        show_about_window(ctx, t, d, vm, state, esc_close, &mut cmds);
    }
    cmds
}

fn show_help_window(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    state: &mut State,
    esc_close: bool,
    cmds: &mut Vec<Cmd>,
) {
    let mut open = state.help_open;
    let mut close_clicked = esc_close;
    let (default_pos, default_size, min_size, max_size) =
        clamped_window_rect(ctx, d, 560.0, 480.0, 420.0, 320.0);

    egui::Window::new("使用指引与快捷键")
        .id(egui::Id::new("plant-help-guide-window"))
        .open(&mut open)
        .movable(true)
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_pos(default_pos)
        .default_size(default_size)
        .min_size(min_size)
        .max_size(max_size)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(space::S2, space::S2);

            let scroll_h = (ui.available_height() - d.px(42.0)).max(d.px(120.0));
            ui.allocate_ui(vec2(ui.available_width(), scroll_h), |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        help_section(
                            ui,
                            t,
                            d,
                            "三维视口操作",
                            &[
                                ("左键 / 中键拖拽", "平移三维场景视角"),
                                ("右键拖拽", "围绕指针落点旋转轨道视角"),
                                ("鼠标滚轮", "向指针所在方向缩放视图"),
                                ("左键单击", "拾取构件并联动模型树与属性面板"),
                                ("ViewCube / Home", "点击方向立方体切换正交/轴测视角，Home 适配全图"),
                            ],
                        );
                        help_section(
                            ui,
                            t,
                            d,
                            "导航与全局快捷键",
                            &[
                                ("Alt + ← / Alt + →", "按浏览历史后退或前进选中项与相机位姿"),
                                ("顶部搜索框", "按元素名称前缀或子串检索并快速定位"),
                                ("Esc", "关闭当前打开的预览、设置或帮助浮窗"),
                            ],
                        );
                        help_section(
                            ui,
                            t,
                            d,
                            "常用工作台入口",
                            &[
                                ("项目 → 打开项目 / 取回工作", "切换项目接入点，或清除缓存刷新树与模型"),
                                ("项目 → 导入虚拟孔洞", "导入 AMS 提资表进行孔洞映射与布尔预览"),
                                ("创建 → 三维数据接口", "预览提取元数据或向数据中心发布提资包"),
                                ("创建 → 三维数据提资", "提交前先检查数据是否最新，落后就先更新"),
                                ("模型更新", "扫描并执行待更新 dbnum 的增量数据与模型重建"),
                                ("底栏接入点芯片", "查看模型库、MDB、服务地址与供数模式明细"),
                            ],
                        );
                        help_section(
                            ui,
                            t,
                            d,
                            "命令行常用指令",
                            &[
                                ("help / q help", "查看完整内置查询命令与 MCP 工具名"),
                                ("q identity [REF]", "查询选中或指定参考号元素的身份与类型"),
                                ("q owners [REF]", "查询元素的祖先 OWNER 层级链"),
                                ("q attrs [REF]", "查询白名单属性或全部属性字段"),
                                ("q room / q bounds", "查询房间归属或世界坐标包围盒"),
                                ("clear", "清空命令行输出缓冲"),
                            ],
                        );
                    });
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .add(widgets::button(t, d, "关于 布置平台").icon(ph::INFO))
                    .clicked()
                {
                    state.open_about();
                    cmds.push(Cmd::RefreshVersions);
                }
                if ui
                    .add(widgets::button(t, d, "命令行运行 help").icon(ph::TERMINAL_WINDOW))
                    .clicked()
                {
                    cmds.push(Cmd::FocusPane(Pane::CommandLine));
                    cmds.push(Cmd::SubmitCommand("help".to_owned()));
                    close_clicked = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(widgets::button(t, d, "关闭")).clicked() {
                        close_clicked = true;
                    }
                });
            });
        });

    state.help_open = open && !close_clicked;
}

fn show_about_window(
    ctx: &egui::Context,
    t: &Tokens,
    d: Density,
    vm: &WorkbenchVm,
    state: &mut State,
    esc_close: bool,
    cmds: &mut Vec<Cmd>,
) {
    let mut open = state.about_open;
    let mut close_clicked = esc_close;
    let (default_pos, default_size, min_size, max_size) =
        clamped_window_rect(ctx, d, 520.0, 460.0, 420.0, 340.0);

    egui::Window::new("关于 布置平台")
        .id(egui::Id::new("plant-about-window"))
        .open(&mut open)
        .movable(true)
        .collapsible(false)
        .resizable(true)
        .constrain(true)
        .default_pos(default_pos)
        .default_size(default_size)
        .min_size(min_size)
        .max_size(max_size)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = vec2(space::S2, space::S2);

            let scroll_h = (ui.available_height() - d.px(42.0)).max(d.px(120.0));
            ui.allocate_ui(vec2(ui.available_width(), scroll_h), |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        // 顶部品牌与前端版本条
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new("布置平台")
                                    .font(Font::strong(d))
                                    .color(t.text_primary),
                            );
                            ui.add(widgets::status_tag(t, d, &app_version_tag(), Status::Info));
                            ui.add(widgets::status_tag(
                                t,
                                d,
                                runtime_platform_label(),
                                Status::Neutral,
                            ));
                        });
                        ui.label(
                            RichText::new("三维工厂/船舶布置审阅与数据协同工作台")
                                .font(Font::meta(d))
                                .color(t.text_muted),
                        );

                        let v = &vm.versions;
                        let ap = &vm.access_point;
                        let (backend_ver, backend_status) = backend_version_display(v);
                        let (data_ver, data_status) = data_service_version_display(v);

                        // 版本与服务端点主卡片
                        egui::Frame::new()
                            .fill(t.bg_header)
                            .stroke(Stroke::new(1.0, t.border))
                            .corner_radius(CornerRadius::same(radius::MD))
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.spacing_mut().item_spacing.y = d.px(6.0);

                                ui.label(
                                    RichText::new("前后端版本信息")
                                        .font(Font::strong(d))
                                        .color(t.text_secondary),
                                );
                                version_badge_row(
                                    ui,
                                    t,
                                    d,
                                    "前端界面 (plant-ui)",
                                    &app_version_tag(),
                                    Status::Info,
                                );
                                version_badge_row(
                                    ui,
                                    t,
                                    d,
                                    "模型后端 (aios-database)",
                                    &backend_ver,
                                    backend_status,
                                );
                                if !v.backend_build_id.is_empty() {
                                    kv_row(ui, t, d, "后端构建标识", &v.backend_build_id, true);
                                }
                                if !v.backend_read_mode.is_empty() || !v.backend_data_face.is_empty()
                                {
                                    let mode = format!(
                                        "{} / {}",
                                        if v.backend_read_mode.is_empty() {
                                            "—"
                                        } else {
                                            &v.backend_read_mode
                                        },
                                        if v.backend_data_face.is_empty() {
                                            "—"
                                        } else {
                                            &v.backend_data_face
                                        }
                                    );
                                    kv_row(ui, t, d, "后端读模 / 数据面", &mode, true);
                                }
                                if !ap.model_api_url.trim().is_empty() {
                                    kv_row(ui, t, d, "模型服务地址", ap.model_api_url.trim(), true);
                                }
                                if let Some(err) = &v.backend_error {
                                    error_hint(ui, t, d, "模型后端查询异常", err);
                                }

                                ui.separator();

                                version_badge_row(
                                    ui,
                                    t,
                                    d,
                                    "数据服务 (rs-server)",
                                    &data_ver,
                                    data_status,
                                );
                                if !ap.data_api_url.trim().is_empty() {
                                    kv_row(ui, t, d, "数据服务地址", ap.data_api_url.trim(), true);
                                }
                                if let Some(err) = &v.data_service_error {
                                    error_hint(ui, t, d, "数据服务查询异常", err);
                                }
                            });

                        if !vm.project.trim().is_empty() || !ap.mdb.trim().is_empty() {
                            egui::Frame::new()
                                .fill(t.bg_header)
                                .stroke(Stroke::new(1.0, t.border))
                                .corner_radius(CornerRadius::same(radius::MD))
                                .inner_margin(Margin::symmetric(10, 8))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.spacing_mut().item_spacing.y = d.px(5.0);
                                    ui.label(
                                        RichText::new("当前工作区")
                                            .font(Font::strong(d))
                                            .color(t.text_secondary),
                                    );
                                    if !vm.project.trim().is_empty() {
                                        let project_val = if vm.project_code.trim().is_empty() {
                                            vm.project.trim().to_owned()
                                        } else {
                                            format!(
                                                "{} ({})",
                                                vm.project.trim(),
                                                vm.project_code.trim()
                                            )
                                        };
                                        kv_row(ui, t, d, "当前项目", &project_val, false);
                                    }
                                    if !ap.mdb.trim().is_empty() {
                                        kv_row(ui, t, d, "当前 MDB", ap.mdb.trim(), true);
                                    }
                                });
                        }

                        if !v.backend_e3d_model_build_id.is_empty()
                            || !v.backend_e3d_io_build_id.is_empty()
                            || !v.backend_e3d_attlib_build_id.is_empty()
                            || !v.backend_started_at.is_empty()
                        {
                            egui::CollapsingHeader::new(
                                RichText::new("核心组件构建信息")
                                    .font(Font::meta(d))
                                    .color(t.text_secondary),
                            )
                            .default_open(false)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = d.px(4.0);
                                if !v.backend_e3d_model_build_id.is_empty() {
                                    kv_row(
                                        ui,
                                        t,
                                        d,
                                        "e3d-model",
                                        &v.backend_e3d_model_build_id,
                                        true,
                                    );
                                }
                                if !v.backend_e3d_io_build_id.is_empty() {
                                    kv_row(ui, t, d, "e3d-io", &v.backend_e3d_io_build_id, true);
                                }
                                if !v.backend_e3d_attlib_build_id.is_empty() {
                                    kv_row(
                                        ui,
                                        t,
                                        d,
                                        "e3d-attlib",
                                        &v.backend_e3d_attlib_build_id,
                                        true,
                                    );
                                }
                                if !v.backend_started_at.is_empty() {
                                    kv_row(ui, t, d, "后端启动时间", &v.backend_started_at, true);
                                }
                            });
                        }
                    });
            });

            ui.separator();
            ui.horizontal(|ui| {
                let refresh_label = if vm.versions.loading {
                    "正在获取…"
                } else {
                    "刷新版本"
                };
                if ui
                    .add_enabled(
                        !vm.versions.loading,
                        widgets::button(t, d, refresh_label).icon(ph::ARROWS_CLOCKWISE),
                    )
                    .clicked()
                {
                    state.copied_feedback = false;
                    cmds.push(Cmd::RefreshVersions);
                }
                if ui
                    .add(widgets::button(t, d, "复制版本信息").icon(ph::COPY))
                    .clicked()
                {
                    ctx.copy_text(version_summary_text(vm));
                    state.copied_feedback = true;
                }
                if state.copied_feedback {
                    ui.label(
                        RichText::new("已复制")
                            .font(Font::micro(d))
                            .color(t.success),
                    );
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(widgets::button(t, d, "关闭")).clicked() {
                        close_clicked = true;
                    }
                });
            });
        });

    state.about_open = open && !close_clicked;
}

fn help_section(ui: &mut Ui, t: &Tokens, d: Density, title: &str, items: &[(&str, &str)]) {
    egui::Frame::new()
        .fill(t.bg_header)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(radius::MD))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = d.px(5.0);
            ui.label(
                RichText::new(title)
                    .font(Font::strong(d))
                    .color(t.text_primary),
            );
            for (key, desc) in items {
                ui.horizontal_wrapped(|ui| {
                    ui.add(widgets::status_tag(t, d, key, Status::Neutral));
                    ui.label(
                        RichText::new(*desc)
                            .font(Font::meta(d))
                            .color(t.text_secondary),
                    );
                });
            }
        });
}

fn version_badge_row(ui: &mut Ui, t: &Tokens, d: Density, label: &str, value: &str, status: Status) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .font(Font::meta(d))
                .color(t.text_secondary),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(widgets::status_tag(t, d, value, status));
        });
    });
}

fn kv_row(ui: &mut Ui, t: &Tokens, d: Density, label: &str, value: &str, mono: bool) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .font(Font::micro(d))
                .color(t.text_muted),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let font = if mono {
                Font::mono_micro(d)
            } else {
                Font::micro(d)
            };
            ui.add(
                egui::Label::new(RichText::new(value).font(font).color(t.text_secondary))
                    .selectable(true),
            );
        });
    });
}

fn error_hint(ui: &mut Ui, t: &Tokens, d: Density, label: &str, err: &str) {
    ui.label(
        RichText::new(format!("{label}：{err}"))
            .font(Font::micro(d))
            .color(t.danger),
    );
}

#[cfg(test)]
mod tests {
    use super::{APP_VERSION, State, app_version_tag, show, version_summary_text};
    use crate::style::tokens::{Density, Tokens};
    use crate::task_queue::Health;
    use crate::vm::{AccessPointVm, ServiceVersionsVm, WorkbenchVm};

    #[test]
    fn app_version_tag_prefixes_v_from_cargo_pkg_version() {
        assert_eq!(app_version_tag(), format!("v{APP_VERSION}"));
        assert_eq!(ServiceVersionsVm::display_version("0.1.55"), "v0.1.55");
        assert_eq!(ServiceVersionsVm::display_version("v0.2"), "v0.2");
        assert_eq!(ServiceVersionsVm::display_version("  "), "—");
    }

    #[test]
    fn service_versions_apply_health_and_summary_text_include_backend_and_data_service() {
        let mut versions = ServiceVersionsVm {
            backend_error: Some("stale".into()),
            ..Default::default()
        };
        let health = Health {
            version: "0.1.55".into(),
            build_id: "0.1.55+g97739569".into(),
            e3d_model_build_id: "0.1.0@abc".into(),
            e3d_io_build_id: "0.5.0@def".into(),
            e3d_attlib_build_id: "0.1.0@ghi".into(),
            data_read_mode: Some("direct".into()),
            data_face: "ingest".into(),
            started_at: "2026-11-26T02:16:27Z".into(),
            ..Default::default()
        };
        versions.apply_health(&health);
        versions.data_service_version = "0.2".into();
        assert_eq!(versions.backend_error, None);
        assert_eq!(versions.backend_version, "0.1.55");

        let vm = WorkbenchVm {
            project: "AvevaMarineSample".into(),
            project_code: "1516".into(),
            access_point: AccessPointVm {
                mdb: "/ALL".into(),
                model_api_url: "http://127.0.0.1:8024".into(),
                data_api_url: "http://127.0.0.1:9098".into(),
                ..Default::default()
            },
            versions,
            ..Default::default()
        };

        let text = version_summary_text(&vm);
        assert!(text.contains(&app_version_tag()), "{text}");
        assert!(text.contains("v0.1.55"), "{text}");
        assert!(text.contains("0.1.55+g97739569"), "{text}");
        assert!(text.contains("v0.2"), "{text}");
        assert!(text.contains("AvevaMarineSample (1516)"), "{text}");
    }

    #[test]
    fn help_and_about_windows_render_headless_without_panicking() {
        let ctx = egui::Context::default();
        let tokens = Tokens::light();
        let density = Density::Standard;
        let mut vm = WorkbenchVm::default();
        vm.versions.backend_version = "0.1.55".into();
        vm.versions.data_service_version = "0.2".into();
        let mut state = State {
            help_open: true,
            about_open: true,
            copied_feedback: false,
        };
        for _ in 0..2 {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                ..Default::default()
            });
            let _ = show(&ctx, &tokens, density, &vm, &mut state);
            let _ = ctx.end_pass();
        }
        assert!(state.help_open && state.about_open);
    }
}
