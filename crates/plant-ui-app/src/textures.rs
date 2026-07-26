//! 运行时纹理装载。跟字体一样从 `assets/` 读文件、不打进二进制：换一张占位图
//! 不必重链，路径按 `CARGO_MANIFEST_DIR` 解析，所以不挑工作目录。

use anyhow::Context as _;
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};

fn texture_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures")
}

/// 三维视口的占位图（M1-5）。M3 接回 Bevy 之后这张换成渲染目标，
/// 绘制层拿到的仍是一个 `TextureId`，不用跟着改。
pub fn viewport_placeholder(ctx: &egui::Context) -> anyhow::Result<TextureHandle> {
    let path = texture_dir().join("viewport-placeholder.jpg");
    let bytes =
        std::fs::read(&path).with_context(|| format!("读不到占位纹理 {}", path.display()))?;
    // 按内容认编码而不是按扩展名：设计导出的那张就叫 .png、里头却是 JPEG。
    let rgba = image::load_from_memory(&bytes)
        .with_context(|| format!("占位纹理解不开 {}", path.display()))?
        .to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let image = ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    // LINEAR：这张图要按视口尺寸放大，NEAREST 会把管线边缘采成锯齿。
    Ok(ctx.load_texture("viewport-placeholder", image, TextureOptions::LINEAR))
}
