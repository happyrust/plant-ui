//! sglDx11 内嵌的环境立方体贴图 `gEnvTexture`（E3D 外观移植计划阶段一）。
//!
//! E3D 3.1 `sglDx11.dll` 的 .data 里嵌着一张 DDS（128²×6 面、B8G8R8A8、无 mip、灰度「摄影棚」环境），
//! 前向着色的反射项 `Kr · env(reflect(−V, N))` 采的就是它。六面按 DDS 顺序 +X −X +Y −Y +Z −Z 导成 PNG，
//! 从 plant3d-web `public/texture/e3d/sgl31-envcube/` 原样复制到本目录的 `sgl31-envcube/`，`include_bytes!`
//! 嵌进 crate：两端一致、不进资产目录、不走网络（共 27 KB）。
//!
//! 采样口径（与 sglDx11 `samLinear` + 无 mip 的 DDS 对齐）：双线性、不生 mip、字节直采不做 sRGB 解码
//!（`Rgba8Unorm`）。像素着色器把 Z-up 世界的反射向量换成 `(R.x, R.z, −R.y)` 再 `texCUBE`；plant-ui 的装载
//! 变换把 PDMS (x, y, z) 转成世界 (x, z, −y)，**恰好就是这个换法**，所以着色器里用世界空间的反射向量
//! 直接采样，不再旋转。wgpu 的立方体面序也是 +X −X +Y −Y +Z −Z，与 DDS / D3D 同源，六面原样上传。

use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssetUsages;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

/// 六面的 PNG 字节，顺序 = wgpu / DDS 立方体面序：+X −X +Y −Y +Z −Z。
const FACES: [&[u8]; 6] = [
    include_bytes!("sgl31-envcube/px.png"),
    include_bytes!("sgl31-envcube/nx.png"),
    include_bytes!("sgl31-envcube/py.png"),
    include_bytes!("sgl31-envcube/ny.png"),
    include_bytes!("sgl31-envcube/pz.png"),
    include_bytes!("sgl31-envcube/nz.png"),
];

/// 解好码、装好的环境立方体贴图句柄。`setup` 里建一次，所有 `SglLookMaterial` 共用；
/// `None` = 解码失败，材质退到天地两段灰度兜底。
#[derive(Resource, Clone)]
pub struct SglEnvCube(pub Option<Handle<Image>>);

/// 把六张 PNG 解成一张 6 层的立方体纹理。解码失败或六面尺寸不一致时返回错误，调用方退到
/// 解析天地兜底（材质的 `use_env = 0`）——材质不该因为一张贴图坐没。
pub fn build_env_cube_image() -> Result<Image, String> {
    let mut faces = Vec::with_capacity(6);
    for (index, bytes) in FACES.iter().enumerate() {
        let decoded = Image::from_buffer(
            bytes,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            // 字节直采：不让 GPU 做 sRGB 解码，同 sglDx11 的 UNORM 采样。
            false,
            ImageSampler::linear(),
            RenderAssetUsages::MAIN_WORLD,
        )
        .map_err(|error| format!("环境贴图第 {index} 面解码失败：{error}"))?;
        // Bevy 的 `from_dynamic` 把灰度 / RGB 的 8-bit PNG 都展成 RGBA8（`is_srgb = false` → `Rgba8Unorm`），
        // 所以这里只核不转：16-bit PNG 会成 `Rgba16Unorm`，那不是这张贴图该有的样子。
        let format = decoded.texture_descriptor.format;
        if format != TextureFormat::Rgba8Unorm {
            return Err(format!(
                "环境贴图第 {index} 面解成了 {format:?}，要的是 Rgba8Unorm"
            ));
        }
        faces.push(decoded);
    }
    let size = faces[0].width();
    if size == 0 || faces[0].height() != size {
        return Err(format!(
            "环境贴图面不是正方形：{}×{}",
            faces[0].width(),
            faces[0].height()
        ));
    }
    let mut stacked = Vec::with_capacity(size as usize * size as usize * 4 * 6);
    for (index, face) in faces.iter().enumerate() {
        if face.width() != size || face.height() != size {
            return Err(format!(
                "环境贴图第 {index} 面尺寸 {}×{} 与第 0 面 {size}×{size} 不一致",
                face.width(),
                face.height()
            ));
        }
        let data = face
            .data
            .as_ref()
            .ok_or_else(|| format!("环境贴图第 {index} 面没有像素数据"))?;
        stacked.extend_from_slice(data);
    }
    let mut image = Image::new(
        Extent3d {
            width: size,
            height: size * 6,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        stacked,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.reinterpret_stacked_2d_as_array(6);
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image.sampler = ImageSampler::linear();
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_six_faces_are_embedded_and_stack_into_a_cube() {
        for face in FACES {
            assert!(face.len() > 1000, "每一面都得是真正的 PNG，不是占位");
            assert_eq!(&face[..8], b"\x89PNG\r\n\x1a\n");
        }
        let image = build_env_cube_image().expect("六面 PNG 能解码成立方体");
        let size = image.texture_descriptor.size;
        assert_eq!(size.depth_or_array_layers, 6);
        assert_eq!(size.width, size.height);
        assert_eq!(size.width, 128, "sglDx11 内嵌的 DDS 是 128²");
        assert_eq!(image.texture_descriptor.format, TextureFormat::Rgba8Unorm);
        assert_eq!(
            image
                .texture_view_descriptor
                .as_ref()
                .and_then(|view| view.dimension),
            Some(TextureViewDimension::Cube)
        );
    }
}
