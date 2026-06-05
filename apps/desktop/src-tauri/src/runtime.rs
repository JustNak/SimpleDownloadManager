use std::future::Future;

pub(crate) type JoinHandle<T> = tauri::async_runtime::JoinHandle<T>;

pub(crate) fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    tauri::async_runtime::spawn(future)
}

pub(crate) fn spawn_blocking<F, R>(func: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(func)
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn runtime_facade_spawns_async_and_blocking_work() {
        let async_handle = super::spawn(async { 41usize });
        let blocking_handle = super::spawn_blocking(|| 1usize);

        let async_result = async_handle
            .await
            .expect("async facade task should complete");
        let blocking_result = blocking_handle
            .await
            .expect("blocking facade task should complete");

        assert_eq!(async_result + blocking_result, 42);
    }
}
