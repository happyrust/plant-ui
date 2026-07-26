use serde::{Serialize, Deserialize};
use crate::cache::mgr::BytesTrait;
use crate::types::*;
use crate::types::NounHash;
use crate::tool::db_tool::db1_hash;
use crate::helper::table::restore_type_name;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CachedRefBasic {
    pub owner: RefU64,
    pub table: String, //提前处理好成了table name，有关键字冲突的地方，删除最后的
}

impl BytesTrait for CachedRefBasic {
}

impl CachedRefBasic{

    #[inline]
    pub fn get_type(&self) -> &str{
        restore_type_name(&self.table)
    }

    #[inline]
    pub fn get_table_name(&self) -> &str{
        self.table.as_str()
    }

    #[inline]
    pub fn get_noun_hash(&self) -> NounHash{
        db1_hash(self.get_type()).into()
    }

    #[inline]
    pub fn get_owner(&self) -> RefU64{
        self.owner
    }

}