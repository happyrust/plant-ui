use chrono::{DateTime, Local};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};
use serde_with::serde_as;

#[serde_as]
#[derive(Serialize, Deserialize, Clone, Debug, Default, DeriveEntityModel)]
#[sea_orm(table_name = "dolt_diff")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub commit_hash: String,
    pub table_name: String,
    pub committer: Option<String>,
    pub email: Option<String>,
    pub date: Option<DateTime<Local>>,
    pub message: Option<String>,
    pub data_change: i32,
    pub schema_change: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

//todo 需要解析查询出的差异数据
