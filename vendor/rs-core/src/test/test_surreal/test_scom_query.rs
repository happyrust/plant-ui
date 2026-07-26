use glam::DQuat;
use crate::tool::math_tool::dquat_to_pdms_ori_xyz_str;

#[tokio::test]
async fn test_query_scom_axis() -> anyhow::Result<()> {
    // init_test_surreal().await;
    // let refno = "17496/202374".into();
    let ori = DQuat::from_xyzw(0.68301266,
                         0.0,
                         0.1830127,
                         0.7320508);
    dbg!(dquat_to_pdms_ori_xyz_str(&ori, true));

    Ok(())
}