//! 右上角的视图立方体（ViewCube）：方位指示 + 一键切视角。
//!
//! 形制按 Autodesk ViewCube 规范裁剪（拷问定案第 4 题）：26 个命中区
//! （6 面 + 12 棱 + 8 角）+ 悬停高亮 + Home 键，点击 0.3s 插值到目标视角；
//! 拖拽立方体旋转不做——右键 orbit 已经覆盖那件事。非激活态 60% 透明度，
//! 无容器直接悬浮（不像 HUD 那样带卡片底），少遮一块模型。
//!
//! 立方体不进宿主场景。它是一枚正交投影的小部件，画面只由相机姿态决定，
//! 而姿态已经躺在 [`View3dVm::camera_rot`] 里——在这儿用二十行向量投影画出来，
//! 命中判定顺便就是屏幕上的凸四边形包含测试，省掉宿主侧第二相机、独立
//! RenderLayer 与射线拾取一整套。视觉与交互对着 `design/plant-ui.pen` 的
//! S1-B / S1-C 稿验收。
//!
//! 坐标语义（拷问定案第 3 题）：X/Y/Z 体系、Z 上。立方体的面语义在 **PDMS 系**
//! 里表达（前 = −Y、右 = +X、顶 = +Z……），发命令前经 [`pdms_to_world`] 换算成
//! 宿主世界系——场景装载时那次「Z-up → Y-up」旋转只有这里需要知道。

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as ph;

use crate::Cmd;
use crate::style::theme_tokens::Font;
use crate::style::tokens::{Density, Tokens};
use crate::vm::View3dVm;

/// 部件占位与视口边距，取 S1-B 稿的「88×88 · inset 14」。
const SIZE: f32 = 88.0;
const INSET: f32 = 14.0;
/// 非激活态整体透明度（S1-C 规格行）。
const IDLE_ALPHA: f32 = 0.6;
/// 单位立方体半对角线 √3 ≈ 1.732，投影比例按它把立方体装进 88 框再留一圈呼吸。
const UNIT_TO_PX: f32 = SIZE / 2.0 * 0.52;

/// 面法向偏到这个余弦以下就当侧棱看：不再画面标，字会被压扁到读不出来。
const LABEL_MIN_FACING: f32 = 0.4;
/// 低于这个余弦的面完全背对相机，连面片都不画。
const FACE_MIN_FACING: f32 = 0.08;

/// PDMS 系 → 宿主世界系：装载场景时绕 X 转了 -90°（Z-up 变 Y-up）。
/// (x, y, z) ↦ (x, z, −y)。换算集中在这一个函数，宿主与测试都对着它。
pub fn pdms_to_world(v: [f32; 3]) -> [f32; 3] {
    [v[0], v[2], -v[1]]
}

/// 六个面：PDMS 法向与中文单字面标（拷问定案第 4 题，全中文 UI 配单字最清晰）。
const FACES: [([i8; 3], &str); 6] = [
    ([1, 0, 0], "右"),
    ([-1, 0, 0], "左"),
    ([0, 1, 0], "后"),
    ([0, -1, 0], "前"),
    ([0, 0, 1], "顶"),
    ([0, 0, -1], "底"),
];

/// 命中区。键是「参与的面法向之和」：面自己是一重，棱是两面之和，角是三面之和
/// ——同一条棱从两个面上的贴边格子算出来的键相同，去重天然成立，全集恰好 26。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region(pub [i8; 3]);

impl Region {
    /// 这块区对应的目标视角（PDMS 系）：相机看向法向和的反方向。
    /// 上方向默认 Z 上；正对顶 / 底时 Z 与视线共线，退到 Y（北）上。
    pub fn target_pdms(self) -> ([f32; 3], [f32; 3]) {
        let v = [self.0[0] as f32, self.0[1] as f32, self.0[2] as f32];
        let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        let forward = [-v[0] / len, -v[1] / len, -v[2] / len];
        let up = if forward[2].abs() > 0.99 {
            [0.0, 1.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        (forward, up)
    }

    /// 面 / 棱 / 角：参与的面数。测试拿它清点 6 / 12 / 8。
    pub fn order(self) -> u8 {
        self.0.iter().map(|c| c.unsigned_abs()).sum()
    }
}

/// Home 键的目标：等距初始视角（前-右-顶那只角），配合 fit 拉回全景。
/// 与 [`Region::target_pdms`] 同一条 up 规则。
pub fn home_target_pdms() -> ([f32; 3], [f32; 3]) {
    Region([1, -1, 1]).target_pdms()
}

/// 画立方体并处理交互，返回占掉的矩形（调用点拿去把拾取与相机手势挡在外面）。
pub fn show(
    ui: &mut Ui,
    t: &Tokens,
    d: Density,
    vm: &View3dVm,
    view: Rect,
    viewport: Id,
    cmds: &mut Vec<Cmd>,
) -> Rect {
    let size = d.px(SIZE);
    let rect = Rect::from_min_size(
        pos2(view.right() - d.px(INSET) - size, view.top() + d.px(INSET)),
        egui::Vec2::splat(size),
    );
    if !view.contains_rect(rect) {
        return Rect::NOTHING;
    }

    let response = ui.interact(rect, viewport.with("viewcube"), Sense::click());
    let active = response.hovered();
    let alpha = if active { 1.0 } else { IDLE_ALPHA };
    let center = rect.center();
    let scale = d.px(UNIT_TO_PX);

    // 相机姿态列向量。独立壳给的是恒等旋转，立方体静态正对「前」。
    let right = vm.camera_rot[0];
    let up = vm.camera_rot[1];
    let back = vm.camera_rot[2];
    let project = |p_pdms: [f32; 3]| -> Pos2 {
        let w = pdms_to_world(p_pdms);
        let x = dot(right, w);
        let y = dot(up, w);
        center + vec2(x, -y) * scale
    };

    let hovered_region = response
        .hover_pos()
        .and_then(|pointer| region_at(pointer, &project, back));

    // 先面片后浮层：可见面从背到前排序无必要——正交投影下可见面互不遮挡。
    for (normal, label) in FACES {
        let n = normal.map(|c| c as f32);
        let facing = dot(back, pdms_to_world(n));
        if facing < FACE_MIN_FACING {
            continue;
        }
        let (u_axis, v_axis) = face_basis(normal);
        let corner = |du: f32, dv: f32| {
            project([
                n[0] + u_axis[0] as f32 * du + v_axis[0] as f32 * dv,
                n[1] + u_axis[1] as f32 * du + v_axis[1] as f32 * dv,
                n[2] + u_axis[2] as f32 * du + v_axis[2] as f32 * dv,
            ])
        };
        let outline = vec![
            corner(-1.0, -1.0),
            corner(1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, 1.0),
        ];
        ui.painter().add(egui::Shape::convex_polygon(
            outline.clone(),
            face_fill(t, normal, facing).gamma_multiply(alpha),
            Stroke::NONE,
        ));
        // 悬停区盖在所属的每一个面上：棱与角横跨多面，只亮一半会看歪。
        if let Some(region) = hovered_region {
            for (i, j) in cells_of(normal, region) {
                let cell = cell_polygon(normal, i, j, &corner);
                ui.painter().add(egui::Shape::convex_polygon(
                    cell,
                    t.accent.gamma_multiply(0.45 * alpha),
                    Stroke::NONE,
                ));
            }
        }
        ui.painter().add(egui::Shape::closed_line(
            outline,
            Stroke::new(1.0, t.border.gamma_multiply(alpha)),
        ));
        if facing > LABEL_MIN_FACING {
            ui.painter().text(
                project(n),
                Align2::CENTER_CENTER,
                label,
                Font::body(d),
                t.text_secondary.gamma_multiply(alpha),
            );
        }
    }

    // Home 键：常驻立方体左上角（S1-C 规格），命中区归它、不归立方体。
    let home = Rect::from_min_size(rect.min, egui::Vec2::splat(d.px(16.0)));
    let home_resp = ui.interact(home, viewport.with("viewcube-home"), Sense::click());
    ui.painter().text(
        home.center(),
        Align2::CENTER_CENTER,
        ph::HOUSE,
        Font::body(d),
        if home_resp.hovered() {
            t.accent
        } else {
            t.text_secondary.gamma_multiply(alpha)
        },
    );
    if home_resp.clicked() {
        ui.memory_mut(|m| m.request_focus(viewport));
        let (forward, up) = home_target_pdms();
        cmds.push(Cmd::SnapView {
            forward: pdms_to_world(forward),
            up: pdms_to_world(up),
            fit: true,
        });
    } else if response.clicked()
        && let Some(pointer) = response.interact_pointer_pos()
        && !home.contains(pointer)
        && let Some(region) = region_at(pointer, &project, back)
    {
        ui.memory_mut(|m| m.request_focus(viewport));
        let (forward, up) = region.target_pdms();
        cmds.push(Cmd::SnapView {
            forward: pdms_to_world(forward),
            up: pdms_to_world(up),
            fit: false,
        });
    }
    rect
}

/// X / Y / Z 轴标签色。轴线本体在宿主里也是这三种颜色，两处必须一致，
/// 深浅主题各配一档明度（S1-B 深浅两稿的取值）。
pub fn axis_color(t: &Tokens, axis: usize) -> Color32 {
    let dark = [(0xD9, 0x54, 0x4D), (0x4C, 0xAF, 0x6E), (0x4B, 0x7B, 0xE5)];
    let light = [(0xC6, 0x40, 0x3A), (0x2F, 0x8F, 0x55), (0x37, 0x63, 0xC8)];
    let (r, g, b) = if t.dark { dark[axis] } else { light[axis] };
    Color32::from_rgb(r, g, b)
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// 面内的两条参数轴。具体取哪两条不影响语义（格子按端值分类），
/// 只要求与法向两两正交、六个面各自稳定。
fn face_basis(normal: [i8; 3]) -> ([i8; 3], [i8; 3]) {
    if normal[0] != 0 {
        ([0, 1, 0], [0, 0, 1])
    } else if normal[1] != 0 {
        ([1, 0, 0], [0, 0, 1])
    } else {
        ([1, 0, 0], [0, 1, 0])
    }
}

/// 3×3 分格的三个断点；两端 1/3 归棱角、中间归面，是 Autodesk 立方体的经典分法。
const BREAKS: [f32; 4] = [-1.0, -1.0 / 3.0, 1.0 / 3.0, 1.0];

/// 第 (i, j) 格所属的命中区：贴哪条边就把哪条边的面法向加进键里。
fn cell_region(normal: [i8; 3], i: usize, j: usize) -> Region {
    let (u_axis, v_axis) = face_basis(normal);
    let mut key = normal;
    let du: i8 = match i {
        0 => -1,
        2 => 1,
        _ => 0,
    };
    let dv: i8 = match j {
        0 => -1,
        2 => 1,
        _ => 0,
    };
    for axis in 0..3 {
        key[axis] += u_axis[axis] * du + v_axis[axis] * dv;
    }
    Region(key)
}

/// 一个面上属于给定命中区的格子（用于悬停高亮：面 1 格、棱 3 格、角 1 格）。
fn cells_of(normal: [i8; 3], region: Region) -> Vec<(usize, usize)> {
    let mut cells = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            if cell_region(normal, i, j) == region {
                cells.push((i, j));
            }
        }
    }
    cells
}

fn cell_polygon(
    normal: [i8; 3],
    i: usize,
    j: usize,
    corner: &impl Fn(f32, f32) -> Pos2,
) -> Vec<Pos2> {
    let _ = normal;
    vec![
        corner(BREAKS[i], BREAKS[j]),
        corner(BREAKS[i + 1], BREAKS[j]),
        corner(BREAKS[i + 1], BREAKS[j + 1]),
        corner(BREAKS[i], BREAKS[j + 1]),
    ]
}

/// 指针落在哪个命中区：逐个可见面做格子的凸四边形包含测试。
/// 正交投影下可见面在屏幕上互不重叠，第一个命中就是答案。
fn region_at(pointer: Pos2, project: &impl Fn([f32; 3]) -> Pos2, back: [f32; 3]) -> Option<Region> {
    for (normal, _) in FACES {
        let n = normal.map(|c| c as f32);
        if dot(back, pdms_to_world(n)) < FACE_MIN_FACING {
            continue;
        }
        let (u_axis, v_axis) = face_basis(normal);
        let corner = |du: f32, dv: f32| {
            project([
                n[0] + u_axis[0] as f32 * du + v_axis[0] as f32 * dv,
                n[1] + u_axis[1] as f32 * du + v_axis[1] as f32 * dv,
                n[2] + u_axis[2] as f32 * du + v_axis[2] as f32 * dv,
            ])
        };
        for i in 0..3 {
            for j in 0..3 {
                if point_in_quad(pointer, &cell_polygon(normal, i, j, &corner)) {
                    return Some(cell_region(normal, i, j));
                }
            }
        }
    }
    None
}

/// 凸四边形包含测试：叉积同号。格子可能因投影翻了绕向，两个方向都认。
fn point_in_quad(p: Pos2, quad: &[Pos2]) -> bool {
    let mut sign = 0.0_f32;
    for k in 0..4 {
        let a = quad[k];
        let b = quad[(k + 1) % 4];
        let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        if cross.abs() < f32::EPSILON {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if sign != cross.signum() {
            return false;
        }
    }
    true
}

/// 面片底色：按「朝上多亮一点、朝向相机多亮一点」调制，深浅主题各一套基色，
/// 对齐 S1-B 稿上 顶 > 前 > 右 的三档明度关系。
fn face_fill(t: &Tokens, normal: [i8; 3], facing: f32) -> Color32 {
    let n_world = pdms_to_world([normal[0] as f32, normal[1] as f32, normal[2] as f32]);
    let up_lift = n_world[1].max(0.0);
    let (base, ambient, up_gain, cam_gain) = if t.dark {
        (Color32::from_rgb(0x33, 0x3F, 0x4B), 0.82, 0.32, 0.18)
    } else {
        (Color32::from_rgb(0xD8, 0xE2, 0xEC), 0.93, 0.09, 0.05)
    };
    let f = ambient + up_gain * up_lift + cam_gain * facing;
    let ch = |c: u8| ((c as f32 * f).round().clamp(0.0, 255.0)) as u8;
    Color32::from_rgb(ch(base.r()), ch(base.g()), ch(base.b()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// 26 区一个不多一个不少：6 面 + 12 棱 + 8 角。分格映射错了这里第一时间响。
    #[test]
    fn the_cube_partitions_into_exactly_26_regions() {
        let mut regions = HashSet::new();
        for (normal, _) in FACES {
            for i in 0..3 {
                for j in 0..3 {
                    regions.insert(cell_region(normal, i, j));
                }
            }
        }
        assert_eq!(regions.len(), 26);
        let count = |order: u8| regions.iter().filter(|r| r.order() == order).count();
        assert_eq!((count(1), count(2), count(3)), (6, 12, 8));
    }

    /// 点「前」面 = 相机看向 +Y（PDMS），换算到世界系是 -Z；Z 上不变。
    /// 这两条向量喂给宿主就是终态相机，算错方向立方体会把人带反。
    #[test]
    fn clicking_the_front_face_aims_north_with_z_up() {
        let (forward, up) = Region([0, -1, 0]).target_pdms();
        assert_eq!(pdms_to_world(forward), [0.0, 0.0, -1.0]);
        assert_eq!(pdms_to_world(up), [0.0, 1.0, 0.0]);
    }

    /// 顶视图的视线与 Z 共线，up 必须退到 Y（北），不然相机姿态无解。
    #[test]
    fn top_view_falls_back_to_north_up() {
        let (forward, up) = Region([0, 0, 1]).target_pdms();
        assert_eq!(forward, [0.0, 0.0, -1.0]);
        assert_eq!(up, [0.0, 1.0, 0.0]);
        assert_eq!(pdms_to_world(up), [0.0, 0.0, -1.0]);
    }

    /// 角区是三面法向之和，方向在三根轴上等权。
    #[test]
    fn corner_regions_aim_along_the_diagonal() {
        let (forward, _) = Region([1, -1, 1]).target_pdms();
        let expect = 1.0 / 3.0_f32.sqrt();
        assert!((forward[0] + expect).abs() < 1e-6);
        assert!((forward[1] - expect).abs() < 1e-6);
        assert!((forward[2] + expect).abs() < 1e-6);
    }

    /// PDMS → 世界系那次 -90° 旋转：X 不动、Z 变 Y、Y 变 -Z。
    /// 场景装载与这里必须是同一套换算，错半个符号立方体就与模型对不上。
    #[test]
    fn pdms_axes_map_to_the_rotated_world() {
        assert_eq!(pdms_to_world([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        assert_eq!(pdms_to_world([0.0, 1.0, 0.0]), [0.0, 0.0, -1.0]);
        assert_eq!(pdms_to_world([0.0, 0.0, 1.0]), [0.0, 1.0, 0.0]);
    }

    /// 恒等姿态（独立壳）只有「前」面正对相机——世界 back = +Z 正是 PDMS 的 -Y。
    #[test]
    fn identity_camera_sees_only_the_front_face() {
        let back = [0.0, 0.0, 1.0];
        let facing: Vec<&str> = FACES
            .iter()
            .filter(|(n, _)| {
                dot(back, pdms_to_world([n[0] as f32, n[1] as f32, n[2] as f32])) > FACE_MIN_FACING
            })
            .map(|(_, label)| *label)
            .collect();
        assert_eq!(facing, ["前"]);
    }
}
