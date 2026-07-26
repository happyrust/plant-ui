use crate::graph::*;
use crate::noun_graph::gen_noun_incoming_relate_sql;
use crate::noun_graph::gen_noun_outcoming_relate_sql;
use crate::pdms_types::CATA_WITHOUT_REUSE_GEO_NAMES;
use crate::petgraph::PetRefnoGraph;
use crate::tool::db_tool::db1_hash;
use petgraph::algo::all_simple_paths;
use petgraph::graph::Graph;
use petgraph::graph::NodeIndex;
use petgraph::graphmap::DiGraphMap;
use petgraph::graphmap::GraphMap;
use std::collections::HashSet;

#[test]
fn test_petgraph_search() {
    let path = r#"E:\RustProject\new\gen-model\assets\pg\1112.pg"#;
    let graph = PetRefnoGraph::load(path).unwrap();
    dbg!(graph.node_indices.len());
    let target_hashes = ["PAVE"].iter().map(|x| db1_hash(x)).collect::<HashSet<_>>();
    let search = graph
        .search_path_refnos("17496_248588".into(), |hash| target_hashes.contains(&hash))
        .unwrap();

    dbg!(&search.len());
}

#[test]
fn test_petgraph_noun_path() {
    // 创建一个有向图
    let mut graph = DiGraphMap::new();
    let node_a = graph.add_node("A");
    let node_b = graph.add_node("B");
    let node_c = graph.add_node("C");
    let node_d = graph.add_node("D");
    let node_e = graph.add_node("E");

    graph.add_edge(node_a, node_b, 1);
    graph.add_edge(node_a, node_c, 1);
    graph.add_edge(node_b, node_d, 1);
    graph.add_edge(node_c, node_d, 1);
    graph.add_edge(node_d, node_e, 1);
    let node_e = graph.add_node("E");

    let start_node = node_a;
    let end_node = node_e;

    // let index = graph.node_indices().find(|i| graph[*i] == "B").unwrap();
    // dbg!(&index);

    // 使用 all_simple_paths 函数找到所有路径
    let paths =
        all_simple_paths::<Vec<_>, _>(&graph, start_node, end_node, 0, None).collect::<Vec<_>>();

    // 遍历路径并计算距离
    for path in paths {
        // let distance: i32 = path
        //     .windows(2)
        //     .map(|window| {
        //         graph
        //             .edge_weight(graph.find_edge(window[0], window[1]).unwrap())
        //             .unwrap()
        //     })
        //     .sum();
        // println!("Path: {:?}, Distance: {}", path, distance);
        println!("Path: {:?}", path);
    }
}


#[tokio::test]
async fn test_query_all_bran_hangers() -> anyhow::Result<()> {
    crate::init_test_surreal().await;
    let refno = "17496/171102".into(); // Replace with your desired refno value
    let result = query_filter_all_bran_hangs(refno).await?;
    dbg!(&result.len());

    let result = query_filter_deep_children(
        refno,
        &CATA_WITHOUT_REUSE_GEO_NAMES,
    )
    .await?;
    dbg!(&result);

    let refno = "17496/171180".into(); // Replace with your desired refno value
    let result = query_filter_all_bran_hangs(refno).await?;
    dbg!(&result);

    // TODO: Add assertions to validate the result

    Ok(())
}

#[tokio::test]
async fn test_query_ancestor_filter() -> anyhow::Result<()> {
    crate::init_test_surreal().await;
    let refno = "25688/7957".into();
    // let type_name = crate::get_type_name(refno).await?;
    let target =
        crate::query_filter_ancestors(refno, &["STWALL", "ZONE"])
            .await
            .unwrap();
    dbg!(target);
    Ok(())
}
