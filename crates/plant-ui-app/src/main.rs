//! plant-ui-app：独立可执行外壳（eframe + plant-ui + plant-ui-data）。
//! 默认进入主工作台 S1；`--gallery` 进入 M0 组件画廊。

mod data;
mod fonts;
mod gallery;

use std::collections::{HashMap, HashSet};

use eframe::egui;
use plant_ui::Cmd;
use plant_ui::style::theme_tokens::{self, set_weight_families_ready};
use plant_ui::style::tokens::{Density, Tokens};
use plant_ui::vm::{TreeRowVm, TreeVm, WorkbenchVm};
use plant_ui::workbench::{self, WorkbenchState};
use plant_ui_data::{EleTreeNode, RefU64};

fn main() -> eframe::Result {
    let gallery = std::env::args().any(|a| a == "--gallery");
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
            let defs = fonts::definitions();
            let weights_ready = defs.font_data.contains_key(fonts::MEDIUM);
            cc.egui_ctx.set_fonts(defs);
            set_weight_families_ready(weights_ready);
            theme_tokens::apply(&cc.egui_ctx, &Tokens::dark(), Density::Standard);
            if gallery {
                Ok(Box::new(gallery::GalleryApp::default()))
            } else {
                Ok(Box::new(App::new(cc.egui_ctx.clone())))
            }
        }),
    )
}

/// App 侧的树结构缓存与展开状态；`vm.tree` 是它按展开状态展平后的产物。
#[derive(Default)]
struct TreeModel {
    /// SITE 根层（MDB 世界的下一层）。
    roots: Vec<EleTreeNode>,
    /// 已加载的子层，按父 refno 缓存。
    children: HashMap<RefU64, Vec<EleTreeNode>>,
    expanded: HashSet<RefU64>,
    /// 子层查询在途的节点。
    loading: HashSet<RefU64>,
}

impl TreeModel {
    /// 按展开状态 DFS 展平可见行。只在结构变化时调用，绘制层逐帧只读。
    fn flatten(&self) -> Vec<TreeRowVm> {
        fn walk(model: &TreeModel, nodes: &[EleTreeNode], depth: u16, rows: &mut Vec<TreeRowVm>) {
            for n in nodes {
                let refno = n.refno.refno();
                let expandable = (n.children_count > 0).then(|| model.expanded.contains(&refno));
                rows.push(TreeRowVm {
                    refno,
                    depth,
                    name: n.name.clone(),
                    noun: n.noun.clone(),
                    expandable,
                    loading: model.loading.contains(&refno),
                });
                if expandable == Some(true)
                    && let Some(kids) = model.children.get(&refno)
                {
                    walk(model, kids, depth + 1, rows);
                }
            }
        }
        let mut rows = Vec::new();
        walk(self, &self.roots, 0, &mut rows);
        rows
    }

    fn element_count(&self) -> usize {
        self.roots.len() + self.children.values().map(Vec::len).sum::<usize>()
    }
}

struct App {
    vm: WorkbenchVm,
    state: WorkbenchState,
    bridge: data::Bridge,
    tree: TreeModel,
}

impl App {
    fn new(ctx: egui::Context) -> Self {
        Self {
            // 连接前不摆任何工程数据：项目 / 库标识等 Ready 事件带真实值。
            vm: WorkbenchVm {
                user: std::env::var("USERNAME").unwrap_or_else(|_| "user".into()),
                ..Default::default()
            },
            state: WorkbenchState::default(),
            bridge: data::spawn(ctx),
            tree: TreeModel::default(),
        }
    }

    fn pump_events(&mut self) {
        let mut dirty = false;
        while let Ok(evt) = self.bridge.evt.try_recv() {
            match evt {
                data::Evt::Ready(Ok(info)) => {
                    self.vm.data_source_ok = true;
                    self.vm.project = info.project;
                    self.vm.db = db_label(&info.ns, &info.db_nums);
                    self.tree.roots = info.sites;
                    dirty = true;
                }
                data::Evt::Ready(Err(e)) => {
                    self.vm.tree = TreeVm::Failed(format!("数据源连接失败：{e:#}"));
                }
                data::Evt::Children(refno, Ok(kids)) => {
                    self.tree.loading.remove(&refno);
                    self.tree.children.insert(refno, kids);
                    dirty = true;
                }
                data::Evt::Children(refno, Err(e)) => {
                    // 展开失败就收回箭头，别让行卡在「加载中」；详情先落日志，
                    // M1-4 命令行视图接上后再上屏。
                    self.tree.loading.remove(&refno);
                    self.tree.expanded.remove(&refno);
                    eprintln!("[data] 子层加载失败 {refno:?}: {e:#}");
                    dirty = true;
                }
            }
        }
        if dirty {
            self.rebuild_tree();
        }
    }

    fn handle_cmds(&mut self, cmds: Vec<Cmd>) {
        let mut dirty = false;
        for cmd in cmds {
            match cmd {
                Cmd::SelectElement(refno) => {
                    self.vm.selected = Some(refno);
                }
                Cmd::ToggleExpand(refno) => {
                    if !self.tree.expanded.remove(&refno) {
                        self.tree.expanded.insert(refno);
                        if !self.tree.children.contains_key(&refno)
                            && self.tree.loading.insert(refno)
                        {
                            let _ = self.bridge.req.send(data::Req::Children(refno));
                        }
                    }
                    dirty = true;
                }
            }
        }
        if dirty {
            self.rebuild_tree();
        }
    }

    fn rebuild_tree(&mut self) {
        self.vm.tree = TreeVm::Ready(self.tree.flatten());
        self.vm.element_count = self.tree.element_count();
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump_events();
        let t = theme_tokens::current();
        let d = Density::Standard;
        let cmds = workbench::show(ui, &t, d, &self.vm, &mut self.state);
        self.handle_cmds(cmds);
    }
}
