use crate::pe::SPdmsElement;

#[test]
fn test_des() {
    let str = r#"
        {
  "cata_hash": "13292_0",
  "dbnum": 5100,
  "deleted": false,
  "sesno": 114,
  "id": "pe:13292_0",
  "lock": false,
  "name": "/*",
  "noun": "WORL",
  "owner": "pe:0_0",
  "refno": "WORL:13292_0",
  "status_tag": null,
  "version_tag": null
}
    "#;

    let pe: SPdmsElement = serde_json::from_str(str).unwrap();
    dbg!(pe);
}