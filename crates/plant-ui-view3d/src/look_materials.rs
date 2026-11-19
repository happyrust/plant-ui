//! 「这一刻该用哪枚材质」的唯一出处（E3D 外观移植计划 §决策「实体着色」「选中」）。
//!
//! 两种外观是两种组件类型：PBR 外观 = `MeshMaterial3d<StandardMaterial>`（改动前的画面，逐像素不变），
//! E3D 外观 = `MeshMaterial3d<SglLookMaterial>`；切外观是一次全场换组件，不是改 uniform。句柄按
//! 外观 × 颜色 × 画法 × 状态缓存——同类构件成千上万，逐网格建材质既费显存也断批。
//!
//! 模型网格出生时**不带材质**：`load_models`、房间截断补出来的兄弟段都只挂 `ModelMesh`，材质由
//! `apply_materials` 同帧按 `Added<ModelMesh>` 补上；虚拟孔洞的 `apply_hole_materials` 也从这里取。
//! 一个出处，两种外观才不会各漏一处。
//!
//! 选中配色（填色 + 描边色）也在这儿定：[`SelectionPalette`]。选中 / 悬停**只换颜色**——明暗照算、透明度还是元素
//! 自己的，「选了谁」由选中描边（`selection_outline`）交代（ADR-0032 修订 2026-11-18）。只有 PBR 外观的悬停在
//! 换色之外再加一点自发光（[`PBR_HOVER_GLOW`]）：那里的常态是类型色，换成悬停色不一定看得出来。
//! 房间壳例外：它的类型色不透明，选中 / 悬停时压到 [`ROOM_SHELL_SELECTED_ALPHA`]（ADR-0032 修订 2026-11-19）。

use std::collections::HashMap;

use bevy::color::ColorToPacked;
use bevy::prelude::*;
use plant_ui::{ColourScheme, LookMode, ViewportLook};

use crate::e3d_look::colour_table::{
    E3D_ACTIVE_COLOUR, E3D_CE_COLOUR, E3D_DEFAULT_ELEMENT_COLOUR, E3D_HIGHLIGHT_COLOUR, rgb_bytes,
};
use crate::e3d_look::material::{EFFECT_ALL, SglLookMaterial};
use crate::e3d_look::presets::preset_light;
use crate::{
    MaterialState, ModelMesh, SELECT_COLOR, XRAY_COLOR, fallback_line_material, model_material,
};

/// `XraySelected` 的 alpha 字节：比房间 X-Ray 的 64 实得多（穿透还看得见，但一眼能认出点中了谁），
/// 又没到不透明（不透明会把房间体内部挡死，那正是 X-Ray 存在的理由）。
const XRAY_SELECTED_ALPHA: u8 = 160;

/// 房间壳（`ROOM` / `PANE`）选中、悬停与 X-Ray 叠选中时的 alpha 字节上限。房间壳的类型色不透明，只换颜色的话
/// 一选中（甚至鼠标划过）整间房就成了一块实心色，里面全挡死。真实库里一块 PANE 往往就是整间房，房间 X-Ray
/// 开着时被选中的也正是它，所以 X-Ray 叠选中也落这一档而不是 [`XRAY_SELECTED_ALPHA`]：X-Ray 回包落地那一下不跳，
/// 也不会反而变实。比房间 X-Ray 的 64 实一点；「点中了谁」由选中色与选中描边交代，不靠不透明度。
const ROOM_SHELL_SELECTED_ALPHA: u8 = 96;

/// PBR 外观悬停实体的自发光，占悬停色（线性空间）的比例。只换颜色不够：设备的类型色与缺省悬停橙是同一个
/// `#ffbe00`，换了画面不变；背光面本来就暗，乘出来的颜色也看不出换没换。自发光是加上去的，这两种情况都
/// 亮得起来，又没亮到把明暗抹平。半透明件的自发光被着色器按透明度再压一遍，几乎不显——它们靠换色。
const PBR_HOVER_GLOW: f32 = 0.3;

/// plant3d-web 默认外观的选中色（`ViewerPanel.vue` 的 `WEB_SELECTION_COLOR` / `WEB_HOVER_COLOR` 与
/// `outlineStyle.edgeColor`）：品红填色、橙色悬停、粉色描边。只在 [`SelectionPalette::Web`] 下用。
const WEB_SELECTION_COLOUR: u32 = 0xff4fd8;
const WEB_HOVER_COLOUR: u32 = 0xffaa44;
const WEB_OUTLINE_COLOUR: u32 = 0xff9ae8;

/// 选中配色：填色与描边色在这一处定，两种外观各取一列（ADR-0032 修订 2026-11-18）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) enum SelectionPalette {
    /// 本项目的口径（缺省）：E3D 外观主选中 CE 黄、其它选中白，PBR 外观选中橙红；
    /// 悬停两种外观都是橙，描边都是橙红。
    #[default]
    Native,
    /// plant3d-web 的口径（`PLANT_UI_SELECTION_PALETTE=web`）：主选中与其它选中都是品红、悬停橙、描边粉。
    /// 品红与剖面封口色（`room_clip` 的 `SECTION_COLOR`）几乎同色，所以不是缺省。
    Web,
}

impl SelectionPalette {
    pub(crate) fn from_env() -> Self {
        Self::parse(std::env::var("PLANT_UI_SELECTION_PALETTE").ok().as_deref())
    }

    /// 只认 `web`；没写、写错都是本项目口径。
    fn parse(value: Option<&str>) -> Self {
        match value
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("web") => Self::Web,
            _ => Self::Native,
        }
    }

    /// E3D 外观：主选中（CE）的填色，PDMS `0xRRGGBB`。
    fn e3d_ce(self) -> u32 {
        match self {
            Self::Native => E3D_CE_COLOUR,
            Self::Web => WEB_SELECTION_COLOUR,
        }
    }

    /// E3D 外观：其它选中的填色。
    fn e3d_highlight(self) -> u32 {
        match self {
            Self::Native => E3D_HIGHLIGHT_COLOUR,
            Self::Web => WEB_SELECTION_COLOUR,
        }
    }

    /// 悬停预选的填色，两种外观同一个：缺省是 E3D 的 active 橙，plant3d-web 口径是它自己的悬停橙。
    fn hover(self) -> u32 {
        match self {
            Self::Native => E3D_ACTIVE_COLOUR,
            Self::Web => WEB_HOVER_COLOUR,
        }
    }

    /// PBR 外观：选中的填色（主选中与其它选中不分）。
    fn pbr_selected(self) -> Color {
        match self {
            Self::Native => SELECT_COLOR,
            Self::Web => rgb_colour(WEB_SELECTION_COLOUR),
        }
    }

    /// 选中描边的颜色，两种外观同一个。橙红在 E3D 外观的浅色渐变底、PBR 外观的深浅两套主题底上都站得住，
    /// 也是 rs-plant3-d 当年描边用的那个色。
    pub(crate) fn outline(self) -> Color {
        match self {
            Self::Native => SELECT_COLOR,
            Self::Web => rgb_colour(WEB_OUTLINE_COLOUR),
        }
    }
}

/// 一枚挑好的模型材质。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ModelMaterial {
    Pbr(Handle<StandardMaterial>),
    Sgl(Handle<SglLookMaterial>),
}

impl ModelMaterial {
    /// 挂到实体上：同类组件就地改句柄（没变就不碰，免得白触发变更检测与重提取），
    /// 异类则摘掉另一种外观的组件再挂新的。
    pub(crate) fn attach(
        self,
        entity: Entity,
        pbr: Option<Mut<'_, MeshMaterial3d<StandardMaterial>>>,
        sgl: Option<Mut<'_, MeshMaterial3d<SglLookMaterial>>>,
        commands: &mut Commands,
    ) {
        match self {
            Self::Pbr(handle) => {
                if sgl.is_some() {
                    commands
                        .entity(entity)
                        .remove::<MeshMaterial3d<SglLookMaterial>>();
                }
                match pbr {
                    Some(mut current) => {
                        if current.0 != handle {
                            current.0 = handle;
                        }
                    }
                    None => {
                        commands.entity(entity).insert(MeshMaterial3d(handle));
                    }
                }
            }
            Self::Sgl(handle) => {
                if pbr.is_some() {
                    commands
                        .entity(entity)
                        .remove::<MeshMaterial3d<StandardMaterial>>();
                }
                match sgl {
                    Some(mut current) => {
                        if current.0 != handle {
                            current.0 = handle;
                        }
                    }
                    None => {
                        commands.entity(entity).insert(MeshMaterial3d(handle));
                    }
                }
            }
        }
    }
}

/// 建材质要用到的资产与共享句柄。
pub(crate) struct MaterialAssets<'a> {
    pub(crate) standard: &'a mut Assets<StandardMaterial>,
    pub(crate) sgl: &'a mut Assets<SglLookMaterial>,
    /// 环境立方体贴图；`None` 走天地两段灰度兜底。
    pub(crate) env: Option<Handle<Image>>,
    /// 房间 X-Ray 共用的那枚半透明 `StandardMaterial`：PBR 外观与无光照线的 X-Ray 态都用它。
    pub(crate) xray: Handle<StandardMaterial>,
}

/// 缓存键 = **解析后**的颜色（sRGB 字节）+ 画法 + 状态。键里放的是算出来的颜色而不是类型色：
/// E3D 出厂配色下所有类型都是 lightgrey，一色就该一枚，几千个构件才合得成一批。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MaterialKey {
    colour: [u8; 4],
    line: bool,
    state: MaterialState,
}

/// 按外观缓存的模型材质句柄表。
#[derive(Resource, Default)]
pub(crate) struct LookMaterials {
    /// 表是按哪份外观建的；外观一变整表作废——光照常量、配色都可能变了，旧句柄没人引用就随资产回收。
    look: Option<ViewportLook>,
    /// 选中配色。进程内不变（启动时读一次环境变量），所以不进「外观变了整表作废」的判据。
    palette: SelectionPalette,
    standard: HashMap<MaterialKey, Handle<StandardMaterial>>,
    sgl: HashMap<MaterialKey, Handle<SglLookMaterial>>,
}

impl LookMaterials {
    pub(crate) fn with_palette(palette: SelectionPalette) -> Self {
        Self {
            palette,
            ..default()
        }
    }

    pub(crate) fn palette(&self) -> SelectionPalette {
        self.palette
    }

    /// 这块网格在这份外观、这个状态下该用的材质。
    pub(crate) fn resolve(
        &mut self,
        look: ViewportLook,
        mesh: &ModelMesh,
        state: MaterialState,
        assets: &mut MaterialAssets<'_>,
    ) -> ModelMaterial {
        if self.look != Some(look) {
            self.standard.clear();
            self.sgl.clear();
            self.look = Some(look);
        }
        match look.mode {
            // PBR 外观：常态按类型色（含半透明），CE 与其它选中都是选中色（缺省橙红 `SELECT_COLOR`），
            // 悬停预选是悬停橙加一点自发光。X-Ray 命中且选中时仍是那枚共享的 X-Ray 材质——PBR 外观不改
            // X-Ray 的观感（只有 E3D 外观叠黄）。
            LookMode::Pbr => {
                let colour = match state {
                    MaterialState::Base => bytes_colour(mesh.colour),
                    // 选中只换颜色，透明度还是元素自己的（与 plant3d-web 同口径）：半透明的房间体被选中后
                    // 不再变成一整块不透明色把里面挡死。
                    MaterialState::Ce | MaterialState::Highlight => self
                        .palette
                        .pbr_selected()
                        .with_alpha(selected_alpha(mesh) as f32 / 255.0),
                    // 悬停预选的颜色同样不动透明度：鼠标掠过半透明的房间体时，里面的构件还得看得见。
                    // 实体另加的那点自发光在 `standard` 里。
                    MaterialState::Hover => rgb_colour(self.palette.hover())
                        .with_alpha(selected_alpha(mesh) as f32 / 255.0),
                    MaterialState::XRay | MaterialState::XraySelected => XRAY_COLOR,
                };
                ModelMaterial::Pbr(self.standard(colour, mesh.line, state, assets))
            }
            LookMode::E3d => {
                let (rgb, alpha) = e3d_colour(look, self.palette, mesh, state);
                if mesh.line {
                    // 无效 TUBI 的 LineList 兜底线在 E3D 外观下仍是无光照 StandardMaterial（它是
                    // handles / aids 一类的辅助画法，不是实体），只换成 E3D 的配色。
                    let [r, g, b] = rgb_bytes(rgb);
                    let colour = Color::srgba_u8(r, g, b, alpha);
                    ModelMaterial::Pbr(self.standard(colour, true, state, assets))
                } else {
                    ModelMaterial::Sgl(self.sgl(look, rgb, alpha, state, assets))
                }
            }
        }
    }

    /// `StandardMaterial` 那一半：PBR 外观的实体与两种外观下的无光照线。X-Ray 共用 `XRayMaterial`；
    /// PBR 外观的悬停实体带自发光（[`hover_material`]），线不带——无光照材质不吃自发光。
    fn standard(
        &mut self,
        colour: Color,
        line: bool,
        state: MaterialState,
        assets: &mut MaterialAssets<'_>,
    ) -> Handle<StandardMaterial> {
        // PBR 外观不区分「X-Ray 命中且选中」：两态都落那枚共享的半透明蓝，画面与改动前一致。
        if matches!(state, MaterialState::XRay | MaterialState::XraySelected) {
            return assets.xray.clone();
        }
        let key = MaterialKey {
            colour: colour.to_srgba().to_u8_array(),
            line,
            state,
        };
        self.standard
            .entry(key)
            .or_insert_with(|| {
                assets.standard.add(if line {
                    fallback_line_material(colour)
                } else if state == MaterialState::Hover {
                    hover_material(colour)
                } else {
                    model_material(colour)
                })
            })
            .clone()
    }

    /// `SglLookMaterial` 那一半：E3D 外观的实体。
    fn sgl(
        &mut self,
        look: ViewportLook,
        rgb: u32,
        alpha: u8,
        state: MaterialState,
        assets: &mut MaterialAssets<'_>,
    ) -> Handle<SglLookMaterial> {
        let [r, g, b] = rgb_bytes(rgb);
        let key = MaterialKey {
            colour: [r, g, b, alpha],
            line: false,
            state,
        };
        self.sgl
            .entry(key)
            .or_insert_with(|| {
                let material = SglLookMaterial::new(
                    rgb,
                    alpha as f32 / 255.0,
                    &preset_light(look.preset),
                    assets.env.clone(),
                    EFFECT_ALL,
                );
                let material = match state {
                    // 房间 X-Ray 直出颜色：那是一层蒙版，不是实体。
                    MaterialState::XRay | MaterialState::XraySelected => material.flat(),
                    // 选中 / 悬停是染色：选中色不该被环境反射洗淡（出厂预设 Kr 0.8，照原公式 CE 黄会变成
                    // (255,255,79)、highlight 白与常态灰只差几十个灰阶），但也不该没有明暗——直出的选中件是一块
                    // 看不出形状的纯色（2026-10-29 → 2026-11-18）。染色只拿光照缩放颜色、不加白。
                    MaterialState::Ce | MaterialState::Highlight | MaterialState::Hover => {
                        material.tint()
                    }
                    MaterialState::Base => material,
                };
                assets.sgl.add(material)
            })
            .clone()
    }
}

/// E3D 外观下一个状态的颜色（PDMS `0xRRGGBB`）与 alpha 字节：X-Ray 用房间 X-Ray 的半透明蓝；
/// 选中 / 悬停取 [`SelectionPalette`]（缺省 CE 黄、其它选中白、悬停橙，`E3D_GRAPHICS_COLOUR_DEFAULTS`）；
/// 常态按配色方案——E3D 出厂一色 lightgrey，按类型沿用类型色。
///
/// 半透明跟着元素自己，**选中 / 悬停也一样**：半透明是显示属性，选中只换颜色（plant3d-web 的选中覆盖色
/// 也只换 RGB）。2026-10-29 曾把选中态一律归一成不透明，为的是半透明件「选了像没选」；代价是房间体
/// 一类的大半透明件一选中（甚至鼠标只是划过）就成了一整块不透明纯色，把里面全挡死。现在「选了谁」
/// 由选中描边交代，这一条撤回。房间壳另压一档（[`selected_alpha`]）。
fn e3d_colour(
    look: ViewportLook,
    palette: SelectionPalette,
    mesh: &ModelMesh,
    state: MaterialState,
) -> (u32, u8) {
    let [r, g, b, a] = mesh.colour;
    let selected = selected_alpha(mesh);
    match state {
        MaterialState::XRay => {
            let [xr, xg, xb, xa] = XRAY_COLOR.to_srgba().to_u8_array();
            (pack_rgb(xr, xg, xb), xa)
        }
        MaterialState::Ce => (palette.e3d_ce(), selected),
        MaterialState::Highlight => (palette.e3d_highlight(), selected),
        // 悬停预选：E3D 的 active orange。
        MaterialState::Hover => (palette.hover(), selected),
        // X-Ray 命中且同时在选中集里：保留穿透（仍是 Blend），但颜色换成主选中色、alpha 抬到
        // `XRAY_SELECTED_ALPHA`——否则房间视图里点中什么都只是那片淡蓝，「点了谁」看不出来。房间壳与它不开
        // X-Ray 时的选中同一档。
        MaterialState::XraySelected if mesh.room_shell => (palette.e3d_ce(), selected),
        MaterialState::XraySelected => (palette.e3d_ce(), XRAY_SELECTED_ALPHA),
        MaterialState::Base => match look.colour_scheme {
            ColourScheme::E3dFactory => (E3D_DEFAULT_ELEMENT_COLOUR, a),
            ColourScheme::ByType => (pack_rgb(r, g, b), a),
        },
    }
}

/// 选中 / 悬停态的 alpha 字节：元素自己的；房间壳压到 [`ROOM_SHELL_SELECTED_ALPHA`]，本来就更透的（演示厂的
/// 房间体）不动。
fn selected_alpha(mesh: &ModelMesh) -> u8 {
    let own = mesh.colour[3];
    if mesh.room_shell {
        own.min(ROOM_SHELL_SELECTED_ALPHA)
    } else {
        own
    }
}

/// PBR 外观悬停实体的材质：悬停色打底、照常打光，再加 [`PBR_HOVER_GLOW`] 的自发光。
fn hover_material(colour: Color) -> StandardMaterial {
    StandardMaterial {
        emissive: colour.to_linear() * PBR_HOVER_GLOW,
        ..model_material(colour)
    }
}

fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn bytes_colour([r, g, b, a]: [u8; 4]) -> Color {
    Color::srgba_u8(r, g, b, a)
}

fn rgb_colour(rgb: u32) -> Color {
    let [r, g, b] = rgb_bytes(rgb);
    Color::srgb_u8(r, g, b)
}

#[cfg(test)]
mod tests {
    use plant_ui::LookPreset;

    use super::*;
    use crate::e3d_look::material::{SHADING_FLAT, SHADING_TINT};
    use crate::{is_room_shell, type_color};

    struct Bench {
        standard: Assets<StandardMaterial>,
        sgl: Assets<SglLookMaterial>,
        xray: Handle<StandardMaterial>,
        table: LookMaterials,
    }

    impl Bench {
        fn new() -> Self {
            let mut standard = Assets::<StandardMaterial>::default();
            let xray = standard.add(crate::xray_material());
            Self {
                standard,
                sgl: Assets::default(),
                xray,
                table: LookMaterials::default(),
            }
        }

        fn resolve(
            &mut self,
            look: ViewportLook,
            mesh: &ModelMesh,
            state: MaterialState,
        ) -> ModelMaterial {
            let mut assets = MaterialAssets {
                standard: &mut self.standard,
                sgl: &mut self.sgl,
                env: None,
                xray: self.xray.clone(),
            };
            self.table.resolve(look, mesh, state, &mut assets)
        }

        fn standard_material(&self, material: &ModelMaterial) -> &StandardMaterial {
            let ModelMaterial::Pbr(handle) = material else {
                panic!("期望 StandardMaterial，拿到 {material:?}");
            };
            self.standard.get(handle).unwrap()
        }

        fn standard_colour(&self, material: &ModelMaterial) -> [u8; 4] {
            self.standard_material(material)
                .base_color
                .to_srgba()
                .to_u8_array()
        }

        fn sgl_colour(&self, material: &ModelMaterial) -> ([u8; 3], f32) {
            let ModelMaterial::Sgl(handle) = material else {
                panic!("期望 SglLookMaterial，拿到 {material:?}");
            };
            let material = self.sgl.get(handle).unwrap();
            let c = material.params.colour;
            (
                [
                    (c.x * 255.0).round() as u8,
                    (c.y * 255.0).round() as u8,
                    (c.z * 255.0).round() as u8,
                ],
                material.opacity(),
            )
        }
    }

    fn mesh(generic: &str, line: bool) -> ModelMesh {
        ModelMesh {
            refno: aios_core::RefU64(1),
            owner: aios_core::RefU64(1),
            colour: type_color(generic).to_srgba().to_u8_array(),
            line,
            room_shell: is_room_shell(generic),
        }
    }

    fn pbr_look() -> ViewportLook {
        let mut look = ViewportLook::default();
        look.mode = LookMode::Pbr;
        look
    }

    /// PBR 外观：类型色常态、橙红选中（CE 与其它选中不分，透明度跟元素自己）、X-Ray 共用那枚；同键同句柄。
    #[test]
    fn pbr_look_keeps_todays_colours_and_shares_handles() {
        let mut bench = Bench::new();
        let pipe = mesh("PIPE", false);
        let base = bench.resolve(pbr_look(), &pipe, MaterialState::Base);
        assert_eq!(
            bench.standard_colour(&base),
            type_color("PIPE").to_srgba().to_u8_array()
        );
        assert_eq!(
            bench.resolve(pbr_look(), &pipe, MaterialState::Base),
            base,
            "同一键不新建"
        );
        let ce = bench.resolve(pbr_look(), &pipe, MaterialState::Ce);
        let highlight = bench.resolve(pbr_look(), &pipe, MaterialState::Highlight);
        assert_eq!(
            bench.standard_colour(&ce),
            SELECT_COLOR.to_srgba().to_u8_array()
        );
        assert_eq!(
            bench.standard_colour(&highlight),
            SELECT_COLOR.to_srgba().to_u8_array()
        );
        assert_eq!(
            bench.resolve(pbr_look(), &pipe, MaterialState::XRay),
            ModelMaterial::Pbr(bench.xray.clone())
        );
        // PBR 外观不区分「X-Ray 命中且选中」：两态同落那枚共享的半透明蓝，画面与改动前一致。
        assert_eq!(
            bench.resolve(pbr_look(), &pipe, MaterialState::XraySelected),
            ModelMaterial::Pbr(bench.xray.clone())
        );
        let material = bench.standard.get(match &base {
            ModelMaterial::Pbr(handle) => handle,
            _ => unreachable!(),
        });
        let material = material.unwrap();
        assert!(!material.unlit);
        assert_eq!(material.cull_mode, None);
        assert!(material.double_sided);

        // 半透明元素（类型色 alpha 200）选中：只换颜色，alpha 还是元素自己的，材质仍是 Blend。
        let ce_type = mesh("CE", false);
        let selected = bench.resolve(pbr_look(), &ce_type, MaterialState::Ce);
        let [r, g, b, _] = SELECT_COLOR.to_srgba().to_u8_array();
        assert_eq!(bench.standard_colour(&selected), [r, g, b, 200]);
        let ModelMaterial::Pbr(handle) = &selected else {
            unreachable!()
        };
        assert_eq!(
            bench.standard.get(handle).unwrap().alpha_mode,
            AlphaMode::Blend
        );
    }

    /// PBR 外观的悬停预选：悬停橙打底、照常打光、透明度跟元素自己，实体再加一点自发光；常态与选中不发光。
    #[test]
    fn pbr_look_hover_is_the_hover_colour_with_a_glow() {
        let mut bench = Bench::new();
        let pipe = mesh("PIPE", false);
        let base = bench.resolve(pbr_look(), &pipe, MaterialState::Base);
        let hover = bench.resolve(pbr_look(), &pipe, MaterialState::Hover);
        assert_eq!(bench.standard_colour(&hover), [255, 190, 0, 255]);
        assert_ne!(hover, base, "悬停不是类型色");
        let material = bench.standard_material(&hover);
        assert!(!material.unlit, "悬停件照常打光");
        assert_eq!(
            material.emissive,
            Color::srgb_u8(255, 190, 0).to_linear() * PBR_HOVER_GLOW
        );
        for state in [
            MaterialState::Base,
            MaterialState::Ce,
            MaterialState::Highlight,
        ] {
            let material = bench.resolve(pbr_look(), &pipe, state);
            assert_eq!(
                bench.standard_material(&material).emissive,
                LinearRgba::BLACK,
                "{state:?} 不发光"
            );
        }

        // 类型色碰巧就是悬停橙的件（今天的设备）：颜色换了等于没换，认得出悬停靠的是那点自发光。
        let orange = ModelMesh {
            colour: [255, 190, 0, 255],
            ..mesh("EQUI", false)
        };
        let base = bench.resolve(pbr_look(), &orange, MaterialState::Base);
        let hover = bench.resolve(pbr_look(), &orange, MaterialState::Hover);
        assert_eq!(bench.standard_colour(&hover), bench.standard_colour(&base));
        assert_ne!(hover, base, "同色也不是同一枚材质");
        let glow = bench.standard_material(&hover).emissive;
        assert_ne!(glow, LinearRgba::BLACK);

        // 半透明元素（类型色 alpha 200）：透明度不变，材质仍是 Blend。
        let hover = bench.resolve(pbr_look(), &mesh("CE", false), MaterialState::Hover);
        assert_eq!(bench.standard_colour(&hover), [255, 190, 0, 200]);
        let material = bench.standard_material(&hover);
        assert_eq!(material.alpha_mode, AlphaMode::Blend);
    }

    /// 选中配色：缺省是本项目口径；`PLANT_UI_SELECTION_PALETTE=web` 换成 plant3d-web 的品红 / 橙 / 粉，
    /// 别的值都当没写。两种外观的填色与描边色一起换。
    #[test]
    fn the_selection_palette_switches_fill_and_outline_together() {
        assert_eq!(SelectionPalette::parse(None), SelectionPalette::Native);
        assert_eq!(SelectionPalette::parse(Some("")), SelectionPalette::Native);
        assert_eq!(
            SelectionPalette::parse(Some("magenta")),
            SelectionPalette::Native
        );
        assert_eq!(
            SelectionPalette::parse(Some(" Web ")),
            SelectionPalette::Web
        );
        assert_eq!(
            SelectionPalette::Native.outline().to_srgba().to_u8_array(),
            SELECT_COLOR.to_srgba().to_u8_array()
        );
        assert_eq!(
            SelectionPalette::Web.outline().to_srgba().to_u8_array(),
            [0xff, 0x9a, 0xe8, 0xff]
        );

        let mut bench = Bench::new();
        assert_eq!(bench.table.palette(), SelectionPalette::Native);
        bench.table = LookMaterials::with_palette(SelectionPalette::Web);
        assert_eq!(bench.table.palette(), SelectionPalette::Web);
        let pipe = mesh("PIPE", false);
        let look = ViewportLook::default();
        for state in [MaterialState::Ce, MaterialState::Highlight] {
            let material = bench.resolve(look, &pipe, state);
            assert_eq!(
                bench.sgl_colour(&material),
                ([0xff, 0x4f, 0xd8], 1.0),
                "{state:?}"
            );
            let material = bench.resolve(pbr_look(), &pipe, state);
            assert_eq!(
                bench.standard_colour(&material),
                [0xff, 0x4f, 0xd8, 0xff],
                "{state:?}"
            );
        }
        let hover = bench.resolve(look, &pipe, MaterialState::Hover);
        assert_eq!(bench.sgl_colour(&hover), ([0xff, 0xaa, 0x44], 1.0));
        let hover = bench.resolve(pbr_look(), &pipe, MaterialState::Hover);
        assert_eq!(bench.standard_colour(&hover), [0xff, 0xaa, 0x44, 0xff]);
        // X-Ray 叠选中跟着主选中色走。
        let xray_selected = bench.resolve(look, &pipe, MaterialState::XraySelected);
        assert_eq!(bench.sgl_colour(&xray_selected).0, [0xff, 0x4f, 0xd8]);
    }

    /// E3D 外观：实体是 `SglLookMaterial`，出厂配色一色 lightgrey（不同类型共用一枚）、CE 黄、其它选中白；
    /// 半透明跟着元素自己（选中 / 悬停也一样），选中 / 悬停是染色、X-Ray 是直出；按类型配色才回到类型色。
    #[test]
    fn e3d_look_paints_lightgrey_ce_yellow_highlight_white() {
        let mut bench = Bench::new();
        let look = ViewportLook::default();
        assert_eq!(look.mode, LookMode::E3d);
        assert_eq!(look.colour_scheme, ColourScheme::E3dFactory);
        let pipe = mesh("PIPE", false);
        let equi = mesh("EQUI", false);
        let base = bench.resolve(look, &pipe, MaterialState::Base);
        assert_eq!(bench.sgl_colour(&base), ([189, 189, 189], 1.0));
        assert_eq!(
            bench.resolve(look, &equi, MaterialState::Base),
            base,
            "一色 = 一枚"
        );
        let ce = bench.resolve(look, &pipe, MaterialState::Ce);
        assert_eq!(bench.sgl_colour(&ce), ([255, 255, 0], 1.0));
        let highlight = bench.resolve(look, &pipe, MaterialState::Highlight);
        assert_eq!(bench.sgl_colour(&highlight), ([255, 255, 255], 1.0));
        // 选中态是染色：明暗照算、不加白（不被 Kr 0.8 洗淡），也不再是没有明暗的直出色块。
        for material in [&ce, &highlight] {
            let ModelMaterial::Sgl(handle) = material else {
                unreachable!()
            };
            assert_eq!(
                bench.sgl.get(handle).unwrap().params.flags.z,
                SHADING_TINT,
                "选中态染色"
            );
        }
        // 半透明元素（类型色 alpha 200）：选中 / 悬停只换颜色，透明度还是元素自己的——不然房间体一类的
        // 大半透明件一选中就成了一整块不透明色，把里面全挡死。
        let ce_type = mesh("CE", false);
        let translucent = bench.resolve(look, &ce_type, MaterialState::Base);
        let (_, opacity) = bench.sgl_colour(&translucent);
        assert!((opacity - 200.0 / 255.0).abs() < 1e-6, "常态保留半透明");
        for (state, colour) in [
            (MaterialState::Ce, [255, 255, 0]),
            (MaterialState::Highlight, [255, 255, 255]),
            (MaterialState::Hover, [255, 190, 0]),
        ] {
            let material = bench.resolve(look, &ce_type, state);
            let (rgb, opacity) = bench.sgl_colour(&material);
            assert_eq!(rgb, colour, "{state:?}");
            assert!(
                (opacity - 200.0 / 255.0).abs() < 1e-6,
                "{state:?} 的透明度跟元素自己"
            );
        }
        // X-Ray：半透明蓝、不打光。
        let xray = bench.resolve(look, &pipe, MaterialState::XRay);
        let ([r, g, b], opacity) = bench.sgl_colour(&xray);
        assert_eq!([r, g, b], [89, 169, 255]);
        assert!((opacity - 64.0 / 255.0).abs() < 1e-6);
        let ModelMaterial::Sgl(handle) = &xray else {
            unreachable!()
        };
        assert_eq!(bench.sgl.get(handle).unwrap().params.flags.z, SHADING_FLAT);
        // X-Ray 叠选中：仍是半透明（穿透保留），但换成 CE 黄、alpha 抬到 160。
        let xray_selected = bench.resolve(look, &pipe, MaterialState::XraySelected);
        let ([r, g, b], opacity) = bench.sgl_colour(&xray_selected);
        assert_eq!([r, g, b], [255, 255, 0], "X-Ray 命中且选中 = CE 黄");
        assert!((opacity - 160.0 / 255.0).abs() < 1e-6, "比纯 X-Ray 实得多");
        assert_ne!(xray_selected, xray, "与纯 X-Ray 不是同一枚");
        let ModelMaterial::Sgl(handle) = &xray_selected else {
            unreachable!()
        };
        assert_eq!(bench.sgl.get(handle).unwrap().params.flags.z, SHADING_FLAT);
        // 悬停预选：E3D 的 active orange，同样是染色。
        let hover = bench.resolve(look, &pipe, MaterialState::Hover);
        assert_eq!(bench.sgl_colour(&hover), ([255, 190, 0], 1.0));
        let ModelMaterial::Sgl(handle) = &hover else {
            unreachable!()
        };
        assert_eq!(
            bench.sgl.get(handle).unwrap().params.flags.z,
            SHADING_TINT,
            "悬停态染色"
        );

        let mut by_type = look;
        by_type.colour_scheme = ColourScheme::ByType;
        let typed_pipe = bench.resolve(by_type, &pipe, MaterialState::Base);
        assert_eq!(bench.sgl_colour(&typed_pipe), ([250, 250, 30], 1.0));
        let typed_equi = bench.resolve(by_type, &equi, MaterialState::Base);
        assert_ne!(typed_pipe, typed_equi);
    }

    /// 房间壳（PANE / ROOM）的类型色不透明：常态照旧，选中 / 悬停（E3D 外观连 X-Ray 叠选中）压到
    /// `ROOM_SHELL_SELECTED_ALPHA`、PBR 外观落进 Blend；本来就比这一档更透的房间体（演示厂的 alpha 64）
    /// 选中后不变实。别的构件不受影响，见 `e3d_look_paints_lightgrey_ce_yellow_highlight_white`。
    #[test]
    fn room_shells_turn_see_through_when_selected_or_hovered() {
        let mut bench = Bench::new();
        let look = ViewportLook::default();
        let shell_opacity = ROOM_SHELL_SELECTED_ALPHA as f32 / 255.0;
        for generic in ["PANE", "ROOM"] {
            let shell = mesh(generic, false);
            assert!(shell.room_shell, "{generic}");
            assert_eq!(shell.colour[3], 255, "{generic} 的类型色不透明");
            let base = bench.resolve(look, &shell, MaterialState::Base);
            assert_eq!(bench.sgl_colour(&base).1, 1.0, "{generic} 常态不透明");
            for state in [
                MaterialState::Ce,
                MaterialState::Highlight,
                MaterialState::Hover,
                MaterialState::XraySelected,
            ] {
                let material = bench.resolve(look, &shell, state);
                let (_, opacity) = bench.sgl_colour(&material);
                assert!(
                    (opacity - shell_opacity).abs() < 1e-6,
                    "{generic} {state:?}"
                );
            }
            let base = bench.resolve(pbr_look(), &shell, MaterialState::Base);
            assert_eq!(bench.standard_material(&base).alpha_mode, AlphaMode::Opaque);
            for state in [
                MaterialState::Ce,
                MaterialState::Highlight,
                MaterialState::Hover,
            ] {
                let material = bench.resolve(pbr_look(), &shell, state);
                assert_eq!(
                    bench.standard_colour(&material)[3],
                    ROOM_SHELL_SELECTED_ALPHA,
                    "{generic} {state:?}"
                );
                assert_eq!(
                    bench.standard_material(&material).alpha_mode,
                    AlphaMode::Blend
                );
            }
        }

        assert!(!mesh("PIPE", false).room_shell && !mesh("CE", false).room_shell);

        let thin = ModelMesh {
            colour: [213, 191, 169, 64],
            ..mesh("PANE", false)
        };
        let selected = bench.resolve(look, &thin, MaterialState::Ce);
        let (_, opacity) = bench.sgl_colour(&selected);
        assert!((opacity - 64.0 / 255.0).abs() < 1e-6, "更透的不变实");
    }

    /// 无效 TUBI 的兜底线两种外观下都是无光照 StandardMaterial，只换配色。
    #[test]
    fn lines_stay_unlit_standard_materials_in_both_looks() {
        let mut bench = Bench::new();
        let line = mesh("TUBI", true);
        let pbr = bench.resolve(pbr_look(), &line, MaterialState::Base);
        assert_eq!(
            bench.standard_colour(&pbr),
            type_color("TUBI").to_srgba().to_u8_array()
        );
        let e3d = bench.resolve(ViewportLook::default(), &line, MaterialState::Base);
        assert_eq!(bench.standard_colour(&e3d), [189, 189, 189, 255]);
        let ce = bench.resolve(ViewportLook::default(), &line, MaterialState::Ce);
        assert_eq!(bench.standard_colour(&ce), [255, 255, 0, 255]);
        let hover = bench.resolve(pbr_look(), &line, MaterialState::Hover);
        assert_eq!(bench.standard_colour(&hover), [255, 190, 0, 255]);
        for material in [&pbr, &e3d, &ce, &hover] {
            let ModelMaterial::Pbr(handle) = material else {
                unreachable!()
            };
            assert!(bench.standard.get(handle).unwrap().unlit);
        }
        assert_eq!(
            bench.resolve(ViewportLook::default(), &line, MaterialState::XRay),
            ModelMaterial::Pbr(bench.xray.clone())
        );
        assert_eq!(
            bench.resolve(ViewportLook::default(), &line, MaterialState::XraySelected),
            ModelMaterial::Pbr(bench.xray.clone())
        );
    }

    /// 外观一变（预设换光照）整表作废：同键换新句柄；外观不变就一直是同一枚。
    #[test]
    fn changing_the_look_rebuilds_the_table() {
        let mut bench = Bench::new();
        let pipe = mesh("PIPE", false);
        let factory = ViewportLook::default();
        let first = bench.resolve(factory, &pipe, MaterialState::Base);
        assert_eq!(bench.resolve(factory, &pipe, MaterialState::Base), first);
        let mut machine = factory;
        machine.apply_preset(LookPreset::SglMachine);
        let second = bench.resolve(machine, &pipe, MaterialState::Base);
        assert_ne!(second, first);
        let ModelMaterial::Sgl(handle) = &second else {
            unreachable!()
        };
        let light = preset_light(LookPreset::SglMachine);
        assert_eq!(
            bench.sgl.get(handle).unwrap().params.coefficients.x,
            light.ambient
        );
        assert_ne!(
            bench.resolve(factory, &pipe, MaterialState::Base),
            first,
            "切回来也是重建，不缓存旧表"
        );
    }
}
