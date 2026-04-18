use std::sync::Arc;
use std::{future::Future, marker::Send, ops::FnOnce};
use tokio::sync::Semaphore;

#[tokio::main]
async fn main() {
    let mut urls = Vec::new();

    for i in 0..100 {
        urls.push(format!("https://{i}.com").to_owned())
    }

    let tasks: Vec<_> = urls
        .into_iter()
        .map(|url| move || async move { fetch_url(url).await })
        .collect();

    let results = run_with_limit(tasks, 30).await;

    for res in results {
        println!("{}", res);
    }
}

async fn fetch_url(url: String) -> String {
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    format!("fetched: {}", url)
}

async fn run_with_limit<F, Fut, T>(tasks: Vec<F>, limit: usize) -> Vec<T>
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
