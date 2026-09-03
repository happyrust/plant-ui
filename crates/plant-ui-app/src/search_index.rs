//! 子串索引在宿主这一侧的家：什么时候去开、什么时候去建、建的过程中界面看见什么。
//!
//! 索引本身（schema、分词、验真、排序）住在 `plant_ui_data::name_index`；这里只
//! 管生命周期，三条规矩：
//!
//! 1. **单飞**。启动、取回工作、重连、批次到终态、`reindex` 五个触发点都往这儿
//!    撞，同一时刻只许一个在跑，后来的直接掉头——它们要的是同一件事。
//! 2. **重建期间旧索引继续服务**。换代只发生在新的那份开起来之后；建失败就什么
//!    都不动，下一个触发点自然重试。
//! 3. **目录名即戳**。在就开、不在就建，不需要在索引里另存一份元数据再去比对。
//!
//! 浏览器端整块是个空壳：`plant-ui-data` 的索引模块只在原生端存在，wasm 上子串
//! 一路回「不提供」（ADR-0023 决定 10）。

use std::sync::mpsc;

use crate::data::Evt;

/// 子串索引这一刻的状态。同时喂两处：下拉里那行说明，和 logs 面板。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchIndexState {
    /// 就绪，索引里有这么多个名字。
    Ready(u64),
    /// 正在建，已经拉完 `done`/`total` 个设计库。
    Building { done: usize, total: usize },
    /// 建不起来。旧索引（若在）继续服务。
    Failed(String),
    /// 这个构建不提供子串搜索。
    Off,
}

/// 一次搜索里子串那一路的结果。
///
/// 与前缀那一路的 `Result` 分开成两种类型不是啰嗦：子串没就绪和「搜索失败」在
/// 界面上该说的话完全不同，前缀那半照样有效。
#[derive(Debug, Clone)]
pub enum SubstringHits {
    Hits(Vec<plant_ui_data::NameHit>),
    /// 索引还没就绪：首次启动，或者数据变了正在重建。
    Building,
    /// 这一刻没有索引可查（浏览器端，或建失败还没重试上）。
    Unavailable,
}

/// 索引取材与落脚的范围。`ns` + `mdb` 决定它住哪个目录，`dbnums` 决定收哪些库。
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub ns: String,
    pub mdb: String,
    pub dbnums: Vec<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::SearchIndex;
#[cfg(target_arch = "wasm32")]
pub use stub::SearchIndex;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, RwLock};

    use anyhow::{Context, Result, ensure};
    use plant_ui_data::name_index::{self, NameIndex};

    use super::{Scope, SearchIndexState, SubstringHits, emit};
    use crate::data::Evt;

    /// 索引目录的整体覆盖（绝对路径）。测试用，与 `PLANT_UI_SETTINGS_FILE` 同一
    /// 条路子——不覆盖时索引跟着发行包走。
    const DIR_ENV: &str = "PLANT_UI_SEARCH_INDEX_DIR";

    /// 一个随手可克隆的索引把手：数据线程持一份，每次搜索读一次。
    #[derive(Clone, Default)]
    pub struct SearchIndex(Arc<Shared>);

    #[derive(Default)]
    struct Shared {
        /// 在役的那一份。`None` = 还没有（首次启动，或上一次建失败）。
        open: RwLock<Option<Arc<NameIndex>>>,
        /// 单飞闸。
        working: AtomicBool,
    }

    impl SearchIndex {
        /// 查一次子串。亚毫秒，同步。
        ///
        /// 只在锁里克隆一下 `Arc` 就放手：查询本身不该压着写锁那一侧的换代。
        pub fn search(&self, needle: &str, limit: usize) -> Result<SubstringHits> {
            let Ok(open) = self.0.open.read().map(|open| open.clone()) else {
                // 锁中毒 = 某次持锁时 panic 过。索引未必坏，但这个把手不能再信。
                return Ok(SubstringHits::Unavailable);
            };
            match open {
                Some(index) => Ok(SubstringHits::Hits(index.search(needle, limit)?)),
                None if self.0.working.load(Ordering::Relaxed) => Ok(SubstringHits::Building),
                None => Ok(SubstringHits::Unavailable),
            }
        }

        /// 校验戳，该开的开、该建的建。`force` = 跳过戳比对硬建（`reindex`）。
        ///
        /// **单飞**：已经有一个在跑就直接返回，不排队也不叠加——五个触发点要的
        /// 是同一件事，正在做的那一次做完就是最新的。
        pub async fn refresh(
            self,
            scope: Scope,
            force: bool,
            evt_tx: mpsc::Sender<Evt>,
            ctx: egui::Context,
        ) {
            // 一个设计库都没有时无从谈起：还没连上库，或者当前 MDB 就是空的。
            if scope.dbnums.is_empty() || self.0.working.swap(true, Ordering::SeqCst) {
                return;
            }
            let outcome = self.work(&scope, force, &evt_tx, &ctx).await;
            self.0.working.store(false, Ordering::SeqCst);
            emit(
                &evt_tx,
                &ctx,
                match outcome {
                    Ok(names) => SearchIndexState::Ready(names),
                    Err(error) => SearchIndexState::Failed(crate::logs::error_chain(&error)),
                },
            );
        }

        async fn work(
            &self,
            scope: &Scope,
            force: bool,
            evt_tx: &mpsc::Sender<Evt>,
            ctx: &egui::Context,
        ) -> Result<u64> {
            let root = root(scope)?;
            let stamp = name_index::stamp(&scope.dbnums).await?;
            let dir = root.join(stamp.dir_name());
            if !force && dir.is_dir() {
                // 开不动就当它不存在：目录残缺、格式对不上、被谁写坏——重建一份
                // 就是了，没有需要抢救的东西。
                if let Ok(index) = name_index::open(&dir) {
                    return Ok(self.install(index));
                }
            }
            // 强制重建要先把自己的句柄放掉。Windows 上开着的目录删不掉，而
            // `name_index::write` 删不掉旧目录时只会把新建的那份丢掉、拿回旧的
            // ——那正好等于 `reindex` 什么也没做。
            if force {
                self.clear();
            }
            let total = scope.dbnums.len();
            emit(evt_tx, ctx, SearchIndexState::Building { done: 0, total });
            let progress_tx = evt_tx.clone();
            let progress_ctx = ctx.clone();
            let index = name_index::build(&root, &stamp, move |done, total| {
                emit(
                    &progress_tx,
                    &progress_ctx,
                    SearchIndexState::Building { done, total },
                );
            })
            .await?;
            Ok(self.install(index))
        }

        /// 换代。返回新索引里的名字条数，日志要说这个数。
        fn install(&self, index: NameIndex) -> u64 {
            let names = index.len();
            if let Ok(mut open) = self.0.open.write() {
                *open = Some(Arc::new(index));
            }
            names
        }

        fn clear(&self) {
            if let Ok(mut open) = self.0.open.write() {
                *open = None;
            }
        }
    }

    /// 索引的家：`<exe 旁>/search-index/<ns>__<mdb>/`。
    ///
    /// 与 `settings.ron` 同一条房规——跟着发行包走，一份发行包一份索引；开发构建
    /// 落在 `target/debug/` 下，`cargo clean` 会一并带走，那是可接受的。
    fn root(scope: &Scope) -> Result<PathBuf> {
        if let Some(raw) = std::env::var_os(DIR_ENV).filter(|raw| !raw.is_empty()) {
            let path = PathBuf::from(raw);
            ensure!(
                path.is_absolute(),
                "{DIR_ENV} 必须是绝对路径：{}",
                path.display()
            );
            return Ok(path);
        }
        let exe = std::env::current_exe().context("读取可执行文件路径失败")?;
        Ok(exe
            .parent()
            .unwrap_or(Path::new("."))
            .join("search-index")
            .join(format!("{}__{}", slug(&scope.ns), slug(&scope.mdb))))
    }

    /// 目录名里只留字母数字，其余一律 `-`。MDB 名是 `/ALL` 这种带斜杠的东西，
    /// 原样进路径会平白多出一层目录。
    fn slug(raw: &str) -> String {
        let cleaned: String = raw
            .trim()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        match cleaned.trim_matches('-') {
            "" => "none".to_owned(),
            trimmed => trimmed.to_owned(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::slug;

        #[test]
        fn mdb_names_become_one_path_segment() {
            assert_eq!(slug("/ALL"), "ALL");
            assert_eq!(slug("1516"), "1516");
            assert_eq!(slug("/A B/C"), "A-B-C");
            // 全是分隔符的名字不能塌成空串——空串会让两个不同范围共用一个目录。
            assert_eq!(slug("//"), "none");
            assert_eq!(slug("  "), "none");
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod stub {
    use std::sync::mpsc;

    use super::{Scope, SearchIndexState, SubstringHits, emit};
    use crate::data::Evt;

    /// 浏览器端没有索引：`plant-ui-data` 的索引模块整块 `cfg(not(wasm32))`。
    #[derive(Clone, Default)]
    pub struct SearchIndex;

    impl SearchIndex {
        pub fn search(&self, _needle: &str, _limit: usize) -> anyhow::Result<SubstringHits> {
            Ok(SubstringHits::Unavailable)
        }

        pub async fn refresh(
            self,
            _scope: Scope,
            _force: bool,
            evt_tx: mpsc::Sender<Evt>,
            ctx: egui::Context,
        ) {
            emit(&evt_tx, &ctx, SearchIndexState::Off);
        }
    }
}

/// 广播一次状态并叫醒 UI。重建要跑几十秒，逐库的进度不叫醒的话没人看得见。
fn emit(evt_tx: &mpsc::Sender<Evt>, ctx: &egui::Context, state: SearchIndexState) {
    let _ = evt_tx.send(Evt::SearchIndex(state));
    ctx.request_repaint();
}
