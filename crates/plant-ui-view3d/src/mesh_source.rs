//! `meshes` 资产源：把网格文件的根目录从「资产根下固定的一层」变成一项可改的设置。
//!
//! 为什么要自己写一个 reader：`register_asset_source` 必须早于 `AssetPlugin`，而
//! `AssetSourceBuilder::with_reader` 那个闭包只在建源时调用一次——照搬
//! `FileAssetReader` 的话根目录就跟着进程一辈子，改设置只能重启。这里把根放在
//! 进程级的锁里、每次读文件现取，设置窗改完当场生效。
//!
//! 全局状态是刻意的，与 `model_update_api` 的服务地址同一套路：一个进程只注册一个
//! 源，源里也只有一个根，没有第二份可言。
//!
//! 浏览器端没有本地目录可谈，网格由站点供给，但同样走这个源（`web` 模块）：一个 SITE
//! 上万个网格文件，要排着队取、取不到要重来，这两件事默认源都不管。

/// 资产路径里的源名——`meshes://24381_46952.mesh` 里的 `meshes`。
pub const SOURCE_NAME: &str = "meshes";

/// 一个网格实例的资产路径。两端都经 `meshes` 源：桌面端读可换根的本地目录，浏览器端
/// 向站点资产根底下的 `meshes/` 取。
pub fn asset_path(geo_hash: &str) -> String {
    format!("{SOURCE_NAME}://{geo_hash}.mesh")
}

/// 目录里有没有至少一个 `.mesh`。
///
/// 只求「有没有」不求「有几个」：设置窗要抓的是**选错一层**——指到了资产根而不是它
/// 底下的 `meshes`。那种情况第一个条目就见分晓，而真实的网格目录动辄几万个文件，
/// 数完一遍要让界面停住。
#[cfg(not(target_arch = "wasm32"))]
pub fn contains_mesh_file(dir: &std::path::Path) -> std::io::Result<bool> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "mesh") {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{mesh_dir, set_mesh_dir, source_builder};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::{Path, PathBuf};
    use std::sync::{OnceLock, RwLock};

    use bevy::asset::io::{
        AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, PathStream, Reader,
        VecReader,
    };

    fn root() -> &'static RwLock<PathBuf> {
        static ROOT: OnceLock<RwLock<PathBuf>> = OnceLock::new();
        ROOT.get_or_init(|| RwLock::new(PathBuf::new()))
    }

    /// 当前网格目录。空路径表示还没人设过——那种状态下每次读都会落空。
    pub fn mesh_dir() -> PathBuf {
        root().read().unwrap_or_else(|err| err.into_inner()).clone()
    }

    /// 换网格目录。下一次读文件就走新目录。
    ///
    /// 已经加载成功的网格不用管：文件名是内容哈希，换个目录取到的是同一份内容。
    /// 要重来的只有上一轮失败的那些，那件事由 `View3d::retry_failed_meshes` 发起。
    pub fn set_mesh_dir(dir: impl Into<PathBuf>) {
        *root().write().unwrap_or_else(|err| err.into_inner()) = dir.into();
    }

    /// 注册到 `App::register_asset_source` 的源定义。必须在 `AssetPlugin` 之前注册。
    pub fn source_builder() -> AssetSourceBuilder {
        AssetSource::build().with_reader(|| Box::new(SwappableMeshReader))
    }

    struct SwappableMeshReader;

    impl SwappableMeshReader {
        fn resolve(path: &Path) -> PathBuf {
            mesh_dir().join(path)
        }

        /// 同步读整个文件。
        ///
        /// 走的是 IO 任务池的线程而不是主线程，网格文件又都是小文件，所以这里用
        /// `std::fs` 换掉一个额外依赖是划算的；`MeshLoader` 拿到 reader 之后本来
        /// 也是一次 `read_to_end`，中间并没有流式可言。
        fn read_file(full: PathBuf) -> Result<VecReader, AssetReaderError> {
            match std::fs::read(&full) {
                Ok(bytes) => Ok(VecReader::new(bytes)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Err(AssetReaderError::NotFound(full))
                }
                Err(error) => Err(error.into()),
            }
        }
    }

    /// 只给测试用：绕开 `AssetServer`，直接问这个 reader 拿字节。
    #[cfg(test)]
    pub(super) fn read_for_test(path: &str) -> Result<Vec<u8>, AssetReaderError> {
        bevy::tasks::block_on(async {
            let mut reader = SwappableMeshReader.read(Path::new(path)).await?;
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            Ok(bytes)
        })
    }

    impl AssetReader for SwappableMeshReader {
        async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
            Self::read_file(Self::resolve(path))
        }

        async fn read_meta<'a>(
            &'a self,
            path: &'a Path,
        ) -> Result<impl Reader + 'a, AssetReaderError> {
            let mut meta = Self::resolve(path).into_os_string();
            meta.push(".meta");
            Self::read_file(PathBuf::from(meta))
        }

        async fn read_directory<'a>(
            &'a self,
            path: &'a Path,
        ) -> Result<Box<PathStream>, AssetReaderError> {
            // 这个源只按内容哈希点名取文件，没有人枚举它；真要枚举，一个厂区
            // 几万个网格的目录列表也不该由资产系统来背。
            Err(AssetReaderError::NotFound(Self::resolve(path)))
        }

        async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
            Ok(Self::resolve(path).is_dir())
        }
    }
}

/// 浏览器端网格文件的地址前缀（末尾不带斜杠）：站点资产根底下的 `meshes`。
///
/// 与默认源拼出来的地址一致——改走这个源之前，网格就是从那里取的。
#[cfg(any(target_arch = "wasm32", test))]
fn web_mesh_base(asset_root: &str) -> String {
    if asset_root.is_empty() {
        return "meshes".to_owned();
    }
    format!("{}/meshes", asset_root.trim_end_matches('/'))
}

#[cfg(target_arch = "wasm32")]
pub use web::source_builder;

/// 浏览器端：向站点取网格文件。
///
/// 排队、重试、服务连不上时怎么收场，这些规矩在 `mesh_fetch` 里，桌面端的测试跑得到；
/// 这里只有它够不着的那一半——真的发 `fetch`、真的等 `setTimeout`。
#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::Cell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::Duration;

    use bevy::asset::io::{
        AssetReader, AssetReaderError, AssetSource, AssetSourceBuilder, PathStream, Reader,
        VecReader,
    };
    use wasm_bindgen::prelude::*;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{AbortController, RequestInit, Response};
    use web_time::Instant;

    use crate::mesh_fetch::{Failure, FetchError, Fetcher, Policy, Transport};

    #[wasm_bindgen]
    extern "C" {
        // 绑的是全局函数：页面主线程和 worker 里都叫这几个名字，不必先分辨全局对象是哪一种。
        #[wasm_bindgen(catch, js_name = fetch)]
        fn js_fetch(url: &str, init: &RequestInit) -> Result<js_sys::Promise, JsValue>;
        #[wasm_bindgen(js_name = setTimeout)]
        fn set_timeout(handler: &js_sys::Function, millis: i32) -> i32;
        #[wasm_bindgen(js_name = clearTimeout)]
        fn clear_timeout(handle: i32);
    }

    /// 注册到 `App::register_asset_source` 的源定义。必须在 `AssetPlugin` 之前注册。
    ///
    /// `asset_root` 是站点的资产根（与默认源同一个）；`concurrency` 是同时在途的请求数
    /// 上限，`None` 用缺省值。
    pub fn source_builder(asset_root: &str, concurrency: Option<usize>) -> AssetSourceBuilder {
        let base: Arc<str> = super::web_mesh_base(asset_root).into();
        // 通道在这里建好、各个 reader 共用：名额和「服务还通不通」的账只能有一份。
        let fetcher = Arc::new(Fetcher::new(
            Browser {
                started: Instant::now(),
            },
            Policy::new(concurrency),
        ));
        AssetSource::build().with_reader(move || {
            Box::new(WebMeshReader {
                base: Arc::clone(&base),
                fetcher: Arc::clone(&fetcher),
            })
        })
    }

    struct WebMeshReader {
        base: Arc<str>,
        fetcher: Arc<Fetcher<Browser>>,
    }

    impl AssetReader for WebMeshReader {
        async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
            let url = format!("{}/{}", self.base, path.to_string_lossy());
            match self.fetcher.fetch(&url).await {
                Ok(bytes) => Ok(VecReader::new(bytes)),
                Err(FetchError::NotFound) => Err(AssetReaderError::NotFound(PathBuf::from(url))),
                Err(FetchError::Rejected(status)) => Err(AssetReaderError::HttpError(status)),
                Err(error) => Err(std::io::Error::other(error.to_string()).into()),
            }
        }

        async fn read_meta<'a>(
            &'a self,
            path: &'a Path,
        ) -> Result<impl Reader + 'a, AssetReaderError> {
            // 网格没有 .meta，宿主也把 `AssetMetaCheck` 设成了 `Never`。真有人来问，也不为
            // 一个注定没有的文件去占一个名额。
            Err::<VecReader, _>(AssetReaderError::NotFound(path.to_owned()))
        }

        async fn read_directory<'a>(
            &'a self,
            path: &'a Path,
        ) -> Result<Box<PathStream>, AssetReaderError> {
            Err(AssetReaderError::NotFound(path.to_owned()))
        }

        async fn is_directory<'a>(&'a self, _path: &'a Path) -> Result<bool, AssetReaderError> {
            Ok(false)
        }
    }

    /// 浏览器给的那几样：`fetch`、`setTimeout`、一只单调钟。
    struct Browser {
        started: Instant,
    }

    impl Transport for Browser {
        async fn get(&self, url: &str, timeout: Duration) -> Result<Vec<u8>, Failure> {
            let controller = AbortController::new().map_err(|error| {
                Failure::Unreached(format!("请求没发出去：{}", describe(&error)))
            })?;
            let init = RequestInit::new();
            init.set_signal(Some(&controller.signal()));

            let timed_out = Rc::new(Cell::new(false));
            let _timer = Timer::start(timeout, {
                let controller = controller.clone();
                let timed_out = Rc::clone(&timed_out);
                move || {
                    timed_out.set(true);
                    controller.abort();
                }
            });

            let outcome = request(url, &init).await;
            if outcome.is_err() {
                // 没成的这一次可能还留着一份没读的正文（应答头到了、状态码不对就没往下读），
                // 掐掉；已经结束的请求掐了也无妨。
                controller.abort();
            }
            match outcome {
                Err(_) if timed_out.get() => Err(Failure::Unreached(format!(
                    "超过 {} 秒没有取完",
                    timeout.as_secs()
                ))),
                outcome => outcome,
            }
        }

        async fn sleep(&self, wait: Duration) {
            let woken = js_sys::Promise::new(&mut |resolve, _reject| {
                set_timeout(&resolve, millis(wait));
            });
            // 这个 Promise 只会兑现，不会拒绝。
            let _ = JsFuture::from(woken).await;
        }

        fn now(&self) -> Duration {
            self.started.elapsed()
        }
    }

    /// 发一次请求并把正文读完。
    ///
    /// 每一步的失败都变成返回值，没有一处 `unwrap`：应答头到了不等于文件到了，连接可以
    /// 在正文读到一半时断开——Bevy 自带的读取器就是在那一步 panic 的。
    async fn request(url: &str, init: &RequestInit) -> Result<Vec<u8>, Failure> {
        let pending = js_fetch(url, init)
            .map_err(|error| Failure::Unreached(format!("请求没发出去：{}", describe(&error))))?;
        let response = JsFuture::from(pending)
            .await
            .map_err(|error| Failure::Unreached(format!("没有应答：{}", describe(&error))))?
            .dyn_into::<Response>()
            .map_err(|other| Failure::Unreached(format!("应答不是 Response：{other:?}")))?;
        if let Some(failure) = Failure::from_status(response.status()) {
            return Err(failure);
        }
        let body = response
            .array_buffer()
            .map_err(|error| Failure::Unreached(format!("正文没读完：{}", describe(&error))))?;
        let buffer = JsFuture::from(body)
            .await
            .map_err(|error| Failure::Unreached(format!("正文没读完：{}", describe(&error))))?;
        Ok(js_sys::Uint8Array::new(&buffer).to_vec())
    }

    /// 把 JS 抛出来的东西写成一句话。
    ///
    /// `Error` 的 `name` / `message` 不可枚举，`JSON.stringify` 只会给出 `{}`，所以点名取。
    fn describe(value: &JsValue) -> String {
        match value.dyn_ref::<js_sys::Error>() {
            Some(error) => format!(
                "{}: {}",
                String::from(error.name()),
                String::from(error.message())
            ),
            None => value.as_string().unwrap_or_else(|| format!("{value:?}")),
        }
    }

    fn millis(duration: Duration) -> i32 {
        i32::try_from(duration.as_millis()).unwrap_or(i32::MAX)
    }

    /// 到点调一次 `fire` 的定时器；先被丢掉就取消。
    struct Timer {
        handle: i32,
        _fire: Closure<dyn FnMut()>,
    }

    impl Timer {
        fn start(after: Duration, fire: impl FnMut() + 'static) -> Self {
            let fire = Closure::<dyn FnMut()>::new(fire);
            let handle = set_timeout(fire.as_ref().unchecked_ref(), millis(after));
            Self {
                handle,
                _fire: fire,
            }
        }
    }

    impl Drop for Timer {
        fn drop(&mut self) {
            clear_timeout(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 网格根是进程级的，动它的测试必须排队——`cargo test` 默认并行跑，
    /// 两个测试各设各的根就会互相看到对方那一份。
    #[cfg(not(target_arch = "wasm32"))]
    fn root_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// 建一个本次测试专用的空目录，名字带进程号与纳秒，跑多少遍都不打架。
    #[cfg(not(target_arch = "wasm32"))]
    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "plant-ui-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        base
    }

    #[test]
    fn mesh_paths_go_through_the_meshes_source() {
        assert_eq!(asset_path("24381_46952"), "meshes://24381_46952.mesh");
    }

    /// 浏览器端改走 `meshes` 源之后，取文件的地址必须还是默认源先前拼出来的那个：
    /// 资产根、`meshes`、文件名，三段之间各一个斜杠。
    #[test]
    fn web_meshes_are_fetched_from_under_the_asset_root() {
        assert_eq!(
            web_mesh_base("http://127.0.0.1:18722/assets"),
            "http://127.0.0.1:18722/assets/meshes"
        );
        assert_eq!(
            web_mesh_base("http://127.0.0.1:18722/assets/"),
            "http://127.0.0.1:18722/assets/meshes"
        );
        assert_eq!(web_mesh_base("assets"), "assets/meshes");
        assert_eq!(web_mesh_base(""), "meshes");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_directory_without_mesh_files_is_reported_as_such() {
        let base = scratch_dir("mesh-probe");
        let empty = base.join("assets");
        let filled = base.join("assets/meshes");
        std::fs::create_dir_all(&filled).unwrap();
        std::fs::write(filled.join("24381_46952.mesh"), b"not a real mesh").unwrap();

        assert!(!contains_mesh_file(&empty).unwrap());
        assert!(contains_mesh_file(&filled).unwrap());
        assert!(contains_mesh_file(&base.join("nope")).is_err());

        std::fs::remove_dir_all(base).unwrap();
    }

    /// 换根之后读到的必须是另一个目录里那一份。
    ///
    /// 这一条是整个功能的地基：`AssetSourceBuilder` 那个闭包只在建源时调一次，
    /// 所以「改完设置当场生效」全靠 reader 每次读现取根目录。
    ///
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn reading_follows_whatever_root_is_current() {
        use bevy::asset::io::AssetReaderError;

        let _guard = root_guard();
        let base = scratch_dir("mesh-root");
        let first = base.join("first");
        let second = base.join("second");
        for dir in [&first, &second] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(first.join("a.mesh"), b"first").unwrap();
        std::fs::write(second.join("a.mesh"), b"second").unwrap();

        set_mesh_dir(&first);
        assert_eq!(native::read_for_test("a.mesh").unwrap(), b"first");
        set_mesh_dir(&second);
        assert_eq!(native::read_for_test("a.mesh").unwrap(), b"second");
        set_mesh_dir(base.join("nowhere"));
        assert!(matches!(
            native::read_for_test("a.mesh"),
            Err(AssetReaderError::NotFound(_))
        ));

        std::fs::remove_dir_all(base).unwrap();
    }

    /// 整条链走一遍：注册这个源、按 `meshes://` 取文件、换根、把先前失败的那个
    /// 重发一次并真的读出来。
    ///
    /// 这是设置窗保存那一刻发生的全部事情。尤其押在最后一步上——对**已经失败过**的资产
    /// 再 `load` 一次，`AssetServer` 认不认账、状态什么时候翻，光看文档看不出来。
    /// `retry_meshes` 重发用的就是这一下。
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_missing_mesh_loads_after_the_dir_is_switched() {
        use aios_core::shape::pdms_shape::PlantMesh;
        use bevy::asset::{AssetApp, AssetPlugin, AssetServer, LoadState};
        use bevy::prelude::*;

        let _guard = root_guard();
        let base = scratch_dir("mesh-switch");
        let empty = base.join("empty");
        let filled = base.join("filled");
        for dir in [&empty, &filled] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let mesh = PlantMesh {
            indices: vec![0, 1, 2],
            vertices: vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            normals: vec![Vec3::Z; 3],
            ..Default::default()
        };
        std::fs::write(filled.join("probe.mesh"), mesh.ser_to_bytes()).unwrap();

        set_mesh_dir(&empty);
        let mut app = App::new();
        app.register_asset_source(SOURCE_NAME, source_builder())
            .add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Mesh>()
            .init_asset_loader::<crate::MeshLoader>();
        let path = asset_path("probe");
        let handle: Handle<Mesh> = app.world().resource::<AssetServer>().load(path.clone());

        assert!(
            settles(&mut app, &handle, |state| matches!(
                state,
                Some(LoadState::Failed(_))
            )),
            "空目录里不该读出网格"
        );

        set_mesh_dir(&filled);
        let again: Handle<Mesh> = app.world().resource::<AssetServer>().load(path.clone());
        assert_eq!(again.id(), handle.id(), "重发拿到的应当还是同一个资产");
        // 重发当场就离开失败态，不等文件读回来——此后再见到的失败态就是这一轮的结论。
        assert!(matches!(
            app.world()
                .resource::<AssetServer>()
                .get_load_state(&handle),
            Some(LoadState::Loading)
        ));
        assert!(
            settles(&mut app, &handle, |state| matches!(
                state,
                Some(LoadState::Loaded)
            )),
            "换过网格目录之后，先前失败的那个应当读得出来"
        );

        std::fs::remove_dir_all(base).unwrap();
    }

    /// 转帧直到这个句柄的状态合意；等太久就是没等到。
    #[cfg(not(target_arch = "wasm32"))]
    fn settles(
        app: &mut bevy::prelude::App,
        handle: &bevy::prelude::Handle<bevy::prelude::Mesh>,
        wanted: impl Fn(Option<bevy::asset::LoadState>) -> bool,
    ) -> bool {
        for _ in 0..200 {
            app.update();
            let state = app
                .world()
                .resource::<bevy::asset::AssetServer>()
                .get_load_state(handle);
            if wanted(state) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        false
    }
}
