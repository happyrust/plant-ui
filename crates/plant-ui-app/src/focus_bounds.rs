//! One camera request may ensure geometry once. Late replies never move the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Query,
    Ensure,
    Requery,
}

pub struct Request<T> {
    epoch: u64,
    pending: Option<(T, Phase)>,
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
        self.pending = Some((target, Phase::Query));
        self.epoch
    }
    pub fn accepts_bounds(&self, epoch: u64, target: T) -> bool {
        epoch == self.epoch
            && matches!(self.pending, Some((t, Phase::Query | Phase::Requery)) if t == target)
    }
    pub fn ensure_once(&mut self, epoch: u64, target: T) -> bool {
        if epoch != self.epoch || self.pending != Some((target, Phase::Query)) {
            return false;
        }
        self.pending = Some((target, Phase::Ensure));
        true
    }
    pub fn accepts_ensure(&self, epoch: u64, target: T) -> bool {
        epoch == self.epoch && self.pending == Some((target, Phase::Ensure))
    }
    pub fn requery(&mut self) {
        if let Some((target, Phase::Ensure)) = self.pending {
            self.pending = Some((target, Phase::Requery));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_after_generation_is_terminal_not_another_generation() {
        let mut request = Request::default();
        let epoch = request.begin(1);
        assert!(request.accepts_bounds(epoch, 1));
        assert!(request.ensure_once(epoch, 1));
        assert!(!request.accepts_bounds(epoch, 1));
        assert!(!request.ensure_once(epoch, 1));
        assert!(request.accepts_ensure(epoch, 1));
        request.requery();
        assert!(request.accepts_bounds(epoch, 1));
        assert!(!request.ensure_once(epoch, 1));
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
        assert!(request.ensure_once(b, 2));
        request.cancel();
        assert!(!request.accepts_ensure(b, 2));
    }
}
