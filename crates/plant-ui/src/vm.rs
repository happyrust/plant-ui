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
    /// 有一次取回工作正在跑。菜单据此置灰，免得连点堆出几轮全量重载。
    pub get_work_busy: bool,
    /// 设计库里还没被应用到模型的会话数；`None` = 还没读到，那一行整个不画。
    ///
    /// 摆在取回工作旁边，是为了把两个入口的分工说清楚：取回工作只取界面，
    /// 真要把这些会话应用进模型得走「模型更新」。它是提示不是判据——
    /// 数据侧的 `file_latest_sesno` 只有上一次扫描时那么新。
    pub pending_sessions: Option<u32>,
    /// 已加载元素计数（状态栏右侧）。
    pub element_count: usize,
    /// 当前选择集（状态栏 + 属性视图跟随其 `primary`）。
    pub selection: Selection,
    /// 一次性的「把这个元素滚进模型树视野」请求（M1-6：命令行定位过来时用）。
    /// 绘制层只读，消费不掉自己，所以由 App 在每帧 `show` 之后清掉。
    pub tree_reveal: Option<RefU64>,
    /// 模型树（M1-2）。
    pub tree: TreeVm,
    /// 属性视图（M1-3）。
    pub props: PropsVm,
    /// 命令交互视图。
    pub command: CommandVm,
    /// 应用运行日志。
    pub logs: LogsVm,
    /// 三维视口的画面（M1-5 是占位纹理，M3 换成 Bevy 的渲染目标）。
    /// `None` = 还没有可画的东西。
    pub view3d: Option<View3dVm>,
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

/// 一行在三维里的可见性。
///
/// 三态而不是 `bool`：宿主开机时可见性台账是空的，每一行都既不是显示也不是隐藏，
/// 而是从没被显示指令提到过。二态在这里必然说谎，理由与台账本身见 ADR-0010。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RowVisibility {
    /// 从没被显示指令提到过——三维里压根没有它的实体。
    #[default]
    Unloaded,
    Shown,
    Hidden,
}

impl RowVisibility {
    /// 点这一行的眼睛之后，`SetVisible` 该带什么方向。
    /// 未加载与已隐藏都是「让它出来」，只有显示中才是「藏起来」。
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
    /// 这一行在三维里的可见性。真值在宿主的可见性台账上，绘制层只读。
    ///
    /// 容器行（SITE / ZONE / PIPE）自己没有 mesh 实体，它这一格说的是
    /// **对这一行下过的指令**，不是子树此刻的实际状态。
    pub visibility: RowVisibility,
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

impl PropsVm {
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

    /// 未加载与已隐藏都是「让它出来」。把这两档合并成 `!visible` 是最容易写错的
    /// 一处：开机时整棵树都是未加载，那一版点下去会把元素**隐藏**掉。
    #[test]
    fn clicking_the_eye_shows_everything_that_is_not_already_shown() {
        assert!(RowVisibility::Unloaded.on_click());
        assert!(RowVisibility::Hidden.on_click());
        assert!(!RowVisibility::Shown.on_click());
    }

    #[test]
    fn shift_onto_a_collapsed_row_degrades_to_single() {
        let order = [r(1), r(2)];
        let mut s = Selection::single(r(1));
        s.range(&order, r(9));
        assert_eq!(s.to_vec(), vec![r(9)]);
    }
}
