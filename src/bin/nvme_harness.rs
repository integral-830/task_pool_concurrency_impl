use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Semaphore;

const FILE_SIZE: u64 = 4 * 1024 * 1024 * 1024;
const OPERATIONS: usize = 100_000;

const BLOCK_SIZES: [usize; 3] = [512, 4096, 64 * 1024];
const QUEUE_DEPTHS: [usize; 4] = [1, 4, 16, 64];

#[derive(Clone, Copy)]
enum AccessPattern {
    Sequential,
    Random,
}

struct BenchmarkResult {
    pattern: &'static str,
    block_size: usize,
    queue_depth: usize,
    operations: usize,
    bytes: u64,
    elapsed: Duration,
}

impl BenchmarkResult {
    fn iops(&self) -> f64 {
        self.operations as f64 / self.elapsed.as_secs_f64()
    }

    fn throughput_mib(&self) -> f64 {
        self.bytes as f64 / self.elapsed.as_secs_f64() / 1024.0 / 1024.0
    }

    fn avg_latency_us(&self) -> f64 {
        self.elapsed.as_secs_f64() * 1_000_000.0 / self.operations as f64
    }
}

fn random_offset(operation: usize, block_size: usize) -> u64 {
    let blocks = FILE_SIZE / block_size as u64;
    let block = (operation as u64 * 7919) % blocks;

    block * block_size as u64
}

fn sequential_offset(operation: usize, block_size: usize) -> u64 {
    let blocks = FILE_SIZE / block_size as u64;
    let block = operation as u64 % blocks;

    block * block_size as u64
}

fn offset(operation: usize, block_size: usize, pattern: AccessPattern) -> u64 {
    match pattern {
        AccessPattern::Sequential => sequential_offset(operation, block_size),
        AccessPattern::Random => random_offset(operation, block_size),
    }
}

fn read_block(
    path: &Path,
    operation: usize,
    block_size: usize,
    pattern: AccessPattern,
) -> io::Result<usize> {
    let mut file = OpenOptions::new().read(true).open(path)?;

    file.seek(SeekFrom::Start(offset(operation, block_size, pattern)))?;

    let mut buffer = vec![0u8; block_size];

    file.read_exact(&mut buffer)?;

    Ok(buffer.len())
}

async fn run_benchmark(
    path: &Path,
    pattern: AccessPattern,
    block_size: usize,
    queue_depth: usize,
) -> io::Result<BenchmarkResult> {
    let semaphore = Arc::new(Semaphore::new(queue_depth));

    let start = Instant::now();

    let mut tasks = Vec::with_capacity(OPERATIONS);

    for operation in 0..OPERATIONS {
        let permit = semaphore.clone().acquire_owned().await.unwrap();
        let path = path.to_owned();

        let task = tokio::task::spawn_blocking(move || {
            let result = read_block(&path, operation, block_size, pattern);

            drop(permit);

            result
        });

        tasks.push(task);
    }

    let mut bytes = 0u64;

    for task in tasks {
        bytes += task.await?? as u64;
    }

    Ok(BenchmarkResult {
        pattern: match pattern {
            AccessPattern::Sequential => "sequential",
            AccessPattern::Random => "random",
        },
        block_size,
        queue_depth,
        operations: OPERATIONS,
        bytes,
        elapsed: start.elapsed(),
    })
}

fn prepare_file(path: &Path) -> io::Result<()> {
    if path.exists() {
        let metadata = fs::metadata(path)?;

        if metadata.len() >= FILE_SIZE {
            return Ok(());
        }
    }

    let file = OpenOptions::new().create(true).write(true).open(path)?;

    file.set_len(FILE_SIZE)?;
    file.sync_all()?;

    Ok(())
}

fn format_block_size(size: usize) -> String {
    match size {
        512 => "512B".to_string(),
        4096 => "4KB".to_string(),
        65536 => "64KB".to_string(),
        _ => format!("{}B", size),
    }
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let path = Path::new("data/io-matrix.dat");

    fs::create_dir_all("data")?;

    println!(
        "Preparing {} GiB test file...",
        FILE_SIZE / 1024 / 1024 / 1024
    );

    prepare_file(path)?;

    println!("Operations per test: {}", OPERATIONS);
    println!("Total configurations: 24");
    println!();

    println!(
        "{:<12} {:>8} {:>8} {:>14} {:>14} {:>16}",
        "Pattern", "Block", "QD", "IOPS", "MiB/s", "Avg Latency"
    );

    println!("{}", "-".repeat(80));

    for pattern in [AccessPattern::Sequential, AccessPattern::Random] {
        for block_size in BLOCK_SIZES {
            for queue_depth in QUEUE_DEPTHS {
                let result = run_benchmark(path, pattern, block_size, queue_depth).await?;

                println!(
                    "{:<12} {:>8} {:>8} {:>14.0} {:>14.2} {:>13.2} us",
                    result.pattern,
                    format_block_size(result.block_size),
                    result.queue_depth,
                    result.iops(),
                    result.throughput_mib(),
                    result.avg_latency_us()
                );
            }
        }
    }

    Ok(())
}
