use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum StreamItemWait<T> {
    Item(Option<T>),
    Interrupted(DownloadOutcome),
    Stalled,
}

const WORKER_CONTROL_CONTINUE: u8 = 0;
const WORKER_CONTROL_PAUSED: u8 = 1;
const WORKER_CONTROL_CANCELED: u8 = 2;

#[derive(Debug, Clone)]
pub(super) struct WorkerControlSignal {
    value: Arc<AtomicU8>,
}

impl Default for WorkerControlSignal {
    fn default() -> Self {
        Self {
            value: Arc::new(AtomicU8::new(WORKER_CONTROL_CONTINUE)),
        }
    }
}

impl WorkerControlSignal {
    pub(super) fn store_control(&self, control: WorkerControl) {
        let value = match control {
            WorkerControl::Continue => WORKER_CONTROL_CONTINUE,
            WorkerControl::Paused => WORKER_CONTROL_PAUSED,
            WorkerControl::Canceled | WorkerControl::Missing => WORKER_CONTROL_CANCELED,
        };
        self.value.store(value, Ordering::Relaxed);
    }

    pub(super) fn current_outcome(&self) -> Option<DownloadOutcome> {
        match self.value.load(Ordering::Relaxed) {
            WORKER_CONTROL_PAUSED => Some(DownloadOutcome::Paused),
            WORKER_CONTROL_CANCELED => Some(DownloadOutcome::Canceled),
            _ => None,
        }
    }
}

pub(super) struct WorkerControlPoller {
    stop: Arc<AtomicBool>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

impl WorkerControlPoller {
    pub(super) fn spawn(state: SharedState, job_id: String, signal: WorkerControlSignal) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let poll_stop = stop.clone();
        let handle = tauri::async_runtime::spawn(async move {
            while !poll_stop.load(Ordering::Relaxed) {
                let control = state.worker_control(&job_id).await;
                signal.store_control(control);
                if !matches!(control, WorkerControl::Continue) {
                    break;
                }
                tokio::time::sleep(THROTTLE_CONTROL_INTERVAL).await;
            }
        });

        Self { stop, handle }
    }
}

impl Drop for WorkerControlPoller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.abort();
    }
}

#[cfg(test)]
pub(super) struct StreamController<'a> {
    state: &'a SharedState,
    job_id: &'a str,
    stall_timeout: Option<Duration>,
}

#[cfg(test)]
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

pub(super) struct SignalStreamController {
    signal: WorkerControlSignal,
    stall_timeout: Option<Duration>,
    control_interval: tokio::time::Interval,
}

impl SignalStreamController {
    pub(super) fn new(signal: WorkerControlSignal, stall_timeout: Option<Duration>) -> Self {
        let mut control_interval = tokio::time::interval(THROTTLE_CONTROL_INTERVAL);
        control_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        Self {
            signal,
            stall_timeout,
            control_interval,
        }
    }

    pub(super) async fn next<T, F>(&mut self, next_item: F) -> StreamItemWait<T>
    where
        F: Future<Output = Option<T>>,
    {
        tokio::pin!(next_item);

        if let Some(outcome) = self.signal.current_outcome() {
            return StreamItemWait::Interrupted(outcome);
        }

        if let Some(timeout) = self.stall_timeout {
            let stall_sleep = tokio::time::sleep(timeout);
            tokio::pin!(stall_sleep);
            loop {
                tokio::select! {
                    item = &mut next_item => return StreamItemWait::Item(item),
                    _ = &mut stall_sleep => return StreamItemWait::Stalled,
                    _ = self.control_interval.tick() => {
                        if let Some(outcome) = self.signal.current_outcome() {
                            return StreamItemWait::Interrupted(outcome);
                        }
                    }
                }
            }
        }

        loop {
            tokio::select! {
                item = &mut next_item => return StreamItemWait::Item(item),
                _ = self.control_interval.tick() => {
                    if let Some(outcome) = self.signal.current_outcome() {
                        return StreamItemWait::Interrupted(outcome);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
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
