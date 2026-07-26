use crate::petgraph::{PetRefnoGraph, PetRefnoNode};
use crate::tool::db_tool::db1_dehash;
use crate::{AttrVal, RefU64};
use dashmap::DashMap;
use petgraph::prelude::*;
use std::collections::{BTreeMap, HashMap};

#[derive(Default, Debug)]
pub struct DbBasicData {
    pub ses_pgno: u32,
    pub bytes: Vec<u8>,
    pub world_refno: RefU64,
    pub children_map: HashMap<RefU64, Vec<RefU64>>,
    pub refno_table_map: DashMap<RefU64, EleDataEntry>,
}

#[derive(Default, Debug)]
pub struct EleDataEntry {
    pub pos: usize,
    pub noun_hash: i32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DbInfo {
    pub numb_db: AttrVal,
    pub db_name: String,
}

impl DbBasicData {
    #[inline]
    pub fn get_type(&self, refno: RefU64) -> String {
        let Some(entry) = self.refno_table_map.get(&refno) else {
            return "unset".to_string();
        };
        db1_dehash(entry.noun_hash as _)
    }

    #[inline]
    pub fn get_type_hash(&self, refno: RefU64) -> u32 {
        let Some(entry) = self.refno_table_map.get(&refno) else {
            return 0;
        };
        entry.noun_hash as _
    }

    pub fn gen_petgraph(&self) -> PetRefnoGraph {
        let mut graph = DiGraph::<PetRefnoNode, ()>::new();
        let mut node_indices = HashMap::<u64, NodeIndex>::new();
        for &refno in self.children_map.keys() {
            let node_index = graph.add_node(PetRefnoNode {
                id: *refno,
                noun_hash: self.get_type_hash(refno),
            });
            node_indices.insert(*refno, node_index);
        }
        for (id, children) in self.children_map.iter() {
            if let Some(parent_index) = node_indices.get(id) {
                for child_id in children {
                    if let Some(child_index) = node_indices.get(child_id) {
                        graph.add_edge(*parent_index, *child_index, ());
                    }
                }
            }
        }

        PetRefnoGraph {
            ses_pgno: self.ses_pgno,
            graph,
            node_indices,
        }
    }
}
