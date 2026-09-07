//! Union overlapping queried scopes without losing coincident instances within a scope,
//! and refuse to mix two model sources inside one dbnum.
//!
//! gen-model 按**这根所属的库**选取数源（spec §4.12）：已初始化的库读 rocksdb
//! （`model-database`），还在初始化的库由 API 从进程内投影供数（`model-memory`）。
//! 一次装载跨几个库很正常，各库各自的源也正常；**同一个库两页记录来自两个源**
//! 就不正常了——那是翻面正好夹在两次请求之间，两页算的可能不是同一版文件。
//! 这里沿用「不混纪元」的纪律：按 dbnum 分桶，一个桶只认一个源，混了直接报错，
//! 让调用点整趟重来，而不是把两版几何拼成一个内部不一致的视口。
use aios_core::GeomInstQuery;
use plant_ui::task_queue::ModelSource;
use std::collections::HashMap;

/// 一页记录是从哪个库、哪个源来的（`/model/records` 回执的 `dbnum` / `source`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecordScope {
    /// 老服务端不给 `dbnum`：那时只能按整个响应当一个桶，退回「整次装载不混源」。
    pub dbnum: Option<u32>,
    /// `source` 字面值认不出（老服务端、或新加的第三种）时为 `None`，不参与混源判定。
    pub source: Option<ModelSource>,
}

impl RecordScope {
    /// 契约里还没有 `dbnum` / `source` 那阵子的形态（测试与 sim 用）。
    pub(super) fn legacy() -> Self {
        Self {
            dbnum: None,
            source: None,
        }
    }
}

#[derive(Default)]
pub(super) struct ModelRecordUnion {
    records: Vec<GeomInstQuery>,
    multiplicity: HashMap<String, usize>,
    /// 每个桶（dbnum）已经认下的源。`None` 键 = 服务端没给 dbnum 的那一桶。
    sources: HashMap<Option<u32>, ModelSource>,
}

impl ModelRecordUnion {
    pub(super) fn extend_scope(
        &mut self,
        scope: RecordScope,
        records: Vec<GeomInstQuery>,
    ) -> anyhow::Result<()> {
        if let Some(source) = scope.source {
            match self.sources.get(&scope.dbnum) {
                Some(seen) if *seen != source => anyhow::bail!(
                    "{} 的模型记录混了两个取数源：先到的是 {}，这一页是 {}。\
                     同一个库只接受一个源（初始化发布刚翻面，整趟重查一次即可）",
                    match scope.dbnum {
                        Some(dbnum) => format!("db{dbnum}"),
                        None => "这次装载".to_owned(),
                    },
                    seen.as_str(),
                    source.as_str()
                ),
                Some(_) => {}
                None => {
                    self.sources.insert(scope.dbnum, source);
                }
            }
        }
        // A BRAN and its members overlap. Refno alone is not an identity:
        // separate implied tubes share the BRAN refno but have different transforms.
        // Preserve the maximum multiplicity per scope, including coincident records.
        let mut scope_counts = HashMap::<String, usize>::new();
        for record in records {
            // The records endpoint labels `owner` with the queried scope, so
            // BRAN and member queries return different owners for the same mesh.
            // Keep the first record's owner but do not use this scope label as identity.
            let mut identity = serde_json::to_value(&record)?;
            identity.as_object_mut().expect("GeomInstQuery serializes as an object").remove("owner");
            let key = serde_json::to_string(&identity)?;
            let count = scope_counts.entry(key.clone()).or_default();
            *count += 1;
            let published = self.multiplicity.entry(key).or_default();
            if *count > *published {
                self.records.push(record);
                *published = *count;
            }
        }
        Ok(())
    }

    /// 这次装载里每个库认下的源（dbnum 升序，没给 dbnum 的那一桶排最前）。
    /// 给日志用：一次装载里哪几个库还在吃内存投影，人要看得见。
    pub(super) fn sources(&self) -> Vec<(Option<u32>, ModelSource)> {
        let mut out: Vec<_> = self.sources.iter().map(|(k, v)| (*k, *v)).collect();
        out.sort_by_key(|(dbnum, _)| *dbnum);
        out
    }

    pub(super) fn finish(self) -> Vec<GeomInstQuery> {
        self.records
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<GeomInstQuery> {
        let page: serde_json::Value = serde_json::from_str(include_str!(
            "fixtures/pipe26229-deleted-records.json"
        )).unwrap();
        serde_json::from_value(page["items"].clone()).unwrap()
    }

    fn scope(dbnum: u32, source: ModelSource) -> RecordScope {
        RecordScope {
            dbnum: Some(dbnum),
            source: Some(source),
        }
    }

    #[test]
    fn overlapping_branch_and_members_keep_fourteen_not_twenty_five() {
        let records = fixture();
        assert_eq!(records.len(), 14);
        let members: Vec<Vec<GeomInstQuery>> = serde_json::from_str(include_str!(
            "fixtures/pipe26229-member-records.json"
        )).unwrap();
        assert_eq!(members.len(), 11);
        let mut union = ModelRecordUnion::default();
        union.extend_scope(RecordScope::legacy(), records).unwrap();
        for scope in members { union.extend_scope(RecordScope::legacy(), scope).unwrap(); }
        let result = union.finish();
        assert_eq!(result.len(), 14);
        assert_eq!(result.iter().filter(|r| r.generic == "TUBI").count(), 3);
    }

    #[test]
    fn repeated_scope_preserves_within_scope_multiplicity_and_order() {
        let mut records = fixture();
        records.push(fixture().remove(0));
        let expected = serde_json::to_value(&records).unwrap();
        let mut union = ModelRecordUnion::default();
        union.extend_scope(RecordScope::legacy(), serde_json::from_value(expected.clone()).unwrap()).unwrap();
        union.extend_scope(RecordScope::legacy(), records).unwrap();
        assert_eq!(serde_json::to_value(union.finish()).unwrap(), expected);
    }

    /// 跨库各自的源是常态（一个库已初始化、另一个还在初始化）；同一个库两个源是事故。
    #[test]
    fn buckets_by_dbnum_and_refuses_two_sources_inside_one_dbnum() {
        let mut union = ModelRecordUnion::default();
        union.extend_scope(scope(8000, ModelSource::Database), fixture()).unwrap();
        union.extend_scope(scope(8021, ModelSource::Memory), Vec::new()).unwrap();
        union.extend_scope(scope(8000, ModelSource::Database), Vec::new()).unwrap();
        assert_eq!(
            union.sources(),
            vec![(Some(8000), ModelSource::Database), (Some(8021), ModelSource::Memory)]
        );

        let error = union
            .extend_scope(scope(8000, ModelSource::Memory), Vec::new())
            .unwrap_err()
            .to_string();
        assert!(error.contains("db8000"), "{error}");
        assert!(error.contains("database") && error.contains("memory"), "{error}");
    }

    /// 老服务端没有 `dbnum`：整次装载算一个桶，两个源照样拒绝——这正是此前「不混纪元」的口径。
    #[test]
    fn without_dbnum_the_whole_load_is_one_bucket() {
        let mut union = ModelRecordUnion::default();
        let memory = RecordScope { dbnum: None, source: Some(ModelSource::Memory) };
        let database = RecordScope { dbnum: None, source: Some(ModelSource::Database) };
        union.extend_scope(memory, Vec::new()).unwrap();
        union.extend_scope(memory, Vec::new()).unwrap();
        assert!(union.extend_scope(database, Vec::new()).is_err());
        // 认不出的源不参与判定：老服务端连 `source` 都没有的响应不能因此整趟失败。
        union.extend_scope(RecordScope::legacy(), fixture()).unwrap();
        assert_eq!(union.finish().len(), 14);
    }
}
