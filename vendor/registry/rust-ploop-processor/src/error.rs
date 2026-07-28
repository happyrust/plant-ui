use thiserror::Error;

/// PLOOP处理错误类型
#[derive(Error, Debug)]
pub enum PLoopError {
    #[error("解析错误: {0}")]
    ParseError(String),
    
    #[error("几何计算错误: {0}")]
    GeometryError(String),
    
    #[error("FRADIUS处理错误: {0}")]
    FradiusError(String),
    
    #[error("SVG生成错误: {0}")]
    SvgError(String),
    
    #[error("IO错误: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("JSON序列化错误: {0}")]
    JsonError(#[from] serde_json::Error),
    
    #[error("通用错误: {0}")]
    Other(String),
}

/// 结果类型别名
pub type Result<T> = std::result::Result<T, PLoopError>;
