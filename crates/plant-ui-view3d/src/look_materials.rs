//! 「这一刻该用哪枚材质」的唯一出处（E3D 外观移植计划 §决策「实体着色」「选中」）。
//!
//! 两种外观是两种组件类型：PBR 外观 = `MeshMaterial3d<StandardMaterial>`（改动前的画面，逐像素不变），
//! E3D 外观 = `MeshMaterial3d<SglLookMaterial>`；切外观是一次全场换组件，不是改 uniform。句柄按
//! 外观 × 颜色 × 画法 × 状态缓存——同类构件成千上万，逐网格建材质既费显存也断批。
//!
//! 模型网格出生时**不带材质**：`load_models`、房间截断补出来的兄弟段都只挂 `ModelMesh`，材质由
//! `apply_materials` 同帧按 `Added<ModelMesh>` 补上；虚拟孔洞的 `apply_hole_materials` 也从这里取。
//! 一个出处，两种外观才不会各漏一处。

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
    standard: HashMap<MaterialKey, Handle<StandardMaterial>>,
    sgl: HashMap<MaterialKey, Handle<SglLookMaterial>>,
}

impl LookMaterials {
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
            // PBR 外观 = 改动前的画面：常态按类型色（含半透明），CE 与其它选中都是橙红 `SELECT_COLOR`。
            // X-Ray 命中且选中时仍是那枚共享的 X-Ray 材质——PBR 外观不改 X-Ray 的观感（只有 E3D 外观叠黄）。
            LookMode::Pbr => {
                let colour = match state {
                    MaterialState::Base => bytes_colour(mesh.colour),
                    MaterialState::Ce | MaterialState::Highlight => SELECT_COLOR,
                    // 悬停预选在 PBR 外观下不另配色：沿用今天的行为（未选中件就是类型色），
                    // 免得动改动前的画面。
                    MaterialState::Hover => bytes_colour(mesh.colour),
                    MaterialState::XRay | MaterialState::XraySelected => XRAY_COLOR,
                };
                ModelMaterial::Pbr(self.standard(colour, mesh.line, state, assets))
            }
            LookMode::E3d => {
                let (rgb, alpha) = e3d_colour(look, mesh.colour, state);
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

    /// `StandardMaterial` 那一半：PBR 外观的实体与两种外观下的无光照线。X-Ray 共用 `XRayMaterial`。
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
                // 选中态与 X-Ray 都直出颜色：选中是界面语义，不该被头灯 / 环境反射洗淡
                // （出厂预设 Kr 0.8，CE 黄的蓝通道会被抬到约 0.4，屏上从纯黄变成 (255,255,79)；
                // highlight 白被顶到 (249,249,249)，与常态 189 只差 60 个灰阶）。直出只关颜色项，
                // 合成段的 bytes = col · hlr · ao 照旧，边线与伪阴影都还在。
                let flat = matches!(
                    state,
                    MaterialState::XRay
                        | MaterialState::XraySelected
                        | MaterialState::Ce
                        | MaterialState::Highlight
                        | MaterialState::Hover
                );
                let material = if flat { material.flat() } else { material };
                assets.sgl.add(material)
            })
            .clone()
    }
}

/// E3D 外观下一个状态的颜色（PDMS `0xRRGGBB`）与 alpha 字节：X-Ray 用房间 X-Ray 的半透明蓝；
/// CE 黄、其它选中白（`E3D_GRAPHICS_COLOUR_DEFAULTS`）；常态按配色方案——E3D 出厂一色 lightgrey，
/// 按类型沿用类型色。
///
/// **常态**的半透明跟着元素自己：半透明是显示属性，不是颜色。**选中态**不继承它——选中是界面语义，
/// 元素自己的 alpha 会把 CE 黄稀释、还会让它掉进 `AlphaMode::Blend`（半透明网格不进 prepass，于是
/// 选中件没有边线与伪阴影，等于「选了像没选」）。所以 CE / Highlight 一律 alpha 255；
/// 这也是 E3D `EnhancedEdgesTranslucent = ON` 想要的观感。
fn e3d_colour(look: ViewportLook, colour: [u8; 4], state: MaterialState) -> (u32, u8) {
    let [r, g, b, a] = colour;
    match state {
        MaterialState::XRay => {
            let [xr, xg, xb, xa] = XRAY_COLOR.to_srgba().to_u8_array();
            (pack_rgb(xr, xg, xb), xa)
        }
        MaterialState::Ce => (E3D_CE_COLOUR, 255),
        MaterialState::Highlight => (E3D_HIGHLIGHT_COLOUR, 255),
        // 悬停预选：E3D 的 active orange，同样直出、不透明（未选中件本来也不该被半透明稀释）。
        MaterialState::Hover => (E3D_ACTIVE_COLOUR, 255),
        // X-Ray 命中且同时在选中集里：保留穿透（仍是 Blend），但颜色换成 CE 黄、alpha 抬到 160
        // ——否则房间视图里点中什么都只是那片淡蓝，「点了谁」看不出来。
        MaterialState::XraySelected => (E3D_CE_COLOUR, XRAY_SELECTED_ALPHA),
        MaterialState::Base => match look.colour_scheme {
            ColourScheme::E3dFactory => (E3D_DEFAULT_ELEMENT_COLOUR, a),
            ColourScheme::ByType => (pack_rgb(r, g, b), a),
        },
    }
}

fn pack_rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) << 16 | (g as u32) << 8 | b as u32
}

fn bytes_colour([r, g, b, a]: [u8; 4]) -> Color {
    Color::srgba_u8(r, g, b, a)
}

#[cfg(test)]
mod tests {
    use plant_ui::LookPreset;

    use super::*;
    use crate::type_color;

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

        fn standard_colour(&self, material: &ModelMaterial) -> [u8; 4] {
            let ModelMaterial::Pbr(handle) = material else {
                panic!("期望 StandardMaterial，拿到 {material:?}");
            };
            self.standard
                .get(handle)
                .unwrap()
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
        }
    }

    fn pbr_look() -> ViewportLook {
        let mut look = ViewportLook::default();
        look.mode = LookMode::Pbr;
        look
    }

    /// PBR 外观 = 改动前：类型色常态、橙红选中（CE 与其它选中不分）、X-Ray 共用那枚；同键同句柄。
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
    }

    /// E3D 外观：实体是 `SglLookMaterial`，出厂配色一色 lightgrey（不同类型共用一枚）、CE 黄、其它选中白；
    /// 常态的半透明跟着元素自己，选中态归一成不透明；按类型配色才回到类型色。
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
        // 选中态直出颜色（不打光不反射）：合成段的 col 才是配色表里的字节，不被 Kr 0.8 洗淡。
        for material in [&ce, &highlight] {
            let ModelMaterial::Sgl(handle) = material else {
                unreachable!()
            };
            assert_eq!(
                bench.sgl.get(handle).unwrap().params.flags.z,
                1,
                "选中态直出颜色"
            );
        }
        // 半透明元素（类型色 alpha 200）：常态保留透明度，选中态归一成不透明——否则 CE 黄被稀释，
        // 而且材质会落进 Blend（半透明网格不进 prepass，选中件就没有边线与伪阴影）。
        let ce_type = mesh("CE", false);
        let translucent = bench.resolve(look, &ce_type, MaterialState::Base);
        let (_, opacity) = bench.sgl_colour(&translucent);
        assert!((opacity - 200.0 / 255.0).abs() < 1e-6, "常态保留半透明");
        let translucent_ce = bench.resolve(look, &ce_type, MaterialState::Ce);
        assert_eq!(
            bench.sgl_colour(&translucent_ce),
            ([255, 255, 0], 1.0),
            "选中态不继承元素的半透明"
        );
        let translucent_highlight = bench.resolve(look, &ce_type, MaterialState::Highlight);
        assert_eq!(
            bench.sgl_colour(&translucent_highlight),
            ([255, 255, 255], 1.0)
        );
        // X-Ray：半透明蓝、不打光。
        let xray = bench.resolve(look, &pipe, MaterialState::XRay);
        let ([r, g, b], opacity) = bench.sgl_colour(&xray);
        assert_eq!([r, g, b], [89, 169, 255]);
        assert!((opacity - 64.0 / 255.0).abs() < 1e-6);
        let ModelMaterial::Sgl(handle) = &xray else {
            unreachable!()
        };
        assert_eq!(bench.sgl.get(handle).unwrap().params.flags.z, 1);
        // X-Ray 叠选中：仍是半透明（穿透保留），但换成 CE 黄、alpha 抬到 160。
        let xray_selected = bench.resolve(look, &pipe, MaterialState::XraySelected);
        let ([r, g, b], opacity) = bench.sgl_colour(&xray_selected);
        assert_eq!([r, g, b], [255, 255, 0], "X-Ray 命中且选中 = CE 黄");
        assert!((opacity - 160.0 / 255.0).abs() < 1e-6, "比纯 X-Ray 实得多");
        assert_ne!(xray_selected, xray, "与纯 X-Ray 不是同一枚");
        let ModelMaterial::Sgl(handle) = &xray_selected else {
            unreachable!()
        };
        assert_eq!(bench.sgl.get(handle).unwrap().params.flags.z, 1);
        // 悬停预选：E3D 的 active orange，同样直出、不透明。
        let hover = bench.resolve(look, &pipe, MaterialState::Hover);
        assert_eq!(bench.sgl_colour(&hover), ([255, 190, 0], 1.0));
        let ModelMaterial::Sgl(handle) = &hover else {
            unreachable!()
        };
        assert_eq!(
            bench.sgl.get(handle).unwrap().params.flags.z,
            1,
            "悬停态直出"
        );

        let mut by_type = look;
        by_type.colour_scheme = ColourScheme::ByType;
        let typed_pipe = bench.resolve(by_type, &pipe, MaterialState::Base);
        assert_eq!(bench.sgl_colour(&typed_pipe), ([250, 250, 30], 1.0));
        let typed_equi = bench.resolve(by_type, &equi, MaterialState::Base);
        assert_ne!(typed_pipe, typed_equi);
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
        for material in [&pbr, &e3d, &ce] {
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
