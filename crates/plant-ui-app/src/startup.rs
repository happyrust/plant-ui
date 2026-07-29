use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use url::{Host, Url};

pub const LEGACY_PROJECT_CONFIG: &str = "config/e3d.project.ron";

#[derive(Debug, Clone, Deserialize)]
pub struct LegacyProjectConfig {
    pub api_host: String,
    pub db_host: String,
    pub mdb_name: String,
    pub project_name: String,
    pub project_code: String,
    pub module: String,
    pub auto_gen_mesh: bool,
}

#[derive(Debug)]
pub struct RuntimeConfig {
    pub db: aios_core::options::DbOption,
    pub model_api_url: Option<String>,
    pub data_api_url: String,
    pub auto_gen_mesh: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BrowserStartupConfig {
    Current(BrowserConfig),
    Legacy(LegacyBrowserConfig),
}

#[derive(Debug, Deserialize)]
struct BrowserConfig {
    db: BrowserDb,
    model_api_url: String,
    data_api_url: String,
}

#[derive(Debug, Deserialize)]
struct BrowserDb {
    host: String,
    port: u16,
    #[serde(default)]
    secure: bool,
    namespace: String,
    database: String,
    mdb: String,
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct LegacyBrowserConfig {
    legacy_project_ron: String,
    model_api_url: String,
    #[serde(default)]
    data_api_url: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
}

pub fn browser_runtime_config(json: &str) -> Result<RuntimeConfig> {
    let config: BrowserStartupConfig =
        serde_json::from_str(json).context("解析浏览器启动配置失败")?;
    match config {
        BrowserStartupConfig::Legacy(config) => {
            if config.model_api_url.trim().is_empty() {
                bail!("model_api_url 不能为空");
            }
            legacy_runtime_config(
                &config.legacy_project_ron,
                &config.username,
                &config.password,
                Some(config.model_api_url),
                Some(config.data_api_url),
            )
        }
        BrowserStartupConfig::Current(config) => {
            if config.db.host.trim().is_empty() {
                bail!("db.host 不能为空");
            }
            if config.db.port == 0 {
                bail!("db.port 不能为 0");
            }
            for (label, value) in [
                ("db.namespace", config.db.namespace.as_str()),
                ("db.database", config.db.database.as_str()),
                ("db.mdb", config.db.mdb.as_str()),
            ] {
                if value.trim().is_empty() {
                    bail!("{label} 不能为空");
                }
            }
            let model_api_url = normalize_http_url("model_api_url", &config.model_api_url)?;
            let data_api_url = normalize_http_url("data_api_url", &config.data_api_url)?;
            let db_host = config.db.host.trim().trim_end_matches('/');
            let mut db = aios_core::options::DbOption::default();
            db.v_ip = if config.db.secure
                && !db_host.starts_with("ws://")
                && !db_host.starts_with("wss://")
            {
                format!("wss://{db_host}")
            } else {
                db_host.to_owned()
            };
            db.v_port = config.db.port;
            db.surreal_ns = config.db.namespace.trim().to_owned();
            db.project_name = config.db.database.trim().to_owned();
            db.mdb_name = config.db.mdb.trim().to_owned();
            db.v_user = defaulted(&config.db.username, "root");
            db.v_password = defaulted(&config.db.password, "root");
            Ok(RuntimeConfig {
                db,
                model_api_url: Some(model_api_url),
                data_api_url,
                auto_gen_mesh: false,
            })
        }
    }
}

pub fn legacy_runtime_config(
    text: &str,
    username: &str,
    password: &str,
    model_api_url: Option<String>,
    data_api_url: Option<String>,
) -> Result<RuntimeConfig> {
    let legacy: LegacyProjectConfig =
        ron::from_str(text).context("解析旧版 e3d.project.ron 失败")?;
    for (label, value) in [
        ("api_host", legacy.api_host.as_str()),
        ("db_host", legacy.db_host.as_str()),
        ("mdb_name", legacy.mdb_name.as_str()),
        ("project_name", legacy.project_name.as_str()),
        ("project_code", legacy.project_code.as_str()),
        ("module", legacy.module.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("旧版项目配置的 {label} 不能为空");
        }
    }
    if !legacy.module.trim().eq_ignore_ascii_case("DESI") {
        bail!(
            "当前客户端只支持 DESI，旧版项目配置指定的是 {}",
            legacy.module.trim()
        );
    }

    let (v_ip, v_port) = split_db_host(&legacy.db_host)?;
    let data_api_url = normalize_http_url(
        "data_api_url/api_host",
        data_api_url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(&legacy.api_host),
    )?;
    let model_api_url = model_api_url
        .filter(|value| !value.trim().is_empty())
        .map(|value| normalize_http_url("model_api_url", &value))
        .transpose()?;

    let mut db = aios_core::options::DbOption::default();
    db.v_ip = v_ip;
    db.v_port = v_port;
    db.surreal_ns = legacy.project_code.trim().to_owned();
    db.project_name = legacy.project_name.trim().to_owned();
    db.mdb_name = legacy.mdb_name.trim().to_owned();
    db.v_user = defaulted(username, "root");
    db.v_password = defaulted(password, "root");

    Ok(RuntimeConfig {
        db,
        model_api_url,
        data_api_url,
        auto_gen_mesh: legacy.auto_gen_mesh,
    })
}

pub fn resolve_asset_root(
    configured: Option<OsString>,
    executable_dir: Option<&Path>,
    development_root: &Path,
) -> PathBuf {
    if let Some(configured) = configured.filter(|value| !value.to_string_lossy().trim().is_empty())
    {
        return PathBuf::from(configured);
    }
    if let Some(executable_dir) = executable_dir {
        let packaged = executable_dir
            .parent()
            .map(|root| root.join("backend/assets"));
        for candidate in [Some(executable_dir.join("assets")), packaged]
            .into_iter()
            .flatten()
        {
            if candidate.is_dir() {
                return candidate;
            }
        }
    }
    development_root.to_owned()
}

fn split_db_host(value: &str) -> Result<(String, u16)> {
    let value = value.trim();
    let (raw_scheme, authority) = value
        .split_once("://")
        .ok_or_else(|| anyhow::anyhow!("旧版 db_host 必须以 ws:// 或 wss:// 开头"))?;
    let scheme = raw_scheme.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "ws" | "wss") {
        bail!("旧版 db_host 必须以 ws:// 或 wss:// 开头");
    }
    let (_, port) = authority
        .trim_end_matches('/')
        .rsplit_once(':')
        .ok_or_else(|| anyhow::anyhow!("旧版 db_host 必须包含显式端口"))?;
    let port = port.parse::<u16>().context("旧版 db_host 的端口无效")?;
    if port == 0 {
        bail!("旧版 db_host 的端口不能为 0");
    }

    let url = Url::parse(value).context("旧版 db_host 不是有效 URL")?;
    if !url.username().is_empty() || url.password().is_some() {
        bail!("旧版 db_host 不能包含用户名或密码");
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        bail!("旧版 db_host 只能包含主机和显式端口");
    }
    let host = match url.host().context("旧版 db_host 的主机不能为空")? {
        Host::Domain(host) => host.to_owned(),
        Host::Ipv4(host) => host.to_string(),
        Host::Ipv6(host) => format!("[{host}]"),
    };
    Ok((format!("{scheme}://{host}"), port))
}

fn normalize_http_url(label: &str, value: &str) -> Result<String> {
    let value = value.trim().trim_end_matches('/');
    let url = Url::parse(value).with_context(|| format!("{label} 不是有效 URL"))?;
    if !matches!(url.scheme(), "http" | "https") {
        bail!("{label} 必须以 http:// 或 https:// 开头");
    }
    if url.host().is_none() {
        bail!("{label} 缺少主机");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("{label} 不能包含用户名或密码");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("{label} 不能包含查询参数或片段");
    }
    Ok(value.to_owned())
}

fn defaulted(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_owned()
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_ron(db_host: &str, module: &str) -> String {
        format!(
            r#"(
                api_host: "http://127.0.0.1:9099/",
                db_host: "{db_host}",
                mdb_name: "ALL",
                project_name: "AvevaMarineSample",
                project_code: "1516",
                module: "{module}",
                auto_gen_mesh: true,
            )"#
        )
    }

    #[test]
    fn legacy_project_config_maps_to_the_current_runtime() {
        let config = legacy_runtime_config(
            &legacy_ron("ws://localhost:8009", "DESI"),
            "operator",
            "secret",
            Some("http://127.0.0.1:8021/".into()),
            None,
        )
        .unwrap();

        assert_eq!(config.db.v_ip, "ws://localhost");
        assert_eq!(config.db.v_port, 8009);
        assert_eq!(config.db.surreal_ns, "1516");
        assert_eq!(config.db.project_name, "AvevaMarineSample");
        assert_eq!(config.db.mdb_name, "ALL");
        assert_eq!(config.db.v_user, "operator");
        assert_eq!(config.db.v_password, "secret");
        assert_eq!(
            config.model_api_url.as_deref(),
            Some("http://127.0.0.1:8021")
        );
        assert_eq!(config.data_api_url, "http://127.0.0.1:9099");
        assert!(config.auto_gen_mesh);
    }

    #[test]
    fn legacy_project_config_accepts_wss_and_rejects_invalid_inputs() {
        let secure = legacy_runtime_config(
            &legacy_ron("wss://db.example.test:443", "desi"),
            "root",
            "root",
            None,
            Some("https://data.example.test/".into()),
        )
        .unwrap();
        assert_eq!(secure.db.v_ip, "wss://db.example.test");
        assert_eq!(secure.db.v_port, 443);
        assert_eq!(secure.data_api_url, "https://data.example.test");

        for (host, module, message) in [
            ("localhost:8009", "DESI", "ws://"),
            ("ws://localhost", "DESI", "端口"),
            ("ws://localhost:nope", "DESI", "端口"),
            ("ws://user@localhost:8009", "DESI", "用户名"),
            ("ws://localhost:8009", "CATA", "DESI"),
        ] {
            let error =
                legacy_runtime_config(&legacy_ron(host, module), "root", "root", None, None)
                    .unwrap_err()
                    .to_string();
            assert!(
                error.contains(message),
                "{error:?} should contain {message:?}"
            );
        }

        let empty_project = legacy_ron("ws://localhost:8009", "DESI")
            .replace(r#"project_code: "1516""#, r#"project_code: """#);
        assert!(
            legacy_runtime_config(&empty_project, "root", "root", None, None)
                .unwrap_err()
                .to_string()
                .contains("project_code")
        );

        let ipv6 =
            legacy_runtime_config(&legacy_ron("ws://[::1]:8009/", "DESI"), "", "", None, None)
                .unwrap();
        assert_eq!(ipv6.db.v_ip, "ws://[::1]");
        assert_eq!(ipv6.db.v_user, "root");
        assert_eq!(ipv6.db.v_password, "root");

        let invalid_api = legacy_ron("ws://localhost:8009", "DESI")
            .replace("http://127.0.0.1:9099/", "http://?query");
        assert!(
            legacy_runtime_config(&invalid_api, "root", "root", None, None)
                .unwrap_err()
                .to_string()
                .contains("有效 URL")
        );
    }

    #[test]
    fn asset_root_prefers_configuration_then_executable_layouts() {
        let base = std::env::temp_dir().join(format!(
            "plant-ui-asset-root-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let configured = base.join("old data").join("assets");
        let executable = base.join("release").join("pc");
        let beside_executable = executable.join("assets");
        let packaged = base.join("release").join("backend").join("assets");
        let development = base.join("development-assets");
        for path in [&configured, &executable, &packaged, &development] {
            std::fs::create_dir_all(path).unwrap();
        }

        assert_eq!(
            resolve_asset_root(
                Some(configured.as_os_str().to_owned()),
                Some(&executable),
                &development,
            ),
            configured
        );
        assert_eq!(
            resolve_asset_root(None, Some(&executable), &development),
            packaged
        );
        std::fs::create_dir_all(&beside_executable).unwrap();
        assert_eq!(
            resolve_asset_root(None, Some(&executable), &development),
            beside_executable
        );
        assert_eq!(
            resolve_asset_root(Some("   ".into()), None, &development),
            development
        );
        assert_eq!(resolve_asset_root(None, None, &development), development);

        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn browser_startup_accepts_legacy_and_current_payloads() {
        let legacy = serde_json::json!({
            "legacy_project_ron": legacy_ron("wss://db.example.test:443", "DESI"),
            "model_api_url": "https://plant.example.test/",
            "data_api_url": "https://data.example.test/",
            "username": "web-user",
            "password": "web-secret",
        });
        let legacy = browser_runtime_config(&legacy.to_string()).unwrap();
        assert_eq!(legacy.db.v_ip, "wss://db.example.test");
        assert_eq!(legacy.db.v_user, "web-user");
        assert_eq!(
            legacy.model_api_url.as_deref(),
            Some("https://plant.example.test")
        );
        assert_eq!(legacy.data_api_url, "https://data.example.test");

        let current = serde_json::json!({
            "db": {
                "host": "127.0.0.1",
                "port": 8009,
                "secure": false,
                "namespace": "1516",
                "database": "AvevaMarineSample",
                "mdb": "/ALL",
                "username": "root",
                "password": "root"
            },
            "model_api_url": "http://127.0.0.1:8021",
            "data_api_url": "http://127.0.0.1:9099"
        });
        let current = browser_runtime_config(&current.to_string()).unwrap();
        assert_eq!(current.db.v_ip, "127.0.0.1");
        assert_eq!(current.db.v_port, 8009);
        assert_eq!(current.db.surreal_ns, "1516");
        assert_eq!(current.db.project_name, "AvevaMarineSample");
        assert_eq!(current.db.mdb_name, "/ALL");
        assert!(!current.auto_gen_mesh);
    }
}
