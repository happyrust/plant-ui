//! `meshes` 资产源：把网格文件的根目录从「资产根下固定的一层」变成一项可改的设置。
//!
//! 为什么要自己写一个 reader：`register_asset_source` 必须早于 `AssetPlugin`，而
//! `AssetSourceBuilder::with_reader` 那个闭包只在建源时调用一次——照搬
//! `FileAssetReader` 的话根目录就跟着进程一辈子，改设置只能重启。这里把根放在
//! 进程级的锁里、每次读文件现取，设置窗改完当场生效。
//!
//! 全局状态是刻意的，与 `model_update_api` 的服务地址同一套路：一个进程只注册一个
//! 源，源里也只有一个根，没有第二份可言。

/// 资产路径里的源名——`meshes://24381_46952.mesh` 里的 `meshes`。
pub const SOURCE_NAME: &str = "meshes";

/// 一个网格实例的资产路径。
///
/// 浏览器端没有本地目录可谈，网格和字体贴图一样由站点供给，所以那一侧仍旧走默认源
/// 底下的 `meshes/` 子目录，不经过这个可换根的源。
#[cfg(not(target_arch = "wasm32"))]
pub fn asset_path(geo_hash: &str) -> String {
    format!("{SOURCE_NAME}://{geo_hash}.mesh")
}

#[cfg(target_arch = "wasm32")]
pub fn asset_path(geo_hash: &str) -> String {
    format!("{SOURCE_NAME}/{geo_hash}.mesh")
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
    fn native_paths_go_through_the_swappable_source() {
        let path = asset_path("24381_46952");
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(path, "meshes://24381_46952.mesh");
        #[cfg(target_arch = "wasm32")]
        assert_eq!(path, "meshes/24381_46952.mesh");
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
    /// 这是设置窗保存那一刻发生的全部事情。尤其押在最后一步上——`AssetServer::reload`
    /// 对**已经失败过**的资产还认不认账，光看文档看不出来。
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
        app.world().resource::<AssetServer>().reload(path.as_str());
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
