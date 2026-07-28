//! plant-ui 共用字体注册。

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

pub const MEDIUM: &str = "puhui-medium";
pub const SEMIBOLD: &str = "puhui-semibold";

const REGULAR_KEY: &str = "puhui-regular";

#[cfg(not(target_arch = "wasm32"))]
fn font_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/public/assets/fonts")
}

#[cfg(not(target_arch = "wasm32"))]
fn load(file: &str, warnings: &mut Vec<String>) -> Option<Arc<FontData>> {
    let path = font_dir().join(file);
    match std::fs::read(&path) {
        Ok(bytes) => Some(Arc::new(FontData::from_owned(bytes))),
        Err(err) => {
            warnings.push(format!(
                "字体缺失，回退到 egui 默认字体：{} ({err})",
                path.display()
            ));
            None
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn load(_file: &str, _warnings: &mut Vec<String>) -> Option<Arc<FontData>> {
    Some(Arc::new(FontData::from_static(include_bytes!(
        "../../../web/public/assets/fonts/AlibabaPuHuiTi-2-55-Regular.ttf"
    ))))
}

pub fn definitions() -> (FontDefinitions, Vec<String>) {
    let mut fonts = FontDefinitions::default();
    let mut warnings = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    let faces = [
        (REGULAR_KEY, "AlibabaPuHuiTi-2-55-Regular.ttf"),
        (MEDIUM, "AlibabaPuHuiTi-2-65-Medium.ttf"),
        (SEMIBOLD, "AlibabaPuHuiTi-2-75-SemiBold.ttf"),
    ];
    // ponytail: WASM 共用常规字重；需要严格视觉字重时再嵌入另外两份约 16 MB 字体。
    #[cfg(target_arch = "wasm32")]
    let faces = [(REGULAR_KEY, "AlibabaPuHuiTi-2-55-Regular.ttf")];

    let mut loaded = Vec::new();
    for (key, file) in faces {
        if let Some(data) = load(file, &mut warnings) {
            fonts.font_data.insert(key.to_owned(), data);
            loaded.push(key);
        }
    }

    if loaded.contains(&REGULAR_KEY) {
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, REGULAR_KEY.to_owned());
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push(REGULAR_KEY.to_owned());
    }

    for key in [MEDIUM, SEMIBOLD] {
        if !loaded.contains(&key) {
            continue;
        }
        let mut chain = vec![key.to_owned()];
        if loaded.contains(&REGULAR_KEY) {
            chain.push(REGULAR_KEY.to_owned());
        }
        chain.extend(
            fonts
                .families
                .get(&FontFamily::Proportional)
                .cloned()
                .unwrap_or_default(),
        );
        fonts
            .families
            .insert(FontFamily::Name(key.into()), dedup(chain));
    }

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    (fonts, warnings)
}

fn dedup(chain: Vec<String>) -> Vec<String> {
    let mut seen = Vec::with_capacity(chain.len());
    for item in chain {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    seen
}
