use aios_core::RefU64;

use crate::style::tokens::Status;

/// 主工作台视图模型（外壳字段随 M1 各视图逐步补齐）。
///
/// 外壳原则：宁可少一格，也不摆一个永远是 0 的假数字。所以 gen-model 侧口径的
/// 已应用 sesno / 文件 sesno / 待更新批次 / 待重试单元不进这里，等 M4-4 定下
/// 两仓库的数据边界后再补。
#[derive(Debug, Clone, Default)]
pub struct WorkbenchVm {
    /// 项目名（标题栏芯片 + 状态栏）。
    pub project: String,
    /// 项目代号（标题栏 + 状态栏），等宽渲染。
    pub project_code: String,
    /// 当前用户（标题栏右侧）。
    pub user: String,
    /// 数据源是否就绪（状态栏指示点）。
    pub data_source_ok: bool,
    /// 数据树、属性与三维都越过同一个增量刷新屏障后递增。自动化用它确认自己
    /// 观察的不是数据批次结束前的旧一帧。
    pub refresh_generation: u64,
    /// 有一次取回工作正在跑。菜单据此置灰，免得连点堆出几轮全量重载。
    pub get_work_busy: bool,
    /// 有一趟重新生成正在跑。它与取回工作互相置灰——两者都会大动三维，
    /// 而重新生成中途还会删库里的产物，两条路交叉起来说不清谁踩了谁。
    /// eye 不受影响：显示 / 隐藏改的只是画面。
    pub regen_busy: bool,
    /// 本期执行范围内还没应用进模型的保存有多少；`None` = 还没取到过队列快照，
    /// 那一行整个不画。
    ///
    /// 摆在取回工作旁边，是为了把两个入口的分工说清楚：取回工作只取界面，
    /// 真要把这些保存应用进模型得走「模型更新」。它是提示不是判据——
    /// 数据来自 `/dbnums` 的上一拍轮询，只有那时那么新。界面上说「保存」不说
    /// 「会话」（ADR-0019）。
    pub pending_saves: Option<crate::task_queue::PendingSaves>,
    /// 已加载元素计数（状态栏右侧）。
    pub element_count: usize,
    /// 当前选择集（状态栏 + 属性视图跟随其 `primary`）。
    pub selection: Selection,
    /// 「把这个元素滚进模型树视野」的待办（M1-6 命令行定位、日志行回指）。
    ///
    /// 绘制层只读、消费不掉自己，由 App 在每帧 `show` 之后结算：目标那一行已经在
    /// `tree` 里就收掉，还不在就留到下一帧。**不是一次性的**——定位目标的祖先子层
    /// 是异步一层层回来的，只活一帧的话它会在行还不存在时被空手接走，树停在原地
    /// （ADR-0014）。
    pub tree_reveal: Option<RefU64>,
    /// 模型树（M1-2）。
    pub tree: TreeVm,
    /// 属性视图（M1-3）。
    pub props: PropsVm,
    /// 选中元素的房间归属（右键「查看所属房间」子菜单与「房间」页签共用），
    /// 随 `selection.primary` 与属性同拍预取。
    pub rooms: RoomVm,
    /// 「房间」页签聚焦房间的详情，`Cmd::FocusRoom` 后由宿主填。
    pub room_detail: RoomDetailVm,
    /// 命令交互视图。
    pub command: CommandVm,
    /// 标题栏搜索框的查询结果。
    pub search: SearchVm,
    /// 应用运行日志。
    pub logs: LogsVm,
    /// 三维视口的画面（M1-5 是占位纹理，M3 换成 Bevy 的渲染目标）。
    /// `None` = 还没有可画的东西。
    pub view3d: Option<View3dVm>,
    /// 正在进行的房间视图（隔离 + 取景）。`Some` 时视口 HUD 挂房间徽章与
    /// 「退出」入口；退出隔离或重连时由宿主清掉。
    pub room_view: Option<RoomViewVm>,
    /// eye 查询和增量 mesh 装载的短状态；完成/失败由宿主延时清除。
    pub model_load: Option<ModelLoadVm>,
    /// 任务队列在状态栏上的那一格。
    pub queue: QueueStatusVm,
    /// 当前项目接入点（状态栏那枚数据库芯片点开后的内容）。
    pub access_point: AccessPointVm,
}

/// 一个项目接入点在界面上的样子：这一刻**实际生效**的那组地址与身份。
///
/// 全部由宿主从运行配置解出来填进来，不是任何一份配置文件的字面值——两者本来就
/// 可能不一样，而分辨它们正是这块面板存在的理由。口令不进这里，界面任何一处都
/// 不显示它。
#[derive(Debug, Clone, Default)]
pub struct AccessPointVm {
    /// 模型本体库的实际连接串。
    pub db_url: String,
    pub namespace: String,
    pub database: String,
    /// 当前 MDB 名（带前导 `/`）。配空时为空串，不摆一个谁都不是的 `/`。
    pub mdb: String,
    pub user: String,
    pub model_api_url: String,
    pub data_api_url: String,
    /// 这组配置来自哪儿。`get_db_option()` 在没人注入配置时会**静默回落**去读工作
    /// 目录的 `DbOption.toml`——不把来源说出来，人就没法知道自己连到了哪儿。
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelLoadVm {
    Resolving(String),
    Loading {
        label: String,
        done: usize,
        total: usize,
    },
    Success(String),
    Failed(String),
}

impl ModelLoadVm {
    pub fn label(&self) -> &str {
        match self {
            Self::Resolving(label)
            | Self::Loading { label, .. }
            | Self::Success(label)
            | Self::Failed(label) => label,
        }
    }

    pub fn fraction(&self) -> Option<f32> {
        match self {
            Self::Resolving(_) => None,
            Self::Loading { done, total, .. } if *total > 0 => {
                Some((*done as f32 / *total as f32).clamp(0.0, 1.0))
            }
            Self::Loading { .. } => None,
            Self::Success(_) => Some(1.0),
            Self::Failed(_) => Some(1.0),
        }
    }
}

/// 状态栏上的队列计数。队列视图被折起时由它叫住人，**不重复面板上的明细**
/// ——同一组数字两处渲染就是两处维护。
#[derive(Debug, Clone, Copy, Default)]
pub struct QueueStatusVm {
    /// 还没干完的行数：运行中 + 排队中。
    pub active: usize,
    pub paused: bool,
    /// 快照里属于别的项目、因此没画上去的条目数。跨项目过滤不许无声——
    /// 不然人会对着一块空面板怀疑服务没连上。
    pub filtered_out: usize,
    /// 本项目的历史行里缺 dbnum、连行都拼不出来的条数。契约破损不许无声——
    /// 它与跨项目过滤是两回事，分开报。
    pub malformed: usize,
    /// 已达重试上限、自动路径永不再碰的交付单元数。**不并进 `active`**：
    /// 那一格是「还有活在干」，而死信恰恰是「不会再有人干它了」，非人工不动。
    pub dead_letters: usize,
    /// 读到过队列快照没有。没有的话这一格整个不画，不摆一个假的 0。
    pub known: bool,
}

/// 模型树的选择集。有序：批量动作的日志文案与将来的导出都要复现用户点选的先后。
///
/// 字段私有——`anchor`、`cursor` 与 `items` 的一致性只在这个文件里维护。三种落点
/// （普通点 / Ctrl 点 / Shift 点）由绘制层算出**结果**整体交出（`TreeCmd::SetSelection`），
/// App 侧不复原语义：区间要按可见行序算，那个序只有绘制层手上的 rows 才有。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    items: Vec<RefU64>,
    /// Shift 区间的支点。它不跟着最后一次点击走，而是停在最初那一下——
    /// 连着 Shift 点几下都该以最初那一下为准，Ctrl 点掉一个之后也不该漂。
    anchor: Option<RefU64>,
    /// 主选中：最后一次点中的那一个，属性视图与状态栏跟着它。
    ///
    /// 与 `anchor` 分成两个字段，是因为 Shift 区间之后两者必然分开：支点停在
    /// 起点，而用户刚点的是终点。合用一个字段的话，行表 `[1,2,3,4,5]` 上点行 4
    /// 再 Shift 点行 2，属性面板会去显示行 4。
    cursor: Option<RefU64>,
}

impl Selection {
    pub fn single(refno: RefU64) -> Self {
        Self {
            items: vec![refno],
            anchor: Some(refno),
            cursor: Some(refno),
        }
    }

    /// Ctrl 点：加进来或去掉。点掉的若正好是支点或主选中，两者各自顺延到剩下的末位。
    pub fn toggle(&mut self, refno: RefU64) {
        if let Some(i) = self.items.iter().position(|r| *r == refno) {
            self.items.remove(i);
            if self.anchor == Some(refno) {
                self.anchor = self.items.last().copied();
            }
            if self.cursor == Some(refno) {
                self.cursor = self.items.last().copied();
            }
        } else {
            self.items.push(refno);
            self.anchor = Some(refno);
            self.cursor = Some(refno);
        }
    }

    /// Shift 点：从锚到落点，按 `order` 给的**可见行序**取闭区间，整体替换。
    ///
    /// 替换而不是并入，是 Shift 的常规语义；要在已有选择上追加一段是 Ctrl+Shift，
    /// 那一档本轮不做。锚在区间选择后不动：连着 Shift 点几下都该以最初那一下为准。
    ///
    /// 锚或落点不在可见行里（被折叠进去了）就退化成单选——选中一段用户看不见的行
    /// 比少选更糟。
    pub fn range(&mut self, order: &[RefU64], to: RefU64) {
        let Some(anchor) = self.anchor else {
            *self = Self::single(to);
            return;
        };
        let (Some(a), Some(b)) = (
            order.iter().position(|r| *r == anchor),
            order.iter().position(|r| *r == to),
        ) else {
            *self = Self::single(to);
            return;
        };
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        self.items = order[lo..=hi].to_vec();
        self.cursor = Some(to);
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.anchor = None;
        self.cursor = None;
    }

    pub fn contains(&self, refno: RefU64) -> bool {
        self.items.contains(&refno)
    }

    /// 属性面板跟随的那一个 = 最后一次点中的元素。
    ///
    /// 多选时属性面板仍然显示它，面板头另给一枚「已选 N 项」说明属性只对应其中
    /// 一个。比多选时整块空掉有用，也比合并显示公共属性诚实——求交集是另一件事。
    pub fn primary(&self) -> Option<RefU64> {
        self.cursor.or_else(|| self.items.last().copied())
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = RefU64> + '_ {
        self.items.iter().copied()
    }

    pub fn to_vec(&self) -> Vec<RefU64> {
        self.items.clone()
    }
}

/// 三维视口对绘制层就是一张纹理。M1-5 里它是磁盘上的占位图，M3 接回 Bevy 后
/// 换成每帧更新的渲染目标，这一层不用改。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View3dVm {
    pub texture: egui::TextureId,
    /// 纹理的像素尺寸。视口要按它等比裁切铺满，不然图会被拉变形。
    pub size: egui::Vec2,
    /// true = Bevy 相机实时渲染目标；false = 独立壳的静态占位图。
    /// 工具栏只在 true 时出现——没有渲染器的壳里那排按钮按下去什么都不会发生。
    pub live: bool,
    /// 距离测量是否正在进行。真值只有宿主知道（测量状态机在 Bevy 侧），
    /// 绘制层拿它点亮工具栏上的尺子，不自己维护开关。
    pub measurement_active: bool,
    /// 相机的世界旋转，三列依次是 right / up / back（宿主世界系单位向量）。
    /// ViewCube 是一枚跟着相机转的正交小部件，它要的只有姿态，不要投影矩阵，
    /// 所以只发布这 9 个数。独立壳没有相机，恒等旋转就是「正对北面」的静态立方体。
    pub camera_rot: [[f32; 3]; 3],
    /// X / Y / Z 轴端标签在渲染纹理上的归一化 UV；轴尖出画或在相机身后时为 None。
    /// 投影只有宿主的相机做得了，绘制层拿到的已经是算完的位置。
    pub axis_labels: [Option<[f32; 2]>; 3],
    /// 地面网格一格代表多长，单位**毫米**（模型的真实尺度，不是渲染的世界单位）。
    /// 宿主的网格换档就按这个数落档，HUD 把它原样念出来——两边同源，读数才不会
    /// 与眼睛看见的格子对不上。独立壳没有网格，给 0 表示「无读数」。
    pub grid_cell_mm: f32,
}

/// 应用运行日志数据。
///
/// 旧壳的分级靠关键词猜（`PrintConsoleLine` 只带一个字符串），词表要照真实语料
/// 校准还是会错判。新链路里发日志的就是 App 自己，级别在发的那一刻就定死，
/// 不再有猜的环节。
#[derive(Debug, Clone, Default)]
pub struct LogsVm {
    /// 按时间先后排列的日志行（App 侧限长，最早的先丢）。
    pub lines: Vec<LogLineVm>,
    /// 分级计数。筛选芯片每帧都要显示它，App 侧增量维护，绘制层不逐帧扫全表。
    pub counts: LogCounts,
}

/// 命令行会话。输入框和上下键历史属于绘制状态，不进入 Vm。
#[derive(Debug, Clone, Default)]
pub struct CommandVm {
    pub lines: Vec<CommandLineVm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLineKind {
    Input,
    Output,
    Error,
}

/// 标题栏搜索框的查询**结果**。输入串与下拉高亮是绘制状态，不进 Vm。
///
/// `query` 与 `running` 分成两个字段，是因为结果总要晚一拍回来：那段时间里手上的
/// `hits` 仍是上一个查询串的结果，合用一个字段就分不出「这些命中是给谁的」，
/// 下拉会把旧结果挂在新输入下面。
#[derive(Debug, Clone, Default)]
pub struct SearchVm {
    /// `hits` 与 `sub_hits` 是**哪一个**输入串的结果（原样回显用户输入，
    /// 绘制层拿它对表）。
    pub query: String,
    /// 在途的那次查询；`None` = 没有。
    pub running: Option<SearchRunVm>,
    /// 名字**以输入开头**的命中。走库的名称索引，不限设计库，可能含树外元素。
    pub hits: Vec<SearchHitVm>,
    /// 名字**中间含输入**的命中。走本地 ngram 索引，范围是当前 MDB 的设计库。
    pub sub_hits: Vec<SearchHitVm>,
    /// 子串那一路此刻的状态。它决定下拉里该说哪句话，也决定该不该有子串这一节。
    pub sub_state: SubIndexVm,
    /// 任一路命中数触到上限，后面还有没显示出来的。
    pub truncated: bool,
    /// 前缀那一路失败的原因。子串那一路的失败在 `sub_state` 里——库断了子串
    /// 照样能搜，两件事不该合成一句话。
    pub error: Option<String>,
    /// 子串能搜的设计库个数（当前 MDB 声明的那些）。0 = 没有可搜的范围。
    pub scope_dbs: usize,
}

/// 子串索引这一刻的状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SubIndexVm {
    /// 索引就绪，子串那一节说话算数。
    Ready,
    /// 正在建（首次启动或数据变了）。这期间只有前缀那半。
    Building { done: usize, total: usize },
    /// 建不起来。带上原因，并告诉人 `reindex` 这个门。
    Failed(String),
    /// 这个构建不提供子串搜索（浏览器端），或者还没连上库。
    #[default]
    Off,
}

/// 在途的那次搜索。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchRunVm {
    pub query: String,
}

/// 搜索命中的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHitVm {
    pub refno: RefU64,
    /// PDMS 全名，带前导 `/`。
    pub name: String,
    pub noun: String,
    /// 命中落在当前 MDB 的设计库里。false = 树外元素：选得中、看得到属性，
    /// 但模型树上没有它那一行，下拉里要提前说清楚。
    pub in_tree: bool,
}

#[derive(Debug, Clone)]
pub struct CommandLineVm {
    pub kind: CommandLineKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LogCounts {
    pub info: usize,
    pub warn: usize,
    pub error: usize,
}

impl LogCounts {
    pub fn total(self) -> usize {
        self.info + self.warn + self.error
    }

    pub fn of(self, level: LogLevel) -> usize {
        match level {
            LogLevel::Info => self.info,
            LogLevel::Warn => self.warn,
            LogLevel::Error => self.error,
        }
    }
}

/// 日志的一行（C/LogRow：高 24、时间等宽 + 52pt 分级标签 + 正文）。
#[derive(Debug, Clone)]
pub struct LogLineVm {
    /// 稳定行号。处置入口拿它回指对应操作——缓冲限长会让 Vec 下标漂移。
    pub id: u64,
    /// 本地时钟 HH:MM:SS，等宽渲染。
    pub time: String,
    pub level: LogLevel,
    /// 这一行说的是哪个元素。Some 时正文前多一段可点的元素名，点它把模型树
    /// 和属性视图一起带过去（M1-6）。
    pub element: Option<LogElement>,
    /// 单行摘要，超出行宽截断。
    pub message: String,
    /// 完整信息（错误链等）；None = 摘要就是全部。
    pub detail: Option<String>,
    /// 该行对应的操作可以重来，行尾给「重试」。
    pub retryable: bool,
}

/// 日志行指向的元素。
///
/// 名字是**写这行日志那一刻**的名字，之后改名了也不回填——日志记的是当时发生了
/// 什么。回指用的是 refno，名字改了照样定位得到。
#[derive(Debug, Clone)]
pub struct LogElement {
    pub refno: RefU64,
    /// 形如 `EQUI /VESSEL-01`；缓存里查不到名字时退回 refno。
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const ALL: [LogLevel; 3] = [LogLevel::Info, LogLevel::Warn, LogLevel::Error];

    pub fn label(self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
        }
    }

    /// 分级色沿用状态色，不另起一套。
    pub fn status(self) -> Status {
        match self {
            LogLevel::Info => Status::Info,
            LogLevel::Warn => Status::Warn,
            LogLevel::Error => Status::Error,
        }
    }
}

/// 模型树数据状态。
///
/// 连接与查询都是异步的，加载中 / 失败是客观存在的画面而非「四态规范」的
/// 提前设计；统一的加载 / 空 / 错误 / 未初始化四态视觉在 M1-7 收口。
#[derive(Debug, Clone, Default)]
pub enum TreeVm {
    /// 数据源连接或根层查询在途。
    #[default]
    Loading,
    /// 可见行（App 侧按展开状态展平；空 Vec = 当前库下没有 SITE）。
    Ready(Vec<TreeRowVm>),
    /// 连接或查询失败，附原因。
    Failed(String),
}

/// 一行在三维里的可见性。说的是**三维此刻真的画成什么样**，不是下过什么指令
/// （ADR-0016）。
///
/// 四态而不是 `bool`：三维里没有它的实体、全显示、全隐藏、以及只显示出一部分，
/// 是四件不同的事。二态在这里必然说谎，理由见 ADR-0010 与 ADR-0016。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowVisibility {
    /// 三维里压根没有它的实体：没显示过，或者显示了但一个网格都没画出来。
    #[default]
    Unloaded,
    Shown,
    Hidden,
    /// 半可见：后代有的显示有的隐藏，或者可见的模型只画出了一部分网格。
    Partial,
}

impl RowVisibility {
    /// 点这一行的眼睛之后，`SetVisible` 该带什么方向。
    /// 未加载、已隐藏与半可见都是「让它出来」，只有全显示才是「藏起来」。
    pub fn on_click(self) -> bool {
        self != Self::Shown
    }
}

/// 模型树的一个可见行。
///
/// 树的展平由 App 侧在结构变化（展开 / 折叠 / 子层到达）时做一次，
/// 绘制层逐帧只读——万行级列表每帧重建才是卡顿的来源。
#[derive(Debug, Clone)]
pub struct TreeRowVm {
    pub refno: RefU64,
    /// 层级深度（SITE 为 0）。
    pub depth: u16,
    /// 显示名（无名节点由数据侧按 noun 补号）。
    pub name: String,
    /// PDMS 类型（SITE / ZONE / PIPE / BRAN…），行尾 meta 显示。
    pub noun: String,
    /// None = 叶子；Some(open) = 可展开及其展开态。
    pub expandable: Option<bool>,
    /// 子层查询在途（行尾以「加载中…」提示）。
    pub loading: bool,
    /// 这一行在三维里的可见性。真值是三维回执上来的实际渲染结果，绘制层只读。
    ///
    /// 容器行（SITE / ZONE / PIPE）自己没有 mesh 实体，它这一格是把它那次显示
    /// 解析出来的那些模型聚合出来的：有显示有隐藏就是半可见。
    pub visibility: RowVisibility,
    /// 点这一行眼睛时 `SetVisible` 该带的方向。
    ///
    /// 不能由 `visibility` 现推：eye 现在跟的是实际渲染结果，而指令下出去到
    /// 画面变过来之间隔着查询与网格装载。那段时间里图标停在原样，再点一下要的是
    /// **反转上一次指令**，不是反转图标。
    pub next_visible: bool,
}

/// 属性视图数据状态（跟随 `WorkbenchVm::selected`）。
///
/// 没有「空」态：属性表至少带 TYPE 与 NAME，选中了元素就不会一条都没有。
#[derive(Debug, Clone, Default)]
pub enum PropsVm {
    /// 尚未选中任何元素——是「还没轮到它」而不是「查完了没有」。
    #[default]
    Uninit,
    /// 选中元素的属性查询在途。切换元素时保留上一份完整属性数据供绘制，首次查询为 None。
    Loading(Option<PropsDataVm>),
    /// 属性到位，按设计稿分组展示。
    Ready(PropsDataVm),
    /// 查询失败，附原因。
    Failed(String),
}

impl PropsVm {
    /// 开始查询下一份属性。已有数据继续显示到新结果完整到位，避免切换元素时
    /// 先把整张属性表换成加载提示、下一帧又换回来造成闪烁。
    pub fn begin_query(&mut self) {
        let previous = match std::mem::take(self) {
            Self::Ready(data) | Self::Loading(Some(data)) => Some(data),
            _ => None,
        };
        *self = Self::Loading(previous);
    }

    /// Apply an accepted scalar edit to the displayed snapshot.
    pub fn edit(&mut self, refno: RefU64, attr: &str, value: String) {
        let Self::Ready(data) = self else {
            return;
        };
        if data.refno != refno {
            return;
        }
        for row in data
            .common
            .iter_mut()
            .chain(&mut data.attrs)
            .chain(&mut data.udas)
        {
            if row.attr == attr {
                row.value = value.clone();
                row.muted = false;
                break;
            }
        }
        if attr == "NAME" {
            data.name = value;
        }
    }
}

/// 一个元素的属性面板内容。
///
/// 分组口径：通用属性 = 类型 / 名称 / OWNER / REFNO 四行定序；
/// UDA 属性 = 以 ':' 开头的用户自定义属性；其余全部进元件属性（字母序）。
/// 设计稿里的会话号 / 修改人 / 修改时间是版本数据，M4 接 his_pe 后再补——
/// 外壳原则：宁可少一行，不摆假数据。
#[derive(Debug, Clone, Default)]
pub struct PropsDataVm {
    pub refno: RefU64,
    /// 面板头：元素显示名 + noun。
    pub name: String,
    pub noun: String,
    pub common: Vec<PropRowVm>,
    pub attrs: Vec<PropRowVm>,
    pub udas: Vec<PropRowVm>,
}

/// 属性面板的一行（C/PropRow：键 88pt 次级色 + 值等宽填充）。
#[derive(Debug, Clone)]
pub struct PropRowVm {
    /// 显示用键名，通用组会译成中文（TYPE -> 类型）。
    pub key: String,
    /// 真实属性名，`Cmd::EditAttr` 用它回指，不能拿 `key` 顶替。
    pub attr: String,
    pub value: String,
    /// 值的可编辑形态，决定给什么控件。
    pub kind: PropKind,
    /// unset / 空值以弱化色渲染。
    pub muted: bool,
}

/// 值的可编辑形态（对应数据层的 `AttrKind`）。
///
/// 旧壳的属性行由 `reflect_att_map` 反射驱动，值是可编辑的；新链路没有 Bevy 反射，
/// 由数据层把 `NamedAttrValue` 的变体降成这个枚举，绘制层据此选控件。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PropKind {
    /// 未设值，或引用 / 方位 / 数组这类要选择器才能改的值：只读。
    #[default]
    ReadOnly,
    Int,
    Real,
    Bool,
    Text,
}

impl PropKind {
    pub fn editable(self) -> bool {
        self != PropKind::ReadOnly
    }
}

/// 房间归属数据状态（跟随 `WorkbenchVm::selection` 的 primary），四态语义与
/// [`PropsVm`] 相同。有「空」态语义：`Ready` 且 `relations` 为空 = 查过了、
/// 确实不属于任何房间。
#[derive(Debug, Clone, Default, PartialEq)]
pub enum RoomVm {
    /// 尚未选中任何元素。
    #[default]
    Uninit,
    /// 查询在途。切换元素时保留上一份数据供绘制，避免闪烁；首次查询为 None。
    Loading(Option<RoomsDataVm>),
    Ready(RoomsDataVm),
    Failed(String),
}

impl RoomVm {
    /// 开始查询下一份归属，防闪烁语义同 [`PropsVm::begin_query`]。
    pub fn begin_query(&mut self) {
        let previous = match std::mem::take(self) {
            Self::Ready(data) | Self::Loading(Some(data)) => Some(data),
            _ => None,
        };
        *self = Self::Loading(previous);
    }
}

/// 一次房间视图（隔离 + 取景）的身份牌：HUD 徽章与退出入口的数据源。
#[derive(Debug, Clone, PartialEq)]
pub struct RoomViewVm {
    /// 房间 FRMW。
    pub room: RefU64,
    /// 房号（如 R301），HUD 徽章文案用。
    pub room_num: String,
    /// 去重后的成员总数。
    pub member_count: usize,
}

/// 一个元素的房间归属数据。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoomsDataVm {
    pub refno: RefU64,
    /// 已按归属强度排好序（inside_count 降序 -> center_dist 升序 -> 房号升序，
    /// ADR-010 §5），首条即「主归属」。
    pub relations: Vec<RoomRelationVm>,
}

/// 一条房间归属（数据层 `room::RoomRelation` 的绘制层镜像，字段语义见彼处）。
#[derive(Debug, Clone, PartialEq)]
pub struct RoomRelationVm {
    /// 房间 FRMW；面板不在册（陈旧边）时为 None。
    pub room: Option<RefU64>,
    pub room_num: String,
    /// 如 `1RX-R301`；算不出时不显示。
    pub room_code: Option<String>,
    /// 归属经由的面板。
    pub panel: RefU64,
    /// 0-8，越大归属越强。
    pub inside_count: u8,
    pub center_dist: f32,
    /// 归属来自直接子层成员而非元素自身（容器元素的聚合口径）。
    pub via_member: bool,
}

/// 「房间」页签聚焦房间的详情状态。四态语义同 [`PropsVm`]，但多一层含义：
/// `Uninit` 不是错，是「还没点过任何房间」——页签此时只画归属列表与提示。
/// 聚焦目标由宿主持有（`Cmd::FocusRoom` 设置），选中元素一换就归零。
#[derive(Debug, Clone, Default, PartialEq)]
pub enum RoomDetailVm {
    /// 还没聚焦任何房间。
    #[default]
    Uninit,
    /// 查询在途。切换房间时保留上一份撑住布局，避免闪烁。
    Loading(Option<RoomDetailDataVm>),
    Ready(RoomDetailDataVm),
    Failed(String),
}

impl RoomDetailVm {
    /// 开始查询下一间房，防闪烁语义同 [`PropsVm::begin_query`]。
    pub fn begin_query(&mut self) {
        let previous = match std::mem::take(self) {
            Self::Ready(data) | Self::Loading(Some(data)) => Some(data),
            _ => None,
        };
        *self = Self::Loading(previous);
    }
}

/// 一间房的详情（数据层 `room::RoomDetail` 的绘制层镜像）。
#[derive(Debug, Clone, PartialEq)]
pub struct RoomDetailDataVm {
    pub room: RefU64,
    /// 房间 FRMW 全名（如 `/1RX-RM03-R301`）。
    pub name: String,
    pub room_num: String,
    pub room_code: Option<String>,
    /// 在册面板。「缩放到房间」的取景目标集要面板参与——房间的空间范围
    /// 由墙面板围出来，光取成员会把相机贴到设备堆上。
    pub panels: Vec<RefU64>,
    /// 去重后的成员总数。
    pub member_count: usize,
    /// 全量成员 refno，按归属强度排序（隔离 / 取景用，预览截断后凑不齐）。
    pub member_refnos: Vec<RefU64>,
    /// 成员预览（前若干个）。
    pub members: Vec<RoomMemberVm>,
    /// 这间房是否在待重算队列里（S12 房间泳道同源）。
    pub pending_recalc: bool,
}

/// 房间成员预览行。
#[derive(Debug, Clone, PartialEq)]
pub struct RoomMemberVm {
    pub refno: RefU64,
    pub name: String,
    pub noun: String,
    /// 0-8，对这间房的归属强度。
    pub inside_count: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(n: u64) -> RefU64 {
        RefU64(n)
    }

    #[test]
    fn ctrl_click_toggles_and_keeps_order() {
        let mut s = Selection::single(r(1));
        s.toggle(r(3));
        s.toggle(r(2));
        assert_eq!(s.to_vec(), vec![r(1), r(3), r(2)]);
        s.toggle(r(3));
        assert_eq!(s.to_vec(), vec![r(1), r(2)]);
    }

    #[test]
    fn dropping_the_anchor_moves_it_to_the_last_survivor() {
        let mut s = Selection::single(r(1));
        s.toggle(r(2));
        assert_eq!(s.primary(), Some(r(2)));
        s.toggle(r(2));
        assert_eq!(s.primary(), Some(r(1)));
    }

    #[test]
    fn shift_range_follows_visible_row_order_in_both_directions() {
        let order = [r(1), r(2), r(3), r(4), r(5)];
        let mut s = Selection::single(r(4));
        s.range(&order, r(2));
        assert_eq!(s.to_vec(), vec![r(2), r(3), r(4)]);
        // 锚仍是 4，再 Shift 点一次以它为准而不是以上一段的端点为准。
        s.range(&order, r(5));
        assert_eq!(s.to_vec(), vec![r(4), r(5)]);
    }

    /// 支点停在起点、主选中跟到终点：属性视图与三维高亮跟的是用户刚点的那一行。
    #[test]
    fn shift_range_moves_the_primary_to_the_clicked_row() {
        let order = [r(1), r(2), r(3), r(4), r(5)];
        let mut s = Selection::single(r(4));
        s.range(&order, r(2));
        assert_eq!(s.primary(), Some(r(2)));
        // 支点没动，所以下一次 Shift 仍以行 4 为准；主选中跟到行 5。
        s.range(&order, r(5));
        assert_eq!(s.to_vec(), vec![r(4), r(5)]);
        assert_eq!(s.primary(), Some(r(5)));
    }

    #[test]
    fn props_query_keeps_the_previous_data_until_replacement_arrives() {
        let mut props = PropsVm::Ready(PropsDataVm {
            refno: r(1),
            name: "previous".into(),
            ..Default::default()
        });

        props.begin_query();

        assert!(matches!(
            props,
            PropsVm::Loading(Some(PropsDataVm { ref name, .. })) if name == "previous"
        ));

        props.begin_query();
        assert!(matches!(
            props,
            PropsVm::Loading(Some(PropsDataVm { ref name, .. })) if name == "previous"
        ));

        let mut first_query = PropsVm::Uninit;
        first_query.begin_query();
        assert!(matches!(first_query, PropsVm::Loading(None)));
    }

    /// 未加载与已隐藏都是「让它出来」。把这两档合并成 `!visible` 是最容易写错的
    /// 一处：开机时整棵树都是未加载，那一版点下去会把元素**隐藏**掉。
    /// 半可见也归「让它出来」——点一个显示了一半的 ZONE，要的是显示全部。
    #[test]
    fn clicking_the_eye_shows_everything_that_is_not_already_shown() {
        assert!(RowVisibility::Unloaded.on_click());
        assert!(RowVisibility::Hidden.on_click());
        assert!(RowVisibility::Partial.on_click());
        assert!(!RowVisibility::Shown.on_click());
    }

    #[test]
    fn model_load_progress_uses_real_completed_counts() {
        let progress = ModelLoadVm::Loading {
            label: "加载模型网格".into(),
            done: 3,
            total: 4,
        };
        assert_eq!(progress.fraction(), Some(0.75));
        assert_eq!(
            ModelLoadVm::Resolving("解析模型范围".into()).fraction(),
            None
        );
    }

    #[test]
    fn shift_onto_a_collapsed_row_degrades_to_single() {
        let order = [r(1), r(2)];
        let mut s = Selection::single(r(1));
        s.range(&order, r(9));
        assert_eq!(s.to_vec(), vec![r(9)]);
    }
}
