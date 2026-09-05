use task_pool::foundations::endianness::{serialize_record, Record};
use task_pool::foundations::task_pool::{fetch_url, run_with_limit};

#[tokio::main]
async fn main() {
    /* let mut urls = Vec::new();

    for i in 0..100 {
        urls.push(format!("https://{i}.com").to_owned());
    }

    let tasks: Vec<_> = urls
        .into_iter()
        .map(|url| move || async move { fetch_url(url).await })
        .collect();

    let results = run_with_limit(tasks, 30).await;

    for res in results {
        println!("{res}");
    } */
    let record = Record {
        id: 0x1234_5678,
        flags: 0xABCD,
        kind: 7,
    };

    let bytes = serialize_record(&record);

    println!("{:02X?}", bytes);

    let decoded = task_pool::foundations::endianness::deserialize_record(&bytes).unwrap();

    println!("{decoded:?}");
    println!("original id:  {:08X}", record.id);
    println!("decoded id:   {:08X}", decoded.id);

    println!("original flags: {:04X}", record.flags);
    println!("decoded flags:  {:04X}", decoded.flags);

    println!("original kind: {}", record.kind);
    println!("decoded kind:  {}", decoded.kind);

    println!("equal: {}", record == decoded);

    assert_eq!(record, decoded);
}
