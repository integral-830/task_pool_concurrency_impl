use std::sync::Arc;
use std::{future::Future, marker::Send, ops::FnOnce};
use tokio::sync::Semaphore;

pub async fn fetch_url(url: String) -> String {
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    format!("fetched: {}", url)
}

pub async fn run_with_limit<F, Fut, T>(tasks: Vec<F>, limit: usize) -> Vec<T>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let semaphore = Arc::new(Semaphore::new(limit));
    let mut handles = Vec::new();

    for task in tasks {
        let permit = Arc::clone(&semaphore);
        let handle = tokio::spawn(async move {
            let _permit = permit.acquire().await.unwrap();
            task().await
        });
        handles.push(handle);
    }
    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.unwrap());
    }
    results
}
