# aios_core

## 版本管理的 API
- 查询 sesno 对应的所有历史数据，包括新增、修改、删除的数据
  ```rust
  pub struct HisRefno{
        pub refno: Refno,
        pub sesno: i32,
        pub operation: Operation,
  }
  aios_core::query_ses_history(sesno: i32) -> Vec<HisRefno>

  ```
- 查询一个参考号的所有历史变化
  ```rust
    aios_core::query_history_data(refno: Refno) -> Vec<HisRefno>
  ```
- 返回一个 refno 的 两个sesno 数据的差异
  ```rust
    aios_core::diff_sesno(refno: Refno, sesno1: i32, sesno2: i32) -> Vec<Diff>
    pub struct Diff{
        pub items: Vec<DiffItem>,
    }
  ```
