use serde::{Serialize, Deserialize};
use clap::Parser;

#[derive(Debug, Default, Clone, Parser, Serialize, Deserialize)]
pub struct DataCenterOptions {
    // 电气支吊架型钢类型
    #[clap(long)]
    pub dq_support_sctn_types: Vec<String>,
}

