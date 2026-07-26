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
    /// 库标识（如 "ns 1516 / db 7997"），等宽渲染。
    pub db: String,
    /// 当前用户（标题栏右侧）。
    pub user: String,
    /// 数据源是否就绪（状态栏指示点）。
    pub data_source_ok: bool,
    /// 已加载元素计数（状态栏右侧）。
    pub element_count: usize,
    /// 当前选中元素（状态栏 + 属性视图跟随）。
    pub selected: Option<RefU64>,
    /// 一次性的「把这个元素滚进模型树视野」请求（M1-6：命令行定位过来时用）。
    /// 绘制层只读，消费不掉自己，所以由 App 在每帧 `show` 之后清掉。
    pub tree_reveal: Option<RefU64>,
    /// 模型树（M1-2）。
    pub tree: TreeVm,
    /// 属性视图（M1-3）。
    pub props: PropsVm,
    /// 命令行视图（M1-4）。
    pub console: ConsoleVm,
    /// 三维视口的画面（M1-5 是占位纹理，M3 换成 Bevy 的渲染目标）。
    /// `None` = 还没有可画的东西。
    pub view3d: Option<View3dVm>,
}

/// 三维视口对绘制层就是一张纹理。M1-5 里它是磁盘上的占位图，M3 接回 Bevy 后
/// 换成每帧更新的渲染目标，这一层不用改。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View3dVm {
    pub texture: egui::TextureId,
    /// 纹理的像素尺寸。视口要按它等比裁切铺满，不然图会被拉变形。
    pub size: egui::Vec2,
}

/// 命令行视图数据。
///
/// 旧壳的分级靠关键词猜（`PrintConsoleLine` 只带一个字符串），词表要照真实语料
/// 校准还是会错判。新链路里发日志的就是 App 自己，级别在发的那一刻就定死，
/// 不再有猜的环节。
#[derive(Debug, Clone, Default)]
pub struct ConsoleVm {
    /// 按时间先后排列的日志行（App 侧限长，最早的先丢）。
    pub lines: Vec<LogLineVm>,
    /// 分级计数。筛选芯片每帧都要显示它，App 侧增量维护，绘制层不逐帧扫全表。
    pub counts: LogCounts,
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

/// 命令行的一行（C/LogRow：高 24、时间等宽 + 52pt 分级标签 + 正文）。
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
}

/// 属性视图数据状态（跟随 `WorkbenchVm::selected`）。
///
/// 没有「空」态：属性表至少带 TYPE 与 NAME，选中了元素就不会一条都没有。
#[derive(Debug, Clone, Default)]
pub enum PropsVm {
    /// 尚未选中任何元素——是「还没轮到它」而不是「查完了没有」。
    #[default]
    Uninit,
    /// 选中元素的属性查询在途。
    Loading,
    /// 属性到位，按设计稿分组展示。
    Ready(PropsDataVm),
    /// 查询失败，附原因。
    Failed(String),
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
