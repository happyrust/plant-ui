//! # Rust PLOOP Processor
//!
//! 基于Python算法的Rust实现，用于处理PLOOP截面数据并生成SVG可视化
//!
//! ## 功能特性
//!
//! - PLOOP数据解析
//! - FRADIUS圆弧处理
//! - 顶点去重和共线处理
//! - SVG可视化生成
//!
//! ## 使用示例
//!
//! ```rust
//! use rust_ploop_processor::*;
//!
//! let processor = PLoopProcessor::new();
//! let content = std::fs::read_to_string("717.txt")?;
//! let ploops = processor.parse_file(&content)?;
//!
//! for ploop in ploops {
//!     let processed = processor.process_ploop(&ploop)?;
//!     let svg = processor.generate_svg(&ploop, &processed, "output.svg")?;
//! }
//! ```

pub mod vertex;
pub mod ploop;
pub mod processor;
pub mod fradius;
pub mod fradius_consecutive;
pub mod svg_generator_simple;
pub mod error;
pub mod parser;
pub mod json_exporter;
pub mod set2d;


pub use vertex::*;
pub use ploop::*;
pub use processor::*;
pub use fradius::*;
pub use fradius_consecutive::*;
pub use svg_generator_simple::*;
pub use error::*;
pub use parser::*;
pub use json_exporter::*;
pub use set2d::*;


/// 默认容差值（毫米）
pub const DEFAULT_TOLERANCE: f64 = 0.001;

/// 顶点去重容差（毫米）
pub const DEDUP_TOLERANCE: f64 = 1.0;

/// 共线检测容差
pub const COLLINEAR_TOLERANCE: f64 = 0.1;

/// 调试输出宏 - 只在debug feature启用时输出
#[macro_export]
macro_rules! debug_println {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug")]
        println!($($arg)*);
    };
}

/// 调试错误输出宏 - 只在debug feature启用时输出
#[macro_export]
macro_rules! debug_eprintln {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug")]
        eprintln!($($arg)*);
    };
}

/// 调试信息宏 - 只在debug feature启用时输出
#[macro_export]
macro_rules! debug_info {
    ($($arg:tt)*) => {
        #[cfg(feature = "debug")]
        println!("[DEBUG] {}", format!($($arg)*));
    };
}
