use crate::{get_db_option, get_default_pdms_db_info, query_types, RefU64};

///获得branch下的所有托臂
#[tokio::test]
async fn test_query_support_arms() -> anyhow::Result<()> {
    // let gensec_refno: RefU64 = "24384/25797".into();
    // let mgr = get_test_ams_db_manager_async().await;
    // let mut tmp = mgr.query_foreign_refnos(&[gensec_refno], &[&["SPRE", "CATR"]],
    //                                     &["PSTR", "PTSS"], &[], 4).await?;
    // assert_eq!(tmp.pop().unwrap().to_string(), "21438/2368");

    Ok(())
}

#[tokio::test]
async fn test_query_same_refnos() -> anyhow::Result<()> {
    crate::init_test_surreal().await;
    let refno: RefU64 = "17496/274055".into();
    let db_option = get_db_option();
    // let same_refnos = query_same_type_refnos(
    //     refno,
    //     db_option.mdb_name.clone(),
    //     crate::DBType::DESI,
    //     false,
    // )
    // .await?;
    // dbg!(&same_refnos);

    // let types = query_types(&same_refnos).await?;
    // dbg!(&types[..10]);

    // let r = crate::query_multi_deep_children_filter_inst(
    //     &same_refnos[0..20],
    //     &["BRAN", "HANG"],
    //     false,
    // )
    //     .await
    //     .unwrap();
    //
    // dbg!(&r);
    //
    // let r = crate::query_multi_deep_children_filter_inst(
    //     &same_refnos[0..20],
    //     &["FITT"],
    //     false,
    // )
    //     .await
    //     .unwrap();

    Ok(())
}

#[tokio::test]
async fn test_query_deep_children() -> anyhow::Result<()> {
    use crate::pdms_types::GNERAL_LOOP_OWNER_NOUN_NAMES;
    use crate::query_multi_deep_children_filter_inst;
    crate::init_test_surreal().await;
    let refno: RefU64 = "17496/274056".into();
    let deep =
        query_multi_deep_children_filter_inst(&[refno], &GNERAL_LOOP_OWNER_NOUN_NAMES, false)
            .await?;

    dbg!(deep);

    Ok(())
}

//query_same_refnos_by_type
