//! One camera request = one bounds query. Late replies never move the camera.
//!
//! 范围内没有已生成的模型就是没有：相机不动、日志提示右键「重新生成模型」，不再为聚焦
//! 悄悄生成一次（ADR-0030 D1 / 后果 4；此前这里有 Query → Ensure → Requery 三相）。
pub struct Request<T> {
    epoch: u64,
    pending: Option<T>,
}

impl<T: Copy + PartialEq> Default for Request<T> {
    fn default() -> Self {
        Self {
            epoch: 0,
            pending: None,
        }
    }
}

impl<T: Copy + PartialEq> Request<T> {
    pub fn cancel(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.pending = None;
    }
    pub fn begin(&mut self, target: T) -> u64 {
        self.cancel();
        self.pending = Some(target);
        self.epoch
    }
    pub fn accepts_bounds(&self, epoch: u64, target: T) -> bool {
        epoch == self.epoch && self.pending == Some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_request_accepts_exactly_its_own_reply_until_cancelled() {
        let mut request = Request::default();
        let epoch = request.begin(1);
        assert!(request.accepts_bounds(epoch, 1));
        assert!(!request.accepts_bounds(epoch, 2));
        request.cancel();
        assert!(!request.accepts_bounds(epoch, 1));
    }
    #[test]
    fn latest_target_wins_and_reconnect_invalidates_replies() {
        let mut request = Request::default();
        let a = request.begin(1);
        let b = request.begin(2);
        assert!(!request.accepts_bounds(a, 1));
        assert!(!request.accepts_bounds(b, 1));
        assert!(request.accepts_bounds(b, 2));
        request.cancel();
        assert!(!request.accepts_bounds(b, 2));
    }
}
