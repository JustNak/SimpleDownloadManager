use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StreamItemWait<T> {
    Item(Option<T>),
    Interrupted(DownloadOutcome),
    Stalled,
}

pub(super) struct StreamController<'a> {
    state: &'a SharedState,
    job_id: &'a str,
    stall_timeout: Option<Duration>,
}

impl<'a> StreamController<'a> {
    pub(super) fn new(
        state: &'a SharedState,
        job_id: &'a str,
        stall_timeout: Option<Duration>,
    ) -> Self {
        Self {
            state,
            job_id,
            stall_timeout,
        }
    }

    pub(super) async fn next<T, F>(&self, next_item: F) -> StreamItemWait<T>
    where
        F: Future<Output = Option<T>>,
    {
        tokio::pin!(next_item);

        if let Some(outcome) = self.control_outcome().await {
            return StreamItemWait::Interrupted(outcome);
        }

        if let Some(timeout) = self.stall_timeout {
            let stall_sleep = tokio::time::sleep(timeout);
            tokio::pin!(stall_sleep);
            loop {
                let control_sleep = tokio::time::sleep(THROTTLE_CONTROL_INTERVAL);
                tokio::pin!(control_sleep);
                tokio::select! {
                    item = &mut next_item => return StreamItemWait::Item(item),
                    _ = &mut stall_sleep => return StreamItemWait::Stalled,
                    _ = &mut control_sleep => {
                        if let Some(outcome) = self.control_outcome().await {
                            return StreamItemWait::Interrupted(outcome);
                        }
                    }
                }
            }
        }

        loop {
            let control_sleep = tokio::time::sleep(THROTTLE_CONTROL_INTERVAL);
            tokio::pin!(control_sleep);
            tokio::select! {
                item = &mut next_item => return StreamItemWait::Item(item),
                _ = &mut control_sleep => {
                    if let Some(outcome) = self.control_outcome().await {
                        return StreamItemWait::Interrupted(outcome);
                    }
                }
            }
        }
    }

    async fn control_outcome(&self) -> Option<DownloadOutcome> {
        match self.state.worker_control(self.job_id).await {
            WorkerControl::Continue => None,
            WorkerControl::Paused => Some(DownloadOutcome::Paused),
            WorkerControl::Canceled | WorkerControl::Missing => Some(DownloadOutcome::Canceled),
        }
    }
}

pub(super) async fn next_stream_item_with_control<T, F>(
    state: &SharedState,
    job_id: &str,
    stall_timeout: Option<Duration>,
    next_item: F,
) -> StreamItemWait<T>
where
    F: Future<Output = Option<T>>,
{
    StreamController::new(state, job_id, stall_timeout)
        .next(next_item)
        .await
}
