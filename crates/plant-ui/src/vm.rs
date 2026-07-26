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
