//! Explicitly ignored scale harness for the ten-million-capture metadata target.

use std::time::{Duration, Instant};

use screensearch_persistence::LibSqlArchive;
use screensearch_ports::ArchiveRepository;
use tempfile::TempDir;

#[tokio::test]
#[ignore = "long-running benchmark; set SCREENSEARCH_RUN_SCALE_BENCH=1 and run explicitly"]
async fn synthetic_ten_million_capture_index() {
    assert_eq!(
        std::env::var("SCREENSEARCH_RUN_SCALE_BENCH").as_deref(),
        Ok("1"),
        "explicit opt-in is required"
    );
    let target = std::env::var("SCREENSEARCH_SCALE_ROWS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(10_000_000);
    let directory = TempDir::new().unwrap();
    let repository = LibSqlArchive::open(directory.path().join("scale.db"))
        .await
        .unwrap();
    repository.migrate().await.unwrap();

    let insertion_started = Instant::now();
    repository
        .seed_synthetic_capture_metadata(target)
        .await
        .unwrap();
    let insertion_elapsed = insertion_started.elapsed();
    let metrics = repository.storage_metrics().await.unwrap();
    assert_eq!(metrics.capture_count, target);

    let mut query_durations = repository
        .benchmark_capture_metadata_queries(100)
        .await
        .unwrap();
    query_durations.sort_unstable();
    let database_bytes = repository.database_size_bytes().await.unwrap();
    eprintln!(
        "rows={target} insert={insertion_elapsed:?} throughput={:.0}_rows/s db_bytes={database_bytes} query_p50={:?} query_p95={:?} query_p99={:?} logical_cpus={}",
        f64::from(u32::try_from(target).expect("scale target fits in u32"))
            / insertion_elapsed.as_secs_f64(),
        percentile(&query_durations, 50),
        percentile(&query_durations, 95),
        percentile(&query_durations, 99),
        std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
    );
}

fn percentile(samples: &[Duration], percentile: usize) -> Duration {
    let index = (samples.len().saturating_sub(1) * percentile) / 100;
    samples[index]
}
