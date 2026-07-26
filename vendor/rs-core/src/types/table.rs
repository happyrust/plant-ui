use itertools::Itertools;
use crate::RefU64;

pub trait ToTable{

    // fn to_table(&self) -> String;

    fn to_table_key(&self, tbl: &str) -> String;
}

impl ToTable for RefU64{
    fn to_table_key(&self, tbl: &str) -> String {
        self.to_table_key(tbl)
    }
}

impl ToTable for &[RefU64]{
    fn to_table_key(&self, tbl: &str) -> String {
        self.iter().map(|x| x.to_table_key(tbl)).join(",")
    }
}