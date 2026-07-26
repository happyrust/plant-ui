use crate::pdms_types::AttrInfo;
use crate::tool::db_tool::{db1_dehash, db1_hash};
use crate::types::attmap::AttrMap;
use crate::types::named_attmap::NamedAttrMap;
use dashmap::DashMap;
use glam::i32;
#[cfg(feature = "sea-orm")]
use sea_query::*;
use serde_derive::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct PdmsDatabaseInfo {
    // 第一个i32是type_hash ，第二个i32是属性的hash
    pub noun_attr_info_map: DashMap<i32, DashMap<i32, AttrInfo>>,
    pub named_attr_info_map: DashMap<String, DashMap<String, AttrInfo>>,
}

const BASIC_TYPE_NAMES: [&'static str; 3] = ["REFNO", "OWNER", "TYPEX"];

impl PdmsDatabaseInfo {
    ///获得所有的explicit nouns
    pub fn get_all_explicit_nouns(&self) -> DashMap<i32, AttrInfo> {
        let mut map = DashMap::default();
        for kv in &self.noun_attr_info_map {
            if *kv.key() == 0 {
                continue;
            }
            for info in kv.value() {
                if info.offset == 0 && !map.contains_key(info.key()) {
                    map.insert(*info.key(), info.value().clone());
                }
            }
        }
        map
    }

    pub fn fill_named_map(&mut self) {
        for kv in &self.noun_attr_info_map {
            let noun = *kv.key();
            if noun == 0 {
                continue;
            }
            let mut map = DashMap::default();
            for info in kv.value() {
                map.insert(db1_dehash(*info.key() as _), info.value().clone());
            }
            self.named_attr_info_map.insert(db1_dehash(noun as _), map);
        }
    }

    pub fn fix(&self) {
        for kv in self.noun_attr_info_map.iter_mut() {
            for mut info in kv.value().iter_mut() {
                info.name = db1_dehash(*info.key() as _);
            }
        }
    }

    ///生成所有db info里的table
    #[cfg(feature = "sea-orm")]
    pub fn gen_all_create_table_sql(&self) -> Vec<String> {
        let mut sqls = vec![];
        for noun_att_info in &self.noun_attr_info_map {
            // 遍历数据库中的名词属性信息
            let type_name = db1_dehash(*noun_att_info.key() as _); // 获取属性类型名
            if type_name.is_empty() {
                continue;
            }
            if let Some(sql) = self.gen_create_table_sql(&type_name) {
                sqls.push(sql);
            }
        }
        sqls
    }

    ///生成创建table的语句
    #[cfg(feature = "sea-orm")]
    pub fn gen_create_table_sql(&self, type_name: &str) -> Option<String> {
        let mut table_create_statement = Table::create()
            .table(Alias::new(type_name))
            .if_not_exists()
            .to_owned();
        let hash = db1_hash(type_name) as i32;
        let info = self.noun_attr_info_map.get(&hash)?;
        let bmap: BTreeMap<String, AttrInfo> =
            info.iter().map(|x| (x.name.clone(), x.clone())).collect(); // 创建BTreeMap用于存储属性信息

        // pub REFNO: RefU64,
        // pub NAME: String,
        // pub OWNER: RefU64,
        // pub TYPE: String,
        // pub TYPEX: String,
        table_create_statement.col(
            &mut ColumnDef::new(Alias::new("REFNO"))
                .string()
                .primary_key()
                .not_null(),
        );
        table_create_statement.col(&mut ColumnDef::new(Alias::new("NAME")).string());
        table_create_statement.col(&mut ColumnDef::new(Alias::new("OWNER")).string());
        table_create_statement.col(&mut ColumnDef::new(Alias::new("TYPE")).not_null().string());
        //应该是扩展类型？，先暂时忽略
        table_create_statement.col(&mut ColumnDef::new(Alias::new("TYPEX")).string());
        for kv in bmap.values() {
            let att_name = db1_dehash(kv.hash as _);
            if att_name == "NAME"
                || att_name == "TYPE"
                || att_name == "TYPEX"
                || att_name == "OWNER"
                || att_name.contains(":")
                || att_name.contains("@")
            {
                // 如果属性名是"NAME"或"TYPE"，则跳过
                continue;
            }
            let mut column_def = ColumnDef::new(Alias::new(att_name));
            if kv.offset == 0 {
                column_def.not_null();
            }
            match &kv.default_val {
                AttrVal::IntegerType(_) => column_def.integer(),
                //不需要存储double这么高精度
                AttrVal::DoubleType(_) => column_def.float(),
                AttrVal::BoolType(_) => column_def.boolean(),
                AttrVal::StringType(_)
                | AttrVal::WordType(_)
                | AttrVal::ElementType(_)
                | AttrVal::RefU64Type(_) => column_def.string(),
                _ => column_def.json(),
            };
            table_create_statement.col(&mut column_def);
        }

        let query_string = table_create_statement.to_string(MysqlQueryBuilder);
        Some(query_string)
    }

    ///检查当前的att_map，是否需要更新schema数据
    pub fn check_schema(&self, noun: i32, att_map: &NamedAttrMap) -> Option<String> {
        // let mut found_diff = false;
        // // let mut new_atts_info = None;
        // let atts_info = self.noun_attr_info_map.get_mut(&noun)?;
        // // dbg!(atts_info.value());
        // let type_hash = db1_hash("TYPE") as _;
        // atts_info.insert(
        //     type_hash,
        //     AttrInfo {
        //         name: "TYPE".to_string(),
        //         hash: type_hash,
        //         offset: 0,
        //         default_val: AttrVal::WordType("unset".to_string()),
        //         att_type: DbAttributeType::WORD,
        //     },
        // );
        // {
        //     att_map.iter().for_each(|(k, v)| {
        //         let hash = db1_hash(k) as _;
        //         if hash > 0
        //             && !BASIC_TYPE_NAMES.contains(&k.as_str())
        //             && !atts_info.contains_key(&(hash as _))
        //         {
        //             found_diff = true;
        //             match v {
        //                 NamedAttrValue::F32VecType(_) => {
        //                     atts_info.insert(
        //                         hash,
        //                         AttrInfo {
        //                             name: k.clone(),
        //                             hash,
        //                             offset: 0,
        //                             default_val: AttrVal::DoubleArrayType(vec![]),
        //                             att_type: DbAttributeType::DOUBLE,
        //                         },
        //                     );
        //                 }
        //                 NamedAttrValue::IntArrayType(_) => {
        //                     atts_info.insert(
        //                         hash,
        //                         AttrInfo {
        //                             name: k.clone(),
        //                             hash,
        //                             offset: 0,
        //                             default_val: AttrVal::IntArrayType(vec![]),
        //                             att_type: DbAttributeType::INTEGER,
        //                         },
        //                     );
        //                 }
        //                 NamedAttrValue::StringArrayType(_) => {
        //                     atts_info.insert(
        //                         hash,
        //                         AttrInfo {
        //                             name: k.clone(),
        //                             hash,
        //                             offset: 0,
        //                             default_val: AttrVal::StringArrayType(vec![]),
        //                             att_type: DbAttributeType::STRING,
        //                         },
        //                     );
        //                 }
        //                 NamedAttrValue::F32Type(_) => {
        //                     atts_info.insert(
        //                         hash,
        //                         AttrInfo {
        //                             name: k.clone(),
        //                             hash,
        //                             offset: 0,
        //                             default_val: AttrVal::DoubleType(0.0),
        //                             att_type: DbAttributeType::DOUBLE,
        //                         },
        //                     );
        //                 }
        //                 NamedAttrValue::BoolType(_) => {
        //                     atts_info.insert(
        //                         hash,
        //                         AttrInfo {
        //                             name: k.clone(),
        //                             hash,
        //                             offset: 0,
        //                             default_val: AttrVal::BoolType(false),
        //                             att_type: DbAttributeType::BOOL,
        //                         },
        //                     );
        //                 }
        //                 NamedAttrValue::IntegerType(_) => {
        //                     atts_info.insert(
        //                         hash,
        //                         AttrInfo {
        //                             name: k.clone(),
        //                             hash,
        //                             offset: 0,
        //                             default_val: AttrVal::IntegerType(0),
        //                             att_type: DbAttributeType::INTEGER,
        //                         },
        //                     );
        //                 }
        //                 NamedAttrValue::StringType(_)
        //                 | NamedAttrValue::ElementType(_)
        //                 | NamedAttrValue::WordType(_)
        //                 | NamedAttrValue::RefU64Type(_) => {
        //                     atts_info.insert(
        //                         hash,
        //                         AttrInfo {
        //                             name: k.clone(),
        //                             hash,
        //                             offset: 0,
        //                             default_val: AttrVal::StringType("".into()),
        //                             att_type: DbAttributeType::STRING,
        //                         },
        //                     );
        //                 }
        //                 _ => {}
        //             }
        //             dbg!((k, v));
        //         }
        //     });
        // }

        // // let Some(new) = new_atts_info else{
        // //     return None;
        // // };
        // if !found_diff {
        //     return None;
        // }

        // use serde_json::{Map, Value};
        // let mut context: Map<String, Value> = serde_json::from_str(
        //     r#"{
        //     "@base": "terminusdb:///data/",
        //     "@schema": "terminusdb:///schema#",
        //     "@type": "@context"
        // }"#,
        // )
        // .unwrap_or_default();

        // //todo 需要考虑UDA的schema
        // let schema = Self::gen_schema(noun, &atts_info)?;
        // let mut modify = vec![context, schema];

        // // self.noun_attr_info_map.remove(&noun);
        // // self.noun_attr_info_map.insert(noun, new);
        // // *self.noun_attr_info_map.get_mut(&noun).unwrap() = new;
        // serde_json::to_string(&modify).ok()
        None
    }

    pub fn gen_schema(
        noun: i32,
        info_map: &DashMap<i32, AttrInfo>,
    ) -> Option<serde_json::Map<String, serde_json::Value>> {
        if noun <= 0 {
            return None;
        }
        use serde_json::{Map, Value};
        // dbg!(*k);
        let type_name = db1_dehash(noun as _);
        if type_name.is_empty() {
            dbg!(noun);
            dbg!(&type_name);
            return None;
        }
        let mut att_schemas_vec = vec![];
        for info in info_map {
            //需要执行for 循环，来生成 schema
            let Some(s) = info.gen_schema() else {
                continue;
            };
            att_schemas_vec.push(s);
        }
        let mut json = if att_schemas_vec.is_empty() {
            format!(
                r#"
                    {{
                        "@type" : "Class",
                        "@id"   : "{}",
                        "@key"  : {{ "@type": "Lexical", "@fields": ["REFNO"] }},
                        "REFNO"    : "xsd:string",
                        "ELEMENT" : "PdmsElement",
                        "TYPEX"    : {{
                            "@class": "xsd:string",
                            "@type": "Optional"
                        }},
                        "OWNER"    : {{
                            "@class": "xsd:string",
                            "@type": "Optional"
                        }}
                    }}
                "#,
                &type_name
            )
        } else {
            format!(
                r#"
                    {{
                        "@type" : "Class",
                        "@id"   : "{}",
                        "@key"  : {{ "@type": "Lexical", "@fields": ["REFNO"] }},
                        "REFNO"    : "xsd:string",
                        "ELEMENT" : "PdmsElement",
                        "TYPEX"    : {{
                            "@class": "xsd:string",
                            "@type": "Optional"
                        }},
                        "OWNER"    : {{
                            "@class": "xsd:string",
                            "@type": "Optional"
                        }},
                        {}
                    }}
                "#,
                &type_name,
                att_schemas_vec.join(",")
            )
        };
        if let Ok(obj) = serde_json::from_str::<serde_json::Map<String, Value>>(&json) {
            return Some(obj);
        } else {
            let pretty_json = jsonxf::minimize(&json).unwrap();
            dbg!(&pretty_json);
        }
        None
    }

    pub fn get_all_schemas(&self) -> Vec<serde_json::Map<String, serde_json::Value>> {
        use serde_json::{Map, Value};
        let mut schemas = Vec::new();
        for kv in &self.noun_attr_info_map {
            if *kv.key() < 0 {
                continue;
            }
            let type_name = db1_dehash(*kv.key() as _);
            if let Some(obj) = Self::gen_schema(*kv.key(), kv.value()) {
                schemas.push(obj);
            }
        }
        schemas
    }

    pub fn fill_default_values(&self, att_map: &mut AttrMap) {
        let noun_hash = att_map.get_noun();
        if let Some(m) = self.noun_attr_info_map.get(&noun_hash) {
            for info in m.value() {
                if info.offset == 0 && !att_map.contains_attr_hash(noun_hash as _) {
                    att_map.insert(noun_hash as _, info.default_val.clone());
                }
            }
        }
    }

    ///合并
    pub fn merge(&self, new: &DashMap<i32, DashMap<i32, AttrInfo>>) {
        for kv in new {
            self.noun_attr_info_map
                .insert(*kv.key(), kv.value().clone());
        }
    }

    pub fn save(&self, path: Option<&str>) -> anyhow::Result<()> {
        let path = path.unwrap_or("all_attr_info.json");
        let bytes = serde_json::to_string(self)?;
        let mut file = std::fs::File::create(path)?;
        let _ = file.write_all(bytes.as_bytes());
        Ok(())
    }
}

unsafe impl Send for PdmsDatabaseInfo {}

unsafe impl Sync for PdmsDatabaseInfo {}
