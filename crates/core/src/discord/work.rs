//! Bounded asynchronous room work, including cancelled tasks awaiting their final poll.
use super::protocol::RoomKey;
use std::future::Future;
use tokio::task::AbortHandle;
const LIMIT: usize = 12;
#[derive(Default)]
pub struct TaskBudget {
    tasks: Vec<(RoomKey, AbortHandle)>,
}
impl TaskBudget {
    pub fn spawn(&mut self, key: RoomKey, work: impl Future<Output = ()> + Send + 'static) -> bool {
        self.tasks.retain(|(_, task)| !task.is_finished());
        if self.tasks.len() == LIMIT {
            return false;
        }
        self.tasks.push((key, tokio::spawn(work).abort_handle()));
        true
    }
    pub fn cancel(&mut self, key: &RoomKey) {
        for (owner, task) in &self.tasks {
            if owner == key {
                task.abort();
            }
        }
        // Cancellation is a request; retain occupancy until the runtime observes completion.
        self.tasks.retain(|(_, task)| !task.is_finished());
    }
}
impl Drop for TaskBudget {
    fn drop(&mut self) {
        for (_, task) in &self.tasks {
            task.abort();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn key(intent: u64) -> RoomKey {
        RoomKey {
            destination_id: "discord-1".into(),
            intent,
            server_id: "1".into(),
            channel_id: "2".into(),
        }
    }
    #[tokio::test]
    async fn cancellation_keeps_outstanding_work_bounded_until_actual_completion() {
        let mut tasks = TaskBudget::default();
        for intent in 1..=LIMIT as u64 {
            assert!(tasks.spawn(key(intent), std::future::pending()));
            tasks.cancel(&key(intent));
        }
        assert!(!tasks.spawn(key(100), std::future::pending()));
        assert_eq!(tasks.tasks.len(), LIMIT);
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        assert!(tasks.spawn(key(101), std::future::pending()));
        assert_eq!(tasks.tasks.len(), 1);
    }
    #[tokio::test]
    async fn cancelling_one_room_preserves_a_live_peer_and_rejects_old_generation() {
        let mut tasks = TaskBudget::default();
        let (send, receive) = tokio::sync::oneshot::channel();
        let (done, received) = tokio::sync::oneshot::channel();
        assert!(tasks.spawn(key(1), std::future::pending()));
        assert!(tasks.spawn(key(2), async move {
            let _ = receive.await;
            let _ = done.send(());
        }));
        tasks.cancel(&key(1));
        send.send(()).unwrap();
        received.await.unwrap();
    }
}
