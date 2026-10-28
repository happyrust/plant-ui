//! plant-ui-app：Bevy 宿主（bevy_egui35 + plant-ui + plant-ui-data）。
//! 默认进入主工作台 S1；`--gallery` 进入 M0 组件画廊；`--look-demo`（浏览器端 `?look_demo=1`；debug 构建或
//! `look-demo` feature）在主工作台里摆一座
//! 演示厂，不要后端也能看三维外观（E3D 外观移植计划）；`--look-demo=far`（`?look_demo=far`）只框远景检验件取景。
//! `--look-debug=<模式>`（`?look_debug=<模式>`，任何构建都带）把 E3D 后处理链的中间量直出到视口，模式名见
//! `plant_ui_view3d::e3d_look::post::SglLookDebugView::parse`。

mod command;
mod data;
mod data_publish_api;
mod dimension_layer;
mod focus_bounds;
#[cfg(not(target_arch = "wasm32"))]
mod gallery;
mod logs;
mod mbd_api;
mod model_update_api;
mod model_update_ws;
mod nav_history;
mod read_face;
mod search_index;
mod settings_store;
mod sim;
mod source_versions;
mod spatial_api;
mod virtual_hole;
mod virtual_hole_pick;
mod virtual_hole_session;
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
use plant_ui::data_publish::{self, State as DataPublishState};
use plant_ui::fonts;
use plant_ui::manual_data_publish::{self, State as ManualDataPublishState};
use plant_ui::model_mutation;
use plant_ui::model_update::{
    self, Feed as ModelUpdateFeed, State as ModelUpdateState, Vm as ModelUpdateVm,
};
use plant_ui::project_picker::{self, State as ProjectPickerState};
use plant_ui::room_browser::{self, State as RoomBrowserState, Vm as RoomBrowserVm};
use plant_ui::room_tree::{
    EyeSource, EyeState, ExpandNeed, LARGE_LOAD_CONFIRM_THRESHOLD, LeafSelector, RoomNodeId,
    RoomTreeAction, RoomTreeModel, RootsStatus, rooms_status_fallback,
};
use plant_ui::settings::{self, State as SettingsState};
use plant_ui::style::theme_tokens::{self, set_weight_families_ready};
use plant_ui::style::tokens::{Density, Tokens};
use plant_ui::task_queue;
use plant_ui::vm::{
    AccessPointVm, CommandLineKind, CommandLineVm, DimensionLabelVm, DimensionsDataVm,
    DimensionsVm, LogElement, ModelLoadVm, NavEntryVm, NavHistoryVm, PropKind, PropRowVm,
    PropsDataVm, PropsVm, RoomDetailDataVm, RoomDetailVm, RoomMemberVm, RoomRelationVm,
    RoomTreeConfirmVm, RoomTreeDataVm, RoomTreeVm, RoomViewVm, RoomVm, RoomsDataVm,
    RowVisibility, SearchHitVm, SearchRunVm, SearchVm, Selection, SubIndexVm, TreeRowVm, TreeVm,
    TruncationVm, View3dVm, WorkbenchVm,
};
use plant_ui::virtual_hole::{VirtualHoleOp, VirtualHoleReceipt, VirtualHoleStatus};
use plant_ui::workbench::{self, Pane, WorkbenchState};
use plant_ui::{CameraPose, Cmd, IsolateMatching, NavStep};
use plant_ui_data::{EleTreeNode, RefU64};
use virtual_hole_pick::{HoleFilePick, HolePickPoll, PickedHoleFile};
use virtual_hole_session::{ReceiptEffect, VirtualHoleSession};

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == read_face::parity::FLAG) {
        // 对拍探针：无头子命令，不起 Bevy、不开窗（计划 §5.4 / D14）。
        if let Err(error) = run_parity(args) {
            eprintln!("对拍失败：{error:#}");
            std::process::exit(1);
        }
        return;
    }
    let gallery = args.iter().any(|a| a == "--gallery");
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
            "canvas 必须是形如 #rs-plant 的 CSS id 选择器",
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
    // 三维外观：站点默认（config.json 的 look 段）打底，地址栏 `?look=…` 压一层、只活到刷新
    //（E3D 外观移植计划阶段四；浏览器端没有 settings.ron）。
    let query = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    let look = config
        .look
        .then(plant_ui::ViewportLookOverrides::from_query(&query))
        .apply(plant_ui::ViewportLook::default());
    settings_store::set_browser_look(look);
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
    let asset_root = configure_native_access_point()?;

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
    // DirectTree 的启动查询走 `model_update_api::base_url()`，不是 `App` 保存的
    // 请求地址。原生端既然已经采用了落盘设置，就必须在启动数据线程之前把同一份
    // 地址交给全局 API 客户端；否则界面显示的是设置值，首个 SITE 查询却仍会连
    // 出厂默认端口。
    model_update_api::set_base_url(settings.model_api_url.clone())?;
    // 供数模式（ADR-0026）：设置项那一格，开发期可由 PLANT_READ_FACE 压过。认不出的值
    // 出声一次、按设置走——不悄悄退回出厂默认。
    let read_face = settings_store::resolve_read_face(
        settings.read_face,
        std::env::var_os(settings_store::READ_FACE_ENV),
    );
    if let Some(warning) = read_face.warning.clone() {
        warnings.push(warning);
    }
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
        read_face,
        warnings,
    });

    run("#plant-ui", asset_root.to_string_lossy().into_owned());
    Ok(())
}

/// 对拍探针（计划 §5.4 / D14）：`plant-ui-app --read-face-parity …`。
///
/// 接入点的读法与界面同一条——项目配置给库账号、`settings.ron` 给模型服务地址
/// （`--service` 压过它）；供数模式那一格**不看**：探针两面都要，读面在 `parity::run`
/// 里各造一份。不起 Bevy，所以数据线程的运行时也得自己起一个。
#[cfg(not(target_arch = "wasm32"))]
fn run_parity(args: Vec<String>) -> anyhow::Result<()> {
    use anyhow::Context;

    let options = read_face::parity::parse_args(args)?;
    configure_native_access_point()?;
    let base = match options.service.clone() {
        Some(service) => service,
        None => match settings_store::load() {
            Ok(Some(settings)) => settings.model_api_url,
            Ok(None) => model_update_api::base_url(),
            Err(error) => {
                eprintln!("[对拍] {error:#}；模型服务地址按环境变量 / 出厂默认");
                model_update_api::base_url()
            }
        },
    };
    model_update_api::set_base_url(base)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("起 tokio 运行时失败")?;
    let report = runtime.block_on(read_face::parity::run(&options))?;
    read_face::parity::deliver(&report, options.out.as_deref())
}

/// 资产根 + 项目接入点：找到资产根，把旧版项目配置（库地址与账号）交给 `aios_core`，
/// 没有就记下「回落到工作目录 `DbOption.toml`」这一条。界面与对拍探针共用。
#[cfg(not(target_arch = "wasm32"))]
fn configure_native_access_point() -> anyhow::Result<std::path::PathBuf> {
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
    Ok(asset_root)
}

/// 网格目录的环境变量。夹在设置项与出厂默认之间（ADR-0008 的优先级）。
#[cfg(not(target_arch = "wasm32"))]
const PLANT_MESH_DIR: &str = "PLANT_MESH_DIR";

#[cfg(not(target_arch = "wasm32"))]
fn run_gallery() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 1000.0])
            .with_title("rs-plant"),
        ..Default::default()
    };
    eframe::run_native(
        "rs-plant",
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
        title: "rs-plant".into(),
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
            })
            // E3D 外观（阶段二）：`StandardMaterial` 的辅助几何（网格 / 三轴 / 断口 / 标注线）不进 prepass，
            // 在边线 / 伪阴影里等于 E3D 的 handles 全关；PBR 外观本来就不跑 prepass，不受影响。
            .set(bevy::pbr::PbrPlugin {
                prepass_enabled: false,
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
    // `--look-demo`（桌面）/ `?look_demo=1`（浏览器）：E3D 外观移植计划的演示厂，不要后端也能看三维外观；
    // `--look-demo=far` / `?look_demo=far` 再摆远景检验件并只框它取景。debug 构建总带着，release 构建要开 `look-demo` feature。
    #[cfg(any(debug_assertions, feature = "look-demo"))]
    if let Some(demo) = look_demo_requested() {
        app.insert_resource(demo);
    }
    // `--look-debug=<模式>` / `?look_debug=<模式>`：E3D 后处理链的中间量直出到视口（查两端画面差异用），任何构建都带。
    if let Some(view) = look_debug_requested() {
        app.insert_resource(view);
    }
    app.run();
}

/// 命令行 / 地址栏里的 `look_demo` 值 → 演示厂资源：`1` 演示厂，`far` 连远景检验件。别的值当没写。
#[cfg(any(debug_assertions, feature = "look-demo"))]
fn look_demo_from(value: &str) -> Option<plant_ui_view3d::e3d_look::demo::LookDemo> {
    use plant_ui_view3d::e3d_look::demo::LookDemo;
    match value {
        "1" => Some(LookDemo { far: false }),
        "far" => Some(LookDemo { far: true }),
        _ => None,
    }
}

/// `--look-demo`（= `--look-demo=1`）/ `--look-demo=far`。
#[cfg(all(
    not(target_arch = "wasm32"),
    any(debug_assertions, feature = "look-demo")
))]
fn look_demo_requested() -> Option<plant_ui_view3d::e3d_look::demo::LookDemo> {
    std::env::args().find_map(|a| {
        if a == "--look-demo" {
            return look_demo_from("1");
        }
        a.strip_prefix("--look-demo=").and_then(look_demo_from)
    })
}

/// 浏览器端看页面地址的查询串：`?look_demo=1` / `?look_demo=far`（可与别的参数并列）。
#[cfg(all(target_arch = "wasm32", any(debug_assertions, feature = "look-demo")))]
fn look_demo_requested() -> Option<plant_ui_view3d::e3d_look::demo::LookDemo> {
    query_value(&location_search(), "look_demo").and_then(|value| look_demo_from(&value))
}

/// `--look-debug=<模式>`（桌面）。
#[cfg(not(target_arch = "wasm32"))]
fn look_debug_requested() -> Option<plant_ui_view3d::e3d_look::post::SglLookDebugView> {
    std::env::args().find_map(|a| {
        a.strip_prefix("--look-debug=")
            .and_then(plant_ui_view3d::e3d_look::post::SglLookDebugView::parse)
    })
}

/// `?look_debug=<模式>`（浏览器）。
#[cfg(target_arch = "wasm32")]
fn look_debug_requested() -> Option<plant_ui_view3d::e3d_look::post::SglLookDebugView> {
    query_value(&location_search(), "look_debug")
        .and_then(|value| plant_ui_view3d::e3d_look::post::SglLookDebugView::parse(&value))
}

/// 页面地址的查询串（带 `?`）；拿不到就是空串。
#[cfg(target_arch = "wasm32")]
fn location_search() -> String {
    web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default()
}

/// 查询串里某个键的值（第一个；带不带 `?` 都行）。
#[cfg(any(test, target_arch = "wasm32"))]
fn query_value(search: &str, key: &str) -> Option<String> {
    search.trim_start_matches('?').split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| v.to_owned())
    })
}

/// 绘制层要的渲染目标尺寸是不是真的装得进窗口（物理像素）。窗口尺寸还不知道就放行。
fn viewport_fits_window(size: [u32; 2], window_px: Option<bevy::prelude::UVec2>) -> bool {
    window_px.is_none_or(|window| size[0] <= window.x && size[1] <= window.y)
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
    if let Err(error) = egui_inspection::attach_from_env(ctx, Some("rs-plant".into())) {
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
    windows: bevy::prelude::Query<&Window, bevy::prelude::With<bevy::window::PrimaryWindow>>,
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
    // 直段的逐段回执（ADR-0033 阶段四）：房间树直段行的眼睛照它画。
    let tube_states = view3d.take_tube_render_states();
    if !tube_states.is_empty() {
        app.sync_tube_render_states(tube_states);
    }
    // 房间视图的截断回执（直段截断计划 M4）：视口上一拍的进度抄给 HUD。
    app.sync_room_clip(view3d.room_clip_status());
    // 虚拟孔洞（ADR-0031 阶段四）：视口上一拍的布尔回执取回，落进会话。
    for receipt in view3d.take_virtual_hole_receipts() {
        app.apply_hole_receipt(receipt);
    }
    // 边线 / 伪阴影在这台设备上跑不了（WebGL2 缺半浮点颜色附件）时，设置窗那两格灰掉并说明原因；设置值照存。
    app.settings_state.post_effects_lock = (!view3d.post_effects_supported()).then(|| {
        "这台设备的渲染器没有半浮点颜色附件（WebGL2 缺 EXT_color_buffer_float），边线 / 伪阴影在这里不生效；设置照存，换台机器就有"
            .to_owned()
    });
    // 导航历史抄的相机：这一帧的位姿，与下面 `camera_rot` 同拍。
    app.camera_pose = Some(view3d.camera_pose);
    app.vm.view3d = Some(View3dVm {
        texture: view3d.texture,
        size: egui::vec2(view3d.size.x as f32, view3d.size.y as f32),
        live: true,
        measurement_active: false,
        camera_rot: view3d.camera_rot,
        axis_labels: view3d.axis_labels,
        grid_cell_mm: view3d.grid_cell_mm,
        // 尺寸标注文字：视口上一拍投影好的锚点（计划 B3），只带画内的那些。
        dimension_labels: view3d
            .visible_dimension_labels()
            .map(|(uv, text)| DimensionLabelVm {
                uv,
                text: text.to_owned(),
            })
            .collect(),
        // 虚拟孔洞选中行的标签（ADR-0031）：同一套投影，单独一份名单。
        hole_labels: view3d
            .visible_hole_labels()
            .map(|(uv, text)| DimensionLabelVm {
                uv,
                text: text.to_owned(),
            })
            .collect(),
        // 视口底色跟着视口此刻生效的外观走（E3D 外观移植计划）：纹理空白处透明，底色由绘制层画。
        background: look_background(view3d.look()),
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
    // 主选中一并下发：E3D 外观把它染 CE 黄，其余选中染 highlight 白。
    view3d.set_selection(app.vm.selection.to_vec(), app.vm.selection.primary());
    if let Some(models) = app.pending_models.take() {
        view3d.load(models);
    }
    for models in app.pending_incremental_models.drain(..) {
        view3d.append(models);
    }
    for (stale, models) in app.pending_refreshes.drain(..) {
        view3d.refresh(stale, models);
    }
    match app.pending_dimensions.take() {
        Some(DimensionLayerUpdate::Set(batch)) => view3d.set_dimensions(batch),
        Some(DimensionLayerUpdate::Clear) => view3d.clear_dimensions(),
        None => {}
    }
    // 房间体图层按序交：换房间是「先撤旧的、再挂新的」两条，顺序不能乱。
    for update in app.pending_room_volume.drain(..) {
        match update {
            RoomVolumeUpdate::Set(batch) => view3d.set_room_volume(batch),
            RoomVolumeUpdate::Truncate(on) => view3d.set_room_truncation(on),
            RoomVolumeUpdate::Clear => view3d.clear_room_volume(),
        }
    }
    // 虚拟孔洞图层指令按序交给视口（ADR-0031）。
    for op in app.pending_hole_ops.drain(..) {
        view3d.virtual_hole_op(op);
    }
    let window_px = windows.single().ok().map(|window| window.physical_size());
    let mut retried = 0;
    for command in app.view3d_commands.drain(..) {
        match command {
            Cmd::PickViewport(uv) => view3d.pick(uv),
            Cmd::Camera(gesture) => view3d.camera(gesture),
            Cmd::Model(action) => view3d.model(action),
            // 视口不可能比窗口大：比窗口还大的请求来自 egui 还没拿到真窗口尺寸的头几帧（浏览器里按缺省的
            // 10000×10000 布局），按它建图会要一张六七千像素见方的渲染目标（超采样再翻倍），WebGL2 分不出来还丢上下文。
            Cmd::ResizeViewport(size) => {
                if viewport_fits_window(size, window_px) {
                    view3d.resize(size);
                }
            }
            Cmd::SetViewportGrid(grid) => view3d.set_grid_color(grid),
            Cmd::SetViewportLook(look) => view3d.set_look(look),
            Cmd::SnapView { forward, up, fit } => view3d.snap(forward, up, fit),
            Cmd::RestoreCamera(pose) => view3d.restore_camera(pose),
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

/// 交给视口的尺寸标注层指令（计划 B3）：整层换成这一批，或整层撤掉。
enum DimensionLayerUpdate {
    Set(plant_ui_view3d::DimensionBatch),
    Clear,
}

/// 交给视口的房间体图层指令（2026-09-29 直段截断计划 M4）：挂上一间房、拨开关、撤掉。
enum RoomVolumeUpdate {
    Set(plant_ui_view3d::RoomVolumeBatch),
    Truncate(bool),
    Clear,
}

/// 一次房间视图的编排账（直段截断计划 §4.4）。
struct RoomViewCtx {
    room: RefU64,
    /// 进房间视图时的详情：拨开关、重装之后重新隔离都照它来。
    detail: plant_ui_data::room::RoomDetail,
    /// 这一次房间体请求的帧号（发出时 `room_view_epoch` 的值）。
    epoch: u64,
    /// 房间体没取到：面板照旧进隔离，取景退回面板与成员的并集，不截断。
    volume_failed: bool,
    /// 截断结果那一句日志已经为哪个开关值说过了。拨一下开关就再说一次。
    announced: Option<bool>,
}

/// 房间体回包是不是这一次房间视图要的：帧号与房间都得对上，连点两间房只认最后一间。
fn room_volume_is_current(ctx: &RoomViewCtx, epoch: u64, room: RefU64) -> bool {
    ctx.epoch == epoch && ctx.room == room
}

/// 视口的截断回执 → HUD 芯片那一格。
fn truncation_vm(status: &plant_ui_view3d::RoomClipStatus) -> TruncationVm {
    use plant_ui_view3d::RoomClipPhase;
    match &status.phase {
        RoomClipPhase::Loading { done, total } => TruncationVm::Loading {
            done: *done,
            total: *total,
        },
        RoomClipPhase::Computing { done, total } => TruncationVm::Computing {
            done: *done,
            total: *total,
        },
        RoomClipPhase::Ready => TruncationVm::Ready {
            cut: status.cut,
            hidden: status.hidden,
            whole: status.whole,
            skipped: status.skipped,
        },
        RoomClipPhase::Unavailable(reason) => TruncationVm::Unavailable(reason.clone()),
    }
}

/// 截断做完那一句日志的正文。
fn truncation_summary(status: &plant_ui_view3d::RoomClipStatus) -> String {
    let skipped = if status.skipped > 0 {
        format!(" · 未能截断 {} 段", status.skipped)
    } else {
        String::new()
    };
    format!(
        "截断 {} 段 · 房间外 {} 段不画 · 整段在内 {} 段{skipped}",
        status.cut, status.hidden, status.whole
    )
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
    /// 行节点的库号：`EleTreeNode.dbnum`（服务供数 `/tree/*` 注的那一格）落表。
    /// 库供数没有这一格，显示路径的库级预检查只认这张表里的库号。
    dbnum: HashMap<RefU64, u32>,
    /// 无名元素显示全称（`ZONE 4 of SITE 2`）还是短名（`ZONE 4`）。跟设置窗那一格
    /// （`Settings::tree_full_names`），改了就重新展平；有名字的元素两种都是 NAME。
    full_names: bool,
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

/// 尺寸标注的靶子只认 BRAN（计划 B2 / D3：PIPE 级批量首版不做）。HVAC 的 BRAN 同名
/// 也放行——它解不出尺寸是服务端 422 说的事（`MbdError::NotDimensionable`），
/// 客户端不替它预判。
fn is_bran_noun(noun: &str) -> bool {
    noun.eq_ignore_ascii_case("BRAN")
}

/// 树上一行显示的名字。`node.name` 是 E3D 的默认全名（有名字就是 NAME，无名是
/// `ZONE 4 of SITE 2`）；`short_name` 是数据侧给无名元素的短形态（`ZONE 4`），有名字的
/// 与老查询不带。不起名的老模型服务由服务供数那一面按同一规则补上短名
/// （`model_update_api::fill_legacy_default_names`），走到这里的无名元素都有名字可显示。
/// 默认显示短形态——E3D 模型树就是这么显示的；开了「显示全称」或没有短形态时显示 `name`。
fn tree_label(node: &EleTreeNode, full_names: bool) -> String {
    if full_names {
        return node.name.clone();
    }
    node.short_name
        .clone()
        .filter(|short| !short.trim().is_empty())
        .unwrap_or_else(|| node.name.clone())
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
        /// `branch` 是走到这一层时头上最近的 BRAN（右键「查看尺寸标注」的靶子，计划 B2）：
        /// 顺着 DFS 往下带，比每行回头爬一遍父链便宜，也不用绘制层自己找父行。
        fn walk(
            model: &TreeModel,
            scopes: &HashMap<RefU64, Vec<RefU64>>,
            nodes: &[EleTreeNode],
            depth: u16,
            branch: Option<RefU64>,
            rows: &mut Vec<TreeRowVm>,
        ) {
            for n in nodes {
                let refno = n.refno.refno();
                let expandable = (n.children_count > 0).then(|| model.expanded.contains(&refno));
                let visibility = model.visibility_for(refno, scopes);
                let dimension_branch = if is_bran_noun(&n.noun) {
                    Some(refno)
                } else {
                    branch
                };
                rows.push(TreeRowVm {
                    refno,
                    depth,
                    name: tree_label(n, model.full_names),
                    full_name: n.name.clone(),
                    noun: n.noun.clone(),
                    expandable,
                    loading: model.loading.contains(&refno),
                    visibility,
                    next_visible: next_click_visible(
                        model.pending_direction.get(&refno).copied(),
                        visibility,
                    ),
                    dimension_branch,
                });
                if expandable == Some(true)
                    && let Some(kids) = model.children.get(&refno)
                {
                    walk(model, scopes, kids, depth + 1, dimension_branch, rows);
                }
            }
        }
        let mut rows = Vec::new();
        walk(self, scopes, &self.roots, 0, None, &mut rows);
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

    /// 已加载树节点的 noun。PANEL 专用房间拓扑查询不得对 BRAN/EQUI 等普通
    /// 节点并发发出；旧实现每次选中都无条件查询，严格 schema 下会把普通元素
    /// 的正常选择误报成「PANEL 房间查询失败」。
    fn noun(&self, refno: RefU64) -> Option<&str> {
        self.roots
            .iter()
            .chain(self.children.values().flatten())
            .find(|node| node.refno.refno() == refno)
            .map(|node| node.noun.as_str())
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

    /// 这个元素在模型树上有一行吗。树只由当前 MDB 的设计库长出来，所以「在树上」
    /// 同时也是「它是设计库元素」的一份凭据（属性面板判空表要它，计划 §5.5）。
    /// 只看已缓存的那部分：没展开过的分支不算，那时候本端确实还不知道。
    fn contains(&self, refno: RefU64) -> bool {
        self.is_root(refno)
            || self.parent.contains_key(&refno)
            || self
                .children
                .values()
                .flatten()
                .any(|n| n.refno.refno() == refno)
    }

    /// 节点入库时把服务端注的库号落表（`EleTreeNode.dbnum`）；库供数 / 老服务端
    /// 没有这一格，按「说不出」处理。
    fn note_dbnum(&mut self, node: &EleTreeNode) {
        if let Some(dbnum) = node.dbnum.filter(|dbnum| *dbnum != 0) {
            self.dbnum.insert(node.refno.refno(), dbnum);
        }
    }

    /// 这个 refno 属于哪个库。节点自己的 `dbnum` 是直接答案；还没物化出来的行
    /// 沿 `parent` 链往上走，任一跳落表就算数。都说不出回 `None`。
    fn dbnum_of(&self, refno: RefU64) -> Option<u32> {
        let mut current = refno;
        loop {
            if let Some(dbnum) = self.dbnum.get(&current) {
                return Some(*dbnum);
            }
            current = *self.parent.get(&current)?;
        }
    }

    /// 已确认删除的分支即使 OWNER 未重查，也必须从缓存子层摘掉。
    fn detach_missing(&mut self, missing: &[RefU64]) {
        let missing: HashSet<_> = missing.iter().copied().collect();
        // Preserve identities until prune_unreachable collects the full unload set,
        // including a missing SITE with no cached children or parent entry.
        for refno in &missing {
            self.children.entry(*refno).or_default();
        }
        self.roots
            .retain(|node| !missing.contains(&node.refno.refno()));
        for children in self.children.values_mut() {
            children.retain(|node| !missing.contains(&node.refno.refno()));
        }
    }

    /// 取回工作之后的清扫：把从根层已经走不到的条目摘掉。
    ///
    /// 一次重查可以让整条分支消失——元素被删了，或者挪到了别的 OWNER 底下——
    /// 而它底下那些子层、`parent` 指向、展开标记还留在表里。不摘掉的话
    /// `ancestors` 会顺着 `parent` 走进一串已经不存在的祖先，状态栏的元素计数
    /// 也会一直虚高。
    fn prune_unreachable(&mut self) -> Vec<RefU64> {
        let mut known: HashSet<RefU64> = self
            .roots
            .iter()
            .chain(self.children.values().flatten())
            .map(|node| node.refno.refno())
            .collect();
        known.extend(self.children.keys().copied());
        known.extend(self.parent.keys().copied());
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
        self.visibility.retain(|refno, _| alive.contains(refno));
        self.pending_direction
            .retain(|refno, _| alive.contains(refno));
        self.pending_visibility
            .retain(|refno, _| alive.contains(refno));
        self.visibility_unavailable
            .retain(|refno| alive.contains(refno));
        self.dbnum.retain(|refno, _| alive.contains(refno));
        let mut removed: Vec<_> = known.difference(&alive).copied().collect();
        removed.sort_by_key(|refno| refno.0);
        removed
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

    /// 这个元素能挂尺寸标注的 BRAN（计划 B2 / D3）：自己是 BRAN 就是自己，是 BRAN 的
    /// 成员就顺着父链上溯到所在 BRAN；PIPE / ZONE / EQUI 这些回 None——它们头上没有
    /// BRAN。只看已加载的树：元素不在缓存里就回 None，那时候本端确实还不知道它是什么
    /// （视口拾取到的元素随定位把祖先一路加载回来，那之后就认得了）。
    fn dimension_branch(&self, refno: RefU64) -> Option<RefU64> {
        let noun_of = |refno: RefU64| {
            self.roots
                .iter()
                .chain(self.children.values().flatten())
                .find(|node| node.refno.refno() == refno)
                .map(|node| node.noun.as_str())
        };
        let mut cur = refno;
        // 与 `ancestors` 同一条封顶：数据异常造出环时不许挂住 UI 线程。
        for _ in 0..=self.parent.len() {
            if noun_of(cur).is_some_and(is_bran_noun) {
                return Some(cur);
            }
            cur = *self.parent.get(&cur)?;
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
    /// 「房间树」面板（plant3d-web「房间」页签移植）的状态机：根清单、各房取回的树、展开 / 在途 /
    /// 失败、树内选中。不做 IO：取数走 `data::Req::Spatial*`，回包喂进来；行的 eye 读模型树那份
    /// 实际可见性账（`tree.visibility` / `model_scopes`），不另记勾选表（计划 D4）。
    room_tree: RoomTreeModel,
    /// 房间树取数的帧号：刷新 / 重连进一帧，晚到的旧回包丢。
    room_tree_epoch: u64,
    /// 等树 / 等叶子补齐再执行的房间树动作（显隐 / 聚焦 / 隔离 / 加载；计划 D7）。
    room_tree_deferred: Vec<DeferredRoomTreeAction>,
    /// 「加载模型」超过阈值时开着的确认框。
    room_tree_confirm: Option<RoomTreeConfirmVm>,
    /// 待滚动的房间树行（外部选中联动 / 「在房间树查看」），行画出来了就收掉（同 `tree_reveal`）。
    room_tree_reveal: Option<RoomNodeId>,
    /// 根清单还没到就被点了「在房间树查看」的那间房：清单到了再定位。
    room_tree_reveal_room: Option<RefU64>,
    /// 「加载模型」落地后要取景的目标：全部目标的范围查询都回来了才 `FocusGroup`（理由同
    /// `pending_room_frame`：范围没回来前视口手上没有它们的包围盒）。
    room_tree_frame: Option<Vec<RefU64>>,
    /// 树内点选正在写全局选中：`set_selection` 里的联动看到它就不再反过来展开 / 滚动
    /// （plant3d-web 的 `internalSelection` 同法）。
    room_tree_internal_selection: bool,
    /// 直段的逐段实际可见性（ADR-0033 阶段四）：View3d 按 BRAN 整份回执、这里整份替换；BRAN → 直段身份 → 可见。
    /// 只有带身份进场景的直段在账上；房间树直段行的眼睛照它画。
    tube_visibility: HashMap<RefU64, HashMap<plant_ui::TubeSegment, bool>>,
    /// 直段行点过眼睛、还没落到画面上的方向（与 `TreeModel::pending_direction` 同一套规矩，只是按段记）。
    tube_pending: HashMap<RefU64, HashMap<plant_ui::TubeSegment, bool>>,
    data_publish_state: DataPublishState,
    manual_data_publish_state: ManualDataPublishState,
    project_picker_state: ProjectPickerState,
    settings_state: SettingsState,
    model_api_url: String,
    data_api_url: String,
    /// 这一刻实际生效的供数模式（ADR-0026）。数据线程里的读面就是按它造的；设置项被
    /// `PLANT_READ_FACE` 压过时它与 `settings_state.saved.read_face` 不同。
    read_face: settings::ReadFaceKind,
    /// 当前 MDB 名（带前导 `/`），连库时取回。模型更新的预览与执行都要带上它
    /// ——本期执行范围就是照这个 MDB 解出来的 DESI 库号。
    mdb: String,
    /// 当前数据源的 Surreal namespace；与项目、MDB 一起校验 gen-model 固定服务范围。
    namespace: String,
    /// 当前 MDB 声明的设计库编号。搜索用它做两件事：包含匹配的搜索范围，
    /// 以及判断一条命中在不在模型树里（不在的是树外元素）。
    desi_dbs: Vec<u32>,
    /// 搜索框每敲一下就发一次查询。晚到的旧结果靠它认出来丢掉——包含匹配要跑
    /// 几秒，那期间用户早就改了输入。
    search_epoch: u64,
    /// 最近一次搜索命中各自所在的库号，随每次结果整份换掉（≤ `data::SEARCH_LIMIT` 条）。
    ///
    /// 库供数下属性表空了要判它是不是元件库元素，而元件库元素只能从搜索进门——模型树
    /// 只长设计库元素。选中之后下拉多半已经关了、`vm.search` 也清了，所以这份库号
    /// 单独留着，别处不用（计划 §5.5）。
    search_hit_dbs: HashMap<RefU64, u32>,
    /// 「重新生成模型」/「清理模型」共用的确认框，`None` = 没开（ADR-0030 D2：确认 → 一发
    /// generate → 显示；清理：确认 → 一发 DELETE → 卸载）。一次只开一件事。
    mutation: Option<model_mutation::Vm>,
    /// 这一趟的帧号。确认那一刻钉住，generate / DELETE 的整批回执拿它认帧；换过目标、关过窗
    /// 都加一，上一趟的尾巴回来就丢。两种动作共用一个帧号：同时只会有一趟在跑。
    mutation_epoch: u64,
    /// 确认框打开到回执落地之间的那一趟：是哪件事、窗上的标题与右键那几行。`None` = 没在跑。
    mutation_run: Option<MutationRun>,
    /// generate 成功、正等显示回包替换旧几何的目标。右键之前这些目标名下的模型可能已经在
    /// 场景里（旧产物）；它们的 records 回包到了要先卸掉已加载的同名模型再装，否则
    /// `claim_unloaded_model_refnos` 会把重算过的几何当成「已经有了」。
    /// 清理请求超时的目标也进这里（服务端可能删了一半）：下次点眼睛重查，空了就把残留卸掉。
    regenerate_refresh: HashSet<RefU64>,
    /// 任务队列视图的数据。它不跟着任何一次运行走——队列是常驻的，进程活着它就在。
    queue: task_queue::Vm,
    /// 队列的明细长连接：订阅全部任务，逐条带 task_id 回来。
    queue_feed: Option<model_update_ws::Feed>,
    last_queue_poll: Instant,
    queue_poll_pending: bool,
    source_versions: source_versions::SourceVersions,
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
    /// 取回工作清场前的场景快照：模型集 + 方向，外加点过眼睛的范围目标。只记第一次——
    /// 清场之后场景一直是空的，重试轮次再拍只会拍到空白；重载成功落地才消费，
    /// 失败后的欠账补载仍按这份最初的快照重查、再回放。
    model_reload_restore: Option<ReloadSnapshot>,
    /// 换供数模式（ADR-0026 热切）清场前拍的那份快照。另放一格而不借
    /// `model_reload_restore`：换面走的是重连那套复位，它会把那一格清掉；新面 `Ready`
    /// 之后这份才移过去、按取回工作那条路重装。只记第一次——换面失败再换回来时场景
    /// 已经空了，再拍只会拍到空白。
    read_face_switch_restore: Option<ReloadSnapshot>,
    /// 库供数下「库与模型服务不是同一套身份」那句话已经说过了（计划 D12 后半）。
    /// 队列轮询忙时 1 秒一拍，不记着就是每拍刷一行；身份对上了、或者重连 / 换面把
    /// 身份清空了就复位，下次再撞上还得说。
    identity_mismatch_said: bool,
    /// 清场前点过眼睛、且范围回包非空的树目标——取回工作重装时服务供数以它们为祖先重查
    /// records 的范围（ADR-0030 D3；一个 ZONE 一次往返，不必逐叶子问）。只查不生。
    /// `model_scopes` 混着每个模型的自映射条目，不拿它当名单。
    scope_targets: HashSet<RefU64>,
    /// 增量整场查询已经完成，正等 View3d 报 mesh/AABB 全部落地。
    refresh_generation_pending: bool,
    /// 还没回包的那一次房间视图请求（`Cmd::FocusRoom` -> `Req::RoomDetail`）。
    /// 同时只留一个：连点两间房只聚焦最后一间，晚到的旧详情靠它认出来丢掉。
    focus_room_pending: Option<RefU64>,
    /// 房间视图的取景账：这一次进视图，房间体落地时要不要把相机对准它。
    ///
    /// 与 `focus_room_pending` 分开：那一格管「进不进视图」（详情回包靠它认领，页签与历史
    /// 都已经切过去了），这一格只管取景。拆开之后，`focus_room` 之后用户又按了 F 或点了
    /// 别处的定位，`supersede_camera_intents` 撤掉这一格，晚到的房间体照旧进视图、照旧
    /// 隔离截断挂 HUD，只是不再把相机从用户最新那一下上拽走。
    focus_room_frame: Option<RefU64>,
    /// 选中 PANEL 后等待房间详情的 X-Ray 请求：(发起时的 PANEL, 房间)。
    /// 选中一换立即清空；晚到的详情只有同时匹配这两个值才可改材质。
    xray_room_pending: Option<(RefU64, RefU64)>,
    /// 上一次已经记过日志的房间镜像失败，`消息|错误链` 拼成一个键。
    ///
    /// 房间归属是随选中预取的：库没起（或端口不通）时，每换一次选中就再失败一次，
    /// 每次都记一行红底 ERROR，几十次点选就把日志刷成同一句话的复读。同一个原因只
    /// 记一次，界面上的 `vm.rooms` 照旧每次更新成失败态；换原因、或者镜像恢复之后
    /// 复位，下次真出事还会说。
    mirror_error_said: Option<String>,
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
    /// 导航历史栈（S1-D）：一条 = 选择集 + 相机位姿 + 激活页签（+ 房间聚焦）。
    /// 记录点只有两处——`set_selection` 换了主选中、`focus_room` 聚焦了一间房。
    nav: nav_history::NavHistory,
    /// 视口这一帧发布的相机位姿。入栈 / 走一步时抄给历史；独立壳没有相机时为 `None`。
    camera_pose: Option<CameraPose>,
    /// 尺寸标注取数的帧号（`Req::PipeDimensions`，计划 B1）。每次右键「查看尺寸标注」
    /// 或清层都进一帧：大 BRAN 的求解要跑几秒，那期间人早就右键了别的 BRAN 或点了隐藏，
    /// 晚到的旧结果靠它认出来丢掉——与搜索 / 清点同一套取消口径。
    dimensions_epoch: u64,
    /// 等着交给视口的尺寸标注层指令（计划 B3）。与 `view3d_commands` 分开：那条队列装的是
    /// `plant_ui::Cmd`，而这份线段批次是视口自己的类型，绘制层 crate 不该认识它。同一帧
    /// 只留最后一条——换目标 / 清层都是整层替换，前一条还没交出去就已经作废。
    pending_dimensions: Option<DimensionLayerUpdate>,
    /// 正在进行的房间视图的编排账（2026-09-29 直段截断计划 M4）：详情、房间体的帧号与来龙去脉。
    /// `vm.room_view` 是它画给 HUD 的那一面，两者同生同灭。
    room_view: Option<RoomViewCtx>,
    /// 房间体请求的帧号。每进一次房间视图、每退一次、每次重装后重取都进一帧：晚到的房间体与
    /// 视口回执都拿它认，连点两间房只认最后一间。
    room_view_epoch: u64,
    /// 等着交给视口的房间体图层指令。与 `pending_dimensions` 同理：批次是视口自己的类型；
    /// 按序排着交（先撤旧的再挂新的），不是只留最后一条。
    pending_room_volume: Vec<RoomVolumeUpdate>,
    /// 虚拟孔洞会话（ADR-0031）：孔洞列表、映射与布尔状态的编排账。内存-only，
    /// 布尔结果只换三维画面，不写回正式模型。
    hole_session: VirtualHoleSession,
    /// 等着交给视口的虚拟孔洞图层指令（ADR-0031 阶段二 / 阶段四）。
    pending_hole_ops: Vec<VirtualHoleOp>,
    /// 虚拟孔洞表的文件选择：原生端是对话框线程，浏览器端是隐藏 input。
    hole_pick: Option<HoleFilePick>,
    /// 布尔回执「目标一个实例都没有」时的重发等待：宿主先显示模型，等网格落地
    /// （`sync_render_states` 里看到 `mesh_loaded`）再重发同一批孔洞的预览。可能同时等好几个目标
    /// （孔开在几面墙上、一面都没加载），所以是一个集合，谁先落地谁先重发。
    hole_retry_pending: HashSet<RefU64>,
    /// 「定位目标」点在还没加载的目标上：先显示它，等几何落地（视口手上才有包围盒）再取景；立刻发 `Focus`
    /// 相机是不动的。与 `hole_retry_pending` 同一处结算（`settle_hole_waits`）。
    hole_focus_pending: Option<RefU64>,
    /// 已经为「目标未加载」重试过的目标。每目标只重试一次——模型加载失败时
    /// 再重试也只是第二个 `no_instances`，徒增一轮空转。
    hole_target_retried: HashSet<RefU64>,
    bridge: data::Bridge,
    tree: TreeModel,
    /// 还没落地的那一次树定位。同时只留一个：连续定位只完成最后一次。
    pending_locate: Option<PendingLocate>,
    logs: logs::LogBuffer,
    pending_models: Option<Vec<plant_ui::ModelRecord>>,
    pending_incremental_models: Vec<Vec<plant_ui::ModelRecord>>,
    /// 重新生成之后的换装：（场景里要卸掉的旧 refno，重算过的记录），交给 `View3d::refresh`
    /// 同一批先卸后装（ADR-0030 D2 第 3 步）。
    pending_refreshes: Vec<(Vec<RefU64>, Vec<plant_ui::ModelRecord>)>,
    /// tree/container refno -> 实际可见 ModelRoot refno。
    model_scopes: HashMap<RefU64, Vec<RefU64>>,
    model_scope_pending: HashSet<RefU64>,
    model_scope_epoch: u64,
    /// 「这个库有没有按当前源生成的模型」的会话级结论：`GET /dbnums/{n}/model/roots`
    /// 探针的 `ready_total > 0`，或该库的目标真查出过 records（正写）。`false`
    /// 不压已确认的 `true`——探针晚回的空结论不许推翻证据。
    dbnum_model_known: HashMap<u32, bool>,
    /// 在途的库级探针（每库单飞，回包时摘掉）。
    dbnum_model_probing: HashSet<u32>,
    /// 显示路径「库无模型 / 查空 → 默认重新生成」的去重账：每 refno 每会话只自动
    /// 一发，右键显式「重新生成模型」另走 `begin_mutation` 不受此限。
    auto_generate_attempted: HashSet<RefU64>,
    focus_bounds_request: focus_bounds::Request<RefU64>,
    /// `clear` bumps this generation so replies from the cleared session are ignored.
    command_epoch: u64,
    loaded_models: HashSet<RefU64>,
    /// 几何行的 owner。清理范围沿它补树上没展开的那一跳（P1-1）。
    model_owner: HashMap<RefU64, RefU64>,
    model_show_waiting: HashSet<RefU64>,
    latest_mesh_progress: Option<plant_ui_view3d::MeshLoadProgress>,
    model_scope_failures: usize,
    model_progress_until: Option<Instant>,
    /// 范围定位的「正在查询」提示压着的到期时刻：(epoch, target, 到期)。
    ///
    /// 记录级目标（选中元件按 F）回包在毫秒级，立刻记一条 INFO 只会和结果那条挤在
    /// 同一秒里，纯属噪音。压 400 ms 再发：快查询只留结果一条，慢查询（SITE 级）
    /// 照样有反馈。到期时若请求已被取代或已回包，`accepts_bounds` 为假，这一句自动不发。
    focus_scope_pending: Option<(u64, RefU64, Instant)>,
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
    /// 上一次发给三维视口的主题网格线色。网格在三维侧重建、主题只有绘制层认识，
    /// 主题一换就要把新颜色送下去；对着这份留底比对，别把同一个颜色每帧发一遍。
    /// 视口底色不经三维侧：绘制层每帧按外观（`look_background`）画在视口纹理底下。
    sent_viewport_grid: Option<egui::Color32>,
    /// 上一次发给三维视口的外观（E3D 外观移植计划）。与 `sent_viewport_grid` 同一条房规：对着已存设置比对，
    /// 变了才发一整份；设置的写入口不止设置窗一处（恢复默认值、将来的 URL 覆盖），这里只认结果。
    sent_look: Option<plant_ui::ViewportLook>,
}

const COMMAND_CAP: usize = 2000;

/// 库供数下房间树面板的注释（计划 D2；ADR-0026：库供数缺的格要标出来，不许安静空着）。
const ROOM_TREE_STORE_NOTE: &str = "房间树只在服务供数下有：它由 gen-model 服务端聚合，库供数没有这一棵。设置 → 供数模式 切回「服务供数」即可";

/// 房间树一次动作前最多补几组未内联的叶子（照 plant3d-web `collectRefnos` 的 `maxGroups`）。
const ROOM_TREE_LEAF_GROUP_CAP: usize = 64;

/// 房间树上等数据到齐再做的动作（计划 D7：房间根没取过树先取；叶子未内联的组先补）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoomTreeDeferred {
    SetVisible(bool),
    Focus,
    Isolate,
    /// 「加载模型」；`confirmed` = 已过确认框（或不超阈值、不需要）。
    ///
    /// `frame` = 落地后要不要取景。挂起期间用户又发起了别的取景意图，这条就成了旧账：
    /// 显隐照做，相机不该再被它拽走（`supersede_camera_intents` 会把它置 false）。
    LoadModels {
        confirmed: bool,
        frame: bool,
    },
}

/// 一条挂着的房间树动作。`leaves_requested` = 这条动作有没有替自己补过一轮叶子（最多 64 组）：树还没取时挂起的动作
/// 没补过，树到了得先补再做——否则 `leaves_inline = false` 的大房间只会按已内联的那一小部分做（隔离会把房间自己的大半
/// 构件藏掉）；补过一轮之后还没补到的组就按已知的做（来源同）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeferredRoomTreeAction {
    id: RoomNodeId,
    action: RoomTreeDeferred,
    leaves_requested: bool,
}

/// 回包落地后一条挂着的房间树动作的去向（`settle_room_tree_deferred` 的裁决，不碰 IO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredStep {
    /// 树或叶子还在途：继续挂。
    Wait,
    /// 等的那一条没取回来：作废（原因由调用方取）。
    Abort,
    /// 树刚到、这条动作还没替自己补过叶子：先补一轮，补完再做。
    RequestLeaves,
    /// 到齐了（或补过一轮了）：做。
    Execute,
}

fn deferred_step(
    leaves_requested: bool,
    tree_loading: bool,
    tree_ready: bool,
    leaves_loading: bool,
    leaves_failed: bool,
) -> DeferredStep {
    if tree_loading {
        return DeferredStep::Wait;
    }
    if !tree_ready {
        return DeferredStep::Abort;
    }
    if !leaves_requested {
        return DeferredStep::RequestLeaves;
    }
    if leaves_loading {
        return DeferredStep::Wait;
    }
    if leaves_failed {
        return DeferredStep::Abort;
    }
    DeferredStep::Execute
}

/// 虚拟孔洞在等的目标此刻的去向（`settle_hole_waits` 的裁决，不碰 IO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HoleTargetVerdict {
    /// 范围在查 / 网格在装 / 默认重新生成在跑：继续等。
    Waiting,
    /// 目标至少有一个模型画出来了：可以做了。
    Loaded,
    /// 永远等不到：范围查空 / 网格全没加载成功 / 没有可画的几何。带一句原因（接在「目标 X」后面）。
    Unavailable(&'static str),
}

/// `models` = 范围查询解出的模型（没解出过是 `None`），`actual` = 三维的实际回执（只有 settled 的模型在账上）。
fn hole_target_verdict(
    scope_pending: bool,
    scope_unavailable: bool,
    models: Option<&Vec<RefU64>>,
    actual: &HashMap<RefU64, ModelVisibility>,
) -> HoleTargetVerdict {
    if let Some(models) = models.filter(|models| !models.is_empty()) {
        let states: Vec<Option<ModelVisibility>> =
            models.iter().map(|model| actual.get(model).copied()).collect();
        if states.iter().flatten().any(|state| state.mesh_loaded > 0) {
            return HoleTargetVerdict::Loaded;
        }
        if states.iter().all(Option::is_some) {
            let failed = states.iter().flatten().any(|state| state.mesh_failed > 0);
            return HoleTargetVerdict::Unavailable(if failed {
                "的网格文件没加载成功"
            } else {
                "没有可切的几何"
            });
        }
        return HoleTargetVerdict::Waiting;
    }
    if scope_pending {
        return HoleTargetVerdict::Waiting;
    }
    if scope_unavailable {
        return HoleTargetVerdict::Unavailable("在库里没有已生成的模型");
    }
    HoleTargetVerdict::Waiting
}

/// E3D 外观的视口底色（grey→白，渐变关是纯 grey）；PBR 外观还是跟主题，给 `None`。
/// 颜色由 view3d 的 `e3d_look::presets` 算（字节域，159 / 242），这里只换成 egui 的 `Color32`。
fn look_background(look: plant_ui::ViewportLook) -> Option<(egui::Color32, egui::Color32)> {
    use bevy::color::ColorToPacked;
    match look.mode {
        plant_ui::LookMode::Pbr => None,
        plant_ui::LookMode::E3d => {
            let (top, bottom) = plant_ui_view3d::e3d_look::presets::e3d_background(look.gradient);
            let to_color32 = |colour: bevy::prelude::Color| {
                let [r, g, b, _] = colour.to_srgba().to_u8_array();
                egui::Color32::from_rgb(r, g, b)
            };
            Some((to_color32(top), to_color32(bottom)))
        }
    }
}

/// 子串有命中时留给前缀那一段的行数上限（ADR-0023 决定 7）。总行数仍是
/// `data::SEARCH_LIMIT`，余下的补给子串——不切一刀的话前缀一满 20 条，
/// 下面那一段就永远露不出来。
const PREFIX_QUOTA: usize = 15;

/// 一条命中折成下拉里的一行。
///
/// `in_tree` 在**库名单还没到手时一律算真**：那一刻人人都在树外，满屏的「树外」
/// 标只是在说「我还不知道」。
fn hit_row(hit: plant_ui_data::NameHit, desi_dbs: &[u32]) -> SearchHitVm {
    SearchHitVm {
        refno: hit.refno,
        name: hit.name,
        noun: hit.noun,
        in_tree: desi_dbs.is_empty() || desi_dbs.contains(&hit.dbnum),
        dbnum: hit.dbnum,
    }
}

/// 右键「重新生成模型」/「清理模型」从打开确认框到回执落地之间的那一趟（ADR-0030 D2；
/// 2026-10-10 清理模型计划）。
///
/// 只记是哪件事、标题与右键那几行：客户端不清点、不解根，没有名单要守。
struct MutationRun {
    kind: model_mutation::Kind,
    label: String,
    /// 右键那几行。各一发 generate / DELETE；成功之后重新生成以它们为祖先发显示，清理把它们
    /// 名下已加载的模型卸掉。整库清理时为空。
    targets: Vec<RefU64>,
    /// 显示路径「库无模型 / 查空」发起的自动生成（`begin_auto_generate`）。与右键显式发起
    /// 的区别只在回执落地那一下：自动趟尊重期间翻成隐藏的方向，不再反掰回来。
    auto: bool,
    /// 库行「清理模型（整库）」的库号。有值时这一趟不走右键那几行。
    dbnum: Option<u32>,
    /// 「等房间参照模型」档的开关（`Req::RebuildRooms` 在用）：重建已发出、服务端
    /// 收不回——置位只是让数据线程停止轮询（「后台继续」），重建照跑到底。
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// 一发 generate 的回执归到哪一档（ADR-0030 D2 第 3 步）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GenerateOutcome {
    /// 服务端做完了（含「范围内本来就没有可画几何」）：对这个目标发显示。
    Display,
    /// 120 s 没等到、服务端后台继续：**不显示**，等它跑完再点眼睛。
    Background,
    /// 真失败：ERROR，不显示。
    Failed,
}

/// 回执 → 三档。`timeout` 是服务端那道 120 s 预算（它回 504 并说明后台继续跑），不是失败。
fn generate_outcome(result: &anyhow::Result<model_update_api::GenerateReply>) -> GenerateOutcome {
    match result {
        Ok(_) => GenerateOutcome::Display,
        Err(error) => {
            if model_update_api::failure_of(error).form() == model_update::FailForm::Timeout {
                GenerateOutcome::Background
            } else {
                GenerateOutcome::Failed
            }
        }
    }
}

/// 一发 `DELETE /model/subtree` 的回执归到哪一档（清理模型计划 C3）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupOutcome {
    /// 服务端删完了：卸载这个目标名下已加载的模型。
    Cleaned,
    /// 409：范围内有根正在生成、或同库有整库任务在跑。服务端一行没删，三维不动，稍后再试。
    Rejected,
    /// 连不上 / 超时：这条没有预算包装、同步删到底，客户端等不到**不等于没删**。三维不动，
    /// 这一行标成待重查，人点眼睛确认。
    Unknown,
    /// 其他失败：ERROR 原文，三维不动。
    Failed,
}

/// 回执 → 四档。`conflict` 就是 spec §4.5.1 的 409；`timeout` 在客户端这一侧还包括连不上。
fn cleanup_outcome(result: &anyhow::Result<model_update_api::CleanupReply>) -> CleanupOutcome {
    match result {
        Ok(_) => CleanupOutcome::Cleaned,
        Err(error) => {
            let failure = model_update_api::failure_of(error);
            if failure.code == "conflict" {
                CleanupOutcome::Rejected
            } else if failure.form() == model_update::FailForm::Timeout {
                CleanupOutcome::Unknown
            } else {
                CleanupOutcome::Failed
            }
        }
    }
}

/// 整库清理要卸的叶子：已加载、且高位 Ref0 落在回执 `ref0s` 里（E7）。
fn dbnum_unload_set(loaded: &HashSet<RefU64>, ref0s: &[u32]) -> Vec<RefU64> {
    let owned: HashSet<u32> = ref0s.iter().copied().collect();
    let mut gone: Vec<RefU64> = loaded
        .iter()
        .copied()
        .filter(|refno| owned.contains(&refno.get_0()))
        .collect();
    gone.sort_by_key(|refno| refno.0);
    gone
}

/// 清理成功之后三维里该卸哪些模型、本端「已知范围」里该摘哪些键（清理模型计划 D3）。
#[derive(Debug, Default, PartialEq, Eq)]
struct CleanupUnload {
    /// 要发 `ModelAction::Unload` 的叶子（只含此刻真在场景里的），按 refno 排序。
    refnos: Vec<RefU64>,
    /// 范围内的「已知范围」键：目标自己、以及祖先链含目标的键（含叶子的自映射）。
    dropped_scopes: Vec<RefU64>,
}

/// 沿 `tree.parent`，缺的那一跳用几何装进来时记下的 `model_owner` 补。
/// 走到目标就算在范围内，不必走到 SITE；链在碰到目标之前断了就停，不当成在范围内。
fn chain_hits_target(
    start: RefU64,
    target: RefU64,
    parent_of: &impl Fn(RefU64) -> Option<RefU64>,
) -> bool {
    if start == target {
        return true;
    }
    let mut cur = start;
    let mut seen = HashSet::from([start]);
    for _ in 0..64 {
        let Some(parent) = parent_of(cur) else {
            return false;
        };
        if parent == target {
            return true;
        }
        if !seen.insert(parent) {
            return false;
        }
        cur = parent;
    }
    false
}

/// 只看本端缓存、不发请求地圈出一个目标的清理范围。
///
/// 范围 = 目标自身 + 它子树下的一切。两类材料：
/// - `scopes` 里键等于目标、或半链碰到目标的键，**整条成员**都算在范围内——成员自己的链在不在
///   缓存里无所谓：范围键既然在目标之下，它名下的模型就都在目标之下（这是「保守」的那一半）；
/// - `loaded` 里自己就是目标、或半链碰到目标的叶子。
///
/// 半链：先走 `tree_parent`，这一跳没有再用 `model_owner`（几何行的 owner）。碰到目标即算在
/// 范围内，不必走到 SITE 根。链在碰到目标之前断了的叶子**不动**：本端分不清它在哪个 ZONE
/// 底下，猜成「在范围内」会把别处正看着的模型一并卸掉。漏掉的残留由下一次取回工作纠正。
fn cleanup_unload_plan(
    target: RefU64,
    scopes: &HashMap<RefU64, Vec<RefU64>>,
    loaded: &HashSet<RefU64>,
    tree_parent: impl Fn(RefU64) -> Option<RefU64>,
    model_owner: &HashMap<RefU64, RefU64>,
) -> CleanupUnload {
    let parent_of = |refno: RefU64| {
        tree_parent(refno).or_else(|| model_owner.get(&refno).copied())
    };
    let in_range = |refno: RefU64| chain_hits_target(refno, target, &parent_of);
    let mut dropped_scopes: Vec<RefU64> = scopes
        .keys()
        .copied()
        .filter(|key| in_range(*key))
        .collect();
    dropped_scopes.sort_by_key(|refno| refno.0);
    let mut refnos: HashSet<RefU64> = dropped_scopes
        .iter()
        .flat_map(|key| scopes[key].iter().copied())
        .filter(|refno| loaded.contains(refno))
        .collect();
    refnos.extend(loaded.iter().copied().filter(|leaf| in_range(*leaf)));
    let mut refnos: Vec<RefU64> = refnos.into_iter().collect();
    refnos.sort_by_key(|refno| refno.0);
    CleanupUnload {
        refnos,
        dropped_scopes,
    }
}

/// 一发成功回执的日志摘要：生成根数 / 重算数 / 命中数，加「内存供数」那半句（spec §4.12）。
///
/// 两种要点名的自相矛盾：服务端换了新状态名（按「做过了」计，但得说出来）；「已生成」却
/// 「没有可用模型」——它不是失败，可人会盯着一片空白等一个不会来的模型。
fn generate_summary(label: &str, reply: &model_update_api::GenerateReply) -> String {
    let mut line = match reply.status {
        model_update_api::GenerateStatus::NoRenderableGeometry => format!(
            "{label}：生成完成，范围内无可渲染几何（{} 个生成根）",
            reply.generation_root_count
        ),
        _ => format!(
            "{label}：生成完成，{} 个生成根（重算 {}、命中 {}）",
            reply.generation_root_count, reply.generated_root_count, reply.cached_root_count
        ),
    };
    if reply.status == model_update_api::GenerateStatus::Unknown {
        line.push_str(&format!(
            "；服务端回了一个不认识的状态（生成根 {}），按「做过了」计",
            reply.generation_root
        ));
    }
    if reply.status == model_update_api::GenerateStatus::Generated && !reply.model_available {
        line.push_str(&format!(
            "；报「已生成」但没有可用模型（生成根 {}），三维里不会多出东西",
            reply.generation_root
        ));
    }
    if reply.failed_root_count > 0 {
        // 部分失败要点名：三维里出来的只是补齐成功的那一半。
        let heads = reply
            .failed_roots
            .iter()
            .take(3)
            .map(|root| format!("{}（{}）", root.generation_root, root.error))
            .collect::<Vec<_>>()
            .join("；");
        line.push_str(&format!(
            "；{} 个生成根没生成出来{}{}",
            reply.failed_root_count,
            if heads.is_empty() { "" } else { "：" },
            heads
        ));
    }
    if let Some(note) = reply.source_note() {
        line.push_str(&note);
    }
    line
}

/// 确认框标题里的那个范围。单选就报那一行，多选点名头一个再报总数——
/// 「1 项」是废话，右键菜单那边也是这个口径。
fn regenerate_label(head: String, targets: usize) -> String {
    if targets > 1 {
        format!("{head} 等 {targets} 项")
    } else {
        head
    }
}

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

/// 取回工作清场前拍下的场景（ADR-0021 / ADR-0024）。只记第一次：清场之后场景一直
/// 是空的，重试轮次再拍只会拍到空白；重装成功落地才消费。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ReloadSnapshot {
    /// 场景里每个模型 refno 配上「取回前是否可见」。库供数的重查范围就是这份名单，
    /// 隐藏回放也按它。
    models: Vec<(RefU64, bool)>,
    /// 清场前点过眼睛、且范围回包非空的树目标。服务供数以它们为祖先重查 records
    /// （ADR-0030 D3，只查不生）；空表 = 老快照没有范围记录，服务供数退到按 `models` 问。
    targets: Vec<RefU64>,
}

fn prune_removed_reload_nodes(snapshot: &mut ReloadSnapshot, removed: &[RefU64]) {
    // Absence from the loaded tree alone is not deletion evidence: implicit
    // tubes and outside-tree models must remain in the reload snapshot.
    let removed: HashSet<_> = removed.iter().copied().collect();
    snapshot
        .models
        .retain(|(refno, _)| !removed.contains(refno));
    snapshot.targets.retain(|refno| !removed.contains(refno));
}

#[cfg(test)]
mod removed_reload_scope_tests {
    use super::*;

    #[test]
    fn deleted_scope_is_not_queried_or_restored_on_later_refreshes() {
        let deleted = RefU64(26292);
        let kept = RefU64(26233);
        let outside_tree = RefU64(90000);
        let mut snapshot = ReloadSnapshot {
            models: vec![(deleted, true), (kept, false), (outside_tree, true)],
            targets: vec![deleted, kept, outside_tree],
        };
        prune_removed_reload_nodes(&mut snapshot, &[deleted]);
        assert_eq!(snapshot.targets, vec![kept, outside_tree]);
        assert_eq!(snapshot.models, vec![(kept, false), (outside_tree, true)]);
        let next = snapshot.clone();
        prune_removed_reload_nodes(&mut snapshot, &[deleted]);
        assert_eq!(snapshot, next);
    }

    #[test]
    fn no_removal_evidence_preserves_scope_and_hidden_models() {
        let mut snapshot = ReloadSnapshot {
            models: vec![(RefU64(1), false)],
            targets: vec![RefU64(2)],
        };
        let before = snapshot.clone();
        prune_removed_reload_nodes(&mut snapshot, &[]);
        assert_eq!(snapshot, before);
    }
}

/// 取回工作清场前的快照：场景里每个模型 refno 配上「取回前是否可见」，外加
/// 点过眼睛的范围目标名单。
///
/// 方向的优先序：用户最后一次指令（`pending_direction`，模型行直接命中）>
/// 三维实际回执（`visibility`）> 可见。最后一档兜的是「刚被显示指令带进场景、
/// 回执还没上来」的模型。容器行上还没落地的在途指令拍不进来——反解要走范围表，
/// 清场重装拿到的就是那条指令发出前的样子，边缘窄且诚实。
///
/// 目标名单只按 refno 排序去重，不按树序：按祖先查 records 对同一片范围幂等，先问 ZONE
/// 还是先问它底下的 BRAN 结果一样（重复实例由记录合并去重）。
fn reload_snapshot(
    loaded: &HashSet<RefU64>,
    pending_direction: &HashMap<RefU64, bool>,
    visibility: &HashMap<RefU64, ModelVisibility>,
    scope_targets: &HashSet<RefU64>,
) -> ReloadSnapshot {
    let mut models: Vec<(RefU64, bool)> = loaded
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
    models.sort_by_key(|(refno, _)| refno.0);
    let mut targets: Vec<RefU64> = scope_targets.iter().copied().collect();
    targets.sort_by_key(|refno| refno.0);
    targets.dedup();
    ReloadSnapshot { models, targets }
}

/// 重载落地这一刻要回放的隐藏集：快照里方向为隐藏、且这次真的查了回来的那批。
/// 快照就此消费——回放只认清场那一刻的样子，成功之后它的使命就结束了。
fn take_hidden_for_replay(
    restore: &mut Option<ReloadSnapshot>,
    loaded: &HashSet<RefU64>,
) -> Vec<RefU64> {
    restore
        .take()
        .unwrap_or_default()
        .models
        .into_iter()
        .filter(|(refno, visible)| !visible && loaded.contains(refno))
        .map(|(refno, _)| refno)
        .collect()
}

/// 设置窗保存之后要不要换供数模式（ADR-0026 热切）。
///
/// 只有设置值真的与此刻生效的那一面不同才换；被 `PLANT_READ_FACE` 压过时不换——
/// 环境变量在场时那一格是灰的，进程活着就该一直按它走，落盘的值等下次不带环境变量
/// 启动时再生效（计划 D11）。
fn read_face_switch(
    current: settings::ReadFaceKind,
    saved: settings::ReadFaceKind,
    overridden: bool,
) -> Option<settings::ReadFaceKind> {
    (!overridden && saved != current).then_some(saved)
}

/// 一张空属性表的由来（计划 §5.5「不说谎五格」）。`None` = 照旧画表。
///
/// 服务供数下属性来自 e3d-io 直读，元件库元素照样有值，空表是别的事，不在这儿定论。
/// 库供数读的是 `ATT_*`，两种空法要分开说：
///
/// - **元件库元素**：压根不入模型本体库，切到服务供数才看得见。判据是它所在的库在
///   `/dbnums` 上是 `CATA` 行——`db_type` 由服务端给，客户端不自己按库号猜。
/// - **设计库元素**：`pe` 有这一行、属性没同步过来。树上长出来的元素都属这一档
///   （模型树只由当前 MDB 的设计库长出来），所以 `in_tree` 与查得到的设计库行同解。
///
/// gen-model 不在场时 `/dbnums` 是空的，两档都判不出来：只说「属性为空」，不猜。
fn empty_props_verdict(
    read_face: settings::ReadFaceKind,
    in_tree: bool,
    dbnum: Option<u32>,
    dbnums: &[task_queue::DbnumStatus],
) -> Option<&'static str> {
    if read_face != settings::ReadFaceKind::Store {
        return None;
    }
    let row = dbnum.and_then(|dbnum| dbnums.iter().find(|row| row.dbnum == dbnum));
    Some(match row {
        Some(row) if row.db_type.eq_ignore_ascii_case("CATA") => {
            "元件库元素不入模型本体库；切到服务供数可看"
        }
        Some(_) => "库里没有这个元素的属性（未同步）；切到服务供数可看",
        None if in_tree => "库里没有这个元素的属性（未同步）；切到服务供数可看",
        None => "属性为空",
    })
}

/// 服务端翻面通告落成的那一句日志（计划 §5.5）。
///
/// 翻的是**服务端**这个库的模型从内存还是 rocksdb 取（ADR-0025）。服务供数下客户端的
/// 实例正是经它来的，所以那两句说的就是本客户端下一次装载会拿到什么；库供数下实例直读
/// `inst_relate`、压根不经服务端，同样两句话就成了替服务端说本客户端的事——那时候只报
/// 这件事发生过，并且明说它不影响眼前这一份。
fn model_source_changed_line(
    dbnum: u32,
    source: task_queue::ModelSource,
    read_face: settings::ReadFaceKind,
) -> String {
    if read_face == settings::ReadFaceKind::Store {
        return format!(
            "服务端 db{dbnum} 的模型改由{}供数；本客户端库供数，不受影响",
            match source {
                task_queue::ModelSource::Database => "数据库",
                task_queue::ModelSource::Memory => "进程内存",
            }
        );
    }
    match source {
        task_queue::ModelSource::Database => format!(
            "db{dbnum} 的模型改由数据库供数：初始化发布已收口，rocksdb 为准；已显示的模型是同一版，不必重载"
        ),
        task_queue::ModelSource::Memory => format!(
            "db{dbnum} 的模型改由 API 从内存供数：库已清空待重建，重建收口前看到的模型活不过服务端重启"
        ),
    }
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

/// 一次队列轮询发现变化之后，界面该刷什么。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AutoRefresh {
    /// 已加载范围重载：树与三维一起更新，也用于自动保存变化。
    FullReload,
    /// 整棵树重查，三维保持原样。
    TreeOnly,
    /// 按交付单元精确重查那几个分支，三维同样保持原样。
    Units,
}

/// 有新保存、应用结果或已完成的交付单元时，调用方记入重载需求。
/// 模型屏障就绪后复用已加载范围重载，保留相机和显示方向；未就绪时先刷新树，
/// 需求留到后续轮询。读穿模式的保存没有队列任务，由 SourceVersions 检测。
fn auto_refresh(owed: bool, models_settled: bool, data_applied: bool) -> AutoRefresh {
    if owed && models_settled {
        AutoRefresh::FullReload
    } else if data_applied {
        AutoRefresh::TreeOnly
    } else {
        AutoRefresh::Units
    }
}

fn restore_model_reload(owed: &mut bool, failed_debt_reload: bool) {
    *owed |= failed_debt_reload;
}

fn selection_after_removed_nodes(selection: &Selection, removed: &[RefU64]) -> Selection {
    let mut next = selection.clone();
    for refno in removed {
        if next.contains(*refno) {
            next.toggle(*refno);
        }
    }
    next
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

/// 房间视图的隔离指令（直段截断计划 §4.4）。
///
/// - 截断开：只认 refno。成员与截断候选（BRAN / HANG）进来，不按 owner 展开，也不并已加载范围。
///   墙外的管件因此不再跟着整根分支亮起来，候选分支的直段交给视口截断。
/// - 截断关：原来的口径，成员经已加载范围展开，refno 或 owner 命中都留。
///
/// 面板默认不进来：房间体图层已经把墙画成半透明，场景里的实体面板再亮，就是米色墙叠着蓝色墙。
/// 房间体取不到时（`with_panels`）面板照旧进来，至少看得见墙。
fn room_view_isolate(
    detail: &plant_ui_data::room::RoomDetail,
    truncate: bool,
    scopes: &HashMap<RefU64, Vec<RefU64>>,
    with_panels: bool,
) -> plant_ui::ModelAction {
    let panels: HashSet<RefU64> = detail.panels.iter().copied().collect();
    let wanted = |refno: &RefU64| with_panels || !panels.contains(refno);
    let mut seen = HashSet::new();
    let panel_seed = detail.panels.iter().filter(|_| with_panels);
    if truncate {
        let refnos = panel_seed
            .chain(&detail.member_refnos)
            .chain(&detail.tube_carriers)
            .copied()
            .filter(|refno| wanted(refno) && seen.insert(*refno))
            .collect();
        plant_ui::ModelAction::Isolate {
            refnos,
            matching: IsolateMatching::Exact,
        }
    } else {
        let seed: Vec<RefU64> = panel_seed
            .chain(&detail.member_refnos)
            .copied()
            .filter(|refno| seen.insert(*refno))
            .collect();
        let refnos = expand_room_model_targets(seed, scopes)
            .into_iter()
            .filter(|refno| wanted(refno))
            .collect();
        plant_ui::ModelAction::Isolate {
            refnos,
            matching: IsolateMatching::RefnoOrOwner,
        }
    }
}

/// 房间体取不到时的兜底取景目标：原来的口径，面板在前、成员在后，经已加载范围展开。
fn room_view_fallback_focus(
    detail: &plant_ui_data::room::RoomDetail,
    scopes: &HashMap<RefU64, Vec<RefU64>>,
) -> Vec<RefU64> {
    let mut seen = HashSet::new();
    let targets = detail
        .panels
        .iter()
        .chain(&detail.member_refnos)
        .copied()
        .filter(|refno| seen.insert(*refno))
        .collect();
    expand_room_model_targets(targets, scopes)
}

fn cache_model_scope(scopes: &mut HashMap<RefU64, Vec<RefU64>>, target: RefU64, model: RefU64) {
    let models = scopes.entry(target).or_default();
    if !models.contains(&model) {
        models.push(model);
    }
}

/// 几何行装进来时记下 owner，给清理范围补 `tree.parent` 上缺的那一跳。自己指向自己的不记。
fn remember_model_owner(owners: &mut HashMap<RefU64, RefU64>, model: &aios_core::GeomInstQuery) {
    let leaf = model.refno.refno();
    let owner = model.owner.refno();
    if owner != leaf {
        owners.insert(leaf, owner);
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

/// 显示查空时那一句（ADR-0030 D1）：树行停「未加载」，INFO 一句说清出路，不记 ERROR。
/// 眼睛、视口工具栏、房间模型、树范围聚焦查空时都用它——显示从不生成，生成只在右键。
const EMPTY_SCOPE_HINT: &str = "范围内没有已生成的模型；右键「重新生成模型」可生成";

/// 老服务端（gen-model ADR-086 之前）对没生成过的范围回 409 `conflict`、消息以 `not_generated`
/// 开头。那不是查询失败，是「没生成过」——客户端把它当查空（ADR-0030 后果 6）。
fn scope_not_generated(error: &anyhow::Error) -> bool {
    let failure = model_update_api::failure_of(error);
    failure.code == "conflict" && failure.message.contains("not_generated")
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
fn access_point_vm(
    model_api_url: &str,
    data_api_url: &str,
    read_face: settings::ReadFaceKind,
) -> AccessPointVm {
    let mut vm = AccessPointVm {
        model_api_url: model_api_url.to_owned(),
        data_api_url: data_api_url.to_owned(),
        read_face,
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
            // 宿主没交底（浏览器端）：服务地址仍旧由 `base_url()` 那条链解出来；三维外观由 `start()` 按站点
            // 默认 + 地址栏覆盖算好交底，没交就是出厂位。
            adopted.model_api_url = model_update_api::base_url();
            adopted.data_api_url = data_publish_api::base_url();
            if let Some(look) = settings_store::browser_look() {
                adopted.look = look;
            }
        }
        let model_api_url = adopted.model_api_url.clone();
        let data_api_url = adopted.data_api_url.clone();
        // 浏览器端没人交底：那一侧没有库可连，只有服务供数。
        let read_face = startup
            .map(|startup| startup.read_face.kind)
            .unwrap_or_default();
        let mut settings_state = SettingsState::default();
        settings_state.mesh_dir_hint = startup
            .map(|startup| startup.default_mesh_dir.clone())
            .unwrap_or_default();
        // 环境变量压过设置时，设置窗那一格锁上并说明为什么（计划 D11）。
        settings_state.read_face_lock = startup.and_then(|startup| startup.read_face.lock_notice());
        let tree_full_names = adopted.tree_full_names;
        settings_state.adopt(adopted);
        let mut app = Self {
            // 连接前不摆任何工程数据：项目 / 库标识等 Ready 事件带真实值。
            // 接入点是例外——它说的是「这次冲着谁去」，连不上时反而最该看得见。
            vm: WorkbenchVm {
                user: std::env::var("USERNAME").unwrap_or_else(|_| "user".into()),
                access_point: access_point_vm(&model_api_url, &data_api_url, read_face),
                // 下拉靠它说清这两路命中覆盖到哪儿（计划 §5.5）。
                search: SearchVm {
                    coverage: read_face,
                    ..SearchVm::default()
                },
                ..Default::default()
            },
            state: WorkbenchState::default(),
            model_update: ModelUpdateVm::default(),
            model_update_state: ModelUpdateState::default(),
            rooms_overview: RoomBrowserVm::default(),
            room_browser_state: RoomBrowserState::default(),
            room_tree: RoomTreeModel::default(),
            room_tree_epoch: 0,
            room_tree_deferred: Vec::new(),
            room_tree_confirm: None,
            room_tree_reveal: None,
            room_tree_reveal_room: None,
            room_tree_frame: None,
            room_tree_internal_selection: false,
            tube_visibility: HashMap::new(),
            tube_pending: HashMap::new(),
            data_publish_state: DataPublishState::default(),
            manual_data_publish_state: ManualDataPublishState::default(),
            project_picker_state: ProjectPickerState::new(Vec::new(), false),
            settings_state,
            model_api_url,
            data_api_url,
            read_face,
            mdb: String::new(),
            namespace: String::new(),
            desi_dbs: Vec::new(),
            search_epoch: 0,
            search_hit_dbs: HashMap::new(),
            mutation: None,
            mutation_epoch: 0,
            mutation_run: None,
            regenerate_refresh: HashSet::new(),
            // 队列面板按它决定库行「模型来源」画不画、轮询失败怎么说（计划 §5.5）。
            queue: task_queue::Vm::new(read_face),
            queue_feed: None,
            // 开机就欠一拍：第一帧立刻去取第一份快照，别让面板空等一秒。
            last_queue_poll: Instant::now()
                .checked_sub(QUEUE_POLL_BUSY)
                .unwrap_or_else(Instant::now),
            queue_poll_pending: false,
            source_versions: source_versions::SourceVersions::default(),
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
            read_face_switch_restore: None,
            identity_mismatch_said: false,
            scope_targets: HashSet::new(),
            refresh_generation_pending: false,
            focus_room_pending: None,
            focus_room_frame: None,
            xray_room_pending: None,
            mirror_error_said: None,
            xray_active_room: None,
            room_panel_cache: HashMap::new(),
            pending_room_frame: None,
            room_pane_focus: None,
            nav: nav_history::NavHistory::default(),
            camera_pose: None,
            dimensions_epoch: 0,
            pending_dimensions: None,
            room_view: None,
            room_view_epoch: 0,
            pending_room_volume: Vec::new(),
            hole_session: VirtualHoleSession::new(),
            pending_hole_ops: Vec::new(),
            hole_pick: None,
            hole_retry_pending: HashSet::new(),
            hole_focus_pending: None,
            hole_target_retried: HashSet::new(),
            // 交互通道与模型通道都按这一个供数模式造读面（ADR-0026）。
            bridge: data::spawn(ctx.clone(), tasks, read_face),
            tree: TreeModel {
                full_names: tree_full_names,
                ..TreeModel::default()
            },
            pending_locate: None,
            logs: logs::LogBuffer::default(),
            pending_models: None,
            pending_incremental_models: Vec::new(),
            pending_refreshes: Vec::new(),
            model_scopes: HashMap::new(),
            model_scope_pending: HashSet::new(),
            model_scope_epoch: 0,
            dbnum_model_known: HashMap::new(),
            dbnum_model_probing: HashSet::new(),
            auto_generate_attempted: HashSet::new(),
            focus_bounds_request: Default::default(),
            command_epoch: 0,
            loaded_models: HashSet::new(),
            model_owner: HashMap::new(),
            model_show_waiting: HashSet::new(),
            latest_mesh_progress: None,
            model_scope_failures: 0,
            model_progress_until: None,
            focus_scope_pending: None,
            view3d_commands: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            checked_mesh_dir: None,
            #[cfg(not(target_arch = "wasm32"))]
            mesh_dir_pick: None,
            font_weights_pending,
            sent_viewport_grid: None,
            sent_look: None,
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

    fn pump_events(&mut self, ctx: &egui::Context) {
        if let Some(feed) = &mut self.queue_feed {
            feed.poll();
        }
        let mut dirty = false;
        while let Ok(evt) = self.bridge.evt.try_recv() {
            match evt {
                data::Evt::Ready(Ok(info)) => {
                    // 供数模式跟着说一句：库供数是保留档，连上了也该看得出这次是哪条路。
                    let msg = format!(
                        "已连接 {}（{}），根层 {} 个 SITE · {}",
                        info.project,
                        db_label(&info.ns, &info.db_nums),
                        info.sites.len(),
                        self.read_face.label()
                    );
                    self.logs.info(&mut self.vm.logs, msg);
                    self.vm.data_source_ok = true;
                    self.vm.project = info.project;
                    self.desi_dbs = info.db_nums;
                    self.vm.search.scope_dbs = self.desi_dbs.len();
                    // 模型更新面板的「显示 SITE」那一列。根层每个 SITE 只属于一个库，
                    // 反过来一个库可以有好几个，所以按库归堆；顺序照根层原序，不重排
                    // ——那是 MDB 里成员表的顺序，人在模型树上看到的就是它。
                    self.model_update_state.site_names = info.site_dbs.into_iter().fold(
                        std::collections::BTreeMap::new(),
                        |mut acc: std::collections::BTreeMap<u32, Vec<String>>, (dbnum, name)| {
                            acc.entry(dbnum).or_default().push(name);
                            acc
                        },
                    );
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
                    // 换面之后的库号是另一份世界的：整表重落，别留旧连接的账。
                    self.tree.dbnum.clear();
                    for site in &info.sites {
                        self.tree.note_dbnum(site);
                    }
                    self.tree.roots = info.sites;
                    // 换面之后的重装（ADR-0026 热切，计划 §5.3）：清场前的快照到这儿才
                    // 移进取回工作那一格，随后走取回工作那条现成路——`clear_scene_for_reload`
                    // 见已有快照就不重拍（此刻场景是空的，重拍只会拍到空白），
                    // `Evt::GetWork` 按快照重查已生成的模型（只查不生），显隐照原样回放。
                    if let Some(snapshot) = self.read_face_switch_restore.take() {
                        if snapshot.models.is_empty() {
                            self.logs.info(
                                &mut self.vm.logs,
                                "供数模式已切换；换面前三维空着，没有要重装的模型",
                            );
                        } else {
                            self.logs.info(
                                &mut self.vm.logs,
                                format!(
                                    "供数模式已切换；按换面前的快照重装 {} 个模型（{} 个范围目标）",
                                    snapshot.models.len(),
                                    snapshot.targets.len()
                                ),
                            );
                            self.model_reload_restore = Some(snapshot);
                            self.get_work();
                        }
                    }
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
                        self.tree.note_dbnum(kid);
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
                        // 空表不画成一张只剩 refno 的表：库供数下它另有由来，替它说出来。
                        Ok(kvs) if kvs.is_empty() => match empty_props_verdict(
                            self.read_face,
                            self.tree.contains(refno),
                            self.search_hit_dbs.get(&refno).copied(),
                            &self.queue.dbnums,
                        ) {
                            Some(verdict) => PropsVm::Verdict(verdict.to_owned()),
                            None => PropsVm::Ready(self.build_props(refno, kvs)),
                        },
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
                        Ok(relations) => {
                            // 镜像回来了：清掉「同一原因只记一次」的记账，下次再断还会说。
                            self.mirror_error_said = None;
                            RoomVm::Ready(build_rooms(refno, relations))
                        }
                        Err(e) if e.downcast_ref::<data::MirrorFeatureUnavailable>().is_some() => {
                            RoomVm::Unavailable(e.to_string())
                        }
                        Err(e) => {
                            let text = logs::error_chain(&e);
                            let key = format!("房间归属查询失败|{text}");
                            if self.mirror_error_said.as_deref() != Some(key.as_str()) {
                                let el = self.tree.element(refno);
                                self.logs
                                    .error_of(&mut self.vm.logs, el, "房间归属查询失败", &e, None);
                                self.mirror_error_said = Some(key);
                            }
                            RoomVm::Failed(format!("房间归属查询失败：{text}"))
                        }
                    };
                }
                data::Evt::ElementRooms(..) => {}
                // 尺寸标注回包（计划 B1 / B2 / B3）。帧号对不上 = 右键之后又换了目标或点了
                // 隐藏，丢弃。对上了就落进 `vm.dimensions`（菜单据它把「查看」翻成「隐藏」，
                // B4 的状态入口据它报数），几何翻成线段批次交给视口整层替换。
                data::Evt::PipeDimensions {
                    epoch,
                    refno,
                    result,
                } if epoch == self.dimensions_epoch => {
                    let el = self.tree.element(refno);
                    self.vm.dimensions = match result {
                        Ok(data) => {
                            let mapped = dimension_layer::batch_of(&data);
                            let msg = format!(
                                "尺寸标注：BRAN {}，{} 个图元 → {} 条线 + {} 条文字，{} 条提示，布局 {}",
                                data.branch_refno,
                                data.primitives.len(),
                                mapped.batch.lines.len(),
                                mapped.batch.labels.len(),
                                data.issues.len(),
                                data.meta.layout_mode.as_deref().unwrap_or("-")
                            );
                            self.logs.info_of(&mut self.vm.logs, el.clone(), msg);
                            if mapped.skipped > 0 {
                                // 画面上少了东西必须说出来（fail-closed 只是不画半截，不是装没事）。
                                self.logs.warn_of(
                                    &mut self.vm.logs,
                                    el,
                                    format!("{} 个图元几何不合法，未画", mapped.skipped),
                                );
                            }
                            self.pending_dimensions = Some(DimensionLayerUpdate::Set(mapped.batch));
                            DimensionsVm::Ready(DimensionsDataVm {
                                refno,
                                primitives: data.primitives.len(),
                                issues: data.issues.len(),
                                layout_mode: data.meta.layout_mode.clone(),
                            })
                        }
                        Err(error) => {
                            let message = error.to_string();
                            self.logs.error_of(
                                &mut self.vm.logs,
                                el,
                                "尺寸标注取数失败",
                                &anyhow::Error::new(error),
                                None,
                            );
                            DimensionsVm::Failed { refno, message }
                        }
                    };
                }
                data::Evt::PipeDimensions { .. } => {}
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
                        // 与房间归属同一条去刷屏规矩：镜像断着的时候，选一块面板就再失败
                        // 一次，每次一行红底 ERROR 只是同一句话的复读。
                        let text = logs::error_chain(&e);
                        let key = format!("PANEL 房间查询失败|{text}");
                        if self.mirror_error_said.as_deref() != Some(key.as_str()) {
                            let el = self.tree.element(refno);
                            self.logs.error_of(
                                &mut self.vm.logs,
                                el,
                                "PANEL 房间查询失败",
                                &e,
                                None,
                            );
                            self.mirror_error_said = Some(key);
                        }
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
                            self.logs
                                .error(&mut self.vm.logs, "房间 PANEL 查询失败", &e, None);
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
                    // 取景账只在真进了视图时才由房间体落地来销；下面四种没进成的情况
                    // （查不到 / 查询失败 / 没有实时渲染器 / 空房壳）留着它就是个死账，
                    // 下次房间体落地会莫名其妙动相机。帧号是「进视图」的唯一凭据：
                    // 空房壳在 `enter_room_view` 里提前返回，帧号根本没推进。
                    let epoch_before = self.room_view_epoch;
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
                            let live = self.vm.view3d.as_ref().is_some_and(|v| v.live);
                            if for_viewport && live {
                                // 隔离、取房间体、挂 HUD（直段截断计划 §4.1）。取景等房间体回来再做。
                                self.enter_room_view(room, detail);
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
                            self.logs
                                .error(&mut self.vm.logs, "房间详情查询失败", &e, None);
                        }
                    }
                    // 帧号没变 = 这一趟没进成视图（含空房壳、没有实时渲染器）：
                    // 取景账销掉，免得挂到下一次房间体落地上去。
                    if for_viewport && self.room_view_epoch == epoch_before {
                        self.focus_room_frame.take_if(|pending| *pending == room);
                    }
                }
                data::Evt::RoomDetail(..) => {}
                data::Evt::RoomVolume {
                    epoch,
                    room,
                    result,
                } => self.settle_room_volume(epoch, room, result),
                // 房间树（plant3d-web「房间」页签移植）：根清单回包。帧号对不上 = 刷新 / 重连之后的旧回包，丢。
                data::Evt::SpatialRooms { epoch, result } if epoch == self.room_tree_epoch => {
                    match result {
                        Ok(response) => self.room_tree.set_roots(response),
                        // 老服务端没有这条路由：与服务端自己说 `unsupported` 是同一句注释，不当故障。
                        Err(spatial_api::SpatialError::Unsupported(_)) => self
                            .room_tree
                            .set_unavailable(rooms_status_fallback("unsupported")),
                        Err(error) => {
                            let reason = error.to_string();
                            self.logs.error(
                                &mut self.vm.logs,
                                "房间树：在册房间清单取数失败",
                                &anyhow::Error::new(error),
                                None,
                            );
                            self.room_tree.fail_roots(reason);
                        }
                    }
                    if let Some(room) = self.room_tree_reveal_room.take() {
                        if matches!(self.room_tree.status, RootsStatus::Ready { .. }) {
                            self.reveal_room_in_tree(room);
                        } else {
                            self.logs.warn(
                                &mut self.vm.logs,
                                "房间树：在册房间清单没取到，「在房间树查看」作废",
                            );
                        }
                    }
                    self.rebuild_room_tree();
                }
                data::Evt::SpatialRooms { .. } => {}
                // 一间房的树 / 一组叶子回包。哪个节点在等由 `room` / `only` 找回来；失败挂在那个节点上
                // 列在树上方，等着它的动作随即结算（到齐了就执行，没到齐就作废）。
                data::Evt::SpatialRoomTree {
                    epoch,
                    room,
                    only,
                    result,
                } if epoch == self.room_tree_epoch => {
                    // 同一个 noun 在两个专业的「其他构件」下都未内联时，一条 `other_noun=` 回包对应两个节点
                    //（合并时两组都填），在途集里有几个就收几个；没人在等的是过期回包，丢。
                    let ids = self.room_tree.loading_nodes_for(room, only.as_ref());
                    if !ids.is_empty() {
                        for id in &ids {
                            self.room_tree.loading.remove(id);
                        }
                        let outcome = match result {
                            Ok(tree) if only.is_none() => self.room_tree.set_tree(room, tree),
                            Ok(tree) => self.room_tree.merge_leaves(room, tree),
                            Err(error) => Err(error.to_string()),
                        };
                        if let Err(message) = outcome {
                            for id in ids {
                                let label = self
                                    .room_tree
                                    .node(&id)
                                    .map(|node| node.label.clone())
                                    .unwrap_or_else(|| room.to_slash_string());
                                self.logs.warn(
                                    &mut self.vm.logs,
                                    format!("房间树：{label} 取数失败——{message}"),
                                );
                                self.room_tree.set_error(id, message.clone());
                            }
                        }
                    }
                    self.settle_room_tree_deferred();
                    self.rebuild_room_tree();
                }
                data::Evt::SpatialRoomTree { .. } => {}
                data::Evt::RoomsOverview(result) => {
                    self.rooms_overview = match result {
                        Ok(rows) => {
                            RoomBrowserVm::Ready(rows.into_iter().map(build_overview_row).collect())
                        }
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
                        Ok(loaded) => {
                            // 哪几个库此刻由 API 从内存供数（spec §4.12）——跟在计数后面说一句，
                            // 常态（全部 rocksdb）不出声。
                            let memory_note = loaded.memory_note().unwrap_or_default();
                            let models = loaded.records;
                            self.set_get_work_busy(false);
                            self.run_deferred_get_work();
                            let mesh_count =
                                models.iter().map(|model| model.insts.len()).sum::<usize>();
                            self.refresh_generation_pending |= debt_reload;
                            self.pending_incremental_models.clear();
                            self.pending_refreshes.clear();
                            self.model_show_waiting.clear();
                            self.latest_mesh_progress = None;
                            self.model_scope_failures = 0;
                            self.model_scopes.clear();
                            self.tree.visibility_unavailable.clear();
                            self.loaded_models.clear();
                            self.model_owner.clear();
                            for model in &models {
                                let refno = model.refno.refno();
                                self.loaded_models.insert(refno);
                                cache_model_scope(&mut self.model_scopes, refno, refno);
                                remember_model_owner(&mut self.model_owner, model);
                            }
                            // 清场重装：旧的实际状态全作废，新的等 View3d 装完
                            // 网格再回执上来。这中间 eye 停在未加载而不是抢先
                            // 说「已显示」。
                            self.tree.visibility.clear();
                            self.tree.pending_direction.clear();
                            self.tube_visibility.clear();
                            self.tube_pending.clear();
                            // 范围目标名单随快照放回来：重装后它们仍是「点过眼睛的
                            // 范围」，下一次取回工作还要以它们为祖先重查（ADR-0030 D3）。
                            if let Some(snapshot) = self.model_reload_restore.as_ref() {
                                self.scope_targets.extend(snapshot.targets.iter().copied());
                            }
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
                                    "三维模型已就绪：{} 个元素，{} 个网格实例{replay}{memory_note}",
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
                            // 还在房间视图里：隔离随整场重装冲掉了，房间体也可能刚重算过。
                            self.reapply_room_view_after_reload();
                            dirty = true;
                        }
                        Err(error) => {
                            self.set_get_work_busy(false);
                            self.run_deferred_get_work();
                            self.logs.error(
                                &mut self.vm.logs,
                                "三维模型查询失败：场景已清空；错误已记录，不会自动重试，可再点「取回工作」",
                                &error,
                                None,
                            );
                        }
                    }
                }
                data::Evt::ReloadProgress { done, total } => {
                    self.vm.model_load = Some(if total == 0 {
                        ModelLoadVm::Resolving("正在重查模型…".into())
                    } else {
                        ModelLoadVm::Loading {
                            label: "重查模型".into(),
                            done,
                            total,
                        }
                    });
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
                    // 老服务端（ADR-086 之前）对没生成过的范围回 409 `not_generated`：与新服务端的
                    // 200 空是同一件事，按查空处理、不记 ERROR（ADR-0030 后果 6）。
                    let result = match result {
                        Err(error) if scope_not_generated(&error) => {
                            Ok(model_update_api::ModelRecords::default())
                        }
                        other => other,
                    };
                    match result {
                        Ok(loaded) if loaded.records.is_empty() => {
                            // 查空 → 默认重新生成（只限显示方向、每 refno 每会话一发；
                            // 发不出去 / 已经试过的照旧收「未加载」，见 `mark_model_scope_unavailable`）。
                            let wants_show =
                                self.tree.pending_direction.get(&target).copied() == Some(true);
                            let auto = wants_show
                                && !self.auto_generate_attempted.contains(&target)
                                && !self
                                    .begin_auto_generate(
                                        vec![target],
                                        "范围内没有已生成的模型".into(),
                                    )
                                    .is_empty();
                            // 这一发已由库级探针抢先发起、还在在途：安静地等回执落定，
                            // 别标「未加载」、别打「右键重新生成」的提示——生成正在跑。
                            let in_flight = self
                                .mutation_run
                                .as_ref()
                                .is_some_and(|run| run.targets.contains(&target));
                            if auto || in_flight {
                                if auto {
                                    let element = self.tree.element(target);
                                    self.logs.info_of(
                                        &mut self.vm.logs,
                                        element,
                                        "范围内没有已生成的模型，正在默认重新生成（回执落定自动显示）"
                                            .to_string(),
                                    );
                                }
                                self.finish_model_progress("模型加载完成");
                                self.pending_room_frame.take_if(|room| *room == target);
                            } else {
                                mark_model_scope_unavailable(
                                    &mut self.tree.pending_direction,
                                    &mut self.tree.visibility_unavailable,
                                    &mut self.model_scope_failures,
                                    target,
                                );
                                // 重新生成之后查空（范围内已无可画几何）：这个目标上一次显示的旧产物
                                // 也不该再留在场景里。
                                if self.regenerate_refresh.remove(&target) {
                                    let stale: Vec<RefU64> = self
                                        .model_scopes
                                        .remove(&target)
                                        .unwrap_or_default()
                                        .into_iter()
                                        .filter(|refno| self.loaded_models.remove(refno))
                                        .collect();
                                    for refno in &stale {
                                        self.model_owner.remove(refno);
                                    }
                                    if !stale.is_empty() {
                                        self.view3d_commands.push(Cmd::Model(
                                            plant_ui::ModelAction::Unload { refnos: stale },
                                        ));
                                    }
                                }
                                // 查空就是空：树行停「未加载」，一句提示说清出路，不是告警
                                // （ADR-0030 D1）。
                                let element = self.tree.element(target);
                                self.logs
                                    .info_of(&mut self.vm.logs, element, EMPTY_SCOPE_HINT);
                                self.finish_model_progress("模型加载完成");
                                // 这间房没有已生成模型，取景的账就地销掉——留着它，
                                // 下一个范围回包会替它把相机带走。
                                self.pending_room_frame.take_if(|room| *room == target);
                            }
                            dirty = true;
                        }
                        Ok(loaded) => {
                            let memory_note = loaded.memory_note().unwrap_or_default();
                            let models = loaded.records;
                            self.tree.visibility_unavailable.remove(&target);
                            // 真查出记录 = 这个库有模型：库级结论正写，探针晚到的空结论压不掉它。
                            if let Some(dbnum) = self.tree.dbnum_of(target) {
                                self.dbnum_model_known.insert(dbnum, true);
                            }
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
                                remember_model_owner(&mut self.model_owner, model);
                            }
                            // 右键重新生成之后的显示（ADR-0030 D2 第 3 步）：这批记录是重算过的
                            // 产物，场景里同名的旧几何要换掉。先从已装账上摘下来，下面才会把它们
                            // 当新模型认领；卸旧装新交给 `View3d::refresh` 在同一批里做。
                            let stale: Vec<RefU64> = if self.regenerate_refresh.remove(&target) {
                                refs.iter()
                                    .copied()
                                    .filter(|refno| self.loaded_models.remove(refno))
                                    .collect()
                            } else {
                                Vec::new()
                            };
                            self.model_scopes.insert(target, refs.clone());
                            // 登记成范围目标：取回工作重装时服务供数以它为祖先重查（ADR-0030 D3）。
                            self.scope_targets.insert(target);
                            let element = self.tree.element(target);
                            self.logs.info_of(
                                &mut self.vm.logs,
                                element,
                                format!(
                                    "查询到 {} 个元素、{} 个网格实例{memory_note}",
                                    refs.len(),
                                    mesh_count
                                ),
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
                                if stale.is_empty() {
                                    self.pending_incremental_models.push(new_models);
                                } else {
                                    self.pending_refreshes.push((stale, new_models));
                                }
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
                                        || !self.pending_refreshes.is_empty()
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
                data::Evt::SubtreeBounds {
                    epoch,
                    target,
                    result,
                } if self.focus_bounds_request.accepts_bounds(epoch, target) => {
                    match result {
                        Ok(bounds) => {
                            self.focus_bounds_request.cancel();
                            // 相机这一动没有别的回声：说一句落到哪儿、框了多少个模型，
                            // 用户才知道「定位成功」与「框错了范围」的区别。
                            let element = self.tree.element(target);
                            self.logs.info_of(
                                &mut self.vm.logs,
                                element,
                                format!(
                                    "节点 {target}：已定位到 {} 个模型的范围",
                                    bounds.model_count
                                ),
                            );
                            self.view3d_commands.push(Cmd::Model(
                                plant_ui::ModelAction::FocusBounds {
                                    min_mm: bounds.min_mm,
                                    max_mm: bounds.max_mm,
                                },
                            ));
                        }
                        // 范围内没有已生成的模型：相机不动，一句提示说清出路。聚焦是显示
                        // 入口，不再为它悄悄生成一次（ADR-0030 D1 / 后果 4）。
                        Err(error) if read_face::is_no_renderable_geometry(&error) => {
                            self.focus_bounds_request.cancel();
                            let element = self.tree.element(target);
                            self.logs.info_of(
                                &mut self.vm.logs,
                                element,
                                format!("节点 {target}：{EMPTY_SCOPE_HINT}；相机保持不变"),
                            );
                        }
                        Err(error) => {
                            self.focus_bounds_request.cancel();
                            self.logs.error(
                                &mut self.vm.logs,
                                format!("节点 {} 范围定位失败，相机保持不变", target),
                                &error,
                                None,
                            );
                        }
                    }
                    dirty = true;
                }
                data::Evt::SubtreeBounds { .. } => {}
                data::Evt::ModelScopeProgress { .. } | data::Evt::ModelScope(..) => {}
                // 库级模型预检查的回包：换代前的不认。`ready_total == 0` = 这个库没有按
                // 当前源生成的模型——还在等 records 回包的同库目标直接默认重新生成，不必
                // 先吃一拍查空；`ready_total > 0` / `Err` 都只记账，records 那一路照常裁决。
                data::Evt::DbnumModelProbe { epoch, .. } if epoch != self.model_scope_epoch => {}
                data::Evt::DbnumModelProbe { dbnum, result, .. } => {
                    self.dbnum_model_probing.remove(&dbnum);
                    if let Ok(roots) = result {
                        let has_model = roots.ready_total > 0;
                        match self.dbnum_model_known.entry(dbnum) {
                            std::collections::hash_map::Entry::Occupied(mut entry) => {
                                if has_model {
                                    entry.insert(true);
                                }
                            }
                            std::collections::hash_map::Entry::Vacant(entry) => {
                                entry.insert(has_model);
                            }
                        }
                        if !has_model {
                            // 只替「显示」方向的待决目标发起：隐藏方向的查空不生成。
                            let pending: Vec<RefU64> = self
                                .model_scope_pending
                                .iter()
                                .copied()
                                .filter(|target| {
                                    self.tree.dbnum_of(*target) == Some(dbnum)
                                        && self.tree.pending_direction.get(target).copied()
                                            == Some(true)
                                })
                                .collect();
                            self.begin_auto_generate(
                                pending,
                                format!("db{dbnum} 还没有已生成的模型"),
                            );
                            // 没发出去的留在在途：records 回空那一臂照常收口。
                        }
                    }
                    dirty = true;
                }
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
                // 搜索框每敲一下发一条，回来时输入多半已经变过：只认最后发出的那一条。
                data::Evt::SearchElements { epoch, .. } if epoch != self.search_epoch => {}
                data::Evt::SearchElements {
                    query,
                    prefix,
                    substring,
                    ..
                } => {
                    let dbs = self.desi_dbs.clone();
                    let mut sub_hits: Vec<SearchHitVm> = match substring {
                        search_index::SubstringHits::Hits(hits) => {
                            hits.into_iter().map(|hit| hit_row(hit, &dbs)).collect()
                        }
                        // 「还没就绪」与「不提供」都不是空结果，界面靠 sub_state
                        // 那一行说清楚，这里只是没有行可列。
                        search_index::SubstringHits::Building
                        | search_index::SubstringHits::Unavailable => Vec::new(),
                    };
                    let sub_full = sub_hits.len() >= data::SEARCH_LIMIT;
                    let prefix = prefix.map(|hits| {
                        hits.into_iter()
                            .map(|hit| hit_row(hit, &dbs))
                            .collect::<Vec<SearchHitVm>>()
                    });
                    // 命中的库号整份换掉：属性面板判空表要它，而下拉那时候多半已经关了。
                    self.search_hit_dbs = prefix
                        .iter()
                        .flatten()
                        .chain(&sub_hits)
                        .map(|hit| (hit.refno, hit.dbnum))
                        .collect();
                    let search = &mut self.vm.search;
                    search.running = None;
                    search.query = query;
                    match prefix {
                        Ok(mut hits) => {
                            let prefix_full = hits.len() >= data::SEARCH_LIMIT;
                            // 配额（ADR-0023 决定 7）：总行数照旧 ≤ SEARCH_LIMIT；
                            // 子串有命中时前缀最多占 PREFIX_QUOTA，余下留给子串。
                            // 不这么切的话前缀一满，子串那一段永远露不出来。
                            if !sub_hits.is_empty() {
                                hits.truncate(PREFIX_QUOTA);
                            }
                            sub_hits.truncate(data::SEARCH_LIMIT - hits.len());
                            search.truncated = prefix_full || sub_full;
                            search.error = None;
                            search.hits = hits;
                        }
                        Err(error) => {
                            // 前缀那一路断了不牵连子串：索引是本地的，库不在也照查。
                            search.hits.clear();
                            search.truncated = sub_full;
                            search.error = Some(format!("搜索失败：{}", logs::error_chain(&error)));
                        }
                    }
                    search.sub_hits = sub_hits;
                    dirty = true;
                }
                data::Evt::SearchIndex(state) => {
                    use search_index::SearchIndexState as State;
                    let next = match &state {
                        State::Ready(_) => SubIndexVm::Ready,
                        State::Building { done, total } => SubIndexVm::Building {
                            done: *done,
                            total: *total,
                        },
                        State::Failed(reason) => SubIndexVm::Failed(reason.clone()),
                        State::Off => SubIndexVm::Off,
                    };
                    // 日志只记状态**变化**：重建期间每拉完一个库就来一条进度，
                    // 全记下来会把面板刷满，而它们在下拉里已经逐格看得见了。
                    let current = &self.vm.search.sub_state;
                    match &state {
                        State::Ready(names) if !matches!(current, SubIndexVm::Ready) => {
                            let msg = format!("子串索引已就绪：{names} 个名字");
                            self.logs.info(&mut self.vm.logs, msg);
                        }
                        State::Building { total, .. }
                            if !matches!(current, SubIndexVm::Building { .. }) =>
                        {
                            let msg = format!("子串索引开始后台重建（{total} 个设计库）");
                            self.logs.info(&mut self.vm.logs, msg);
                        }
                        State::Failed(reason) if *current != next => {
                            let msg =
                                format!("子串索引不可用：{reason}（命令行输入 reindex 重建）");
                            self.logs.warn(&mut self.vm.logs, msg);
                        }
                        _ => {}
                    }
                    self.vm.search.sub_state = next;
                    dirty = true;
                }
                // generate 的整批回执拿确认那一刻钉住的帧号认；对不上的一律不认
                // ——这一趟已经收了，回来的是上一趟的尾巴。
                data::Evt::ModelsGenerated { epoch, .. } if epoch != self.mutation_epoch => {}
                data::Evt::ModelsGenerated { outcomes, .. } => {
                    self.settle_models_generated(outcomes);
                    dirty = true;
                }
                // 「等房间参照模型」进度：只认本帧——就绪回 Running（那一发随之发出），
                // 建中进 WaitingResident。「重新生成房间」那趟就绪后不换挡：紧随的
                // `Evt::RoomsRebuilt` 会替它收摊。
                data::Evt::ResidentWait { epoch, .. } if epoch != self.mutation_epoch => {}
                data::Evt::ResidentWait { status, .. } => match status {
                    model_update_api::ResidentStatus::Building { phase, progress } => {
                        // 只在「在跑 / 在等」那一趟上换档；还停在确认框时不越位。
                        let waiting = matches!(
                            self.mutation,
                            Some(model_mutation::Vm::Running { .. })
                                | Some(model_mutation::Vm::WaitingResident { .. })
                        );
                        if waiting && let Some(run) = self.mutation_run.as_ref() {
                            let files = progress
                                .as_ref()
                                .filter(|p| p.files_total > 0)
                                .map(|p| (p.files_done, p.files_total));
                            let meshes = progress
                                .as_ref()
                                .filter(|p| p.meshes_total > 0)
                                .map(|p| (p.meshes_done, p.meshes_total, p.meshes_failed));
                            let dbnums = progress
                                .as_ref()
                                .filter(|p| p.dbnums_total > 0)
                                .map(|p| (p.dbnums_done, p.dbnums_total));
                            self.mutation = Some(model_mutation::Vm::WaitingResident {
                                kind: run.kind,
                                label: run.label.clone(),
                                phase: phase.unwrap_or_else(|| "装载中".into()),
                                files,
                                meshes,
                                dbnums,
                            });
                            dirty = true;
                        }
                    }
                    model_update_api::ResidentStatus::Ready => {
                        // 房间重建的「就绪」只是换代装上——逐库重算的回执还没来，
                        // 留在等待档不动：`RoomsRebuilt` 紧跟其后替它收摊。
                        let rebuild_pending = matches!(
                            self.mutation_run.as_ref(),
                            Some(run) if run.kind == model_mutation::Kind::RoomRebuild
                        );
                        if !rebuild_pending
                            && let Some(model_mutation::Vm::WaitingResident {
                                kind, label, ..
                            }) = self.mutation.take()
                        {
                            self.mutation =
                                Some(model_mutation::Vm::Running { kind, label });
                            dirty = true;
                        }
                    }
                    _ => {}
                },
                // 「重新生成房间」的收摊回执：跑完关屏记日志；失败进 Failed 档。
                data::Evt::RoomsRebuilt { epoch, .. } if epoch != self.mutation_epoch => {}
                data::Evt::RoomsRebuilt { result, .. } => {
                    self.vm.regen_busy = false;
                    let label = self
                        .mutation_run
                        .as_ref()
                        .map(|run| run.label.clone())
                        .unwrap_or_else(|| "全部库".to_string());
                    match result {
                        Ok(()) => {
                            self.mutation = None;
                            self.mutation_run = None;
                            self.logs.info(
                                &mut self.vm.logs,
                                "重新生成房间完成：房间参照模型已重建，各库房间关系已重算"
                                    .to_string(),
                            );
                        }
                        Err(error) => {
                            self.mutation = Some(model_mutation::Vm::Failed {
                                kind: model_mutation::Kind::RoomRebuild,
                                label,
                                reason: format!("{error:#}"),
                            });
                        }
                    }
                    dirty = true;
                }
                data::Evt::ModelsCleaned { epoch, .. } if epoch != self.mutation_epoch => {}
                data::Evt::ModelsCleaned { outcomes, .. } => {
                    self.settle_models_cleaned(outcomes);
                    dirty = true;
                }
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
                // 虚拟孔洞映射确认（ADR-0031 阶段一）：帧号对不上是上一份导入的尾巴，丢掉。
                data::Evt::VirtualHoleTargets { epoch, results } => {
                    if self.hole_session.apply_targets(epoch, &results) {
                        let vm = self.hole_session.vm();
                        let mapped = vm
                            .holes
                            .iter()
                            .filter(|row| {
                                matches!(row.target, plant_ui::vm::VirtualHoleTargetVm::Mapped(_))
                            })
                            .count();
                        let missing = vm
                            .holes
                            .iter()
                            .filter(|row| {
                                matches!(row.target, plant_ui::vm::VirtualHoleTargetVm::Missing)
                            })
                            .count();
                        self.sync_hole_vm();
                        let mut message = format!("虚拟孔洞映射确认：{mapped} 条已映射");
                        if missing > 0 {
                            message.push_str(&format!("，{missing} 条的目标在库里不存在"));
                        }
                        self.logs.info(&mut self.vm.logs, message);
                    }
                }
                // 预览是长请求，用户可能中途放弃等待（Vm 回到 Idle）。晚到的结果
                // 不许把界面拽回来——那一下会覆盖掉用户刚做出的选择。
                data::Evt::ModelUpdatePreview(_)
                    if !matches!(self.model_update, ModelUpdateVm::Loading) => {}
                data::Evt::ModelUpdatePreview(result) => match result {
                    Ok(preview) => {
                        // 读透形态回的是空表 + up_to_date——那不是「已是最新」，日志不许这么说。
                        let line = if preview.read_through() {
                            "模型更新预览：模型服务以读透形态运行，不摄入数据、水位不动；\
                             可「复核模型与缓存」让模型追到文件最新"
                                .to_owned()
                        } else {
                            // `dbnums.len()` 是预览表的行数，里面还坐着非 DESI、MDB 声明
                            // 了却没文件、以及阻断的库——拿它当「N 个设计库」报出来，日志
                            // 说的数就永远大于真会跑的那一批。执行范围只由 `will_run()` 定。
                            let totals = preview.totals();
                            let mut line = format!(
                                "模型更新预览完成：{} 个库会执行（预览表 {} 行）",
                                totals.batches,
                                preview.dbnums.len()
                            );
                            for (count, what) in [
                                (totals.blocked, "阻断"),
                                (totals.not_in_project, "MDB 声明了但项目目录里没有文件"),
                                (totals.excluded, "非 DESI 不在范围"),
                                (preview.shadowed.len(), "跨项目同号文件被遮蔽未读"),
                            ] {
                                if count > 0 {
                                    line.push_str(&format!("；{count} 个{what}"));
                                }
                            }
                            line
                        };
                        self.logs.info(&mut self.vm.logs, line);
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
                // 「重算模型（整库）」的回执：一库一个 task_id。成功只进日志、催一拍轮询让那一行
                // 尽快出现；失败按 code 分型进日志（`not_found` = 这一档服务端没有这条路由，D9）。
                data::Evt::ModelRebuild { dbnum, result } => match result {
                    Ok(receipt) => {
                        self.logs.info(&mut self.vm.logs, receipt.summary());
                        self.poll_queue_now();
                    }
                    Err(error) => {
                        let title = if model_update_api::endpoint_missing(&error) {
                            format!(
                                "重算模型（整库）db{dbnum} 未受理：这一档服务端没有整库重建接口"
                            )
                        } else {
                            format!("重算模型（整库）db{dbnum} 提交失败")
                        };
                        self.logs.error(&mut self.vm.logs, &title, &error, None);
                    }
                },
                data::Evt::DbnumModelCleaned { epoch, .. } if epoch != self.mutation_epoch => {}
                data::Evt::DbnumModelCleaned { dbnum, result, .. } => {
                    self.settle_dbnum_model_cleaned(dbnum, result);
                }
                data::Evt::GetWork(result) => {
                    match result {
                        Ok(fresh) => {
                            let refreshed = std::mem::take(&mut self.get_work_task_ids);
                            if fresh.failed.is_empty() {
                                for task_id in refreshed {
                                    self.queue.mark_refreshed(&task_id);
                                }
                            }
                            let before = self.tree.element_count();
                            for site in &fresh.sites {
                                self.tree.note_dbnum(site);
                            }
                            self.tree.roots = fresh.sites;
                            for (refno, kids) in fresh.branches {
                                for kid in &kids {
                                    self.tree.parent.insert(kid.refno.refno(), refno);
                                    self.tree.note_dbnum(kid);
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
                            self.tree.detach_missing(&fresh.missing);
                            let removed = self.tree.prune_unreachable();
                            if !removed.is_empty() {
                                // Only remove nodes the refreshed tree proved unreachable.
                                // Keep unrelated/outside-tree selections, and invalidate the
                                // deleted primary before refetching properties or rooms below.
                                self.set_selection(selection_after_removed_nodes(
                                    &self.vm.selection,
                                    &removed,
                                ));
                                self.view3d_commands.push(Cmd::Model(
                                    plant_ui::ModelAction::Unload {
                                        refnos: removed.clone(),
                                    },
                                ));
                                self.logs.info(
                                    &mut self.vm.logs,
                                    format!("已卸载 {} 个从资料树消失的模型节点", removed.len()),
                                );
                            }
                            // Prune the preserved request/replay snapshot before sending
                            // Models, not only the scene. Otherwise deleted scopes are
                            // re-queried now and reintroduced by Evt::Models every refresh.
                            if let Some(snapshot) = self.model_reload_restore.as_mut() {
                                prune_removed_reload_nodes(snapshot, &removed);
                            }
                            self.scope_targets.retain(|refno| !removed.contains(refno));
                            if fresh.reload_models {
                                // 重查范围 = 清场快照里的那批模型（显示 + 已隐藏），
                                // 不再是全部 SITE 根：取回工作刷的是「已加载的三维
                                // 模型」（CONTEXT.md），叶子 refno 当根走的是同一条
                                // anc 索引查询（决定 6）。
                                let snapshot =
                                    self.model_reload_restore.clone().unwrap_or_default();
                                let roots: Vec<RefU64> =
                                    snapshot.models.iter().map(|(refno, _)| *refno).collect();
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
                                    .send(data::Req::Models {
                                        roots,
                                        // 服务供数以清场前点过眼睛的那些目标为祖先重查
                                        // （ADR-0030 D3）：一个 ZONE 一次往返，不逐个模型问；
                                        // 只查不生。
                                        scope_targets: snapshot.targets,
                                        debt_reload: true,
                                        base: model_update_api::base_url(),
                                        project: self.vm.project.clone(),
                                        mdb: self.mdb.clone(),
                                        namespace: self.namespace.clone(),
                                    })
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
                            // 清扫可以把待滚动路径上的某一节摘掉（元素被删或挪了
                            // OWNER）。那条路径已经通不到目标，待滚动跟着结束。
                            self.drop_locate_off_the_tree();
                            let after = self.tree.element_count();
                            // 只换了树的那次必须自己说出来：三维还是旧几何，
                            // 而画面上没有任何东西会替它认这件事。
                            let scene = if fresh.reload_models {
                                ""
                            } else {
                                "；三维保持原样，要换新点菜单「取回工作」"
                            };
                            self.logs.info(
                                &mut self.vm.logs,
                                format!("取回工作完成：已加载元素 {before} → {after}{scene}"),
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
                            let source_changed = self.source_versions.observe(&poll.dbnums);
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
                            // Source saves in read-through mode have no task terminal event.
                            // Remember reload demand until the model barrier settles; the
                            // existing loaded-scope reload preserves visibility and camera.
                            self.model_reload_owed |=
                                source_changed || data_applied || !fresh.is_empty();
                            let plan =
                                auto_refresh(self.model_reload_owed, models_settled, data_applied);
                            if !models_settled {
                                fresh.clear();
                            }
                            self.queue.project = self.vm.project.clone();
                            self.queue.mdb = self.mdb.clone();
                            self.queue.namespace = self.namespace.clone();
                            self.queue.adopt(poll);
                            // 库供数下两边接的不是同一个项目要说出来（计划 D12 后半）：
                            // 队列面板那条横幅只在队列页看得见，撞上的人多半正对着树。
                            // 一次撞上说一次——轮询忙时 1 秒一拍，每拍都说就是刷屏。
                            match self.queue.identity_mismatch_line() {
                                Some(line) if !self.identity_mismatch_said => {
                                    self.identity_mismatch_said = true;
                                    self.command_error(line.clone());
                                    self.logs.warn(&mut self.vm.logs, line);
                                }
                                Some(_) => {}
                                None => self.identity_mismatch_said = false,
                            }
                            // 取回工作旁那行提示从这份 `/dbnums` 算：队列轮询常驻
                            // （忙 1 s / 闲 5 s），提示随每拍刷新，不再单独打一次水位表。
                            self.vm.watermark_lag = Some(self.queue.watermark_lag());
                            match plan {
                                AutoRefresh::FullReload => {
                                    self.get_work_with_models_for_tasks(true, task_ids)
                                }
                                AutoRefresh::TreeOnly => {
                                    self.get_work_with_models_for_tasks(false, task_ids)
                                }
                                AutoRefresh::Units => self.refresh_for_units(fresh, task_ids),
                            }
                        }
                        // 上一份快照留在界面上，横幅把「这份是旧的」说出来。
                        Err(error) => {
                            self.queue.error = Some(logs::error_chain(&error));
                        }
                    }
                    self.vm.queue = task_queue::status(&self.queue);
                }
                data::Evt::TaskChanges(task_id, result) => {
                    let state = match result {
                        Ok(changes) => task_queue::ChangesState::Loaded(changes),
                        Err(error) => {
                            let text = logs::error_chain(&error);
                            self.logs
                                .error(&mut self.vm.logs, "取变化清单失败", &error, None);
                            task_queue::ChangesState::Failed(text)
                        }
                    };
                    self.queue.changes.insert(task_id, state);
                }
                data::Evt::OlderTasks(result) => match result {
                    Ok(page) => self.queue.adopt_older(page),
                    Err(error) => {
                        self.queue.archive_loading = false;
                        self.logs
                            .error(&mut self.vm.logs, "加载更早的增量记录失败", &error, None);
                    }
                },
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
                data::Evt::DataPublish(request, result) => {
                    self.data_publish_state.submitting = false;
                    self.data_publish_state.delete_confirmation_open = false;
                    self.data_publish_state.pending_request = None;
                    self.data_publish_state.deletion_candidates.clear();
                    self.handle_publish_result(
                        ctx,
                        request.category.label(),
                        request.category.endpoint(),
                        result,
                    );
                }
                data::Evt::DataPublishDeletions(result) => {
                    self.data_publish_state.submitting = false;
                    match result {
                        Ok(candidates) => {
                            self.data_publish_state.deletion_candidates = candidates;
                            self.data_publish_state.delete_confirmation_open = true;
                        }
                        Err(error) => {
                            self.data_publish_state.pending_request = None;
                            let message =
                                format!("获取待删除提资失败：{}", logs::error_chain(&error));
                            eprintln!("[data_publish_delete_lookup] {message}");
                            self.logs.error(&mut self.vm.logs, message, &error, None);
                        }
                    }
                }
                data::Evt::RoomCodePublish(result) => {
                    self.manual_data_publish_state.submitting = false;
                    self.handle_publish_result(
                        ctx,
                        "手动提资接口",
                        "/send_room_code_to_data_center",
                        result,
                    );
                }
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
                // 翻面通告：只改库行那一格，场景不动（plan 2026-09-06 §8 R5）。手上那份
                // `/dbnums` 里还没有这个库（刚加进 MDB、或还没轮询到）就提前一拍去取快照，
                // 免得那一格空到下一拍。日志说一句：人等的多半就是这件事。
                data::Evt::ModelSourceChanged { dbnum, source } => {
                    if !self.queue.set_model_source(dbnum, source) {
                        self.poll_queue_now();
                    }
                    self.logs.info(
                        &mut self.vm.logs,
                        model_source_changed_line(dbnum, source, self.read_face),
                    );
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

    fn handle_publish_result(
        &mut self,
        ctx: &egui::Context,
        label: &str,
        endpoint: &str,
        result: anyhow::Result<data_publish_api::SubmitResult>,
    ) {
        match result {
            Ok(response) => {
                let mut message = format!("{label} 已提交（{endpoint}）：{}", response.message);
                if let Some(login_url) = response.login_url.filter(|url| !url.trim().is_empty()) {
                    message.push_str(&format!("；登录地址：{login_url}"));
                    ctx.open_url(egui::OpenUrl {
                        url: login_url,
                        new_tab: true,
                    });
                }
                self.logs.info(&mut self.vm.logs, message);
            }
            Err(error) => self.logs.error(
                &mut self.vm.logs,
                format!("{label}提交失败：{}", logs::error_chain(&error)),
                &error,
                None,
            ),
        }
        ctx.request_repaint();
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

    /// 范围定位的「正在查询」提示到期：只在这一拍仍在途时才说。
    ///
    /// 请求已回包（`cancel` 换了 epoch）、被新请求取代、或被别的动作作废时
    /// `accepts_bounds` 为假，这一句就不发了——400 ms 内就结束的查询只留结果那一条。
    /// （400 ms 之后才被作废的，那句「正在查询…」已经记下了，没有下文；这是可接受的：
    /// 定位是异步的，日志里留一条「查过」比什么都不说好。）
    fn tick_focus_scope(&mut self) {
        let Some((epoch, target, until)) = self.focus_scope_pending else {
            return;
        };
        // 已回包、已被取代、或已被别的动作作废：这一句就永远不发了。
        if !self.focus_bounds_request.accepts_bounds(epoch, target) {
            self.focus_scope_pending = None;
            return;
        }
        if Instant::now() < until {
            return;
        }
        self.focus_scope_pending = None;
        self.logs.info_of(
            &mut self.vm.logs,
            self.tree.element(target),
            format!("节点 {target}：正在查询模型范围…"),
        );
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

    /// 收下 View3d 的逐段回执（ADR-0033 阶段四）：按 BRAN 整份替换，落了地的在途方向退场；
    /// 还没落地的（回执是别的指令引起的）继续挂着，段已经不在场景里的也退场。
    fn sync_tube_render_states(&mut self, states: Vec<plant_ui_view3d::TubeRenderState>) {
        for state in states {
            let segments: HashMap<plant_ui::TubeSegment, bool> =
                state.segments.into_iter().collect();
            let settled_all = self.tube_pending.get_mut(&state.bran).map(|pending| {
                pending.retain(|segment, wanted| {
                    segments
                        .get(segment)
                        .is_some_and(|actual| actual != wanted)
                });
                pending.is_empty()
            });
            if settled_all == Some(true) {
                self.tube_pending.remove(&state.bran);
            }
            self.tube_visibility.insert(state.bran, segments);
        }
        self.rebuild_room_tree();
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
                Cmd::ImportVirtualHoles => self.open_hole_pick(),
                Cmd::SelectVirtualHole(id) => {
                    if id.is_empty() {
                        self.hole_session.select(None);
                    } else {
                        self.hole_session.select(Some(&id));
                    }
                    let selected = self.hole_session.selected().map(str::to_string);
                    self.pending_hole_ops.push(VirtualHoleOp::SelectHole(selected));
                    self.sync_hole_vm();
                }
                Cmd::MapVirtualHole { hole_id, refno } => {
                    if self.hole_session.map_manually(&hole_id, refno) {
                        // 映射换了目标，旧的布尔结果（连同被换过的网格）整个作废：
                        // 清预览层再按新目标来，不留一半新旧混着的画面。切削体摆回去：表还在。
                        self.clear_hole_preview();
                        self.restore_hole_layer();
                        self.logs.info(
                            &mut self.vm.logs,
                            format!("虚拟孔洞 {hole_id} 已手动映射到 {refno}，预览已清掉请重新发起"),
                        );
                    }
                }
                Cmd::PreviewVirtualHoles => {
                    let targets = self.hole_session.begin_preview();
                    if targets.is_empty() {
                        self.logs.info(
                            &mut self.vm.logs,
                            "虚拟孔洞：没有可预览的记录（需要 RelyItemREF 映射到库里的元素）",
                        );
                    } else {
                        let epoch = self.hole_session.epoch();
                        let target_count = targets.len();
                        for target in targets {
                            let holes = self
                                .hole_session
                                .holes_for_target(target)
                                .into_iter()
                                .cloned()
                                .collect();
                            self.pending_hole_ops.push(VirtualHoleOp::Preview {
                                epoch,
                                target,
                                holes,
                            });
                        }
                        self.hole_retry_pending.clear();
                        self.hole_target_retried.clear();
                        self.sync_hole_vm();
                        self.logs.info(
                            &mut self.vm.logs,
                            format!("虚拟孔洞预览已发起：{target_count} 个目标"),
                        );
                    }
                }
                Cmd::ClearVirtualHolePreview => {
                    if !self.hole_session.is_empty() {
                        self.clear_hole_preview();
                        self.logs
                            .info(&mut self.vm.logs, "虚拟孔洞预览已清掉，原始网格已恢复");
                    }
                }
                Cmd::FocusVirtualHole(id) => {
                    if self.hole_session.hole(&id).is_some() {
                        // 孔洞面板的「定位」也是显式取景（视口那条 FocusHole 会动相机）：
                        // 更早挂着的范围查询 / 房间取景 / 房间树取景就此作废，否则它们晚一拍
                        // 落地时会把相机从这一次上拽走。孔洞不存在就什么都不做。
                        self.supersede_camera_intents();
                        self.pending_hole_ops.push(VirtualHoleOp::FocusHole(id));
                    }
                }
                Cmd::FocusVirtualHoleTarget(id) => {
                    if let Some(target) = self.hole_session.target_of(&id) {
                        // 点孔洞就是一次取景意图：更早挂着的范围查询 / 房间取景都作废。放在最前，
                        // 未加载分支（等几何落地）也算数——不然范围回包先到时相机会先飞旧范围。
                        self.supersede_camera_intents();
                        // 先选中并展开树上那一行，再取景到它的包围盒。还没加载的先显示，等几何落地再取景
                        // （`settle_hole_waits`）——范围没回来前视口手上没有它的包围盒，现在发 `Focus` 相机不动。
                        self.handle_cmds(ctx, vec![Cmd::LocateElement(target)]);
                        // 显示无条件先做：`hole_target_verdict` 只看 `mesh_loaded`，不看 `visible`，
                        // 目标装过但被眼睛藏着时它是 `Loaded`——只发 `Focus` 会让相机飞向一个看不见的东西。
                        self.set_model_visible(vec![target], true);
                        let loaded = hole_target_verdict(
                            self.model_scope_pending.contains(&target),
                            self.tree.visibility_unavailable.contains(&target),
                            self.model_scopes.get(&target),
                            &self.tree.visibility,
                        ) == HoleTargetVerdict::Loaded;
                        if loaded {
                            self.view3d_commands
                                .push(Cmd::Model(plant_ui::ModelAction::Focus(target)));
                        } else {
                            self.hole_focus_pending = Some(target);
                        }
                    }
                }
                Cmd::ShowVirtualHoleOriginal(on) => {
                    self.hole_session.set_show_original(on);
                    self.pending_hole_ops.push(VirtualHoleOp::SetShowOriginal(on));
                    self.sync_hole_vm();
                }
                Cmd::SetVirtualHoleXRay(on) => {
                    // 与房间 X-Ray 抢同一份半透明材质：后拨的赢。
                    if on {
                        self.clear_room_xray();
                    }
                    self.hole_session.set_xray(on);
                    self.pending_hole_ops.push(VirtualHoleOp::SetXRay(on));
                    self.sync_hole_vm();
                }
                Cmd::SubmitCommand(command) => dirty |= self.submit_command(command),
                Cmd::SearchElements { query } => self.search_elements(query),
                Cmd::CloseSearch => {
                    // epoch 一进就把在途那次隔在门外：回来时框可能已经关了、
                    // 或者装着另一个词。
                    self.search_epoch = self.search_epoch.wrapping_add(1);
                    // 索引状态不跟着搜索框走：它是常驻的，关一次框不该让「正在
                    // 重建」的进度从头开始报。
                    let search = SearchVm {
                        scope_dbs: self.vm.search.scope_dbs,
                        sub_state: std::mem::take(&mut self.vm.search.sub_state),
                        // 命中范围跟着供数模式走，关一次框不该让它退回出厂默认。
                        coverage: self.read_face,
                        ..Default::default()
                    };
                    self.vm.search = search;
                }
                Cmd::OpenProjectPicker => self.project_picker_state.open = true,
                Cmd::LoadProject(_) => self.project_picker_state.open = false,
                Cmd::OpenSettings => self.settings_state.open(),
                Cmd::OpenDataPublish => self.data_publish_state.open = true,
                Cmd::OpenManualDataPublish => self.manual_data_publish_state.open = true,
                Cmd::SubmitDataPublish(request) => {
                    if self
                        .bridge
                        .req
                        .send(data::Req::DataPublish {
                            base: self.data_api_url.clone(),
                            request,
                        })
                        .is_err()
                    {
                        self.data_publish_state.submitting = false;
                        self.logs.error(
                            &mut self.vm.logs,
                            "提交数据发布请求失败",
                            &anyhow::anyhow!("数据请求线程不可用"),
                            None,
                        );
                    }
                }
                Cmd::LookupDataPublishDeletions(request) => {
                    if self
                        .bridge
                        .req
                        .send(data::Req::LookupDataPublishDeletions {
                            base: self.data_api_url.clone(),
                            request,
                        })
                        .is_err()
                    {
                        self.data_publish_state.submitting = false;
                        self.data_publish_state.pending_request = None;
                        eprintln!("[data_publish_delete_lookup] 数据服务线程不可用");
                        self.logs.error(
                            &mut self.vm.logs,
                            "获取待删除提资失败：数据服务线程不可用",
                            &anyhow::anyhow!("数据服务线程不可用"),
                            None,
                        );
                    }
                }
                Cmd::SubmitRoomCodePublish(request) => {
                    if self
                        .bridge
                        .req
                        .send(data::Req::RoomCodePublish {
                            base: self.data_api_url.clone(),
                            request,
                        })
                        .is_err()
                    {
                        self.manual_data_publish_state.submitting = false;
                        self.logs.error(
                            &mut self.vm.logs,
                            "手动提资请求发送失败",
                            &anyhow::anyhow!("数据请求线程不可用"),
                            None,
                        );
                    }
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
                    if !self.model_service_executable() {
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
                    if !self.model_service_executable() {
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
                // 库行「提前执行」（09-08 计划 D1 A / U2）：同一个 execute，名单只有
                // 这一个库。判据在按钮上就说完了（task_queue::early_run），这里不再判
                // ——按钮可点即此刻可执行，回执进日志，进度看队列行。
                Cmd::RunDbnumNow { dbnum } => {
                    if !self.model_service_executable() {
                        continue;
                    }
                    self.logs.info(
                        &mut self.vm.logs,
                        &format!("提前执行 db{dbnum}：已提交，任务排上后进度在任务队列里"),
                    );
                    let _ = self.bridge.req.send(data::Req::ModelUpdateExecute {
                        base: self.model_api_url.clone(),
                        project: self.vm.project.clone(),
                        mdb: self.mdb.clone(),
                        namespace: self.namespace.clone(),
                        dbnums: Some(vec![dbnum]),
                        from_wizard: false,
                    });
                }
                // 库行「重算模型（整库）」（2026-09-10 重新计算计划 D1 ② / R1）：服务端整库
                // 重建，一库一个 task_id。判据在按钮上说完（task_queue::model_rebuild），这里
                // 不再判——回执进日志，进度看队列里 `model_rebuild` 那一行。
                Cmd::RebuildDbnumModel { dbnum } => {
                    if !self.model_service_executable() {
                        continue;
                    }
                    self.logs.info(
                        &mut self.vm.logs,
                        &format!("重算模型（整库）db{dbnum}：已提交，任务排上后进度在任务队列里"),
                    );
                    let _ = self.bridge.req.send(data::Req::ModelRebuild {
                        base: self.model_api_url.clone(),
                        project: self.vm.project.clone(),
                        mdb: self.mdb.clone(),
                        namespace: self.namespace.clone(),
                        dbnum,
                    });
                }
                // 库行「清理模型（整库）」只打开确认框。真正的 DELETE 等确认（C5）。
                Cmd::CleanupDbnumModel { dbnum } => self.begin_dbnum_model_cleanup(dbnum),
                // 顶栏「重新生成房间」（全库）只打开确认框。真正的 POST 等确认——
                // 确认之后服务端清边表、换代重建、逐库重算，收不回来。
                Cmd::RebuildRooms => self.begin_room_rebuild(),
                Cmd::SetQueuePaused(paused) => {
                    if !self.model_service_writable() {
                        continue;
                    }
                    let _ = self.bridge.req.send(data::Req::QueueSetPaused {
                        base: self.model_api_url.clone(),
                        paused,
                    });
                }
                // 增量记录页「变化清单」：只读、按需；在飞或已取到就不重发（失败的许重试）。
                Cmd::LoadTaskChanges(task_id) => {
                    if matches!(
                        self.queue.changes.get(&task_id),
                        Some(
                            task_queue::ChangesState::Loading | task_queue::ChangesState::Loaded(_)
                        )
                    ) {
                        continue;
                    }
                    self.queue
                        .changes
                        .insert(task_id.clone(), task_queue::ChangesState::Loading);
                    let _ = self.bridge.req.send(data::Req::TaskChanges {
                        base: self.model_api_url.clone(),
                        task_id,
                    });
                }
                // 增量记录页「加载更早」：只读，上界是手上最老一条的入队时刻；一次在飞就不重发。
                Cmd::LoadOlderTasks => {
                    if self.queue.archive_loading {
                        continue;
                    }
                    let Some(before) = self.queue.oldest_created_at() else {
                        continue;
                    };
                    self.queue.archive_loading = true;
                    let _ = self.bridge.req.send(data::Req::OlderTasks {
                        base: self.model_api_url.clone(),
                        before,
                    });
                }
                // 死信只有人按这一下才会再动一次。它不入队，所以按完不会立刻多出
                // 一行——变化要等下一拍轮询，界面上那句「已提交 · 等下一拍」说的
                // 就是这件事。
                Cmd::RetryPendingUnit { dbnum, root_refno } => {
                    if !self.model_service_executable() {
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
                // 右键「查看 / 隐藏尺寸标注」（计划 B2）。带过来的已经是 BRAN——成员上溯
                // 在绘制层的靶子字段里做完了，这里不再判 noun。
                Cmd::ShowPipeDimensions(refno) => self.show_pipe_dimensions(refno),
                Cmd::ClearPipeDimensions => self.clear_pipe_dimensions(),
                Cmd::SetTubeTruncation(on) => self.set_tube_truncation(on),
                Cmd::FocusPane(pane) => self.state.focus(pane),
                Cmd::ToggleDock(side) => self.state.toggle_dock(side),
                Cmd::OpenRoomBrowser => {
                    self.room_browser_state.open = true;
                    // 全表在途时只开窗不重发：这查询几十秒级，叠一份是纯浪费。
                    if !self.rooms_overview.loading() {
                        self.rooms_overview.begin_query();
                        let _ = self.bridge.req.send(data::Req::RoomsOverview);
                    }
                }
                Cmd::RoomTree(action) => self.room_tree_action(action),
                Cmd::RevealRoomInTree(room) => self.reveal_room_in_tree(room),
                Cmd::FocusTreeScope(target) => {
                    // 用户显式点了定位：更早挂着的孔洞取景 / 房间取景 / 房间树取景就此作废，
                    // 否则它们晚一拍落地时会把这次查询作废，相机飞回旧目标、最新这一下被静默吞掉。
                    self.supersede_camera_intents();
                    let epoch = self.focus_bounds_request.begin(target);
                    if self
                        .bridge
                        .req
                        .send(data::Req::SubtreeBounds { epoch, target })
                        .is_err()
                    {
                        self.focus_bounds_request.cancel();
                        self.logs.warn(&mut self.vm.logs, "模型服务通道已断开");
                    } else {
                        // 大范围（SITE 级）这一拍可能要几秒，得有反馈；但记录级目标
                        // 毫秒就回来，立刻记一句只是噪音。压 400 ms 再发，快查询只留
                        // 结果那一条（见 `tick_focus_scope`）。
                        self.focus_scope_pending =
                            Some((epoch, target, Instant::now() + Duration::from_millis(400)));
                        // `ui()` 里的 tick 与 100 ms 重绘判定都在本帧命令产生之前跑过了，
                        // 这里补一次到期重绘：不补就得指望 egui 在输入事件后再排一帧。
                        ctx.request_repaint_after(Duration::from_millis(400));
                    }
                    dirty = true;
                }
                Cmd::Model(action) => {
                    match action {
                        action @ (plant_ui::ModelAction::Focus(_)
                        | plant_ui::ModelAction::FocusGroup { .. }
                        | plant_ui::ModelAction::FocusBounds { .. }) => {
                            // 取景入口：撤掉更早挂着的取景账，相机只听这一次。
                            self.supersede_camera_intents();
                            self.view3d_commands.push(Cmd::Model(action));
                        }
                        plant_ui::ModelAction::SetVisible { refnos, visible } => {
                            self.set_model_visible(refnos, visible)
                        }
                        // 「视图适应」也是一次取景：对准的是可见并集（房间视图里房间体落地之后
                        // 就是房间体；房间体还在途时先框隔离后的并集，落地那笔账会再纠正），
                        // 更早挂着的范围 / 孔洞 / 房间模型 / 房间树取景就此作废。
                        plant_ui::ModelAction::FitAll => {
                            self.supersede_camera_intents_for_fit();
                            self.view3d_commands
                                .push(Cmd::Model(plant_ui::ModelAction::FitAll));
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
                            self.leave_room_view();
                            self.view3d_commands
                                .push(Cmd::Model(plant_ui::ModelAction::ExitIsolate));
                        }
                        action => self.view3d_commands.push(Cmd::Model(action)),
                    }
                    dirty = true;
                }
                // 视方块的 Home 带 `fit: true`：和「视图适应」同一个框，算一次取景意图。
                // 点面（`fit: false`）只改朝向，不算——落到下面那条直通臂。
                command @ Cmd::SnapView { fit: true, .. } => {
                    self.supersede_camera_intents_for_fit();
                    self.view3d_commands.push(command);
                }
                command @ (Cmd::PickViewport(_)
                | Cmd::Camera(_)
                | Cmd::ResizeViewport(_)
                | Cmd::SetViewportGrid(_)
                | Cmd::SetViewportLook(_)
                | Cmd::SnapView { fit: false, .. }
                | Cmd::RestoreCamera(_)
                | Cmd::RetryFailedMeshes) => self.view3d_commands.push(command),
                Cmd::Navigate(step) => {
                    self.navigate(step);
                    dirty = true;
                }
                Cmd::RegenerateModels { targets } => {
                    self.begin_mutation(model_mutation::Kind::Regenerate, targets)
                }
                Cmd::CleanupModels { targets } => {
                    self.begin_mutation(model_mutation::Kind::Cleanup, targets)
                }
                Cmd::ModelMutationConfirm {
                    accepted,
                    purge_meshes,
                } => self.settle_mutation_confirm(accepted, purge_meshes),
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
                    "help          显示帮助\nclear         清空命令会话\nreindex       重建子串搜索索引\nq help        显示模型查询帮助\nq <属性>      查询当前元素属性\n/<名称>       按名称定位元素\n=<参考号>     按参考号定位元素",
                );
                false
            }
            ParsedCommand::Clear => {
                self.vm.command.lines.clear();
                self.command_epoch = self.command_epoch.wrapping_add(1);
                false
            }
            ParsedCommand::Reindex => {
                match self.bridge.req.send(data::Req::RebuildSearchIndex) {
                    // 回执只说「开始了」：重建在后台跑几十秒，就绪与失败都由
                    // `Evt::SearchIndex` 落进 logs 面板。
                    Ok(()) => self.command_output("搜索索引重建已开始（后台），完成后日志可见"),
                    Err(_) => self.command_error("重建失败：数据线程已关闭"),
                }
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
            // 查成功但一条属性都没有：把面板上那句定论原样说给命令行，别报「属性不存在」
            // ——那句听起来像打错了名字。
            PropsVm::Verdict(verdict) => return Err(format!("当前元素没有属性：{verdict}")),
            PropsVm::Ready(data) => data,
        };
        let attr = attr.trim();
        let value = if attr.eq_ignore_ascii_case("REF") || attr.eq_ignore_ascii_case("REFNO") {
            data.refno.to_e3d_id()
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

    /// 标题栏搜索框发起的一次查询。两路一起走，一条 Evt 回来。
    ///
    /// 不做防抖也不排队：前缀是索引范围扫、子串是本地索引，都在毫秒量级，多发
    /// 几条比压着不发划算；晚到的旧结果由 epoch 认出来丢掉。所以这里只管发，
    /// 认领在 `Evt::SearchElements` 那一侧。
    fn search_elements(&mut self, query: String) {
        self.search_epoch = self.search_epoch.wrapping_add(1);
        let request = data::Req::SearchElements {
            epoch: self.search_epoch,
            query: query.clone(),
        };
        if self.bridge.req.send(request).is_err() {
            self.vm.search.running = None;
            self.vm.search.error = Some("搜索失败：数据线程已关闭".into());
            return;
        }
        self.vm.search.running = Some(SearchRunVm { query });
    }

    /// 右键「重新生成模型」/「清理模型」的第一步：打开确认框（ADR-0030 D2；清理模型计划 D2）。
    ///
    /// 不清点、不解根、不删产物——确认框不带数字，人要判断的只有「这个范围要不要重算 / 清掉」。
    /// 右键换一行就重开一次，上一趟没确认的那份目标随之作废；跑起来的那一趟不会走到这里
    /// （`regen_busy` 挡着右键菜单的这两项）。
    fn begin_mutation(&mut self, kind: model_mutation::Kind, targets: Vec<RefU64>) {
        let Some(first) = targets.first().copied() else {
            return;
        };
        let label = regenerate_label(self.tree.label(first), targets.len());
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        self.mutation_run = Some(MutationRun {
            kind,
            label: label.clone(),
            targets,
            dbnum: None,
            auto: false,
            cancel: Default::default(),
        });
        self.mutation = Some(model_mutation::Vm::Confirm {
            kind,
            label,
            dbnum: None,
            // 「一并删除网格文件」默认勾上（2026-10-23）。只有清理那一档画这一格、也只有
            // 它消费这个值；重新生成走的是 `force = true`，服务端那条不收这个参数。
            purge_meshes: matches!(kind, model_mutation::Kind::Cleanup),
        });
    }

    /// 库行「清理模型（整库）」的确认框。已有一趟模型变更在跑或正等确认时不重开。
    fn begin_dbnum_model_cleanup(&mut self, dbnum: u32) {
        if !self.model_service_executable() {
            return;
        }
        if self.vm.regen_busy || self.mutation.is_some() {
            self.logs.info(
                &mut self.vm.logs,
                format!("清理模型（整库）db{dbnum}：已经有一趟重新生成 / 清理在进行"),
            );
            return;
        }
        let label = format!("db{dbnum}");
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        self.mutation_run = Some(MutationRun {
            kind: model_mutation::Kind::Cleanup,
            label: label.clone(),
            targets: Vec::new(),
            dbnum: Some(dbnum),
            auto: false,
            cancel: Default::default(),
        });
        self.mutation = Some(model_mutation::Vm::Confirm {
            kind: model_mutation::Kind::Cleanup,
            label,
            dbnum: Some(dbnum),
            // 整库清理也画这一格（`DELETE /dbnums/{dbnum}/model` 收 `purge_meshes`），默认与
            // 右键那一档一致：勾上。
            purge_meshes: true,
        });
    }

    /// 顶栏「重新生成房间」（全库）的确认框。已有一趟模型变更在跑或正等确认时不重开——
    /// 重建会拒掉在跑的生成（409），不该与它并发。
    fn begin_room_rebuild(&mut self) {
        if !self.model_service_executable() {
            return;
        }
        if self.vm.regen_busy || self.mutation.is_some() {
            self.logs.info(
                &mut self.vm.logs,
                "重新生成房间：已经有一趟重新生成 / 清理 / 重建在进行".to_string(),
            );
            return;
        }
        let label = "全部库".to_string();
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        self.mutation_run = Some(MutationRun {
            kind: model_mutation::Kind::RoomRebuild,
            label: label.clone(),
            targets: Vec::new(),
            dbnum: None,
            auto: false,
            cancel: Default::default(),
        });
        self.mutation = Some(model_mutation::Vm::Confirm {
            kind: model_mutation::Kind::RoomRebuild,
            label,
            dbnum: None,
            purge_meshes: false,
        });
    }

    /// 取回工作 / 换面之前放弃本端对「重新生成」那一趟的等待：数据线程要先排干在途请求，而补齐
    /// 循环最长等 30 分钟。置位之后补齐循环交回手里最近的回执、还没发出的目标不再发，服务端照跑。
    /// 只管生成：等房间参照模型那一档有自己的进度屏与取消入口。
    fn abandon_generate_wait(&self) {
        if let Some(run) = self.mutation_run.as_ref()
            && run.kind == model_mutation::Kind::Regenerate
        {
            run.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// 「没有模型就默认重新生成」的自动入口：显示路径发现范围内没有已生成模型
    /// （库级探针 `ready_total == 0`，或按祖先查 records 一条没有）时替用户发起——
    /// 与右键同一条 `GenerateModels` 通道，只是跳过确认框：这两种发现说的本来就是
    /// 「现在没有可看的」，问一次没有意义。进度屏照常进 `Running`，回执仍由
    /// [`Self::settle_models_generated`] 收，成功即按原方向显示。
    ///
    /// 返回真正发出去的那批目标。约束：每 refno 每会话只自动一发
    /// （`auto_generate_attempted`，发不出去不记账，下次点击还能再试）；已有一趟
    /// 变更在跑或服务不可写时一个都不发，调用方按原来的查空收口。
    fn begin_auto_generate(&mut self, targets: Vec<RefU64>, reason: String) -> Vec<RefU64> {
        let targets: Vec<RefU64> = targets
            .into_iter()
            .filter(|target| !self.auto_generate_attempted.contains(target))
            .collect();
        if targets.is_empty() {
            return Vec::new();
        }
        if self.mutation_run.is_some() || self.vm.regen_busy || !self.model_service_executable() {
            return Vec::new();
        }
        for target in &targets {
            self.auto_generate_attempted.insert(*target);
        }
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        let label = regenerate_label(self.tree.label(targets[0]), targets.len());
        let cancel = std::sync::Arc::<std::sync::atomic::AtomicBool>::default();
        self.mutation_run = Some(MutationRun {
            kind: model_mutation::Kind::Regenerate,
            label: label.clone(),
            targets: targets.clone(),
            dbnum: None,
            auto: true,
            cancel: cancel.clone(),
        });
        let sent = self
            .bridge
            .req
            .send(data::Req::GenerateModels {
                epoch: self.mutation_epoch,
                refnos: targets.clone(),
                // 显示补齐只补缺口、不先删（gen-model spec §4.5）：「查空」误判时先删会把已有模型清掉。
                force: false,
                cancel,
                base: self.model_api_url.clone(),
                project: self.vm.project.clone(),
                mdb: self.mdb.clone(),
                namespace: self.namespace.clone(),
            })
            .is_ok();
        if !sent {
            self.mutation_run = None;
            self.logs.warn(
                &mut self.vm.logs,
                format!("{label}：数据线程已关闭，这一趟没有开始"),
            );
            return Vec::new();
        }
        self.logs.info(
            &mut self.vm.logs,
            format!(
                "{label}：{reason}，默认重新生成（{} 个目标，服务端预算 120 秒，超时自动续跑补齐）…",
                targets.len()
            ),
        );
        self.vm.regen_busy = true;
        self.mutation = Some(model_mutation::Vm::Running {
            kind: model_mutation::Kind::Regenerate,
            label,
        });
        targets
    }

    /// 确认框的回执。接受就对右键那几行**各一发**——重新生成是 `generate(force = true)`，整批
    /// 回执落在 `Evt::ModelsGenerated`；清理是 `DELETE /model/subtree`，整批回执落在
    /// `Evt::ModelsCleaned`。取消 / 关窗 / 关掉失败态一个请求都不发。
    ///
    /// `purge_meshes` 是框上「一并删除网格文件」那一格此刻的状态。两个清理档都消费它
    /// （右键走子树、整库走整库，两条服务端接口都收这个参数）；重新生成那一档丢掉它——
    /// 它走 `force = true`，服务端先删后算，网格删了就得整批重写一遍。
    fn settle_mutation_confirm(&mut self, accepted: bool, purge_meshes: bool) {
        let closed = self.mutation.take();
        // 「等房间参照模型」那一档的叉 /「取消等待」走到这里：生成请求还没发，置位开关
        // 让数据线程放弃余下目标（已发出的照常跑完——那一档没有叉，走不到这里）。
        let waiting_cancelled = matches!(
            closed,
            Some(model_mutation::Vm::WaitingResident { .. })
        );
        let Some(model_mutation::Vm::Confirm { .. }) = closed else {
            if waiting_cancelled {
                if let Some(run) = self.mutation_run.as_ref() {
                    run.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    match run.kind {
                        // 「后台继续」：重建那发服务端收不回，这里只是摘屏停轮询。
                        model_mutation::Kind::RoomRebuild => self.logs.info(
                            &mut self.vm.logs,
                            "房间重建已在服务端后台继续；再点「重新生成房间」可重新查看进度"
                                .to_string(),
                        ),
                        _ => self.logs.info(
                            &mut self.vm.logs,
                            "已取消等待房间参照模型".to_string(),
                        ),
                    }
                }
                self.vm.regen_busy = false;
            }
            // 失败态被关掉（跑起来的那一档没有叉，到不了这里）：收摊。
            self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
            self.mutation_run = None;
            return;
        };
        if !accepted {
            self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
            self.mutation_run = None;
            return;
        }
        let Some(run) = self.mutation_run.as_ref() else {
            self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
            return;
        };
        let (kind, label, targets, dbnum, cancel) = (
            run.kind,
            run.label.clone(),
            run.targets.clone(),
            run.dbnum,
            run.cancel.clone(),
        );
        // 整库清理是三条库行命令里唯一破坏性的那条，确认发出去之前再过一次执行闸门。
        // 按钮灰着挡不住已经开着的确认框，也不挡绕过按钮的入口。
        if dbnum.is_some() && !self.model_service_executable() {
            self.mutation_run = None;
            self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
            return;
        }
        // 「一并删除网格文件」只对两个清理档有意义：重新生成走 `force = true`（服务端先删
        // 后算，网格不能删——删了就得重写一遍全部网格）。框上那一格的值只在这两档落地。
        let purge_meshes = purge_meshes && kind == model_mutation::Kind::Cleanup;
        // 帧号从这里起不再变：整批回执拿它认帧。整库清理没有右键那几行，单独一发。
        let request = match (kind, dbnum) {
            (_, Some(dbnum)) => data::Req::CleanupDbnumModel {
                epoch: self.mutation_epoch,
                base: self.model_api_url.clone(),
                project: self.vm.project.clone(),
                mdb: self.mdb.clone(),
                namespace: self.namespace.clone(),
                dbnum,
                purge_meshes,
            },
            (model_mutation::Kind::Regenerate, None) => data::Req::GenerateModels {
                epoch: self.mutation_epoch,
                refnos: targets.clone(),
                // 右键「重新生成模型」= 先删后生成（ADR-0030 D2）。
                force: true,
                cancel: cancel.clone(),
                base: self.model_api_url.clone(),
                project: self.vm.project.clone(),
                mdb: self.mdb.clone(),
                namespace: self.namespace.clone(),
            },
            (model_mutation::Kind::RoomRebuild, None) => data::Req::RebuildRooms {
                epoch: self.mutation_epoch,
                cancel: cancel.clone(),
                base: self.model_api_url.clone(),
                project: self.vm.project.clone(),
                mdb: self.mdb.clone(),
                namespace: self.namespace.clone(),
            },
            (model_mutation::Kind::Cleanup, None) => data::Req::CleanupModels {
                epoch: self.mutation_epoch,
                refnos: targets.clone(),
                purge_meshes,
                base: self.model_api_url.clone(),
                project: self.vm.project.clone(),
                mdb: self.mdb.clone(),
                namespace: self.namespace.clone(),
            },
        };
        if self.bridge.req.send(request).is_err() {
            self.mutation_run = None;
            self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
            self.logs.warn(
                &mut self.vm.logs,
                format!("{label}：数据线程已关闭，这一趟没有开始"),
            );
            return;
        }
        let started = match (kind, dbnum) {
            (_, Some(dbnum)) => format!(
                "db{dbnum}：正在清理全部模型产物（只删产物、水位归零，不自动重算{}）…",
                if purge_meshes {
                    "；另把这个库引用过的网格文件从磁盘删掉"
                } else {
                    ""
                }
            ),
            (model_mutation::Kind::Regenerate, None) => format!(
                "{label}：正在生成模型（{} 个目标，范围内已有的产物会被重算替换；服务端预算 120 秒，超时后自动续跑补齐）…",
                targets.len()
            ),
            (model_mutation::Kind::RoomRebuild, None) => {
                "{label}：已提交重新生成房间（清边表 → 重建参照模型 → 逐库重算），进度如下…"
                    .to_string()
            }
            (model_mutation::Kind::Cleanup, None) => format!(
                "{label}：正在清理模型产物（{} 个目标，服务端按子树删已生成的产物、不重算；只删产物，不删设计元素{}）…",
                targets.len(),
                if purge_meshes {
                    "；另把这片范围引用过的网格文件从磁盘删掉"
                } else {
                    ""
                }
            ),
        };
        self.logs.info(&mut self.vm.logs, started);
        self.vm.regen_busy = true;
        // 重建那一发受理即 202、随后全程是「等它跑完」：直接进等待档，进度条由
        // `Evt::ResidentWait` 填；其余两种是在途请求，进 `Running`。
        self.mutation = Some(match kind {
            model_mutation::Kind::RoomRebuild => model_mutation::Vm::WaitingResident {
                kind,
                label,
                phase: "提交重建请求".to_string(),
                files: None,
                meshes: None,
                dbnums: None,
            },
            _ => model_mutation::Vm::Running { kind, label },
        });
    }

    /// generate 的整批回执落地（ADR-0030 D2 第 3 步）：成功的目标**生成完成之后再显示**——
    /// 以它们为祖先发显示（等于替用户点一次眼睛，旧几何由 `regenerate_refresh` 换掉）；
    /// `timeout` 的不显示，日志说明服务端后台继续；失败的 ERROR。
    fn settle_models_generated(
        &mut self,
        outcomes: Vec<(RefU64, anyhow::Result<model_update_api::GenerateReply>)>,
    ) {
        let Some(run) = self.mutation_run.take() else {
            return;
        };
        self.vm.regen_busy = false;
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        let mut display = Vec::new();
        let mut failures = Vec::new();
        let mut legacy_said = false;
        for (refno, result) in &outcomes {
            let label = self.tree.label(*refno);
            match generate_outcome(result) {
                GenerateOutcome::Display => {
                    let reply = result.as_ref().expect("Display 只来自 Ok");
                    if reply.legacy_endpoint && !legacy_said {
                        legacy_said = true;
                        self.logs.info(
                            &mut self.vm.logs,
                            "模型服务是旧版（没有 /model/generate），已按旧路径生成；请升级 gen-model",
                        );
                    }
                    self.logs
                        .info(&mut self.vm.logs, generate_summary(&label, reply));
                    // 有根没生成出来 = 部分成功：三维照常显示已补齐的那一半，横幅把这半句
                    // 留住——不然人对着缺口的模型只会以为「本来就这样」。
                    if reply.failed_root_count > 0 {
                        failures.push(format!(
                            "{label}：{} 个生成根未生成（详见日志）",
                            reply.failed_root_count
                        ));
                    }
                    // 生成成功 = 这个库有模型了：库级结论正写，显示路径不再为它发探针。
                    if let Some(dbnum) = self.tree.dbnum_of(*refno) {
                        self.dbnum_model_known.insert(dbnum, true);
                    }
                    display.push(*refno);
                }
                GenerateOutcome::Background => {
                    // 走得到这里 = 续跑补齐也等到上限（30 分钟）或宿主退出，服务端可能仍在跑。
                    self.logs.warn(
                        &mut self.vm.logs,
                        format!(
                            "{label}：服务端还没做完（已续跑补齐至上限），后台继续生成；完成后再点眼睛查看（这次不显示）"
                        ),
                    );
                }
                GenerateOutcome::Failed => {
                    let error = result.as_ref().expect_err("Failed 只来自 Err");
                    self.logs
                        .error(&mut self.vm.logs, format!("{label}：生成失败"), error, None);
                    failures.push(format!(
                        "{label}：{}",
                        model_update_api::failure_of(error).message
                    ));
                }
            }
        }
        self.mutation = if failures.is_empty() {
            None
        } else {
            Some(model_mutation::Vm::Failed {
                kind: model_mutation::Kind::Regenerate,
                label: run.label.clone(),
                reason: failures.join("；"),
            })
        };
        if display.is_empty() {
            return;
        }
        // 生成完成之后，再调用显示接口：这几个目标名下的旧几何要换成重算过的，所以先把
        // 它们从「已知范围」里摘掉，让 `set_model_visible` 走查询那条路，回包时再卸旧装新。
        // 自动趟（显示路径的默认重新生成）尊重期间翻成隐藏的方向：生成最长两分钟，
        // 用户中途点掉眼睛 / HideAll 不该被回执掰回显示。显式右键趟照旧一律显示。
        for target in &display {
            self.model_scopes.remove(target);
            self.regenerate_refresh.insert(*target);
        }
        if run.auto {
            display
                .retain(|target| self.tree.pending_direction.get(target).copied() != Some(false));
        }
        if display.is_empty() {
            return;
        }
        self.set_model_visible(display, true);
    }

    /// `DELETE /model/subtree` 的整批回执落地（清理模型计划 C3 / D3）：成功的目标把它名下
    /// 已加载的模型从三维卸掉、树行回「未加载」——产物已经在服务端删了，这是已知结论，
    /// 不值一次 records 往返；409 / 超时 / 其他失败逐目标点名，各归各。
    fn settle_models_cleaned(
        &mut self,
        outcomes: Vec<(RefU64, anyhow::Result<model_update_api::CleanupReply>)>,
    ) {
        let Some(run) = self.mutation_run.take() else {
            return;
        };
        self.vm.regen_busy = false;
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        let mut failures = Vec::new();
        let mut unload = Vec::new();
        for (refno, result) in &outcomes {
            let label = self.tree.label(*refno);
            match cleanup_outcome(result) {
                CleanupOutcome::Cleaned => {
                    let reply = result.as_ref().expect("Cleaned 只来自 Ok");
                    let removed = self.forget_cleaned_scope(*refno);
                    let count = removed.len();
                    unload.extend(removed);
                    self.logs.info(
                        &mut self.vm.logs,
                        format!(
                            "{label}：已清理模型产物（{}）；三维卸载 {count} 个模型；需要时右键「重新生成模型」",
                            reply.server_note()
                        ),
                    );
                }
                CleanupOutcome::Rejected => {
                    let error = result.as_ref().expect_err("Rejected 只来自 Err");
                    self.logs.error(
                        &mut self.vm.logs,
                        format!("{label}：范围内有根正在生成或同库有整库任务在跑，稍后再试"),
                        error,
                        None,
                    );
                    failures.push(format!(
                        "{label}：{}",
                        model_update_api::failure_of(error).message
                    ));
                }
                CleanupOutcome::Unknown => {
                    // 服务端这条没有预算包装、同步删到底：客户端等不到不等于它没删。摘掉
                    // 范围缓存并标成待重查，人点眼睛就会重查一次 records——空了就把残留卸掉，
                    // 还在就照常显示。不动 eye：超时并不知道删没删成，eye 只读渲染回执
                    // （ADR-0016），标成「未加载」会和还画着的三维对不上。
                    self.model_scopes.remove(refno);
                    self.regenerate_refresh.insert(*refno);
                    self.logs.warn(
                        &mut self.vm.logs,
                        format!("{label}：清理请求超时，服务端可能已删一部分；点眼睛重查确认"),
                    );
                    failures.push(format!("{label}：清理请求超时，服务端可能已删一部分；点眼睛重查确认"));
                }
                CleanupOutcome::Failed => {
                    let error = result.as_ref().expect_err("Failed 只来自 Err");
                    self.logs
                        .error(&mut self.vm.logs, format!("{label}：清理失败"), error, None);
                    failures.push(format!(
                        "{label}：{}",
                        model_update_api::failure_of(error).message
                    ));
                }
            }
        }
        if !unload.is_empty() {
            self.view3d_commands
                .push(Cmd::Model(plant_ui::ModelAction::Unload { refnos: unload }));
        }
        self.mutation = if failures.is_empty() {
            None
        } else {
            Some(model_mutation::Vm::Failed {
                kind: model_mutation::Kind::Cleanup,
                label: run.label.clone(),
                reason: failures.join("；"),
            })
        };
    }

    /// `DELETE /dbnums/{n}/model` 的回执（C5 / E7）。成功按回执里的 `ref0s` 卸已加载叶子；
    /// 缺席则只记日志。409 原文进 ERROR，老服务端没有这条路由是一句 INFO。
    fn settle_dbnum_model_cleaned(
        &mut self,
        dbnum: u32,
        result: anyhow::Result<model_update_api::DbnumModelCleanupReceipt>,
    ) {
        // 没有对得上的这一趟就原样留下：晚到的回执不许关掉此刻新开的确认框，也不许把
        // regen_busy 清掉。认帧靠事件上的 epoch，这里再对一下库号。
        if self
            .mutation_run
            .take_if(|run| run.dbnum == Some(dbnum))
            .is_none()
        {
            return;
        }
        self.vm.regen_busy = false;
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        match result {
            Ok(receipt) => {
                let unloaded = match receipt.ref0s.as_deref() {
                    Some(ref0s) => self.forget_cleaned_dbnum(ref0s),
                    None => Vec::new(),
                };
                let tail = if receipt.ref0s.is_none() {
                    "老服务端不给 ref0s，请点眼睛重查".to_owned()
                } else {
                    format!(
                        "三维卸载 {} 个模型；点「重算模型（整库）」重新生成（服务重启也会自动补）",
                        unloaded.len()
                    )
                };
                // 网格那一格：勾了才有这三格（老服务端 / 没勾都不给），一句接在尾巴上。
                let meshes = model_update_api::mesh_purge_note(
                    receipt.mesh_files_removed,
                    receipt.mesh_files_absent,
                    receipt.mesh_files_failed,
                );
                self.logs.info(
                    &mut self.vm.logs,
                    format!(
                        "已清理 db{dbnum} 全部模型产物（{} 条语句 / {} ms{meshes}）；{tail}",
                        receipt.range_statements, receipt.elapsed_ms
                    ),
                );
                if !unloaded.is_empty() {
                    self.view3d_commands
                        .push(Cmd::Model(plant_ui::ModelAction::Unload { refnos: unloaded }));
                }
                self.mutation = None;
                self.poll_queue_now();
            }
            Err(error) => {
                if model_update_api::endpoint_missing(&error) {
                    self.logs.info(
                        &mut self.vm.logs,
                        format!("清理模型（整库）db{dbnum}：这一档服务端没有整库清理接口"),
                    );
                    self.mutation = None;
                    return;
                }
                let failure = model_update_api::failure_of(&error);
                let title = if failure.code == "conflict" {
                    format!("清理模型（整库）db{dbnum}：该库有任务在跑或有生成在飞")
                } else {
                    format!("清理模型（整库）db{dbnum} 失败")
                };
                self.logs.error(&mut self.vm.logs, &title, &error, None);
                self.mutation = Some(model_mutation::Vm::Failed {
                    kind: model_mutation::Kind::Cleanup,
                    label: format!("db{dbnum}"),
                    reason: failure.message,
                });
            }
        }
    }

    /// 整库清理之后，把属于这些 Ref0 的已加载叶子从本端账上抹掉（E7）。
    fn forget_cleaned_dbnum(&mut self, ref0s: &[u32]) -> Vec<RefU64> {
        let gone = dbnum_unload_set(&self.loaded_models, ref0s);
        let gone_set: HashSet<RefU64> = gone.iter().copied().collect();
        let drop_keys: Vec<RefU64> = self
            .model_scopes
            .iter()
            .filter(|(_, members)| members.iter().any(|member| gone_set.contains(member)))
            .map(|(key, _)| *key)
            .collect();
        for key in drop_keys {
            self.model_scopes.remove(&key);
            self.scope_targets.remove(&key);
            self.regenerate_refresh.remove(&key);
            self.tree.pending_direction.remove(&key);
            self.tree.pending_visibility.remove(&key);
        }
        for refno in &gone_set {
            self.loaded_models.remove(refno);
            self.tree.visibility.remove(refno);
            self.tube_visibility.remove(refno);
            self.tube_pending.remove(refno);
            self.model_show_waiting.remove(refno);
            self.tree.pending_direction.remove(refno);
            self.tree.pending_visibility.remove(refno);
            self.tree.visibility_unavailable.insert(*refno);
            self.model_owner.remove(refno);
        }
        gone
    }

    /// 清理成功之后把一个目标名下的模型从本端账上抹掉（D3），回要卸载的那批叶子。
    ///
    /// 范围由 [`cleanup_unload_plan`] 只按本端缓存算；这里只负责把结论落到各本账上：范围内的
    /// 「已知范围」键整条摘掉（连同它作为范围目标 / 待替换目标的登记），卸掉的叶子从已装账、
    /// 实际渲染状态与别的范围键的成员表里拿走，目标自己标成「明确没有」——不然树行会从还显示着
    /// 的祖先那里把 eye 继承过来，看着像没清。
    fn forget_cleaned_scope(&mut self, target: RefU64) -> Vec<RefU64> {
        let parents = &self.tree.parent;
        let plan = cleanup_unload_plan(
            target,
            &self.model_scopes,
            &self.loaded_models,
            |refno| parents.get(&refno).copied(),
            &self.model_owner,
        );
        for key in &plan.dropped_scopes {
            self.model_scopes.remove(key);
            self.scope_targets.remove(key);
            self.regenerate_refresh.remove(key);
            self.tree.pending_direction.remove(key);
            self.tree.pending_visibility.remove(key);
        }
        let gone: HashSet<RefU64> = plan.refnos.iter().copied().collect();
        for refno in &gone {
            self.loaded_models.remove(refno);
            self.tree.visibility.remove(refno);
            self.tube_visibility.remove(refno);
            self.tube_pending.remove(refno);
            self.model_show_waiting.remove(refno);
            self.tree.pending_direction.remove(refno);
            self.tree.pending_visibility.remove(refno);
            self.model_owner.remove(refno);
        }
        // 别的范围键（比如目标所在的 ZONE）成员表里还记着这些叶子：摘掉，空了的键一起退场，
        // 下次点那一行眼睛就重查而不是对着一张空名单下指令。
        self.model_scopes.retain(|_, members| {
            members.retain(|member| !gone.contains(member));
            !members.is_empty()
        });
        self.tree.pending_direction.remove(&target);
        self.tree.pending_visibility.remove(&target);
        self.tree.visibility_unavailable.insert(target);
        plan.refnos
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
        // 视口右键菜单的尺寸标注靶子跟着主选中走（计划 B2）。
        self.sync_selection_branch();
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
                // 主选中换了 = 到了一个新地方，导航历史记一条。所有入口
                // （树 / 视口拾取 / 搜索 / 日志 / 房间成员 / 命令行）都汇到这里。
                self.record_nav();
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
        // 房间树的外部选中联动（计划 D6）：别处选中了一个构件，若它在**已取过树的房**下且面板在前台，
        // 展开到它并滚到可见；不为它去拉没展开的房。树内自己点的不再反过来动。
        if !self.room_tree_internal_selection
            && before != self.vm.selection.primary()
            && let Some(refno) = self.vm.selection.primary()
            && self.state.is_pane_active(Pane::RoomTree)
            && let Some(id) = self.room_tree.reveal_refno(refno)
        {
            self.room_tree_reveal = Some(id);
            self.rebuild_room_tree();
        }
    }

    /// 主选中能挂尺寸标注的 BRAN 随选中与树同步（视口右键菜单的靶子，计划 B2）。
    /// 树也是触发点：视口拾取到的成员要等定位把祖先加载回来，才认得出它头上的 BRAN。
    fn sync_selection_branch(&mut self) {
        self.vm.selection_branch = self
            .vm
            .selection
            .primary()
            .and_then(|refno| self.tree.dimension_branch(refno));
    }

    /// 取这条 BRAN 的尺寸标注并挂到视口上（计划 B2）。一层只挂一条，换目标就是换帧：
    /// 在途的旧取数回来时帧号对不上，宿主直接丢——与搜索 / 清点同一套取消口径。
    fn show_pipe_dimensions(&mut self, refno: RefU64) {
        self.dimensions_epoch = self.dimensions_epoch.wrapping_add(1);
        self.vm.dimensions = DimensionsVm::Loading(refno);
        let sent = self.bridge.req.send(data::Req::PipeDimensions {
            epoch: self.dimensions_epoch,
            refno,
        });
        if sent.is_err() {
            let message = "模型服务通道已断开".to_owned();
            self.logs.warn(&mut self.vm.logs, message.clone());
            self.vm.dimensions = DimensionsVm::Failed { refno, message };
        }
    }

    /// 清掉尺寸标注层。也进一帧：在途的取数回来不再上屏。重连 / 换项目 / 整场重装走的
    /// 也是它——旧库的 refno 挂在层上，菜单会对着新库里同号的元素说「隐藏」；视口那一层
    /// 与 `SceneRoot` 平级，整场换模型不会顺手把它带走，得由这里明说撤掉（计划 B3）。
    fn clear_pipe_dimensions(&mut self) {
        self.dimensions_epoch = self.dimensions_epoch.wrapping_add(1);
        self.vm.dimensions = DimensionsVm::Off;
        self.pending_dimensions = Some(DimensionLayerUpdate::Clear);
    }

    /// 会话 → 页签 Vm 的同步。会话是真值，`vm.virtual_holes` 是每帧抄过去的投影。
    fn sync_hole_vm(&mut self) {
        self.vm.virtual_holes = self.hole_session.vm();
    }

    /// 打开虚拟孔洞表的文件选择。已经开着时再点只当没听见——对话框自己排队。
    fn open_hole_pick(&mut self) {
        if self.hole_pick.is_some() {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.hole_pick = Some(HoleFilePick::open());
        }
        // 浏览器端：click 必须落在用户手势里（这条命令就是那一下），
        // 拿不到 input（没 DOM）时静默失败，日志里说一声。
        #[cfg(target_arch = "wasm32")]
        {
            match HoleFilePick::open() {
                Some(pick) => self.hole_pick = Some(pick),
                None => self.logs.warn(
                    &mut self.vm.logs,
                    "虚拟孔洞表选择器打不开（浏览器环境异常）",
                ),
            }
        }
    }

    /// 文件选择回来了：喂给导入器，换会话、换切削体层、发起映射确认。
    fn import_hole_file(&mut self, file: PickedHoleFile) {
        let result = match &file {
            PickedHoleFile::Path(path) => virtual_hole::import_file(path),
            PickedHoleFile::Bytes { name, bytes } => virtual_hole::import_bytes(name, bytes),
        };
        let report = match result {
            Ok(report) => report,
            Err(error) => {
                self.logs.error(
                    &mut self.vm.logs,
                    "虚拟孔洞表导入失败",
                    &error,
                    None,
                );
                return;
            }
        };
        let hole_count = report.holes.len();
        let error_count = report.row_errors.len();
        // 重导入是整份替换：旧会话进新帧（在途回执作废），切削体层整层换掉。
        self.hole_session.import(report);
        self.hole_retry_pending.clear();
        self.hole_focus_pending = None;
        self.hole_target_retried.clear();
        let holes = self.hole_session.holes().to_vec();
        self.pending_hole_ops.push(VirtualHoleOp::SetLayer { holes });
        self.sync_hole_vm();
        // 自动映射：RelyItemREF 解析出的参考号去库里确认真伪。
        let refnos = self.hole_session.pending_mapping_refnos();
        if !refnos.is_empty() {
            // 映射帧号只有会话那一份：宿主曾另记一份，换库时只加自己那份，从此每次导入都差 1，回包全被当过期丢掉。
            let _ = self.bridge.req.send(data::Req::VirtualHoleTargets {
                epoch: self.hole_session.map_epoch(),
                refnos,
            });
        }
        // 导入完自己跳到「虚拟孔洞」页签（ADR-0011：动作之后进度该自己现身）。
        self.state.focus(Pane::VirtualHoles);
        let mut message = format!("虚拟孔洞表已导入：{hole_count} 条记录");
        if error_count > 0 {
            message.push_str(&format!("，{error_count} 行解析失败（见页签「解析错误」）"));
        }
        self.logs.info(&mut self.vm.logs, message);
    }

    /// 视口的一份布尔回执落地：行状态进会话，「目标没实例」的重试在这里排队。
    fn apply_hole_receipt(&mut self, receipt: VirtualHoleReceipt) {
        let effect = self.hole_session.apply_receipt(&receipt);
        match effect {
            ReceiptEffect::RetryAfterShow(refno) => {
                if self.hole_target_retried.insert(refno) {
                    // 只改画面不改库：显示模型，等网格落地再重发预览。
                    self.set_model_visible(vec![refno], true);
                    self.hole_retry_pending.insert(refno);
                    self.logs.info(
                        &mut self.vm.logs,
                        format!("虚拟孔洞目标 {refno} 未加载，已发起显示，网格落地后自动重试预览"),
                    );
                }
            }
            ReceiptEffect::StatusOnly => match receipt.status {
                VirtualHoleStatus::Changed => {
                    self.logs.info(
                        &mut self.vm.logs,
                        format!(
                            "虚拟孔洞预览完成：{}，{} 个孔洞，{} ms",
                            receipt.refno,
                            receipt.hole_ids.len(),
                            receipt.elapsed_ms
                        ),
                    );
                }
                VirtualHoleStatus::Failed => {
                    let el = self.tree.element(receipt.refno);
                    let error = anyhow::anyhow!(
                        "{}",
                        receipt.error.as_deref().unwrap_or("未知错误")
                    );
                    self.logs
                        .error_of(&mut self.vm.logs, el, "虚拟孔洞布尔失败", &error, None);
                }
                _ => {}
            },
        }
        // 帧号对不上（清预览 / 重导入之后回来的）apply_receipt 内部已丢弃，这里照常同步。
        self.sync_hole_vm();
    }

    /// 虚拟孔洞在等目标落地的两笔账（ADR-0031 阶段四）：「目标没实例」的重试预览，与「定位目标」的取景。
    /// 目标的几何画出来了就做（可能同时等好几面墙，谁先落地谁先做）；范围查空 / 网格全没加载成功就销账并说一声
    /// ——否则行停在「已就绪」、相机不动，谁也不知道在等什么。裁决见 [`hole_target_verdict`]。
    fn settle_hole_waits(&mut self) {
        if self.hole_retry_pending.is_empty() && self.hole_focus_pending.is_none() {
            return;
        }
        let mut targets: Vec<RefU64> = self.hole_retry_pending.iter().copied().collect();
        targets.extend(self.hole_focus_pending);
        targets.sort();
        targets.dedup();
        for target in targets {
            let verdict = hole_target_verdict(
                self.model_scope_pending.contains(&target),
                self.tree.visibility_unavailable.contains(&target),
                self.model_scopes.get(&target),
                &self.tree.visibility,
            );
            match verdict {
                HoleTargetVerdict::Waiting => {}
                HoleTargetVerdict::Loaded => {
                    if self.hole_retry_pending.remove(&target) {
                        let holes: Vec<_> = self
                            .hole_session
                            .holes_for_target(target)
                            .into_iter()
                            .cloned()
                            .collect();
                        if !holes.is_empty() {
                            self.pending_hole_ops.push(VirtualHoleOp::Preview {
                                epoch: self.hole_session.epoch(),
                                target,
                                holes,
                            });
                            self.logs.info(
                                &mut self.vm.logs,
                                format!("虚拟孔洞目标 {target} 已加载，重试预览"),
                            );
                        }
                    }
                    if self.hole_focus_pending.take_if(|t| *t == target).is_some() {
                        // 只取景，不撤别人的账：这里是落地点（几何终于到了），手上这笔就是
                        // 用户那次意图，反过来作废在途的范围查询会吞掉后来的点击。
                        self.view3d_commands
                            .push(Cmd::Model(plant_ui::ModelAction::Focus(target)));
                    }
                }
                HoleTargetVerdict::Unavailable(reason) => {
                    if self.hole_retry_pending.remove(&target) {
                        let failed = self.hole_session.fail_target(target);
                        self.logs.warn(
                            &mut self.vm.logs,
                            format!(
                                "虚拟孔洞目标 {target} {reason}，{} 条记录按失败落账",
                                failed.len()
                            ),
                        );
                        self.sync_hole_vm();
                    }
                    if self.hole_focus_pending.take_if(|t| *t == target).is_some() {
                        self.logs.warn(
                            &mut self.vm.logs,
                            format!("虚拟孔洞：定位目标 {target} 作废——它{reason}"),
                        );
                    }
                }
            }
        }
    }

    /// 清掉虚拟孔洞预览层：恢复原始网格、撤切削体；孔洞列表与映射保留。
    fn clear_hole_preview(&mut self) {
        self.hole_session.clear_preview();
        self.hole_retry_pending.clear();
        self.hole_focus_pending = None;
        self.hole_target_retried.clear();
        self.pending_hole_ops.push(VirtualHoleOp::Clear);
        self.sync_hole_vm();
    }

    /// 把会话里的孔洞重新摆成切削体层。`Clear` 连层一起撒（「清除」按钮说好的），但重映射一行、清场重装
    /// 这种只是预览作废的场合，会话里的孔洞还在，切削体不该跟着没——否则「定位孔洞」静默无效，要重导入才回来。
    fn restore_hole_layer(&mut self) {
        if self.hole_session.holes().is_empty() {
            return;
        }
        let holes = self.hole_session.holes().to_vec();
        self.pending_hole_ops.push(VirtualHoleOp::SetLayer { holes });
    }

    fn refetch_props(&mut self, refno: RefU64) {
        self.vm.props.begin_query();
        let _ = self.bridge.req.send(data::Req::Props(refno));
    }

    /// 房间归属随选中预取（计划决定 5）：查询极轻，右键菜单打开的瞬间就有现成数据。
    fn refetch_rooms(&mut self, refno: RefU64) {
        self.vm.rooms.begin_query();
        let _ = self.bridge.req.send(data::Req::ElementRooms(refno));
        if self.tree.noun(refno).is_some_and(is_panel_noun) {
            let _ = self.bridge.req.send(data::Req::PanelRoom(refno));
        }
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
        // 房间 X-Ray 与虚拟孔洞 X-Ray 抢同一份半透明材质：后拨的赢。
        if self.hole_session.xray() {
            self.hole_session.set_xray(false);
            self.pending_hole_ops.push(VirtualHoleOp::SetXRay(false));
            self.sync_hole_vm();
        }
        self.logs.info(
            &mut self.vm.logs,
            format!("房间 X-Ray 已启用：{room} · 面板 {panel_count} · 不透明度 25%"),
        );
    }

    /// 房间视图的上半程：记下在途目标，去数据层解析全量成员集。
    /// 下半程在 `Evt::RoomDetail` 到达时展开成隔离 + 取景（有实时渲染器时），
    /// 并把详情填进「房间」页签——同一次往返喂两个消费者。
    fn focus_room(&mut self, room: RefU64) {
        // 进房间视图也会取景（房间体到了才动相机），算一次意图。
        self.supersede_camera_intents();
        self.focus_room_pending = Some(room);
        self.focus_room_frame = Some(room);
        self.room_pane_focus = Some(room);
        self.vm.room_detail.begin_query();
        let _ = self.bridge.req.send(data::Req::RoomDetail(room));
        // 聚焦一间房也是「到了一个地方」：同一个选中换了房间算另一条历史。
        self.record_nav();
    }

    // ------------------------------------------------ 房间视图 · 房间体与直段截断（2026-09-29 计划 M4）

    /// 「截断直段」开关此刻的值（设置里那一格）。
    fn truncate_tubes(&self) -> bool {
        self.settings_state.saved.room_view_truncate_tubes
    }

    /// 房间视图的下半程（详情回来之后）：按开关隔离、去取房间体、挂 HUD。
    ///
    /// 取景不在这里做：对准的是房间体，而它要等在册面板的模型记录回来（`Evt::RoomVolume`）。
    fn enter_room_view(&mut self, room: RefU64, detail: plant_ui_data::room::RoomDetail) {
        if detail.panels.is_empty() && detail.member_refnos.is_empty() {
            // 空房壳是正常现象：房间参照模型里有这间房，但它没有面板，也就取不到
            // 房间体与成员。按信息说一句就走，不升级成警告。
            self.logs.info(
                &mut self.vm.logs,
                format!("房间 {} 是空房壳（没有面板也没有成员），无处取景", detail.room_num),
            );
            return;
        }
        let truncate = self.truncate_tubes();
        // 换一间房：上一间的房间体整层撤掉，直段先复原，再按这一间截。
        self.pending_room_volume.push(RoomVolumeUpdate::Clear);
        self.room_view_epoch = self.room_view_epoch.wrapping_add(1);
        let epoch = self.room_view_epoch;
        self.view3d_commands.push(Cmd::Model(room_view_isolate(
            &detail,
            truncate,
            &self.model_scopes,
            false,
        )));
        self.logs.info(
            &mut self.vm.logs,
            format!(
                "房间视图：{} · 面板 {} · 成员 {} · 截断直段 {}",
                detail.room_num,
                detail.panels.len(),
                detail.member_count,
                if truncate { "开" } else { "关" },
            ),
        );
        self.vm.room_view = Some(RoomViewVm {
            room,
            room_num: detail.room_num.clone(),
            member_count: detail.member_count,
            truncate,
            truncation: TruncationVm::Loading { done: 0, total: 0 },
        });
        let unsent = if detail.panels.is_empty() {
            Some("房间没有在册面板".to_owned())
        } else if self
            .bridge
            .req
            .send(data::Req::RoomVolume {
                epoch,
                room,
                panels: detail.panels.clone(),
            })
            .is_err()
        {
            Some("数据线程已断开，取不到房间体".to_owned())
        } else {
            None
        };
        self.room_view = Some(RoomViewCtx {
            room,
            detail,
            epoch,
            volume_failed: false,
            announced: None,
        });
        if let Some(reason) = unsent {
            self.room_volume_fallback(reason);
        }
    }

    /// 房间体回来了：交给视口画成半透明并截断直段；取景账还立着才对准它。取不到就走兜底。
    fn settle_room_volume(
        &mut self,
        epoch: u64,
        room: RefU64,
        result: anyhow::Result<Vec<aios_core::GeomInstQuery>>,
    ) {
        if !self
            .room_view
            .as_ref()
            .is_some_and(|ctx| room_volume_is_current(ctx, epoch, room))
        {
            return;
        }
        let records = match result {
            Ok(records) if !records.is_empty() => records,
            Ok(_) => return self.room_volume_fallback("在册面板都还没有模型".to_owned()),
            Err(error) => {
                return self.room_volume_fallback(format!(
                    "房间体取不到：{}",
                    logs::error_chain(&error)
                ));
            }
        };
        let truncate = self.truncate_tubes();
        // 这一次的房间体要不要动相机：只有进视图那一下登记过账才算。这一句在帧号早退与兜底
        // 分流之后，过期回包走不到这里，账不会被吃掉。
        let frame = self
            .focus_room_frame
            .take_if(|pending| *pending == room)
            .is_some();
        let Some(ctx) = self.room_view.as_mut() else {
            return;
        };
        let found: HashSet<RefU64> = records.iter().map(|record| record.refno.refno()).collect();
        let missing = ctx
            .detail
            .panels
            .iter()
            .filter(|panel| !found.contains(panel))
            .count();
        // 缺面板就不截：宁可不截，也不按半个房间截。半透明照样画，看得见缺在哪。
        let unavailable = (missing > 0).then(|| format!("{missing} 块在册面板没有模型，房间体不全"));
        // 上一次房间体没取到、兜底把面板放进了隔离：这次取到了，按正常口径重新隔离。
        if std::mem::take(&mut ctx.volume_failed) {
            self.view3d_commands.push(Cmd::Model(room_view_isolate(
                &ctx.detail,
                truncate,
                &self.model_scopes,
                false,
            )));
        }
        if frame && let Some((min_mm, max_mm)) = plant_ui_view3d::panel_bounds_mm(&records) {
            self.view3d_commands
                .push(Cmd::Model(plant_ui::ModelAction::FocusBounds {
                    min_mm,
                    max_mm,
                }));
        }
        self.pending_room_volume
            .push(RoomVolumeUpdate::Set(plant_ui_view3d::RoomVolumeBatch {
                generation: epoch,
                panels: records,
                carriers: ctx.detail.tube_carriers.clone(),
                truncate,
                unavailable,
            }));
    }

    /// 房间体取不到时的兜底：面板照旧进隔离，HUD 与日志说「不可用」，不装作截断过。
    /// 取景账还立着才退回面板与成员的并集。
    fn room_volume_fallback(&mut self, reason: String) {
        let truncate = self.truncate_tubes();
        let Some(room) = self.room_view.as_ref().map(|ctx| ctx.room) else {
            return;
        };
        // 取景账与 `settle_room_volume` 同一条口径：只有进视图那一下登记过才算。
        let frame = self
            .focus_room_frame
            .take_if(|pending| *pending == room)
            .is_some();
        let Some(ctx) = self.room_view.as_mut() else {
            return;
        };
        ctx.volume_failed = true;
        // 下面这句告警就是这一次的交代，视口那边不会再有回执。
        ctx.announced = Some(truncate);
        self.view3d_commands.push(Cmd::Model(room_view_isolate(
            &ctx.detail,
            truncate,
            &self.model_scopes,
            true,
        )));
        let focus = room_view_fallback_focus(&ctx.detail, &self.model_scopes);
        let room_num = ctx.detail.room_num.clone();
        if frame && !focus.is_empty() {
            self.view3d_commands
                .push(Cmd::Model(plant_ui::ModelAction::FocusGroup {
                    refnos: focus,
                }));
        }
        // 半截的旧房间体（重装后重取失败的那种）不能留着冒充这一次的。
        self.pending_room_volume.push(RoomVolumeUpdate::Clear);
        if let Some(view) = self.vm.room_view.as_mut() {
            view.truncation = TruncationVm::Unavailable(reason.clone());
        }
        self.logs.warn(
            &mut self.vm.logs,
            format!("房间 {room_num} 的房间体不可用：{reason}；这次不截断直段"),
        );
    }

    /// HUD 上拨了「截断直段」：记进设置（桌面端落盘），正在进行的房间视图按新口径重新隔离，
    /// 视口复原或截断。
    fn set_tube_truncation(&mut self, on: bool) {
        self.settings_state.set_room_view_truncate_tubes(on);
        let saved = self.settings_state.saved.clone();
        self.persist_settings(&saved);
        let Some(ctx) = self.room_view.as_mut() else {
            return;
        };
        ctx.announced = None;
        let volume_failed = ctx.volume_failed;
        self.view3d_commands.push(Cmd::Model(room_view_isolate(
            &ctx.detail,
            on,
            &self.model_scopes,
            volume_failed,
        )));
        if !volume_failed {
            self.pending_room_volume.push(RoomVolumeUpdate::Truncate(on));
        }
        if let Some(view) = self.vm.room_view.as_mut() {
            view.truncate = on;
        }
        self.logs.info(
            &mut self.vm.logs,
            if on {
                "直段截断已开：直段只画房间体内的那一截，房间外的管件不显示"
            } else {
                "直段截断已关：整根分支显示"
            },
        );
    }

    /// 视口上一拍的截断回执抄进 HUD；做完（或不可用）那一刻说一句日志。
    fn sync_room_clip(&mut self, status: Option<plant_ui_view3d::RoomClipStatus>) {
        let (Some(ctx), Some(status)) = (self.room_view.as_mut(), status) else {
            return;
        };
        // 兜底之后的「不可用」是宿主定的；帧号不对的是上一间房（或重装前）的回执。
        if ctx.volume_failed || status.generation != ctx.epoch {
            return;
        }
        let settled = matches!(
            status.phase,
            plant_ui_view3d::RoomClipPhase::Ready | plant_ui_view3d::RoomClipPhase::Unavailable(_)
        );
        let announce = settled && ctx.announced != Some(status.truncate);
        if announce {
            ctx.announced = Some(status.truncate);
        }
        let room_num = ctx.detail.room_num.clone();
        if let Some(view) = self.vm.room_view.as_mut() {
            view.truncation = truncation_vm(&status);
        }
        if !announce {
            return;
        }
        match &status.phase {
            plant_ui_view3d::RoomClipPhase::Unavailable(reason) => self.logs.warn(
                &mut self.vm.logs,
                format!("房间 {room_num} 的房间体不可用：{reason}；这次不截断直段"),
            ),
            _ if status.truncate => self.logs.info(
                &mut self.vm.logs,
                format!("直段截断：{room_num} · {}", truncation_summary(&status)),
            ),
            _ => {}
        }
    }

    /// 退出房间视图：摘徽章、撤房间体（直段整批复原）。可见性由随后的 `ExitIsolate` 按快照恢复。
    fn leave_room_view(&mut self) {
        self.vm.room_view = None;
        self.focus_room_pending = None;
        self.focus_room_frame = None;
        if self.room_view.take().is_some() {
            self.pending_room_volume.push(RoomVolumeUpdate::Clear);
        }
        self.room_view_epoch = self.room_view_epoch.wrapping_add(1);
    }

    /// 整场重装之后还在房间视图里：隔离被冲掉了，按同一口径再隔离一次（快照只记第一次，退出照旧
    /// 回到进房间之前）；房间体也重取一次。新装进来的直段在新房间体到之前，照旧按旧的那层截。
    fn reapply_room_view_after_reload(&mut self) {
        let truncate = self.truncate_tubes();
        let Some(ctx) = self.room_view.as_mut() else {
            return;
        };
        self.view3d_commands.push(Cmd::Model(room_view_isolate(
            &ctx.detail,
            truncate,
            &self.model_scopes,
            ctx.volume_failed,
        )));
        if ctx.detail.panels.is_empty() {
            return;
        }
        self.room_view_epoch = self.room_view_epoch.wrapping_add(1);
        ctx.epoch = self.room_view_epoch;
        ctx.announced = None;
        let request = data::Req::RoomVolume {
            epoch: ctx.epoch,
            room: ctx.room,
            panels: ctx.detail.panels.clone(),
        };
        if self.bridge.req.send(request).is_err() {
            self.room_volume_fallback("数据线程已断开，取不到房间体".to_owned());
        }
    }

    // ---------------------------------------------------------------- 导航历史（S1-D）

    /// 把此刻当成一个「地方」记进导航历史：选择集 + 有焦点的页签 + 房间聚焦 + 相机。
    /// 回放期间（`nav.navigating`）由栈自己挡掉，不在调用点判。
    fn record_nav(&mut self) {
        let entry = nav_history::NavEntry {
            selection: self.vm.selection.clone(),
            pane: self.state.active_pane(),
            room: self.room_pane_focus,
            camera: self.camera_pose,
        };
        self.nav.record(entry, self.camera_pose);
        self.sync_nav_vm();
    }

    /// 命令栏那两枚箭头 / 快捷键 / 右键列表按下去：走一步并回放目标。
    ///
    /// 回放 = 选中 → 树定位（展开祖先、滚到行）→ 页签 → 房间详情 → 相机。相机走
    /// `RestoreCamera` 的 0.3s 插值；房间只还详情不重做隔离 / 取景——那一下会把刚
    /// 还回去的相机顶掉，而且隔离改的是整个场景的可见性，不该由「后退」悄悄做。
    fn navigate(&mut self, step: NavStep) {
        let Some(entry) = self.nav.step(step, self.camera_pose) else {
            return;
        };
        self.nav.navigating = true;
        if let Some(primary) = entry.selection.primary() {
            // `locate` 先把主选中落成单选并把树带过去；多选再整体换回来。
            self.locate(primary, false);
            if entry.selection.len() > 1 {
                self.set_selection(entry.selection.clone());
            }
        }
        if let Some(pane) = entry.pane {
            self.state.focus(pane);
        }
        if let Some(room) = entry.room {
            self.restore_room_detail(room);
        }
        if let Some(camera) = entry.camera {
            // 回到历史位置也是一次取景意图：更早挂着的账（范围查询、孔洞、房间取景）都作废，
            // 否则它们晚落地时会把相机从刚回退到的位置拽走。
            self.supersede_camera_intents();
            self.view3d_commands.push(Cmd::RestoreCamera(camera));
        }
        self.nav.navigating = false;
        self.sync_nav_vm();
    }

    /// 只把「房间」页签的详情还回去，不动视口。与 [`Self::focus_room`] 的分别就是
    /// 不置 `focus_room_pending`——回包到了只喂页签那一半。
    fn restore_room_detail(&mut self, room: RefU64) {
        self.room_pane_focus = Some(room);
        self.vm.room_detail.begin_query();
        let _ = self.bridge.req.send(data::Req::RoomDetail(room));
    }

    /// 历史栈变了就把只读投影同步给绘制层：按钮启用态、hover 文案、右键列表都读它。
    /// 名字按此刻的树缓存取；树上还没有它（视口拾取到的成员、祖先还没回来）就退回 refno。
    fn sync_nav_vm(&mut self) {
        let entries = self
            .nav
            .entries()
            .iter()
            .map(|entry| {
                let (label, noun) = match entry.selection.primary() {
                    Some(refno) => {
                        let rest = entry.selection.len() - 1;
                        let mut label = self.tree.label(refno);
                        if rest > 0 {
                            label.push_str(&format!(" +{rest}"));
                        }
                        (label, self.tree.noun(refno).unwrap_or("").to_owned())
                    }
                    None => ("未选中".to_owned(), String::new()),
                };
                NavEntryVm {
                    label,
                    noun,
                    pane: entry.pane,
                }
            })
            .collect();
        self.vm.nav = NavHistoryVm {
            entries,
            cursor: self.nav.cursor(),
        };
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
        // 「显示房间模型」以取景收尾，所以先撤掉更早挂着的账。
        self.supersede_camera_intents();
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
                    || !self.pending_refreshes.is_empty()
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
            // 库级预检查（只在「显示」方向做；隐藏方向的查询照旧只查不生）：已知这库
            // 没有就绪模型的目标跳过 records 往返，直接默认重新生成；结论未知的库
            // 随这次显示各发一发探针（每库单飞，回包落在 `Evt::DbnumModelProbe`）。
            let mut query = Vec::with_capacity(plan.query.len());
            let mut regen_now = Vec::new();
            for target in plan.query {
                let known = self
                    .tree
                    .dbnum_of(target)
                    .and_then(|dbnum| self.dbnum_model_known.get(&dbnum).copied());
                match known {
                    Some(false) if visible => regen_now.push(target),
                    _ => {
                        if visible && let Some(dbnum) = self.tree.dbnum_of(target) {
                            if !self.dbnum_model_known.contains_key(&dbnum)
                                && self.dbnum_model_probing.insert(dbnum)
                            {
                                let sent = self
                                    .bridge
                                    .req
                                    .send(data::Req::DbnumModelProbe {
                                        epoch: self.model_scope_epoch,
                                        dbnum,
                                        base: model_update_api::base_url(),
                                        project: self.vm.project.clone(),
                                        mdb: self.mdb.clone(),
                                        namespace: self.namespace.clone(),
                                    })
                                    .is_ok();
                                if !sent {
                                    self.dbnum_model_probing.remove(&dbnum);
                                }
                            }
                        }
                        query.push(target);
                    }
                }
            }
            if !regen_now.is_empty() {
                let all = regen_now.clone();
                let fired: HashSet<RefU64> = self
                    .begin_auto_generate(regen_now, "所在库还没有已生成的模型".into())
                    .into_iter()
                    .collect();
                // 没发出去的（已有变更在跑 / 服务不可写）退回查询路，查空那一臂照常收口。
                query.extend(all.into_iter().filter(|target| !fired.contains(target)));
            }
            if !query.is_empty() {
                if self.model_scope_pending.is_empty() && self.model_show_waiting.is_empty() {
                    self.model_scope_failures = 0;
                }
                self.model_scope_pending.extend(query.iter().copied());
                let verb = if visible { "显示" } else { "隐藏" };
                self.logs.info(
                    &mut self.vm.logs,
                    format!("开始查询并{verb}模型：{} 个目标", query.len()),
                );
                self.model_progress_until = None;
                self.vm.model_load = Some(ModelLoadVm::Resolving("解析模型范围…".into()));
                let targets = query;
                if self
                    .bridge
                    .req
                    .send(data::Req::ModelScopes {
                        epoch: self.model_scope_epoch,
                        targets: targets.clone(),
                        base: model_update_api::base_url(),
                        project: self.vm.project.clone(),
                        mdb: self.mdb.clone(),
                        namespace: self.namespace.clone(),
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
        self.abandon_generate_wait();
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
                &self.scope_targets,
            ));
        }
        let snapshot = self.model_reload_restore.clone().unwrap_or_default();
        let total = snapshot.models.len();
        let hidden = snapshot
            .models
            .iter()
            .filter(|(_, visible)| !visible)
            .count();
        let targets = snapshot.targets.len();
        // 名单已经拍进快照；重装落地时按快照放回来（ADR-0030 D3）。
        self.scope_targets.clear();
        self.pending_models = Some(Vec::new());
        self.pending_incremental_models.clear();
        self.pending_refreshes.clear();
        self.model_show_waiting.clear();
        self.latest_mesh_progress = None;
        self.model_scope_failures = 0;
        self.model_scopes.clear();
        self.model_scope_pending.clear();
        // 在途的范围查询说的是清场前那个世界，回包一律作废。库级探针同帧换代，
        // 在途账一并摘掉，不然这一库就再也探不动了；已问出的结论（known）仍有效。
        self.focus_bounds_request.cancel();
        self.model_scope_epoch = self.model_scope_epoch.wrapping_add(1);
        self.dbnum_model_probing.clear();
        // 尺寸标注量的是清场前那份几何，重装回来的 BRAN 可能已经不是那个样子（计划 B3：
        // 换场景清层）。要看就再右键一次。
        self.clear_pipe_dimensions();
        // 场景清空，被替换过的预览网格随实体一起没了：撤掉孔洞预览层，孔洞列表
        // 与映射保留（重装回来还能再发预览）。切削体层与 SceneRoot 平级，清场本不该带走它；
        // `Clear` 撒了，按会话里的孔洞摆回去。
        if !self.hole_session.is_empty() {
            self.clear_hole_preview();
            self.restore_hole_layer();
        }
        self.loaded_models.clear();
        self.model_owner.clear();
        self.tree.visibility.clear();
        self.tree.pending_direction.clear();
        self.tree.pending_visibility.clear();
        self.tree.visibility_unavailable.clear();
        self.tube_visibility.clear();
        self.tube_pending.clear();
        self.model_progress_until = None;
        if total == 0 {
            self.vm.model_load = None;
            self.logs
                .info(&mut self.vm.logs, "三维本来就空着，这次只刷新树");
        } else if targets == 0 {
            // 老快照 / 旧路径加载的模型没有范围目标记录：按模型 refno 逐个重查已生成的产物。
            self.vm.model_load = Some(ModelLoadVm::Resolving("已清空三维，正在重查模型…".into()));
            self.logs.info(
                &mut self.vm.logs,
                format!(
                    "已清空三维场景：无范围目标记录，按上次的产物重查 {total} 个模型（其中 {hidden} 个保持隐藏）"
                ),
            );
        } else {
            // 只重查、不先生成（ADR-0030 D3）：三维不自己追新，要最新右键「重新生成模型」。
            self.vm.model_load = Some(ModelLoadVm::Resolving(format!(
                "已清空三维，正在按 {targets} 个范围重查已生成的模型…"
            )));
            self.logs.info(
                &mut self.vm.logs,
                format!(
                    "已清空三维场景：按 {targets} 个范围（点过眼睛的目标）重查已生成的模型，重装 {total} 个模型（其中 {hidden} 个保持隐藏）；不重新生成，要最新请右键「重新生成模型」"
                ),
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
        let mut terminal_batches = false;
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
            // 一个数据批次走到终态（成功 / 部分 / 失败都算），就意味着库里可能
            // 多了、少了或改了名字——子串索引该去对一次戳了。
            terminal_batches = true;
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
        if terminal_batches {
            // 校验是单飞的：同一拍里几个批次一起到终态也只会跑一次。
            let _ = self.bridge.req.send(data::Req::CheckSearchIndex);
        }
        (data_applied, units, task_ids)
    }

    /// 按一份确切的清单取回工作，人不用记得再去点一下菜单。
    ///
    /// 与菜单点进来的那次不同：这里手上有确切线索，所以只重查真正会变的
    /// 分支——单元自己、它的原 OWNER、新 OWNER，外加树里记着的当前父节点
    /// （元素被移走时后端的 `old_owner` 未必解得出来，本端的缓存却还留着移动前
    /// 那一头）。没有模型重载需求的树补查路径仍保留三维（见 [`auto_refresh`]）。
    fn refresh_for_units(&mut self, units: Vec<model_update::RefreshUnit>, task_ids: Vec<String>) {
        if units.is_empty() {
            return;
        }
        if !task_ids.is_empty() {
            self.get_work_with_models_for_tasks(false, task_ids);
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
        self.start_get_work_for_tasks(branches.into_iter().collect(), false, task_ids);
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
        self.start_get_work(vec![anchor], false);
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
        self.reset_for_reconnect();
        self.logs.info(&mut self.vm.logs, "正在重连数据源…");
        let _ = self.bridge.req.send(data::Req::Reconnect);
    }

    /// 换供数模式（ADR-0026 热切，计划 §5.3）。与重连同一副骨架，多三步：清场前先拍
    /// 快照（另放一格，重连那套复位会清掉取回工作的那一格）、把三维真正清空（重连不
    /// 清几何，这里必须清——三维不许留旧供数方的几何，计划 D10）、告诉数据线程换面。
    /// 新面 `Evt::Ready(Ok)` 到了再按快照重装；失败则停在未连接态，下拉可再改回去。
    fn switch_read_face(&mut self, kind: settings::ReadFaceKind) {
        let from = self.read_face;
        // 只记第一次：换面失败后再换回来时场景早就空了，再拍只会拍到空白，把用户
        // 真正的显示 / 隐藏冲掉（与 `clear_scene_for_reload` 同一条规矩）。
        if self.read_face_switch_restore.is_none() {
            self.read_face_switch_restore = Some(reload_snapshot(
                &self.loaded_models,
                &self.tree.pending_direction,
                &self.tree.visibility,
                &self.scope_targets,
            ));
        }
        let snapshot = self.read_face_switch_restore.clone().unwrap_or_default();
        self.reset_for_reconnect();
        // `Replace(空)` despawn 全部场景根（与取回工作的清场同一招）。相机不动。
        self.pending_models = Some(Vec::new());
        // 搜索下拉里的是旧面的结果，在途那次也隔在门外；子串索引的状态由新面的
        // `refresh_search_index` 重报（库供数开 / 建，服务供数 Off），不留旧面那份。
        self.search_epoch = self.search_epoch.wrapping_add(1);
        self.vm.search = SearchVm {
            coverage: kind,
            ..SearchVm::default()
        };
        self.read_face = kind;
        // 接入点面板报的是这一刻真正在用的那一面；队列面板据它决定库行「模型来源」
        // 画不画、轮询失败那句怎么说，搜索下拉据它说清命中覆盖到哪儿（计划 §5.5）。
        self.vm.access_point.read_face = kind;
        self.queue.read_face = kind;
        let line = format!(
            "供数模式：{} → {}；已清场，重装 {} 个已加载模型 / {} 个范围目标",
            from.label(),
            kind.label(),
            snapshot.models.len(),
            snapshot.targets.len()
        );
        self.command_output(line.clone());
        self.logs.info(&mut self.vm.logs, line);
        self.abandon_generate_wait();
        let _ = self.bridge.req.send(data::Req::SwitchReadFace(kind));
    }

    /// 重连 / 换面前把上一段连接的一切归零。新连接必须从空缓存开始：否则失败时状态栏
    /// 和属性仍在说旧模型已经就绪。
    fn reset_for_reconnect(&mut self) {
        self.clear_room_xray();
        self.room_panel_cache.clear();
        self.focus_bounds_request.cancel();
        self.model_scope_epoch = self.model_scope_epoch.wrapping_add(1);
        self.model_scopes.clear();
        // 库级结论与「每会话一发」的账都记上一段连接的：换接入点后重新问。
        self.dbnum_model_known.clear();
        self.dbnum_model_probing.clear();
        self.auto_generate_attempted.clear();
        // 上一段连接的库号：换接入点后同一个号未必还是同一个库。
        self.search_hit_dbs.clear();
        self.tree.visibility_unavailable.clear();
        self.model_scope_pending.clear();
        self.loaded_models.clear();
        self.model_owner.clear();
        self.model_show_waiting.clear();
        self.pending_incremental_models.clear();
        self.pending_refreshes.clear();
        self.latest_mesh_progress = None;
        self.model_scope_failures = 0;
        self.model_progress_until = None;
        self.pending_room_frame = None;
        self.vm.model_load = None;
        self.tree = TreeModel::default();
        self.tube_visibility.clear();
        self.tube_pending.clear();
        self.vm.data_source_ok = false;
        self.vm.project.clear();
        self.mdb.clear();
        self.namespace.clear();
        // 队列面板那份跟着一起清。它平时随轮询回填，而轮询可能几秒才来一拍——
        // 那几秒里「立刻扫一遍」还亮着，按下去带的是上一次连接的 MDB。
        self.queue.mdb.clear();
        self.queue.namespace.clear();
        // 那行「数据水位落后 N 次保存」说的是旧接入点，下一拍轮询会按新的重算。
        self.vm.watermark_lag = None;
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
        self.scope_targets.clear();
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
        self.focus_room_frame = None;
        // 房间体属于旧连接：撤层，在途的房间体回包凭帧号作废。
        if self.room_view.take().is_some() {
            self.pending_room_volume.push(RoomVolumeUpdate::Clear);
        }
        self.room_view_epoch = self.room_view_epoch.wrapping_add(1);
        self.pending_room_frame = None;
        self.room_pane_focus = None;
        // 历史栈上的 refno 属于旧接入点：同一个号未必还是同一个元素，整栈作废。
        self.nav.clear();
        self.sync_nav_vm();
        // 尺寸标注层挂的是旧库的 BRAN：清掉，在途的取数回来也不再上屏。
        self.clear_pipe_dimensions();
        // 旧库的虚拟孔洞参考号在新库里没意义：整份会话作废，预览层撤掉。
        if !self.hole_session.is_empty() {
            self.hole_session.reset();
            self.hole_retry_pending.clear();
            self.hole_focus_pending = None;
            self.hole_target_retried.clear();
            self.pending_hole_ops.push(VirtualHoleOp::Clear);
            self.sync_hole_vm();
        }
        self.vm.selection_branch = None;
        // 房间树是旧接入点的：整份丢掉，面板回到 Idle，下次画出来再拉；在途的回包凭帧号作废。
        self.room_tree.clear();
        self.room_tree_epoch = self.room_tree_epoch.wrapping_add(1);
        self.room_tree_deferred.clear();
        self.room_tree_confirm = None;
        self.room_tree_reveal = None;
        self.room_tree_reveal_room = None;
        self.room_tree_frame = None;
        self.vm.room_tree = RoomTreeVm::Idle;
        // 换库了，旧库的房间总览不作数。窗口开着就顺手重拉，关着就等下次打开。
        if self.room_browser_state.open {
            self.rooms_overview.begin_query();
            let _ = self.bridge.req.send(data::Req::RoomsOverview);
        } else {
            self.rooms_overview = RoomBrowserVm::Idle;
        }
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

    /// 身份闸门：预览、暂停 / 恢复队列走它——这些不需要 worker 在场。
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

    /// 执行闸门：开始更新 / 立刻扫一遍 / 立刻重试走它。身份之外还要服务**会消化**——
    /// 执行请求一律 202 入队，服务以 direct 形态运行、没起 worker 时它会永远排队，
    /// 服务端不会替界面说「不行」（`task_queue::Vm::execution_blocked_reason`）。
    /// 按钮本来就该灰着；这里是命令行 / 快捷键那类绕过按钮的入口的最后一道。
    fn model_service_executable(&mut self) -> bool {
        if !self.model_service_writable() {
            return false;
        }
        if let Some(reason) = self.queue.execution_blocked_reason() {
            self.logs
                .warn(&mut self.vm.logs, format!("执行请求已阻断：{reason}"));
            return false;
        }
        true
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
        // 树变了，主选中头上的 BRAN 可能这才加载到（视口拾取 -> 定位 -> 祖先落地）。
        self.sync_selection_branch();
        // 房间树的 eye 读的是同一份实际可见性账，跟着一起重新展平；「加载模型」的取景也在这里等范围回包。
        self.settle_room_tree_frame();
        self.rebuild_room_tree();
        // 虚拟孔洞在等的目标（重试预览 / 定位目标）同理：范围回包与渲染回执都从这里过，查空也在这里销账。
        self.settle_hole_waits();
    }

    // ------------------------------------------------ 房间树（plant3d-web「房间」页签移植，ADR 0068）

    /// 把状态机展平成绘制层的 `vm.room_tree`。行的 eye 与模型树同一份账：按 refno 读 View3d 的
    /// 实际可见性与在途方向（`TreeModel::visibility_for` / `pending_direction`），分组行由状态机按子行聚合。
    fn rebuild_room_tree(&mut self) {
        let next = match self.room_tree.status.clone() {
            RootsStatus::Idle => RoomTreeVm::Idle,
            RootsStatus::Loading => RoomTreeVm::Loading,
            RootsStatus::Unavailable(reason) => RoomTreeVm::Unavailable(reason),
            RootsStatus::Failed(reason) => RoomTreeVm::Failed(reason),
            RootsStatus::Ready { note } => {
                let tree = &self.tree;
                let scopes = &self.model_scopes;
                let eye = |refno: RefU64| EyeState {
                    visibility: tree.visibility_for(refno, scopes),
                    pending: tree.pending_direction.get(&refno).copied(),
                };
                // 直段行（阶段四）：按逐段账画；账上没这一段 = 没带身份进场景，所属 BRAN 整根都没画出来时也算未加载。
                let tube_visibility = &self.tube_visibility;
                let tube_pending = &self.tube_pending;
                let tube_eye = |bran: RefU64, segment: &plant_ui::TubeSegment| {
                    let actual = tube_visibility
                        .get(&bran)
                        .and_then(|segments| segments.get(segment));
                    let visibility = match actual {
                        _ if tree.visibility_for(bran, scopes) == RowVisibility::Unloaded => {
                            RowVisibility::Unloaded
                        }
                        Some(true) => RowVisibility::Shown,
                        Some(false) => RowVisibility::Hidden,
                        None => RowVisibility::Unloaded,
                    };
                    EyeState {
                        visibility,
                        pending: tube_pending
                            .get(&bran)
                            .and_then(|pending| pending.get(segment))
                            .copied(),
                    }
                };
                RoomTreeVm::Ready(RoomTreeDataVm {
                    rows: self.room_tree.rows(&EyeSource {
                        model: &eye,
                        tube: &tube_eye,
                    }),
                    total: self.room_tree.total(),
                    matched: self.room_tree.filtered_roots().len(),
                    filter: self.room_tree.filter.clone(),
                    note,
                    errors: self.room_tree.errors_vm(),
                    reveal: self.room_tree_reveal.clone(),
                    confirm: self.room_tree_confirm.clone(),
                })
            }
        };
        self.vm.room_tree = next;
    }

    /// 拉根清单。`force` = 刷新钮：丢掉全部缓存重来；否则在途 / 已就绪就不重发。
    /// 库供数下不发请求，整面板一句注释（计划 D2：树是服务端聚合的，库供数没有第二条路）。
    fn room_tree_load(&mut self, force: bool) {
        if force {
            self.room_tree.clear();
            self.room_tree_deferred.clear();
            self.room_tree_confirm = None;
            self.room_tree_reveal = None;
            self.room_tree_reveal_room = None;
            self.room_tree_frame = None;
        } else if matches!(
            self.room_tree.status,
            RootsStatus::Loading | RootsStatus::Ready { .. }
        ) {
            return;
        }
        self.room_tree_epoch = self.room_tree_epoch.wrapping_add(1);
        if self.read_face == settings::ReadFaceKind::Store {
            self.room_tree
                .set_unavailable(ROOM_TREE_STORE_NOTE.to_owned());
            // 库供数下没有树可定位：等根清单的那条「在房间树查看」作废，别留到将来切回服务供数时突然跳出来。
            if let Some(room) = self.room_tree_reveal_room.take() {
                self.logs.warn(
                    &mut self.vm.logs,
                    format!(
                        "房间树：库供数下没有房间树，「在房间树查看」{} 作废",
                        room.to_slash_string()
                    ),
                );
            }
            return;
        }
        self.room_tree.begin_roots();
        if self
            .bridge
            .req
            .send(data::Req::SpatialRooms {
                epoch: self.room_tree_epoch,
            })
            .is_err()
        {
            self.room_tree.fail_roots("数据查询线程已停止".to_owned());
        }
    }

    fn room_tree_action(&mut self, action: RoomTreeAction) {
        match action {
            RoomTreeAction::Load => self.room_tree_load(false),
            RoomTreeAction::Refresh => self.room_tree_load(true),
            RoomTreeAction::SetFilter(text) => self.room_tree.filter = text,
            RoomTreeAction::ToggleExpand(id) => match self.room_tree.toggle_expand(&id) {
                Some(ExpandNeed::Tree(room)) => self.ensure_room_tree(room),
                Some(ExpandNeed::Leaves(node, selector)) => self.ensure_room_leaves(node, selector),
                None => {}
            },
            RoomTreeAction::Select { id, modifiers } => {
                // Shift 区间按此刻的可见行序算，那个序就是上一帧画出去的 rows。
                let order: Vec<RoomNodeId> = match &self.vm.room_tree {
                    RoomTreeVm::Ready(data) => data.rows.iter().map(|row| row.id.clone()).collect(),
                    _ => Vec::new(),
                };
                let clicked = self.room_tree.node(&id).and_then(|node| {
                    node.refno
                        .or_else(|| node.tube.as_ref().map(|tube| tube.unit))
                });
                let mut refnos = self.room_tree.select(&id, modifiers, &order);
                // 只选了分组行：树内高亮，全局选中不动（它们没有场景键，清掉属性面板只会让人以为点空了）。
                if !refnos.is_empty() {
                    // 主选中 = 刚点的那一行（有场景键的话），摆到末位让 `toggle` 把光标落到它身上。
                    if let Some(clicked) = clicked
                        && let Some(pos) = refnos.iter().position(|refno| *refno == clicked)
                    {
                        let primary = refnos.remove(pos);
                        refnos.push(primary);
                    }
                    let mut selection = Selection::single(refnos[0]);
                    for refno in &refnos[1..] {
                        selection.toggle(*refno);
                    }
                    self.room_tree_internal_selection = true;
                    self.set_selection(selection);
                    self.room_tree_internal_selection = false;
                }
            }
            RoomTreeAction::SetVisible { id, visible } => {
                self.run_room_tree_action(id, RoomTreeDeferred::SetVisible(visible))
            }
            RoomTreeAction::Focus(id) => {
                // 房间树上的聚焦也是一次取景意图：撤掉更早挂着的账，再挂这一次的。
                self.supersede_camera_intents();
                self.run_room_tree_action(id, RoomTreeDeferred::Focus)
            }
            RoomTreeAction::Isolate(id) => self.run_room_tree_action(id, RoomTreeDeferred::Isolate),
            RoomTreeAction::LoadModels { id, confirmed } => {
                self.room_tree_confirm = None;
                // 「加载模型」落地后要取景，所以它也算取景意图。
                self.supersede_camera_intents();
                self.run_room_tree_action(
                    id,
                    RoomTreeDeferred::LoadModels {
                        confirmed,
                        frame: true,
                    },
                );
            }
            RoomTreeAction::CancelLoadModels => self.room_tree_confirm = None,
        }
        // 刚挂起的动作若它等的请求根本没发出去（数据线程已停），这一拍就作废并说一声，别一直挂着。
        self.settle_room_tree_deferred();
        self.rebuild_room_tree();
    }

    /// 取一间房的整树（取过 / 在途 / 空房都不再发）。
    fn ensure_room_tree(&mut self, room: RefU64) {
        if !self.room_tree.needs_tree(room) {
            return;
        }
        let id = RoomNodeId::Room(room);
        self.room_tree.loading.insert(id.clone());
        self.room_tree.clear_error(&id);
        if self
            .bridge
            .req
            .send(data::Req::SpatialRoomTree {
                epoch: self.room_tree_epoch,
                room,
                only: None,
            })
            .is_err()
        {
            self.room_tree.loading.remove(&id);
            self.room_tree
                .set_error(id, "数据查询线程已停止".to_owned());
        }
    }

    /// 按 `unit=` / `other_noun=` 补一组未内联的叶子。同（房间、选择器）已有一条在途（同名 noun 在另一个专业的
    ///「其他构件」下）时只把这个节点挂进在途集一起等，不再发第二条 HTTP——回包落地时 `loading_nodes_for` 把两个都找回来。
    fn ensure_room_leaves(&mut self, id: RoomNodeId, selector: LeafSelector) {
        let room = id.room();
        if self.room_tree.loading.contains(&id) || !self.room_tree.is_fetched(room) {
            return;
        }
        let in_flight = self.room_tree.leaves_in_flight(room, &selector);
        self.room_tree.loading.insert(id.clone());
        self.room_tree.clear_error(&id);
        if in_flight {
            return;
        }
        if self
            .bridge
            .req
            .send(data::Req::SpatialRoomTree {
                epoch: self.room_tree_epoch,
                room,
                only: Some(selector),
            })
            .is_err()
        {
            self.room_tree.loading.remove(&id);
            self.room_tree
                .set_error(id, "数据查询线程已停止".to_owned());
        }
    }

    /// 房间树上的动作要等数据到齐（计划 D7）：房间根没取过树先取；叶子未内联的组先补（最多 64 组，
    /// 照来源 `collectRefnos`）；到齐了再执行，没到齐先挂着，回包落地时由 `settle_room_tree_deferred` 结算。
    fn run_room_tree_action(&mut self, id: RoomNodeId, action: RoomTreeDeferred) {
        let room = id.room();
        if self.room_tree.needs_tree(room)
            || self.room_tree.loading.contains(&RoomNodeId::Room(room))
        {
            self.ensure_room_tree(room);
            self.room_tree_deferred.push(DeferredRoomTreeAction {
                id,
                action,
                leaves_requested: false,
            });
            return;
        }
        self.run_room_tree_action_on_fetched_tree(id, action);
    }

    /// 树已在手的那一半：叶子未内联的组先补一轮（最多 64 组）再挂起等回包；没有要补的就直接做。
    /// 树刚到时 `settle_room_tree_deferred` 也走这里，所以在收起的房间根上发起的动作同样会先补叶子。
    fn run_room_tree_action_on_fetched_tree(&mut self, id: RoomNodeId, action: RoomTreeDeferred) {
        let pending = self.room_tree.pending_leaf_nodes_under(&id);
        if pending.is_empty() {
            self.execute_room_tree_action(id, action);
            return;
        }
        for (node, selector) in pending.into_iter().take(ROOM_TREE_LEAF_GROUP_CAP) {
            self.ensure_room_leaves(node, selector);
        }
        self.room_tree_deferred.push(DeferredRoomTreeAction {
            id,
            action,
            leaves_requested: true,
        });
    }

    /// 回包落地后过一遍挂着的动作（裁决见 [`deferred_step`]）：还在等的继续挂；等的那一条失败了就作废并说一声；
    /// 树刚到、还没替自己补过叶子的先补一轮；到齐了就做。超过 64 组的尾巴不再补（来源同）：那时候按已知的构件做。
    fn settle_room_tree_deferred(&mut self) {
        let deferred = std::mem::take(&mut self.room_tree_deferred);
        for entry in deferred {
            let room = entry.id.room();
            let root = RoomNodeId::Room(room);
            let pending = self.room_tree.pending_leaf_nodes_under(&entry.id);
            let step = deferred_step(
                entry.leaves_requested,
                self.room_tree.loading.contains(&root),
                self.room_tree.is_fetched(room) || self.room_tree.is_empty_room(room),
                pending
                    .iter()
                    .any(|(node, _)| self.room_tree.loading.contains(node)),
                pending
                    .iter()
                    .any(|(node, _)| self.room_tree.error_of(node).is_some()),
            );
            match step {
                DeferredStep::Wait => self.room_tree_deferred.push(entry),
                DeferredStep::RequestLeaves => {
                    self.run_room_tree_action_on_fetched_tree(entry.id, entry.action)
                }
                DeferredStep::Execute => self.execute_room_tree_action(entry.id, entry.action),
                DeferredStep::Abort => {
                    let message = if !self.room_tree.is_fetched(room)
                        && !self.room_tree.is_empty_room(room)
                    {
                        format!(
                            "房间树：{} 上的动作作废——{}",
                            room.to_slash_string(),
                            self.room_tree.error_of(&root).unwrap_or("树没取回来")
                        )
                    } else {
                        let failed = pending
                            .iter()
                            .find(|(node, _)| self.room_tree.error_of(node).is_some());
                        let label = failed
                            .and_then(|(node, _)| self.room_tree.node(node))
                            .map(|node| node.label.clone())
                            .unwrap_or_default();
                        let reason = failed
                            .and_then(|(node, _)| self.room_tree.error_of(node))
                            .unwrap_or_default();
                        format!("房间树：{label} 的叶子没补回来，这一支上的动作作废——{reason}")
                    };
                    self.logs.warn(&mut self.vm.logs, message);
                }
            }
        }
    }

    /// 数据到齐之后真正执行：全部落到既有的 `ModelAction` / `set_model_visible` 上，没有第二套显隐路径。
    /// 单元级及以上连所属 BRAN 整条（方案 A）；构件行不扩。
    fn execute_room_tree_action(&mut self, id: RoomNodeId, action: RoomTreeDeferred) {
        let label = self
            .room_tree
            .node(&id)
            .map(|node| node.label.clone())
            .unwrap_or_else(|| id.room().to_slash_string());
        match action {
            RoomTreeDeferred::SetVisible(visible) => {
                // 直段行只动这一段（阶段四），不扩到所属 BRAN。
                if let Some((bran, segment)) = id.tube_segment() {
                    self.set_tube_segment_visible(bran, segment, visible, &label);
                    return;
                }
                let refnos = self.room_tree.scene_refnos_under(&id);
                if refnos.is_empty() {
                    self.logs
                        .info(&mut self.vm.logs, format!("房间树：{label} 下没有构件"));
                    return;
                }
                self.set_model_visible(refnos, visible);
            }
            RoomTreeDeferred::Isolate => {
                let refnos = self.room_tree.scene_refnos_under(&id);
                if refnos.is_empty() {
                    self.logs.info(
                        &mut self.vm.logs,
                        format!("房间树：{label} 下没有构件，没什么可隔离"),
                    );
                    return;
                }
                // 正在房间视图里就先摘徽章 / 撤房间体（来源 `exitRoomView({ keepIsolation })`），隔离换成这一支。
                self.leave_room_view();
                self.view3d_commands
                    .push(Cmd::Model(plant_ui::ModelAction::Isolate {
                        refnos,
                        matching: IsolateMatching::RefnoOrOwner,
                    }));
            }
            RoomTreeDeferred::Focus => {
                if let Some(aabb) = self
                    .room_tree
                    .node(&id)
                    .and_then(|node| node.tube.as_ref())
                    .map(|tube| tube.aabb)
                {
                    // 直段按服务端给的世界盒飞，不要求已加载。
                    // 这里只是**执行**点：作废在途查询与撤旧取景在挂起那一刻（意图点）就做过了，
                    // 晚执行时不能再作废——那会把挂起期间更新的意图一起吞掉。
                    self.view3d_commands
                        .push(Cmd::Model(plant_ui::ModelAction::FocusBounds {
                            min_mm: [aabb.min[0] as f32, aabb.min[1] as f32, aabb.min[2] as f32],
                            max_mm: [aabb.max[0] as f32, aabb.max[1] as f32, aabb.max[2] as f32],
                        }));
                    return;
                }
                // 构件 / 分组按已加载几何的并集盒；一个都没加载就说一声，相机不动。
                let loaded: Vec<RefU64> = self
                    .room_tree
                    .scene_refnos_under(&id)
                    .into_iter()
                    .filter(|refno| self.loaded_models.contains(refno))
                    .collect();
                if loaded.is_empty() {
                    self.logs.info(
                        &mut self.vm.logs,
                        format!("房间树：{label} 下还没有已加载几何的构件，先「加载模型」再聚焦"),
                    );
                    return;
                }
                // 同上面那条 FocusBounds：执行点不作废，意图点已经作废过了。
                self.view3d_commands
                    .push(Cmd::Model(plant_ui::ModelAction::FocusGroup {
                        refnos: loaded,
                    }));
            }
            RoomTreeDeferred::LoadModels { confirmed, frame } => {
                let refnos = self.room_tree.refnos_under(&id);
                if refnos.is_empty() {
                    self.logs
                        .info(&mut self.vm.logs, format!("房间树：{label} 下没有构件"));
                    return;
                }
                if !confirmed && refnos.len() > LARGE_LOAD_CONFIRM_THRESHOLD {
                    self.room_tree_confirm = Some(RoomTreeConfirmVm {
                        id,
                        label,
                        count: refnos.len(),
                    });
                    return;
                }
                // 加载 = 显示这些构件（来源：加载链按生成根走，不扩 BRAN），范围回包落地后再取景。
                self.logs.info(
                    &mut self.vm.logs,
                    format!("房间树：加载 {label} 下 {} 个构件的模型", refnos.len()),
                );
                // `frame` 是挂起那一刻记下的：期间用户发起过别的取景意图，这条就只显示不取景。
                // 只许**写**、不许清：`frame=false` 时 `room_tree_frame` 里可能正躺着更新那次
                // 「加载模型」登记的账，这里写成 None 就成了执行点作废更新的意图。
                if frame {
                    self.room_tree_frame = Some(refnos.clone());
                }
                self.set_model_visible(refnos, true);
                self.settle_room_tree_frame();
            }
        }
    }

    /// 直段行的眼睛（ADR-0033 阶段四）：只动这一段，落到 `ModelAction::SetTubeSegmentsVisible`。这段还没以
    /// 身份进场景（所属 BRAN 没显示过，或服务端这批记录没带 `tube`）就说一声、不记方向——没有实体可落，
    /// 方向只会一直挂着。之后对整根 BRAN 的显隐仍按 refno 整根生效，把逐段结果盖掉（回执会把账对回来）。
    fn set_tube_segment_visible(
        &mut self,
        bran: RefU64,
        segment: plant_ui::TubeSegment,
        visible: bool,
        label: &str,
    ) {
        let loaded = self
            .tube_visibility
            .get(&bran)
            .is_some_and(|segments| segments.contains_key(&segment));
        if !loaded {
            let hint = if self.loaded_models.contains(&bran) {
                "这根 BRAN 的记录没带直段身份（老服务端 / 库供数），只能整根显隐"
            } else {
                "先对所属 BRAN「显示」把直段装进场景，再逐段显隐"
            };
            self.logs.info(
                &mut self.vm.logs,
                format!("房间树：直段 {label} 还不能单独显隐——{hint}"),
            );
            return;
        }
        self.tube_pending
            .entry(bran)
            .or_default()
            .insert(segment.clone(), visible);
        self.view3d_commands
            .push(Cmd::Model(plant_ui::ModelAction::SetTubeSegmentsVisible {
                bran,
                segments: vec![segment],
                visible,
            }));
    }

    /// 「加载模型」的取景：目标的范围查询都回来了才 `FocusGroup`（视口的包围盒来自范围回包的
    /// `world_aabb`）。一个都没查出模型就不动相机，账一样销掉。
    fn settle_room_tree_frame(&mut self) {
        let Some(targets) = &self.room_tree_frame else {
            return;
        };
        if targets
            .iter()
            .any(|target| self.model_scope_pending.contains(target))
        {
            return;
        }
        let targets = self.room_tree_frame.take().unwrap_or_default();
        let mut refnos: Vec<RefU64> = Vec::new();
        let mut seen: HashSet<RefU64> = HashSet::new();
        for target in targets {
            for model in self.model_scopes.get(&target).into_iter().flatten() {
                if seen.insert(*model) {
                    refnos.push(*model);
                }
            }
        }
        if !refnos.is_empty() {
            self.view3d_commands
                .push(Cmd::Model(plant_ui::ModelAction::FocusGroup { refnos }));
        }
    }

    /// 「在房间树查看」一间房（「房间」页签过来）：切到房间树页签、取它的树、展开到第一层、滚到它。
    /// 根清单还没到就先去拉，到了再定位；不在册就在日志里说一句。
    fn reveal_room_in_tree(&mut self, room: RefU64) {
        self.state.focus(Pane::RoomTree);
        if !matches!(self.room_tree.status, RootsStatus::Ready { .. }) {
            self.room_tree_reveal_room = Some(room);
            self.room_tree_load(false);
            self.rebuild_room_tree();
            return;
        }
        match self.room_tree.reveal_room(room) {
            Some(id) => {
                self.room_tree_reveal = Some(id);
                self.ensure_room_tree(room);
            }
            None => self.logs.warn(
                &mut self.vm.logs,
                format!(
                    "房间树：{} 不在服务端的在册房间清单里",
                    room.to_slash_string()
                ),
            ),
        }
        self.rebuild_room_tree();
    }

    /// 每帧末尾结算房间树的待滚动：行已经画出来了就把请求收掉，还没有就留着（同 `settle_tree_reveal`）。
    fn settle_room_tree_reveal(&mut self) {
        let Some(id) = &self.room_tree_reveal else {
            return;
        };
        let shown = matches!(&self.vm.room_tree, RoomTreeVm::Ready(data) if data.rows.iter().any(|row| &row.id == id));
        if shown {
            self.room_tree_reveal = None;
            self.rebuild_room_tree();
        }
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
            let is_uda = attr.is_uda;
            let row = PropRowVm {
                key: attr.name.clone(),
                attr: attr.name,
                value: attr.value,
                kind: prop_kind(attr.kind),
                muted,
            };
            match row.attr.as_str() {
                _ if is_uda => data.udas.push(row),
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

    /// 视图适应（FitAll / Home 键 / 视方块的 Home）算一次取景意图，但**房间取景账要留住**。
    ///
    /// 房间视图里 Home 对准的框就是房间体（`room_clip.rs` 的 `panel_bounds_mm`，与房间体落地
    /// 取景同源），所以它不跟房间取景抢相机，反而是替用户把那次没落地的取景提前兑现。撤掉
    /// 房间账就等于「进了房间却永远不取景」。房间体还在途时先框隔离后的并集，账留着，落地时
    /// 再纠正到房间体——终态一致，只是相机动两次。
    ///
    /// 补 `focus_room_pending` 是兜住「frame 已经没了、进视图那半还在途」的两种来法：
    /// (i) 进 R 之后别的意图跑了普通 helper（它清 frame、不碰 pending）；
    /// (ii) 同一间房连点两次，旧帧号的房间体先把新点击登记的账吃掉。
    /// 两种情况下补回 pending 都让「进 R → 定位 → Home」等同于「进 R → Home」；不补的话，
    /// Home 在隔离前框全场景，隔离后房间很小，也没人再取景。
    ///
    /// 视方块点面（`SnapView { fit: false }`）只改朝向，不动焦点与距离，不算取景意图。
    fn supersede_camera_intents_for_fit(&mut self) {
        let room_frame = self.focus_room_frame.take().or(self.focus_room_pending);
        self.supersede_camera_intents();
        self.focus_room_frame = room_frame;
    }

    /// 相机意图只认最新一次：用户又发起了一次取景，把更早的账全撤掉。
    ///
    /// 取景是异步的——范围查询要等回包，孔洞 / 房间模型要等几何落地，房间树动作要等树和叶子
    /// 到齐。这些账各自挂着一个「稍后取景」，互不相识；不管的话，晚落地的那笔会把相机从用户
    /// 最新那一下上拽走，日志里还多一条与当前画面无关的「已定位到 N 个模型」。
    ///
    /// 所以：每个**意图点**（用户刚点下去那一处）先调这个，再登记自己的取景。**执行点 / 落地点**
    /// 不调——那时手上这笔才是最新意图，反过来作废别人就变成吞掉用户操作了。
    ///
    /// 不碰 `focus_room_pending`（进不进视图由详情回包自己认领，页签与历史已经切过去了，
    /// 在这里清掉会让状态劈叉）与 `focus_scope_pending`（它到期时会自己问 `accepts_bounds`，
    /// 被作废即静默丢弃）。房间视图的**取景**账 `focus_room_frame` 归这里撤：晚到的房间体
    /// 照旧隔离截断挂 HUD，只是不再动相机。
    fn supersede_camera_intents(&mut self) {
        self.focus_bounds_request.cancel();
        self.hole_focus_pending = None;
        self.pending_room_frame = None;
        self.room_tree_frame = None;
        self.focus_room_frame = None;
        // 挂着的房间树取景：整条丢掉（它只为取景而挂）；挂着的「加载模型」只留显示，不再取景。
        self.room_tree_deferred
            .retain(|deferred| deferred.action != RoomTreeDeferred::Focus);
        for deferred in &mut self.room_tree_deferred {
            if let RoomTreeDeferred::LoadModels { frame, .. } = &mut deferred.action {
                *frame = false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AutoRefresh, CleanupOutcome, CleanupUnload, EMPTY_SCOPE_HINT, EleTreeNode, GenerateOutcome,
        ModelVisibility, ModelVisibilityPlan, PanelRoomResolution, RefU64, ReloadSnapshot,
        RowVisibility, TreeModel, TreeRowVm, Window, auto_refresh, background_models_settled,
        begin_get_work, cache_model_scope, claim_unloaded_model_refnos,         cleanup_outcome,
        cleanup_unload_plan,
        dbnum_unload_set, command_reply_is_current, complete_refresh_generation,
        empty_props_verdict, expand_room_model_targets,
        finished_after, generate_outcome, generate_summary, in_mdb_path, is_panel_noun,
        mark_model_scope_unavailable, model_drains_idle, model_progress_terminal,
        model_source_changed_line, model_visibility_plan, needs_children_query, read_face_switch,
        refresh_anchor, regenerate_label, reload_snapshot, resolve_panel_room_reply,
        restore_model_reload, scope_not_generated, settle_pending_directions,
        settle_refresh_generation, settled_pending_roots, sync_ime_window, take_hidden_for_replay,
        task_matches_project, task_queue, tree_label, viewport_fits_window,
    };
    // 房间视图 · 直段截断（2026-09-29 计划 M4）的纯函数与编排账。
    use super::{
        IsolateMatching, RoomViewCtx, TruncationVm, room_view_fallback_focus, room_view_isolate,
        room_volume_is_current, truncation_summary, truncation_vm,
    };
    use crate::model_update_api;
    use chrono::{TimeZone, Utc};
    use plant_ui::model_update::PendingModelUnit;
    use std::collections::{HashMap, HashSet};

    /// 浏览器里 egui 头几帧按缺省的 10000×10000 布局，绘制层会要一张比窗口还大的渲染目标；这种请求不交给视口，
    /// 等真尺寸。窗口尺寸还没有时不拦。
    #[test]
    fn only_viewport_sizes_that_fit_the_window_reach_the_viewport() {
        let window = Some(bevy::prelude::UVec2::new(2379, 1325));
        assert!(viewport_fits_window([1463, 767], window));
        assert!(
            viewport_fits_window([2379, 1325], window),
            "正好占满也算装得下"
        );
        assert!(
            !viewport_fits_window([6154, 6839], window),
            "缺省布局的虚尺寸"
        );
        assert!(
            !viewport_fits_window([2400, 1500], window),
            "按起始分辨率而不是真窗口算的那一帧"
        );
        assert!(
            viewport_fits_window([6154, 6839], None),
            "没有窗口尺寸就放行"
        );
    }

    /// 地址栏的 `look_demo` / `look_debug`：取第一个同名键的值；`1` 演示厂、`far` 连远景检验件，别的值当没写。
    #[test]
    fn look_demo_and_look_debug_come_from_the_query_string() {
        use plant_ui_view3d::e3d_look::post::SglLookDebugView;

        assert_eq!(
            super::query_value("?look_demo=far&look_debug=edges", "look_demo").as_deref(),
            Some("far")
        );
        assert_eq!(
            super::query_value("look_debug=edges", "look_debug").as_deref(),
            Some("edges")
        );
        assert_eq!(super::query_value("?look=pbr", "look_debug"), None);
        assert_eq!(
            super::query_value("?look_debug", "look_debug"),
            None,
            "没有等号当没写"
        );
        assert!(!super::look_demo_from("1").unwrap().far);
        assert!(super::look_demo_from("far").unwrap().far);
        assert!(super::look_demo_from("yes").is_none());
        assert_eq!(
            super::query_value("?look_debug=depth_error", "look_debug")
                .and_then(|value| SglLookDebugView::parse(&value)),
            Some(SglLookDebugView::DepthError)
        );
    }

    /// 房间树挂着的动作在回包落地后的去向：树还在途 → 等；树没回来 → 作废；树刚到而这条动作还没替自己补过叶子 →
    /// 先补一轮（在收起的房间根上发起的动作就是这种，不补就只按已内联的那一小部分做）；补过一轮后叶子在途 → 等、
    /// 有失败 → 作废、都到齐 → 做。
    #[test]
    fn a_deferred_room_tree_action_requests_leaves_before_it_runs() {
        use super::{DeferredStep, deferred_step};
        assert_eq!(deferred_step(false, true, false, false, false), DeferredStep::Wait);
        assert_eq!(deferred_step(true, true, true, false, false), DeferredStep::Wait);
        assert_eq!(deferred_step(false, false, false, false, false), DeferredStep::Abort);
        assert_eq!(
            deferred_step(false, false, true, false, false),
            DeferredStep::RequestLeaves,
            "树到了但没补过叶子：先补一轮，不能直接做"
        );
        assert_eq!(
            deferred_step(false, false, true, true, true),
            DeferredStep::RequestLeaves,
            "没补过叶子时在途 / 失败都由补那一轮去处理（失败的组会重发）"
        );
        assert_eq!(deferred_step(true, false, true, true, false), DeferredStep::Wait);
        assert_eq!(deferred_step(true, false, true, false, true), DeferredStep::Abort);
        assert_eq!(deferred_step(true, false, true, false, false), DeferredStep::Execute);
    }

    /// 虚拟孔洞等目标落地的裁决：范围没解出且在查 → 等；查空 → 作废；解出了但网格还在装 → 等；任一模型画出来 → 做；
    /// 全部 settled 却一个都没画出来 → 作废（网格失败 / 没有几何分开说）。原来只认 `mesh_loaded > 0`，其余情形行永远「已就绪」。
    #[test]
    fn a_hole_target_without_geometry_is_failed_instead_of_waited_for() {
        use super::{HoleTargetVerdict, hole_target_verdict};
        let a = RefU64(11);
        let b = RefU64(12);
        let scope = vec![a, b];
        let state = |loaded: usize, failed: usize| ModelVisibility {
            visible: true,
            mesh_loaded: loaded,
            mesh_failed: failed,
        };
        let empty: HashMap<RefU64, ModelVisibility> = HashMap::new();
        assert_eq!(hole_target_verdict(true, false, None, &empty), HoleTargetVerdict::Waiting);
        assert_eq!(
            hole_target_verdict(false, false, None, &empty),
            HoleTargetVerdict::Waiting,
            "范围还没问过（或默认重新生成在跑）：等"
        );
        assert!(matches!(
            hole_target_verdict(false, true, None, &empty),
            HoleTargetVerdict::Unavailable(_)
        ));
        assert_eq!(
            hole_target_verdict(false, false, Some(&scope), &empty),
            HoleTargetVerdict::Waiting,
            "解出了模型、网格还在装"
        );
        let half = HashMap::from([(a, state(0, 2))]);
        assert_eq!(
            hole_target_verdict(false, false, Some(&scope), &half),
            HoleTargetVerdict::Waiting,
            "另一个模型还没 settled"
        );
        let loaded = HashMap::from([(a, state(0, 2)), (b, state(1, 0))]);
        assert_eq!(
            hole_target_verdict(false, false, Some(&scope), &loaded),
            HoleTargetVerdict::Loaded
        );
        let all_failed = HashMap::from([(a, state(0, 2)), (b, state(0, 1))]);
        assert_eq!(
            hole_target_verdict(false, false, Some(&scope), &all_failed),
            HoleTargetVerdict::Unavailable("的网格文件没加载成功")
        );
        let no_geometry = HashMap::from([(a, state(0, 0)), (b, state(0, 0))]);
        assert_eq!(
            hole_target_verdict(false, false, Some(&scope), &no_geometry),
            HoleTargetVerdict::Unavailable("没有可切的几何")
        );
        assert_eq!(
            hole_target_verdict(true, true, Some(&scope), &loaded),
            HoleTargetVerdict::Loaded,
            "已经画出来的不看范围那两位"
        );
    }

    /// 「定位目标」先显示再取景：`hole_target_verdict` 只看 `mesh_loaded`、不看 `visible`，
    /// 目标装过但被眼睛藏着时它是 `Loaded`——只发 `Focus` 会让相机飞向一个看不见的东西。
    /// 钉住分支内的顺序（显示 → 裁决 → 取景 / 挂起），并钉住 `else` 里不再重复显示。
    #[test]
    fn focusing_a_hole_target_shows_it_before_focusing() {
        let source = include_str!("main.rs");
        let branch = source
            .split_once("Cmd::FocusVirtualHoleTarget(id) =>")
            .expect("定位目标分支")
            .1
            .split_once("Cmd::ShowVirtualHoleOriginal(on) =>")
            .expect("下一个分支")
            .0;
        let show = "self.set_model_visible(vec![target], true);";
        assert!(branch.contains(show), "先显示目标：{branch}");
        let show_at = branch.find(show).expect("显示语句");
        let verdict_at = branch.find("hole_target_verdict(").expect("裁决");
        assert!(show_at < verdict_at, "显示要在裁决之前：{branch}");
        let else_part = branch
            .split_once("} else {")
            .expect("未加载分支")
            .1;
        assert!(
            !else_part.contains("set_model_visible"),
            "`else` 里不该再显示一次：{else_part}"
        );
        assert!(
            else_part.contains("self.hole_focus_pending = Some(target);"),
            "未加载的目标挂起等几何落地：{else_part}"
        );
    }

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
    fn panel_room_lookup_is_only_for_panel_nodes() {
        assert!(is_panel_noun("PANE"));
        assert!(is_panel_noun("pane"));
        assert!(!is_panel_noun("BRAN"));
        assert!(!is_panel_noun("EQUI"));
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
        let scopes = HashMap::from([(branch, vec![member, branch]), (unrelated, vec![unrelated])]);

        assert_eq!(
            expand_room_model_targets(vec![member], &scopes),
            vec![member, branch]
        );
    }

    fn room_detail(panels: &[u64], members: &[u64], carriers: &[u64]) -> plant_ui_data::room::RoomDetail {
        let refnos = |ids: &[u64]| ids.iter().map(|id| RefU64(*id)).collect::<Vec<_>>();
        plant_ui_data::room::RoomDetail {
            room: RefU64(1),
            name: "/1RX-RM03-R301".into(),
            room_num: "R301".into(),
            room_code: None,
            panels: refnos(panels),
            member_count: members.len(),
            member_refnos: refnos(members),
            members: Vec::new(),
            pending_recalc: false,
            tube_carriers: refnos(carriers),
        }
    }

    /// 截断开：只认 refno，成员 + 截断候选；同一根分支上墙外的阀门不跟进来，面板也不进
    /// （房间体图层画墙）。截断关：原来的口径，经已加载范围展开、owner 命中也算。
    /// 房间体取不到时面板照旧进来。
    #[test]
    fn a_truncating_room_view_isolates_exactly_its_members_and_carriers() {
        let (panel, elbo, bran, valve) = (RefU64(20), RefU64(10), RefU64(11), RefU64(12));
        let detail = room_detail(&[20], &[10], &[11]);
        let scopes = HashMap::from([(bran, vec![elbo, bran, valve])]);

        assert_eq!(
            room_view_isolate(&detail, true, &scopes, false),
            plant_ui::ModelAction::Isolate {
                refnos: vec![elbo, bran],
                matching: IsolateMatching::Exact,
            }
        );
        assert_eq!(
            room_view_isolate(&detail, false, &scopes, false),
            plant_ui::ModelAction::Isolate {
                refnos: vec![elbo, bran, valve],
                matching: IsolateMatching::RefnoOrOwner,
            }
        );
        let plant_ui::ModelAction::Isolate { refnos, .. } =
            room_view_isolate(&detail, true, &scopes, true)
        else {
            panic!("房间视图发的是隔离");
        };
        assert_eq!(refnos, vec![panel, elbo, bran]);
        // 兜底取景：面板在前、成员在后，经已加载范围展开。
        assert_eq!(
            room_view_fallback_focus(&detail, &scopes),
            vec![panel, elbo, bran, valve]
        );
    }

    /// 房间体回包帧号与房间都对上才认：连点两间房只认最后一间，重装后重取的旧回包也丢。
    #[test]
    fn room_volume_replies_must_match_epoch_and_room() {
        let ctx = RoomViewCtx {
            room: RefU64(1),
            detail: room_detail(&[20], &[10], &[]),
            epoch: 5,
            volume_failed: false,
            announced: None,
        };
        assert!(room_volume_is_current(&ctx, 5, RefU64(1)));
        assert!(!room_volume_is_current(&ctx, 4, RefU64(1)));
        assert!(!room_volume_is_current(&ctx, 5, RefU64(2)));
    }

    /// 视口回执照原样进 HUD；做完那一句日志说清各几段，「未能截断」只在有的时候说。
    #[test]
    fn truncation_receipts_map_onto_the_hud_and_the_log() {
        use plant_ui_view3d::{RoomClipPhase, RoomClipStatus};
        let status = |phase, skipped| RoomClipStatus {
            generation: 1,
            truncate: true,
            phase,
            cut: 3,
            hidden: 5,
            whole: 7,
            skipped,
        };
        assert_eq!(
            truncation_vm(&status(RoomClipPhase::Ready, 0)),
            TruncationVm::Ready {
                cut: 3,
                hidden: 5,
                whole: 7,
                skipped: 0
            }
        );
        assert_eq!(
            truncation_vm(&status(RoomClipPhase::Computing { done: 2, total: 9 }, 0)),
            TruncationVm::Computing { done: 2, total: 9 }
        );
        assert_eq!(
            truncation_vm(&status(RoomClipPhase::Unavailable("缺网格".into()), 0)),
            TruncationVm::Unavailable("缺网格".into())
        );
        assert_eq!(
            truncation_summary(&status(RoomClipPhase::Ready, 0)),
            "截断 3 段 · 房间外 5 段不画 · 整段在内 7 段"
        );
        assert!(truncation_summary(&status(RoomClipPhase::Ready, 2)).ends_with(" · 未能截断 2 段"));
    }

    /// 房间体图层有三处必须撤（计划 M4 验收）：退出房间视图、重连 / 换面的复位、房间体兜底
    /// （重装后重取失败时，半截的旧房间体不能冒充这一次的）。漏一处，直段就停在截断的样子，
    /// 半透明房间体也留在画面上。宿主只经 `pending_room_volume` 交给视口，桥上 `Clear` 对到
    /// `clear_room_volume`。
    #[test]
    fn leaving_reconnecting_and_falling_back_all_clear_the_room_volume() {
        let source = include_str!("main.rs");
        let body_of = |head: &str| {
            source
                .split(head)
                .nth(1)
                .unwrap_or_else(|| panic!("找不到 {head}"))
                .split("\n    fn ")
                .next()
                .unwrap()
        };
        for head in [
            "fn leave_room_view(",
            "fn reset_for_reconnect(",
            "fn room_volume_fallback(",
        ] {
            assert!(
                body_of(head).contains("self.pending_room_volume.push(RoomVolumeUpdate::Clear)"),
                "{head} 没有撤房间体"
            );
        }
        let exit = source
            .split("plant_ui::ModelAction::ExitIsolate => {")
            .nth(1)
            .unwrap()
            .split("Cmd::Model(plant_ui::ModelAction::ExitIsolate)")
            .next()
            .unwrap();
        assert!(
            exit.contains("self.leave_room_view()"),
            "退出隔离之前先退出房间视图"
        );
        let (code, _) = source.split_once("\nmod tests {").unwrap();
        assert!(code.contains("RoomVolumeUpdate::Clear => view3d.clear_room_volume()"));
    }

    /// 确认框标题跟着右键落点走，成批时才报数。「/ZONE-A 等 1 项」是废话，
    /// 而它出现的地方正是确认框那句「对 … 重新生成模型？」的主语。
    #[test]
    fn the_regenerate_label_only_counts_a_real_batch() {
        assert_eq!(regenerate_label("ZONE /ZONE-A".into(), 1), "ZONE /ZONE-A");
        assert_eq!(
            regenerate_label("ZONE /ZONE-A".into(), 3),
            "ZONE /ZONE-A 等 3 项"
        );
    }

    fn generate_reply(status: model_update_api::GenerateStatus) -> model_update_api::GenerateReply {
        model_update_api::GenerateReply {
            status,
            generation_root: "24381/100677".into(),
            model_available: true,
            generation_root_count: 3,
            cached_root_count: 1,
            generated_root_count: 2,
            failed_root_count: 0,
            failed_roots: vec![],
            model_source: None,
            model_source_reason: None,
            legacy_endpoint: false,
        }
    }

    fn api_failure(code: &str, message: &str) -> anyhow::Error {
        anyhow::Error::new(model_update_api::ApiError(
            plant_ui::model_update::Failure::new(code, message),
        ))
    }

    /// 右键：确认之后恰好一发 `generate(force = true)`（每个目标一发，不清点、不解根、不删产物），
    /// 成功回执之后对同一批目标发显示（ADR-0030 实施约束 3）。这里钉三件事：数据线程那一臂
    /// 只调 `generate_model(.., true, ..)`；确认那一步只发 `Req::GenerateModels`；回执落地后成功的
    /// 目标进 `regenerate_refresh` 并经 `set_model_visible(.., true)` 走显示那条路。
    #[test]
    fn regenerate_generates_once_then_displays_the_targets() {
        let data = include_str!("data.rs");
        let lane = data
            .split_once("Req::GenerateModels {")
            .expect("生成那一臂")
            .1
            .split_once("Req::CleanupModels {")
            .expect("清理那一臂紧随其后")
            .0;
        assert_eq!(lane.matches("generate_model(").count(), 1, "{lane}");
        assert!(
            lane.contains("&base, &slash, force,"),
            "force 由发起方定（右键 true、显示补齐 false）：{lane}"
        );
        // 生成那一臂不删：删旧是服务端 `force = true` 里的事，客户端的 DELETE 只属于清理那一臂。
        assert!(!lane.contains("delete_model_subtree"), "{lane}");
        for forbidden in ["regeneration_count", "regeneration_roots"] {
            assert!(!data.contains(forbidden), "旧流水还在：{forbidden}");
        }

        let source = include_str!("main.rs");
        let confirm = source
            .split_once("fn settle_mutation_confirm(&mut self, accepted: bool, purge_meshes: bool) {")
            .expect("确认那一步")
            .1
            .split_once("fn settle_models_generated(")
            .expect("回执那一步")
            .0;
        // 确认步四种请求：右键生成、右键清理、整库清理、全库重建房间。
        assert_eq!(confirm.matches("data::Req::").count(), 4, "{confirm}");
        assert!(confirm.contains("data::Req::GenerateModels {"));
        assert!(
            confirm.contains("force: true,"),
            "右键先删后生成：{confirm}"
        );
        assert!(confirm.contains("data::Req::CleanupModels {"));
        assert!(confirm.contains("data::Req::CleanupDbnumModel {"));
        assert!(confirm.contains("data::Req::RebuildRooms {"));
        assert!(
            !confirm.contains("RegenerateScope") && !confirm.contains("RegenerateDelete"),
            "清点 / 整片删已退役"
        );
        // 「一并删除网格文件」那一格落在两个清理档上：右键清理与整库清理的请求里都带上它，
        // 重新生成那一档不带（它走 `force = true`）。
        let cleanup_request = confirm
            .split_once("(model_mutation::Kind::Cleanup, None) => data::Req::CleanupModels {")
            .expect("右键清理那一发")
            .1
            .split_once("},")
            .expect("请求体结束")
            .0;
        assert!(
            cleanup_request.contains("purge_meshes,"),
            "框上那一格要原样进请求：{cleanup_request}"
        );
        let dbnum_request = confirm
            .split_once("(_, Some(dbnum)) => data::Req::CleanupDbnumModel {")
            .expect("整库清理那一发")
            .1
            .split_once("},")
            .expect("请求体结束")
            .0;
        assert!(
            dbnum_request.contains("purge_meshes,"),
            "整库那一档今天也收这一格：{dbnum_request}"
        );
        assert!(
            confirm.contains("purge_meshes && kind == model_mutation::Kind::Cleanup;"),
            "只有两个清理档消费它（重新生成那一档丢掉）：{confirm}"
        );
        let begin = source
            .split_once("fn begin_mutation(&mut self, kind: model_mutation::Kind, targets: Vec<RefU64>) {")
            .expect("开框那一步")
            .1
            .split_once("fn begin_dbnum_model_cleanup(")
            .expect("整库清理开框")
            .0;
        assert!(
            begin.contains("purge_meshes: matches!(kind, model_mutation::Kind::Cleanup),"),
            "默认勾上（只有清理那一档画它）：{begin}"
        );
        let dbnum_begin = source
            .split_once("fn begin_dbnum_model_cleanup(&mut self, dbnum: u32) {")
            .expect("整库清理开框那一步")
            .1
            .split_once("fn begin_room_rebuild(")
            .expect("下一个开框")
            .0;
        assert!(
            dbnum_begin.contains("purge_meshes: true,"),
            "整库那一档也默认勾上：{dbnum_begin}"
        );
        assert!(
            confirm.contains("另把这片范围引用过的网格文件从磁盘删掉")
                && confirm.contains("另把这个库引用过的网格文件从磁盘删掉"),
            "两个清理档开跑那一行都要说清勾了之后多删什么：{confirm}"
        );

        let settle = source
            .split_once("fn settle_models_generated(")
            .expect("回执那一步")
            .1
            .split_once("fn settle_models_cleaned(")
            .expect("下一个方法")
            .0;
        let refresh = settle
            .find("self.regenerate_refresh.insert(*target);")
            .expect("成功的目标登记成待替换");
        let show = settle
            .find("self.set_model_visible(display, true);")
            .expect("再调显示接口");
        assert!(refresh < show, "先登记再显示");
        assert!(
            !settle.contains("get_work"),
            "显示走眼睛那条路，不再整场取回工作重装：{settle}"
        );

        assert_eq!(
            generate_outcome(&Ok(generate_reply(
                model_update_api::GenerateStatus::Generated
            ))),
            GenerateOutcome::Display
        );
        assert_eq!(
            generate_outcome(&Ok(generate_reply(
                model_update_api::GenerateStatus::NoRenderableGeometry
            ))),
            GenerateOutcome::Display,
            "没有可画几何也是做完了：显示回包为空会把旧产物卸掉"
        );
        let line = generate_summary(
            "ZONE /ZONE-A",
            &generate_reply(model_update_api::GenerateStatus::Generated),
        );
        assert!(
            line.contains("3 个生成根") && line.contains("重算 2") && line.contains("命中 1"),
            "{line}"
        );
        assert!(!line.contains("没有可用模型"), "{line}");
        let hollow = model_update_api::GenerateReply {
            model_available: false,
            ..generate_reply(model_update_api::GenerateStatus::Generated)
        };
        let line = generate_summary("ZONE /ZONE-A", &hollow);
        assert!(
            line.contains("没有可用模型") && line.contains("24381/100677"),
            "{line}"
        );
        let line = generate_summary(
            "ZONE /ZONE-A",
            &generate_reply(model_update_api::GenerateStatus::Unknown),
        );
        assert!(line.contains("不认识的状态"), "{line}");
    }

    /// 库号落表与继承：节点自带的 `dbnum` 是直接答案；没注的行沿 `parent` 链往上
    /// 继承（SITE 根那一跳总有库号）；`dbnum = 0` / 缺失按「说不出」处理，不造号。
    #[test]
    fn dbnum_of_walks_the_parent_chain() {
        let site = RefU64(1);
        let zone = RefU64(2);
        let leaf = RefU64(3);
        let mut tree = TreeModel::default();
        let mut site_node = node(site, "SITE", 1);
        site_node.dbnum = Some(7997);
        tree.note_dbnum(&site_node);
        tree.parent.insert(zone, site);
        tree.parent.insert(leaf, zone);
        assert_eq!(tree.dbnum_of(site), Some(7997), "节点自己带库号");
        assert_eq!(tree.dbnum_of(leaf), Some(7997), "沿父链继承 SITE 的库号");

        let mut zero = node(RefU64(4), "ZONE", 0);
        zero.dbnum = Some(0);
        tree.note_dbnum(&zero);
        assert_eq!(tree.dbnum_of(RefU64(4)), None, "dbnum = 0 不落表");
        assert_eq!(tree.dbnum_of(RefU64(99)), None, "查不到就说不出");
    }

    /// 取回工作与换面都要先排干数据线程的在途请求：发之前先放弃「重新生成」那一趟的补齐等待，
    /// 不然要陪它等到补齐上限（30 分钟）。
    #[test]
    fn get_work_and_face_switch_abandon_the_generate_wait_first() {
        let source = include_str!("main.rs");
        let get_work = source
            .split_once("fn start_get_work_for_tasks(")
            .expect("取回工作")
            .1
            .split_once("data::Req::GetWork {")
            .expect("发取回工作")
            .0;
        assert!(
            get_work.contains("self.abandon_generate_wait();"),
            "{get_work}"
        );
        let switch = source
            .split_once("fn switch_read_face(")
            .expect("换面")
            .1
            .split_once("data::Req::SwitchReadFace(kind)")
            .expect("发换面")
            .0;
        assert!(switch.contains("self.abandon_generate_wait();"), "{switch}");
        let abandon = source
            .split_once("fn abandon_generate_wait(&self) {")
            .expect("放弃等待")
            .1
            .split_once("fn begin_auto_generate(")
            .expect("下一方法")
            .0;
        assert!(
            abandon.contains("model_mutation::Kind::Regenerate"),
            "只管生成：{abandon}"
        );
    }

    /// 「显示前预检查 dbnum 有没有模型，没有就默认重新生成；节点查空同样重新生成」
    /// （需求口径）。钉住五处接线：显示方向才发探针与默认生成（隐藏只查不生）；库级
    /// 结论 `ready_total == 0` 直接进生成不绕 records；查空先试生成、发不出去才收
    /// 「未加载」；每 refno 每会话只自动一发；自动趟尊重生成期间翻成隐藏的方向。
    #[test]
    fn display_regenerates_when_dbnum_has_no_model() {
        let source = include_str!("main.rs");
        let visible_fn = source
            .split_once("fn set_model_visible(")
            .expect("显示/隐藏入口")
            .1
            .split_once("fn get_work(")
            .expect("下一个方法")
            .0;
        // 探针随显示方向发出：与 ModelScopes 同一条路，按库单飞去重。
        assert!(
            visible_fn.contains("visible && let Some(dbnum) = self.tree.dbnum_of(target)"),
            "探针只在显示方向发：{visible_fn}"
        );
        assert!(
            visible_fn.contains("self.dbnum_model_probing.insert(dbnum)"),
            "每库单飞：{visible_fn}"
        );
        assert!(
            visible_fn.contains("data::Req::DbnumModelProbe {")
                && visible_fn.contains("data::Req::ModelScopes {"),
            "探针与 records 查询同路并发：{visible_fn}"
        );
        // 已知无模型的库直接默认重新生成，只在显示方向；发不出去的退回查询路收口。
        assert!(
            visible_fn.contains("Some(false) if visible => regen_now.push(target)"),
            "隐藏方向不许自动生成：{visible_fn}"
        );
        assert!(
            visible_fn.contains("begin_auto_generate(regen_now"),
            "{visible_fn}"
        );
        let query_fallback = visible_fn
            .find("query.extend(all.into_iter().filter(|target| !fired.contains(target)))");
        assert!(query_fallback.is_some(), "发不出去的退回查询：{visible_fn}");

        // 查空那一臂：先试着默认生成（显示方向 + 没试过的），发不出去才收「未加载」；
        // 重新生成之后仍查空的，把上一趟的旧几何卸掉。
        let empty = source
            .split_once("Ok(loaded) if loaded.records.is_empty() => {")
            .expect("查空那一臂")
            .1
            .split_once("Ok(loaded) => {")
            .expect("非空那一臂")
            .0;
        assert!(
            empty.contains("pending_direction.get(&target).copied() == Some(true)"),
            "只替显示方向生成：{empty}"
        );
        assert!(
            empty.contains("auto_generate_attempted.contains(&target)"),
            "每 refno 每会话只自动一发：{empty}"
        );
        let generate = empty.find("begin_auto_generate").expect("查空默认生成");
        let unavailable = empty
            .find("mark_model_scope_unavailable(")
            .expect("兜底收口");
        assert!(generate < unavailable, "先试着生成，发不出去才收未加载");
        assert!(
            empty.contains("regenerate_refresh.remove(&target)"),
            "重生成后仍空要卸旧几何：{empty}"
        );
        assert!(
            empty.contains("run.targets.contains(&target)"),
            "生成在途的目标不标未加载、不打提示：{empty}"
        );

        // 探针回包：换代的不认；`ready_total == 0` 才催生成，且只替显示方向的待决目标。
        assert!(
            source
                .contains("Evt::DbnumModelProbe { epoch, .. } if epoch != self.model_scope_epoch"),
            "换代的探针回包要丢"
        );
        let probe = source
            .split_once("data::Evt::DbnumModelProbe { dbnum, result, .. } => {")
            .expect("探针回包那一臂")
            .1
            .split_once("data::Evt::ResolvedName")
            .expect("下一臂")
            .0;
        assert!(probe.contains("roots.ready_total > 0"), "就绪判据：{probe}");
        assert!(
            probe.contains("pending_direction.get(target).copied()"),
            "探针只替显示方向的待决目标发起：{probe}"
        );
        assert!(probe.contains("begin_auto_generate("), "{probe}");

        // 自动入口：跳过确认框，走同一条 GenerateModels 通道；发不出去不记账。
        let begin = source
            .split_once("fn begin_auto_generate(")
            .expect("自动入口")
            .1
            .split_once("fn settle_mutation_confirm(")
            .expect("下一方法")
            .0;
        assert!(begin.contains("data::Req::GenerateModels {"), "{begin}");
        assert!(
            begin.contains("force: false,"),
            "显示补齐只补缺口、不先删：{begin}"
        );
        assert!(
            !begin.contains("model_mutation::Vm::Confirm"),
            "自动生成不弹确认框：{begin}"
        );
        let mark = begin.find("auto_generate_attempted.insert(*target)");
        let send = begin.find("data::Req::GenerateModels {");
        assert!(
            mark.is_some() && send.is_some() && mark.unwrap() < send.unwrap(),
            "先记账再发：{begin}"
        );

        // 回执落地：自动趟尊重期间翻成隐藏的方向；显式右键趟照旧一律显示。
        let settle = source
            .split_once("fn settle_models_generated(")
            .expect("回执那一步")
            .1
            .split_once("fn settle_models_cleaned(")
            .expect("下一个方法")
            .0;
        let retain = settle.find("retain(|target| self.tree.pending_direction.get(target)");
        let show = settle.find("self.set_model_visible(display, true);");
        assert!(
            settle.contains("run.auto")
                && retain.is_some()
                && show.is_some()
                && retain.unwrap() < show.unwrap(),
            "自动趟先按方向过滤再显示：{settle}"
        );
        assert!(
            settle.contains("pending_direction.get(target).copied() != Some(false)"),
            "{settle}"
        );
    }

    /// `timeout` 回执（服务端 120 s 预算到点、后台继续）不发显示，其余失败也不发（ADR-0030 实施约束 3）。
    #[test]
    fn a_timed_out_generate_does_not_display() {
        assert_eq!(
            generate_outcome(&Err(api_failure(
                "timeout",
                "按需生成超时(120s)，后台继续执行"
            ))),
            GenerateOutcome::Background
        );
        assert_eq!(
            generate_outcome(&Err(api_failure(
                "conflict",
                "dbnum=7997 已有整库模型任务在运行"
            ))),
            GenerateOutcome::Failed
        );
        assert_eq!(
            generate_outcome(&Err(anyhow::anyhow!("connection refused"))),
            GenerateOutcome::Failed
        );
        let source = include_str!("main.rs");
        let settle = source
            .split_once("fn settle_models_generated(")
            .expect("回执那一步")
            .1
            .split_once("fn settle_models_cleaned(")
            .expect("下一个方法")
            .0;
        let background = settle
            .split_once("GenerateOutcome::Background => {")
            .expect("超时那一档")
            .1
            .split_once("GenerateOutcome::Failed => {")
            .expect("失败那一档")
            .0;
        assert!(background.contains("warn("), "超时是 WARN：{background}");
        assert!(background.contains("后台继续生成"), "{background}");
        assert!(
            !background.contains("display.push"),
            "超时的不进显示名单：{background}"
        );
    }

    /// 右键「清理模型」：确认之后恰好一发 `DELETE /model/subtree`（每个目标一发），回执落地后
    /// 成功的目标**本地卸载**、不重查 records（清理模型计划 D3 / V6：整个流程只有 DELETE）。
    /// 这里钉：数据线程那一臂只调 `delete_model_subtree`；回执那一步不发任何请求、不走显示；
    /// 409 那一档不卸载；超时那一档是 WARN、把目标标成待重查。
    #[test]
    fn cleanup_deletes_once_then_unloads_locally() {
        let data = include_str!("data.rs");
        let lane = data
            .split_once("Req::CleanupModels {")
            .expect("清理那一臂")
            .1
            .split_once("Req::CheckSearchIndex =>")
            .expect("下一臂")
            .0;
        assert_eq!(lane.matches("delete_model_subtree(").count(), 1, "{lane}");
        assert!(!lane.contains("generate_model("), "清理不生成：{lane}");
        assert!(lane.contains("Evt::ModelsCleaned {"), "整批回执：{lane}");

        let source = include_str!("main.rs");
        let settle = source
            .split_once("fn settle_models_cleaned(")
            .expect("回执那一步")
            .1
            .split_once("fn forget_cleaned_scope(")
            .expect("下一个方法")
            .0;
        assert!(!settle.contains("data::Req::"), "清理回执不再发请求：{settle}");
        assert!(
            !settle.contains("set_model_visible"),
            "清理后不走显示那条路：{settle}"
        );
        assert!(settle.contains("ModelAction::Unload {"), "{settle}");
        assert!(!settle.contains("get_work"), "{settle}");
        let cleaned = settle
            .split_once("CleanupOutcome::Cleaned => {")
            .expect("成功那一档")
            .1
            .split_once("CleanupOutcome::Rejected => {")
            .expect("409 那一档")
            .0;
        assert!(cleaned.contains("forget_cleaned_scope("), "{cleaned}");
        assert!(cleaned.contains("重新生成模型"), "日志指路：{cleaned}");
        let rejected = settle
            .split_once("CleanupOutcome::Rejected => {")
            .expect("409 那一档")
            .1
            .split_once("CleanupOutcome::Unknown => {")
            .expect("超时那一档")
            .0;
        assert!(!rejected.contains("forget_cleaned_scope("), "409 不卸载：{rejected}");
        assert!(rejected.contains("error("), "409 是 ERROR：{rejected}");
        let unknown = settle
            .split_once("CleanupOutcome::Unknown => {")
            .expect("超时那一档")
            .1
            .split_once("CleanupOutcome::Failed => {")
            .expect("失败那一档")
            .0;
        assert!(!unknown.contains("forget_cleaned_scope("), "超时不卸载：{unknown}");
        assert!(unknown.contains("warn("), "超时是 WARN：{unknown}");
        assert!(
            unknown.contains("regenerate_refresh.insert("),
            "超时的目标标成待重查：{unknown}"
        );
        assert!(unknown.contains("可能已删一部分"), "{unknown}");

        let reply = model_update_api::CleanupReply {
            scope: "exact_subtree".into(),
            status: "deleted".into(),
            model_source: None,
            model_source_reason: None,
            // 没勾「一并删除网格文件」：三格账全缺席，日志里也不该冒出网格那半句。
            meshes_removed: None,
            meshes_absent: None,
            meshes_failed: None,
        };
        assert_eq!(cleanup_outcome(&Ok(reply.clone())), CleanupOutcome::Cleaned);
        assert_eq!(reply.server_note(), "服务端 exact_subtree");
        // 勾了那一格：删了几个、几个本来就不在、几个失败都要说出来（含「一个都没删到」）。
        let purged = model_update_api::CleanupReply {
            meshes_removed: Some(0),
            meshes_absent: Some(388),
            ..reply.clone()
        };
        assert_eq!(
            purged.server_note(),
            "服务端 exact_subtree；一并删除网格文件 0 个（另 388 个本来就不在盘上）"
        );
        let partly = model_update_api::CleanupReply {
            meshes_removed: Some(386),
            meshes_absent: Some(0),
            meshes_failed: Some(2),
            ..reply.clone()
        };
        assert_eq!(
            partly.server_note(),
            "服务端 exact_subtree；一并删除网格文件 386 个，2 个删失败"
        );
        // 服务端换了状态名、或只清了内存投影：按「删过了」计，但要说出来（宪法 III）。
        let odd = model_update_api::CleanupReply {
            status: "purged".into(),
            model_source: Some(plant_ui::task_queue::ModelSource::Memory),
            model_source_reason: Some("initializing".into()),
            ..reply
        };
        let note = odd.server_note();
        assert!(note.contains("不认识的状态「purged」"), "{note}");
        assert!(note.contains("API 内存里的投影"), "{note}");
        assert_eq!(
            cleanup_outcome(&Err(api_failure(
                "conflict",
                "generation root 24381/100677 is being generated"
            ))),
            CleanupOutcome::Rejected
        );
        assert_eq!(
            cleanup_outcome(&Err(api_failure("timeout", "请求超时"))),
            CleanupOutcome::Unknown
        );
        assert_eq!(
            cleanup_outcome(&Err(api_failure("internal", "boom"))),
            CleanupOutcome::Failed
        );
        assert_eq!(
            cleanup_outcome(&Err(anyhow::anyhow!("connection refused"))),
            CleanupOutcome::Failed
        );
    }

    /// 清理范围只按本端缓存圈（D3）：目标自己 + 祖先链含目标的范围键整条成员 + 链含目标的叶子；
    /// 整库清理只卸回执 `ref0s` 里的已加载叶子，别的库的叶子留着（E7）。
    #[test]
    fn dbnum_unload_set_keeps_only_leaves_in_the_receipt_ref0s() {
        let leaf = |ref0: u32, seq: u32| RefU64(((ref0 as u64) << 32) | seq as u64);
        let here = leaf(24381, 10);
        let also = leaf(24381, 20);
        let other = leaf(31909, 1);
        let loaded = HashSet::from([other, here, also]);
        assert_eq!(dbnum_unload_set(&loaded, &[24381]), vec![here, also]);
        assert!(dbnum_unload_set(&loaded, &[]).is_empty());
    }

    /// 链不在缓存里、又不属于范围内键的叶子不动——猜成在范围内会把别处正看着的模型一并卸掉。
    #[test]
    fn cleanup_unload_plan_covers_the_subtree_and_leaves_strangers_alone() {
        let site = RefU64(1);
        let zone_a = RefU64(2);
        let pipe = RefU64(3);
        let bran_1 = RefU64(4);
        let bran_2 = RefU64(5);
        let zone_b = RefU64(6);
        let bran_3 = RefU64(7);
        // 不在树缓存里的叶子：取回工作之后只剩自映射、父链未知的那种。
        let stranger = RefU64(8);
        // 范围键成员表里记着、但此刻不在场景里（隐藏后被别的路卸了）的叶子。
        let not_loaded = RefU64(9);
        let mut tree = TreeModel {
            roots: vec![node(site, "SITE", 2)],
            ..Default::default()
        };
        tree.parent.extend([
            (zone_a, site),
            (pipe, zone_a),
            (bran_1, pipe),
            (bran_2, pipe),
            (zone_b, site),
            (bran_3, zone_b),
        ]);
        let scopes: HashMap<RefU64, Vec<RefU64>> = [
            (zone_a, vec![bran_1, bran_2, not_loaded]),
            (bran_1, vec![bran_1]),
            (bran_2, vec![bran_2]),
            (zone_b, vec![bran_3]),
            (bran_3, vec![bran_3]),
            (stranger, vec![stranger]),
        ]
        .into_iter()
        .collect();
        let loaded: HashSet<RefU64> = [bran_1, bran_2, bran_3, stranger].into_iter().collect();
        let owners = HashMap::<RefU64, RefU64>::new();
        let plan = |target| {
            cleanup_unload_plan(
                target,
                &scopes,
                &loaded,
                |r| tree.parent.get(&r).copied(),
                &owners,
            )
        };

        // 单根：只有它自己。
        assert_eq!(
            plan(bran_1),
            CleanupUnload {
                refnos: vec![bran_1],
                dropped_scopes: vec![bran_1],
            }
        );
        // 容器：名下两根都卸，自映射键一并摘；没在场景里的成员不进 Unload。
        assert_eq!(
            plan(zone_a),
            CleanupUnload {
                refnos: vec![bran_1, bran_2],
                dropped_scopes: vec![zone_a, bran_1, bran_2],
            }
        );
        // 中间层（自己没有范围键）：靠叶子与子键的祖先链圈出来。
        assert_eq!(
            plan(pipe),
            CleanupUnload {
                refnos: vec![bran_1, bran_2],
                dropped_scopes: vec![bran_1, bran_2],
            }
        );
        // 别的 ZONE 与父链未知的叶子不动。
        for target in [bran_1, zone_a, pipe] {
            let plan = plan(target);
            assert!(!plan.refnos.contains(&bran_3), "{target}: {plan:?}");
            assert!(!plan.refnos.contains(&stranger), "{target}: {plan:?}");
        }
        // 直接清那个父链未知的叶子：它自己当然在范围内。
        assert_eq!(
            plan(stranger),
            CleanupUnload {
                refnos: vec![stranger],
                dropped_scopes: vec![stranger],
            }
        );
        // 整个 SITE：全场。
        assert_eq!(plan(site).refnos, vec![bran_1, bran_2, bran_3]);
    }

    /// BRAN 没展开时 tree.parent 没有那两跳，几何装进来记下的 owner 补上。
    /// 链碰到目标即在范围内；在碰到之前断了的不当成在范围内。
    /// 这里的范围键只有叶子自映射，避免「范围键整条成员」把半链的结论盖掉。
    #[test]
    fn cleanup_unload_plan_follows_a_partial_owner_chain() {
        let site = RefU64(1);
        let zone_a = RefU64(2);
        let pipe = RefU64(3);
        let bran_1 = RefU64(4);
        let bran_2 = RefU64(5);
        let zone_b = RefU64(6);
        let stranger = RefU64(8);
        let mut parent = HashMap::from([(zone_a, site), (pipe, zone_a), (zone_b, site)]);
        let owners = HashMap::from([(bran_1, pipe), (bran_2, pipe)]);
        let scopes: HashMap<RefU64, Vec<RefU64>> = [
            (bran_1, vec![bran_1]),
            (bran_2, vec![bran_2]),
            (stranger, vec![stranger]),
        ]
        .into_iter()
        .collect();
        let loaded: HashSet<RefU64> = [bran_1, bran_2, stranger].into_iter().collect();
        let plan = |target, parent: &HashMap<RefU64, RefU64>| {
            cleanup_unload_plan(
                target,
                &scopes,
                &loaded,
                |r| parent.get(&r).copied(),
                &owners,
            )
        };

        let pipe_plan = plan(pipe, &parent);
        assert_eq!(pipe_plan.refnos, vec![bran_1, bran_2]);
        assert!(!pipe_plan.refnos.contains(&stranger));
        assert!(plan(zone_a, &parent).refnos.contains(&bran_1));
        assert!(!plan(zone_b, &parent).refnos.contains(&bran_1));

        parent.remove(&pipe);
        let stopped = plan(zone_a, &parent);
        assert!(
            !stopped.refnos.contains(&bran_1),
            "链在 PIPE 断了、还没碰到 ZONE，不能猜成在范围内：{stopped:?}"
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

    /// 无名元素默认显示数据侧给的短名，开了「显示全称」才显示整条；有名字的元素没有短名，
    /// 两种口径都是 NAME；老服务端 / 老查询不带短名时退回 `name`，不显示空串。
    #[test]
    fn unnamed_rows_show_the_short_name_unless_full_names_are_on() {
        let mut zone = node(RefU64(7), "ZONE", 0);
        zone.name = "ZONE 4 of SITE 2".into();
        zone.short_name = Some("ZONE 4".into());
        assert_eq!(tree_label(&zone, false), "ZONE 4");
        assert_eq!(tree_label(&zone, true), "ZONE 4 of SITE 2");

        let named = node(RefU64(8), "SITE", 1);
        assert!(named.short_name.is_none(), "有名字的节点数据侧不给短名");
        assert_eq!(tree_label(&named, false), named.name);
        assert_eq!(tree_label(&named, true), named.name);

        zone.short_name = None;
        assert_eq!(tree_label(&zone, false), "ZONE 4 of SITE 2");
        zone.short_name = Some("  ".into());
        assert_eq!(tree_label(&zone, false), "ZONE 4 of SITE 2");
    }

    #[test]
    fn get_work_reports_removed_subtree_and_clears_its_ui_state() {
        let site = RefU64(1);
        let zone = RefU64(2);
        let equipment = RefU64(3);
        let primitive = RefU64(4);
        let mut tree = TreeModel {
            roots: vec![node(site, "SITE", 1)],
            ..Default::default()
        };
        tree.children.insert(site, vec![node(zone, "ZONE", 1)]);
        tree.children.insert(zone, vec![node(equipment, "EQUI", 1)]);
        tree.children
            .insert(equipment, vec![node(primitive, "BOX", 0)]);
        tree.parent
            .extend([(zone, site), (equipment, zone), (primitive, equipment)]);
        tree.expanded.extend([site, zone, equipment]);
        tree.visibility.insert(
            primitive,
            ModelVisibility {
                visible: true,
                mesh_loaded: 1,
                mesh_failed: 0,
            },
        );
        tree.pending_direction.insert(equipment, true);
        tree.pending_visibility
            .insert(equipment, RowVisibility::Shown);
        tree.visibility_unavailable.insert(primitive);

        // GET WORK says the equipment is no longer a child of the zone; its cached BOX layer is
        // still present until prune_unreachable walks reachability from SITE.
        tree.children.insert(zone, Vec::new());
        let removed = tree.prune_unreachable();

        assert_eq!(removed, vec![equipment, primitive]);
        assert!(!tree.children.contains_key(&equipment));
        assert!(!tree.parent.contains_key(&equipment));
        assert!(!tree.parent.contains_key(&primitive));
        assert!(!tree.expanded.contains(&equipment));
        assert!(!tree.visibility.contains_key(&primitive));
        assert!(!tree.pending_direction.contains_key(&equipment));
        assert!(!tree.pending_visibility.contains_key(&equipment));
        assert!(!tree.visibility_unavailable.contains(&primitive));
    }

    #[test]
    fn get_work_missing_branch_removes_descendants_without_refreshing_parent() {
        let site = RefU64(1);
        let zone = RefU64(2);
        let equipment = RefU64(3);
        let primitive = RefU64(4);
        let sibling = RefU64(5);
        let mut tree = TreeModel {
            roots: vec![node(site, "SITE", 1)],
            ..Default::default()
        };
        tree.children.insert(site, vec![node(zone, "ZONE", 2)]);
        tree.children.insert(
            zone,
            vec![node(equipment, "EQUI", 1), node(sibling, "EQUI", 0)],
        );
        tree.children
            .insert(equipment, vec![node(primitive, "BOX", 0)]);
        tree.parent.extend([
            (zone, site),
            (equipment, zone),
            (primitive, equipment),
            (sibling, zone),
        ]);
        tree.detach_missing(&[equipment]);
        assert_eq!(tree.prune_unreachable(), vec![equipment, primitive]);
        assert_eq!(tree.children[&zone].len(), 1);
        assert_eq!(tree.children[&zone][0].refno.refno(), sibling);
        assert_eq!(tree.parent.get(&sibling), Some(&zone));
    }

    #[test]
    fn get_work_missing_site_is_included_in_unload_set() {
        let site = RefU64(1);
        let mut tree = TreeModel {
            roots: vec![node(site, "SITE", 0)],
            ..Default::default()
        };
        tree.detach_missing(&[site]);
        assert_eq!(tree.prune_unreachable(), vec![site]);
        assert!(tree.roots.is_empty());
    }

    /// SITE > ZONE > PIPE > BRAN > { ELBO, ATTA }，旁边一台 EQUI；整棵展开。
    /// 右键「查看尺寸标注」的靶子（计划 B2 / D3）就在这棵树上算。
    fn pipe_tree() -> TreeModel {
        let (site, zone, pipe, bran, elbo, atta, equi) = (
            RefU64(1),
            RefU64(2),
            RefU64(3),
            RefU64(4),
            RefU64(5),
            RefU64(6),
            RefU64(7),
        );
        let mut tree = TreeModel {
            roots: vec![node(site, "SITE", 1)],
            ..Default::default()
        };
        tree.children.insert(site, vec![node(zone, "ZONE", 2)]);
        tree.children
            .insert(zone, vec![node(pipe, "PIPE", 1), node(equi, "EQUI", 0)]);
        tree.children.insert(pipe, vec![node(bran, "BRAN", 2)]);
        tree.children
            .insert(bran, vec![node(elbo, "ELBO", 0), node(atta, "ATTA", 0)]);
        tree.parent.extend([
            (zone, site),
            (pipe, zone),
            (equi, zone),
            (bran, pipe),
            (elbo, bran),
            (atta, bran),
        ]);
        tree.expanded.extend([site, zone, pipe, bran]);
        tree
    }

    /// BRAN 归自己，成员上溯到所在 BRAN，PIPE / ZONE / EQUI 与不在树里的元素都没有靶子
    /// ——其余元素不出现该项（计划 D3），而且不许把 PIPE 当成「一整根管」去问服务。
    #[test]
    fn dimension_target_is_the_branch_itself_or_the_owning_branch() {
        let tree = pipe_tree();
        let bran = RefU64(4);
        assert_eq!(tree.dimension_branch(bran), Some(bran));
        assert_eq!(tree.dimension_branch(RefU64(5)), Some(bran));
        assert_eq!(tree.dimension_branch(RefU64(6)), Some(bran));
        for outsider in [RefU64(1), RefU64(2), RefU64(3), RefU64(7), RefU64(99)] {
            assert_eq!(tree.dimension_branch(outsider), None, "{outsider}");
        }
        assert!(super::is_bran_noun("bran"));
        assert!(!super::is_bran_noun("PIPE"));
    }

    /// 展平时每一行都带着自己的靶子，与逐个上溯算出来的一致——模型树的行菜单读的是
    /// 行上这一格，视口菜单读的是主选中那一格，两个入口不许答出两个数。
    #[test]
    fn flattened_rows_carry_the_same_dimension_target_as_the_lookup() {
        let tree = pipe_tree();
        let rows = tree.flatten(&HashMap::new());
        assert_eq!(rows.len(), 7, "整棵都展开了");
        for row in &rows {
            assert_eq!(
                row.dimension_branch,
                tree.dimension_branch(row.refno),
                "{} {}",
                row.noun,
                row.refno
            );
        }
        let targets: Vec<_> = rows
            .iter()
            .filter_map(|row| row.dimension_branch.map(|b| (row.noun.as_str(), b)))
            .collect();
        assert_eq!(
            targets,
            vec![
                ("BRAN", RefU64(4)),
                ("ELBO", RefU64(4)),
                ("ATTA", RefU64(4))
            ]
        );
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
    fn applied_data_refreshes_the_tree_and_leaves_the_scene_alone() {
        // 数据应用只换树。回到旧写法（把它记进整场重载的欠账）这两条会红。
        assert_eq!(auto_refresh(false, true, true), AutoRefresh::TreeOnly);
        assert_eq!(auto_refresh(false, false, true), AutoRefresh::TreeOnly);
        // 没有数据应用就只剩交付单元那条精确线索。
        assert_eq!(auto_refresh(false, true, false), AutoRefresh::Units);
    }

    #[test]
    fn an_owed_reload_waits_for_the_queue_to_drain() {
        assert!(!background_models_settled(false, true, true, true, true));
        assert!(!background_models_settled(true, true, false, true, true));
        assert!(!background_models_settled(true, true, true, false, true));
        assert!(!background_models_settled(true, true, true, true, false));
        assert!(background_models_settled(true, true, true, true, true));
        // 欠着账但后台还没静下来：这一拍照旧只换树，不去抢那次冷查询。
        assert_eq!(auto_refresh(true, false, true), AutoRefresh::TreeOnly);
        assert_eq!(auto_refresh(true, true, false), AutoRefresh::FullReload);
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
    fn failed_model_result_is_logged_without_restoring_reload_debt() {
        let source = include_str!("main.rs");
        let failure = source
            .split_once("data::Evt::Models(debt_reload, result) =>")
            .expect("model result arm")
            .1
            .split_once("data::Evt::ReloadProgress")
            .expect("next event arm")
            .0
            .split_once("Err(error) =>")
            .expect("model failure arm")
            .1;
        assert!(failure.contains("三维模型查询失败"));
        assert!(
            !failure.contains("restore_model_reload"),
            "模型查询已经返回失败时只记录，不把同一笔欠账交给队列轮询自动重试"
        );
    }

    #[test]
    fn failed_reload_before_model_result_restores_the_reload_debt() {
        let mut owed = false;
        restore_model_reload(&mut owed, true);
        assert_eq!(auto_refresh(owed, true, false), AutoRefresh::FullReload);
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
            reload_snapshot(&loaded, &pending, &visibility, &HashSet::new()).models,
            vec![
                (shown, true),
                (hidden, false),
                (toggling, false),
                (fresh, true)
            ],
            "刚进场没回执的按可见记，其余按指令 > 回执"
        );
    }

    /// 快照除了模型集，还要带上清场前点过眼睛的范围目标（ADR-0030 D3）：服务供数重装时
    /// 以它们为祖先重查。名单按 refno 排序、去重，与模型集一样可复现；没点过眼睛就是空表
    /// ——那是「老快照，退到按模型 refno 问」的信号，不许伪造。
    #[test]
    fn reload_snapshot_records_scope_targets_sorted_and_deduped() {
        let zone = RefU64(20);
        let bran = RefU64(7);
        let loaded = HashSet::from([RefU64(1)]);
        let targets = HashSet::from([zone, bran]);

        let snapshot = reload_snapshot(&loaded, &HashMap::new(), &HashMap::new(), &targets);
        assert_eq!(snapshot.targets, vec![bran, zone]);
        assert_eq!(snapshot.models, vec![(RefU64(1), true)]);

        let bare = reload_snapshot(&loaded, &HashMap::new(), &HashMap::new(), &HashSet::new());
        assert!(bare.targets.is_empty());
    }

    /// 换供数模式（ADR-0026 热切，计划 §5.3）。只有设置值与此刻生效的那一面不同、且没被
    /// `PLANT_READ_FACE` 压过才换。换面那一步的顺序是「拍快照 → 重连那套复位 → 清空三维
    /// → 发 `SwitchReadFace`」——复位在拍照之前就是对着空场景拍；新面 `Ready(Ok)` 之后
    /// 快照要先移进取回工作那一格再 `get_work()`，反过来 `clear_scene_for_reload` 会重拍
    /// 一份空白快照顶掉它。
    #[test]
    fn a_switch_clears_the_scene_and_reloads_the_snapshot() {
        use plant_ui::settings::ReadFaceKind::{Service, Store};

        assert_eq!(read_face_switch(Service, Store, false), Some(Store));
        assert_eq!(read_face_switch(Store, Service, false), Some(Service));
        assert_eq!(read_face_switch(Store, Store, false), None);
        assert_eq!(
            read_face_switch(Service, Store, true),
            None,
            "被 PLANT_READ_FACE 压过时那一格是灰的，进程活着就一直按环境变量走"
        );

        let source = include_str!("main.rs");
        let switch = source
            .split("fn switch_read_face(")
            .nth(1)
            .unwrap()
            .split("fn reset_for_reconnect(")
            .next()
            .unwrap();
        let snapshot = switch
            .find("self.read_face_switch_restore = Some(reload_snapshot(")
            .expect("清场前先拍快照");
        let reset = switch
            .find("self.reset_for_reconnect()")
            .expect("重连那套复位");
        let clear = switch
            .find("self.pending_models = Some(Vec::new())")
            .expect("三维不许留旧供数方的几何");
        let send = switch
            .find("data::Req::SwitchReadFace(kind)")
            .expect("告诉数据线程换面");
        assert!(snapshot < reset && reset < clear && clear < send);

        let ready = source
            .split("data::Evt::Ready(Ok(info)) => {")
            .nth(1)
            .unwrap()
            .split("data::Evt::Ready(Err(e)) => {")
            .next()
            .unwrap();
        let restore = ready
            .find("self.read_face_switch_restore.take()")
            .expect("新面 Ready 之后接过换面前的快照");
        let hand_over = ready
            .find("self.model_reload_restore = Some(snapshot)")
            .expect("移进取回工作那一格");
        let get_work = ready.find("self.get_work()").expect("按取回工作那条路重装");
        assert!(restore < hand_over && hand_over < get_work);
    }

    /// 库供数下两边身份对不上，命令行与日志各说一句（计划 D12 后半）。说不说由
    /// `task_queue::Vm::identity_mismatch_line` 判（三个前提的全档在那儿钉着），这里钉
    /// 说话的时机：按这一拍换代之后的身份算、一次撞上只说一次、不再对不上就复位。
    #[test]
    fn a_mismatched_identity_in_store_mode_is_said_once_per_encounter() {
        let source = include_str!("main.rs");
        let poll = source
            .split("data::Evt::QueuePoll(result) => {")
            .nth(1)
            .unwrap()
            .split("data::Evt::QueueSetPaused(")
            .next()
            .unwrap();
        let adopt = poll
            .find("self.queue.adopt(poll)")
            .expect("先把这一拍的快照换代");
        let say = poll
            .find("self.queue.identity_mismatch_line()")
            .expect("再看两边身份");
        assert!(adopt < say, "对不上与否按这一拍的 /health 算，不是上一拍的");
        assert!(
            poll.contains("self.command_error(line.clone())")
                && poll.contains("self.logs.warn(&mut self.vm.logs, line)"),
            "命令行与日志各说一句"
        );
        assert!(
            poll.contains("None => self.identity_mismatch_said = false"),
            "不再对不上就复位，重连或换了接入点再撞上还得说"
        );
    }

    /// 库供数下一张空属性表要说清由来（计划 §5.5 T2）。三档：元件库元素给定论、
    /// 设计库元素说未同步、`/dbnums` 认不出这个库就只说「属性为空」不猜。
    /// 服务供数不进这条路——那一面属性来自 e3d-io 直读，空表是另一件事。
    #[test]
    fn a_catalogue_element_in_store_mode_gets_a_verdict() {
        use plant_ui::settings::ReadFaceKind::{Service, Store};

        let db = |dbnum, db_type: &str| task_queue::DbnumStatus {
            dbnum,
            db_type: db_type.to_owned(),
            ..Default::default()
        };
        let dbnums = [db(7997, "DESI"), db(5100, "CATA")];

        assert_eq!(
            empty_props_verdict(Store, false, Some(5100), &dbnums),
            Some("元件库元素不入模型本体库；切到服务供数可看")
        );
        assert_eq!(
            empty_props_verdict(Store, false, Some(7997), &dbnums),
            Some("库里没有这个元素的属性（未同步）；切到服务供数可看"),
            "设计库元素：pe 有这一行，ATT_* 没同步过来"
        );
        assert_eq!(
            empty_props_verdict(Store, true, None, &dbnums),
            Some("库里没有这个元素的属性（未同步）；切到服务供数可看"),
            "树只由当前 MDB 的设计库长出来，在树上就是设计库元素"
        );
        assert_eq!(
            empty_props_verdict(Store, false, Some(9999), &dbnums),
            Some("属性为空"),
            "认不出这个库就不猜"
        );
        assert_eq!(
            empty_props_verdict(Store, false, None, &[]),
            Some("属性为空"),
            "gen-model 不在场，`/dbnums` 是空的：哪一档都判不出来"
        );
        // 大小写按服务端给的字面照收，不自己规范化。
        assert_eq!(
            empty_props_verdict(Store, false, Some(5100), &[db(5100, "cata")]),
            Some("元件库元素不入模型本体库；切到服务供数可看")
        );

        for in_tree in [true, false] {
            assert_eq!(
                empty_props_verdict(Service, in_tree, Some(5100), &dbnums),
                None,
                "服务供数照旧画表"
            );
        }
    }

    /// 服务端翻面通告：库供数下那两句话不能原样说（计划 §5.5 T1）。
    ///
    /// 「不必重载」「活不过服务端重启」说的是**本客户端下一次装载会拿到什么**，
    /// 而库供数的实例直读 `inst_relate`、根本不经服务端——照说就是替服务端说了
    /// 本客户端的事。那时候只报这件事发生过，并挑明它不影响眼前这一份。
    #[test]
    fn a_flip_in_store_mode_says_it_changes_nothing_here() {
        use plant_ui::settings::ReadFaceKind::{Service, Store};
        use task_queue::ModelSource::{Database, Memory};

        assert_eq!(
            model_source_changed_line(7997, Database, Store),
            "服务端 db7997 的模型改由数据库供数；本客户端库供数，不受影响"
        );
        assert_eq!(
            model_source_changed_line(7997, Memory, Store),
            "服务端 db7997 的模型改由进程内存供数；本客户端库供数，不受影响"
        );
        for source in [Database, Memory] {
            let line = model_source_changed_line(7997, source, Service);
            assert!(line.starts_with("db7997 的模型改由"), "{line}");
            assert!(!line.contains("不受影响"), "{line}");
        }
        assert!(
            model_source_changed_line(7997, Database, Service).contains("不必重载"),
            "服务供数照旧说本客户端下一次装载会拿到什么"
        );
    }

    #[test]
    fn replay_hides_only_survivors_and_consumes_the_snapshot() {
        let kept = RefU64(1);
        let gone = RefU64(2);
        let shown = RefU64(3);
        let mut restore = Some(ReloadSnapshot {
            models: vec![(kept, false), (gone, false), (shown, true)],
            targets: vec![RefU64(9)],
        });
        // 库里删掉的 gone 这次没查回来：不回放，免得对着空气记一笔隐藏账。
        let loaded = HashSet::from([kept, shown]);

        assert_eq!(take_hidden_for_replay(&mut restore, &loaded), vec![kept]);
        assert!(restore.is_none(), "快照回放即消费，下一次取回重新拍");
        assert!(take_hidden_for_replay(&mut restore, &loaded).is_empty());
    }

    /// 显示查空走 INFO 提示、不走 ERROR（ADR-0030 D1，实施约束 2）：新服务端回 200 空，老服务端
    /// 回 409 `not_generated`，两者在 `Evt::ModelScope` 里是同一件事——树行停「未加载」、
    /// 一句「右键可生成」；其余错误照旧 ERROR「已有模型查询失败」。
    #[test]
    fn an_empty_scope_is_a_hint_not_an_error() {
        let conflict = |message: &str| {
            anyhow::Error::new(model_update_api::ApiError(
                plant_ui::model_update::Failure::new("conflict", message),
            ))
        };
        assert!(scope_not_generated(&conflict(
            "not_generated: generation root 24381/100677 of dbnum 7997 is not in the process projection yet"
        )));
        assert!(
            !scope_not_generated(&conflict("dbnum=7997 已有整库模型任务在运行")),
            "真冲突不是「没生成过」"
        );
        assert!(!scope_not_generated(&anyhow::Error::new(
            model_update_api::ApiError(plant_ui::model_update::Failure::new(
                "timeout",
                "not_generated"
            ))
        )));

        let source = include_str!("main.rs");
        let arm = source
            .split_once(
                "data::Evt::ModelScope(epoch, target, result) if epoch == self.model_scope_epoch => {",
            )
            .expect("眼睛回包那一臂")
            .1
            .split_once("data::Evt::SubtreeBounds {")
            .expect("下一臂")
            .0;
        let normalize = arm
            .find("Err(error) if scope_not_generated(&error) =>")
            .expect("老服务端的 409 先归一成查空");
        let empty = arm
            .find("Ok(loaded) if loaded.records.is_empty() =>")
            .expect("查空分支");
        assert!(normalize < empty, "归一要在分支判断之前");
        let empty_branch = arm[empty..]
            .split_once("Ok(loaded) =>")
            .expect("非空分支")
            .0;
        assert!(
            empty_branch.contains("info_of(") && empty_branch.contains("EMPTY_SCOPE_HINT"),
            "查空是 INFO 提示：{empty_branch}"
        );
        assert!(
            !empty_branch.contains("warn_of(") && !empty_branch.contains("error_of("),
            "查空不许记 WARN / ERROR：{empty_branch}"
        );
        assert!(arm.contains("\"已有模型查询失败\""), "其余错误照旧 ERROR");
        assert_eq!(
            EMPTY_SCOPE_HINT,
            "范围内没有已生成的模型；右键「重新生成模型」可生成"
        );
    }

    /// 范围定位是异步的：点下去到相机动之间若什么都不说，大层级（SITE）看着就是
    /// 「没反应」；但记录级目标毫秒就回，立刻记一句只是噪音。所以「正在查询」
    /// 压 400 ms 才发（`focus_scope_pending` → `tick_focus_scope`），快查询只留
    /// 结果那一条；两句都在 INFO——定位不是错误。
    #[test]
    fn focus_scope_reports_start_and_outcome() {
        let source = include_str!("main.rs");

        let request = source
            .split_once("Cmd::FocusTreeScope(target) => {")
            .expect("范围定位的发起臂")
            .1
            .split_once("Cmd::Model(action) => {")
            .expect("下一臂")
            .0;
        let warn = request.find("模型服务通道已断开").expect("通道断开那一句");
        let start = request
            .find("focus_scope_pending")
            .expect("发起时登记「在查」的到期时刻");
        assert!(warn < start, "只在发送成功后才登记「在查」：{request}");
        assert!(
            request.contains("Instant::now() + Duration::from_millis(400)"),
            "「在查」要压 400 ms，别和结果挤在同一秒：{request}"
        );
        assert!(
            request.contains("ctx.request_repaint_after("),
            "登记了到期时刻就得排一次到期重绘：{request}"
        );
        assert!(
            !request.contains("正在查询模型范围"),
            "发起臂本身不许直接记「在查」：{request}"
        );
        // 新的定位意图必须让更早挂着的取景账失效，否则它们晚一拍落地时会把刚发起的这次
        // 查询作废，相机飞回旧目标、最新这一下被静默吞掉。
        assert!(
            request.contains("self.supersede_camera_intents()"),
            "发起范围定位要先撤掉更早的取景意图：{request}"
        );

        let tick = source
            .split_once("fn tick_focus_scope(&mut self) {")
            .expect("到期判定")
            .1
            // 拿下一个函数头作结尾，不拿 `\n    }\n`：CRLF 检出时那个字面量永远找不到。
            .split_once("fn sync_mesh_progress(")
            .expect("到期判定结尾")
            .0;
        assert!(
            tick.contains("accepts_bounds(") && tick.contains("info_of("),
            "到期时仍在途才说「在查」，且走 info_of：{tick}"
        );
        assert!(
            !tick.contains("warn_of(") && !tick.contains("error_of("),
            "「在查」不许记 WARN / ERROR：{tick}"
        );
        // 这条钉必须搜**正文里的** `fn ui`，不能搜整个文件：断言的针本身写在这条测试里，
        // 搜整个文件等于搜到自己，把条件删了照样绿。
        let body = source
            .rsplit_once("fn ui(&mut self, ui: &mut egui::Ui) {")
            .expect("正文里的 fn ui（测试模块之后）")
            .1;
        assert!(
            body.contains(concat!(
                "self.model_progress_until.is_some()",
                " || self.focus_scope_pending.is_some()"
            )),
            "在途时要按 100 ms 重绘，否则 400 ms 的到期判定等不到下一拍"
        );

        let reply = source
            .split_once("data::Evt::SubtreeBounds {")
            .expect("范围回包那一臂")
            .1
            .split_once("data::Evt::SubtreeBounds { .. } => {}")
            .expect("过期回包那一臂")
            .0;
        let ok = reply
            .split_once("Ok(bounds) => {")
            .expect("成功分支")
            .1
            .split_once("Err(error) if read_face::is_no_renderable_geometry(&error) => {")
            .expect("查空分支")
            .0;
        assert!(
            ok.contains("已定位到") && ok.contains("model_count") && ok.contains("info_of("),
            "成功要把框了多少个模型说出来：{ok}"
        );
        assert!(
            !ok.contains("warn_of(") && !ok.contains("error_of("),
            "成功不许记 WARN / ERROR：{ok}"
        );
    }

    /// 取景是异步的，账有好几本（范围查询、孔洞等几何、房间模型等范围、房间树等树与叶子）。
    /// 只有一处收口才管得住：**意图点**（用户刚点下去）先撤掉更早的账，**执行点 / 落地点**
    /// 不许反过来作废别人——那会把挂起期间更新的意图吞掉。
    #[test]
    fn every_camera_intent_supersedes_older_deferred_framing() {
        let source = include_str!("main.rs");
        // 正文在测试模块之前，取第一处就是真的那个。
        let helper = source
            .split_once("fn supersede_camera_intents(&mut self) {")
            .expect("收口函数")
            .1
            .split_once("#[cfg(test)]")
            .expect("正文结尾")
            .0;
        for required in [
            "self.focus_bounds_request.cancel()",
            "self.hole_focus_pending = None",
            "self.pending_room_frame = None",
            "self.room_tree_frame = None",
            "self.focus_room_frame = None",
            "RoomTreeDeferred::Focus",
            "*frame = false",
        ] {
            assert!(helper.contains(required), "收口要管 {required}：{helper}");
        }
        // 房间视图的**进视图**门控不归这里管：详情回包靠它认领，页签与历史已经切过去了，
        // 在这里清掉会让状态劈叉。取景账（`focus_room_frame`）已经拆出来归这里撤。
        assert!(
            !helper.contains("focus_room_pending") && !helper.contains("room_view"),
            "进视图门控与房间视图上下文不归这里管：{helper}"
        );
        // `focus_scope_pending` 归 `tick_focus_scope` 自己判（到期时问 `accepts_bounds`），
        // 收口不必碰它——碰了就得在这里同步维护它的到期时刻。
        assert!(
            !helper.contains("focus_scope_pending"),
            "「正在查询」的到期账不归这里管：{helper}"
        );

        // 九个意图点：都必须在自己的取景槽写入 / 相机命令之前收口。
        // 每条配一根「登记针」——只断言「切片里含 helper」不够：把 helper 挪到 begin 之后，
        // 刚发出的查询会被自己作废、按 F 没反应，而那种写法照样是绿的。
        let intents = [
            (
                "孔洞定位",
                "Cmd::FocusVirtualHoleTarget(id) => {",
                "Cmd::ShowVirtualHoleOriginal(on) => {",
                // 拿分叉头作针，不拿未加载分支里的 `hole_focus_pending = Some(target)`：
                // 后者只覆盖 else 一支，把收口挪进 else 就漏了 if loaded 那一支。
                "if loaded {",
            ),
            (
                "树上定位",
                "Cmd::FocusTreeScope(target) => {",
                "Cmd::Model(action) => {",
                "self.focus_bounds_request.begin(target)",
            ),
            (
                "取景命令",
                "action @ (plant_ui::ModelAction::Focus(_)",
                "plant_ui::ModelAction::SetVisible { refnos, visible } => {",
                "self.view3d_commands.push(Cmd::Model(action))",
            ),
            (
                "房间树聚焦",
                "RoomTreeAction::Focus(id) => {",
                "RoomTreeAction::Isolate(id) =>",
                "RoomTreeDeferred::Focus)",
            ),
            (
                "房间树加载",
                "RoomTreeAction::LoadModels { id, confirmed } => {",
                "RoomTreeAction::CancelLoadModels =>",
                "frame: true",
            ),
            (
                "显示房间模型",
                "fn show_room_model(&mut self, room: RefU64) {",
                "fn set_model_visible(&mut self, refnos: Vec<RefU64>, visible: bool) {",
                "self.set_model_visible(vec![room], true)",
            ),
            (
                "进房间视图",
                "fn focus_room(&mut self, room: RefU64) {",
                "// ------------------------------------------------ 房间视图 · 房间体与直段截断",
                // 登记针取取景账那一行，不取进视图门控：门控不归 helper 管，
                // 拿它当针会在「helper 挪到门控之后」时照样绿。
                "self.focus_room_frame = Some(room)",
            ),
            (
                "孔洞切削体定位",
                "Cmd::FocusVirtualHole(id) => {",
                "Cmd::FocusVirtualHoleTarget(id) => {",
                "VirtualHoleOp::FocusHole(id)",
            ),
            (
                "历史回退",
                "if let Some(camera) = entry.camera {",
                "self.nav.navigating = false;",
                "Cmd::RestoreCamera(camera)",
            ),
        ];
        for (name, head, tail, registered) in intents {
            let slice = source
                .split_once(head)
                .unwrap_or_else(|| panic!("{name} 的意图点还在：{head}"))
                .1
                .split_once(tail)
                .unwrap_or_else(|| panic!("{name} 的切片结尾：{tail}"))
                .0;
            let close = slice
                .find("self.supersede_camera_intents()")
                .unwrap_or_else(|| panic!("{name} 要先撤掉更早的取景意图：{slice}"));
            let register = slice
                .find(registered)
                .unwrap_or_else(|| panic!("{name} 的登记针没找到：{registered}"));
            assert!(
                close < register,
                "{name} 要先收口、再登记自己的取景（否则会作废刚发出的查询）：{slice}"
            );
        }

        // 执行点不许作废：房间树 Focus 是挂起后晚执行的，它手上这笔不一定是用户最新的意图。
        let execute = source
            .split_once("RoomTreeDeferred::Focus => {")
            .expect("房间树聚焦执行臂")
            .1
            .split_once("RoomTreeDeferred::LoadModels { confirmed, frame } => {")
            .expect("加载执行臂")
            .0;
        assert!(
            !execute.contains("supersede_camera_intents")
                && !execute.contains("focus_bounds_request.cancel()"),
            "执行点不作废在途查询：{execute}"
        );

        // 「加载模型」落地时取不取景，看挂起那一刻记下的 frame；而且只许写、不许清——
        // `frame=false` 时 `room_tree_frame` 里可能正躺着更新那次登记的账。
        let load = source
            .split_once("RoomTreeDeferred::LoadModels { confirmed, frame } => {")
            .expect("加载执行臂")
            .1
            .split_once("self.settle_room_tree_frame();")
            .expect("取景结算")
            .0;
        assert!(
            load.contains("if frame {") && load.contains("self.room_tree_frame = Some("),
            "加载落地要听挂起时的 frame，且只在 frame 为真时写取景账：{load}"
        );
        assert!(
            !load.contains("frame.then("),
            "frame 为假时不许把 room_tree_frame 写成 None——那会抹掉更新那次登记的账：{load}"
        );
        assert!(
            !load.contains("self.room_tree_frame = None"),
            "加载落地只许写、不许清：补一个 else 清空同样会抹掉更新的账：{load}"
        );
    }

    /// 房间视图的取景账拆出来之后（`focus_room_frame`），四处必须同一条口径：意图点登记、
    /// 收口撤、落地只认账、没进成视图要销。任何一处写反了，症状都是「按了别处的定位，
    /// 相机还是被晚到的房间体拽回房间」——或者反过来，「点了房间却不取景」。
    #[test]
    fn room_view_framing_waits_for_its_own_ledger() {
        let source = include_str!("main.rs");
        let body_of = |head: &str| {
            source
                .split(head)
                .nth(1)
                .unwrap_or_else(|| panic!("找不到 {head}"))
                .split("\n    fn ")
                .next()
                .unwrap()
        };

        // a. 房间体落地：先过早退与兜底分流，再取账，再决定取不取景。
        // 取账若挪到 `room_volume_is_current` 早退或分流之前，过期回包 / 兜底分支会先把账
        // 吃掉：同一间房重进、或整场重装之后，新帧号的回包就再也不取景了。
        let settle = body_of("fn settle_room_volume(");
        let current = settle
            .find("room_volume_is_current")
            .expect("房间体落地要认帧号与房间");
        let routed = settle
            .rfind("self.room_volume_fallback(")
            .expect("房间体取不到要走兜底");
        let take = settle
            .find("take_if(|pending| *pending == room)")
            .expect("房间体落地要认取景账");
        let gate = settle.find("if frame &&").expect("取景要受账门控");
        let focus = settle
            .find("plant_ui::ModelAction::FocusBounds")
            .expect("房间体落地要取景");
        assert!(current < routed, "帧号早退要在兜底分流之前：{settle}");
        assert!(routed < take, "取账要在帧号早退与兜底分流之后：{settle}");
        assert!(take < gate, "先取账、再门控：{settle}");
        assert!(gate < focus, "先门控、再取景：{settle}");
        assert!(
            !settle.contains("supersede_camera_intents"),
            "落地点不许反过来作废别人：{settle}"
        );

        // b. 兜底同理，对准的是面板与成员的并集。
        let fallback = body_of("fn room_volume_fallback(");
        let take = fallback
            .find("take_if(|pending| *pending == room)")
            .expect("兜底要认取景账");
        let focus = fallback
            .find("plant_ui::ModelAction::FocusGroup")
            .expect("兜底要取景");
        let gate = fallback.find("if frame &&").expect("兜底取景要受账门控");
        assert!(take < gate, "先取账、再门控：{fallback}");
        assert!(gate < focus, "先门控、再取景：{fallback}");
        assert!(
            !fallback.contains("supersede_camera_intents"),
            "兜底是落地点，不许反过来作废别人：{fallback}"
        );

        // 取账必须认房间号：裸 `take()` 会让 A 的房间体吃掉 B 的账——A 的在途回包把相机
        // 拽回 A，B 的范围到齐后反而不取景。
        for (name, body) in [("房间体落地", &settle), ("兜底", &fallback)] {
            assert!(
                !body.contains("focus_room_frame.take()"),
                "{name} 取账要认房间号，不许裸 take：{body}"
            );
            assert!(
                !body.contains("focus_room_frame = None"),
                "{name} 是落地点，只许取、不许清：{body}"
            );
        }

        // c. 整场重装不重登记：重装保留相机（`should_frame_batch` 只在空场景取景），
        // 所以重装后只重新隔离、重取房间体，不再拽相机。
        assert!(
            !body_of("fn reapply_room_view_after_reload(").contains("focus_room_frame"),
            "整场重装不碰取景账（重装保留相机）"
        );
        // 写点白名单：生产代码里只有「登记这一次的房间」「清空」和「视图适应后放回」三种写法，
        // 各在明处。换 `.replace` / `.insert` / `get_or_insert`，或者跑到重装臂里另写一处登记，
        // 都要变红。
        let (production, _) = source.split_once("\nmod tests {").expect("测试模块分界");
        let mut registered = 0;
        let mut restored = 0;
        for line in production.lines() {
            let Some((_, value)) = line.split_once("self.focus_room_frame = ") else {
                continue;
            };
            let value = value.trim();
            if value == "Some(room);" {
                registered += 1;
            } else if value == "room_frame;" {
                restored += 1;
            } else if value != "None;" {
                panic!("取景账只许登记 / 清空 / 视图适应后放回，不许别的写法：{line}");
            }
        }
        assert_eq!(registered, 1, "登记取景账只有进房间视图那一处");
        assert_eq!(restored, 1, "视图适应后放回只有 wrapper 那一处");
        for forbidden in [
            "focus_room_frame.replace(",
            "focus_room_frame.insert(",
            "focus_room_frame.get_or_insert",
        ] {
            assert!(
                !production.contains(forbidden),
                "取景账只走赋值，不许 {forbidden}"
            );
        }

        // d. 退出房间视图与重连复位都要销账。
        for head in ["fn leave_room_view(", "fn reset_for_reconnect("] {
            assert!(
                body_of(head).contains("self.focus_room_frame = None"),
                "{head} 没有销掉房间取景账"
            );
        }

        // e. 详情回包没进成视图（查不到 / 失败 / 没有实时渲染器 / 空房壳）也要销账：
        // 帧号是「进视图」的唯一凭据，帧号没变就说明这一趟没进。门控也要钉住——
        // 无条件销账会让每一次正常进视图都立刻丢账，房间视图基本都不取景了。
        let detail = source
            .split_once("data::Evt::RoomDetail(room, result)")
            .expect("房间详情守卫臂")
            .1
            .split_once("data::Evt::RoomDetail(..) => {}")
            .expect("房间详情兜底臂")
            .0;
        let epoch = detail
            .find("let epoch_before = self.room_view_epoch;")
            .expect("要先记下进视图之前的帧号");
        let matched = detail.find("match result {").expect("详情分派");
        let guard = detail
            .find("for_viewport && self.room_view_epoch == epoch_before")
            .expect("销账要认「没进成视图」这一条");
        let take = detail
            .find("take_if(|pending| *pending == room)")
            .expect("没进成视图要销账，且要认房间号");
        assert!(epoch < matched, "帧号要在分派之前记：{detail}");
        assert!(matched < guard, "门控要在分派之后：{detail}");
        assert!(guard < take, "销账要在门控之内：{detail}");
    }

    /// 「视图适应」与视方块的 Home（`SnapView { fit: true }`）也算取景意图，但房间取景账要留住：
    /// 房间视图里 Home 对准的就是房间体，撤掉它等于「进了房间却永远不取景」。
    /// 点面（`fit: false`）只改朝向，不许撤账。
    #[test]
    fn fitting_the_view_supersedes_other_intents_but_keeps_the_room() {
        let source = include_str!("main.rs");

        // a. wrapper 的三步顺序：先留住房间账，再撤别人，最后放回。
        let wrapper = source
            .split_once("fn supersede_camera_intents_for_fit(&mut self) {")
            .expect("视图适应的收口")
            .1
            .split_once("\n    fn ")
            .expect("下一个方法")
            .0;
        let keep = wrapper
            .find("self.focus_room_frame.take()")
            .expect("要先取走房间取景账");
        let restore = wrapper
            .find(".or(self.focus_room_pending)")
            .expect("视图正在进时也要留住");
        let close = wrapper
            .find("self.supersede_camera_intents()")
            .expect("再撤掉更早的账");
        let put_back = wrapper
            .find("self.focus_room_frame = room_frame")
            .expect("最后放回房间账");
        assert!(keep < restore, "先取房间账、再兜底问进视图门控：{wrapper}");
        assert!(restore < close, "撤账要在留住房间账之后：{wrapper}");
        assert!(close < put_back, "放回要在撤账之后：{wrapper}");

        // b. FitAll 臂：先收口、再下发。切片只在生产段里找，尾锚用真正的下一臂 HideAll——
        // 拿 SetVisible 当尾锚会跨到测试模块里的字面量，切片长到几千行，位置断言就失去意义。
        let (production, _) = source.split_once("\nmod tests {").expect("测试模块分界");
        let fit_all = production
            .split_once("plant_ui::ModelAction::FitAll => {")
            .expect("视图适应臂")
            .1
            .split_once("plant_ui::ModelAction::HideAll => {")
            .expect("下一个臂")
            .0;
        let close = fit_all
            .find("self.supersede_camera_intents_for_fit()")
            .expect("视图适应要先收口");
        let push = fit_all
            .find("Cmd::Model(plant_ui::ModelAction::FitAll)")
            .expect("视图适应要下发");
        assert!(close < push, "先收口、再下发 FitAll：{fit_all}");

        // c. 视方块 Home：`fit: true` 单独一支，先收口、再下发。
        let home = production
            .split_once("command @ Cmd::SnapView { fit: true, .. } => {")
            .expect("视方块 Home 臂")
            .1
            .split_once("command @ (Cmd::PickViewport(_)")
            .expect("直通臂")
            .0;
        let close = home
            .find("self.supersede_camera_intents_for_fit()")
            .expect("Home 要先收口");
        let push = home
            .find("self.view3d_commands.push(command)")
            .expect("Home 要下发");
        assert!(close < push, "先收口、再下发 Home：{home}");

        // e. 这个 wrapper 只该在这两个臂里被调用：多一处就是没想清楚的新意图点。
        assert_eq!(
            production
                .matches("self.supersede_camera_intents_for_fit()")
                .count(),
            2,
            "视图适应的收口只该在 FitAll 与 Home 两处调用"
        );

        // d. 点面留在直通臂里，且不许撤账。
        let passthrough = production
            .split_once("command @ (Cmd::PickViewport(_)")
            .expect("直通臂")
            .1
            .split_once("Cmd::Navigate(step) => {")
            .expect("下一条命令")
            .0;
        assert!(
            passthrough.contains("Cmd::SnapView { fit: false, .. }"),
            "点面要留在直通臂里：{passthrough}"
        );
        assert!(
            !passthrough.contains("supersede_camera_intents"),
            "点面只改朝向，不许撤账：{passthrough}"
        );
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
        assert!(!handler.contains("generate_model"));
        assert!(!handler.contains("/api/v1/model/ensure"));
        assert!(!handler.contains("/api/v1/model/generate"));
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

fn is_panel_noun(noun: &str) -> bool {
    noun.eq_ignore_ascii_case("PANE")
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
        self.pump_events(ui.ctx());
        self.tick_model_progress();
        self.tick_focus_scope();
        if self.model_progress_until.is_some() || self.focus_scope_pending.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        let t = theme_tokens::current();
        let d = self.settings_state.saved.density;
        // 首帧与每次换主题，把网格线色送给三维侧（视口底色由绘制层画，不经三维侧）。对着当前生效令牌
        // 比对而不是监听设置变更：主题的写入口不止设置窗一处，这里只认结果。
        if self.sent_viewport_grid != Some(t.viewport_grid) {
            self.sent_viewport_grid = Some(t.viewport_grid);
            self.view3d_commands.push(Cmd::SetViewportGrid(t.viewport_grid));
        }
        // 同理把三维外观送下去：首帧一次，之后只在已存设置变了时发（设置窗「保存」落到 saved 的那一帧）。
        let look = self.settings_state.saved.look;
        if self.sent_look != Some(look) {
            // 首次启动说一句：默认已经不是老的 PBR 画面了，知道去哪切回。这台设备跑不了边线 / 伪阴影时不说有。
            if self.sent_look.is_none() && look.mode == plant_ui::LookMode::E3d {
                let message = if self.settings_state.post_effects_lock.is_some() {
                    "三维外观已切为 E3D 出厂口径（lightgrey；边线 / 伪阴影 / 超采样在这台设备上不可用，原因见设置 → 渲染外观）；那里也可切回 web PBR"
                } else {
                    "三维外观已切为 E3D 出厂口径（lightgrey、边线、伪阴影、2×2 超采样）；设置 → 渲染外观 可切回 web PBR"
                };
                self.logs.info(&mut self.vm.logs, message);
            }
            self.sent_look = Some(look);
            self.view3d_commands.push(Cmd::SetViewportLook(look));
        }
        let mut cmds = workbench::show(ui, &t, d, &self.vm, &self.queue, &mut self.state);
        cmds.extend(data_publish::show(
            ui.ctx(),
            &t,
            d,
            &self.vm,
            &mut self.data_publish_state,
        ));
        cmds.extend(manual_data_publish::show(
            ui.ctx(),
            &t,
            d,
            &mut self.manual_data_publish_state,
        ));
        cmds.extend(model_update::show(
            ui.ctx(),
            &t,
            d,
            &self.model_api_url,
            self.queue.applying(),
            &self.mdb,
            self.queue.health.as_ref().map(|health| health.sync_live),
            // `/health` 报的数据面形态。读透形态下数据水位按设计不建立，「库状态」
            // 那一列要说「不建立」而不是「尚未建立」——后者是「需初始化」的意思。
            self.queue
                .health
                .as_ref()
                .map(|health| health.data_face.as_str()),
            self.queue.execution_blocked_reason(),
            // 读透形态的预览是空表，向导的范围名单只能从队列轮询的那份 `/dbnums` 说。
            &self.queue.dbnums,
            // 每个库此刻在队列里的活。队列面板自己有行表，本窗没有，走这一条。
            &task_queue::active_tasks(&self.queue),
            &self.model_update,
            &mut self.model_update_state,
        ));
        cmds.extend(model_mutation::show(
            ui.ctx(),
            &t,
            d,
            // 框里「一并删除网格文件」那一格要就地改这个值，所以是 `&mut`：勾选状态随
            // 确认回执一起交出去（`Cmd::ModelMutationConfirm`）。
            self.mutation.as_mut(),
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
        // 虚拟孔洞表的文件选择（对话框线程 / 浏览器隐藏 input），拿到就导入。
        match self.hole_pick.as_mut().map(|pick| pick.poll(ui.ctx())) {
            Some(HolePickPoll::Picked(file)) => {
                self.hole_pick = None;
                self.import_hole_file(file);
            }
            Some(HolePickPoll::Finished) => self.hole_pick = None,
            _ => {}
        }
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
            // 无名元素短名 / 全称只是同一份数据换个展平口径，重新展平就够，不重查。
            // 树还在加载 / 失败态时只记下口径：那两个态不是展平出来的，别把它们覆成空表。
            if self.tree.full_names != saved.tree_full_names {
                self.tree.full_names = saved.tree_full_names;
                if matches!(self.vm.tree, TreeVm::Ready(_)) {
                    self.rebuild_tree();
                }
            }
            // 供数模式变了就热切（ADR-0026）：先落盘再换，换面失败也不丢这次改动——
            // 界面停在未连接态，下拉可再改回去。
            if let Some(kind) = read_face_switch(
                self.read_face,
                saved.read_face,
                self.settings_state.read_face_lock.is_some(),
            ) {
                self.switch_read_face(kind);
            }
            ui.ctx().request_repaint();
        }
        // 绘制层只读、消费不掉自己的滚动请求，所以由这里判断它落地没有。
        self.settle_tree_reveal();
        self.settle_room_tree_reveal();
        self.handle_cmds(ui.ctx(), cmds);
        // 布尔作业是异步的：计算中有回执没回来时催下一帧，计数别停在旧数字上。
        if self.vm.virtual_holes.calculating > 0 {
            ui.ctx()
                .request_repaint_after(Duration::from_millis(200));
        }
        self.poll_queue(ui.ctx());
        // 这一帧才展开出来的行要等下一帧才画得到，别让界面停在这儿等下一次输入。
        // 子层还在路上的那种不在这里空转——祖先链和子层回来时数据线程会叫醒它。
        if self.tree_reveal_row_ready() {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod deleted_selection_tests {
    use super::*;
    #[test]
    fn deleting_primary_clears_selection_before_property_reload() {
        let selected = Selection::single(RefU64(26291));
        let next = selection_after_removed_nodes(&selected, &[RefU64(26291)]);
        assert_eq!(next.primary(), None);
        assert_eq!(next.len(), 0);
    }
    #[test]
    fn surviving_multi_selection_becomes_primary() {
        let mut selected = Selection::single(RefU64(1));
        selected.toggle(RefU64(2));
        let next = selection_after_removed_nodes(&selected, &[RefU64(2), RefU64(2)]);
        assert_eq!(next.primary(), Some(RefU64(1)));
        assert_eq!(next.len(), 1);
    }
    #[test]
    fn outside_tree_selection_is_not_erased_without_removal_evidence() {
        let selected = Selection::single(RefU64(99));
        assert_eq!(
            selection_after_removed_nodes(&selected, &[RefU64(2)]),
            selected
        );
    }
}
