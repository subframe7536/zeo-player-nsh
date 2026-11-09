//! 应用自更新：`nsh-updater` 的配置 / 清单校验 / 校验下载 / 替换安装，
//! 网络层用 `ureq`（GitHub Releases 的产物要跟随重定向）。只在打包运行
//! 时可用（`nsh package` 把 `nsh-updater.json` 写进资源目录）；开发 /
//! `cargo run` 时 [`supported`] 为假，界面不显示更新区。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use nsh_updater::{Config, Download, install::Installation};

/// `nsh.json`（编译期内嵌）：「关于」页的项目信息与无包时的版本兜底。
const NSH_JSON: &str = include_str!("../../../nsh.json");

fn nsh_json() -> &'static serde_json::Value {
    static V: OnceLock<serde_json::Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(NSH_JSON).expect("nsh.json"))
}

/// 应用 id（`nsh.json` 的 `id`）。
pub fn app_id() -> &'static str {
    nsh_json()["id"].as_str().expect("nsh.json id")
}

/// 应用名（`nsh.json` 的 `name`）。
pub fn app_name() -> &'static str {
    nsh_json()["name"].as_str().expect("nsh.json name")
}

/// 当前版本（`nsh.json` 的 `version`，semver）。
pub fn app_version() -> &'static str {
    nsh_json()["version"].as_str().expect("nsh.json version")
}

/// 构建号（`nsh.json` 的 `build`；没有就 0）。
pub fn app_build() -> u64 {
    nsh_json()["build"].as_u64().unwrap_or(0)
}

/// 打包写入的更新配置（`nsh-updater.json` 在资源目录 `nsh/assets/` 下）。
fn config_bytes() -> Option<Vec<u8>> {
    let exe = std::env::current_exe().ok()?;
    let dir = if cfg!(target_os = "macos") {
        exe.parent()?.parent()?.join("Resources/nsh/assets")
    } else {
        exe.parent()?.join("nsh/assets")
    };
    std::fs::read(dir.join(nsh_updater::CONFIG_ASSET)).ok()
}

/// 打包运行才支持自更新（开发运行解析不到配置）。
pub fn supported() -> bool {
    config_bytes()
        .and_then(|b| Config::parse(&b).ok())
        .is_some()
}

/// 一条可用更新（清单校验过、版本更新、本平台有产物）。
#[derive(Debug, Clone)]
pub struct Update {
    /// 新版本号。
    pub version: String,
    /// 更新说明（清单的 notes）。
    pub notes: Option<String>,
    /// 产物 / 页面地址。
    pub url: String,
    /// 产物可下载安装（`sha256` 存在 = 是归档，不是跳转页）。
    pub can_download: bool,
    /// 可原地替换安装（应用目录用户可写）。
    pub can_install: bool,
    sha256: Option<String>,
    size: Option<u64>,
}

/// 检查更新（有网络 IO，调用方放后台线程）。打包外运行返回
/// `Err("开发运行…")`；没有更新返回 `Ok(None)`。
pub fn check() -> Result<Option<Update>, String> {
    let bytes = config_bytes().ok_or_else(|| "开发运行没有更新渠道".to_string())?;
    let config = Config::parse(&bytes).map_err(|e| e.to_string())?;
    let manifest = get(&config.manifest_url())?;
    let signature = config.signature_url().map(|u| get(&u)).transpose()?;
    let found = config
        .check(&manifest, signature.as_deref(), &nsh_updater::platform())
        .map_err(|e| e.to_string())?;
    Ok(found.map(|a| {
        let can_install = Installation::current(app_id(), &bytes).is_some_and(|i| i.can_install());
        Update {
            version: a.version,
            notes: a.notes,
            url: a.url,
            can_download: a.sha256.is_some(),
            can_install,
            sha256: a.sha256,
            size: a.size,
        }
    }))
}

/// 下载更新到临时目录并校验（网络 IO，后台线程）。`progress` 收到
/// （已下载字节, 总字节）。
pub fn download(
    update: &Update,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let ext = Path::new(update.url.split('?').next().unwrap_or(&update.url))
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("pkg");
    let path = std::env::temp_dir().join(format!("{}-update-{}.{}", app_id(), update.version, ext));
    let response = get_response(&update.url)?;
    let mut sum = Download::new(update.sha256.as_deref().unwrap_or(""), update.size);
    let mut reader = response.into_body().into_reader();
    let mut file =
        std::fs::File::create(&path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut buffer = vec![0u8; 256 * 1024];
    let mut received = 0u64;
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        sum.update(&buffer[..n]).map_err(|e| e.to_string())?;
        file.write_all(&buffer[..n])
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        received += n as u64;
        progress(received, update.size);
    }
    sum.finish().map_err(|e| e.to_string())?;
    Ok(path)
}

/// 用下载的归档替换当前应用（macOS `.app` 目录、Windows 程序目录）。
/// 成功后由调用方重启（`nsh_core::element::desktop::restart`）。
pub fn install(archive: &Path) -> Result<(), String> {
    let bytes = config_bytes().ok_or_else(|| "开发运行没有安装源".to_string())?;
    let installation =
        Installation::current(app_id(), &bytes).ok_or_else(|| "不是打包安装".to_string())?;
    installation.install(archive).map_err(|e| e.to_string())
}

/// ureq 3 的 GET（跟随重定向：GitHub 的 release asset 在另一台主机）。
fn get_response(url: &str) -> Result<ureq::http::Response<ureq::Body>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("GET {url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("GET {url}: {}", response.status()));
    }
    Ok(response)
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    Ok(get_response(url)?
        .into_body()
        .read_to_vec()
        .map_err(|e| format!("GET {url}: {e}"))?)
}
