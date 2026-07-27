//! plant-ui-app：独立可执行外壳（eframe + plant-ui + plant-ui-data）。
//! 默认进入主工作台 S1；`--gallery` 进入 M0 组件画廊。

mod console;
mod data;
mod gallery;
mod model_update_api;
mod textures;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use console::Retry;
use eframe::egui;
use plant_ui::Cmd;
use plant_ui::fonts;
use plant_ui::model_update::{
    self, Run as ModelUpdateRun, State as ModelUpdateState, Vm as ModelUpdateVm,
};
use plant_ui::project_picker::{self, State as ProjectPickerState};
use plant_ui::settings::{self, State as SettingsState};
use plant_ui::style::theme_tokens::{self, set_weight_families_ready};
use plant_ui::style::tokens::{Density, Tokens};
use plant_ui::vm::{
    LogElement, PropKind, PropRowVm, PropsDataVm, PropsVm, TreeRowVm, TreeVm, View3dVm, WorkbenchVm,
};
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
            let (defs, warnings) = fonts::definitions();
            let weights_ready = defs.font_data.contains_key(fonts::MEDIUM);
            cc.egui_ctx.set_fonts(defs);
            set_weight_families_ready(weights_ready);
            theme_tokens::apply(&cc.egui_ctx, &Tokens::light(), Density::Standard);
            if gallery {
                Ok(Box::new(gallery::GalleryApp::default()))
            } else {
                Ok(Box::new(App::new(cc.egui_ctx.clone(), warnings)))
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
    /// 子 -> 父。命令行定位要顺着它把挡在目标前面的祖先一路展开。
    parent: HashMap<RefU64, RefU64>,
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

    /// 从最近的父到 SITE 根的祖先链；不在已加载的树里就是 None。
    ///
    /// 不必为此发查询：能被日志提到的元素都是从某个父层查回来的，那一层连同它上面
    /// 每一层都还在缓存里（缓存只增不减）。
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
    project_picker_state: ProjectPickerState,
    settings_state: SettingsState,
    model_api_url: String,
    last_model_poll: Instant,
    pending_model_polls: HashSet<String>,
    bridge: data::Bridge,
    tree: TreeModel,
    console: console::Console,
    /// 占位纹理的所有者：句柄一丢纹理就被回收，Vm 里那个 id 会指向空。
    _view3d_tex: Option<egui::TextureHandle>,
}

impl App {
    fn new(ctx: egui::Context, font_warnings: Vec<String>) -> Self {
        let mut app = Self {
            // 连接前不摆任何工程数据：项目 / 库标识等 Ready 事件带真实值。
            vm: WorkbenchVm {
                user: std::env::var("USERNAME").unwrap_or_else(|_| "user".into()),
                ..Default::default()
            },
            state: WorkbenchState::default(),
            model_update: ModelUpdateVm::default(),
            model_update_state: ModelUpdateState::default(),
            project_picker_state: ProjectPickerState::new(Vec::new(), false),
            settings_state: SettingsState::default(),
            model_api_url: model_update_api::base_url(),
            last_model_poll: Instant::now(),
            pending_model_polls: HashSet::new(),
            bridge: data::spawn(ctx.clone()),
            tree: TreeModel::default(),
            console: console::Console::default(),
            _view3d_tex: None,
        };
        for w in font_warnings {
            app.console.warn(&mut app.vm.console, w);
        }
        match textures::viewport_placeholder(&ctx) {
            Ok(tex) => {
                app.vm.view3d = Some(View3dVm {
                    texture: tex.id(),
                    size: tex.size_vec2(),
                    live: false,
                });
                app._view3d_tex = Some(tex);
            }
            // 缺一张占位图不该拦住启动，但也不能悄悄少画一块——挂到命令行上。
            Err(e) => app
                .console
                .error(&mut app.vm.console, "三维视口占位纹理载入失败", &e, None),
        }
        app.console.info(&mut app.vm.console, "正在连接数据源…");
        app
    }

    fn pump_events(&mut self) {
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
                    self.console.info(&mut self.vm.console, msg);
                    self.vm.data_source_ok = true;
                    self.vm.project = info.project;
                    self.vm.db = db_label(&info.ns, &info.db_nums);
                    self.tree.roots = info.sites;
                    dirty = true;
                }
                data::Evt::Ready(Err(e)) => {
                    self.vm.tree =
                        TreeVm::Failed(format!("数据源连接失败：{}", console::error_chain(&e)));
                    self.console.error(
                        &mut self.vm.console,
                        "数据源连接失败",
                        &e,
                        Some(Retry::Connect),
                    );
                }
                data::Evt::Children(refno, Ok(kids)) => {
                    self.tree.loading.remove(&refno);
                    let el = self.tree.element(refno);
                    let msg = format!("展开：{} 个子元素", kids.len());
                    self.console.info_of(&mut self.vm.console, el, msg);
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
                    let el = self.tree.element(refno);
                    self.console.error_of(
                        &mut self.vm.console,
                        el,
                        "子层加载失败",
                        &e,
                        Some(Retry::Children(refno)),
                    );
                    dirty = true;
                }
                // 晚到的旧选中结果直接丢弃，属性面板只认当前选中。
                data::Evt::Props(refno, result) if self.vm.selected == Some(refno) => {
                    self.vm.props = match result {
                        Ok(kvs) => PropsVm::Ready(self.build_props(refno, kvs)),
                        Err(e) => {
                            let el = self.tree.element(refno);
                            self.console.error_of(
                                &mut self.vm.console,
                                el,
                                "属性查询失败",
                                &e,
                                Some(Retry::Props(refno)),
                            );
                            PropsVm::Failed(format!("属性查询失败：{}", console::error_chain(&e)))
                        }
                    };
                }
                data::Evt::Props(..) => {}
                data::Evt::ModelUpdatePreview(result) => match result {
                    Ok(preview) => {
                        self.console.info(
                            &mut self.vm.console,
                            format!(
                                "模型更新预览完成：{} 个设计库，执行范围 dbnum + sesno",
                                preview.dbnums.len()
                            ),
                        );
                        self.model_update = ModelUpdateVm::Ready(preview);
                    }
                    Err(error) => {
                        let detail = console::error_chain(&error);
                        self.console
                            .error(&mut self.vm.console, "模型更新预览失败", &error, None);
                        self.model_update = ModelUpdateVm::Failed(detail);
                    }
                },
                data::Evt::ModelUpdateExecute(result) => match result {
                    Ok(accepted) => {
                        self.console.info(
                            &mut self.vm.console,
                            format!("模型增量更新任务已提交：{}", accepted.task_id),
                        );
                        self.last_model_poll = Instant::now() - Duration::from_secs(1);
                        self.model_update = ModelUpdateVm::Running {
                            run: ModelUpdateRun {
                                task_id: accepted.task_id,
                                kind: accepted.kind,
                                state: accepted.state,
                                project: self.vm.project.clone(),
                                ..Default::default()
                            },
                            poll_error: None,
                        };
                    }
                    Err(error) => {
                        let detail = console::error_chain(&error);
                        self.console.error(
                            &mut self.vm.console,
                            "提交模型增量更新失败",
                            &error,
                            None,
                        );
                        self.model_update = ModelUpdateVm::Failed(detail);
                    }
                },
                data::Evt::ModelUpdateTask(run_id, result) => {
                    self.pending_model_polls.remove(&run_id);
                    if let ModelUpdateVm::Running { run, poll_error } = &mut self.model_update {
                        match result {
                            Ok(latest) => {
                                if latest.task_id == run_id {
                                    *run = latest;
                                }
                                *poll_error = None;
                            }
                            Err(error) => *poll_error = Some(console::error_chain(&error)),
                        }
                    }
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
                Cmd::SelectElement(refno) => self.select(refno),
                // 命令行点元素名过来的：先把挡着的祖先展开、记下要滚到哪一行，
                // 再走一遍与树上点选相同的选中路径，属性视图自然跟上。
                Cmd::LocateElement(refno) => {
                    match self.tree.ancestors(refno) {
                        Some(chain) => {
                            for ancestor in chain {
                                self.tree.expanded.insert(ancestor);
                            }
                            self.vm.tree_reveal = Some(refno);
                            dirty = true;
                        }
                        // 按缓存只增不减的前提到不了这里。真到了也得说一声，
                        // 不能点了没动静。
                        None => {
                            let el = self.tree.element(refno);
                            self.console.warn_of(
                                &mut self.vm.console,
                                el,
                                "不在已加载的模型树里，只切了属性",
                            );
                        }
                    }
                    self.select(refno);
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
                    self.vm.props.edit(refno, &attr, value.clone());
                    let el = self.tree.element(refno);
                    let msg = format!("{attr} = {value}（仅改内存，未写回数据库）");
                    self.console.warn_of(&mut self.vm.console, el, msg);
                }
                // 面板错误态上的「重试」，与命令行那条走同一段处置逻辑。
                Cmd::Reconnect => self.reconnect(),
                Cmd::RetryProps(refno) if self.vm.selected == Some(refno) => {
                    self.refetch_props(refno)
                }
                Cmd::RetryProps(_) => {}
                Cmd::RetryLog(id) => match self.console.retry_of(id) {
                    Some(Retry::Connect) => self.reconnect(),
                    Some(Retry::Children(refno)) => {
                        // 重试即重新展开：箭头上次失败时被收回了，这里一并推回展开态。
                        self.tree.expanded.insert(refno);
                        if self.tree.loading.insert(refno) {
                            let _ = self.bridge.req.send(data::Req::Children(refno));
                        }
                        dirty = true;
                    }
                    Some(Retry::Props(refno)) if self.vm.selected == Some(refno) => {
                        self.refetch_props(refno)
                    }
                    // 选中早就挪走的话，属性重查回来也没地方摆，直接跳过。
                    _ => {}
                },
                Cmd::ClearConsole => self.console.clear(&mut self.vm.console),
                Cmd::OpenProjectPicker => self.project_picker_state.open = true,
                Cmd::LoadProject(_) => self.project_picker_state.open = false,
                Cmd::OpenSettings => self.settings_state.open(),
                Cmd::OpenModelUpdate => {
                    self.model_update_state.open = true;
                    if !matches!(
                        self.model_update,
                        ModelUpdateVm::Loading
                            | ModelUpdateVm::Starting
                            | ModelUpdateVm::Running { .. }
                    ) {
                        self.refresh_model_update();
                    }
                }
                Cmd::RefreshModelUpdate => self.refresh_model_update(),
                Cmd::ExecuteModelUpdate => {
                    self.model_update = ModelUpdateVm::Starting;
                    let _ = self.bridge.req.send(data::Req::ModelUpdateExecute {
                        base: self.model_api_url.clone(),
                        project: self.vm.project.clone(),
                    });
                }
            }
        }
        if dirty {
            self.rebuild_tree();
        }
    }

    /// 选中元素并让属性视图跟上。重复选同一个不再发查询。
    fn select(&mut self, refno: RefU64) {
        if self.vm.selected == Some(refno) {
            return;
        }
        self.vm.selected = Some(refno);
        self.refetch_props(refno);
    }

    fn refetch_props(&mut self, refno: RefU64) {
        self.vm.props = PropsVm::Loading;
        let _ = self.bridge.req.send(data::Req::Props(refno));
    }

    fn reconnect(&mut self) {
        // 新连接必须从空缓存开始：否则失败时状态栏和属性仍在说旧模型已经就绪。
        self.tree = TreeModel::default();
        self.vm.data_source_ok = false;
        self.vm.project.clear();
        self.vm.db.clear();
        self.vm.element_count = 0;
        self.vm.selected = None;
        self.vm.tree_reveal = None;
        self.vm.tree = TreeVm::Loading;
        self.vm.props = PropsVm::Uninit;
        self.console.info(&mut self.vm.console, "正在重连数据源…");
        let _ = self.bridge.req.send(data::Req::Reconnect);
    }

    fn refresh_model_update(&mut self) {
        self.model_update = ModelUpdateVm::Loading;
        let _ = self.bridge.req.send(data::Req::ModelUpdatePreview {
            base: self.model_api_url.clone(),
            project: self.vm.project.clone(),
        });
    }

    fn poll_model_update(&mut self, ctx: &egui::Context) {
        let ModelUpdateVm::Running { run, .. } = &self.model_update else {
            return;
        };
        if run.terminal() {
            return;
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        if self.last_model_poll.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_model_poll = Instant::now();
        let run_id = run.task_id.clone();
        if self.pending_model_polls.insert(run_id.clone()) {
            let _ = self.bridge.req.send(data::Req::ModelUpdateTask {
                base: self.model_api_url.clone(),
                run_id,
            });
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
        let d = self.settings_state.saved.density;
        let mut cmds = workbench::show(ui, &t, d, &self.vm, &mut self.state);
        cmds.extend(model_update::show(
            ui.ctx(),
            &t,
            d,
            &self.model_api_url,
            &self.model_update,
            &mut self.model_update_state,
        ));
        cmds.extend(project_picker::show(
            ui.ctx(),
            &t,
            d,
            &mut self.project_picker_state,
        ));
        if let Some(saved) = settings::show(ui.ctx(), &t, d, &mut self.settings_state) {
            theme_tokens::apply(ui.ctx(), &saved.theme.tokens(), saved.density);
            ui.ctx().request_repaint();
        }
        // 定位请求只管一帧：树刚才已经滚过去了，绘制层只读、消费不掉自己。
        self.vm.tree_reveal = None;
        self.handle_cmds(cmds);
        self.poll_model_update(ui.ctx());
        // 这一帧才记下的定位要等下一帧才画得出来，别让界面停在这儿等下一次输入。
        if self.vm.tree_reveal.is_some() {
            ui.ctx().request_repaint();
        }
    }
}
