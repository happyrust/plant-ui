//! S6 设置任务窗。只暴露当前两端都能立即生效的选项。

use egui::{Align, ComboBox, Layout, RichText, TextEdit};

use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens};
use crate::style::widgets;

/// 模型服务地址的出厂默认。
///
/// 8020 的回环侧被另一个 SurrealDB 实例占着，gen-model 因此让到了 8021
/// （见 gen-model/DbOption.toml 的注释）。指向 8020 不会干净地连不上，
/// 而是打进那个实例、拿回一个看着像模像样的 HTTP 错误。
pub const DEFAULT_MODEL_API_URL: &str = "http://127.0.0.1:8021";
/// 数据中心服务地址。它与模型更新服务是两个独立进程，不能复用 8021。
pub const DEFAULT_DATA_API_URL: &str = "http://127.0.0.1:9099";

/// 浏览器端那一行为什么是灰的。按住不放才看得到，所以话要说全。
const WEB_MESH_DIR_HINT: &str = "浏览器端的网格与字体贴图一样由站点供给，不读本地目录";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Theme {
    Light,
    Dark,
}

/// 网格目录的校验结果。
///
/// 绘制层不碰文件系统——两端共用一份代码，浏览器那边压根没有目录可查——所以这一项
/// 由宿主算好填进来，这里只负责把它画出来。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum MeshDirStatus {
    /// 还没校验过，或者这一侧不适用。
    #[default]
    Unknown,
    /// 目录在，里面确实有网格文件。
    Ok,
    /// 目录在，但一个 `.mesh` 都没有。最常见的成因是选错一层——指到了资产根，
    /// 而网格在它底下的 `meshes` 里。
    NoMeshFiles,
    /// 路径不存在、不是目录，或者读不动。
    Unreachable(String),
}

impl Theme {
    pub const fn tokens(self) -> Tokens {
        match self {
            Self::Light => Tokens::light(),
            Self::Dark => Tokens::dark(),
        }
    }
}

/// 供数模式（ADR-0026）：模型树 / 属性 / 搜索 / 三维实例从哪儿读。整个接入点只认一种，
/// 不按读面混；命令面（补齐 / 更新 / 队列）与它无关，永远走模型服务。
///
/// 落盘字面是 `service` / `store`。它住在这个 crate 里是因为设置窗要画它、接入点面板要
/// 报它；真正按它选读面的是宿主（`plant-ui-app` 的 `read_face`）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadFaceKind {
    /// 服务供数：经模型服务的 HTTP 接口读，底下是 e3d-io（数据）与 e3d-model（几何）。出厂默认。
    #[default]
    Service,
    /// 库供数：直连 SurrealDB 的 `pe` / `pe_owner` / `ATT_*` / `inst_relate` 读。保留档：
    /// 给已落盘的 rocksdb 部署、老版本模型服务与对拍用。
    Store,
}

impl ReadFaceKind {
    /// 下拉里的两项，按出厂默认在前的顺序。
    pub const ALL: [Self; 2] = [Self::Service, Self::Store];

    /// 界面上的名字。设置窗的下拉与接入点面板那一行共用，两处不各写一份。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Service => "服务供数",
            Self::Store => "库供数",
        }
    }

    /// 落盘与环境变量里的字面，与 serde 的 `rename_all = "lowercase"` 是同一份。
    /// 宿主拿它拼「由 PLANT_READ_FACE=store 压过」那句话。
    pub const fn key(self) -> &'static str {
        match self {
            Self::Service => "service",
            Self::Store => "store",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    pub density: Density,
    pub model_api_url: String,
    pub data_api_url: String,
    /// 供数模式。老 `settings.ron` 没这一格就是服务供数（`serde(default)`）。
    /// 它描述的是这台机器能不能碰到库，随 `model_api_url` 一起按机器落盘，不进项目 bundle。
    pub read_face: ReadFaceKind,
    /// 网格文件所在的目录。**空串是明确的「我没指定」**，那一格让给环境变量，
    /// 再退到出厂默认 `<资产根>/meshes`；解出来的实际路径由宿主放进
    /// [`State::mesh_dir_hint`] 当占位提示显示。
    ///
    /// 绘制层不知道资产根在哪，所以这里存不了一个「默认的绝对路径」——存了也是把
    /// 宿主那一层知识抄进绘制层，换一种部署方式就对不上。
    pub mesh_dir: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Light,
            density: Density::Standard,
            model_api_url: DEFAULT_MODEL_API_URL.to_owned(),
            data_api_url: DEFAULT_DATA_API_URL.to_owned(),
            read_face: ReadFaceKind::Service,
            mesh_dir: String::new(),
        }
    }
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub saved: Settings,
    /// 「跟随资产根」时网格实际会去哪个目录取。宿主算好放进来，只作占位提示。
    pub mesh_dir_hint: String,
    /// 供数模式那一格为什么改不了。`Some` = 环境变量压过了设置（`PLANT_READ_FACE` 在场，
    /// 计划 D11）：下拉灰掉、悬停与右下小字都说这一句。宿主填，绘制层不读环境变量。
    pub read_face_lock: Option<String>,
    draft: Settings,
    mesh_dir_status: MeshDirStatus,
    browse_requested: bool,
}

impl State {
    pub fn open(&mut self) {
        self.draft = self.saved.clone();
        self.open = true;
    }

    /// 用宿主解析出来的值顶掉出厂默认。环境变量这类来源只有宿主认得，
    /// 绘制层不去读它。
    pub fn adopt(&mut self, settings: Settings) {
        self.draft = settings.clone();
        self.saved = settings;
    }

    /// 草稿里此刻的网格目录。宿主拿它去校验。
    pub fn mesh_dir_draft(&self) -> &str {
        &self.draft.mesh_dir
    }

    /// 宿主选完目录之后填回草稿。
    pub fn set_mesh_dir_draft(&mut self, dir: impl Into<String>) {
        self.draft.mesh_dir = dir.into();
    }

    /// 宿主校验完把结论交回来。
    pub fn set_mesh_dir_status(&mut self, status: MeshDirStatus) {
        self.mesh_dir_status = status;
    }

    /// 「浏览…」被点过没有。绘制层不开系统对话框，只把这个意图挂在这儿等宿主取走。
    pub fn take_browse_request(&mut self) -> bool {
        std::mem::take(&mut self.browse_requested)
    }
}

/// Returns the saved settings when the user confirms the dialog.
pub fn show(ctx: &egui::Context, t: &Tokens, d: Density, state: &mut State) -> Option<Settings> {
    if !state.open {
        return None;
    }

    let mut open = true;
    let mut save = false;
    let mut cancel = false;
    egui::Window::new("设置")
        .id(egui::Id::new("plant-settings-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([880.0, 640.0])
        .min_size([620.0, 480.0])
        .show(ctx, |ui| {
            egui::Panel::bottom("plant-settings-footer")
                .show_separator_line(false)
                .show(ui, |ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.add(widgets::button(t, d, "保存").primary()).clicked() {
                            save = true;
                        }
                        if ui.add(widgets::button(t, d, "取消")).clicked() {
                            cancel = true;
                        }
                        if ui.add(widgets::button(t, d, "恢复默认值")).clicked() {
                            state.draft = Settings::default();
                        }
                    });
                });

            ui.label(RichText::new("外观").strong().color(t.text_secondary));
            setting_row(ui, t, "主题", "亮色为默认主题", |ui| {
                widgets::segmented(
                    ui,
                    t,
                    d,
                    &mut state.draft.theme,
                    &[(Theme::Light, "亮色"), (Theme::Dark, "深色")],
                );
            });
            setting_row(
                ui,
                t,
                "界面密度",
                "影响字号、行高和控件尺寸",
                |ui| {
                    widgets::segmented(
                        ui,
                        t,
                        d,
                        &mut state.draft.density,
                        &[
                            (Density::Compact, "紧凑 12"),
                            (Density::Standard, "标准 13"),
                            (Density::Relaxed, "宽松 15"),
                        ],
                    );
                },
            );
            setting_row(
                ui,
                t,
                "数据服务地址",
                "数据中心与房间查询接口，保存后下一次提交生效",
                |ui| {
                    ui.add(
                        TextEdit::singleline(&mut state.draft.data_api_url)
                            .desired_width(300.0)
                            .font(Font::mono(d))
                            .hint_text(DEFAULT_DATA_API_URL),
                    );
                },
            );

            ui.add_space(12.0);
            ui.label(RichText::new("资产").strong().color(t.text_secondary));
            // 浏览器端整行不适用：禁用而不是隐藏，两端的设置窗才长得一样
            // （与模型树菜单里「取回工作」同一条房规）。
            let local_files = cfg!(not(target_arch = "wasm32"));
            let hint = state.mesh_dir_hint.clone();
            setting_row(
                ui,
                t,
                "网格目录",
                "三维网格文件所在的目录；留空则使用灰字那个默认目录",
                |ui| {
                    let browse = ui.add_enabled(local_files, widgets::button(t, d, "浏览…"));
                    if browse.clicked() {
                        state.browse_requested = true;
                    }
                    browse.on_disabled_hover_text(WEB_MESH_DIR_HINT);
                    ui.add_enabled(
                        local_files,
                        TextEdit::singleline(&mut state.draft.mesh_dir)
                            .desired_width(300.0)
                            .font(Font::mono(d))
                            .hint_text(hint.as_str()),
                    )
                    .on_disabled_hover_text(WEB_MESH_DIR_HINT);
                },
            );
            if local_files && let Some((notice, color)) = mesh_dir_notice(t, &state.mesh_dir_status)
            {
                // 外面这层 `horizontal` 不能省：`with_layout` 会把剩下的竖直空间整块吃掉，
                // 这一行就飘到窗口底部去了，离它解释的那个输入框十万八千里。
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(notice).small().color(color));
                    });
                });
            }

            ui.add_space(12.0);
            ui.label(RichText::new("服务").strong().color(t.text_secondary));
            setting_row(
                ui,
                t,
                "模型服务地址",
                "gen-model 的 REST 与 WebSocket 入口，保存后下一次预览生效",
                |ui| {
                    ui.add(
                        TextEdit::singleline(&mut state.draft.model_api_url)
                            .desired_width(300.0)
                            .font(Font::mono(d))
                            .hint_text(DEFAULT_MODEL_API_URL),
                    );
                },
            );
            // 环境变量压过时这一格改了也不生效：灰掉而不是藏起来，人才看得见现在是哪一面、
            // 为什么改不了（与浏览器端「网格目录」那一行同一条房规）。
            let read_face_lock = state.read_face_lock.clone();
            setting_row(
                ui,
                t,
                "供数模式",
                "模型树 / 属性 / 搜索 / 三维实例从哪儿读；保存后清场并重装已加载范围",
                |ui| {
                    let combo = ui.add_enabled_ui(read_face_lock.is_none(), |ui| {
                        ComboBox::from_id_salt("plant-read-face")
                            .selected_text(state.draft.read_face.label())
                            .width(160.0)
                            .show_ui(ui, |ui| {
                                for kind in ReadFaceKind::ALL {
                                    ui.selectable_value(
                                        &mut state.draft.read_face,
                                        kind,
                                        kind.label(),
                                    );
                                }
                            })
                            .response
                    });
                    if let Some(lock) = read_face_lock.as_deref() {
                        combo.inner.on_disabled_hover_text(lock);
                    }
                },
            );
            if let Some(lock) = read_face_lock.as_deref() {
                // 同「网格目录」的校验提示：外面这层 `horizontal` 挡住 `with_layout` 吃掉
                // 剩余竖直空间，这一行才贴在它解释的那个下拉底下。
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(lock).small().color(t.warn));
                    });
                });
            }
        });

    if cancel || !open {
        state.open = false;
        return None;
    }
    if save {
        state.draft.model_api_url = normalize_api_url(&state.draft.model_api_url);
        state.draft.data_api_url = normalize_data_api_url(&state.draft.data_api_url);
        state.draft.mesh_dir = normalize_mesh_dir(&state.draft.mesh_dir);
        state.saved = state.draft.clone();
        state.open = false;
        return Some(state.saved.clone());
    }
    None
}

/// 校验结论那一行的文案与颜色；没什么可说的时候不画。
fn mesh_dir_notice(t: &Tokens, status: &MeshDirStatus) -> Option<(String, egui::Color32)> {
    match status {
        MeshDirStatus::Unknown => None,
        MeshDirStatus::Ok => Some(("已找到网格文件".to_owned(), t.success)),
        MeshDirStatus::NoMeshFiles => Some((
            "这个目录里没有网格文件；网格通常在资产根底下的 meshes 里".to_owned(),
            t.warn,
        )),
        MeshDirStatus::Unreachable(reason) => Some((reason.clone(), t.danger)),
    }
}

/// 资源管理器的「复制文件地址」给的是带引号的路径，直接粘进来会多一对引号，
/// 而带引号的路径在任何一层都打不开。顺手剥掉，省得人对着一个看不出错在哪的
/// 「目录不存在」发呆。
fn normalize_mesh_dir(raw: &str) -> String {
    raw.trim().trim_matches('"').trim().to_owned()
}

/// 地址拼接是 `{base}{path}`，尾斜杠会拼出 `//api/v1`；清空则退回出厂默认，
/// 免得存下一个连不上任何东西的空串。
fn normalize_api_url(raw: &str) -> String {
    let url = raw.trim().trim_end_matches('/');
    if url.is_empty() {
        DEFAULT_MODEL_API_URL.to_owned()
    } else {
        url.to_owned()
    }
}

fn normalize_data_api_url(raw: &str) -> String {
    let url = raw.trim().trim_end_matches('/');
    if url.is_empty() {
        DEFAULT_DATA_API_URL.to_owned()
    } else {
        url.to_owned()
    }
}

fn setting_row(
    ui: &mut egui::Ui,
    t: &Tokens,
    title: &str,
    detail: &str,
    trailing: impl FnOnce(&mut egui::Ui),
) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(340.0);
            ui.label(RichText::new(title).color(t.text_primary));
            ui.label(RichText::new(detail).small().color(t.text_muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), trailing);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_is_light() {
        assert_eq!(Settings::default().theme, Theme::Light);
    }

    #[test]
    fn default_api_url_avoids_the_surreal_port() {
        assert_eq!(Settings::default().model_api_url, DEFAULT_MODEL_API_URL);
        assert!(!DEFAULT_MODEL_API_URL.ends_with(":8020"));
        assert_eq!(Settings::default().data_api_url, DEFAULT_DATA_API_URL);
    }

    #[test]
    fn normalizing_strips_trailing_slash_and_refuses_empty() {
        assert_eq!(
            normalize_api_url("  http://10.0.0.9:8021/  "),
            "http://10.0.0.9:8021"
        );
        assert_eq!(normalize_api_url("   "), DEFAULT_MODEL_API_URL);
        assert_eq!(
            normalize_data_api_url("  http://10.0.0.9:9099/  "),
            "http://10.0.0.9:9099"
        );
    }

    #[test]
    fn mesh_dir_defaults_to_following_the_asset_root() {
        assert_eq!(Settings::default().mesh_dir, "");
    }

    /// 出厂是服务供数（ADR-0026 / 计划 D1）：库供数是保留档，得有人明确选它。
    #[test]
    fn the_default_read_face_is_service() {
        assert_eq!(Settings::default().read_face, ReadFaceKind::Service);
        assert_eq!(ReadFaceKind::default(), ReadFaceKind::Service);
    }

    /// 落盘字面是小写单词，与计划 D11 写的一致；两个方向都要对得上，
    /// 不然手改过的 `settings.ron` 读回来就是整份解析失败。（RON 那一层的往返在
    /// 宿主的 `settings_store` 测试里，这里只钉 serde 的改名。）
    #[test]
    fn read_face_kind_round_trips_as_lowercase_words() {
        assert_eq!(
            serde_json::to_string(&ReadFaceKind::Service).unwrap(),
            "\"service\""
        );
        assert_eq!(
            serde_json::to_string(&ReadFaceKind::Store).unwrap(),
            "\"store\""
        );
        assert_eq!(
            serde_json::from_str::<ReadFaceKind>("\"store\"").unwrap(),
            ReadFaceKind::Store
        );
        assert_eq!(ReadFaceKind::Service.label(), "服务供数");
        assert_eq!(ReadFaceKind::Store.label(), "库供数");
        // `key()` 与 serde 字面必须是同一份：宿主拿它拼「由 PLANT_READ_FACE=store 压过」，
        // 对不上的话那句话教人照着设的环境变量就认不出来。
        for kind in ReadFaceKind::ALL {
            assert_eq!(
                serde_json::to_string(&kind).unwrap(),
                format!("\"{}\"", kind.key())
            );
        }
    }

    #[test]
    fn normalizing_strips_the_quotes_explorer_pastes() {
        assert_eq!(
            normalize_mesh_dir("  \"D:\\models\\meshes\"  "),
            "D:\\models\\meshes"
        );
        assert_eq!(normalize_mesh_dir("   "), "");
    }

    #[test]
    fn browse_request_is_taken_once() {
        let mut state = State::default();
        assert!(!state.take_browse_request());
        state.browse_requested = true;
        assert!(state.take_browse_request());
        assert!(!state.take_browse_request());
    }

    #[test]
    fn only_a_checked_mesh_dir_says_anything() {
        let t = Tokens::light();
        assert!(mesh_dir_notice(&t, &MeshDirStatus::Unknown).is_none());
        assert_eq!(
            mesh_dir_notice(&t, &MeshDirStatus::Ok).map(|(_, color)| color),
            Some(t.success)
        );
        assert_eq!(
            mesh_dir_notice(&t, &MeshDirStatus::NoMeshFiles).map(|(_, color)| color),
            Some(t.warn)
        );
        assert_eq!(
            mesh_dir_notice(&t, &MeshDirStatus::Unreachable("读不动".into())),
            Some(("读不动".to_owned(), t.danger))
        );
    }

    #[test]
    fn adopt_syncs_draft_so_the_dialog_opens_on_the_host_value() {
        let mut state = State::default();
        state.adopt(Settings {
            model_api_url: "http://10.0.0.9:8021".into(),
            data_api_url: "http://10.0.0.9:9099".into(),
            ..Settings::default()
        });
        state.open();
        assert_eq!(state.saved.model_api_url, "http://10.0.0.9:8021");
        assert_eq!(state.saved.data_api_url, "http://10.0.0.9:9099");
    }

    const WINDOW_ID: &str = "plant-settings-window";
    const READ_FACE_LOCK: &str = "由 PLANT_READ_FACE=store 压过，改这一格这次不会生效";

    /// 开着设置窗画一帧。
    fn one_frame(ctx: &egui::Context, state: &mut State) {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        ctx.begin_pass(input);
        show(ctx, &Tokens::light(), Density::Standard, state);
        let _ = ctx.end_pass();
    }

    /// 开着设置窗转 8 帧，逐帧记下窗口高度。
    fn window_heights(status: MeshDirStatus, read_face_lock: Option<&str>) -> Vec<f32> {
        let ctx = egui::Context::default();
        let window_id = egui::Id::new(WINDOW_ID);
        let mut state = State::default();
        state.read_face_lock = read_face_lock.map(str::to_owned);
        state.open();
        state.set_mesh_dir_status(status);

        (0..8)
            .map(|_| {
                one_frame(&ctx, &mut state);
                ctx.memory(|memory| memory.area_rect(window_id).unwrap().height())
            })
            .collect()
    }

    /// 挂着校验提示的那一档也要测：这一行是条件出现的，多一个分支就多一次
    /// 布局嵌套出错的机会。供数模式被环境变量锁上的那一行同理。
    #[test]
    fn settings_window_height_stays_stable_after_opening() {
        let cases = [
            (MeshDirStatus::Unknown, None),
            (MeshDirStatus::Ok, None),
            (MeshDirStatus::NoMeshFiles, None),
            (MeshDirStatus::Unreachable("目录不存在".into()), None),
            (MeshDirStatus::Ok, Some(READ_FACE_LOCK)),
        ];
        for (status, lock) in cases {
            let heights = window_heights(status.clone(), lock);
            assert!(
                heights[2..]
                    .windows(2)
                    .all(|pair| (pair[1] - pair[0]).abs() < f32::EPSILON),
                "{status:?} / 锁 {lock:?} 下设置窗口高度逐帧增长：{heights:?}"
            );
        }
    }

    /// 这一帧设置窗那一层里有几个灰掉的控件。
    fn disabled_widgets(ctx: &egui::Context) -> usize {
        let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new(WINDOW_ID));
        ctx.viewport(|viewport| {
            viewport
                .prev_pass
                .widgets
                .get_layer(layer)
                .filter(|widget| !widget.enabled)
                .count()
        })
    }

    /// `PLANT_READ_FACE` 在场时下拉灰掉（计划 D11）：改了也不生效的格子不能看着像能改。
    /// 没锁时整窗一个灰控件都没有——网格目录那一行只在浏览器端灰，本机测试不在那一侧。
    ///
    /// 每一态都多转几帧再数：窗口头一帧是量尺寸的那一遍，egui 把整窗控件都标成灰的，
    /// 那一帧数出来的不是设置窗的意思。
    #[test]
    fn an_overridden_read_face_disables_the_dropdown() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        state.open();
        for _ in 0..3 {
            one_frame(&ctx, &mut state);
        }
        assert_eq!(disabled_widgets(&ctx), 0, "没被压过时不该有灰掉的控件");

        state.read_face_lock = Some(READ_FACE_LOCK.to_owned());
        for _ in 0..3 {
            one_frame(&ctx, &mut state);
        }
        assert!(disabled_widgets(&ctx) > 0, "被压过时供数模式下拉该是灰的");

        state.read_face_lock = None;
        for _ in 0..3 {
            one_frame(&ctx, &mut state);
        }
        assert_eq!(disabled_widgets(&ctx), 0, "锁一撤，下拉就该能改");
    }
}
