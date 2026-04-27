use std::time::Duration;

use tokio::time::Instant;

const ATTACH_REFRESH_COALESCE: Duration = Duration::from_millis(16);

#[derive(Debug, Clone)]
pub(super) struct AttachRefreshScheduler {
    deadline: Option<Instant>,
    interval: Duration,
}

impl Default for AttachRefreshScheduler {
    fn default() -> Self {
        Self {
            deadline: None,
            interval: ATTACH_REFRESH_COALESCE,
        }
    }
}

impl AttachRefreshScheduler {
    pub(super) fn schedule_now(&mut self) {
        self.schedule(Instant::now());
    }

    fn schedule(&mut self, now: Instant) {
        if self.deadline.is_none() {
            self.deadline = Some(now + self.interval);
        }
    }

    pub(super) fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    pub(super) fn clear(&mut self) {
        self.deadline = None;
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::Instant;

    use super::AttachRefreshScheduler;

    #[test]
    fn schedule_keeps_the_first_deadline_until_cleared() {
        let mut scheduler = AttachRefreshScheduler::default();
        let first = Instant::now();
        let second = first + scheduler.interval + scheduler.interval;

        scheduler.schedule(first);
        let first_deadline = scheduler.deadline().expect("scheduled deadline");
        scheduler.schedule(second);

        assert_eq!(scheduler.deadline(), Some(first_deadline));
        scheduler.clear();
        scheduler.schedule(second);
        assert_ne!(scheduler.deadline(), Some(first_deadline));
    }
}
