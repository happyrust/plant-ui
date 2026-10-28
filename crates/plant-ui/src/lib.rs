//! plant-ui：绘制层（组件层 + 各屏纯绘制函数 + Vm 定义）。
//! 不认识数据库、不认识 Bevy：输入 &Vm，输出 Vec<Cmd>。

pub mod data_publish;
pub mod fonts;
pub mod increment_records;
pub mod manual_data_publish;
pub mod model_mutation;
pub mod model_update;
pub mod project_picker;
pub mod room_browser;
pub mod room_tree;
pub mod settings;
pub mod style;
pub mod task_queue;
pub mod vm;
pub mod workbench;
pub mod virtual_hole;

pub use aios_core::RefU64;

/// 对模型本身的动作。模型树右键菜单与三维视口工具栏是同一件事的两个入口，
/// 共用这一个枚举，宿主只需映射一次。
///
/// 动作自带 `refno`：右键菜单打开后选中还可能被别处改掉，回头读 `vm.selection`
/// 就会作用到另一个元素上。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelAction {
    /// 可见性作用于一整批：「隐藏这几个」是选中之后最常做的一件事，
    /// 拆成 N 条命令会在宿主侧变成 N 次深度展开与 N 次事件写入。
    SetVisible {
        refnos: Vec<aios_core::RefU64>,
        visible: bool,
    },
    /// 只显隐一根 BRAN 名下点名的**几条直段**（房间树直段行的眼睛，ADR-0033 阶段四）。直段没有自己的
    /// refno——它以所属 BRAN 的 refno 进场景，`SetVisible` 按 refno 一动就是整根连同全部管身；这条按
    /// [`TubeSegment`] 点名其中几条。之后对这根 BRAN 的 `SetVisible` / `HideAll` / `ShowAll` / 隔离照旧
    /// 整根生效，把逐段的结果一并盖掉；没带直段身份进场景的记录（库供数 / 老服务端）这条点不到。
    SetTubeSegmentsVisible {
        bran: aios_core::RefU64,
        segments: Vec<TubeSegment>,
        visible: bool,
    },
    /// 从三维场景中卸载这些模型及其网格。
    ///
    /// 与 `SetVisible(false)` 不同，卸载会同时释放场景实体与渲染状态；用于 GET WORK
    /// 已确认元素从资料树消失、而整场模型重查仍在后台进行的窗口期。
    Unload { refnos: Vec<aios_core::RefU64> },
    /// 原子替换当前 X-Ray 目标集。空集恢复全部模型的常态 / 选中材质。
    /// 只改变材质，不改变可见性、隔离快照或相机。
    SetXRay { refnos: Vec<aios_core::RefU64> },
    /// 隐藏当前已显示的全部模型。全局动作，只在工具栏出现。
    HideAll,
    /// 把已加载的模型全部显示回来，`HideAll` 的对偶。
    ///
    /// 「全部」只到**已加载**为止：没加载过的元素在三维里根本没有实体，这条命令
    /// 不会去替用户加载它们，模型树上那些行也照旧停在「未加载」。加载是 `SetVisible`
    /// 那条路上的事，两者别混。
    ///
    /// 它会把用户在 `HideAll` **之前**亲手隐藏的那几个也一并显示出来——`HideAll`
    /// 已经把「谁是我自己藏的」这件事抹平了，恢复时分不出来，也不该假装分得出来。
    ShowAll,
    /// 相机对准某个元素——**只作用于主选中，不收整批**。
    ///
    /// 给一组元素取景要合并它们的包围盒，而宿主侧的 `FocusModelEvent` 只认单个
    /// refno。塞一组进去的结果是定位到其中某一个、还说不清是哪一个。定位本来
    /// 就是「带我到它跟前」，作用于主选中是说得清的语义。
    Focus(aios_core::RefU64),
    /// 相机对准一个由数据面返回的树范围包围盒。该动作只改变相机，
    /// 不加载模型、不改变可见性，也不写入模型 bounds 索引。
    FocusBounds { min_mm: [f32; 3], max_mm: [f32; 3] },
    /// 相机覆盖一组元素的合并包围盒（房间取景用）。与 `Focus` 分开：房间 FRMW
    /// 自身没有几何实体，能取景的只有它的面板与成员；合并**已加载**那部分的
    /// 包围盒，没加载的不参与，一个都没加载时不动相机。
    FocusGroup { refnos: Vec<aios_core::RefU64> },
    /// 隔离显示：目标可见、其余全部隐藏。首次隔离时视口记下隔离前的可见性
    /// 快照，供 [`Self::ExitIsolate`] 恢复；连续隔离（房间 A -> 房间 B）不覆盖
    /// 快照——退出回到隔离前的世界，而不是上一间房。
    ///
    /// `matching` 决定一个模型怎样才算「在目标里」，见 [`IsolateMatching`]。
    Isolate {
        refnos: Vec<aios_core::RefU64>,
        matching: IsolateMatching,
    },
    /// 退出隔离，恢复隔离前的可见性快照；没有快照时是无操作。
    ExitIsolate,
    /// 相机拉远到覆盖当前所有可见模型。
    FitAll,
    /// 进入 / 退出距离测量。
    ToggleMeasure,
}

/// 隔离时一个模型怎样才算「在目标里」（[`ModelAction::Isolate`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolateMatching {
    /// 模型自己的 refno 或它的 owner 命中目标都算。目标是容器时，底下的构件跟着一起留下。
    /// 「房间」页签的「隔离成员」与截断关着的房间视图用它。
    RefnoOrOwner,
    /// 只认模型自己的 refno。截断开着的房间视图用它（2026-09-29 直段截断计划 §4.4）：直段以所属
    /// 分支的 refno 渲染，分支进了目标只说明它的直段是截断候选。按 owner 展开的话，整根分支连同墙外
    /// 的阀门都会亮起来。
    Exact,
}

/// 一条直段（隐式管身）在所属 BRAN 内的身份：两端构件（`a_b`）+ 同两端之间的序号。与 gen-model
/// `/model/records` 每条记录的 `tube{ordinal,from,to}` 和 `/spatial/rooms/{room}/tree` 的 `tubes[]`
/// 同一套编码，房间树的直段行靠它点名场景里的那一根（ADR-0033 阶段四）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TubeSegment {
    pub ordinal: u32,
    pub from: String,
    pub to: String,
}

/// 进三维场景的一条模型记录：[`aios_core::GeomInstQuery`] 原样（`serde(flatten)`，与 gen-model 读面的
/// `ModelRecord` 同形）+ 直段记录才带的身份。库供数与老服务端没有 `tube`，那样的记录在场景里只按
/// refno 认，与从前一样；`Deref` 到几何记录本身，读字段的代码不必改。
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ModelRecord {
    #[serde(flatten)]
    pub model: aios_core::GeomInstQuery,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tube: Option<TubeSegment>,
}

impl From<aios_core::GeomInstQuery> for ModelRecord {
    fn from(model: aios_core::GeomInstQuery) -> Self {
        Self { model, tube: None }
    }
}

impl std::ops::Deref for ModelRecord {
    type Target = aios_core::GeomInstQuery;

    fn deref(&self) -> &Self::Target {
        &self.model
    }
}

/// 三维视口里的一次相机运动。键位见 `view3d::camera`：左键 / 中键平移、
/// 右键旋转、滚轮缩放；未形成拖拽的左键点击仍用于拾取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraMotion {
    Orbit,
    Pan,
}

/// 视口上的相机手势。
///
/// 视口画的是一张离屏渲染纹理，宿主的相机要的却是**渲染目标像素空间**的量；
/// 两者之间隔着一次等比裁切铺满（`view3d::cover_uv`），换算只有绘制层做得了，
/// 所以这里交出去的已经是折算好的值，宿主拿来直接喂给相机。
///
/// `Drag` 与 `Zoom` 在宿主侧都是「没有运动就不生效」，因此起手被挡下的手势
/// （比如从工具栏上按下去再拖）不会漏出半截运动，调用点不必自己记状态。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraGesture {
    /// 按下：开始一次旋转或平移。`anchor` 是按下点在渲染纹理上的归一化 UV，
    /// 宿主拿它求转心——转心落在指针底下那个东西上，是这类相机的立身之本。
    Begin {
        motion: CameraMotion,
        anchor: [f32; 2],
    },
    /// 拖拽增量，单位是渲染目标的像素。
    Drag([f32; 2]),
    /// 滚轮缩放。`amount` 是 egui 的滚动点数，宿主折算成自己的量纲；
    /// `anchor` 同 `Begin`，缩放要朝着指针指的方向去。
    Zoom { amount: f32, anchor: [f32; 2] },
    /// 松手。之后的惯性归宿主的控制器管，绘制层不模拟。
    End,
}

/// 相机的一次完整位姿：位置、姿态与轨道转心，都是**宿主世界系**的量。
///
/// 导航历史记的就是它——「回到上一次操作的地方」里的「地方」有一半是相机。
/// 三个量缺一不可：只记姿态回不到原位，只记位置转起来会绕错心。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub position: [f32; 3],
    /// 四元数 `(x, y, z, w)`。
    pub rotation: [f32; 4],
    /// 轨道相机的转心。回放时一起还回去，不然回到原位之后第一下旋转就会绕着别处转。
    pub focus: [f32; 3],
}

/// 导航历史上的一步（命令栏最左那两枚箭头，或右键列表里点的一条）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavStep {
    Back,
    Forward,
    /// 右键列表里直接点某一条：历史栈上的下标（与 `vm.nav.entries` 同序）。
    Jump(usize),
}

/// 三维视口的两套外观（E3D 外观移植计划）。
///
/// `Pbr` 是今天的 Bevy PBR 视口；`E3d` 复刻 AVEVA E3D 3.1 的 SGL 显示效果（前向头灯着色 +
/// 环境立方体反射 + 边线 + 伪阴影 + 背景渐变）。它是**视口设置**：随 `settings.ron` 按机器落盘，
/// 不进项目 bundle，不改模型数据、可见性台账与拾取。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LookMode {
    Pbr,
    E3d,
}

/// E3D 外观的预设：一套光照常量 + 四个子开关的出厂位，对应两台不同口径的 E3D 3.1
///（plant3d-web `sglLookPresets.ts`，落盘字面与那边的 id 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookPreset {
    /// 出厂 E3D 3.1：PML `gphviewopt.default()` 走完的视图——边线 / 伪阴影 / 渐变 / 4× 抗锯齿全开，
    /// 光照 {Ka 0.7, Kd 0, Ks 0, Kr 0.8, Kse 0}（无漫反射 / 高光，体积感靠环境反射），背景 grey→白。
    E3d31Factory,
    /// 本机那台经修补脚本启动的 E3D 3.1（没跑 PML 视图默认）：全关，光照落在 SGL 的 C++ 默认
    /// {0.5, 0.8, 0.3, 0.35, 64}，背景纯灰 #828282。
    SglMachine,
}

impl LookPreset {
    /// 设置窗里的两项，出厂默认在前。
    pub const ALL: [Self; 2] = [Self::E3d31Factory, Self::SglMachine];

    pub const fn label(self) -> &'static str {
        match self {
            Self::E3d31Factory => "出厂 E3D 3.1",
            Self::SglMachine => "本机真机",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::E3d31Factory => {
                "PML 出厂视图：边线 / 伪阴影 / 渐变 / 抗锯齿全开，0.7·颜色 + 0.8·环境反射，背景 grey→白"
            }
            Self::SglMachine => {
                "未跑 PML 视图默认的真机：边线 / 伪阴影 / 渐变 / 抗锯齿全关，0.5/0.8 Blinn-Phong + 0.35 反射，背景纯灰"
            }
        }
    }
}

/// E3D 外观下元素怎么着色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColourScheme {
    /// E3D 出厂：所有元素一色 lightgrey #bdbdbd（Design 模块 autocolour 关、Add element colour = lightgrey）。
    E3dFactory,
    /// 保留今天按类型的配色（管道黄、结构蓝…），只换 E3D 的光照与后处理。
    ByType,
}

/// 抗锯齿。E3D 出厂是 4× MSAA；Bevy 的离屏视口在 WebGL2 上读不到多重采样的 prepass 纹理，
/// 两端统一用整帧 2×2 超采样代替（颜色与边线都是 4 样本平均，结构上与 E3D 一致），FXAA 是 E3D 3.1 也有的便宜选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AntiAlias {
    Off,
    Ssaa2x2,
    Fxaa,
}

/// 三维视口外观的全部设置，整份下发（[`Cmd::SetViewportLook`]）。老 `settings.ron` 没这些格就是出厂位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ViewportLook {
    pub mode: LookMode,
    pub preset: LookPreset,
    /// 边线（SGL_ENHANCED_EDGES：HLR 判边 + 去杂）。
    pub hlr: bool,
    /// 伪阴影（SGL_PSEUDO_SHADOWS：HBAO + 深度感知模糊）。
    pub ao: bool,
    /// 背景渐变（SGL_BACKGROUND_GRADIENT：背景色在上 → 端色白在下）。
    pub gradient: bool,
    pub aa: AntiAlias,
    pub colour_scheme: ColourScheme,
}

impl Default for ViewportLook {
    /// 出厂就是 E3D 外观 + 出厂 E3D 3.1 预设（2026-10-22 拍板）。
    fn default() -> Self {
        Self::for_preset(LookPreset::E3d31Factory)
    }
}

impl ViewportLook {
    /// 某预设的出厂位：E3D 外观、E3D 出厂配色；四个子开关照预设（出厂 E3D 3.1 全开 + 2×2 超采样，
    /// 本机真机全关 + 无抗锯齿）。
    pub const fn for_preset(preset: LookPreset) -> Self {
        let on = matches!(preset, LookPreset::E3d31Factory);
        Self {
            mode: LookMode::E3d,
            preset,
            hlr: on,
            ao: on,
            gradient: on,
            aa: if on {
                AntiAlias::Ssaa2x2
            } else {
                AntiAlias::Off
            },
            colour_scheme: ColourScheme::E3dFactory,
        }
    }

    /// 换预设：光照常量换掉，四个子开关回到该预设的出厂位；外观与元素配色不动
    /// （同 plant3d-web `useRenderLookStore.setPreset`）。
    pub fn apply_preset(&mut self, preset: LookPreset) {
        let factory = Self::for_preset(preset);
        self.preset = preset;
        self.hlr = factory.hlr;
        self.ao = factory.ao;
        self.gradient = factory.gradient;
        self.aa = factory.aa;
    }

    /// 四个子开关有没有偏离当前预设的出厂位（设置窗「恢复预设出厂位」按它亮灰）。
    pub fn is_modified_from_preset(&self) -> bool {
        let factory = Self::for_preset(self.preset);
        self.hlr != factory.hlr
            || self.ao != factory.ao
            || self.gradient != factory.gradient
            || self.aa != factory.aa
    }

    /// 后处理（边线 / 伪阴影）这一刻是否一样都不跑（sglDx11 的 `_legacy_mode`）。PBR 外观下恒 true。
    pub fn effects_off(&self) -> bool {
        self.mode == LookMode::Pbr || (!self.hlr && !self.ao)
    }
}

/// 外观的局部覆盖：哪格给了就压哪格。浏览器端没有 `settings.ron`，站点在 `config.json` 的 `look` 段给默认
///（字段同 [`ViewportLook`]，可以只给一部分），地址栏 `?look=…` 再压一层、只活到刷新
///（照 plant3d-web `applyQueryOverrides`）；原生端不经过它。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct ViewportLookOverrides {
    pub mode: Option<LookMode>,
    pub preset: Option<LookPreset>,
    pub hlr: Option<bool>,
    pub ao: Option<bool>,
    pub gradient: Option<bool>,
    pub aa: Option<AntiAlias>,
    pub colour_scheme: Option<ColourScheme>,
}

impl ViewportLookOverrides {
    /// 地址栏查询串（带不带 `?` 都行）：`look=sgl|e3d|pbr`、`look_preset=factory|machine`、
    /// `look_hlr=0|1`、`look_ao=0|1`、`look_gradient=0|1`、`look_aa=0|fxaa|ssaa`、`look_colours=e3d|type`。
    /// 认不出的键值当没写。
    pub fn from_query(query: &str) -> Self {
        let mut out = Self::default();
        for pair in query.trim_start_matches('?').split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let value = value.trim().to_ascii_lowercase();
            let flag = match value.as_str() {
                "0" | "off" | "false" | "no" => Some(false),
                "1" | "on" | "true" | "yes" => Some(true),
                _ => None,
            };
            match key.trim() {
                "look" => {
                    out.mode = match value.as_str() {
                        "sgl" | "e3d" => Some(LookMode::E3d),
                        "pbr" => Some(LookMode::Pbr),
                        _ => None,
                    }
                }
                "look_preset" => {
                    out.preset = match value.as_str() {
                        "factory" | "e3d31_factory" => Some(LookPreset::E3d31Factory),
                        "machine" | "sgl_machine" => Some(LookPreset::SglMachine),
                        _ => None,
                    }
                }
                "look_hlr" => out.hlr = flag,
                "look_ao" => out.ao = flag,
                "look_gradient" => out.gradient = flag,
                "look_aa" => {
                    out.aa = match value.as_str() {
                        "0" | "off" => Some(AntiAlias::Off),
                        "fxaa" => Some(AntiAlias::Fxaa),
                        "ssaa" | "ssaa2x2" | "4" => Some(AntiAlias::Ssaa2x2),
                        _ => None,
                    }
                }
                "look_colours" | "look_colors" => {
                    out.colour_scheme = match value.as_str() {
                        "e3d" | "e3d_factory" => Some(ColourScheme::E3dFactory),
                        "type" | "by_type" => Some(ColourScheme::ByType),
                        _ => None,
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// 两层叠：`over` 给了的格压过 `self` 的。
    pub fn then(self, over: Self) -> Self {
        Self {
            mode: over.mode.or(self.mode),
            preset: over.preset.or(self.preset),
            hlr: over.hlr.or(self.hlr),
            ao: over.ao.or(self.ao),
            gradient: over.gradient.or(self.gradient),
            aa: over.aa.or(self.aa),
            colour_scheme: over.colour_scheme.or(self.colour_scheme),
        }
    }

    /// 压到一份外观上：先换预设（四个子开关回到该预设的出厂位），再逐格压上给了的值。
    pub fn apply(&self, mut look: ViewportLook) -> ViewportLook {
        if let Some(preset) = self.preset {
            look.apply_preset(preset);
        }
        if let Some(mode) = self.mode {
            look.mode = mode;
        }
        if let Some(hlr) = self.hlr {
            look.hlr = hlr;
        }
        if let Some(ao) = self.ao {
            look.ao = ao;
        }
        if let Some(gradient) = self.gradient {
            look.gradient = gradient;
        }
        if let Some(aa) = self.aa {
            look.aa = aa;
        }
        if let Some(colour_scheme) = self.colour_scheme {
            look.colour_scheme = colour_scheme;
        }
        look
    }
}

/// UI 发出的命令：独立应用直接执行，接进 Bevy 后转成 Event。
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    SelectElement(aios_core::RefU64),
    /// 导航历史：后退 / 前进 / 跳到某一条。
    ///
    /// 历史栈归宿主——一条 = 选择集 + 相机位姿 + 激活页签（+ 房间聚焦），记录点在
    /// 宿主的 `set_selection` / `focus_room`；绘制层只拿 `vm.nav` 画按钮与右键列表。
    /// 回放期间宿主自己守住不再入栈，绘制层不必知道「这次选中是回放来的」。
    Navigate(NavStep),
    /// 宿主 → 视口：把相机送回一份记下的位姿（导航历史回放）。
    ///
    /// 与 `SnapView` 走同一条 0.3s 插值动画，中途任何手势立即让位。绘制层不会发它
    /// ——它和 `ResizeViewport` / `SetViewportGrid` 一样，只是借这条队列过路。
    RestoreCamera(CameraPose),
    /// 模型树上的选择集变更（Ctrl 加减 / Shift 区间）。
    ///
    /// 绘制层算好**结果**整体交出，而不是发「加了谁减了谁」：区间要按可见行序算，
    /// 那个序只有绘制层手上的 rows 才有，宿主复原不出来。
    SetSelection(vm::Selection),
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
    RetryRooms(aios_core::RefU64),
    /// 房间视图（`CONTEXT.md`「房间视图」）：隔离这间房的成员，把房间体画成半透明，相机对准
    /// 房间体，同时把这间房的详情填进「房间」页签。
    ///
    /// 宿主先经数据层解析成员集（一拍往返），再展开为 `Model(Isolate)`，同时去取房间体（在册面板
    /// 的模型），回来后取景并交给视口。直段截断开着时，直段只画房间体内的那一截（2026-09-29
    /// 直段截断计划）。没有实时渲染器时视口那一半自然缺席，详情照常。
    FocusRoom(aios_core::RefU64),
    /// 房间视图 HUD 上的「截断直段」开关（`CONTEXT.md`「直段截断」）。
    ///
    /// 这是一项显示偏好，宿主记进设置（桌面端落盘），并对正在进行的房间视图立即生效：按新口径
    /// 重新隔离，视口复原或截断直段。不在房间视图里时只记设置。
    SetTubeTruncation(bool),
    /// 把这间房自己的模型（墙面板那些几何）取回来显示，再取景到它。
    ///
    /// 与 [`Self::FocusRoom`] 分成两条：那条是「只看这间房」——隔离既有场景再
    /// 取景，而隔离与取景都只对**已经加载进场景**的模型生效。房间的墙面板多半
    /// 从没被加载过，于是那条路走完往往是别的模型全隐藏了、房间本身仍然看不见。
    /// 这条走的是显示模型那条既有链路：范围没缓存过就先去查、把几何取回来。
    ShowRoomModel(aios_core::RefU64),
    /// 把这间房自己的模型藏起来。与 [`Self::ShowRoomModel`] 成对，走的也是同一条
    /// 显隐链路——房间 FRMW 当作一个范围目标交给宿主的 `set_model_visible`。
    ///
    /// 与 [`ModelAction::SetVisible`] 分开的原因同上面那条：房间 FRMW 从来不在
    /// 「节点下的构件」里，把它交给按构件收目标的显隐路径是找不到东西的。
    HideRoomModel(aios_core::RefU64),
    /// 打开「房间」页签并对准该元素：元素不是当前主选中时先把选中切过去
    /// （归属数据跟着选中预取，页签一亮出来就有内容）。
    ShowRooms(aios_core::RefU64),
    /// 打开房间浏览器浮窗并（重新）拉取全表。全表是全库扫描级的重查询，
    /// 所以由这条命令按需触发，不进启动路径；查询在途时再按只开窗不重发。
    OpenRoomBrowser,
    /// 「房间树」面板（plant3d-web「房间」页签移植，ADR 0068）的动作：取根清单 / 取一间房的树、
    /// 展开、过滤、点选、显隐 / 聚焦 / 隔离 / 加载。节点身份是 [`room_tree::RoomNodeId`]——分组行
    /// （专业 / 单元类型 / 其他构件）没有 refno，模型树那套按 refno 的命令套不上。宿主把节点下的
    /// refno 收齐（单元级及以上连所属 BRAN）再转成既有的 [`ModelAction`]，没有第二套显隐路径。
    RoomTree(room_tree::RoomTreeAction),
    /// 「房间」页签上的「在房间树查看」：切到房间树面板，取这间房的树、展开到第一层、滚到它。
    /// 带的是房间 FRMW 的 refno；不在册（服务端清单里没有）时宿主在日志里说一句。
    RevealRoomInTree(aios_core::RefU64),
    /// 右键「查看尺寸标注」（计划 B2）：取这条 BRAN 的 MBD 尺寸标注并挂到视口上。
    ///
    /// 带的是**BRAN** 的 refno，不是右键落点：落点是 ELBO / ATTA 这类成员时，归属到
    /// 所在 BRAN 的那一步在绘制层就做完了（`TreeRowVm::dimension_branch` /
    /// `WorkbenchVm::selection_branch`，两者都由宿主对着已加载的树算好），宿主拿到的
    /// 一律是可以直接去问服务的目标。同时只挂一条：再看另一条就把前一条换掉。
    ShowPipeDimensions(aios_core::RefU64),
    /// 右键「隐藏尺寸标注」：清掉视口上挂着的尺寸标注层。不带 refno——这一层同时只有
    /// 一条 BRAN，清的就是它；在途的取数回来也不再上屏（宿主按帧号丢弃）。
    ClearPipeDimensions,
    /// 把某个常驻视图切到前台（如待重算横幅 -> 任务队列）。找不到该页签时
    /// 宿主是无操作——用户可以把页签拖走，那是他的布局。
    FocusPane(workbench::Pane),
    /// 收起 / 展开某一侧 dock（左 / 下 / 右），腾挪工作台空间。
    ///
    /// dock 布局是界面自身的状态而非数据，所以这条不进 Vm：宿主转手交给
    /// `WorkbenchState`（与 `FocusPane` 同一路），由绘制层按可见性重建 dock。
    ToggleDock(workbench::DockSide),
    /// 清空日志缓冲。
    ClearLogs,
    /// 提交一条命令；解析与执行由宿主负责。
    SubmitCommand(String),
    /// 标题栏搜索框：按名称找元素。
    ///
    /// 一条命令两路结果：前缀走 `pe.name` 索引（全库范围，毫秒级），子串走本地
    /// ngram 索引（当前 MDB 范围，亚毫秒）。两路都跟着按键走——子串那一路曾经
    /// 是秒级的库内逐行扫，只能由回车显式发起，ADR-0023 之后不再是了。
    SearchElements {
        query: String,
    },
    /// 关掉搜索：结果作废，在途的那次回来也不再上屏。
    CloseSearch,
    /// 打开项目选择窗口。
    OpenProjectPicker,
    /// 使用宿主提供的配置加载一个项目。
    LoadProject(String),
    /// 取回工作：把库里此刻的样子取到界面上来——丢掉本端缓存、重查已展开的
    /// 树分支、重载已显示的模型。
    ///
    /// **它不触发后端增量更新。** 数据是别人写进去的还是自动同步追进来的，
    /// 这一端都不管；手动发起一轮增量是「模型更新」那个三步窗的事，两者
    /// 不合并——合并之后自动同步开着时这个菜单会被服务端直接拒掉。
    ///
    /// 不带 refno：变化明细本端拿不到（自动同步只广播摘要），所以刷新范围只能
    /// 是「当前看得见的那些」，由处置方按自己的展开状态与已加载集合算出来。
    GetWork,
    /// 打开设置任务窗。
    OpenSettings,
    /// 打开三维数据发布任务窗。
    OpenDataPublish,
    OpenManualDataPublish,
    SubmitRoomCodePublish(manual_data_publish::RoomCodePublishRequest),
    /// 提交三维数据发布请求；实际发送由宿主负责。
    SubmitDataPublish(data_publish::PublishRequest),
    LookupDataPublishDeletions(data_publish::PublishRequest),
    /// 打开并刷新模型增量更新预览。
    OpenModelUpdate,
    /// 重新读取当前项目的增量范围。
    RefreshModelUpdate,
    /// 放弃等待预览结果。**只放弃客户端这一侧**——服务端没有 cancel 接口，
    /// 扫描照跑，界面上也不许暗示它会停。
    CancelModelUpdatePreview,
    /// 执行当前项目的待更新库。`dbnums` 是勾选集折算的**范围内子集**
    /// （gen-model ADR-020）：`None` = 全范围（队列面板的即时扫描走它）；
    /// `Some` 时未勾选的库不入队、水位不动。向导确认总是显式给名单——
    /// 预览之后新冒出来的库不在那份确认里，不该被顺手执行。
    ExecuteModelUpdate {
        dbnums: Option<Vec<u32>>,
    },
    /// 任务队列上的「立刻扫一遍」。它**不插队**，作用只是别等服务端下一个 30 秒轮询。
    ScanNow,
    /// 库行上的「提前执行」（09-08 计划 D1 A）：对一个 dbnum 提前排一次它的任务，
    /// 打的还是既有 execute、带单库名单。它不插队、不改本期执行范围；与自动发现
    /// 排出的是同一种任务。
    RunDbnumNow {
        dbnum: u32,
    },
    /// 库行上的「重算模型（整库）」（2026-09-10 重新计算计划 D1 ② / R1）：对一个 dbnum 排一次
    /// 服务端整库模型重建（`POST /api/v1/dbnums/{dbnum}/model/rebuild`）——该库全部生成根投进
    /// 模型队列重算，不改数据水位，进任务队列一行；同库已在重建时服务端回同一个任务。
    /// 与模型树右键那条「重新生成」（对选中范围直接调服务端 generate）是两件事。
    RebuildDbnumModel {
        dbnum: u32,
    },
    /// 库行「清理模型（整库）」（2026-10-10 清理模型计划 E1 / C5）：只打开确认框。
    /// 确认之后才发 `DELETE /api/v1/dbnums/{dbnum}/model`。不自动重算。
    CleanupDbnumModel {
        dbnum: u32,
    },
    /// 顶栏「重新生成房间」（全库）：只打开确认框。确认之后才发
    /// `POST /api/v1/rooms/rebuild`——服务端清掉全部房间归属数据、换代重建常驻
    /// 参照模型、逐库整库重算房间关系；进度屏挂 `/health` 的 resident 读数。
    RebuildRooms,
    /// 暂停 / 恢复队列出队。暂停**只挡出队**，正在跑的那一批会跑完为止——
    /// 服务端没有中止接口，所以这条命令也不该被当成「停下来」。
    SetQueuePaused(bool),
    /// 增量记录页底部「加载更早」：以手上最老一条的入队时刻为上界再翻一页 `/tasks?before=`
    /// （gen-model 2026-10-24 起终态行落盘，重启前的批次也翻得到）。回包进 `task_queue::Vm::archived`，
    /// 轮询不覆盖。
    LoadOlderTasks,
    /// 增量记录页展开明细里「变化清单」：按需取这一批的元素级清单 `GET /tasks/{id}/changes`
    /// （gen-model 2026-10-24 W3）。回包进 `task_queue::Vm::changes`，取过就留着。
    LoadTaskChanges(String),
    /// 复活一行死信。自动路径到了重试上限就永不再碰它，人按这一下是**唯一**的出路。
    ///
    /// 它不排新的数据批次：服务端只把这一行的 `attempts` 清零、`revision` 加一，
    /// 再叫醒调度器，下一轮空闲积压消化会把它重新取到。所以按完之后队列面板上
    /// 不会立刻多出一行，变化要等下一拍轮询。
    RetryPendingUnit {
        dbnum: u32,
        root_refno: String,
    },
    /// 重开队列视图的明细长连接。断线降级的是明细区，队列行走轮询不受影响。
    ReconnectQueueFeed,
    /// 点击三维视口；坐标是离屏渲染纹理的归一化 UV。
    /// 测量态下宿主把它当取点，否则当模型拾取——判断哪一种归宿主。
    PickViewport([f32; 2]),
    /// 三维视口上的相机操作。与 `PickViewport` 分开：点是一次性的语义动作，
    /// 而相机是一段连续运动，两者的生命周期对不上。
    Camera(CameraGesture),
    /// 离屏渲染目标该开多大，单位是**物理像素**。
    ///
    /// 视口这块矩形跟着 dock 分隔条走，只有绘制层手上有这个数，宿主复原不出来。
    /// 报物理像素而不是逻辑点：150% 缩放下按点数开纹理，铺到屏幕上就是一次放大
    /// 插值，管子边缘会糊。
    ///
    /// 尺寸对不上的每一帧都会发一遍——绘制层没有状态，记不住自己提过。**防抖归
    /// 宿主**：拖分隔条时这条命令每帧都在变，每帧重建一张几兆的纹理会把拖动拖成
    /// 幻灯片。
    ResizeViewport([u32; 2]),
    /// 对模型的显示 / 定位类动作。
    Model(ModelAction),
    /// 树范围聚焦：查这个节点下已生成产物的包围盒并取景。只查不生（ADR-0030 D1）：
    /// 范围内没有已生成的模型就相机不动、日志提示右键「重新生成模型」。
    FocusTreeScope(RefU64),
    /// 主题下发的三维视口地面网格线色。
    ///
    /// 网格线是宿主（Bevy）侧重建的网格，而主题只有绘制层认识，所以切主题的当帧由 App 把颜色送下去；
    /// 宿主无状态，不认识「主题」，只认颜色。视口底色不经这里：它是绘制层画在视口纹理底下的渐变
    /// （App 每帧经 `look_background()` 按外观填——PBR 外观用主题 tokens，E3D 外观用 E3D 的 grey→白），
    /// E3D 外观下后处理的合成段还会把同一份底色画进离屏图。
    SetViewportGrid(egui::Color32),
    /// 设置下发的三维视口外观（E3D 外观移植计划）：外观 / 预设 / 四个子开关 / 元素配色一次整份送下去。
    ///
    /// 与 [`Self::SetViewportGrid`] 同理：宿主无状态，不认识「设置」，只认结果；首帧与每次设置变更由
    /// App 发，视口照它换材质、挂 / 摘后处理。底色随外观走但不归宿主记：切回 PBR 是绘制层重新按主题画。
    SetViewportLook(ViewportLook),
    /// ViewCube 触发的一次视角跳转。
    ///
    /// `forward` / `up` 是**宿主世界系**（场景已含 PDMS Z-up → Bevy Y-up 那次旋转）
    /// 的单位向量——PDMS 语义到世界系的换算只有绘制层的立方体模块知道，宿主拿来
    /// 直接用。`fit = true`（Home 键）时同时把距离拉到能装下全部可见模型。
    /// 跳转带 0.3s 插值动画，动画期间来任何相机手势立即让位。
    SnapView {
        forward: [f32; 3],
        up: [f32; 3],
        fit: bool,
    },
    /// 换过网格目录之后，把上一轮加载失败的网格重来一遍。
    ///
    /// 只碰失败的那些：网格文件名是内容哈希，已经加载成功的换个目录取到的还是
    /// 同一份内容。由宿主在设置保存之后发出，界面上没有对应的按钮。
    RetryFailedMeshes,
    /// 模型树 / 视口右键「重新生成模型」（ADR-0030 D2）：对这几行各一发
    /// `POST /model/generate { force: true }`，服务端自己解根、删旧、重算；生成完成之后
    /// 宿主再以同一批目标为祖先查 records 显示出来（等于替用户点一次眼睛）。
    ///
    /// **不进 [`ModelAction`]。** 那个枚举里的每一项改的都是「三维此刻画成什么样」，
    /// 撤销它只要再下一条相反的命令；这一条改的是模型库里的产物，重算替换掉的几何只能
    /// 再算一遍。两者混在一个枚举里，宿主那边就只剩注释在提醒这件事。
    ///
    /// 这一条只打开**确认框**（不带数字，客户端不清点、不解根、不删产物），等
    /// [`Self::ModelMutationConfirm`] 才发请求。
    RegenerateModels {
        targets: Vec<aios_core::RefU64>,
    },
    /// 模型树 / 视口右键「清理模型」（2026-10-10 清理模型计划）：对这几行各一发
    /// `DELETE /model/subtree?refno&confirm`，服务端按子树删已生成的产物、不重算（与右键
    /// 重新生成里的「先删」是服务端同一份实现）；成功之后宿主把这些目标名下已加载的模型卸掉。
    /// 「特殊情况」用：算错了要清掉再看、或只想清掉不想立刻重算。
    ///
    /// 确认框上另有一格「一并删除网格文件」（2026-10-23，默认勾上）：勾着时请求带
    /// `purge_meshes=true`，服务端把这片范围引用过的内容寻址 `.mesh` 也从磁盘上删掉。
    /// 库行「清理模型（整库）」那一档共用这一格，走的是另一条接口
    /// （`DELETE /dbnums/{dbnum}/model`，同样收 `purge_meshes`）。
    ///
    /// 与 [`Self::RegenerateModels`] 同理**不进 [`ModelAction`]**：删掉的产物只能再算回来。
    /// 这一条同样只打开确认框（与重新生成共用同一个框，文案按 kind 查表），等
    /// [`Self::ModelMutationConfirm`] 才发请求。
    CleanupModels {
        targets: Vec<aios_core::RefU64>,
    },
    /// 「重新生成模型」/「清理模型」确认框的回执。`false`（取消 / 关窗）时一个请求都不发。
    /// 一个框一条回执：框自己知道此刻开的是哪一件事。
    ModelMutationConfirm {
        accepted: bool,
        /// 确认框上「一并删除网格文件」那一格此刻勾没勾。两个清理档消费它（右键
        /// 「清理模型」走 `DELETE /model/subtree`，库行「清理模型（整库）」走
        /// `DELETE /dbnums/{dbnum}/model`，两条都收这个参数）；重新生成一律 `false`
        /// ——它走 `force = true`，服务端先删后算，网格删了要整批重写一遍。
        /// 默认勾上：网格是内容寻址的，删了别的范围复用的同一份也会没——这一格就是让人
        /// 在有意识的情况下决定要不要连盘上那份一起清。
        purge_meshes: bool,
    },
    /// 导入 AMS 虚拟孔洞表（xlsx/csv；ADR-0031 阶段一）。只打开文件选择：
    /// 原生端走系统对话框，浏览器端走隐藏的 `<input type=file>`；解析与映射都在
    /// 宿主拿到字节之后做，这条命令本身不带数据。
    ImportVirtualHoles,
    /// 在「虚拟孔洞」页签里选中一条记录（单选）。`hole_id` 是记录的 `Code`，
    /// 空串表示清掉选中。
    SelectVirtualHole(String),
    /// 手动改一条记录的映射目标（阶段一「映射失败时允许手动选择」）：把孔洞的
    /// `RelyItemREF` 改指到这个 refno。宿主把它当一次新的映射请求发出去，
    /// 原来的自动映射结果作废。
    MapVirtualHole {
        hole_id: String,
        refno: aios_core::RefU64,
    },
    /// 对当前会话里已映射的记录发起布尔预览（阶段四）。不带名单：范围就是
    /// 「此刻已映射且没在算」的那些——点「全部预览」和点单行「预览」都落到这里，
    /// 宿主按行发和按全量发的分别只体现在它先把哪些行翻成 Calculating。
    PreviewVirtualHoles,
    /// 清掉整层预览：恢复所有被替换的原始网格，撤掉切削体，作废在途的布尔
    /// 结果（进一帧 epoch）。不清孔洞列表本身——那是导入会话的内容。
    ClearVirtualHolePreview,
    /// 把相机对准一条孔洞的切削体（阶段二「定位」）。宿主先把孔洞的世界包围盒
    /// 交给视口取景；孔洞没映射实体也照常能定位——切削体是独立画的。
    FocusVirtualHole(String),
    /// 把相机对准孔洞映射到的实体，并选中它（阶段一「映射到 AMS 元素后定位」）。
    /// 与 [`Self::FocusVirtualHole`] 分开：那条对的是孔洞，这条对的是实体——
    /// 实体没加载时宿主先去显示它，落地了再取景。
    FocusVirtualHoleTarget(String),
    /// 原始 / 布尔预览两态开关（阶段二「恢复」的反向操作）。`true` 看原始网格，
    /// `false` 看布尔结果；只对已经算出结果的实体有意义，宿主把开关状态记在
    /// 会话里，之后的回执也按它落。
    ShowVirtualHoleOriginal(bool),
    /// 孔洞预览的 X-Ray 开关：打开时被开孔的实体画成半透明，孔洞切削体照常实画，
    /// 好看清孔洞在实体内部的样子。与房间 X-Ray 互斥——两家抢同一份半透明材质，
    /// 后拨的赢（宿主负责关掉另一家）。
    SetVirtualHoleXRay(bool),
}

#[cfg(test)]
mod tests {
    /// 重新生成与清理是对**模型库**的动作，不是对三维场景的动作。混进 `ModelAction`
    /// 之后，宿主那边处理可见性的那一路会顺手把它也接了，而那条路上没有确认框、
    /// 没有互斥、没有账本——删除会变成一次没人拦得住的点击。
    #[test]
    fn regeneration_and_cleanup_are_not_scene_actions() {
        let source = include_str!("lib.rs");
        let model_action = source
            .split_once("pub enum ModelAction {")
            .expect("ModelAction exists")
            .1
            .split_once("\n}")
            .expect("ModelAction ends")
            .0;
        assert!(
            !model_action.contains("Regenerate"),
            "重新生成不该是三维动作: {model_action}"
        );
        assert!(
            !model_action.contains("Cleanup") && !model_action.contains("Delete"),
            "清理不该是三维动作: {model_action}"
        );
        let cmd = source
            .split_once("pub enum Cmd {")
            .expect("Cmd exists")
            .1
            .split_once("\n}")
            .expect("Cmd ends")
            .0;
        for required in ["RegenerateModels {", "CleanupModels {", "ModelMutationConfirm {"] {
            assert!(cmd.contains(required), "{required}: {cmd}");
        }
        assert!(
            !cmd.contains("CleanupConfirm") && !cmd.contains("RegenerateConfirm"),
            "两件事一个确认框、一条回执: {cmd}"
        );
    }

    /// `/model/records` 的一条按 gen-model 的形状进来：`GeomInstQuery` 的键在顶层（flatten），直段记录
    /// 多一格 `tube`；没有 `tube` 的老记录照常读成 `None`，序列化回去也不会凭空多出这一格。
    #[test]
    fn a_model_record_reads_the_gen_model_shape_with_or_without_a_tube() {
        let raw = serde_json::json!({
            "refno": "24384_26229",
            "old_refno": null,
            "owner": "24384_26229",
            "world_aabb": {"mins": [0.0, 0.0, 0.0], "maxs": [30.0, 30.0, 196.5]},
            "world_trans": {
                "translation": [0.0, 0.0, 0.0],
                "rotation": [0.0, 0.0, 0.0, 1.0],
                "scale": [30.0, 30.0, 196.5]
            },
            "insts": [{"geo_hash": "2714441752093137411", "is_tubi": true}],
            "has_neg": false,
            "generic": "TUBI",
            "pts": null,
            "date": null,
            "tube": {"ordinal": 2, "from": "24384_1", "to": "24384_2"}
        });
        let record: super::ModelRecord = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(record.refno.refno().to_string(), "24384_26229");
        assert_eq!(record.generic, "TUBI");
        assert_eq!(record.insts.len(), 1);
        assert!(record.insts[0].is_tubi);
        assert_eq!(
            record.tube,
            Some(super::TubeSegment {
                ordinal: 2,
                from: "24384_1".into(),
                to: "24384_2".into(),
            })
        );
        let back = serde_json::to_value(&record).unwrap();
        assert_eq!(back["tube"], raw["tube"]);
        assert_eq!(back["generic"], "TUBI", "flatten：几何记录的键仍在顶层");

        let mut legacy = raw;
        legacy.as_object_mut().unwrap().remove("tube");
        let record: super::ModelRecord = serde_json::from_value(legacy).unwrap();
        assert_eq!(record.tube, None);
        assert!(serde_json::to_value(&record).unwrap().get("tube").is_none());
    }

    use super::{
        AntiAlias, ColourScheme, LookMode, LookPreset, ViewportLook, ViewportLookOverrides,
    };

    /// 2026-10-22 拍板：默认就是 E3D 外观的出厂 E3D 3.1 预设，元素一色 lightgrey，2×2 超采样。
    #[test]
    fn the_default_look_is_the_e3d_factory_preset() {
        let look = ViewportLook::default();
        assert_eq!(look.mode, LookMode::E3d);
        assert_eq!(look.preset, LookPreset::E3d31Factory);
        assert!(look.hlr && look.ao && look.gradient);
        assert_eq!(look.aa, AntiAlias::Ssaa2x2);
        assert_eq!(look.colour_scheme, ColourScheme::E3dFactory);
        assert!(!look.is_modified_from_preset());
        assert!(!look.effects_off());
    }

    /// 本机真机预设全关；换预设只动子开关，不动外观与配色（同 plant3d-web setPreset）。
    #[test]
    fn switching_preset_resets_the_toggles_but_keeps_mode_and_colours() {
        let mut look = ViewportLook::default();
        look.colour_scheme = ColourScheme::ByType;
        look.hlr = false;
        assert!(look.is_modified_from_preset());
        look.apply_preset(LookPreset::SglMachine);
        assert_eq!(look.preset, LookPreset::SglMachine);
        assert!(!look.hlr && !look.ao && !look.gradient);
        assert_eq!(look.aa, AntiAlias::Off);
        assert_eq!(look.mode, LookMode::E3d);
        assert_eq!(look.colour_scheme, ColourScheme::ByType);
        assert!(!look.is_modified_from_preset());
        assert!(look.effects_off());
    }

    /// PBR 外观下后处理一律不跑，不管子开关怎么拨。
    #[test]
    fn pbr_mode_never_runs_the_effects() {
        let mut look = ViewportLook::default();
        look.mode = LookMode::Pbr;
        assert!(look.effects_off());
    }

    /// 落盘字面是 plant3d-web 预设 id 的下划线写法（`sgl_machine`，不是 `sgl-machine`）：宿主的
    /// `settings.ron` 走同一套 serde 规则，而 RON 的枚举字面是标识符，连字符读不回来。老文件缺格
    /// 按出厂位读出来（`serde(default)`）。
    #[test]
    fn the_look_round_trips_and_tolerates_missing_fields() {
        let look = ViewportLook::for_preset(LookPreset::SglMachine);
        let text = serde_json::to_string(&look).unwrap();
        assert!(text.contains("\"sgl_machine\""), "{text}");
        assert!(text.contains("\"e3d_factory\""), "{text}");
        assert!(text.contains("\"e3d\""), "{text}");
        assert_eq!(serde_json::from_str::<ViewportLook>(&text).unwrap(), look);
        let partial: ViewportLook = serde_json::from_str(r#"{"mode":"pbr"}"#).unwrap();
        assert_eq!(partial.mode, LookMode::Pbr);
        assert_eq!(partial.preset, LookPreset::E3d31Factory);
        assert_eq!(partial.aa, AntiAlias::Ssaa2x2);
    }

    /// 浏览器端的两层覆盖：config.json 的 look 段可以只给一部分，地址栏的 `?look=…` 再压一层；
    /// 预设先换（子开关回出厂位）再逐格压值；认不出的键值当没写。
    #[test]
    fn look_overrides_layer_config_then_query_over_the_factory_look() {
        let site: ViewportLookOverrides =
            serde_json::from_str(r#"{"aa":"fxaa","colour_scheme":"by_type"}"#).unwrap();
        assert_eq!(site.aa, Some(AntiAlias::Fxaa));
        assert_eq!(site.mode, None);
        let query =
            ViewportLookOverrides::from_query("?look_demo=1&look=pbr&look_hlr=0&look_aa=bogus");
        assert_eq!(query.mode, Some(LookMode::Pbr));
        assert_eq!(query.hlr, Some(false));
        assert_eq!(query.aa, None, "认不出的值当没写");
        let look = site.then(query).apply(ViewportLook::default());
        assert_eq!(look.mode, LookMode::Pbr);
        assert!(!look.hlr);
        assert_eq!(look.aa, AntiAlias::Fxaa, "地址栏没给的格由站点默认顶上");
        assert_eq!(look.colour_scheme, ColourScheme::ByType);

        let machine = ViewportLookOverrides::from_query("look=sgl&look_preset=machine&look_ao=1")
            .apply(ViewportLook::default());
        assert_eq!(machine.mode, LookMode::E3d);
        assert_eq!(machine.preset, LookPreset::SglMachine);
        assert!(!machine.hlr, "换预设先回该预设的出厂位");
        assert!(machine.ao, "再压上地址栏给的值");
        assert_eq!(machine.aa, AntiAlias::Off);
        assert!(ViewportLookOverrides::from_query("").is_empty());
        assert_eq!(
            ViewportLookOverrides::default().apply(ViewportLook::default()),
            ViewportLook::default()
        );
    }
}
