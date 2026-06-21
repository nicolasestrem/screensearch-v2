# P1 Semantic Retrieval and Scale Baseline

Measured: 2026-06-21

## Reference machine

- Windows 11 Pro 10.0.26200
- AMD Ryzen 7 7800X3D, 8 cores / 16 logical processors
- 32 GB RAM
- Kingston SNV3S1000G NVMe system drive

## Ten-million-capture metadata benchmark

Reproducible command from the repository root:

```powershell
$env:SCREENSEARCH_RUN_SCALE_BENCH = '1'
cargo test -p screensearch-persistence --test synthetic_scale synthetic_ten_million_capture_index -- --ignored --nocapture
```

The benchmark creates 10,000,000 capture metadata rows through a recursive SQL seed and one shared synthetic asset reference. It intentionally does not create 10,000,000 image files.

Instrumented result:

| Measure | Result |
|---|---:|
| Rows | 10,000,000 |
| Insert time | 90.734 s |
| Insert throughput | 110,212 rows/s |
| Database size | 3,886,972,928 bytes (3.62 GiB) |
| Metadata query p50 | 38.1 µs |
| Metadata query p95 | 108.6 µs |
| Metadata query p99 | 136.7 µs |
| Test-process peak working set | 23,793,664 bytes (22.69 MiB) |
| Test-process CPU time | 88.516 s |
| Instrumented wall time | 92.037 s |

An independent preceding run also passed: 102.176 seconds, 97,870 rows/s, and 40.6/103.2/156.7 microsecond p50/p95/p99 queries.

## Populated Windows archive search

The ignored live-archive integration test used the locally cached real MiniLM model and a populated archive with 2,773 chunks. It embedded one query, ran hybrid retrieval 20 times, and asserted non-empty evidence, authorized/resolvable screenshots, populated application metadata, and positioned OCR bounds.

```powershell
$env:SCREENSEARCH_DATA_DIR = '<populated smoke archive>'
$env:SCREENSEARCH_SMOKE_QUERY = 'project'
cargo test -p screensearch-persistence --test live_windows_archive real_archive_returns_resolvable_visual_evidence -- --ignored --nocapture
```

| Measure | Result |
|---|---:|
| Query embedding | 232.78 ms |
| Hybrid retrieval p50 | 22.59 ms |
| Hybrid retrieval p95 | 27.69 ms |
| Hybrid retrieval p99 | 27.69 ms |
| Evidence hits | 8 |

The final post-refactor p95 hybrid retrieval result is comfortably below the one-second product target. Archives with at most 50,000 embeddings use deterministic exact cosine ordering; larger archives use the revision-filtered libSQL vector index.

## Scope and interpretation

- The scale result covers capture metadata/index growth, not screenshot-file growth or ten million OCR/vector records.
- The user disk budget applies to immutable captured image assets referenced by captures; it is not a database-size or model-cache quota.
- CPU and working-set figures describe the instrumented benchmark test process, not idle/active production capture. Long-duration capture, OCR queue, and model-worker resource measurements remain release-hardening work.
