//! Bounded timing policies shared by native Discord work and portable fixtures.
use std::time::Duration;
use tokio::time::Instant;

#[derive(Default)]
pub(crate) struct PermissionSchedule {
    checking: bool,
    next: Option<Instant>,
}
impl PermissionSchedule {
    pub fn begin(&mut self, now: Instant) -> bool {
        if self.checking || self.next.is_some_and(|next| now < next) {
            return false;
        }
        self.checking = true;
        true
    }
    pub fn complete(&mut self, now: Instant) {
        self.checking = false;
        self.next = Some(now + Duration::from_secs(5));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RetryReadiness {
    Ready,
    Waiting,
    Expired,
}
#[derive(Default)]
pub(crate) struct RefreshRetry {
    deadline: Option<Instant>,
    next: Option<Instant>,
}
impl RefreshRetry {
    pub fn readiness(&self, now: Instant) -> RetryReadiness {
        if self.deadline.is_some_and(|deadline| now >= deadline) {
            RetryReadiness::Expired
        } else if self.next.is_some_and(|next| now < next) {
            RetryReadiness::Waiting
        } else {
            RetryReadiness::Ready
        }
    }
    pub fn failed(&mut self, now: Instant) -> bool {
        let deadline = *self.deadline.get_or_insert(now + Duration::from_secs(30));
        if now >= deadline {
            return false;
        }
        self.next = Some(now + Duration::from_secs(1));
        true
    }
    pub fn succeeded(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_results_need_no_delay_but_next_request_observes_cooldown() {
        let mut schedule = PermissionSchedule::default();
        let now = Instant::now();
        assert!(schedule.begin(now));
        assert!(!schedule.begin(now + Duration::from_secs(10)));
        // Results may be applied now; only another request has to wait.
        schedule.complete(now + Duration::from_secs(2));
        assert!(!schedule.begin(now + Duration::from_millis(6999)));
        assert!(schedule.begin(now + Duration::from_secs(7)));
    }

    #[test]
    fn refresh_retries_are_paced_and_cannot_extend_the_first_failure_deadline() {
        let now = Instant::now();
        let mut retry = RefreshRetry::default();
        assert_eq!(retry.readiness(now), RetryReadiness::Ready);
        assert!(retry.failed(now));
        assert_eq!(retry.readiness(now), RetryReadiness::Waiting);
        assert_eq!(
            retry.readiness(now + Duration::from_secs(1)),
            RetryReadiness::Ready
        );
        assert!(retry.failed(now + Duration::from_secs(29)));
        assert_eq!(
            retry.readiness(now + Duration::from_secs(30)),
            RetryReadiness::Expired
        );
        assert!(!retry.failed(now + Duration::from_secs(30)));
        retry.succeeded();
        assert_eq!(
            retry.readiness(now + Duration::from_secs(40)),
            RetryReadiness::Ready
        );
        assert!(retry.failed(now + Duration::from_secs(40)));
    }
}
