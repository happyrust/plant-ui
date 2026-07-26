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
use plant_ui::vm::{PropKind, PropRowVm, PropsDataVm, PropsVm, TreeRowVm, TreeVm, WorkbenchVm};
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
                // 晚到的旧选中结果直接丢弃，属性面板只认当前选中。
                data::Evt::Props(refno, result) if self.vm.selected == Some(refno) => {
                    self.vm.props = match result {
                        Ok(kvs) => PropsVm::Ready(self.build_props(refno, kvs)),
                        Err(e) => PropsVm::Failed(format!("属性查询失败：{e:#}")),
                    };
                }
                data::Evt::Props(..) => {}
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
                    if self.vm.selected != Some(refno) {
                        self.vm.selected = Some(refno);
                        self.vm.props = PropsVm::Loading;
                        let _ = self.bridge.req.send(data::Req::Props(refno));
                    }
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
                // 独立应用里派发层就是这里：先只落到内存 Vm，写回 SurrealDB 要连
                // 会话号、撤销、依赖重算一起做，归 M3-3 命令派发接现有 Event。
                Cmd::EditAttr { refno, attr, value } => {
                    if let PropsVm::Ready(data) = &mut self.vm.props
                        && data.refno == refno
                    {
                        let rows = data
                            .common
                            .iter_mut()
                            .chain(&mut data.attrs)
                            .chain(&mut data.udas);
                        for row in rows {
                            if row.attr == attr {
                                row.value = value.clone();
                                row.muted = false;
                                break;
                            }
                        }
                        if attr == "NAME" {
                            data.name = value.clone();
                        }
                    }
                    eprintln!("[edit] {refno:?} {attr} = {value}（未写回数据库）");
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
                    common.insert("TYPE".into(), PropRowVm {
                        key: "类型".into(),
                        ..row
                    });
                }
                "NAME" => {
                    data.name = row.value.clone();
                    common.insert("NAME".into(), PropRowVm {
                        key: "名称".into(),
                        ..row
                    });
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.pump_events();
        let t = theme_tokens::current();
        let d = Density::Standard;
        let cmds = workbench::show(ui, &t, d, &self.vm, &mut self.state);
        self.handle_cmds(cmds);
    }
}
