use crate::{rs_surreal, RefU64, RefnoEnum};
use glam::Vec3;
use std::sync::Arc;

#[tokio::test]
async fn test_group_cata_hash() -> anyhow::Result<()> {
    crate::init_test_surreal().await;
    let refnos: Vec<RefU64> = vec!["24383/73928".into()];

    let r = crate::query_multi_deep_children_filter_inst(
        &refnos,
        &["BRAN", "HANG"],
        false,
    )
        .await
        .unwrap();

    dbg!(&r);
    let branch_refnos: Vec<RefnoEnum> = r.into_iter().map(|x| x.into()).collect();

    let map =
        crate::query_group_by_cata_hash(&branch_refnos)
            .await
            .unwrap_or_default();
    dbg!(map.get("15194523709157400402").map(|x| x.group_refnos.clone()));
    // dbg!(&map);

    // let group = rs_surreal::query_group_by_cata_hash(&refnos)
    //     .await
    //     .unwrap();
    // dbg!(&group);
    Ok(())
}

#[tokio::test]
async fn test_query_deep_children_spre() -> anyhow::Result<()> {
    crate::init_test_surreal().await;
    let refno: RefU64 = "16387/64917".into();
    let should_be_none = crate::query_deep_children_refnos_filter_spre(refno.into(), false).await.unwrap();
    assert_eq!(should_be_none.len(), 0);

    // 16507/4480
    let refno: RefU64 = "16507/4480".into();
    let result = crate::query_deep_children_refnos_filter_spre(refno.into(), false).await.unwrap();
    assert_eq!(result.is_empty(), false);

    Ok(())
}
