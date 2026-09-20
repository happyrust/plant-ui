//! Detect source saves even when read-through refreshes do not create queue tasks.
use plant_ui::task_queue::DbnumStatus;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct SourceVersions {
    previous: Option<BTreeMap<u32, i32>>,
}

impl SourceVersions {
    pub(crate) fn observe(&mut self, rows: &[DbnumStatus]) -> bool {
        let current: BTreeMap<_, _> = rows
            .iter()
            .filter(|row| {
                !row.excluded
                    && !row.not_in_project
                    && !row.blocked
                    && row.file_latest_sesno > 0
                    && (row.db_type.eq_ignore_ascii_case("DESI")
                        || row.db_type.eq_ignore_ascii_case("ISOD"))
            })
            .map(|row| (row.dbnum, row.file_latest_sesno))
            .collect();
        // A missing report is not a reset. Keep the last known versions so a
        // save during a failed poll is still detected on the next success.
        if current.is_empty() {
            return false;
        }
        let changed = self.previous.as_ref().is_some_and(|old| old != &current);
        self.previous = Some(current);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(sesno: i32) -> DbnumStatus {
        DbnumStatus {
            dbnum: 8000,
            db_type: "DESI".into(),
            file_latest_sesno: sesno,
            ..Default::default()
        }
    }
    #[test]
    fn read_through_save_is_detected_without_task_or_cache_epoch() {
        let mut versions = SourceVersions::default();
        assert!(!versions.observe(&[row(288)]));
        assert!(!versions.observe(&[row(288)]));
        assert!(versions.observe(&[row(289)]));
        assert!(!versions.observe(&[row(289)]));
    }
    #[test]
    fn failed_poll_does_not_swallow_saves_or_fixture_reset() {
        let mut versions = SourceVersions::default();
        assert!(!versions.observe(&[row(288)]));
        assert!(!versions.observe(&[]));
        assert!(versions.observe(&[row(290)]));
        assert!(versions.observe(&[row(287)]));
    }
    #[test]
    fn catalogue_and_excluded_rows_do_not_trigger_scene_reload() {
        let mut versions = SourceVersions::default();
        versions.observe(&[row(288)]);
        let mut excluded = row(400);
        excluded.dbnum = 9000;
        excluded.excluded = true;
        let mut catalogue = row(300);
        catalogue.dbnum = 7000;
        catalogue.db_type = "CATA".into();
        assert!(!versions.observe(&[catalogue, excluded, row(288)]));
    }
}
