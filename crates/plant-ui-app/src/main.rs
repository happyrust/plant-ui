//! plant-ui-app：Bevy 宿主（bevy_egui35 + plant-ui + plant-ui-data）。
//! 默认进入主工作台 S1；`--gallery` 进入 M0 组件画廊。

mod command;
mod data;
mod data_publish_api;
#[cfg(not(target_arch = "wasm32"))]
mod gallery;
mod logs;
mod model_update_api;
mod model_update_ws;
mod settings_store;
mod sim;
mod startup;

use std::collections::{HashMap, HashSet};
use web_time::{Duration, Instant};

use bevy::asset::AssetMetaCheck;
use bevy::prelude::{
    App as BevyApp, Camera2d, Commands, DefaultPlugins, PluginGroup, ResMut, Window, WindowPlugin,
};
#[cfg(not(target_arch = "wasm32"))]
use bevy::prelude::{
    Component, Entity, EventWriter, IntoScheduleConfigs, PostUpdate, Query, Trigger, With,
};
#[cfg(not(target_arch = "wasm32"))]
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
#[cfg(not(target_arch = "wasm32"))]
use bevy::window::PrimaryWindow;
use bevy_egui35::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext,
};
#[cfg(not(target_arch = "wasm32"))]
use bevy_egui35::{EguiFullOutput, EguiOutput, EguiPostUpdateSet, input::EguiInputEvent};
use chrono::{DateTime, Utc};
use command::ParsedCommand;
use eframe::egui;
use logs::Retry;
use plant_ui::Cmd;
use plant_ui::data_publish::{self, State as DataPublishState};
use plant_ui::fonts;
use plant_ui::model_update::{
    self, Feed as ModelUpdateFeed, State as ModelUpdateState, Vm as ModelUpdateVm,
};
use plant_ui::project_picker::{self, State as ProjectPickerState};
use plant_ui::room_browser::{self, State as RoomBrowserState, Vm as RoomBrowserVm};
use plant_ui::settings::{self, State as SettingsState};
use plant_ui::style::theme_tokens::{self, set_weight_families_ready};
use plant_ui::style::tokens::{Density, Tokens};
use plant_ui::task_queue;
use plant_ui::vm::{
    AccessPointVm, CommandLineKind, CommandLineVm, LogElement, ModelLoadVm, PropKind, PropRowVm,
    PropsDataVm, PropsVm, RoomDetailDataVm, RoomDetailVm, RoomMemberVm, RoomRelationVm, RoomViewVm,
    RoomVm, RoomsDataVm, RowVisibility, Selection, TreeRowVm, TreeVm, View3dVm, WorkbenchVm,
};
use plant_ui::workbench::{self, Pane, WorkbenchState};
use plant_ui_data::{EleTreeNode, RefU64};

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let gallery = std::env::args().any(|a| a == "--gallery");
    if gallery {
        run_gallery().expect("组件画廊启动失败");
    } else if let Err(error) = run_native() {
        eprintln!("plant-ui 启动失败：{error:#}");
        std::process::exit(1);
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn start(canvas: String, config_json: String) -> Result<(), wasm_bindgen::JsValue> {
    if !canvas.starts_with('#') || canvas.len() < 2 {
        return Err(wasm_bindgen::JsValue::from_str(
            "canvas 必须是形如 #plant-ui 的 CSS id 选择器",
        ));
    }
    let config = startup::browser_runtime_config(&config_json)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&format!("浏览器配置无效：{error:#}")))?;
    aios_core::set_db_option(config.db)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    startup::record_config_source("宿主页面注入");
    if let Some(model_api_url) = config.model_api_url {
        model_update_api::set_base_url(model_api_url)
            .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    }
    data_publish_api::set_base_url(config.data_api_url)
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    if config.auto_gen_mesh {
        console_warn("旧配置的 auto_gen_mesh 由模型服务负责，客户端不会在本地生成 mesh");
    }
    run(&canvas, "assets".into());
    Ok(())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace = console, js_name = warn)]
    fn console_warn(message: &str);
}

#[cfg(not(target_arch = "wasm32"))]
fn run_native() -> anyhow::Result<()> {
    use anyhow::Context;

    let development_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public/assets");
    let executable = std::env::current_exe().context("读取可执行文件路径失败")?;
    let asset_root = startup::resolve_asset_root(
        std::env::var_os("PLANT_ASSET_ROOT"),
        executable.parent(),
        &development_root,
    );
    if !asset_root.is_dir() {
        anyhow::bail!("资产根不存在或不是目录：{}", asset_root.display());
    }

    let legacy_path = asset_root.join(startup::LEGACY_PROJECT_CONFIG);
    if legacy_path.exists() {
        #[derive(serde::Deserialize)]
        struct ReleaseDbCredentials {
            v_user: Option<String>,
            v_password: Option<String>,
        }

        let release_db_path = executable
            .parent()
            .map(|dir| dir.join("DbOption.toml"))
            .filter(|path| path.is_file())
            .unwrap_or_else(|| "DbOption.toml".into());
        let release_db = if release_db_path.is_file() {
            Some(
                config::Config::builder()
                    .add_source(config::File::from(release_db_path))
                    .build()
                    .context("读取 DbOption.toml 失败")?
                    .try_deserialize::<ReleaseDbCredentials>()
                    .context("读取 DbOption.toml 数据库账号失败")?,
            )
        } else {
            None
        };
        let release_user = release_db
            .as_ref()
            .and_then(|db| db.v_user.as_deref())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("root");
        let release_password = release_db
            .as_ref()
            .and_then(|db| db.v_password.as_deref())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("root");
        let username = std::env::var("PLANT_DB_USER")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| release_user.to_owned());
        let password = std::env::var("PLANT_DB_PASSWORD")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| release_password.to_owned());
        let text = std::fs::read_to_string(&legacy_path)
            .with_context(|| format!("读取旧版项目配置失败：{}", legacy_path.display()))?;
        let config = startup::legacy_runtime_config(
            &text,
            &username,
            &password,
            None,
            std::env::var("PLANT_DATA_API_URL").ok(),
        )
        .with_context(|| format!("旧版项目配置无效：{}", legacy_path.display()))?;
        aios_core::set_db_option(config.db)?;
        startup::record_config_source(legacy_path.display().to_string());
        data_publish_api::set_base_url(config.data_api_url)?;
        if config.auto_gen_mesh {
            eprintln!("旧配置的 auto_gen_mesh 由模型服务负责，客户端不会在本地生成 mesh");
        }
    } else {
        // 没有项目配置，`get_db_option()` 会静默回落去读工作目录的 DbOption.toml。
        // 这一步只是把这条回落记下来——它照样会发生，区别只是界面说不说得出口。
        let fallback = std::env::current_dir()
            .unwrap_or_default()
            .join("DbOption.toml");
        startup::record_config_source(format!(
            "{}（回落：没有 {}）",
            fallback.display(),
            legacy_path.display()
        ));
    }

    // 设置项要在起 Bevy 之前读出来：网格资产源必须早于 `AssetPlugin` 注册，
    // 那时候界面还一帧都没画。
    let mut warnings = Vec::new();
    let stored = match settings_store::load() {
        Ok(stored) => stored,
        Err(error) => {
            warnings.push(format!("{error:#}；这次先用默认设置"));
            None
        }
    };
    // 没有设置文件时，服务地址仍旧走环境变量 / 出厂默认那条老路；有文件时它说了算
    // （ADR-0008 的优先级）。
    let settings = stored.unwrap_or_else(|| settings::Settings {
        model_api_url: model_update_api::base_url(),
        data_api_url: data_publish_api::base_url(),
        ..settings::Settings::default()
    });
    let default_mesh_dir =
        settings_store::resolve_mesh_dir("", std::env::var_os(PLANT_MESH_DIR), &asset_root);
    let mesh_dir = settings_store::resolve_mesh_dir(
        &settings.mesh_dir,
        std::env::var_os(PLANT_MESH_DIR),
        &asset_root,
    );
    if !mesh_dir.is_dir() {
        warnings.push(format!(
            "网格目录不存在：{}；三维会一个网格都加载不出来，去设置里改",
            mesh_dir.display()
        ));
    }
    plant_ui_view3d::mesh_source::set_mesh_dir(&mesh_dir);
    settings_store::set_startup(settings_store::Startup {
        settings,
        default_mesh_dir: default_mesh_dir.to_string_lossy().into_owned(),
        warnings,
    });

    run("#plant-ui", asset_root.to_string_lossy().into_owned());
    Ok(())
}

/// 网格目录的环境变量。夹在设置项与出厂默认之间（ADR-0008 的优先级）。
#[cfg(not(target_arch = "wasm32"))]
const PLANT_MESH_DIR: &str = "PLANT_MESH_DIR";

#[cfg(not(target_arch = "wasm32"))]
fn run_gallery() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1000.0])
            .with_title("plant-ui"),
        ..Default::default()
    };
    eframe::run_native(
        "plant-ui",
        options,
        Box::new(move |cc| {
            let (defs, _warnings) = fonts::definitions();
            let weights_ready = defs.font_data.contains_key(fonts::MEDIUM);
            cc.egui_ctx.set_fonts(defs);
            set_weight_families_ready(weights_ready);
            theme_tokens::apply(&cc.egui_ctx, &Tokens::light(), Density::Standard);
            Ok(Box::new(gallery::GalleryApp::default()))
        }),
    )
}

fn run(canvas: &str, asset_root: String) {
    let window = Window {
        title: "plant-ui".into(),
        resolution: (1600.0_f32, 1000.0_f32).into(),
        ..Default::default()
    };
    #[cfg(target_arch = "wasm32")]
    let window = Window {
        canvas: Some(canvas.into()),
        fit_canvas_to_parent: true,
        prevent_default_event_handling: true,
        ..window
    };
    #[cfg(not(target_arch = "wasm32"))]
    let _ = canvas;

    let mut app = BevyApp::new();
    // 网格走自己的资产源，根目录可以在设置里改。**必须先于 `AssetPlugin` 注册**：
    // 源是在那个插件建起来的那一刻定型的，晚一步 Bevy 只会打一行 error 然后无视。
    // 浏览器端没有本地目录，网格与字体贴图一样由站点供给，那一侧不注册这个源
    // （见 `mesh_source::asset_path` 的两个分支）。
    #[cfg(not(target_arch = "wasm32"))]
    {
        use bevy::asset::AssetApp;
        app.register_asset_source(
            plant_ui_view3d::mesh_source::SOURCE_NAME,
            plant_ui_view3d::mesh_source::source_builder(),
        );
    }
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(window),
                ..Default::default()
            })
            .set(bevy::asset::AssetPlugin {
                file_path: asset_root,
                meta_check: AssetMetaCheck::Never,
                ..Default::default()
            }),
    )
    .add_plugins(bevy_wasm_tasks::TasksPlugin::default())
    .add_plugins(EguiPlugin::default())
    .add_plugins(plant_ui_view3d::View3dPlugin)
    .add_systems(bevy::prelude::Startup, setup_ui_camera)
    .add_systems(EguiPrimaryContextPass, show_app);
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(
        PostUpdate,
        (
            sync_desktop_ime.after(EguiPostUpdateSet::ProcessOutput),
            dispatch_egui_screenshots
                .after(EguiPostUpdateSet::EndPass)
                .before(EguiPostUpdateSet::ProcessOutput),
        ),
    );
    app.run();
}

fn setup_ui_camera(mut commands: Commands, mut egui: ResMut<EguiGlobalSettings>) {
    // 三维插件也会创建摄像机；明确绑定，避免 egui 把主界面画进离屏纹理。
    egui.auto_create_primary_context = false;
    commands.spawn((Camera2d, PrimaryEguiContext));
}

#[cfg(not(target_arch = "wasm32"))]
fn sync_desktop_ime(
    output: Query<&EguiOutput, With<PrimaryEguiContext>>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
) {
    let (Ok(output), Ok(mut window)) = (output.single(), window.single_mut()) else {
        return;
    };
    sync_ime_window(&mut window, output.platform_output.ime.as_ref());
}

#[cfg(not(target_arch = "wasm32"))]
fn sync_ime_window(window: &mut Window, ime: Option<&egui::output::IMEOutput>) {
    window.ime_enabled = ime.is_some();
    if let Some(ime) = ime {
        let position = ime.cursor_rect.left_bottom();
        window.ime_position = bevy::math::Vec2::new(position.x, position.y);
    }
}

/// 挂上验收探针的服务端那一半（`egui_inspection`），由 `EGUI_INSPECTION` 环境变量
/// 决定开不开——没设就是个空操作。
///
/// **只有原生端有**：那个 crate 里 `attach_from_env` 是
/// `cfg(not(target_arch = "wasm32"))` 导出的，它靠 `TcpListener` 起服务，浏览器里
/// 压根没有这东西。不门控的话桌面端一切正常、`cargo build --target
/// wasm32-unknown-unknown` 直接 E0425。
#[cfg(not(target_arch = "wasm32"))]
fn attach_inspection(ctx: &egui::Context) {
    if let Err(error) = egui_inspection::attach_from_env(ctx, Some("plant-ui".into())) {
        eprintln!("egui inspection unavailable: {error}");
    }
}

#[cfg(target_arch = "wasm32")]
fn attach_inspection(_ctx: &egui::Context) {}

/// 一次已派发、正等底片回来的截图：`user_data` 是请求方认领它的号牌，
/// `context` / `viewport` 说明回执该塞回哪。
#[cfg(not(target_arch = "wasm32"))]
#[derive(Component)]
struct ScreenshotReplyTo {
    user_data: egui::UserData,
    context: Entity,
    viewport: egui::ViewportId,
}

/// 把 egui 的 `ViewportCommand::Screenshot` 接到 Bevy 的窗口截图上。
///
/// 这条命令在 eframe 那半边由 winit 集成应答，而 `bevy_egui` 的
/// `process_output_system` 只从 `viewport_output` 里取 `repaint_delay`，命令本身
/// 整个丢掉。于是验收探针的 `inspect shot` 发出请求后没有任何人应答，
/// `egui_inspection` 干等满 20 秒超时，报出来的却是「应用没在绘制，把窗口切到前台」
/// —— 那句话在这里是假的：树和注入输入一直是通的，帧也一直在画。
///
/// 必须赶在 `EguiPostUpdateSet::ProcessOutput` 之前跑：那一步会把 `EguiFullOutput`
/// 整个 `take` 走。
#[cfg(not(target_arch = "wasm32"))]
fn dispatch_egui_screenshots(mut commands: Commands, contexts: Query<(Entity, &EguiFullOutput)>) {
    for (context, full_output) in &contexts {
        let Some(output) = full_output.0.as_ref() else {
            continue;
        };
        for (viewport, viewport_output) in output.viewport_output.iter() {
            for command in &viewport_output.commands {
                let egui::ViewportCommand::Screenshot(user_data) = command else {
                    continue;
                };
                commands
                    .spawn((
                        Screenshot::primary_window(),
                        ScreenshotReplyTo {
                            user_data: user_data.clone(),
                            context,
                            viewport: *viewport,
                        },
                    ))
                    .observe(reply_screenshot_to_egui);
            }
        }
    }
}

/// 底片回来了：转成 egui 认的 `ColorImage`，按号牌塞回那个上下文的输入队列，
/// 由 `egui_inspection` 的 `input_hook` 认领。
///
/// 回执要等下一帧的 `EguiInputSet::WriteEguiEvents` 才进 `EguiInput`，比请求晚两三帧
/// —— 探针那边的超时是 20 秒，绰绰有余。
#[cfg(not(target_arch = "wasm32"))]
fn reply_screenshot_to_egui(
    trigger: Trigger<ScreenshotCaptured>,
    replies: Query<&ScreenshotReplyTo>,
    mut egui_input: EventWriter<EguiInputEvent>,
) {
    let Some(target) = trigger.target() else {
        return;
    };
    let Ok(reply) = replies.get(target) else {
        return;
    };
    let dynamic = match trigger.event().0.clone().try_into_dynamic() {
        Ok(dynamic) => dynamic,
        Err(error) => {
            eprintln!("截图格式无法解读，这一次请求作废：{error}");
            return;
        }
    };
    // 开着 HDR 时 alpha 通道装的是亮度而不是透明度，照搬会得到一张半透明的图；
    // Bevy 自己的 `save_to_disk` 也是丢掉 alpha 的。
    let rgb = dynamic.to_rgb8();
    let size = [rgb.width() as usize, rgb.height() as usize];
    egui_input.write(EguiInputEvent {
        context: reply.context,
        event: egui::Event::Screenshot {
            viewport_id: reply.viewport,
            user_data: reply.user_data.clone(),
            image: std::sync::Arc::new(egui::ColorImage::from_rgb(size, rgb.as_raw())),
        },
    });
}

fn show_app(
    mut contexts: EguiContexts,
    mut app: bevy::prelude::Local<Option<App>>,
    mut view3d: bevy::prelude::ResMut<plant_ui_view3d::View3d>,
    tasks: bevy_wasm_tasks::Tasks,
) -> bevy::prelude::Result {
    let ctx = contexts.ctx_mut()?;
    let first_frame = app.is_none();
    if first_frame {
        attach_inspection(ctx);
    }
    let app = app.get_or_insert_with(|| {
        let (defs, warnings) = fonts::definitions();
        let weights_ready = defs.font_data.contains_key(fonts::MEDIUM);
        ctx.set_fonts(defs);
        // Bevy 在当前 egui pass 结束后才应用 set_fonts；本帧启用命名字体会直接 panic。
        set_weight_families_ready(false);
        theme_tokens::apply(ctx, &Tokens::light(), Density::Standard);
        App::new(ctx.clone(), warnings, weights_ready, &tasks)
    });
    if !first_frame && app.font_weights_pending {
        app.font_weights_pending = false;
        set_weight_families_ready(true);
        theme_tokens::apply(
            ctx,
            &app.settings_state.saved.theme.tokens(),
            app.settings_state.saved.density,
        );
    }
    if let Some(refno) = view3d.take_picked() {
        app.handle_cmds(ctx, vec![Cmd::LocateElement(refno)]);
    }
    if let Some(progress) = view3d.take_mesh_progress() {
        app.sync_mesh_progress(progress);
    }
    let render_states = view3d.take_render_states();
    if !render_states.is_empty() {
        app.sync_render_states(render_states);
    }
    app.vm.view3d = Some(View3dVm {
        texture: view3d.texture,
        size: egui::vec2(view3d.size.x as f32, view3d.size.y as f32),
        live: true,
        measurement_active: false,
        camera_rot: view3d.camera_rot,
        axis_labels: view3d.axis_labels,
        grid_cell_mm: view3d.grid_cell_mm,
    });
    let size = ctx.content_rect().size();
    egui::Area::new("plant-workbench".into())
        .fixed_pos(egui::Pos2::ZERO)
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            ui.set_min_size(size);
            ui.set_max_size(size);
            app.ui(ui);
        });
    view3d.set_selection(app.vm.selection.to_vec());
    if let Some(models) = app.pending_models.take() {
        view3d.load(models);
    }
    for models in app.pending_incremental_models.drain(..) {
        view3d.append(models);
    }
    let mut retried = 0;
    for command in app.view3d_commands.drain(..) {
        match command {
            Cmd::PickViewport(uv) => view3d.pick(uv),
            Cmd::Camera(gesture) => view3d.camera(gesture),
            Cmd::Model(action) => view3d.model(action),
            Cmd::ResizeViewport(size) => view3d.resize(size),
            Cmd::SetViewportBackground { top, bottom, grid } => {
                view3d.set_background(top, bottom, grid)
            }
            Cmd::SnapView { forward, up, fit } => view3d.snap(forward, up, fit),
            Cmd::RetryFailedMeshes => retried = view3d.retry_failed_meshes(),
            _ => unreachable!("只缓存三维命令"),
        }
    }
    if retried > 0 {
        app.logs.info(
            &mut app.vm.logs,
            format!("正在重试 {retried} 个加载失败的网格"),
        );
    }
    Ok(())
}

/// 一次还没落地的树定位（ADR-0014）。
///
/// 「落地」= 目标那一行真的出现在展平后的可见行里。在那之前 `vm.tree_reveal`
/// 得一直挂着：祖先的子层是异步回来的，滚动请求只活一帧的话，它会在行还不存在时
/// 就被消费掉，然后树停在原地。
struct PendingLocate {
    target: RefU64,
    /// 目标在当前 MDB 内的祖先，由近到远。祖先链还在查的时候是空的。
    /// 用户手动折叠其中任何一节就等于放弃这次定位。
    path: Vec<RefU64>,
    /// 由命令行发起。回执要写回命令会话，树上点日志过来的那些不写。
    from_command: bool,
    /// 命令会话里已经报过这次定位的结果。发起时就能展开到位的那种当场就报了，
    /// 等祖先链的那种得等真滚过去才报，两者不许都报一遍。
    announced: bool,
}

/// 一次树定位的去向。
enum Locate {
    /// 祖先链已在手上，这一帧就展开到位（缺的子层另走异步）。
    Ready,
    /// 目标还没物化，正在查祖先链，滚动等它回来。
    Resolving,
}

/// App 侧的树结构缓存与展开状态；`vm.tree` 是它按展开状态展平后的产物。
#[derive(Default)]
struct TreeModel {
    /// SITE 根层（MDB 世界的下一层）。
    roots: Vec<EleTreeNode>,
    /// 已加载的子层，按父 refno 缓存。
    children: HashMap<RefU64, Vec<EleTreeNode>>,
    /// 子 -> 父。命令行定位要顺着它把挡在目标前面的祖先一路展开。
    parent: HashMap<RefU64, RefU64>,
    expanded: HashSet<RefU64>,
    /// 子层查询在途的节点。
    loading: HashSet<RefU64>,
    /// 三维回执上来的**实际渲染结果**，按模型参考号记（ADR-0016）。
    /// 树上任何一行的 eye 都由它聚合出来，容器行也不例外。
    visibility: HashMap<RefU64, ModelVisibility>,
    /// **待执行方向**：点过眼睛、指令还没落到画面上的那些目标（可以是容器行）。
    /// 它只决定「下一次点击往哪边翻」和「查询回来之后照哪个方向操作模型」，
    /// 不参与画 eye——画 eye 的是 `visibility`。
    pending_direction: HashMap<RefU64, bool>,
    /// 一次操作开始前的 eye。多模型范围分批回执时先保持这个状态，整组落地后再切换。
    pending_visibility: HashMap<RefU64, RowVisibility>,
    /// 该行已经明确查空或查询失败，不能再从已显示的祖先继承 eye。
    visibility_unavailable: HashSet<RefU64>,
}

/// 一个模型的实际渲染结果，来自 View3d 的回执。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelVisibility {
    visible: bool,
    mesh_loaded: usize,
    mesh_failed: usize,
}

impl ModelVisibility {
    /// 一个网格都没画出来——三维里等于没有它，eye 该退回「未加载」。
    fn nothing_rendered(self) -> bool {
        self.mesh_loaded == 0
    }

    /// 画出来了，但有网格没成：看到的只是这个模型的一部分。
    fn partly_rendered(self) -> bool {
        self.mesh_loaded > 0 && self.mesh_failed > 0
    }
}

/// 一行的 eye：把它那次显示解析出来的模型的实际状态聚合起来。
///
/// 几何行的范围就是它自己（`cache_model_scope` 给每个模型都记了一条自指），
/// 所以容器行与几何行走同一段代码，不用分两套。
fn row_visibility(
    models: Option<&Vec<RefU64>>,
    actual: &HashMap<RefU64, ModelVisibility>,
) -> RowVisibility {
    let Some(models) = models else {
        return RowVisibility::Unloaded;
    };
    let mut shown = 0usize;
    let mut hidden = 0usize;
    let mut unavailable = 0usize;
    let mut partial = false;
    for refno in models {
        match actual.get(refno) {
            Some(state) if !state.nothing_rendered() => {
                if state.visible {
                    shown += 1;
                    partial |= state.partly_rendered();
                } else {
                    hidden += 1;
                }
            }
            _ => unavailable += 1,
        }
    }
    if shown + hidden == 0 {
        RowVisibility::Unloaded
    } else if partial || unavailable > 0 || (shown > 0 && hidden > 0) {
        RowVisibility::Partial
    } else if shown > 0 {
        RowVisibility::Shown
    } else {
        RowVisibility::Hidden
    }
}

/// 点这一行眼睛该下什么方向。
///
/// 有待执行方向就反转**上一次指令**：图标这时候还停在旧样子，拿它反推会把
/// 「显示到一半再点一下」变成又一次显示。没有在途指令时才照实际状态推。
fn next_click_visible(pending: Option<bool>, visibility: RowVisibility) -> bool {
    match pending {
        Some(direction) => !direction,
        None => visibility.on_click(),
    }
}

/// 待执行方向什么时候算落地：范围里每个模型的实际状态都到齐，且方向对上了。
///
/// 「一个网格都没画出来」也算到齐——那种模型永远对不上方向，再等就是死等。
fn direction_settled(
    models: &[RefU64],
    want: bool,
    actual: &HashMap<RefU64, ModelVisibility>,
) -> bool {
    !models.is_empty()
        && models.iter().all(|refno| {
            actual
                .get(refno)
                .is_some_and(|state| state.visible == want || state.nothing_rendered())
        })
}

fn settle_pending_directions(
    pending: &mut HashMap<RefU64, bool>,
    previous: &mut HashMap<RefU64, RowVisibility>,
    scopes: &HashMap<RefU64, Vec<RefU64>>,
    actual: &HashMap<RefU64, ModelVisibility>,
) {
    pending.retain(|target, want| {
        // 范围还没查回来的继续挂着：这一次点击连往哪些模型下手都还不知道。
        scopes
            .get(target)
            .is_none_or(|models| !direction_settled(models, *want, actual))
    });
    previous.retain(|target, _| pending.contains_key(target));
}

impl TreeModel {
    /// 当前行的 eye。没有独立模型范围的树行继承最近祖先的实际状态；
    /// 一旦自己查过范围（含明确查空 / 失败），就以自己的结果为准。
    fn visibility_for(
        &self,
        refno: RefU64,
        scopes: &HashMap<RefU64, Vec<RefU64>>,
    ) -> RowVisibility {
        let mut current = Some(refno);
        for _ in 0..=self.parent.len() {
            let Some(row) = current else {
                break;
            };
            if let Some(visibility) = self.pending_visibility.get(&row) {
                return *visibility;
            }
            if self.visibility_unavailable.contains(&row) {
                return RowVisibility::Unloaded;
            }
            if let Some(models) = scopes.get(&row) {
                return row_visibility(Some(models), &self.visibility);
            }
            current = self.parent.get(&row).copied();
        }
        RowVisibility::Unloaded
    }

    /// 按展开状态 DFS 展平可见行。只在结构变化时调用，绘制层逐帧只读。
    fn flatten(&self, scopes: &HashMap<RefU64, Vec<RefU64>>) -> Vec<TreeRowVm> {
        fn walk(
            model: &TreeModel,
            scopes: &HashMap<RefU64, Vec<RefU64>>,
            nodes: &[EleTreeNode],
            depth: u16,
            rows: &mut Vec<TreeRowVm>,
        ) {
            for n in nodes {
                let refno = n.refno.refno();
                let expandable = (n.children_count > 0).then(|| model.expanded.contains(&refno));
                let visibility = model.visibility_for(refno, scopes);
                rows.push(TreeRowVm {
                    refno,
                    depth,
                    name: n.name.clone(),
                    noun: n.noun.clone(),
                    expandable,
                    loading: model.loading.contains(&refno),
                    visibility,
                    next_visible: next_click_visible(
                        model.pending_direction.get(&refno).copied(),
                        visibility,
                    ),
                });
                if expandable == Some(true)
                    && let Some(kids) = model.children.get(&refno)
                {
                    walk(model, scopes, kids, depth + 1, rows);
                }
            }
        }
        let mut rows = Vec::new();
        walk(self, scopes, &self.roots, 0, &mut rows);
        rows
    }

    fn element_count(&self) -> usize {
        self.roots.len() + self.children.values().map(Vec::len).sum::<usize>()
    }

    /// 命令行里指认某个节点用的短名，形如 `EQUI /VESSEL-01`（name 自带前导斜杠）。
    /// 缓存里找不到就退回 refno——它至少还能在树上定位。
    fn label(&self, refno: RefU64) -> String {
        self.roots
            .iter()
            .chain(self.children.values().flatten())
            .find(|n| n.refno.refno() == refno)
            .map(|n| format!("{} {}", n.noun, n.name))
            .unwrap_or_else(|| refno.to_string())
    }

    /// 日志行里的元素引用。名字按当下的缓存取一次，之后不再回填。
    fn element(&self, refno: RefU64) -> LogElement {
        LogElement {
            refno,
            label: self.label(refno),
        }
    }

    fn is_root(&self, refno: RefU64) -> bool {
        self.roots.iter().any(|n| n.refno.refno() == refno)
    }

    /// 取回工作之后的清扫：把从根层已经走不到的条目摘掉。
    ///
    /// 一次重查可以让整条分支消失——元素被删了，或者挪到了别的 OWNER 底下——
    /// 而它底下那些子层、`parent` 指向、展开标记还留在表里。不摘掉的话
    /// `ancestors` 会顺着 `parent` 走进一串已经不存在的祖先，状态栏的元素计数
    /// 也会一直虚高。
    fn prune_unreachable(&mut self) {
        let mut alive: HashSet<RefU64> = HashSet::new();
        let mut stack: Vec<RefU64> = self.roots.iter().map(|n| n.refno.refno()).collect();
        while let Some(refno) = stack.pop() {
            // 数据异常造出环时这里靠 `alive` 收敛，不会转不出去。
            if !alive.insert(refno) {
                continue;
            }
            if let Some(kids) = self.children.get(&refno) {
                stack.extend(kids.iter().map(|n| n.refno.refno()));
            }
        }
        self.children.retain(|refno, _| alive.contains(refno));
        self.parent.retain(|refno, _| alive.contains(refno));
        self.expanded.retain(|refno| alive.contains(refno));
        self.loading.retain(|refno| alive.contains(refno));
        self.visibility_unavailable
            .retain(|refno| alive.contains(refno));
    }

    /// 从最近的父到 SITE 根的祖先链；不在已加载的树里就是 None。
    ///
    /// 只读缓存，不打库。命中的是常见那一半：能被日志提到的元素都是从某个父层查
    /// 回来的，那一层连同它上面每一层通常都还在缓存里。命令行里直接敲一个参考号、
    /// 或者取回工作刚把某条分支摘掉时会落空，那时定位改走 `Req::Ancestors` 现查
    /// 一条（ADR-0014）。
    fn ancestors(&self, refno: RefU64) -> Option<Vec<RefU64>> {
        if self.is_root(refno) {
            return Some(Vec::new());
        }
        let mut chain = Vec::new();
        let mut cur = refno;
        // 数据异常造出环的话这里会转不出去，按缓存规模封顶兜一下——挂住 UI 线程
        // 比定位错了严重得多。
        for _ in 0..=self.parent.len() {
            let p = *self.parent.get(&cur)?;
            chain.push(p);
            if self.is_root(p) {
                return Some(chain);
            }
            cur = p;
        }
        None
    }
}

struct App {
    vm: WorkbenchVm,
    state: WorkbenchState,
    model_update: ModelUpdateVm,
    model_update_state: ModelUpdateState,
    /// 房间浏览器的全表数据（重查询，只在打开浮窗 / 手动刷新时拉）。
    rooms_overview: RoomBrowserVm,
    room_browser_state: RoomBrowserState,
    data_publish_state: DataPublishState,
    project_picker_state: ProjectPickerState,
    settings_state: SettingsState,
    model_api_url: String,
    data_api_url: String,
    /// 当前 MDB 名（带前导 `/`），连库时取回。模型更新的预览与执行都要带上它
    /// ——本期执行范围就是照这个 MDB 解出来的 DESI 库号。
    mdb: String,
    /// 当前数据源的 Surreal namespace；与项目、MDB 一起校验 gen-model 固定服务范围。
    namespace: String,
    /// 任务队列视图的数据。它不跟着任何一次运行走——队列是常驻的，进程活着它就在。
    queue: task_queue::Vm,
    /// 队列的明细长连接：订阅全部任务，逐条带 task_id 回来。
    queue_feed: Option<model_update_ws::Feed>,
    last_queue_poll: Instant,
    queue_poll_pending: bool,
    /// 已经见过终态的数据批次。轮询靠它认出「这一拍新跑完了哪几个」——
    /// 终态之后快照还会带着它们好几拍，每拍都刷一次就是反复拆装同一批几何。
    queue_finished: HashSet<String>,
    /// 正在现查祖先链、好找出「该刷新哪一层」的那些元素（新增子树的单元根 / OWNER）。
    /// 记着是为了不重复发同一条查询，也为了把回来的链与定位那条路分开。
    refresh_anchors: HashSet<RefU64>,
    /// 根层快照开始读取的时刻。首次队列快照只补这之后完成的批次。
    data_observed_at: Option<DateTime<Utc>>,
    /// 首份可与根层快照比较的队列快照是否已经登记。
    queue_refresh_baselined: bool,
    /// 已经发出去还没回来的那次取回工作。菜单点快了不该排成一串重查。
    get_work_pending: bool,
    /// 忙时又来的刷新只合并成下一次；`true` 表示至少有一次模型已生成完成，
    /// 下一次必须重载三维，`false` 只刷新树与属性。
    get_work_again: Option<bool>,
    /// 当前 / 下一次取回工作完成后，可在终态行确认“模型树已就地刷新”的任务。
    get_work_task_ids: HashSet<String>,
    get_work_again_task_ids: HashSet<String>,
    /// 数据已经应用，但三维仍在等后台模型全部生成完。
    model_reload_owed: bool,
    /// 已经发出一次为清偿欠账的模型刷新；失败时必须把欠账恢复。
    model_reload_in_flight: bool,
    /// 取回工作清场前的场景快照：(模型 refno, 取回前是否可见)。只记第一次——
    /// 清场之后场景一直是空的，重试轮次再拍只会拍到空白；重载成功落地才消费，
    /// 失败后的欠账补载仍按这份最初的快照回放。
    model_reload_restore: Option<Vec<(RefU64, bool)>>,
    /// 增量整场查询已经完成，正等 View3d 报 mesh/AABB 全部落地。
    refresh_generation_pending: bool,
    /// 还没回包的那一次房间视图请求（`Cmd::FocusRoom` -> `Req::RoomDetail`）。
    /// 同时只留一个：连点两间房只聚焦最后一间，晚到的旧详情靠它认出来丢掉。
    focus_room_pending: Option<RefU64>,
    /// 选中 PANEL 后等待房间详情的 X-Ray 请求：(发起时的 PANEL, 房间)。
    /// 选中一换立即清空；晚到的详情只有同时匹配这两个值才可改材质。
    xray_room_pending: Option<(RefU64, RefU64)>,
    xray_active_room: Option<RefU64>,
    /// 已查过的房间 -> 全部在册面板。重复点同一间房不再打一次详情查询。
    room_panel_cache: HashMap<RefU64, Vec<RefU64>>,
    /// 模型还在路上、等它落地就取景的那间房（`Cmd::ShowRoomModel`）。
    ///
    /// 取景不能跟着指令一起下发：视口的包围盒来自模型查询回包里的 `world_aabb`，
    /// 范围查询还没回来时它手上没有这间房的任何包围盒，`FocusGroup` 会判成
    /// 「一个都没加载」而不动相机。同时只留一个，连点两间房只取景最后一间。
    pending_room_frame: Option<RefU64>,
    /// 「房间」页签当前聚焦的房间。与上面那个分开：视口那半回包即清（一次性
    /// 动作），页签这半要一直立着——切换选中或重连才归零。
    room_pane_focus: Option<RefU64>,
    bridge: data::Bridge,
    tree: TreeModel,
    /// 还没落地的那一次树定位。同时只留一个：连续定位只完成最后一次。
    pending_locate: Option<PendingLocate>,
    logs: logs::LogBuffer,
    pending_models: Option<Vec<aios_core::GeomInstQuery>>,
    pending_incremental_models: Vec<Vec<aios_core::GeomInstQuery>>,
    /// tree/container refno -> 实际可见 ModelRoot refno。
    model_scopes: HashMap<RefU64, Vec<RefU64>>,
    model_scope_pending: HashSet<RefU64>,
    model_scope_epoch: u64,
    /// `clear` bumps this generation so replies from the cleared session are ignored.
    command_epoch: u64,
    loaded_models: HashSet<RefU64>,
    model_show_waiting: HashSet<RefU64>,
    latest_mesh_progress: Option<plant_ui_view3d::MeshLoadProgress>,
    model_scope_failures: usize,
    model_progress_until: Option<Instant>,
    view3d_commands: Vec<Cmd>,
    /// 上一次真的落到文件系统上校验过的那个网格目录草稿。
    ///
    /// 逐帧去问一次盘，在网络盘上会把界面拖住；对着这份留底比对，只有字面变过
    /// 才重新校验一次。
    #[cfg(not(target_arch = "wasm32"))]
    checked_mesh_dir: Option<String>,
    /// 正开着的目录对话框。对话框是阻塞的，只能在别的线程上等，结果经这条
    /// 一次性通道回来。
    #[cfg(not(target_arch = "wasm32"))]
    mesh_dir_pick: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    font_weights_pending: bool,
    /// 上一次发给三维视口的主题配色。视口的渐变背景画在宿主侧（拷问定案第 2 题），
    /// 主题一换就要把新颜色送下去；对着这份留底比对，别把同一套颜色每帧发一遍。
    sent_viewport_bg: Option<(egui::Color32, egui::Color32, egui::Color32)>,
}

const COMMAND_CAP: usize = 2000;

fn command_reply_is_current(reply_epoch: u64, command_epoch: u64) -> bool {
    reply_epoch == command_epoch
}

/// 队列里有活在跑时的轮询节奏，与 ADR-0005 / 0007 给进度定的那一拍一致。
const QUEUE_POLL_BUSY: Duration = Duration::from_secs(1);
/// 队列全空时降到这一档。一份最多 200 条的任务表每秒拉一遍、只为看几行不会动的
/// 历史，不值当；有活时 WS 的起讫信封还会把轮询提前叫醒。
const QUEUE_POLL_IDLE: Duration = Duration::from_secs(5);

fn begin_get_work(pending: bool, reload_models: bool, deferred: &mut Option<bool>) -> bool {
    if pending {
        *deferred = Some(deferred.unwrap_or(false) || reload_models);
        false
    } else {
        true
    }
}

/// 取回工作清场前的快照：场景里每个模型 refno 配上「取回前是否可见」。
///
/// 方向的优先序：用户最后一次指令（`pending_direction`，模型行直接命中）>
/// 三维实际回执（`visibility`）> 可见。最后一档兜的是「刚被显示指令带进场景、
/// 回执还没上来」的模型。容器行上还没落地的在途指令拍不进来——反解要走范围表，
/// 清场重装拿到的就是那条指令发出前的样子，边缘窄且诚实。
fn reload_snapshot(
    loaded: &HashSet<RefU64>,
    pending_direction: &HashMap<RefU64, bool>,
    visibility: &HashMap<RefU64, ModelVisibility>,
) -> Vec<(RefU64, bool)> {
    let mut snapshot: Vec<(RefU64, bool)> = loaded
        .iter()
        .map(|refno| {
            let visible = pending_direction
                .get(refno)
                .copied()
                .or_else(|| visibility.get(refno).map(|actual| actual.visible))
                .unwrap_or(true);
            (*refno, visible)
        })
        .collect();
    // HashSet 迭代序不稳定，而重查请求与日志都吃这份序：排一下，可复现。
    snapshot.sort_by_key(|(refno, _)| refno.0);
    snapshot
}

/// 重载落地这一刻要回放的隐藏集：快照里方向为隐藏、且这次真的查了回来的那批。
/// 快照就此消费——回放只认清场那一刻的样子，成功之后它的使命就结束了。
fn take_hidden_for_replay(
    restore: &mut Option<Vec<(RefU64, bool)>>,
    loaded: &HashSet<RefU64>,
) -> Vec<RefU64> {
    restore
        .take()
        .unwrap_or_default()
        .into_iter()
        .filter(|(refno, visible)| !visible && loaded.contains(refno))
        .map(|(refno, _)| refno)
        .collect()
}

fn task_matches_project(task_project: &str, project: &str) -> bool {
    task_project.is_empty() || project.is_empty() || task_project == project
}

fn finished_after(finished_at: Option<&str>, observed_at: DateTime<Utc>) -> bool {
    finished_at
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map_or(true, |finished| finished.with_timezone(&Utc) >= observed_at)
}

/// 祖先链里属于当前 MDB 模型树的那一段，由近到远，**不含目标本人**。
///
/// `chain` 是查询侧给的「自己 -> 上级 -> …」序（`fn::ancestor`，第 0 项是目标）。
/// 从里面找第一个 SITE 根当落脚点，根之上的层属于别的 MDB，一律截掉——树定位
/// 不切换 MDB。
///
/// `None` = 链里没有落脚点，目标是树外元素。目标自己就是 SITE 根时返回空路径：
/// 它那一行本来就在，不用展开谁。
/// 新增子树该刷新哪一层：祖先链上**第一个已经展开过**的祖先。
///
/// 链是「自己 -> 上级 -> …」序，所以第一个命中的自然是离新元素最近、又确实在树上
/// 画着的那一层——刷它一次，新元素就进了它的子层列表。
///
/// `None` = 整条链都没展开过。那时候什么也不做是对的：那条分支下次展开时本来就会
/// 现查，现在去拉是把一次精确刷新做成一次预取。
fn refresh_anchor(chain: &[RefU64], cached: impl Fn(RefU64) -> bool) -> Option<RefU64> {
    chain.iter().copied().find(|a| cached(*a))
}

/// 展开定位路径时，这一节要不要（重新）打一次子层查询。
///
/// 平时「缓存里有」就够了。但**目标的直接父那一节是个例外**：祖先链是刚从库里
/// 查回来的，它说的是目标**此刻**挂在谁下面，而那个父的子层缓存可能比目标还老。
/// 增量更新之后从更新面板或日志跳过去正是这个形状——元素是刚建的，父分支却在
/// 更新之前就展开过。缓存命中就跳过的话那一行永远出不来，而收尾那句还会把
/// 「缓存旧了」讲成「它已不在这条路径下」（gen-model issue #9）。
fn needs_children_query(is_freshly_resolved_parent: bool, cached: bool) -> bool {
    is_freshly_resolved_parent || !cached
}

fn in_mdb_path(chain: &[RefU64], is_root: impl Fn(RefU64) -> bool) -> Option<Vec<RefU64>> {
    let anchor = chain.iter().position(|ancestor| is_root(*ancestor))?;
    Some(chain[1..=anchor].to_vec())
}

fn background_models_settled(
    pending_known: bool,
    pending_empty: bool,
    queue_empty: bool,
    model_drains_known: bool,
    model_drains_idle: bool,
) -> bool {
    pending_known && pending_empty && queue_empty && model_drains_known && model_drains_idle
}

fn model_drains_idle(tasks: &[task_queue::TaskEntry], project: &str) -> bool {
    tasks.iter().all(|task| {
        task.kind != task_queue::KIND_MODEL_DRAIN
            || !task_matches_project(&task.project, project)
            || task.terminal()
    })
}

fn complete_refresh_generation(pending: &mut bool, generation: &mut u64) -> bool {
    if !std::mem::take(pending) {
        return false;
    }
    *generation = generation.saturating_add(1);
    true
}

fn settle_refresh_generation(pending: &mut bool, generation: &mut u64, mesh_ok: bool) -> bool {
    if mesh_ok {
        complete_refresh_generation(pending, generation)
    } else {
        *pending = false;
        false
    }
}

fn model_reload_due(owed: &mut bool, refresh_observed: bool, models_settled: bool) -> bool {
    *owed |= refresh_observed;
    models_settled && *owed
}

fn restore_model_reload(owed: &mut bool, failed_debt_reload: bool) {
    *owed |= failed_debt_reload;
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ModelVisibilityPlan {
    resolved: Vec<RefU64>,
    query: Vec<RefU64>,
}

fn model_visibility_plan(
    targets: &[RefU64],
    scopes: &HashMap<RefU64, Vec<RefU64>>,
    pending: &HashSet<RefU64>,
) -> ModelVisibilityPlan {
    let mut plan = ModelVisibilityPlan::default();
    let mut seen = HashSet::new();
    for target in targets {
        if let Some(models) = scopes.get(target) {
            plan.resolved
                .extend(models.iter().copied().filter(|refno| seen.insert(*refno)));
        } else if !pending.contains(target) {
            plan.query.push(*target);
        }
    }
    plan
}

/// 房间边只挂叶子构件；隐含直管则以 BRAN/HANG refno 渲染。把已经加载的、
/// 且与房间成员相交的模型范围一并纳入隔离，避免隔离动作把同支管直段藏掉。
fn expand_room_model_targets(
    targets: Vec<RefU64>,
    scopes: &HashMap<RefU64, Vec<RefU64>>,
) -> Vec<RefU64> {
    let mut seen: HashSet<RefU64> = targets.iter().copied().collect();
    let mut expanded = targets;
    for models in scopes.values() {
        if models.iter().any(|refno| seen.contains(refno)) {
            expanded.extend(models.iter().copied().filter(|refno| seen.insert(*refno)));
        }
    }
    expanded
}

fn cache_model_scope(scopes: &mut HashMap<RefU64, Vec<RefU64>>, target: RefU64, model: RefU64) {
    let models = scopes.entry(target).or_default();
    if !models.contains(&model) {
        models.push(model);
    }
}

fn claim_unloaded_model_refnos(
    loaded: &mut HashSet<RefU64>,
    incoming: impl IntoIterator<Item = RefU64>,
) -> HashSet<RefU64> {
    let claimed = incoming
        .into_iter()
        .filter(|refno| !loaded.contains(refno))
        .collect::<HashSet<_>>();
    loaded.extend(claimed.iter().copied());
    claimed
}

/// 查空、查失败：这次点击什么都没落到三维上，待执行方向撤掉，eye 留在未加载。
fn mark_model_scope_unavailable(
    pending_direction: &mut HashMap<RefU64, bool>,
    visibility_unavailable: &mut HashSet<RefU64>,
    failures: &mut usize,
    target: RefU64,
) {
    pending_direction.remove(&target);
    visibility_unavailable.insert(target);
    *failures += 1;
}

fn model_progress_terminal(pending_meshes: usize, failures: usize) -> Option<bool> {
    (pending_meshes == 0).then_some(failures == 0)
}

fn settled_pending_roots(
    loaded: bool,
    current_known: bool,
    previous: &[model_update::PendingModelUnit],
    current: &[model_update::PendingModelUnit],
) -> Vec<model_update::RefreshUnit> {
    // ponytail: whole-scene refresh waits for zero debt; use per-root scene patches
    // if one permanently bad root blocking other visual updates becomes unacceptable.
    if !loaded || !current_known || !current.is_empty() {
        return Vec::new();
    }
    previous
        .iter()
        .filter_map(|unit| {
            let root = unit.root_refno.parse::<RefU64>().ok()?;
            root.is_valid().then_some(model_update::RefreshUnit {
                root,
                old_owner: None,
                new_owner: None,
            })
        })
        .collect()
}

/// 解出这一刻**实际生效**的接入点。
///
/// 走 `try_get_db_option`：`get_db_option` 读不着配置时会 panic，而「读不着」恰恰是这块
/// 面板要说清楚的情形之一——为它崩掉整个界面说不过去。解不出来就留空，面板照画，
/// 每一格显示「未配置」。
fn access_point_vm(model_api_url: &str, data_api_url: &str) -> AccessPointVm {
    let mut vm = AccessPointVm {
        model_api_url: model_api_url.to_owned(),
        data_api_url: data_api_url.to_owned(),
        source: startup::config_source().unwrap_or("未记录").to_owned(),
        ..Default::default()
    };
    if let Ok(db) = aios_core::try_get_db_option() {
        vm.db_url = db.get_version_db_conn_str();
        vm.namespace = db.surreal_ns.clone();
        vm.database = db.project_name.clone();
        // 与 `plant_ui_data::project_identity` 同一把尺子：配空时回空串，
        // 不摆一个谁都不是的 `/`。
        vm.mdb = match db.mdb_name.trim() {
            "" => String::new(),
            name => aios_core::helper::to_e3d_name(name).into_owned(),
        };
        vm.user = db.v_user.clone();
    }
    vm
}

impl App {
    fn new(
        ctx: egui::Context,
        font_warnings: Vec<String>,
        font_weights_pending: bool,
        tasks: &bevy_wasm_tasks::Tasks<'_>,
    ) -> Self {
        let startup = settings_store::startup();
        let mut adopted = startup
            .map(|startup| startup.settings.clone())
            .unwrap_or_default();
        if startup.is_none() {
            // 宿主没交底（浏览器端）：服务地址仍旧由 `base_url()` 那条链解出来。
            adopted.model_api_url = model_update_api::base_url();
            adopted.data_api_url = data_publish_api::base_url();
        }
        let model_api_url = adopted.model_api_url.clone();
        let data_api_url = adopted.data_api_url.clone();
        let mut settings_state = SettingsState::default();
        settings_state.mesh_dir_hint = startup
            .map(|startup| startup.default_mesh_dir.clone())
            .unwrap_or_default();
        settings_state.adopt(adopted);
        let mut app = Self {
            // 连接前不摆任何工程数据：项目 / 库标识等 Ready 事件带真实值。
            // 接入点是例外——它说的是「这次冲着谁去」，连不上时反而最该看得见。
            vm: WorkbenchVm {
                user: std::env::var("USERNAME").unwrap_or_else(|_| "user".into()),
                access_point: access_point_vm(&model_api_url, &data_api_url),
                ..Default::default()
            },
            state: WorkbenchState::default(),
            model_update: ModelUpdateVm::default(),
            model_update_state: ModelUpdateState::default(),
            rooms_overview: RoomBrowserVm::default(),
            room_browser_state: RoomBrowserState::default(),
            data_publish_state: DataPublishState::default(),
            project_picker_state: ProjectPickerState::new(Vec::new(), false),
            settings_state,
            model_api_url,
            data_api_url,
            mdb: String::new(),
            namespace: String::new(),
            queue: task_queue::Vm::default(),
            queue_feed: None,
            // 开机就欠一拍：第一帧立刻去取第一份快照，别让面板空等一秒。
            last_queue_poll: Instant::now()
                .checked_sub(QUEUE_POLL_BUSY)
                .unwrap_or_else(Instant::now),
            queue_poll_pending: false,
            queue_finished: HashSet::new(),
            refresh_anchors: HashSet::new(),
            data_observed_at: None,
            queue_refresh_baselined: false,
            get_work_pending: false,
            get_work_again: None,
            get_work_task_ids: HashSet::new(),
            get_work_again_task_ids: HashSet::new(),
            model_reload_owed: false,
            model_reload_in_flight: false,
            model_reload_restore: None,
            refresh_generation_pending: false,
            focus_room_pending: None,
            xray_room_pending: None,
            xray_active_room: None,
            room_panel_cache: HashMap::new(),
            pending_room_frame: None,
            room_pane_focus: None,
            bridge: data::spawn(ctx.clone(), tasks),
            tree: TreeModel::default(),
            pending_locate: None,
            logs: logs::LogBuffer::default(),
            pending_models: None,
            pending_incremental_models: Vec::new(),
            model_scopes: HashMap::new(),
            model_scope_pending: HashSet::new(),
            model_scope_epoch: 0,
            command_epoch: 0,
            loaded_models: HashSet::new(),
            model_show_waiting: HashSet::new(),
            latest_mesh_progress: None,
            model_scope_failures: 0,
            model_progress_until: None,
            view3d_commands: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            checked_mesh_dir: None,
            #[cfg(not(target_arch = "wasm32"))]
            mesh_dir_pick: None,
            font_weights_pending,
            sent_viewport_bg: None,
        };
        // 队列的明细通道进程一起来就开：队列是常驻视图，不像向导那样跟着某一次
        // 运行开合。连不上不影响队列行本身——那些走轮询，断的只是逐单元明细。
        app.reopen_queue_feed(&ctx);
        for w in font_warnings {
            app.logs.warn(&mut app.vm.logs, w);
        }
        for w in startup.iter().flat_map(|startup| &startup.warnings) {
            app.logs.warn(&mut app.vm.logs, w.clone());
        }
        app.logs.info(&mut app.vm.logs, "正在连接数据源…");
        app
    }

    fn pump_events(&mut self) {
        if let Some(feed) = &mut self.queue_feed {
            feed.poll();
        }
        let mut dirty = false;
        while let Ok(evt) = self.bridge.evt.try_recv() {
            match evt {
                data::Evt::Ready(Ok(info)) => {
                    let msg = format!(
                        "已连接 {}（{}），根层 {} 个 SITE",
                        info.project,
                        db_label(&info.ns, &info.db_nums),
                        info.sites.len()
                    );
                    self.logs.info(&mut self.vm.logs, msg);
                    self.vm.data_source_ok = true;
                    self.vm.project = info.project;
                    self.mdb = info.mdb;
                    self.namespace = info.ns.clone();
                    self.data_observed_at = Some(info.observed_at);
                    self.queue_refresh_baselined = false;
                    self.queue_finished.clear();
                    self.refresh_anchors.clear();
                    self.queue.project = self.vm.project.clone();
                    self.queue.mdb = self.mdb.clone();
                    self.queue.namespace = self.namespace.clone();
                    self.vm.project_code = info.ns;
                    self.tree.roots = info.sites;
                    let _ = self.bridge.req.send(data::Req::PendingSessions);
                    dirty = true;
                }
                data::Evt::Ready(Err(e)) => {
                    self.vm.tree =
                        TreeVm::Failed(format!("数据源连接失败：{}", logs::error_chain(&e)));
                    self.logs.error(
                        &mut self.vm.logs,
                        "数据源连接失败",
                        &e,
                        Some(Retry::Connect),
                    );
                }
                data::Evt::Children(refno, Ok(kids)) => {
                    self.tree.loading.remove(&refno);
                    let el = self.tree.element(refno);
                    let msg = format!("展开：{} 个子元素", kids.len());
                    self.logs.info_of(&mut self.vm.logs, el, msg);
                    for kid in &kids {
                        self.tree.parent.insert(kid.refno.refno(), refno);
                    }
                    self.tree.children.insert(refno, kids);
                    dirty = true;
                }
                data::Evt::Children(refno, Err(e)) => {
                    // 展开失败就收回箭头，别让行卡在「加载中」。
                    self.tree.loading.remove(&refno);
                    self.tree.expanded.remove(&refno);
                    // 这一层正挡在定位目标前面的话，那次定位到不了了。
                    self.end_locate_on_failure(refno);
                    let el = self.tree.element(refno);
                    self.logs.error_of(
                        &mut self.vm.logs,
                        el,
                        "子层加载失败",
                        &e,
                        Some(Retry::Children(refno)),
                    );
                    dirty = true;
                }
                // 晚到的旧选中结果直接丢弃，属性面板只认当前选中。
                data::Evt::Props(refno, result) if self.vm.selection.primary() == Some(refno) => {
                    self.vm.props = match result {
                        Ok(kvs) => PropsVm::Ready(self.build_props(refno, kvs)),
                        Err(e) => {
                            let el = self.tree.element(refno);
                            self.logs.error_of(
                                &mut self.vm.logs,
                                el,
                                "属性查询失败",
                                &e,
                                Some(Retry::Props(refno)),
                            );
                            PropsVm::Failed(format!("属性查询失败：{}", logs::error_chain(&e)))
                        }
                    };
                }
                data::Evt::Props(..) => {}
                // 晚到的旧选中结果直接丢弃，房间归属只认当前选中（与属性同一条规矩）。
                data::Evt::ElementRooms(refno, result)
                    if self.vm.selection.primary() == Some(refno) =>
                {
                    self.vm.rooms = match result {
                        Ok(relations) => RoomVm::Ready(build_rooms(refno, relations)),
                        Err(e) => {
                            let el = self.tree.element(refno);
                            self.logs.error_of(
                                &mut self.vm.logs,
                                el,
                                "房间归属查询失败",
                                &e,
                                None,
                            );
                            RoomVm::Failed(format!(
                                "房间归属查询失败：{}",
                                logs::error_chain(&e)
                            ))
                        }
                    };
                }
                data::Evt::ElementRooms(..) => {}
                data::Evt::PanelRoom(refno, result) => match result {
                    Ok(room) => {
                        match resolve_panel_room_reply(self.vm.selection.primary(), refno, room) {
                            PanelRoomResolution::Ignore => {}
                            PanelRoomResolution::Clear => self.clear_room_xray(),
                            PanelRoomResolution::Enable { panel, room } => {
                                self.begin_room_xray(panel, room)
                            }
                        }
                    }
                    Err(e) if self.vm.selection.primary() == Some(refno) => {
                        self.clear_room_xray();
                        let el = self.tree.element(refno);
                        self.logs
                            .error_of(&mut self.vm.logs, el, "PANEL 房间查询失败", &e, None);
                    }
                    Err(_) => {}
                },
                data::Evt::RoomPanels(room, result)
                    if self
                        .xray_room_pending
                        .is_some_and(|(_, pending_room)| pending_room == room) =>
                {
                    let (panel, _) = self.xray_room_pending.expect("guarded above");
                    match result {
                        Ok(panels) => {
                            self.room_panel_cache.insert(room, panels.clone());
                            self.finish_room_xray(panel, room, panels);
                        }
                        Err(e) => {
                            self.xray_room_pending = None;
                            self.logs.error(
                                &mut self.vm.logs,
                                "房间 PANEL 查询失败",
                                &e,
                                None,
                            );
                        }
                    }
                }
                data::Evt::RoomPanels(..) => {}
                // 房间详情回来了，两个消费者各取所需：视口那半（隔离 + 取景，
                // 一次点击的下半程）与「房间」页签那半（详情数据）。谁都不认的
                // 是晚到的旧结果，直接丢。
                data::Evt::RoomDetail(room, result)
                    if self.focus_room_pending == Some(room)
                        || self.room_pane_focus == Some(room)
                        || self
                            .xray_room_pending
                            .is_some_and(|(_, pending_room)| pending_room == room) =>
                {
                    let for_viewport = self.focus_room_pending == Some(room);
                    let for_pane = self.room_pane_focus == Some(room);
                    let for_xray = self
                        .xray_room_pending
                        .filter(|(_, pending_room)| *pending_room == room);
                    if for_viewport {
                        self.focus_room_pending = None;
                    }
                    match result {
                        Ok(Some(detail)) => {
                            self.room_panel_cache.insert(room, detail.panels.clone());
                            if for_pane {
                                self.vm.room_detail =
                                    RoomDetailVm::Ready(build_room_detail(&detail));
                            }
                            if let Some((panel, _)) = for_xray {
                                self.finish_room_xray(panel, room, detail.panels.clone());
                            }
                            // 独立壳（无实时渲染器）里不排视口动作：那支队列没有
                            // 消费者，塞进去只会越攒越多；数据那半已经交给页签。
                            let live = self.vm.view3d.is_some_and(|v| v.live);
                            if for_viewport && live {
                                // 面板在前成员在后；同一个 refno 两边都出现时只留一份。
                                let mut seen: HashSet<RefU64> = HashSet::new();
                                let targets: Vec<RefU64> = detail
                                    .panels
                                    .iter()
                                    .chain(&detail.member_refnos)
                                    .copied()
                                    .filter(|refno| seen.insert(*refno))
                                    .collect();
                                let targets =
                                    expand_room_model_targets(targets, &self.model_scopes);
                                if targets.is_empty() {
                                    self.logs.warn(
                                        &mut self.vm.logs,
                                        format!(
                                            "房间 {} 没有面板也没有成员，无处取景",
                                            detail.room_num
                                        ),
                                    );
                                } else {
                                    self.view3d_commands.push(Cmd::Model(
                                        plant_ui::ModelAction::Isolate {
                                            refnos: targets.clone(),
                                        },
                                    ));
                                    self.view3d_commands.push(Cmd::Model(
                                        plant_ui::ModelAction::FocusGroup { refnos: targets },
                                    ));
                                    self.logs.info(
                                        &mut self.vm.logs,
                                        format!(
                                            "房间视图：{} · 面板 {} · 成员 {}（已加载的关联直段一并显示）",
                                            detail.room_num,
                                            detail.panels.len(),
                                            detail.member_count,
                                        ),
                                    );
                                    self.vm.room_view = Some(RoomViewVm {
                                        room,
                                        room_num: detail.room_num.clone(),
                                        member_count: detail.member_count,
                                    });
                                }
                            }
                        }
                        Ok(None) => {
                            if for_xray.is_some() {
                                self.xray_room_pending = None;
                            }
                            if for_pane {
                                self.vm.room_detail = RoomDetailVm::Failed(format!(
                                    "房间 {room} 在库里查不到，可能已被删除"
                                ));
                            }
                            self.logs.warn(
                                &mut self.vm.logs,
                                format!("房间 {room} 在库里查不到，可能已被删除"),
                            );
                        }
                        Err(e) => {
                            if for_xray.is_some() {
                                self.xray_room_pending = None;
                            }
                            if for_pane {
                                self.vm.room_detail = RoomDetailVm::Failed(format!(
                                    "房间详情查询失败：{}",
                                    logs::error_chain(&e)
                                ));
                            }
                            self.logs.error(
                                &mut self.vm.logs,
                                "房间详情查询失败",
                                &e,
                                None,
                            );
                        }
                    }
                }
                data::Evt::RoomDetail(..) => {}
                data::Evt::RoomsOverview(result) => {
                    self.rooms_overview = match result {
                        Ok(rows) => RoomBrowserVm::Ready(
                            rows.into_iter().map(build_overview_row).collect(),
                        ),
                        Err(e) => {
                            self.logs
                                .error(&mut self.vm.logs, "房间总览查询失败", &e, None);
                            RoomBrowserVm::Failed(format!(
                                "房间总览查询失败：{}",
                                logs::error_chain(&e)
                            ))
                        }
                    };
                }
                // 场景在点击取回工作那一刻已经清空，查回来的批次一律上屏：它比
                // 空场景新，也比旧几何新。上屏时若又有新保存落库，后续自动刷新
                // 照旧只换树并在日志里提示再点取回工作——不再整包丢弃结果，否则
                // 空场景要一直挂到整条队列跑完（决定 4）。
                data::Evt::Models(debt_reload, result) => {
                    if debt_reload {
                        self.model_reload_in_flight = false;
                    }
                    match result {
                        Ok(models) => {
                            self.set_get_work_busy(false);
                            self.run_deferred_get_work();
                            let mesh_count =
                                models.iter().map(|model| model.insts.len()).sum::<usize>();
                            self.refresh_generation_pending |= debt_reload;
                            self.pending_incremental_models.clear();
                            self.model_show_waiting.clear();
                            self.latest_mesh_progress = None;
                            self.model_scope_failures = 0;
                            self.model_scopes.clear();
                            self.tree.visibility_unavailable.clear();
                            self.loaded_models.clear();
                            for model in &models {
                                let refno = model.refno.refno();
                                self.loaded_models.insert(refno);
                                cache_model_scope(&mut self.model_scopes, refno, refno);
                            }
                            // 清场重装：旧的实际状态全作废，新的等 View3d 装完
                            // 网格再回执上来。这中间 eye 停在未加载而不是抢先
                            // 说「已显示」。
                            self.tree.visibility.clear();
                            self.tree.pending_direction.clear();
                            // 清场快照到此消费：取回前隐藏着、这次又查回来的那批
                            // 稍后按原方向回放。
                            let replay_hidden = take_hidden_for_replay(
                                &mut self.model_reload_restore,
                                &self.loaded_models,
                            );
                            let replay = if replay_hidden.is_empty() {
                                String::new()
                            } else {
                                format!("；回放隐藏 {} 个", replay_hidden.len())
                            };
                            self.logs.info(
                                &mut self.vm.logs,
                                format!(
                                    "三维模型已就绪：{} 个元素，{} 个网格实例{replay}",
                                    models.len(),
                                    mesh_count
                                ),
                            );
                            // 帧循环里 load 先行、隐藏方向后至但先于批次在 Bevy
                            // 里落地：被回放的模型直接以 Hidden 出生，不会先亮
                            // 一下再暗。
                            self.pending_models = Some(models);
                            if !replay_hidden.is_empty() {
                                self.set_model_visible(replay_hidden, false);
                            }
                            dirty = true;
                        }
                        Err(error) => {
                            restore_model_reload(&mut self.model_reload_owed, debt_reload);
                            self.set_get_work_busy(false);
                            self.run_deferred_get_work();
                            self.logs.error(
                                &mut self.vm.logs,
                                "三维模型查询失败：场景已清空，队列空闲时自动重试，也可再点「取回工作」",
                                &error,
                                None,
                            );
                        }
                    }
                }
                data::Evt::ModelScopeProgress {
                    epoch,
                    target,
                    done,
                    total,
                } if epoch == self.model_scope_epoch => {
                    if self.model_scope_pending.contains(&target) {
                        self.model_progress_until = None;
                        self.vm.model_load = Some(if total == 0 {
                            ModelLoadVm::Resolving("解析模型范围…".into())
                        } else {
                            ModelLoadVm::Loading {
                                label: "查询已有模型".into(),
                                done,
                                total,
                            }
                        });
                    }
                }
                data::Evt::ModelScope(epoch, target, result) if epoch == self.model_scope_epoch => {
                    self.model_scope_pending.remove(&target);
                    match result {
                        Ok(models) if models.is_empty() => {
                            mark_model_scope_unavailable(
                                &mut self.tree.pending_direction,
                                &mut self.tree.visibility_unavailable,
                                &mut self.model_scope_failures,
                                target,
                            );
                            let element = self.tree.element(target);
                            self.logs
                                .warn_of(&mut self.vm.logs, element, "未找到已生成模型");
                            self.finish_model_progress("模型加载完成");
                            // 这间房没有已生成模型，取景的账就地销掉——留着它，
                            // 下一个范围回包会替它把相机带走。
                            self.pending_room_frame.take_if(|room| *room == target);
                            dirty = true;
                        }
                        Ok(models) => {
                            self.tree.visibility_unavailable.remove(&target);
                            let mesh_count =
                                models.iter().map(|model| model.insts.len()).sum::<usize>();
                            let mut refs = Vec::new();
                            let mut seen = HashSet::new();
                            for model in &models {
                                let refno = model.refno.refno();
                                if seen.insert(refno) {
                                    refs.push(refno);
                                }
                                cache_model_scope(&mut self.model_scopes, refno, refno);
                            }
                            self.model_scopes.insert(target, refs.clone());
                            let element = self.tree.element(target);
                            self.logs.info_of(
                                &mut self.vm.logs,
                                element,
                                format!("查询到 {} 个元素、{} 个网格实例", refs.len(), mesh_count),
                            );

                            // 用**最新**的方向操作模型：查询期间用户可能已经反向
                            // 点过好几下，或者中途来过一次 HideAll。
                            let visible = self
                                .tree
                                .pending_direction
                                .get(&target)
                                .copied()
                                .unwrap_or(true);
                            let new_refnos = claim_unloaded_model_refnos(
                                &mut self.loaded_models,
                                models.iter().map(|model| model.refno.refno()),
                            );
                            let mut new_models = Vec::new();
                            let mut new_meshes = 0;
                            for model in models {
                                let refno = model.refno.refno();
                                if new_refnos.contains(&refno) {
                                    new_meshes += model.insts.len();
                                    new_models.push(model);
                                }
                            }
                            if !new_models.is_empty() {
                                self.pending_incremental_models.push(new_models);
                                if new_meshes > 0 {
                                    self.model_show_waiting.insert(target);
                                    self.model_progress_until = None;
                                    self.vm.model_load = Some(ModelLoadVm::Loading {
                                        label: "加载模型网格".into(),
                                        done: 0,
                                        total: new_meshes,
                                    });
                                } else {
                                    self.logs.info_of(
                                        &mut self.vm.logs,
                                        self.tree.element(target),
                                        if visible {
                                            "显示完成"
                                        } else {
                                            "隐藏完成"
                                        },
                                    );
                                    self.finish_model_progress(if visible {
                                        "模型显示完成"
                                    } else {
                                        "模型隐藏完成"
                                    });
                                }
                            } else {
                                if visible
                                    && (self.pending_models.is_some()
                                        || !self.pending_incremental_models.is_empty()
                                        || self.latest_mesh_progress.is_some())
                                {
                                    let (done, total) = self
                                        .latest_mesh_progress
                                        .as_ref()
                                        .map_or((0, mesh_count), |progress| {
                                            (progress.done, progress.total)
                                        });
                                    self.model_show_waiting.insert(target);
                                    self.model_progress_until = None;
                                    self.vm.model_load = Some(ModelLoadVm::Loading {
                                        label: "加载模型网格".into(),
                                        done,
                                        total,
                                    });
                                } else {
                                    self.logs.info_of(
                                        &mut self.vm.logs,
                                        self.tree.element(target),
                                        if visible {
                                            "显示完成"
                                        } else {
                                            "隐藏完成"
                                        },
                                    );
                                    self.finish_model_progress(if visible {
                                        "模型显示完成"
                                    } else {
                                        "模型隐藏完成"
                                    });
                                }
                            }
                            self.view3d_commands.push(Cmd::Model(
                                plant_ui::ModelAction::SetVisible {
                                    refnos: refs.clone(),
                                    visible,
                                },
                            ));
                            // 「显示房间模型」的下半程。包围盒来自这一批回包里的
                            // `world_aabb`，而视口那边装模型的系统排在执行命令之前，
                            // 所以同一帧「装进场景 + 取景」是成立的，不必再等网格文件。
                            if self.pending_room_frame == Some(target) {
                                self.pending_room_frame = None;
                                self.view3d_commands.push(Cmd::Model(
                                    plant_ui::ModelAction::FocusGroup { refnos: refs },
                                ));
                            }
                            dirty = true;
                        }
                        Err(error) => {
                            mark_model_scope_unavailable(
                                &mut self.tree.pending_direction,
                                &mut self.tree.visibility_unavailable,
                                &mut self.model_scope_failures,
                                target,
                            );
                            let element = self.tree.element(target);
                            self.logs.error_of(
                                &mut self.vm.logs,
                                element,
                                "已有模型查询失败",
                                &error,
                                None,
                            );
                            self.finish_model_progress("模型加载完成");
                            self.pending_room_frame.take_if(|room| *room == target);
                            dirty = true;
                        }
                    }
                }
                data::Evt::ModelScopeProgress { .. } | data::Evt::ModelScope(..) => {}
                data::Evt::ResolvedName(name, result) => match result {
                    // `/名称` 与 `=参考号` 之后走的是同一条定位路径，只有回执文案不同。
                    Ok(Some(refno)) => {
                        let located = self.locate(refno, true);
                        let suffix = match located {
                            Locate::Ready => "",
                            Locate::Resolving => "（正在展开它所在的路径…）",
                        };
                        self.command_output(format!("/{name} = {refno}{suffix}"));
                        dirty |= matches!(located, Locate::Ready);
                    }
                    Ok(None) => self.command_error(format!("未找到元素：/{name}")),
                    Err(error) => {
                        self.command_error(format!("名称查询失败：{}", logs::error_chain(&error)))
                    }
                },
                data::Evt::Ancestors(refno, Ok(chain)) => {
                    // 同一条链可能是两件事要的：一次定位，或者一次「新增子树该刷哪一层」
                    // 的锚点解析。两边各取所需，互不干扰。
                    dirty |= self.refresh_from_chain(refno, &chain);
                    dirty |= self.apply_ancestors(refno, chain);
                }
                data::Evt::Ancestors(refno, Err(error)) => {
                    self.refresh_anchors.remove(&refno);
                    // 链查不动就没法展开，待滚动到此为止；选中与属性仍然留着。
                    let from_command = self
                        .pending_locate
                        .as_ref()
                        .is_some_and(|p| p.target == refno && p.from_command);
                    self.end_locate_on_failure(refno);
                    let el = self.tree.element(refno);
                    self.logs
                        .error_of(&mut self.vm.logs, el, "祖先路径查询失败", &error, None);
                    if from_command {
                        self.command_error(format!(
                            "祖先路径查询失败：{}",
                            logs::error_chain(&error)
                        ));
                    }
                }
                // 预览是长请求，用户可能中途放弃等待（Vm 回到 Idle）。晚到的结果
                // 不许把界面拽回来——那一下会覆盖掉用户刚做出的选择。
                data::Evt::ModelUpdatePreview(_)
                    if !matches!(self.model_update, ModelUpdateVm::Loading) => {}
                data::Evt::ModelUpdatePreview(result) => match result {
                    Ok(preview) => {
                        self.logs.info(
                            &mut self.vm.logs,
                            format!(
                                "模型更新预览完成：{} 个设计库，执行范围 dbnum + sesno",
                                preview.dbnums.len()
                            ),
                        );
                        self.model_update = ModelUpdateVm::Ready(preview);
                    }
                    Err(error) => {
                        let failure = model_update_api::failure_of(&error);
                        self.logs
                            .error(&mut self.vm.logs, "模型更新预览失败", &error, None);
                        self.model_update = ModelUpdateVm::Failed(failure);
                    }
                },
                // 「扫描 + 入队」的回执。执行请求一律入队，回的是一组批次而不是
                // 一个 task_id，所以这里不再开运行实例——进度在任务队列视图上。
                data::Evt::ModelUpdateExecute {
                    from_wizard,
                    result,
                } => {
                    // 只有向导发起的那次才动向导：队列面板上的「立刻扫一遍」打的是
                    // 同一个接口，但它不该把人手上那份预览清掉。
                    let wizard_preview = from_wizard.then(|| match &self.model_update {
                        ModelUpdateVm::Starting(preview) => Some(preview.clone()),
                        _ => None,
                    });
                    let wizard_preview = wizard_preview.flatten();
                    match result {
                        Ok(receipt) => {
                            if let Some(preview) = wizard_preview.as_ref() {
                                self.queue.remember_manual_preview(preview, &receipt);
                            }
                            self.logs.info(&mut self.vm.logs, receipt.summary());
                            for warning in &receipt.warnings {
                                self.logs
                                    .warn(&mut self.vm.logs, format!("入队告警：{warning}"));
                            }
                            if from_wizard {
                                self.model_update = ModelUpdateVm::Idle;
                                self.model_update_state.open = false;
                                self.state.focus(Pane::TaskQueue);
                            }
                            self.poll_queue_now();
                        }
                        Err(error) => {
                            let failure = model_update_api::failure_of(&error);
                            self.logs.error(
                                &mut self.vm.logs,
                                "提交模型增量更新失败",
                                &error,
                                None,
                            );
                            if from_wizard {
                                self.model_update = ModelUpdateVm::Failed(failure);
                            }
                        }
                    }
                }
                // 查不动就把那行提示收起来，不为它报错——它本来就只是提示。
                data::Evt::PendingSessions(result) => {
                    self.vm.pending_sessions = result.ok();
                }
                data::Evt::GetWork(result) => {
                    let _ = self.bridge.req.send(data::Req::PendingSessions);
                    match result {
                        Ok(fresh) => {
                            let refreshed = std::mem::take(&mut self.get_work_task_ids);
                            if fresh.failed.is_empty() {
                                for task_id in refreshed {
                                    self.queue.mark_refreshed(&task_id);
                                }
                            }
                            let before = self.tree.element_count();
                            self.tree.roots = fresh.sites;
                            if fresh.reload_models {
                                // 重查范围 = 清场快照里的那批模型（显示 + 已隐藏），
                                // 不再是全部 SITE 根：取回工作刷的是「已加载的三维
                                // 模型」（CONTEXT.md），叶子 refno 当根走的是同一条
                                // anc 索引查询（决定 6）。
                                let roots: Vec<RefU64> = self
                                    .model_reload_restore
                                    .as_deref()
                                    .unwrap_or_default()
                                    .iter()
                                    .map(|(refno, _)| *refno)
                                    .collect();
                                if roots.is_empty() {
                                    // 清场前一个模型都没有：无可重查，忙碌态与在途
                                    // 标记就地收尾，别让人等一个不存在的回包。
                                    self.model_reload_restore = None;
                                    self.model_reload_in_flight = false;
                                    self.set_get_work_busy(false);
                                    self.run_deferred_get_work();
                                } else if self
                                    .bridge
                                    .req
                                    .send(data::Req::Models(roots, true))
                                    .is_err()
                                {
                                    restore_model_reload(
                                        &mut self.model_reload_owed,
                                        std::mem::take(&mut self.model_reload_in_flight),
                                    );
                                    self.set_get_work_busy(false);
                                    self.run_deferred_get_work();
                                }
                            }
                            for (refno, kids) in fresh.branches {
                                for kid in &kids {
                                    self.tree.parent.insert(kid.refno.refno(), refno);
                                }
                                self.tree.children.insert(refno, kids);
                            }
                            // 摘掉失败的分支之前先把话说完：`element` 要在树还
                            // 认得这个 refno 的时候取名字。
                            for (refno, reason) in &fresh.failed {
                                let el = self.tree.element(*refno);
                                self.logs.warn_of(
                                    &mut self.vm.logs,
                                    el,
                                    format!("子层重查失败，这一层保持原样：{reason}"),
                                );
                            }
                            self.tree.prune_unreachable();
                            // 清扫可以把待滚动路径上的某一节摘掉（元素被删或挪了
                            // OWNER）。那条路径已经通不到目标，待滚动跟着结束。
                            self.drop_locate_off_the_tree();
                            let after = self.tree.element_count();
                            self.logs.info(
                                &mut self.vm.logs,
                                format!("取回工作完成：已加载元素 {before} → {after}"),
                            );
                            // 数据缓存刚换代，属性、归属与房间面板集都不能继续沿用。
                            self.clear_room_xray();
                            self.room_panel_cache.clear();
                            if let Some(refno) = self.vm.selection.primary() {
                                self.refetch_props(refno);
                                self.refetch_rooms(refno);
                            }
                            if !fresh.reload_models {
                                self.set_get_work_busy(false);
                                self.run_deferred_get_work();
                            }
                            dirty = true;
                        }
                        Err(error) => {
                            self.get_work_task_ids.clear();
                            restore_model_reload(
                                &mut self.model_reload_owed,
                                std::mem::take(&mut self.model_reload_in_flight),
                            );
                            self.set_get_work_busy(false);
                            self.run_deferred_get_work();
                            self.logs
                                .error(&mut self.vm.logs, "取回工作失败", &error, None);
                        }
                    }
                }
                data::Evt::QueuePoll(result) => {
                    self.queue_poll_pending = false;
                    match result {
                        Ok(poll) => {
                            let (data_applied, mut fresh, task_ids) = self.newly_finished(&poll);
                            fresh.extend(settled_pending_roots(
                                self.queue.loaded,
                                poll.pending_known,
                                &self.queue.pending,
                                &poll.pending,
                            ));
                            let models_settled = background_models_settled(
                                poll.pending_known,
                                poll.pending.is_empty(),
                                poll.queue.rows.is_empty(),
                                poll.tasks_error.is_none(),
                                model_drains_idle(&poll.tasks, &self.vm.project),
                            );
                            let reload_due = model_reload_due(
                                &mut self.model_reload_owed,
                                data_applied || !fresh.is_empty(),
                                models_settled,
                            );
                            if !models_settled {
                                fresh.clear();
                            }
                            self.queue.project = self.vm.project.clone();
                            self.queue.mdb = self.mdb.clone();
                            self.queue.namespace = self.namespace.clone();
                            self.queue.adopt(poll);
                            if reload_due {
                                self.get_work_with_models_for_tasks(true, task_ids);
                            } else if data_applied {
                                self.get_work_with_models_for_tasks(false, task_ids);
                            } else {
                                self.refresh_for_units(fresh, task_ids);
                            }
                        }
                        // 上一份快照留在界面上，横幅把「这份是旧的」说出来。
                        Err(error) => {
                            self.queue.error = Some(logs::error_chain(&error));
                        }
                    }
                    self.vm.queue = task_queue::status(&self.queue);
                }
                data::Evt::QueueSetPaused(paused, result) => match result {
                    Ok(()) => {
                        let text = if paused {
                            "队列已暂停：不再出队；正在跑的那一批会跑完为止"
                        } else {
                            "队列已恢复出队"
                        };
                        self.logs.info(&mut self.vm.logs, text);
                        self.poll_queue_now();
                    }
                    Err(error) => {
                        let what = if paused {
                            "暂停队列失败"
                        } else {
                            "恢复队列失败"
                        };
                        self.logs.error(&mut self.vm.logs, what, &error, None);
                    }
                },
                data::Evt::RetryPendingUnit(root_refno, result) => match result {
                    Ok(()) => self.poll_queue_now(),
                    Err(error) => {
                        // 失败了就把在途标记撤掉，否则那枚按钮会一直灰着——
                        // 下一拍快照清 `in_flight` 的前提是「提交成功了」。
                        self.state.queue_retry_failed(&root_refno);
                        self.logs.error(
                            &mut self.vm.logs,
                            format!("复活 {root_refno} 失败"),
                            &error,
                            None,
                        );
                    }
                },
                data::Evt::DataPublish(request, result) => match result {
                    Ok(response) => self.logs.info(
                        &mut self.vm.logs,
                        format!(
                            "{} 已提交（{}）：{}",
                            request.category.label(),
                            request.category.endpoint(),
                            response
                        ),
                    ),
                    Err(error) => self.logs.error(
                        &mut self.vm.logs,
                        format!("{}提交失败", request.category.label()),
                        &error,
                        None,
                    ),
                },
                data::Evt::CommandQuery {
                    epoch,
                    label,
                    result,
                } if command_reply_is_current(epoch, self.command_epoch) => match result {
                    Ok(reply) => self.command_output(format!(
                        "[{label}]\n{}",
                        command::format_query_result(&reply.tool, &reply.result)
                    )),
                    Err(error) => self.command_error(format!(
                        "[{label}] 查询失败：{}",
                        logs::error_chain(&error)
                    )),
                },
                data::Evt::CommandQuery { .. } => {}
                data::Evt::QueueProgress(task_id, event) => self.queue.apply(&task_id, event),
                data::Evt::QueueTaskChanged => {
                    self.poll_queue_now();
                }
                data::Evt::QueueFeedLive => self.queue.feed = ModelUpdateFeed::Live,
                data::Evt::QueueFeedDown(reason) => {
                    self.queue.feed = ModelUpdateFeed::Down(reason);
                    // 没收到收尾事件的明细行说不清做没做完，别再显示「生成中」。
                    self.queue.mark_unknown();
                }
                // 没订过就没有半截的明细要收尾，`mark_unknown` 无事可做。
                data::Evt::QueueFeedUnsubscribed(reason) => {
                    self.queue.feed = ModelUpdateFeed::NotSubscribed(reason);
                }
            }
        }
        if dirty {
            self.rebuild_tree();
        }
    }

    fn succeed_model_progress(&mut self, label: impl Into<String>) {
        self.vm.model_load = Some(ModelLoadVm::Success(label.into()));
        self.model_progress_until = Some(Instant::now() + Duration::from_millis(1_500));
    }

    fn fail_model_progress(&mut self, label: impl Into<String>) {
        self.vm.model_load = Some(ModelLoadVm::Failed(label.into()));
        self.model_progress_until = Some(Instant::now() + Duration::from_secs(4));
    }

    fn finish_model_progress(&mut self, success: impl Into<String>) {
        if !self.model_scope_pending.is_empty() {
            self.model_progress_until = None;
            self.vm.model_load = Some(ModelLoadVm::Resolving("解析模型范围…".into()));
            return;
        }
        let Some(succeeded) =
            model_progress_terminal(self.model_show_waiting.len(), self.model_scope_failures)
        else {
            return;
        };
        let failures = std::mem::take(&mut self.model_scope_failures);
        if succeeded {
            self.succeed_model_progress(success);
        } else {
            self.fail_model_progress(format!("模型加载失败（{failures} 项）"));
        }
    }

    fn tick_model_progress(&mut self) {
        if self
            .model_progress_until
            .is_some_and(|until| Instant::now() >= until)
        {
            self.model_progress_until = None;
            self.vm.model_load = None;
        }
    }

    fn sync_mesh_progress(&mut self, progress: plant_ui_view3d::MeshLoadProgress) {
        self.latest_mesh_progress = (!progress.finished()).then(|| progress.clone());
        if progress.finished() {
            let before = self.vm.refresh_generation;
            if settle_refresh_generation(
                &mut self.refresh_generation_pending,
                &mut self.vm.refresh_generation,
                progress.errors.is_empty(),
            ) {
                self.log_refresh_generation();
            }
            debug_assert!(self.vm.refresh_generation >= before);
        }
        if self.model_show_waiting.is_empty() {
            if progress.finished() && !progress.errors.is_empty() {
                let error = anyhow::anyhow!(progress.errors.join("\n"));
                self.logs.error(
                    &mut self.vm.logs,
                    format!("增量刷新网格失败：{} 个", progress.errors.len()),
                    &error,
                    None,
                );
            }
            return;
        }
        self.model_progress_until = None;
        self.vm.model_load = Some(ModelLoadVm::Loading {
            label: "加载模型网格".into(),
            done: progress.done,
            total: progress.total,
        });
        if !progress.finished() {
            return;
        }

        let targets = self.model_show_waiting.iter().copied().collect::<Vec<_>>();
        self.model_show_waiting.clear();
        if progress.errors.is_empty() {
            let shown = targets
                .iter()
                .filter(|target| {
                    self.tree
                        .pending_direction
                        .get(target)
                        .copied()
                        .unwrap_or(true)
                })
                .count();
            let hidden = targets.len() - shown;
            let (message, label) = match (shown, hidden) {
                (shown, 0) => (format!("模型显示完成：{shown} 个目标"), "模型显示完成"),
                (0, hidden) => (format!("模型隐藏完成：{hidden} 个目标"), "模型隐藏完成"),
                (shown, hidden) => (
                    format!("模型加载完成：显示 {shown} 个、隐藏 {hidden} 个目标"),
                    "模型加载完成",
                ),
            };
            self.logs.info(&mut self.vm.logs, message);
            self.finish_model_progress(label);
        } else {
            let error = anyhow::anyhow!(progress.errors.join("\n"));
            self.logs.error(
                &mut self.vm.logs,
                format!("mesh 文件加载失败：{} 个", progress.errors.len()),
                &error,
                None,
            );
            self.model_scope_failures += progress.errors.len();
            self.finish_model_progress("模型加载完成");
        }
    }

    fn log_refresh_generation(&mut self) {
        self.logs.info(
            &mut self.vm.logs,
            format!(
                "UI 刷新屏障完成：generation {}（树、属性、mesh/AABB 已收敛）",
                self.vm.refresh_generation
            ),
        );
    }

    /// 收下 View3d 的实际渲染回执，整批更新 eye。
    ///
    /// 回执按模型参考号来，几何行与容器行都从这一份聚合（`row_visibility`），
    /// 所以这里只管把状态记下、再让落地的待执行方向退场。
    fn sync_render_states(&mut self, states: Vec<plant_ui_view3d::ModelRenderState>) {
        for state in states {
            self.tree.visibility.insert(
                state.refno,
                ModelVisibility {
                    visible: state.visible,
                    mesh_loaded: state.mesh_loaded,
                    mesh_failed: state.mesh_failed,
                },
            );
        }
        self.rebuild_tree();
    }

    fn handle_cmds(&mut self, ctx: &egui::Context, cmds: Vec<Cmd>) {
        let mut dirty = false;
        for cmd in cmds {
            match cmd {
                Cmd::SelectElement(refno) => self.select(refno),
                Cmd::SetSelection(selection) => self.set_selection(selection),
                // 日志行上点元素名过来的定位。祖先链要现查时这一帧还没什么可展平的，
                // 但选中与属性已经换过去了，界面不会看着像没反应。
                Cmd::LocateElement(refno) => {
                    dirty |= matches!(self.locate(refno, false), Locate::Ready);
                }
                Cmd::ToggleExpand(refno) => {
                    if self.tree.expanded.remove(&refno) {
                        self.end_locate_on_collapse(refno);
                    } else {
                        self.tree.expanded.insert(refno);
                        if !self.tree.children.contains_key(&refno)
                            && self.tree.loading.insert(refno)
                        {
                            let _ = self.bridge.req.send(data::Req::Children(refno));
                        }
                    }
                    dirty = true;
                }
                // 独立应用里派发层就是这里：先只落到内存 Vm，写回 SurrealDB 要连
                // 会话号、撤销、依赖重算一起做，归 M3-3 命令派发接现有 Event。
                Cmd::EditAttr { refno, attr, value } => {
                    self.vm.props.edit(refno, &attr, value.clone());
                    let el = self.tree.element(refno);
                    let msg = format!("{attr} = {value}（仅改内存，未写回数据库）");
                    self.logs.warn_of(&mut self.vm.logs, el, msg);
                }
                Cmd::GetWork => self.get_work(),
                // 面板错误态上的「重试」，与命令行那条走同一段处置逻辑。
                Cmd::Reconnect => self.reconnect(),
                Cmd::RetryProps(refno) if self.vm.selection.primary() == Some(refno) => {
                    self.refetch_props(refno)
                }
                Cmd::RetryProps(_) => {}
                Cmd::RetryRooms(refno) if self.vm.selection.primary() == Some(refno) => {
                    self.refetch_rooms(refno)
                }
                // 选中早就挪走：重查回来也过不了回包那道陈旧门，不发。
                Cmd::RetryRooms(_) => {}
                Cmd::RetryLog(id) => match self.logs.retry_of(id) {
                    Some(Retry::Connect) => self.reconnect(),
                    Some(Retry::Children(refno)) => {
                        // 重试即重新展开：箭头上次失败时被收回了，这里一并推回展开态。
                        self.tree.expanded.insert(refno);
                        if self.tree.loading.insert(refno) {
                            let _ = self.bridge.req.send(data::Req::Children(refno));
                        }
                        dirty = true;
                    }
                    Some(Retry::Props(refno)) if self.vm.selection.primary() == Some(refno) => {
                        self.refetch_props(refno)
                    }
                    // 选中早就挪走的话，属性重查回来也没地方摆，直接跳过。
                    _ => {}
                },
                Cmd::ClearLogs => self.logs.clear(&mut self.vm.logs),
                Cmd::SubmitCommand(command) => dirty |= self.submit_command(command),
                Cmd::OpenProjectPicker => self.project_picker_state.open = true,
                Cmd::LoadProject(_) => self.project_picker_state.open = false,
                Cmd::OpenSettings => self.settings_state.open(),
                Cmd::OpenDataPublish => self.data_publish_state.open = true,
                Cmd::SubmitDataPublish(request) => {
                    let _ = self.bridge.req.send(data::Req::DataPublish {
                        base: self.data_api_url.clone(),
                        request,
                    });
                }
                Cmd::OpenModelUpdate => {
                    self.model_update_state.open = true;
                    if !matches!(
                        self.model_update,
                        ModelUpdateVm::Loading | ModelUpdateVm::Starting(_)
                    ) {
                        self.refresh_model_update();
                    }
                }
                Cmd::RefreshModelUpdate => self.refresh_model_update(),
                // 只放弃客户端等待：服务端没有 cancel 接口，扫描照跑完。
                Cmd::CancelModelUpdatePreview => {
                    self.model_update = ModelUpdateVm::Idle;
                    self.logs.info(
                        &mut self.vm.logs,
                        "已放弃等待预览结果；服务端的扫描不受影响，重新预览即可",
                    );
                }
                Cmd::ExecuteModelUpdate { dbnums } => {
                    if !self.model_service_writable() {
                        continue;
                    }
                    let ModelUpdateVm::Ready(preview) = &self.model_update else {
                        continue;
                    };
                    self.model_update = ModelUpdateVm::Starting(preview.clone());
                    let _ = self.bridge.req.send(data::Req::ModelUpdateExecute {
                        base: self.model_api_url.clone(),
                        project: self.vm.project.clone(),
                        mdb: self.mdb.clone(),
                        namespace: self.namespace.clone(),
                        dbnums,
                        from_wizard: true,
                    });
                }
                // 与「确认执行」打同一个接口。它不插队，作用只是别等服务端下一个
                // 30 秒轮询——回执照样进日志，进度在队列面板上。
                Cmd::ScanNow => {
                    if !self.model_service_writable() {
                        continue;
                    }
                    // 队列面板的即时扫描没有勾选语境，走全范围（ADR-020 缺省）。
                    let _ = self.bridge.req.send(data::Req::ModelUpdateExecute {
                        base: self.model_api_url.clone(),
                        project: self.vm.project.clone(),
                        mdb: self.mdb.clone(),
                        namespace: self.namespace.clone(),
                        dbnums: None,
                        from_wizard: false,
                    });
                }
                Cmd::SetQueuePaused(paused) => {
                    if !self.model_service_writable() {
                        continue;
                    }
                    let _ = self.bridge.req.send(data::Req::QueueSetPaused {
                        base: self.model_api_url.clone(),
                        paused,
                    });
                }
                // 死信只有人按这一下才会再动一次。它不入队，所以按完不会立刻多出
                // 一行——变化要等下一拍轮询，界面上那句「已提交 · 等下一拍」说的
                // 就是这件事。
                Cmd::RetryPendingUnit { dbnum, root_refno } => {
                    if !self.model_service_writable() {
                        continue;
                    }
                    self.logs.info(
                        &mut self.vm.logs,
                        format!(
                            "已提交复活 db{dbnum} 的 {root_refno}：重试次数清零，等调度器下一轮取它"
                        ),
                    );
                    let _ = self.bridge.req.send(data::Req::RetryPendingUnit {
                        base: self.model_api_url.clone(),
                        project: self.vm.project.clone(),
                        mdb: self.mdb.clone(),
                        namespace: self.namespace.clone(),
                        root_refno,
                    });
                }
                Cmd::ReconnectQueueFeed => self.reopen_queue_feed(ctx),
                Cmd::FocusRoom(room) => self.focus_room(room),
                Cmd::ShowRoomModel(room) => {
                    self.show_room_model(room);
                    dirty = true;
                }
                Cmd::ShowRooms(refno) => {
                    // 归属数据跟着主选中预取；对准的不是当前主选中时先把选中切
                    // 过去，页签一亮出来就有内容而不是一句「查询中」。
                    if self.vm.selection.primary() != Some(refno) {
                        self.set_selection(Selection::single(refno));
                    }
                    self.state.focus(Pane::Room);
                }
                Cmd::FocusPane(pane) => self.state.focus(pane),
                Cmd::OpenRoomBrowser => {
                    self.room_browser_state.open = true;
                    // 全表在途时只开窗不重发：这查询几十秒级，叠一份是纯浪费。
                    if !self.rooms_overview.loading() {
                        self.rooms_overview.begin_query();
                        let _ = self.bridge.req.send(data::Req::RoomsOverview);
                    }
                }
                Cmd::Model(action) => {
                    match action {
                        plant_ui::ModelAction::SetVisible { refnos, visible } => {
                            self.set_model_visible(refnos, visible)
                        }
                        // 全局的两条只作用于**已加载**的模型：eye 照样等 View3d
                        // 回执。在途的那些点击把方向改成全局这一次的——它就是
                        // 最后一次指令，查询回来时得照它走，不然那几个目标会在
                        // 一片全隐藏里独自冒出来。
                        plant_ui::ModelAction::HideAll => {
                            self.tree
                                .pending_direction
                                .values_mut()
                                .for_each(|visible| *visible = false);
                            self.view3d_commands
                                .push(Cmd::Model(plant_ui::ModelAction::HideAll));
                        }
                        plant_ui::ModelAction::ShowAll => {
                            self.tree
                                .pending_direction
                                .values_mut()
                                .for_each(|visible| *visible = true);
                            self.view3d_commands
                                .push(Cmd::Model(plant_ui::ModelAction::ShowAll));
                        }
                        // 退出房间视图：徽章立即摘下，可见性恢复由视口按快照执行。
                        // 隔离/恢复不写树的 pending 方向账——真实可见性会沿
                        // render_dirty 那条既有链路流回眼睛（ADR-0016）。
                        plant_ui::ModelAction::ExitIsolate => {
                            self.vm.room_view = None;
                            self.focus_room_pending = None;
                            self.view3d_commands
                                .push(Cmd::Model(plant_ui::ModelAction::ExitIsolate));
                        }
                        action => self.view3d_commands.push(Cmd::Model(action)),
                    }
                    dirty = true;
                }
                command @ (Cmd::PickViewport(_)
                | Cmd::Camera(_)
                | Cmd::ResizeViewport(_)
                | Cmd::SetViewportBackground { .. }
                | Cmd::SnapView { .. }
                | Cmd::RetryFailedMeshes) => self.view3d_commands.push(command),
            }
        }
        if dirty {
            self.rebuild_tree();
        }
    }

    fn submit_command(&mut self, input: String) -> bool {
        let input = input.trim();
        let parsed = command::parse(input);
        if parsed == ParsedCommand::Empty {
            return false;
        }
        self.push_command_line(CommandLineKind::Input, input);

        match parsed {
            ParsedCommand::Empty => false,
            ParsedCommand::Help => {
                self.command_output(
                    "help          显示帮助\nclear         清空命令会话\nq help        显示模型查询帮助\nq <属性>      查询当前元素属性\n/<名称>       按名称定位元素\n=<参考号>     按参考号定位元素",
                );
                false
            }
            ParsedCommand::Clear => {
                self.vm.command.lines.clear();
                self.command_epoch = self.command_epoch.wrapping_add(1);
                false
            }
            ParsedCommand::Query(query) => {
                match query {
                    command::QueryInput::Help => self.command_output(command::QUERY_HELP),
                    command::QueryInput::Property(attr) => match self.query_attr(&attr) {
                        Ok(value) => self.command_output(value),
                        Err(error) => self.command_error(error),
                    },
                    command::QueryInput::Remote(query) => {
                        match query.bind(self.vm.selection.primary()) {
                            Ok(query) => {
                                let label = query.label.clone();
                                let request = data::Req::CommandQuery {
                                    epoch: self.command_epoch,
                                    label: query.label,
                                    base: self.model_api_url.clone(),
                                    project: self.vm.project.clone(),
                                    mdb: self.mdb.clone(),
                                    namespace: self.namespace.clone(),
                                    tool: query.tool.into(),
                                    arguments: query.arguments,
                                };
                                if self.bridge.req.send(request).is_err() {
                                    self.command_error(format!(
                                        "[{label}] 查询失败：数据线程已关闭"
                                    ));
                                } else {
                                    self.command_output(format!("[{label}] 查询中…"));
                                }
                            }
                            Err(error) => self.command_error(error),
                        }
                    }
                }
                false
            }
            ParsedCommand::LocateName(name) => {
                if self.bridge.req.send(data::Req::ResolveName(name)).is_err() {
                    self.command_error("名称查询失败：数据线程已关闭");
                }
                false
            }
            ParsedCommand::LocateRef(refno) => {
                // 目标还没物化时这条只报「已选中」：能不能滚到要等祖先链回来，
                // 那时候再补一句。先报成功再改口比晚报一拍难解释得多。
                match self.locate(refno, true) {
                    Locate::Ready => {
                        self.command_output(format!("已定位 {refno}"));
                        true
                    }
                    Locate::Resolving => {
                        self.command_output(format!("已选中 {refno}，正在展开它所在的路径…"));
                        false
                    }
                }
            }
            ParsedCommand::Error(error) => {
                self.command_error(error);
                false
            }
        }
    }

    fn query_attr(&self, attr: &str) -> Result<String, String> {
        let data = match &self.vm.props {
            PropsVm::Uninit => return Err("当前未选中元素".into()),
            PropsVm::Loading(_) => return Err("当前元素属性仍在加载".into()),
            PropsVm::Failed(error) => return Err(format!("当前元素属性不可用：{error}")),
            PropsVm::Ready(data) => data,
        };
        let attr = attr.trim();
        let value = if attr.eq_ignore_ascii_case("REF") || attr.eq_ignore_ascii_case("REFNO") {
            data.refno.to_string()
        } else {
            data.common
                .iter()
                .chain(&data.attrs)
                .chain(&data.udas)
                .find(|row| row.attr.eq_ignore_ascii_case(attr))
                .map(|row| row.value.clone())
                .ok_or_else(|| format!("属性不存在：{}", attr.to_ascii_uppercase()))?
        };
        Ok(format!("{} = {value}", attr.to_ascii_uppercase()))
    }

    /// 树定位：选中目标并让属性跟上，再想办法把它那一行露出来（ADR-0014）。
    ///
    /// 祖先链已经在缓存里的话这一帧就能展开到位；不在的话现查一条，滚动留给
    /// `Evt::Ancestors` 之后的那几层子层。两条路都不切换 MDB、不生成或显示三维、
    /// 不动相机。
    fn locate(&mut self, refno: RefU64, from_command: bool) -> Locate {
        self.select(refno);
        if let Some(chain) = self.tree.ancestors(refno) {
            // 链来自本端缓存：目标既然在 `tree.parent` 里，它那一行本来就在树上，
            // 没有「父分支比目标还老」这回事，不必重查。
            self.expand_path(&chain, false);
            self.pending_locate = Some(PendingLocate {
                target: refno,
                path: chain,
                from_command,
                announced: true,
            });
            self.vm.tree_reveal = Some(refno);
            return Locate::Ready;
        }
        // 上一次还没滚到的定位就此作废：连续定位只完成最后一次。
        self.vm.tree_reveal = None;
        self.pending_locate = Some(PendingLocate {
            target: refno,
            // 路径要等祖先链回来才知道。这期间折叠任何一行都动不到这次定位。
            path: Vec::new(),
            from_command,
            announced: false,
        });
        let _ = self.bridge.req.send(data::Req::Ancestors(refno));
        Locate::Resolving
    }

    /// 展开这条路径，并把还没有子层的那几节补查回来。
    ///
    /// 只补直接子层，不预载整棵树：为一次定位把模型树整个拉下来，代价是几万行
    /// 无人看的节点。
    ///
    /// `from_chain` = 这条路径是刚从库里查回来的祖先链。那时候**直接父那一节
    /// 即使已有缓存也要重查**，理由见 [`needs_children_query`]。
    fn expand_path(&mut self, path: &[RefU64], from_chain: bool) {
        for (i, &ancestor) in path.iter().enumerate() {
            self.tree.expanded.insert(ancestor);
            let cached = self.tree.children.contains_key(&ancestor);
            if needs_children_query(from_chain && i == 0, cached)
                && self.tree.loading.insert(ancestor)
            {
                let _ = self.bridge.req.send(data::Req::Children(ancestor));
            }
        }
    }

    /// 祖先链查回来了：把当前 MDB 内的那一段接进树里，剩下的交给子层查询。
    fn apply_ancestors(&mut self, refno: RefU64, chain: Vec<RefU64>) -> bool {
        // 连续定位时早先那次的链会晚到，它已经不是当前目标了。
        if self.pending_locate.as_ref().map(|p| p.target) != Some(refno) {
            return false;
        }
        let in_mdb = in_mdb_path(&chain, |ancestor| self.tree.is_root(ancestor));
        let Some(path) = in_mdb else {
            // 根层里找不到落脚点：目标在模型本体里，但不在当前 MDB 的树根范围内。
            let pending = self.pending_locate.take();
            let el = self.tree.element(refno);
            self.logs.warn_of(
                &mut self.vm.logs,
                el.clone(),
                "树外元素：已选中并显示属性，不切换 MDB",
            );
            if pending.is_some_and(|p| p.from_command) {
                self.command_output(format!("{} 是树外元素，只切换了属性", el.label));
            }
            return false;
        };
        // OWNER 关系顺手记下来。不记的话下一次定位到同一条分支上的元素还要再查一遍，
        // 而这条链现在已经和 `children` 里的内容一样可靠了。
        for pair in chain[..=path.len()].windows(2) {
            self.tree.parent.insert(pair[0], pair[1]);
        }
        self.expand_path(&path, true);
        if let Some(pending) = &mut self.pending_locate {
            pending.path = path;
        }
        self.vm.tree_reveal = Some(refno);
        true
    }

    /// 用户手动折叠了这一节。它在待滚动的路径上就等于放弃这次定位——他显然不要了，
    /// 而把行滚过去之后再让它藏在折叠里毫无意义。
    ///
    /// 折叠目标自己不算：那只是收起它的子层，它那一行还在，照样滚得到。
    fn end_locate_on_collapse(&mut self, collapsed: RefU64) {
        if self
            .pending_locate
            .as_ref()
            .is_some_and(|p| p.path.contains(&collapsed))
        {
            self.end_locate();
        }
    }

    /// 这一层查不动了。它是目标本身（祖先链失败，路径还没算出来）或者路径上的
    /// 一节（子层失败，路径断了）时，这次定位到不了目标，别把滚动请求一直挂着。
    fn end_locate_on_failure(&mut self, failed: RefU64) {
        if self
            .pending_locate
            .as_ref()
            .is_some_and(|p| p.target == failed || p.path.contains(&failed))
        {
            self.end_locate();
        }
    }

    fn end_locate(&mut self) {
        self.pending_locate = None;
        self.vm.tree_reveal = None;
    }

    /// 取回工作的清扫之后，待滚动的路径是不是还站在树上。
    fn drop_locate_off_the_tree(&mut self) {
        let stale = self.pending_locate.as_ref().is_some_and(|p| {
            p.path
                .iter()
                .any(|ancestor| !self.tree.expanded.contains(ancestor))
        });
        if stale {
            self.end_locate();
        }
    }

    /// 目标那一行已经在可见行里——也就是这一帧的滚动真的滚得到。
    fn tree_reveal_row_ready(&self) -> bool {
        let Some(refno) = self.vm.tree_reveal else {
            return false;
        };
        matches!(&self.vm.tree, TreeVm::Ready(rows) if rows.iter().any(|r| r.refno == refno))
    }

    /// 每帧末尾结算待滚动：行已经画出来了就把请求收掉，还没有就留着。
    ///
    /// 留着是这条路径的关键（ADR-0014）。祖先的子层是一层层异步回来的，滚动请求
    /// 只活一帧的话，它会在目标行还不存在时被 `reveal_offset` 空手接走，之后没人
    /// 再提这件事，树就停在原地。
    fn settle_tree_reveal(&mut self) {
        if !self.tree_reveal_row_ready() {
            // 路径全展开完了、没有子层还在路上，目标那一行却没出现：库里已经没有
            // 这个元素了（查完祖先链到子层回来之间被删掉，或者挪去了别的 OWNER）。
            // 不在这里收尾的话滚动请求会一直挂着，永远等一行不会来的行。
            if self.locate_path_settled() {
                let target = self.pending_locate.as_ref().map(|p| p.target);
                let from_command = self
                    .pending_locate
                    .as_ref()
                    .is_some_and(|p| p.from_command && !p.announced);
                self.end_locate();
                if let Some(target) = target {
                    let el = self.tree.element(target);
                    let label = el.label.clone();
                    self.logs.warn_of(
                        &mut self.vm.logs,
                        el,
                        "路径已展开完，树上却没有这一行：它已不在这条路径下",
                    );
                    if from_command {
                        self.command_error(format!("{label} 已不在模型树的这条路径上"));
                    }
                }
            }
            return;
        }
        self.vm.tree_reveal = None;
        let Some(pending) = self.pending_locate.take() else {
            return;
        };
        if pending.from_command && !pending.announced {
            let label = self.tree.label(pending.target);
            self.command_output(format!("已定位 {label}"));
        }
    }

    /// 待滚动的路径已经无事可等：每一节都展开着、子层都在手上。
    ///
    /// 祖先链还在查（`path` 为空但目标不是根）的那段不算——那时候路径还不知道，
    /// 空的 `path` 会让下面这几个判断一律成立。
    fn locate_path_settled(&self) -> bool {
        let Some(pending) = &self.pending_locate else {
            return false;
        };
        if pending.path.is_empty() && !self.tree.is_root(pending.target) {
            return false;
        }
        pending.path.iter().all(|ancestor| {
            self.tree.expanded.contains(ancestor)
                && !self.tree.loading.contains(ancestor)
                && self.tree.children.contains_key(ancestor)
        })
    }

    fn command_output(&mut self, text: impl Into<String>) {
        self.push_command_line(CommandLineKind::Output, text);
    }

    fn command_error(&mut self, text: impl Into<String>) {
        self.push_command_line(CommandLineKind::Error, text);
    }

    fn push_command_line(&mut self, kind: CommandLineKind, text: impl Into<String>) {
        self.vm.command.lines.push(CommandLineVm {
            kind,
            text: text.into(),
        });
        if self.vm.command.lines.len() > COMMAND_CAP {
            self.vm.command.lines.drain(..COMMAND_CAP / 4);
        }
    }

    /// 选中单个元素并让属性视图跟上。重复选同一个不再发查询。
    fn select(&mut self, refno: RefU64) {
        self.set_selection(Selection::single(refno));
    }

    /// 换一整个选择集。属性只跟主选中，主选中没变就不重新查——
    /// Ctrl 点掉一个非主选中的元素时选择集变了而主选中没变，那一下不该打库。
    fn set_selection(&mut self, selection: Selection) {
        if self.vm.selection == selection {
            return;
        }
        let before = self.vm.selection.primary();
        self.vm.selection = selection;
        if before != self.vm.selection.primary() {
            self.clear_room_xray();
        }
        match self.vm.selection.primary() {
            Some(refno) if before != Some(refno) => {
                self.refetch_props(refno);
                self.refetch_rooms(refno);
                // 换了元素，上一个元素聚焦的房间不再作数——详情回到「还没点
                // 过房间」，页签只画新元素的归属列表。
                self.room_pane_focus = None;
                self.vm.room_detail = RoomDetailVm::Uninit;
            }
            // 全都取消选中了，属性回到「还没轮到它」而不是留着上一个的残影。
            None => {
                self.vm.props = PropsVm::Uninit;
                self.vm.rooms = RoomVm::Uninit;
                self.room_pane_focus = None;
                self.vm.room_detail = RoomDetailVm::Uninit;
            }
            _ => {}
        }
    }

    fn refetch_props(&mut self, refno: RefU64) {
        self.vm.props.begin_query();
        let _ = self.bridge.req.send(data::Req::Props(refno));
    }

    /// 房间归属随选中预取（计划决定 5）：查询极轻，右键菜单打开的瞬间就有现成数据。
    fn refetch_rooms(&mut self, refno: RefU64) {
        self.vm.rooms.begin_query();
        let _ = self.bridge.req.send(data::Req::ElementRooms(refno));
        let _ = self.bridge.req.send(data::Req::PanelRoom(refno));
    }

    /// 主选中一换就先恢复旧房间。新的 PANEL 是否有效要等归属回包确认，不能让
    /// 上一个房间的透明状态在查询期间冒充当前结果。
    fn clear_room_xray(&mut self) {
        self.xray_room_pending = None;
        if self.xray_active_room.take().is_some() {
            self.view3d_commands
                .push(Cmd::Model(plant_ui::ModelAction::SetXRay {
                    refnos: Vec::new(),
                }));
        }
    }

    fn begin_room_xray(&mut self, panel: RefU64, room: RefU64) {
        self.xray_room_pending = Some((panel, room));
        if let Some(panels) = self.room_panel_cache.get(&room).cloned() {
            self.finish_room_xray(panel, room, panels);
            return;
        }

        // RoomDetail 还要展开整间房的成员边，大房间可能很慢；X-Ray 只取直属面板。
        // 若详情也在途，它的同一份 panels 回包仍会刷新缓存并受相同陈旧门保护。
        let _ = self.bridge.req.send(data::Req::RoomPanels(room));
    }

    fn finish_room_xray(&mut self, panel: RefU64, room: RefU64, panels: Vec<RefU64>) {
        if self.xray_room_pending != Some((panel, room))
            || self.vm.selection.primary() != Some(panel)
        {
            return;
        }
        self.xray_room_pending = None;
        if panels.is_empty() {
            return;
        }
        let panel_count = panels.len();
        self.view3d_commands
            .push(Cmd::Model(plant_ui::ModelAction::SetXRay {
                refnos: panels,
            }));
        self.xray_active_room = Some(room);
        self.logs.info(
            &mut self.vm.logs,
            format!("房间 X-Ray 已启用：{room} · 面板 {panel_count} · 不透明度 25%"),
        );
    }

    /// 房间视图的上半程：记下在途目标，去数据层解析全量成员集。
    /// 下半程在 `Evt::RoomDetail` 到达时展开成隔离 + 取景（有实时渲染器时），
    /// 并把详情填进「房间」页签——同一次往返喂两个消费者。
    fn focus_room(&mut self, room: RefU64) {
        self.focus_room_pending = Some(room);
        self.room_pane_focus = Some(room);
        self.vm.room_detail.begin_query();
        let _ = self.bridge.req.send(data::Req::RoomDetail(room));
    }

    /// 「显示房间模型」：把这间房自己的几何取回来显示，再取景到它。
    ///
    /// 走的就是显示模型那条既有链路——房间 FRMW 当作一个范围目标交给
    /// [`Self::set_model_visible`]，没缓存过的范围由它去查、把面板几何取回来。
    /// 与 [`Self::focus_room`] 的分工见 `Cmd::ShowRoomModel` 的文档。
    ///
    /// 取景分两种情形：范围早就缓存过（这间房刚显示过一次）时不会再有
    /// `Evt::ModelScope` 回包，包围盒此刻就在视口手上，当场下发；否则记下这间房，
    /// 等范围落地那一刻再补取景。
    fn show_room_model(&mut self, room: RefU64) {
        self.set_model_visible(vec![room], true);
        match self.model_scopes.get(&room) {
            Some(refnos) => {
                let refnos = refnos.clone();
                self.pending_room_frame = None;
                self.view3d_commands
                    .push(Cmd::Model(plant_ui::ModelAction::FocusGroup { refnos }));
            }
            None => self.pending_room_frame = Some(room),
        }
    }

    /// 显示 / 隐藏一批模型：记下方向、把已知范围下发给视口、未知范围排队去查。
    ///
    /// `Cmd::Model(SetVisible)` 与 [`Self::show_room_model`] 共用它——房间模型
    /// 与树上点眼睛是同一件事，加载与进度反馈那一整套没有第二份的道理。
    fn set_model_visible(&mut self, refnos: Vec<RefU64>, visible: bool) {
        // 只记方向，不动 eye：图标要等三维真的画成这样再变。
        for refno in &refnos {
            let previous = self.tree.visibility_for(*refno, &self.model_scopes);
            self.tree
                .pending_visibility
                .entry(*refno)
                .or_insert(previous);
            self.tree.visibility_unavailable.remove(refno);
            self.tree.pending_direction.insert(*refno, visible);
        }
        let plan = model_visibility_plan(&refnos, &self.model_scopes, &self.model_scope_pending);
        if !plan.resolved.is_empty() {
            self.view3d_commands
                .push(Cmd::Model(plant_ui::ModelAction::SetVisible {
                    refnos: plan.resolved,
                    visible,
                }));
            if visible
                && (self.pending_models.is_some()
                    || !self.pending_incremental_models.is_empty()
                    || self.latest_mesh_progress.is_some())
            {
                let (done, total) = self
                    .latest_mesh_progress
                    .as_ref()
                    .map_or((0, 0), |progress| (progress.done, progress.total));
                self.model_show_waiting.extend(refnos.iter().copied());
                self.model_progress_until = None;
                self.vm.model_load = Some(ModelLoadVm::Loading {
                    label: "加载模型网格".into(),
                    done,
                    total,
                });
            }
            let still_loading = refnos
                .iter()
                .any(|refno| self.model_show_waiting.contains(refno));
            if !still_loading {
                let verb = if visible { "显示" } else { "隐藏" };
                self.logs.info(
                    &mut self.vm.logs,
                    format!("模型{verb}完成：{} 个目标", refnos.len()),
                );
                self.finish_model_progress(format!("模型{verb}完成"));
            }
        }
        if !plan.query.is_empty() {
            if self.model_scope_pending.is_empty() && self.model_show_waiting.is_empty() {
                self.model_scope_failures = 0;
            }
            self.model_scope_pending.extend(plan.query.iter().copied());
            let verb = if visible { "显示" } else { "隐藏" };
            self.logs.info(
                &mut self.vm.logs,
                format!("开始查询并{verb}模型：{} 个目标", plan.query.len()),
            );
            self.model_progress_until = None;
            self.vm.model_load = Some(ModelLoadVm::Resolving("解析模型范围…".into()));
            let targets = plan.query;
            if self
                .bridge
                .req
                .send(data::Req::ModelScopes {
                    epoch: self.model_scope_epoch,
                    targets: targets.clone(),
                })
                .is_err()
            {
                for target in targets {
                    self.model_scope_pending.remove(&target);
                    self.tree.pending_direction.remove(&target);
                    self.tree.visibility_unavailable.insert(target);
                }
                self.model_scope_failures += 1;
                let error = anyhow::anyhow!("数据查询线程已停止");
                self.logs
                    .error(&mut self.vm.logs, "已有模型查询失败", &error, None);
                self.finish_model_progress("模型加载完成");
            }
        }
    }

    /// 取回工作：把库里此刻的样子取到界面上来。
    ///
    /// 刷新范围只到「当前看得见的那些」——树是已展开、且子层已经在手里的分支加上
    /// 根层；三维是清场那一刻场景里存在的那批模型（显示 + 已隐藏，见
    /// [`Self::clear_scene_for_reload`]）。没展开的分支、没加载过的模型都不预取：
    /// 那是把一次刷新做成一次全量拉库，它们下次展开 / 点眼睛时本来就会现查。
    ///
    /// 与 `reconnect` 的分别在这里：重连从空缓存重来，展开状态、选中、属性一起
    /// 清掉；取回工作要的恰恰是这些都留着，只换里面的内容。
    fn get_work(&mut self) {
        self.get_work_with_models(true);
    }

    fn get_work_with_models(&mut self, reload_models: bool) {
        self.get_work_with_models_for_tasks(reload_models, Vec::new());
    }

    fn get_work_with_models_for_tasks(&mut self, reload_models: bool, task_ids: Vec<String>) {
        // 带终态任务的刷新覆盖全部本地缓存分支，而不只覆盖此刻展开的分支：折起的
        // children 仍会在下次展开时复用，漏掉它却标“已就地刷新”就是假完成。
        let branches: Vec<RefU64> = if task_ids.is_empty() {
            self.tree
                .expanded
                .iter()
                .filter(|refno| self.tree.children.contains_key(refno))
                .copied()
                .collect()
        } else {
            self.tree.children.keys().copied().collect()
        };
        self.start_get_work_for_tasks(branches, reload_models, task_ids);
    }

    /// 发一次取回工作。`branches` 是要重查子层的分支，由调用方按自己掌握的线索
    /// 算好——菜单点进来的算不出线索、只能把已展开的全给上，更新跑完那条路则
    /// 有一份确切的清单。
    fn start_get_work(&mut self, branches: Vec<RefU64>, reload_models: bool) {
        self.start_get_work_for_tasks(branches, reload_models, Vec::new());
    }

    fn start_get_work_for_tasks(
        &mut self,
        branches: Vec<RefU64>,
        reload_models: bool,
        task_ids: Vec<String>,
    ) {
        if !self.vm.data_source_ok {
            return;
        }
        if !begin_get_work(
            self.get_work_pending,
            reload_models,
            &mut self.get_work_again,
        ) {
            self.get_work_again_task_ids.extend(task_ids);
            return;
        }
        self.get_work_task_ids.extend(task_ids);
        self.set_get_work_busy(true);
        self.logs.info(
            &mut self.vm.logs,
            format!("正在取回工作：重查根层与 {} 个分支…", branches.len()),
        );
        let sent = self
            .bridge
            .req
            .send(data::Req::GetWork {
                branches,
                reload_models,
            })
            .is_ok();
        if !sent {
            self.get_work_task_ids.clear();
            self.set_get_work_busy(false);
        } else if reload_models {
            self.model_reload_owed = false;
            self.model_reload_in_flight = true;
            self.clear_scene_for_reload();
        }
    }

    /// 取回工作的清场半步：拍快照、despawn 整个场景、台账复位（决定 2/5/6）。
    ///
    /// 这批复位原先挂在 `Evt::Models` 的成功分支里，提前到点击这一刻：eye 立刻
    /// 回「未加载」，加载期间不抢说「已显示」（ADR-0016）；旧几何也不再在冷查询
    /// 期间冒充新数据。`Replace(空)` 会 despawn 全部场景根，不必新造一个清空动作。
    ///
    /// 快照只记第一次：清场之后场景一直是空的，欠账补载那一轮再拍只会拍到空白，
    /// 把用户真正的显示 / 隐藏冲掉。
    fn clear_scene_for_reload(&mut self) {
        if self.model_reload_restore.is_none() {
            self.model_reload_restore = Some(reload_snapshot(
                &self.loaded_models,
                &self.tree.pending_direction,
                &self.tree.visibility,
            ));
        }
        let snapshot = self.model_reload_restore.as_deref().unwrap_or_default();
        let total = snapshot.len();
        let hidden = snapshot.iter().filter(|(_, visible)| !visible).count();
        self.pending_models = Some(Vec::new());
        self.pending_incremental_models.clear();
        self.model_show_waiting.clear();
        self.latest_mesh_progress = None;
        self.model_scope_failures = 0;
        self.model_scopes.clear();
        self.model_scope_pending.clear();
        // 在途的范围查询说的是清场前那个世界，回包一律作废。
        self.model_scope_epoch = self.model_scope_epoch.wrapping_add(1);
        self.loaded_models.clear();
        self.tree.visibility.clear();
        self.tree.pending_direction.clear();
        self.tree.pending_visibility.clear();
        self.tree.visibility_unavailable.clear();
        self.model_progress_until = None;
        if total == 0 {
            self.vm.model_load = None;
            self.logs
                .info(&mut self.vm.logs, "三维本来就空着，这次只刷新树");
        } else {
            self.vm.model_load =
                Some(ModelLoadVm::Resolving("已清空三维，正在重查模型…".into()));
            self.logs.info(
                &mut self.vm.logs,
                format!("已清空三维场景：将重查 {total} 个模型（其中 {hidden} 个保持隐藏）"),
            );
        }
    }

    /// 这一份快照里**刚刚跨进终态**的那些数据批次带来的刷新线索。
    ///
    /// 合流之前这件事挂在向导那次运行的终态上；现在向导不跟运行了，只能由队列
    /// 轮询来发现跃迁——**不发现就等于批次跑完了、树和三维还停在旧模型上**，
    /// 而人只会以为更新没生效。
    ///
    /// 第一份可比较快照里，根层开始读取前结束的是历史，只登记；之后结束的批次
    /// 必须刷新，否则它可能刚好落在根层读取与首次队列轮询之间而永久漏掉。
    fn newly_finished(
        &mut self,
        poll: &plant_ui::task_queue::Poll,
    ) -> (bool, Vec<model_update::RefreshUnit>, Vec<String>) {
        let Some(observed_at) = self.data_observed_at else {
            return (false, Vec::new(), Vec::new());
        };
        let first = !self.queue_refresh_baselined;
        let mut data_applied = false;
        let mut units = Vec::new();
        let mut task_ids = Vec::new();
        for task in &poll.tasks {
            if task.kind != plant_ui::task_queue::KIND_DATA_BATCH
                || !task.terminal()
                || !task_matches_project(&task.project, &self.vm.project)
            {
                continue;
            }
            if !self.queue_finished.insert(task.task_id.clone())
                || (first && !finished_after(task.finished_at.as_deref(), observed_at))
            {
                continue;
            }
            if let Some(outcome) = task.result.as_ref() {
                data_applied |= outcome.data_applied();
                let refresh = outcome.refresh_units();
                if !refresh.is_empty() {
                    task_ids.push(task.task_id.clone());
                }
                units.extend(refresh);
            }
        }
        self.queue_refresh_baselined = true;
        (data_applied, units, task_ids)
    }

    /// 按一份确切的清单取回工作，人不用记得再去点一下菜单。
    ///
    /// 与菜单点进来的那次不同：这里手上有确切线索，所以只重查真正会变的
    /// 分支——单元自己、它的原 OWNER、新 OWNER，外加树里记着的当前父节点
    /// （元素被移走时后端的 `old_owner` 未必解得出来，本端的缓存却还留着移动前
    /// 那一头）。模型刷新只会在后台欠账全部清零后走到这里。
    fn refresh_for_units(&mut self, units: Vec<model_update::RefreshUnit>, task_ids: Vec<String>) {
        if units.is_empty() {
            return;
        }
        if !task_ids.is_empty() {
            self.get_work_with_models_for_tasks(true, task_ids);
            return;
        }
        let mut branches: HashSet<RefU64> = HashSet::new();
        for unit in &units {
            branches.extend(unit.old_owner);
            branches.extend(unit.new_owner);
            branches.extend(self.tree.parent.get(&unit.root).copied());
            branches.insert(unit.root);
        }
        // 只留子层已经在手里的分支。没展开过的下次展开时本来就会现查，现在去拉
        // 就是把一次精确刷新做成一次预取。
        //
        // 但**够不着的那些不能就这么丢掉**：整棵子树是新增的时候（粘贴 / 新建 /
        // 导入），交付单元根与它的 OWNER 都是新元素、都不在缓存里，于是这一句会把
        // 集合清空——而真正多了一个孩子的那一层比它们还高，客户端手上根本没有那条
        // 链。gen-model issue #9 就是这个形状：数据全对，树上却不出现。
        let unresolved: Vec<RefU64> = branches
            .iter()
            .copied()
            .filter(|refno| !self.tree.children.contains_key(refno))
            .collect();
        branches.retain(|refno| self.tree.children.contains_key(refno));
        for refno in unresolved {
            self.resolve_refresh_anchor(refno);
        }
        self.start_get_work_for_tasks(branches.into_iter().collect(), true, task_ids);
    }

    /// 这个元素不在缓存里，现查一条祖先链，找出**第一个已经展开过的祖先**来刷新。
    ///
    /// 交付单元只走到「单元根 + 它的直接 OWNER」，新增子树的这两层都是新的；树形状
    /// 变化发生在更高的那一层，只有链能说出它是谁。链回来之后落到
    /// [`Self::refresh_from_chain`]。
    fn resolve_refresh_anchor(&mut self, refno: RefU64) {
        if self.refresh_anchors.insert(refno) {
            let _ = self.bridge.req.send(data::Req::Ancestors(refno));
        }
    }

    /// 祖先链回来了，找第一个已展开的祖先重查它的子层。
    ///
    /// 一个都找不到就什么也不做：那条分支从没展开过，下次展开时本来就会现查，
    /// 现在去拉是把一次精确刷新做成一次预取。
    fn refresh_from_chain(&mut self, refno: RefU64, chain: &[RefU64]) -> bool {
        if !self.refresh_anchors.remove(&refno) {
            return false;
        }
        let Some(anchor) =
            refresh_anchor(chain, |ancestor| self.tree.children.contains_key(&ancestor))
        else {
            return false;
        };
        self.start_get_work(vec![anchor], true);
        true
    }

    /// 重入闸门与界面上那句「正在取回工作…」共用一个真值，两者不许分开走。
    fn set_get_work_busy(&mut self, busy: bool) {
        self.get_work_pending = busy;
        self.vm.get_work_busy = busy;
    }

    fn run_deferred_get_work(&mut self) {
        if let Some(reload_models) = self.get_work_again.take() {
            let task_ids = self.get_work_again_task_ids.drain().collect();
            self.get_work_with_models_for_tasks(reload_models, task_ids);
        }
    }

    fn reconnect(&mut self) {
        // 新连接必须从空缓存开始：否则失败时状态栏和属性仍在说旧模型已经就绪。
        self.clear_room_xray();
        self.room_panel_cache.clear();
        self.model_scope_epoch = self.model_scope_epoch.wrapping_add(1);
        self.model_scopes.clear();
        self.tree.visibility_unavailable.clear();
        self.model_scope_pending.clear();
        self.loaded_models.clear();
        self.model_show_waiting.clear();
        self.pending_incremental_models.clear();
        self.latest_mesh_progress = None;
        self.model_scope_failures = 0;
        self.model_progress_until = None;
        self.pending_room_frame = None;
        self.vm.model_load = None;
        self.tree = TreeModel::default();
        self.vm.data_source_ok = false;
        self.vm.project.clear();
        self.mdb.clear();
        self.namespace.clear();
        // 队列面板那份跟着一起清。它平时随轮询回填，而轮询可能几秒才来一拍——
        // 那几秒里「立刻扫一遍」还亮着，按下去带的是上一次连接的 MDB。
        self.queue.mdb.clear();
        self.queue.namespace.clear();
        self.queue_finished.clear();
        self.refresh_anchors.clear();
        self.data_observed_at = None;
        self.queue_refresh_baselined = false;
        self.get_work_again = None;
        self.get_work_task_ids.clear();
        self.get_work_again_task_ids.clear();
        self.model_reload_owed = false;
        self.model_reload_in_flight = false;
        self.model_reload_restore = None;
        self.refresh_generation_pending = false;
        self.vm.refresh_generation = 0;
        self.vm.project_code.clear();
        self.vm.element_count = 0;
        self.vm.selection.clear();
        self.vm.tree_reveal = None;
        // 待滚动的目标属于旧树。新连接的根层还没到，留着它只会让第一帧的树乱滚一下。
        self.pending_locate = None;
        self.vm.tree = TreeVm::Loading;
        self.vm.props = PropsVm::Uninit;
        self.vm.rooms = RoomVm::Uninit;
        self.vm.room_detail = RoomDetailVm::Uninit;
        self.vm.room_view = None;
        self.focus_room_pending = None;
        self.pending_room_frame = None;
        self.room_pane_focus = None;
        // 换库了，旧库的房间总览不作数。窗口开着就顺手重拉，关着就等下次打开。
        if self.room_browser_state.open {
            self.rooms_overview.begin_query();
            let _ = self.bridge.req.send(data::Req::RoomsOverview);
        } else {
            self.rooms_overview = RoomBrowserVm::Idle;
        }
        self.logs.info(&mut self.vm.logs, "正在重连数据源…");
        let _ = self.bridge.req.send(data::Req::Reconnect);
    }

    fn refresh_model_update(&mut self) {
        if !self.model_service_writable() {
            self.model_update = ModelUpdateVm::Failed(model_update::Failure::new(
                "identity_mismatch",
                "模型服务范围与当前数据源不一致，或当前数据源身份尚未就绪",
            ));
            return;
        }
        self.model_update = ModelUpdateVm::Loading;
        let _ = self.bridge.req.send(data::Req::ModelUpdatePreview {
            base: self.model_api_url.clone(),
            project: self.vm.project.clone(),
            mdb: self.mdb.clone(),
            namespace: self.namespace.clone(),
        });
    }

    fn model_service_writable(&mut self) -> bool {
        if self.queue.can_mutate() {
            return true;
        }
        self.logs.warn(
            &mut self.vm.logs,
            "模型服务写操作已阻断：请先连接数据源，并确认 project / MDB / namespace 与服务范围一致",
        );
        false
    }

    /// 重开队列的明细通道。断线期间的事件不会补发，所以重连只让后续的行重新流进来，
    /// 已经缺掉的那些补不回，界面上「落后 N 条」照旧成立。
    fn reopen_queue_feed(&mut self, ctx: &egui::Context) {
        self.queue.feed = ModelUpdateFeed::Connecting;
        // sim 模式没有 WebSocket 可连：明细由引擎经同一条事件通道推送，
        // 通道状态直接置 Live，不去连一个不存在的服务再报断线。
        if sim::enabled() {
            let _ = self.bridge.evt_tx.send(data::Evt::QueueFeedLive);
            ctx.request_repaint();
            return;
        }
        self.queue_feed = Some(model_update_ws::Feed::open(
            &self.model_api_url,
            self.bridge.evt_tx.clone(),
            ctx.clone(),
        ));
    }

    /// 让下一帧立刻去取一份新快照。按下暂停 / 入队之后等满一拍再刷新，界面会有
    /// 一秒钟在说刚被推翻的话。
    fn poll_queue_now(&mut self) {
        let now = Instant::now();
        self.last_queue_poll = now.checked_sub(QUEUE_POLL_IDLE).unwrap_or(now);
    }

    /// 队列轮询。队列是常驻视图，这一拍从进程起来一直打到关窗——状态栏那格计数
    /// 就是靠它在面板折起时仍然叫得住人。
    ///
    /// 有活在跑时一秒一拍（行上的「已用 01:12」也要靠这一拍走字），全空时降到五秒。
    fn poll_queue(&mut self, ctx: &egui::Context) {
        let busy = !self.queue.queue.rows.is_empty();
        let every = if busy {
            QUEUE_POLL_BUSY
        } else {
            QUEUE_POLL_IDLE
        };
        ctx.request_repaint_after(every);
        if self.queue_poll_pending || self.last_queue_poll.elapsed() < every {
            return;
        }
        self.last_queue_poll = Instant::now();
        self.queue_poll_pending = true;
        let _ = self.bridge.req.send(data::Req::QueuePoll {
            base: self.model_api_url.clone(),
        });
    }

    fn rebuild_tree(&mut self) {
        // 方向落地与展平放在一处：eye 与「下一次点击往哪边翻」是同一份数据算出来的
        // 两个字段，分开更新迟早会出现图标已经变了、方向还停在上一次的那一帧。
        settle_pending_directions(
            &mut self.tree.pending_direction,
            &mut self.tree.pending_visibility,
            &self.model_scopes,
            &self.tree.visibility,
        );
        self.vm.tree = TreeVm::Ready(self.tree.flatten(&self.model_scopes));
        self.vm.element_count = self.tree.element_count();
    }

    /// 数据层属性 -> 面板分组。通用组固定 类型/名称/OWNER 顺序，
    /// ':' 前缀进 UDA 组，其余按数据侧的字母序进元件属性组。
    /// REFNO 不进任何一组——它是面板头的 refno 芯片，摆进通用组就重复了。
    fn build_props(&self, refno: RefU64, attrs: Vec<plant_ui_data::Attr>) -> PropsDataVm {
        let mut data = PropsDataVm {
            refno,
            ..Default::default()
        };
        let mut common: HashMap<String, PropRowVm> = HashMap::new();
        for attr in attrs {
            let muted = attr.value == "unset";
            let row = PropRowVm {
                key: attr.name.clone(),
                attr: attr.name,
                value: attr.value,
                kind: prop_kind(attr.kind),
                muted,
            };
            match row.attr.as_str() {
                "TYPE" => {
                    data.noun = row.value.clone();
                    common.insert(
                        "TYPE".into(),
                        PropRowVm {
                            key: "类型".into(),
                            ..row
                        },
                    );
                }
                "NAME" => {
                    data.name = row.value.clone();
                    common.insert(
                        "NAME".into(),
                        PropRowVm {
                            key: "名称".into(),
                            ..row
                        },
                    );
                }
                "OWNER" => {
                    common.insert("OWNER".into(), row);
                }
                "REFNO" => {}
                k if k.starts_with(':') => data.udas.push(row),
                _ => data.attrs.push(row),
            }
        }
        for k in ["TYPE", "NAME", "OWNER"] {
            if let Some(row) = common.remove(k) {
                data.common.push(row);
            }
        }
        // 无名元素面板头回退到 noun 序号名之前，先用 refno 占位。
        if data.name.is_empty() || data.name == "unset" {
            data.name = refno.to_string();
        }
        data
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EleTreeNode, ModelVisibility, ModelVisibilityPlan, PanelRoomResolution, RefU64,
        RowVisibility, TreeModel, TreeRowVm, Window, background_models_settled, begin_get_work,
        cache_model_scope, claim_unloaded_model_refnos, command_reply_is_current,
        complete_refresh_generation, expand_room_model_targets, finished_after, in_mdb_path,
        mark_model_scope_unavailable, model_drains_idle, model_progress_terminal, model_reload_due,
        model_visibility_plan, needs_children_query, refresh_anchor, reload_snapshot,
        resolve_panel_room_reply, restore_model_reload, settle_pending_directions,
        settle_refresh_generation, settled_pending_roots, sync_ime_window, take_hidden_for_replay,
        task_matches_project,
    };
    use chrono::{TimeZone, Utc};
    use plant_ui::model_update::PendingModelUnit;
    use std::collections::{HashMap, HashSet};

    fn ime_output(cursor: egui::Rect) -> egui::output::IMEOutput {
        egui::output::IMEOutput {
            rect: cursor.expand(8.0),
            cursor_rect: cursor,
            should_interrupt_composition: false,
        }
    }

    #[test]
    fn ime_focus_enables_window_and_positions_candidate_box() {
        let mut window = Window::default();
        let ime = ime_output(egui::Rect::from_min_max(
            egui::pos2(120.0, 30.0),
            egui::pos2(121.0, 48.0),
        ));

        sync_ime_window(&mut window, Some(&ime));

        assert!(window.ime_enabled);
        assert_eq!(window.ime_position, bevy::math::Vec2::new(120.0, 48.0));
    }

    #[test]
    fn losing_text_focus_disables_window_ime() {
        let mut window = Window {
            ime_enabled: true,
            ..Default::default()
        };

        sync_ime_window(&mut window, None);

        assert!(!window.ime_enabled);
    }

    #[test]
    fn ime_candidate_position_tracks_cursor_changes() {
        let mut window = Window::default();
        let first = ime_output(egui::Rect::from_min_max(
            egui::pos2(10.0, 20.0),
            egui::pos2(11.0, 40.0),
        ));
        let second = ime_output(egui::Rect::from_min_max(
            egui::pos2(210.0, 220.0),
            egui::pos2(211.0, 240.0),
        ));

        sync_ime_window(&mut window, Some(&first));
        sync_ime_window(&mut window, Some(&second));

        assert_eq!(window.ime_position, bevy::math::Vec2::new(210.0, 240.0));
    }

    fn refs(raw: &[&str]) -> Vec<RefU64> {
        raw.iter().map(|s| s.parse().unwrap()).collect()
    }

    #[test]
    fn panel_room_reply_distinguishes_enable_clear_and_stale_results() {
        let panel = RefU64(11);
        let other = RefU64(12);
        let room = RefU64(512);

        assert_eq!(
            resolve_panel_room_reply(Some(panel), panel, Some(room)),
            PanelRoomResolution::Enable { panel, room }
        );
        assert_eq!(
            resolve_panel_room_reply(Some(panel), panel, None),
            PanelRoomResolution::Clear
        );
        assert_eq!(
            resolve_panel_room_reply(Some(other), panel, Some(room)),
            PanelRoomResolution::Ignore
        );
    }

    #[test]
    fn incremental_load_keeps_every_tubi_row_for_a_new_branch() {
        let branch = RefU64(11);
        let already_loaded = RefU64(12);
        let incoming = [branch, branch, branch, already_loaded];
        let mut loaded = HashSet::from([already_loaded]);

        let claimed = claim_unloaded_model_refnos(&mut loaded, incoming);
        let kept = incoming
            .into_iter()
            .filter(|refno| claimed.contains(refno))
            .count();

        assert_eq!(kept, 3);
        assert_eq!(loaded, HashSet::from([branch, already_loaded]));
    }

    #[test]
    fn room_isolate_keeps_the_loaded_straight_tube_from_its_branch_scope() {
        let member = RefU64(10);
        let branch = RefU64(11);
        let unrelated = RefU64(12);
        let scopes = HashMap::from([
            (branch, vec![member, branch]),
            (unrelated, vec![unrelated]),
        ]);

        assert_eq!(
            expand_room_model_targets(vec![member], &scopes),
            vec![member, branch]
        );
    }

    fn node(refno: RefU64, noun: &str, children: u16) -> EleTreeNode {
        EleTreeNode {
            refno: refno.into(),
            noun: noun.to_owned(),
            name: format!("/{refno}"),
            children_count: children,
            ..Default::default()
        }
    }

    /// eye 那条链路的最小复现：点击 -> 查询 -> View3d 回执。
    ///
    /// 中间那几步用的就是 `handle_cmds` / `Evt::ModelScope` / `sync_render_states`
    /// 里的同一批函数，这里只把它们按真实顺序串起来，再补上三维那一侧的回执。
    struct Eyes {
        tree: TreeModel,
        scopes: HashMap<RefU64, Vec<RefU64>>,
        querying: HashSet<RefU64>,
        failures: usize,
    }

    /// SITE > ZONE > 两个几何行，整棵都展开。
    const SITE: RefU64 = RefU64(1);
    const ZONE: RefU64 = RefU64(2);
    const STRU: RefU64 = RefU64(3);
    const PANE_A: RefU64 = RefU64(11);
    const PANE_B: RefU64 = RefU64(12);

    impl Eyes {
        fn new() -> Self {
            let mut tree = TreeModel {
                roots: vec![node(SITE, "SITE", 1)],
                ..Default::default()
            };
            tree.children.insert(SITE, vec![node(ZONE, "ZONE", 2)]);
            tree.children
                .insert(ZONE, vec![node(PANE_A, "PANE", 0), node(PANE_B, "PANE", 0)]);
            tree.parent.insert(ZONE, SITE);
            tree.parent.insert(PANE_A, ZONE);
            tree.parent.insert(PANE_B, ZONE);
            tree.expanded.extend([SITE, ZONE]);
            Self {
                tree,
                scopes: HashMap::new(),
                querying: HashSet::new(),
                failures: 0,
            }
        }

        /// 同 `rebuild_tree`：先让落地的方向退场，再展平。
        fn row(&mut self, refno: RefU64) -> TreeRowVm {
            settle_pending_directions(
                &mut self.tree.pending_direction,
                &mut self.tree.pending_visibility,
                &self.scopes,
                &self.tree.visibility,
            );
            self.tree
                .flatten(&self.scopes)
                .into_iter()
                .find(|row| row.refno == refno)
                .expect("行不在展平结果里")
        }

        fn eye(&mut self, refno: RefU64) -> RowVisibility {
            self.row(refno).visibility
        }

        /// 点一行的眼睛，方向取行上算好的那个。
        fn click(&mut self, target: RefU64) -> ModelVisibilityPlan {
            let visible = self.row(target).next_visible;
            self.dispatch(target, visible)
        }

        fn dispatch(&mut self, target: RefU64, visible: bool) -> ModelVisibilityPlan {
            let previous = self.tree.visibility_for(target, &self.scopes);
            self.tree
                .pending_visibility
                .entry(target)
                .or_insert(previous);
            self.tree.visibility_unavailable.remove(&target);
            self.tree.pending_direction.insert(target, visible);
            let plan = model_visibility_plan(&[target], &self.scopes, &self.querying);
            self.querying.extend(plan.query.iter().copied());
            plan
        }

        /// 范围查询回来。返回这一刻该拿去操作模型的方向。
        fn resolve(&mut self, target: RefU64, models: &[RefU64]) -> Option<bool> {
            self.querying.remove(&target);
            if models.is_empty() {
                mark_model_scope_unavailable(
                    &mut self.tree.pending_direction,
                    &mut self.tree.visibility_unavailable,
                    &mut self.failures,
                    target,
                );
                return None;
            }
            self.tree.visibility_unavailable.remove(&target);
            for model in models {
                cache_model_scope(&mut self.scopes, *model, *model);
            }
            self.scopes.insert(target, models.to_vec());
            Some(
                self.tree
                    .pending_direction
                    .get(&target)
                    .copied()
                    .unwrap_or(true),
            )
        }

        fn fail_query(&mut self, target: RefU64) {
            self.querying.remove(&target);
            mark_model_scope_unavailable(
                &mut self.tree.pending_direction,
                &mut self.tree.visibility_unavailable,
                &mut self.failures,
                target,
            );
        }

        /// View3d 的一条回执。
        fn report(&mut self, refno: RefU64, visible: bool, loaded: usize, failed: usize) {
            self.tree.visibility.insert(
                refno,
                ModelVisibility {
                    visible,
                    mesh_loaded: loaded,
                    mesh_failed: failed,
                },
            );
        }

        fn hide_all(&mut self) {
            self.tree
                .pending_direction
                .values_mut()
                .for_each(|visible| *visible = false);
        }

        fn show_all(&mut self) {
            self.tree
                .pending_direction
                .values_mut()
                .for_each(|visible| *visible = true);
        }
    }

    /// eye 跟的是画面不是指令：点完到三维真的画出来之间，图标一动不动。
    #[test]
    fn the_eye_waits_for_the_render_receipt_instead_of_the_click() {
        let mut eyes = Eyes::new();

        let plan = eyes.click(PANE_A);
        assert_eq!(plan.query, vec![PANE_A]);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Unloaded);

        assert_eq!(eyes.resolve(PANE_A, &[PANE_A]), Some(true));
        // 模型已经在装网格了，画面上还什么都没有——这一刻仍然不许变。
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Unloaded);

        eyes.report(PANE_A, true, 3, 0);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Shown);
    }

    /// 几何行显示、隐藏，以及缓存之后再切换：第二次起不该再查一遍库。
    #[test]
    fn a_cached_geometry_row_toggles_without_querying_again() {
        let mut eyes = Eyes::new();
        eyes.click(PANE_A);
        eyes.resolve(PANE_A, &[PANE_A]);
        eyes.report(PANE_A, true, 2, 0);

        let plan = eyes.click(PANE_A);
        assert!(plan.query.is_empty(), "已缓存的范围不该再查一次");
        assert_eq!(plan.resolved, vec![PANE_A]);
        assert_eq!(
            eyes.eye(PANE_A),
            RowVisibility::Shown,
            "回执之前图标停在原样"
        );
        eyes.report(PANE_A, false, 2, 0);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Hidden);

        let plan = eyes.click(PANE_A);
        assert!(plan.query.is_empty());
        eyes.report(PANE_A, true, 2, 0);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Shown);
    }

    /// 容器行的 eye 由它那次显示解析出来的模型聚合而成。
    #[test]
    fn a_container_row_aggregates_the_models_it_resolved_to() {
        let mut eyes = Eyes::new();
        eyes.click(ZONE);
        eyes.resolve(ZONE, &[PANE_A, PANE_B]);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Unloaded);

        eyes.report(PANE_A, true, 1, 0);
        assert_eq!(
            eyes.eye(ZONE),
            RowVisibility::Unloaded,
            "同一范围未全部回执前保持旧 eye"
        );
        eyes.report(PANE_B, true, 1, 0);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Shown);

        // 只隐藏其中一个几何行：容器变半可见，SITE 那一层没解析过范围，仍是未加载。
        eyes.click(PANE_A);
        eyes.report(PANE_A, false, 1, 0);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Hidden);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Partial);
        assert_eq!(eyes.eye(SITE), RowVisibility::Unloaded);

        eyes.report(PANE_B, false, 1, 0);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Hidden);
    }

    /// 父范围已经把后代模型显示出来时，中间容器不该因为没单独查过 scope 而装成未加载。
    #[test]
    fn a_child_container_inherits_the_rendered_scope_of_its_parent() {
        let mut eyes = Eyes::new();
        eyes.tree.children.insert(ZONE, vec![node(STRU, "STRU", 1)]);
        eyes.tree
            .children
            .insert(STRU, vec![node(PANE_A, "PANE", 0)]);
        eyes.tree.parent.insert(STRU, ZONE);
        eyes.tree.parent.insert(PANE_A, STRU);
        eyes.tree.expanded.insert(STRU);

        eyes.click(ZONE);
        eyes.resolve(ZONE, &[PANE_A]);
        eyes.report(PANE_A, true, 1, 0);

        assert_eq!(eyes.eye(STRU), RowVisibility::Shown);

        eyes.click(ZONE);
        eyes.report(PANE_A, false, 1, 0);
        assert_eq!(eyes.eye(STRU), RowVisibility::Hidden);
    }

    #[test]
    fn an_explicit_empty_child_scope_overrides_inherited_visibility() {
        let mut eyes = Eyes::new();
        eyes.tree.children.insert(ZONE, vec![node(STRU, "STRU", 1)]);
        eyes.tree
            .children
            .insert(STRU, vec![node(PANE_A, "PANE", 0)]);
        eyes.tree.parent.insert(STRU, ZONE);
        eyes.tree.parent.insert(PANE_A, STRU);
        eyes.tree.expanded.insert(STRU);
        eyes.click(ZONE);
        eyes.resolve(ZONE, &[PANE_A]);
        eyes.report(PANE_A, true, 1, 0);

        eyes.click(STRU);
        assert_eq!(
            eyes.eye(STRU),
            RowVisibility::Shown,
            "子范围查询期间保持继承来的旧 eye"
        );
        eyes.resolve(STRU, &[]);
        assert_eq!(eyes.eye(STRU), RowVisibility::Unloaded);
    }

    /// 半可见的容器点一下是「显示全部」，不是把剩下那半也藏起来。
    #[test]
    fn clicking_a_half_visible_container_shows_everything() {
        let mut eyes = Eyes::new();
        eyes.click(ZONE);
        eyes.resolve(ZONE, &[PANE_A, PANE_B]);
        eyes.report(PANE_A, true, 1, 0);
        eyes.report(PANE_B, true, 1, 0);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Shown);

        // 容器上一轮显示已经落地，再单独隐藏一个后代，才形成真实的半可见。
        eyes.click(PANE_B);
        eyes.report(PANE_B, false, 1, 0);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Partial);
        assert!(eyes.row(ZONE).next_visible);

        eyes.click(ZONE);
        eyes.report(PANE_A, true, 1, 0);
        eyes.report(PANE_B, true, 1, 0);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Shown);
    }

    /// 查询期间连着反向点：图标全程不动，最终以最后一次指令为准。
    #[test]
    fn reversing_clicks_while_the_query_is_in_flight_obeys_the_last_one() {
        let mut eyes = Eyes::new();
        eyes.click(ZONE);
        for expected in [false, true, false] {
            let plan = eyes.click(ZONE);
            assert!(plan.query.is_empty(), "查询在途时不该再发一次");
            assert_eq!(eyes.tree.pending_direction[&ZONE], expected);
            assert_eq!(eyes.eye(ZONE), RowVisibility::Unloaded);
        }
        assert_eq!(eyes.resolve(ZONE, &[PANE_A]), Some(false));
        eyes.report(PANE_A, false, 1, 0);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Hidden);
    }

    /// 回执还没到就再点一下：方向翻回去，最终状态是最后一次点击那个。
    #[test]
    fn clicking_again_before_the_receipt_arrives_ends_on_the_last_direction() {
        let mut eyes = Eyes::new();
        eyes.click(PANE_A);
        eyes.resolve(PANE_A, &[PANE_A]);
        eyes.report(PANE_A, true, 1, 0);

        eyes.click(PANE_A);
        assert!(!eyes.tree.pending_direction[&PANE_A]);
        let plan = eyes.click(PANE_A);
        assert!(eyes.tree.pending_direction[&PANE_A]);
        assert_eq!(plan.resolved, vec![PANE_A]);
        eyes.report(PANE_A, true, 1, 0);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Shown);
    }

    /// 部分 mesh 失败 = 半可见；全部失败 = 三维里什么都没有，退回未加载。
    #[test]
    fn mesh_failures_show_up_as_half_visible_or_unloaded() {
        let mut eyes = Eyes::new();
        eyes.click(PANE_A);
        eyes.resolve(PANE_A, &[PANE_A]);

        eyes.report(PANE_A, true, 2, 1);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Partial);

        eyes.report(PANE_A, true, 0, 3);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Unloaded);
        // 死等一个永远对不上的方向没有意义：这次点击到此为止。
        assert!(!eyes.tree.pending_direction.contains_key(&PANE_A));
    }

    #[test]
    fn a_container_with_one_rendered_and_one_failed_model_is_partial() {
        let mut eyes = Eyes::new();
        eyes.click(ZONE);
        eyes.resolve(ZONE, &[PANE_A, PANE_B]);
        eyes.report(PANE_A, true, 2, 0);
        eyes.report(PANE_B, true, 0, 2);

        assert_eq!(eyes.eye(ZONE), RowVisibility::Partial);

        eyes.click(PANE_A);
        eyes.report(PANE_A, false, 2, 0);
        assert_eq!(eyes.eye(ZONE), RowVisibility::Partial);
    }

    /// 一个 mesh 都没有的模型：查得到、画不出，eye 停在未加载。
    #[test]
    fn a_model_without_any_mesh_stays_unloaded() {
        let mut eyes = Eyes::new();
        eyes.click(PANE_A);
        eyes.resolve(PANE_A, &[PANE_A]);
        eyes.report(PANE_A, true, 0, 0);

        assert_eq!(eyes.eye(PANE_A), RowVisibility::Unloaded);
        assert!(eyes.row(PANE_A).next_visible, "还是「让它出来」那一档");
    }

    /// 查空与查失败都不该在图标上留下痕迹。
    #[test]
    fn an_empty_or_failed_scope_query_restores_the_unloaded_eye() {
        let mut eyes = Eyes::new();
        eyes.click(PANE_A);
        assert_eq!(eyes.resolve(PANE_A, &[]), None);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Unloaded);
        assert!(!eyes.tree.pending_direction.contains_key(&PANE_A));

        eyes.click(PANE_B);
        eyes.fail_query(PANE_B);
        assert_eq!(eyes.eye(PANE_B), RowVisibility::Unloaded);
        assert!(!eyes.tree.pending_direction.contains_key(&PANE_B));
        assert_eq!(eyes.failures, 2);
    }

    /// `HideAll` / `ShowAll` 只动实际已加载的模型；在途的那次点击跟着改方向，
    /// 免得它查回来之后在一片全隐藏里独自冒出来。
    #[test]
    fn hide_all_and_show_all_only_move_models_that_are_really_loaded() {
        let mut eyes = Eyes::new();
        eyes.click(PANE_A);
        eyes.resolve(PANE_A, &[PANE_A]);
        eyes.report(PANE_A, true, 1, 0);
        // PANE_B 的范围还在查。
        eyes.click(PANE_B);

        eyes.hide_all();
        assert_eq!(
            eyes.eye(PANE_A),
            RowVisibility::Shown,
            "回执没来之前图标不动"
        );
        eyes.report(PANE_A, false, 1, 0);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Hidden);
        assert_eq!(eyes.resolve(PANE_B, &[PANE_B]), Some(false));
        eyes.report(PANE_B, false, 1, 0);
        assert_eq!(eyes.eye(PANE_B), RowVisibility::Hidden);

        eyes.show_all();
        eyes.report(PANE_A, true, 1, 0);
        eyes.report(PANE_B, true, 1, 0);
        assert_eq!(eyes.eye(PANE_A), RowVisibility::Shown);
        assert_eq!(eyes.eye(PANE_B), RowVisibility::Shown);
        // 从没显示过的容器不在「全部」里：三维里压根没有它的实体。
        assert_eq!(eyes.eye(SITE), RowVisibility::Unloaded);
    }

    /// 刚查回来的祖先链上，目标的直接父必须重查子层——哪怕缓存里已经有。
    ///
    /// gen-model issue #9：在 E3D 里复制一个 PIPE 粘到另一个 ZONE，增量更新把数据
    /// 应用了（实测库里 `pe_owner` 边、`owner` 标量都在，树的原始查询也返回得出
    /// 那一行），但界面上跳过去之后树上没有节点。原因就在这一格：那个 ZONE 在更新
    /// 之前展开过、子层进了缓存，而祖先链是现查的——缓存命中就跳过重查的话，新元素
    /// 永远进不了那份列表，收尾还会把「缓存旧了」讲成「它已不在这条路径下」。
    ///
    /// 只重查直接父那一节：再往上的祖先结构没变，为一次定位把整条链都重查是白花钱。
    #[test]
    fn a_freshly_resolved_parent_is_requeried_even_when_its_children_are_cached() {
        assert!(
            needs_children_query(true, true),
            "祖先链现查回来的直接父，缓存再新也可能比目标还老"
        );
        assert!(needs_children_query(true, false));
        assert!(
            needs_children_query(false, false),
            "没缓存的那几节照旧要补查"
        );
        assert!(
            !needs_children_query(false, true),
            "路径上其余各节有缓存就够，不该为一次定位重查整条链"
        );
    }

    /// 新增一整棵子树时，该刷新的那一层比交付单元的 OWNER 还高。
    ///
    /// gen-model issue #9 的第二半：粘贴一个 PIPE 进 ZONE，交付单元根是里面的 BRAN
    /// （`DEFAULT_DELIVERY_UNIT_TYPES` 里没有 PIPE，ZONE 更是永远不能当生成根），
    /// 而 `unit.new_owner` 只走到 PIPE——BRAN 与 PIPE 都是新元素、都没展开过，
    /// 「只留缓存里有的分支」那一句会把刷新集清空，多了一个孩子的 ZONE 一次都没被
    /// 重查。所以够不着的那些要现查一条链，取链上第一个展开过的祖先来刷。
    #[test]
    fn a_brand_new_subtree_refreshes_the_nearest_ancestor_that_is_actually_on_the_tree() {
        let [bran, pipe, zone, site] =
            refs(&["25688/76273", "25688/76272", "17496/171104", "17496/1"])
                .try_into()
                .unwrap();
        // 链：BRAN -> PIPE -> ZONE -> SITE。只有 ZONE 与 SITE 展开过。
        let chain = vec![bran, pipe, zone, site];
        let cached = |r: RefU64| r == zone || r == site;

        assert_eq!(
            refresh_anchor(&chain, cached),
            Some(zone),
            "要取最近的那一层，取到 SITE 就是把整棵树重查一遍"
        );
        // 一层都没展开过就什么也不做：下次展开时本来就会现查。
        assert_eq!(refresh_anchor(&chain, |_| false), None);
    }

    #[test]
    fn the_ancestor_path_stops_at_the_site_root() {
        // `fn::ancestor` 给的是「自己 -> 上级 -> …」，末尾那些层在 WORLD 一侧，
        // 属于别的 MDB。树定位不切换 MDB，所以路径到 SITE 根为止。
        let chain = refs(&[
            "24381/100677",
            "24381/100600",
            "24381/100500",
            "24381/1",
            "24381/0",
        ]);
        let site = chain[3];
        let path = in_mdb_path(&chain, |r| r == site).unwrap();
        assert_eq!(path, refs(&["24381/100600", "24381/100500", "24381/1"]));
    }

    #[test]
    fn a_target_outside_the_current_mdb_has_no_path() {
        // 链上一个都不是当前根：树外元素，选中并显示属性，但没有可展开的路径。
        let chain = refs(&["24381/100677", "24381/100600"]);
        assert!(in_mdb_path(&chain, |_| false).is_none());
    }

    #[test]
    fn a_site_root_target_needs_no_expansion() {
        // 目标自己就是根，路径为空——它那一行本来就在，别去展开它自己。
        let chain = refs(&["24381/1", "24381/0"]);
        let site = chain[0];
        assert!(in_mdb_path(&chain, |r| r == site).unwrap().is_empty());
    }

    #[test]
    fn refresh_requests_preserve_models_until_generation_finishes() {
        let mut deferred = None;
        assert!(begin_get_work(false, false, &mut deferred));
        assert!(!begin_get_work(true, false, &mut deferred));
        assert_eq!(deferred, Some(false));
        assert!(!begin_get_work(true, true, &mut deferred));
        assert_eq!(deferred, Some(true));
    }

    #[test]
    fn initial_queue_snapshot_only_refreshes_current_project_work_after_the_data_snapshot() {
        let observed = Utc.with_ymd_and_hms(2026, 7, 29, 3, 0, 0).unwrap();
        assert!(!finished_after(Some("2026-07-29T02:59:59Z"), observed));
        assert!(finished_after(Some("2026-07-29T03:00:01Z"), observed));
        // 缺时间戳时宁可多刷一次，也不能静默漏掉已经完成的数据批次。
        assert!(finished_after(None, observed));

        assert!(task_matches_project(
            "AvevaMarineSample",
            "AvevaMarineSample"
        ));
        assert!(!task_matches_project("OtherProject", "AvevaMarineSample"));
        // 老服务没带项目字段时保留兼容口径：未知不等于别的项目。
        assert!(task_matches_project("", "AvevaMarineSample"));
    }

    #[test]
    fn only_a_settled_pending_root_requests_a_model_reload() {
        let pending = PendingModelUnit {
            dbnum: 7997,
            root_refno: "24381/100677".into(),
            ..Default::default()
        };

        assert!(settled_pending_roots(false, true, &[pending.clone()], &[]).is_empty());
        assert!(
            settled_pending_roots(true, true, &[pending.clone()], &[pending.clone()]).is_empty()
        );
        assert!(settled_pending_roots(true, false, &[pending.clone()], &[]).is_empty());
        let other = PendingModelUnit {
            dbnum: 7997,
            root_refno: "24381/100817".into(),
            ..Default::default()
        };
        assert!(settled_pending_roots(true, true, &[pending.clone()], &[other]).is_empty());
        assert_eq!(
            settled_pending_roots(true, true, &[pending], &[])[0].root,
            "24381/100677".parse::<RefU64>().unwrap()
        );
    }

    #[test]
    fn pending_failure_then_empty_still_reloads_after_the_queue_drains() {
        let mut owed = false;
        assert!(!model_reload_due(&mut owed, true, false));
        assert!(owed);
        assert!(!background_models_settled(false, true, true, true, true));
        assert!(!background_models_settled(true, true, false, true, true));
        assert!(!background_models_settled(true, true, true, false, true));
        assert!(!background_models_settled(true, true, true, true, false));
        assert!(background_models_settled(true, true, true, true, true));
        assert!(model_reload_due(&mut owed, false, true));
    }

    #[test]
    fn a_running_model_drain_holds_the_ui_refresh_barrier_but_yielded_is_terminal() {
        let mut drain = plant_ui::task_queue::TaskEntry {
            task_id: "model-1".into(),
            kind: plant_ui::task_queue::KIND_MODEL_DRAIN.into(),
            state: "running".into(),
            project: "P".into(),
            ..Default::default()
        };
        assert!(!model_drains_idle(std::slice::from_ref(&drain), "P"));
        assert!(model_drains_idle(std::slice::from_ref(&drain), "OTHER"));
        drain.state = "yielded".into();
        assert!(model_drains_idle(&[drain], "P"));
    }

    #[test]
    fn refresh_generation_publishes_once_per_completed_barrier() {
        let mut pending = true;
        let mut generation = 7;
        assert!(complete_refresh_generation(&mut pending, &mut generation));
        assert_eq!(generation, 8);
        assert!(!complete_refresh_generation(&mut pending, &mut generation));
        assert_eq!(generation, 8);
    }

    #[test]
    fn mesh_failure_consumes_the_pending_barrier_without_publishing_generation() {
        let mut pending = true;
        let mut generation = 7;
        assert!(!settle_refresh_generation(
            &mut pending,
            &mut generation,
            false
        ));
        assert!(!pending);
        assert_eq!(generation, 7);
        assert!(!settle_refresh_generation(
            &mut pending,
            &mut generation,
            true
        ));
        assert_eq!(
            generation, 7,
            "later unrelated mesh success must not publish"
        );
    }

    #[test]
    fn failed_model_load_restores_the_reload_debt() {
        let mut owed = false;
        restore_model_reload(&mut owed, true);
        assert!(model_reload_due(&mut owed, false, true));
    }

    #[test]
    fn reload_snapshot_prefers_the_users_last_direction() {
        let shown = RefU64(1);
        let hidden = RefU64(2);
        let toggling = RefU64(3);
        let fresh = RefU64(4);
        let loaded = HashSet::from([shown, hidden, toggling, fresh]);
        let actual = |visible| ModelVisibility {
            visible,
            mesh_loaded: 1,
            mesh_failed: 0,
        };
        let visibility = HashMap::from([
            (shown, actual(true)),
            (hidden, actual(false)),
            // 用户刚点了隐藏、回执还没上来：快照信指令，不信旧回执。
            (toggling, actual(true)),
        ]);
        let pending = HashMap::from([(toggling, false)]);

        assert_eq!(
            reload_snapshot(&loaded, &pending, &visibility),
            vec![(shown, true), (hidden, false), (toggling, false), (fresh, true)],
            "刚进场没回执的按可见记，其余按指令 > 回执"
        );
    }

    #[test]
    fn replay_hides_only_survivors_and_consumes_the_snapshot() {
        let kept = RefU64(1);
        let gone = RefU64(2);
        let shown = RefU64(3);
        let mut restore = Some(vec![(kept, false), (gone, false), (shown, true)]);
        // 库里删掉的 gone 这次没查回来：不回放，免得对着空气记一笔隐藏账。
        let loaded = HashSet::from([kept, shown]);

        assert_eq!(take_hidden_for_replay(&mut restore, &loaded), vec![kept]);
        assert!(restore.is_none(), "快照回放即消费，下一次取回重新拍");
        assert!(take_hidden_for_replay(&mut restore, &loaded).is_empty());
    }

    #[test]
    fn unloaded_eye_queries_existing_models_before_changing_view3d_visibility() {
        let target = "24381/1".parse::<RefU64>().unwrap();
        let scopes = HashMap::new();
        let pending = HashSet::new();

        let plan = model_visibility_plan(&[target], &scopes, &pending);

        assert_eq!(plan.query, vec![target]);
        assert!(plan.resolved.is_empty());
    }

    #[test]
    fn cached_container_scope_reuses_actual_models_and_pending_clicks_are_deduplicated() {
        let [container, first, second, pending_target] =
            refs(&["24381/1", "24381/11", "24381/12", "24381/2"])
                .try_into()
                .unwrap();
        let scopes = HashMap::from([(container, vec![first, second])]);
        let pending = HashSet::from([pending_target]);

        let plan = model_visibility_plan(&[container, pending_target], &scopes, &pending);

        assert_eq!(plan.resolved, vec![first, second]);
        assert!(plan.query.is_empty());
    }

    #[test]
    fn eye_dispatch_does_not_call_the_model_generation_api() {
        let source = include_str!("main.rs");
        let handler = source
            .split("fn handle_cmds")
            .nth(1)
            .unwrap()
            .split("fn submit_command")
            .next()
            .unwrap();
        assert!(!handler.contains("ensure_model"));
        assert!(!handler.contains("/api/v1/model/ensure"));
    }

    #[test]
    fn batch_failure_becomes_terminal_only_after_queries_and_meshes_finish() {
        assert_eq!(model_progress_terminal(1, 1), None);
        assert_eq!(model_progress_terminal(0, 1), Some(false));
        assert_eq!(model_progress_terminal(0, 0), Some(true));
    }

    #[test]
    fn clear_generation_drops_late_query_replies() {
        assert!(command_reply_is_current(7, 7));
        assert!(!command_reply_is_current(6, 7));
    }
}

/// 数据层的总览行 -> 浏览器镜像。逐字段拷贝，排序沿用数据层（房号升序）。
fn build_overview_row(row: plant_ui_data::room::RoomOverviewRow) -> room_browser::Row {
    room_browser::Row {
        room: row.room,
        name: row.name,
        room_num: row.room_num,
        room_code: row.room_code,
        valid_name: row.valid_name,
        panel_count: row.panel_count,
        member_count: row.member_count,
        pending_recalc: row.pending_recalc,
    }
}

/// 数据层的房间详情 -> 绘制层镜像。取引用不取所有权：视口那半还要用同一份
/// 详情算隔离目标集。
fn build_room_detail(detail: &plant_ui_data::room::RoomDetail) -> RoomDetailDataVm {
    RoomDetailDataVm {
        room: detail.room,
        name: detail.name.clone(),
        room_num: detail.room_num.clone(),
        room_code: detail.room_code.clone(),
        panels: detail.panels.clone(),
        member_count: detail.member_count,
        member_refnos: detail.member_refnos.clone(),
        members: detail
            .members
            .iter()
            .map(|member| RoomMemberVm {
                refno: member.refno,
                name: member.name.clone(),
                noun: member.noun.clone(),
                inside_count: member.inside_count,
            })
            .collect(),
        pending_recalc: detail.pending_recalc,
    }
}

/// 数据层的房间归属 -> 绘制层镜像。逐字段拷贝，顺序原样保留（数据层已按
/// ADR-010 §5 排好，绘制层不重排）。
fn build_rooms(refno: RefU64, relations: Vec<plant_ui_data::room::RoomRelation>) -> RoomsDataVm {
    RoomsDataVm {
        refno,
        relations: relations
            .into_iter()
            .map(|r| RoomRelationVm {
                room: r.room,
                room_num: r.room_num,
                room_code: r.room_code,
                panel: r.panel,
                inside_count: r.inside_count,
                center_dist: r.center_dist,
                via_member: r.via_member,
            })
            .collect(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelRoomResolution {
    Ignore,
    Clear,
    Enable { panel: RefU64, room: RefU64 },
}

fn resolve_panel_room_reply(
    current_selection: Option<RefU64>,
    reply_for: RefU64,
    room: Option<RefU64>,
) -> PanelRoomResolution {
    if current_selection != Some(reply_for) {
        return PanelRoomResolution::Ignore;
    }
    room.map_or(PanelRoomResolution::Clear, |room| {
        PanelRoomResolution::Enable {
            panel: reply_for,
            room,
        }
    })
}

/// 数据层的属性形态 -> 绘制层的控件选择。数据层的 `Unset` / `Opaque` 在 UI 侧
/// 都只是「不给控件」，合成一个 `ReadOnly`。
fn prop_kind(kind: plant_ui_data::AttrKind) -> PropKind {
    use plant_ui_data::AttrKind as K;
    match kind {
        K::Unset | K::Opaque => PropKind::ReadOnly,
        K::Int => PropKind::Int,
        K::Real => PropKind::Real,
        K::Bool => PropKind::Bool,
        K::Text => PropKind::Text,
    }
}

/// 状态栏 / 标题栏的库标识，如 "ns 1516 / db 7997,7999,8000"。
fn db_label(ns: &str, db_nums: &[u32]) -> String {
    if db_nums.is_empty() {
        format!("ns {ns}")
    } else {
        let nums = db_nums
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        format!("ns {ns} / db {nums}")
    }
}

/// 目录对话框开着的时候主窗口收不到输入，不自己叫醒自己的话，选完的路径要等到
/// 下一次鼠标划过才落地。
#[cfg(not(target_arch = "wasm32"))]
const MESH_DIR_PICK_POLL: Duration = Duration::from_millis(120);

/// 设置窗里与本地文件系统打交道的那几件事。浏览器端整节不适用：那里既没有本地
/// 目录可选，也没有地方存这份设置——设置窗上那一行是灰的。
#[cfg(not(target_arch = "wasm32"))]
impl App {
    /// 草稿改过就重新校验一次网格目录。
    fn refresh_mesh_dir_status(&mut self) {
        if !self.settings_state.open {
            return;
        }
        let draft = self.settings_state.mesh_dir_draft().to_owned();
        if self.checked_mesh_dir.as_deref() == Some(draft.as_str()) {
            return;
        }
        self.settings_state
            .set_mesh_dir_status(settings_store::check_mesh_dir(&draft));
        self.checked_mesh_dir = Some(draft);
    }

    /// 开系统目录对话框。
    ///
    /// 必须离开主线程：对话框是阻塞的，开在渲染循环里整个应用会僵住，连它自己的
    /// 窗口都不重绘。
    fn browse_mesh_dir(&mut self) {
        if self.mesh_dir_pick.is_some() {
            // 已经开着一个了。再点也只会是同一个对话框，不该再起一个线程。
            return;
        }
        let start = [
            self.settings_state.mesh_dir_draft(),
            self.settings_state.mesh_dir_hint.as_str(),
        ]
        .into_iter()
        .map(str::trim)
        .find(|dir| !dir.is_empty() && std::path::Path::new(dir).is_dir())
        .map(std::path::PathBuf::from);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title("选择网格目录");
            if let Some(start) = start {
                dialog = dialog.set_directory(start);
            }
            let _ = tx.send(dialog.pick_folder());
        });
        self.mesh_dir_pick = Some(rx);
    }

    fn poll_mesh_dir_pick(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.mesh_dir_pick else {
            return;
        };
        match rx.try_recv() {
            Ok(picked) => {
                if let Some(dir) = picked {
                    self.settings_state
                        .set_mesh_dir_draft(dir.to_string_lossy().into_owned());
                }
                self.mesh_dir_pick = None;
                ctx.request_repaint();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(MESH_DIR_PICK_POLL)
            }
            // 线程没了却没送回东西，只能当这次没选。挂着不放的话「浏览…」再也点不动。
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.mesh_dir_pick = None,
        }
    }

    /// 存盘，并把网格目录换过去。
    fn persist_settings(&mut self, saved: &settings::Settings) {
        if let Err(error) = settings_store::save(saved) {
            self.logs.warn(
                &mut self.vm.logs,
                format!("{error:#}；这次的改动只在本次会话里有效"),
            );
        }
        let wanted = std::path::PathBuf::from(if saved.mesh_dir.trim().is_empty() {
            self.settings_state.mesh_dir_hint.clone()
        } else {
            saved.mesh_dir.clone()
        });
        if wanted == plant_ui_view3d::mesh_source::mesh_dir() {
            return;
        }
        plant_ui_view3d::mesh_source::set_mesh_dir(&wanted);
        self.logs.info(
            &mut self.vm.logs,
            format!("网格目录已切换到 {}", wanted.display()),
        );
        // 已经加载成功的网格不用重来：文件名是内容哈希，换个目录取到的是同一份内容。
        self.view3d_commands.push(Cmd::RetryFailedMeshes);
    }
}

#[cfg(target_arch = "wasm32")]
impl App {
    fn refresh_mesh_dir_status(&mut self) {}
    fn browse_mesh_dir(&mut self) {}
    fn poll_mesh_dir_pick(&mut self, _ctx: &egui::Context) {}
    fn persist_settings(&mut self, _saved: &settings::Settings) {}
}

impl App {
    fn ui(&mut self, ui: &mut egui::Ui) {
        self.pump_events();
        self.tick_model_progress();
        if self.model_progress_until.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let t = theme_tokens::current();
        let d = self.settings_state.saved.density;
        // 首帧与每次换主题，把视口配色送给三维侧。对着当前生效令牌比对而不是
        // 监听设置变更：主题的写入口不止设置窗一处，这里只认结果。
        let viewport_bg = (t.viewport_top, t.viewport_bottom, t.viewport_grid);
        if self.sent_viewport_bg != Some(viewport_bg) {
            self.sent_viewport_bg = Some(viewport_bg);
            self.view3d_commands.push(Cmd::SetViewportBackground {
                top: t.viewport_top,
                bottom: t.viewport_bottom,
                grid: t.viewport_grid,
            });
        }
        let mut cmds = workbench::show(ui, &t, d, &self.vm, &self.queue, &mut self.state);
        cmds.extend(data_publish::show(
            ui.ctx(),
            &t,
            d,
            &self.vm,
            &mut self.data_publish_state,
        ));
        cmds.extend(model_update::show(
            ui.ctx(),
            &t,
            d,
            &self.model_api_url,
            self.queue.applying(),
            &self.mdb,
            self.queue.health.as_ref().map(|health| health.sync_live),
            &self.model_update,
            &mut self.model_update_state,
        ));
        cmds.extend(room_browser::show(
            ui.ctx(),
            &t,
            d,
            &self.rooms_overview,
            &mut self.room_browser_state,
        ));
        cmds.extend(project_picker::show(
            ui.ctx(),
            &t,
            d,
            &mut self.project_picker_state,
        ));
        self.refresh_mesh_dir_status();
        let saved = settings::show(ui.ctx(), &t, d, &mut self.settings_state);
        if self.settings_state.take_browse_request() {
            self.browse_mesh_dir();
        }
        self.poll_mesh_dir_pick(ui.ctx());
        if let Some(saved) = saved {
            theme_tokens::apply(ui.ctx(), &saved.theme.tokens(), saved.density);
            let moved = self.model_api_url != saved.model_api_url;
            self.model_api_url = saved.model_api_url.clone();
            self.data_api_url = saved.data_api_url.clone();
            // 接入点面板报的必须是这一刻真正在用的地址，而不是启动时解出来那份。
            self.vm.access_point.model_api_url = self.model_api_url.clone();
            self.vm.access_point.data_api_url = self.data_api_url.clone();
            // 换了服务地址，那条长连接还挂在旧地址上——不重开的话队列明细会一直
            // 指着一台没人管的服务。
            if moved {
                self.reopen_queue_feed(ui.ctx());
                self.poll_queue_now();
            }
            self.persist_settings(&saved);
            ui.ctx().request_repaint();
        }
        // 绘制层只读、消费不掉自己的滚动请求，所以由这里判断它落地没有。
        self.settle_tree_reveal();
        self.handle_cmds(ui.ctx(), cmds);
        self.poll_queue(ui.ctx());
        // 这一帧才展开出来的行要等下一帧才画得到，别让界面停在这儿等下一次输入。
        // 子层还在路上的那种不在这里空转——祖先链和子层回来时数据线程会叫醒它。
        if self.tree_reveal_row_ready() {
            ui.ctx().request_repaint();
        }
    }
}
