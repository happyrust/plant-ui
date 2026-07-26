use itertools::Itertools;
//使用once_cell宏来实现单例模式, 读取 noun_graph.json 得到一个 DiGraph
use once_cell::sync::Lazy;
use petgraph::graph::DiGraph;
use petgraph::graph::NodeIndex;
use std::fs::File;
use std::io::Read;

use crate::tool::db_tool::*;

//使用once_cell宏来实现单例模式, 读取 noun_graph.json 得到一个 DiGraph
pub static NOUN_GRAPH: Lazy<DiGraph<u32, u32>> = Lazy::new(|| {
    let graph: DiGraph<u32, u32> =
        serde_json::from_slice(include_bytes!("../noun_graph.json")).unwrap();
    graph
});

pub fn is_owner_type(child_noun: &str, own_noun: &str) -> bool {
    let Some(child_node) = NOUN_GRAPH
        .node_indices()
        .find(|i| NOUN_GRAPH[*i] == db1_hash(child_noun))
    else {
        return false;
    };
    let Some(own_node) = NOUN_GRAPH
        .node_indices()
        .find(|i| NOUN_GRAPH[*i] == db1_hash(own_noun))
    else {
        return false;
    };
    let mut paths = petgraph::algo::all_simple_paths::<Vec<NodeIndex<u32>>, &DiGraph<u32, u32>>(
        &NOUN_GRAPH,
        child_node,
        own_node,
        0,
        None,
    );
    while let Some(path) = paths.next() {
        if path.len() == 2 {
            return true;
        }
    }
    false
}

//指定起始的和目标noun node, 传入的参数是noun name, 通过 db1_hash 找到对应的node，找到经过的路径的noun， 使用all_simple_paths
pub fn find_noun_path(start_noun: &str, end_noun: &str) -> Vec<Vec<String>> {
    // dbg!(start_noun);
    // dbg!(end_noun);
    let Some(start_node) = NOUN_GRAPH
        .node_indices()
        .find(|i| NOUN_GRAPH[*i] == db1_hash(start_noun))
    else {
        return vec![];
    };
    let Some(end_node) = NOUN_GRAPH
        .node_indices()
        .find(|i| NOUN_GRAPH[*i] == db1_hash(end_noun))
    else {
        return vec![];
    };
    let mut paths = petgraph::algo::all_simple_paths::<Vec<NodeIndex<u32>>, &DiGraph<u32, u32>>(
        &NOUN_GRAPH,
        start_node,
        end_node,
        0,
        None,
    );
    let mut result = Vec::new();
    while let Some(path) = paths.next() {
        let mut path_str = Vec::new();
        for node in path {
            path_str.push(db1_dehash(NOUN_GRAPH[node]));
        }
        result.push(path_str);
    }
    result
}

//TODO: 合并方法, 调用一个统一的
///沿着owner一直往上找到过滤的节点
pub fn gen_noun_outcoming_relate_sql(start_noun: &str, filter_nouns: &[&str]) -> Option<String> {
    // dbg!(&start_noun);
    let paths = filter_nouns
        .into_iter()
        .map(|n| {
            find_noun_path(start_noun, n)
                .into_iter()
                .map(|x: Vec<String>| x.clone())
                .collect::<Vec<_>>()
        })
        .filter(|x| !x.is_empty())
        .flatten()
        .collect::<Vec<_>>();
    // dbg!(&paths);
    let contains_self = filter_nouns.contains(&start_noun);
    if paths.is_empty() && !contains_self {
        return None;
    }
    let min_len = paths.iter().map(|x| x.len()).min().unwrap_or_default();
    let max_len = paths.iter().map(|x| x.len()).max().unwrap_or_default();
    let mut sql = String::new();
    if contains_self {
        sql.push_str("id as p0,");
    }
    for i in 1..max_len {
        if i >= min_len - 1 {
            let filter = paths
                .iter()
                .filter_map(|x| x.get(i).map(|s| format!("'{s}'")))
                .unique()
                .collect::<Vec<_>>();
            sql.push_str(&format!(
                "->pe_owner[where out.noun in [{}]]->(? as p{})",
                filter.join(","),
                i,
            ));
        } else {
            sql.push_str(&format!("->pe_owner->(?)"));
        }
    }
    if sql.ends_with(',') {
        sql.remove(sql.len() - 1);
    }
    Some(sql)
}

/// 获取与指定终止名词相关的路径，传入的参数是终止名词和过滤名词列表
pub fn gen_noun_incoming_relate_sql(end_noun: &str, filter_nouns: &[&str]) -> Option<String> {
    let mut paths = filter_nouns
        .into_iter()
        .map(|n| {
            find_noun_path(n, end_noun)
                .into_iter()
                .map(|x: Vec<String>| {
                    let mut v = x.into_iter().rev().collect::<Vec<_>>();
                    v
                })
                .collect::<Vec<_>>()
        })
        .filter(|x| !x.is_empty())
        .flatten()
        .collect::<Vec<_>>();
    let contains_self = filter_nouns.contains(&end_noun);
    if paths.is_empty() && !contains_self {
        return None;
    }
    let min_len = paths.iter().map(|x| x.len()).min().unwrap_or_default();
    let max_len = paths.iter().map(|x| x.len()).max().unwrap_or_default();
    let mut sql = String::new();
    if contains_self {
        sql.push_str("id as p0,");
    }
    for i in 1..max_len {
        if i >= min_len - 1 {
            let filter = paths
                .iter()
                .filter_map(|x| x.get(i).map(|s| format!("'{s}'")))
                .unique()
                .collect::<Vec<_>>();
            sql.push_str(&format!(
                "<-pe_owner[where in.noun in [{}]]<-(? as p{})",
                filter.join(","),
                i,
            ));
        } else {
            sql.push_str(&format!("<-pe_owner<-(?)"));
        }
    }
    if sql.ends_with(',') {
        sql.remove(sql.len() - 1);
    }
    Some(sql)
}

//指定起始的和目标noun node, 传入的参数是noun name, 通过 db1_hash 找到对应的node，找到经过的路径的noun， 使用all_simple_paths
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_is_owner() {
        // put some values into your NOUN_GRAPH for testing.
        // NOUN_GRAPH.add_node(...);

        assert_eq!(is_owner_type("ELBO", "BRAN"), true);
        assert_eq!(is_owner_type("BRAN", "PIPE"), true);
        // add as many test cases as you need
    }

    #[test]
    fn test_find_path() {
        // Test case 1: Start and end nouns are the same
        let start_noun = "ELBO";
        let end_noun = "SITE";
        // let result = gen_noun_outcoming_relate_path(&[start_noun], end_noun, "");
        // dbg!(&result);
        // assert_eq!(result, vec![vec![start_noun.to_string()]]);

        // Test case 2: Start and end nouns are connected directly
        // let start_noun = "noun1";
        // let end_noun = "noun2";
        // let result = find_path(start_noun, end_noun);
        // assert_eq!(result, vec![vec![start_noun.to_string(), end_noun.to_string()]]);

        // // Test case 3: Start and end nouns are connected through multiple paths
        // let start_noun = "noun1";
        // let end_noun = "noun4";
        // let result = find_path(start_noun, end_noun);
        // assert_eq!(
        //     result,
        //     vec![
        //         vec![
        //             start_noun.to_string(),
        //             "noun2".to_string(),
        //             "noun3".to_string(),
        //             end_noun.to_string()
        //         ],
        //         vec![
        //             start_noun.to_string(),
        //             "noun2".to_string(),
        //             "noun4".to_string()
        //         ]
        //     ]
        // );

        // // Test case 4: Start and end nouns are not connected
        // let start_noun = "noun1";
        // let end_noun = "noun5";
        // let result = find_path(start_noun, end_noun);
        // assert_eq!(result, Vec::<Vec<String>>::new());
    }
}
