//! 设置落盘：`<exe 旁>/config/settings.ron`。
//!
//! 位置沿用 `DbOption.toml` 那条房规——配置跟着发行包走，一份发行包一份设置。开发
//! 构建落在 `target/debug/config/` 下，`cargo clean` 会一并带走；那是可接受的，
//! 它本来就是那次构建的配套。
//!
//! **这份文件出现之前，ADR-0008 写的「设置项 > 环境变量 > 出厂默认」只兑现了一半**：
//! 一行都不落盘，所以每次启动实际总是环境变量赢。从现在起才真按那条优先级走——
//! 启动脚本里的 `PLANT_MODEL_API_URL` 会被上一次在界面里存下的地址顶掉。

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use plant_ui::settings::Settings;

const DIR_NAME: &str = "config";
const FILE_NAME: &str = "settings.ron";

/// 设置文件的位置。读不到可执行文件路径时退回当前工作目录。
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
        .unwrap_or_default()
        .join(DIR_NAME)
        .join(FILE_NAME)
}

/// 读一次设置。
///
/// 文件不在就是 `None`——头一回启动本来就没有，那不是错。文件在却读不动（手改坏了、
/// 权限不够）则带着原因报错：把它当成「没有设置」会让人对着一个悄悄回到默认值的
/// 界面找半天。
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn load() -> Result<Option<Settings>> {
    let path = path();
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("读取设置失败：{}", path.display()))?;
    let settings = ron::from_str::<Settings>(&text)
        .with_context(|| format!("设置文件解析失败：{}", path.display()))?;
    Ok(Some(settings))
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn save(settings: &Settings) -> Result<()> {
    let path = path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("创建设置目录失败：{}", dir.display()))?;
    }
    let text = ron::ser::to_string_pretty(settings, ron::ser::PrettyConfig::default())
        .context("设置序列化失败")?;
    std::fs::write(&path, text).with_context(|| format!("写入设置失败：{}", path.display()))
}

/// 解出这次启动实际要用的网格目录。
///
/// 与 [`crate::startup::resolve_asset_root`] 同一副骨架，也是同一条优先级：设置项 >
/// 环境变量 > 出厂默认。设置项留空是**明确的「我没指定」**，不是「指定为空」——
/// 那一格让给环境变量。
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn resolve_mesh_dir(configured: &str, env: Option<OsString>, asset_root: &Path) -> PathBuf {
    if !configured.trim().is_empty() {
        return PathBuf::from(configured.trim());
    }
    if let Some(env) = env.filter(|value| !value.to_string_lossy().trim().is_empty()) {
        return PathBuf::from(env);
    }
    asset_root.join("meshes")
}

/// 校验一个网格目录，给设置窗那一行结论。
///
/// 只问「有没有网格文件」，不数有几个：要抓的是选错一层——指到了资产根而不是它底下
/// 的 `meshes`。那种情况第一个条目就见分晓，而真实的网格目录动辄几万个文件。
#[cfg(not(target_arch = "wasm32"))]
pub fn check_mesh_dir(dir: &str) -> plant_ui::settings::MeshDirStatus {
    use plant_ui::settings::MeshDirStatus;

    let dir = dir.trim();
    if dir.is_empty() {
        // 留空是合法的「用默认目录」，没什么可报的。
        return MeshDirStatus::Unknown;
    }
    let path = Path::new(dir);
    if !path.is_dir() {
        return MeshDirStatus::Unreachable(format!("目录不存在：{dir}"));
    }
    match plant_ui_view3d::mesh_source::contains_mesh_file(path) {
        Ok(true) => MeshDirStatus::Ok,
        Ok(false) => MeshDirStatus::NoMeshFiles,
        Err(error) => MeshDirStatus::Unreachable(format!("目录读不动：{error}")),
    }
}

/// 启动时解析一次的设置与网格目录。
///
/// 宿主在起 Bevy 之前算好放这儿，`App::new` 从这里取——两处各解一遍优先级迟早会
/// 漂移。与 `model_update_api` 的服务地址同一套路：一个进程只有一份。
#[derive(Default)]
pub struct Startup {
    pub settings: Settings,
    /// 设置项留空时网格实际会去哪个目录取。设置窗拿它当占位提示。
    pub default_mesh_dir: String,
    /// 读设置时出的岔子，进日志面板。
    pub warnings: Vec<String>,
}

static STARTUP: std::sync::OnceLock<Startup> = std::sync::OnceLock::new();

/// 浏览器端没人调它，那一侧取到的就是出厂默认。
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn set_startup(startup: Startup) {
    let _ = STARTUP.set(startup);
}

/// `None` = 宿主没交底。浏览器端就是这一支：那里的服务地址由宿主页面注入，
/// 网格由站点供给，本地设置文件无从谈起。
pub fn startup() -> Option<&'static Startup> {
    STARTUP.get()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_dir_prefers_the_setting_then_the_environment() {
        let asset_root = Path::new("D:/release/backend/assets");

        assert_eq!(
            resolve_mesh_dir("D:/models/meshes", Some("D:/env/meshes".into()), asset_root),
            PathBuf::from("D:/models/meshes")
        );
        assert_eq!(
            resolve_mesh_dir("  ", Some("D:/env/meshes".into()), asset_root),
            PathBuf::from("D:/env/meshes")
        );
        assert_eq!(
            resolve_mesh_dir("", Some("   ".into()), asset_root),
            asset_root.join("meshes")
        );
        assert_eq!(
            resolve_mesh_dir("", None, asset_root),
            asset_root.join("meshes")
        );
    }

    #[test]
    fn settings_survive_a_round_trip_through_ron() {
        let settings = Settings {
            theme: plant_ui::settings::Theme::Dark,
            density: plant_ui::style::tokens::Density::Compact,
            model_api_url: "http://10.0.0.9:8021".into(),
            data_api_url: "http://10.0.0.9:9099".into(),
            mesh_dir: "D:/models/meshes".into(),
        };
        let text =
            ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default()).unwrap();
        assert_eq!(ron::from_str::<Settings>(&text).unwrap(), settings);
    }

    /// 存量文件里没有的字段要能补上默认值，不能整份读失败——否则一次升级就把
    /// 用户上次存的主题和地址一起丢了。
    #[test]
    fn an_older_file_without_the_mesh_dir_still_loads() {
        let text = r#"(
            theme: Dark,
            density: Relaxed,
            model_api_url: "http://10.0.0.9:8021",
            data_api_url: "http://10.0.0.9:9099",
        )"#;
        let settings = ron::from_str::<Settings>(text).unwrap();
        assert_eq!(settings.theme, plant_ui::settings::Theme::Dark);
        assert_eq!(settings.mesh_dir, "");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn an_empty_mesh_dir_is_not_worth_a_verdict() {
        assert_eq!(
            check_mesh_dir("   "),
            plant_ui::settings::MeshDirStatus::Unknown
        );
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_missing_mesh_dir_says_so() {
        let missing = std::env::temp_dir().join("plant-ui-no-such-mesh-dir-9d8f7921");
        let status = check_mesh_dir(&missing.to_string_lossy());
        assert!(matches!(
            status,
            plant_ui::settings::MeshDirStatus::Unreachable(_)
        ));
    }
}
