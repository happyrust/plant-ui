use aios_core::RefU64;

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
    /// 当前选中元素（状态栏；M1-6 起驱动属性视图跟随）。
    pub selected: Option<RefU64>,
    /// 模型树（M1-2）。
    pub tree: TreeVm,
    /// 属性视图（M1-3）。
    pub props: PropsVm,
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
#[derive(Debug, Clone, Default)]
pub enum PropsVm {
    /// 尚未选中任何元素。
    #[default]
    Empty,
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
