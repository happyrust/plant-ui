//! plant-ui：绘制层（组件层 + 各屏纯绘制函数 + Vm 定义）。
//! 不认识数据库、不认识 Bevy：输入 &Vm，输出 Vec<Cmd>。

pub mod fonts;
pub mod model_update;
pub mod project_picker;
pub mod settings;
pub mod style;
pub mod vm;
pub mod workbench;

pub use aios_core::RefU64;

/// UI 发出的命令：独立应用直接执行，接进 Bevy 后转成 Event。
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    SelectElement(aios_core::RefU64),
    /// 定位到元素：选中它、把树上挡着的祖先一路展开、再把行滚进视野。
    /// 与 `SelectElement` 分开，是因为树上点行本来就看得见，不该跟着跳。
    LocateElement(aios_core::RefU64),
    /// 展开 / 折叠模型树节点；子层未加载时由 App 侧懒加载。
    ToggleExpand(aios_core::RefU64),
    /// 属性面板提交了一次改值（回车或失焦）。`attr` 是真实属性名，
    /// `value` 是已按类型校验过的显示串。
    EditAttr {
        refno: aios_core::RefU64,
        attr: String,
        value: String,
    },
    /// 命令行错误行的处置入口：重做该行对应的操作（App 侧按行号找回原请求）。
    RetryLog(u64),
    /// 面板错误态上的「重试」。命令行按行号回指得到操作，面板上没有行号，
    /// 所以直接说重做哪件事——一块面板的失败只有一种来源，说得出来。
    Reconnect,
    RetryProps(aios_core::RefU64),
    /// 清空命令行缓冲。
    ClearConsole,
    /// 打开项目选择窗口。
    OpenProjectPicker,
    /// 使用宿主提供的配置加载一个项目。
    LoadProject(String),
    /// 打开设置任务窗。
    OpenSettings,
    /// 打开并刷新模型增量更新预览。
    OpenModelUpdate,
    /// 重新读取当前项目的增量范围。
    RefreshModelUpdate,
    /// 执行当前项目的全部待更新库；范围由服务端的 Committed Watermark 决定。
    ExecuteModelUpdate,
}
