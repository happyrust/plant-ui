use once_cell::sync::Lazy;
use std::io::{Read, Write};
use tokio::sync::RwLock;

use crate::{
    RefU64, SUL_DB,
    accel_tree::acceleration_tree::{AccelerationTree, RStarBoundingBox},
};

//或者改成第一次，需要去加载，后续就不用了
//启动的时候就要去加载到内存里
pub static GLOBAL_AABB_TREE: Lazy<RwLock<AccelerationTree>> =
    Lazy::new(|| RwLock::new(AccelerationTree::default()));

pub static GLOBAL_ROOM_AABB_TREE: Lazy<RwLock<AccelerationTree>> =
    Lazy::new(|| RwLock::new(AccelerationTree::default()));

// 不要每次都加载，需要检查缓存，如果缓存有，就不用从数据库里刷新了
#[cfg(not(target_arch = "wasm32"))]
pub async fn load_aabb_tree() -> anyhow::Result<bool> {
    // 如果已生成了空间树的文件，直接读取文件中的数据即可
    //改成使用 AccelerationTree 的反序列化方法
    //不用重复加载
    if !GLOBAL_AABB_TREE.read().await.is_empty() {
        return Ok(true);
    }
    #[cfg(not(feature = "web"))]
    {
        *GLOBAL_AABB_TREE.write().await =
            AccelerationTree::deserialize_from_bin_file().unwrap_or_default();
    }
    // {
    //     if !GLOBAL_AABB_TREE.read().await.is_empty() {
    //         return Ok(true);
    //     }
    // }
    // //如果有缓存文件，直接读取缓存文件
    // //测试分页查询
    // let mut rstar_objs = vec![];
    // let mut offset = 0;

    // let page_count = 1000;
    // loop {
    //     //需要过滤
    //     let sql = format!(
    //         "select in as refno, aabb.d.* as aabb, in.noun as noun from inst_relate where aabb.d!=none and aabb.d.mins[0] < 1000000 and solid start {} limit {page_count}",
    //         offset
    //     );
    //     let mut response = SUL_DB.query(&sql).await?;
    //     let refno_aabbs: Vec<RStarBoundingBox> = response.take(0).unwrap();
    //     if refno_aabbs.is_empty() {
    //         break;
    //     }
    //     rstar_objs.extend(refno_aabbs);
    //     offset += page_count;
    // }

    // //存储在全局变量里, 每次都重新加载，还是就用数据文件来表达？当做资源来加载，不用每次都去加载
    // //加个时间戳，来表达是不是最新的rtree
    // let tree = AccelerationTree::load(rstar_objs);

    // // 将查询好的数据写入到文件中
    // let json = serde_json::to_string(&tree)?;
    // let mut file = std::fs::File::create_new("spa_tree.json")?;
    // file.write_all(&json.into_bytes())?;

    // tree.serialize_to_bin_file();
    // *GLOBAL_AABB_TREE.write().await = tree;
    Ok(false)
}

pub async fn load_room_aabb_tree() -> anyhow::Result<bool> {
    {
        if !GLOBAL_ROOM_AABB_TREE.read().await.is_empty() {
            return Ok(true);
        }
    }
    //如果有缓存文件，直接读取缓存文件
    //测试分页查询
    let mut rstar_objs = vec![];
    let mut offset = 0;

    let page_count = 1000;
    loop {
        //需要过滤
        let sql = format!(
            "select value (select in as refno, aabb.d.* as aabb,
                in.noun as noun from only out->inst_relate 
                where aabb.d!=none
            start {offset} limit {page_count}"
        );
        let mut response = SUL_DB.query(&sql).await?;
        let refno_aabbs: Vec<Option<RStarBoundingBox>> = response.take(0).unwrap();
        if refno_aabbs.is_empty() {
            break;
        }
        for r in refno_aabbs {
            if let Some(r) = r {
                if r.aabb.extents().magnitude().is_infinite()
                    || r.aabb.extents().magnitude().is_nan()
                {
                    continue;
                }
                rstar_objs.push(r);
            }
        }
        // rstar_objs.extend(refno_aabbs.iter().filter_map(|x| x.clone()));
        offset += page_count;
    }
    dbg!(rstar_objs.len());

    //存储在全局变量里, 每次都重新加载，还是就用数据文件来表达？当做资源来加载，不用每次都去加载
    //加个时间戳，来表达是不是最新的rtree
    let tree = AccelerationTree::load(rstar_objs);
    *GLOBAL_ROOM_AABB_TREE.write().await = tree;

    Ok(true)
}
