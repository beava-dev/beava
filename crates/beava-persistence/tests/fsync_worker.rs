//! Group-commit fsync worker + watermark fanout tests.

use beava_persistence::{WalReader, WalSink, WalSinkConfig};
use std::time::{Duration, Instant};

fn config_for_test(dir: std::path::PathBuf) -> WalSinkConfig {
    WalSinkConfig {
        dir,
        initial_start_lsn: 1,
        initial_registry_version: 1,
        fsync_interval_ms: 2,
        fsync_bytes: 1 << 20,
        segment_bytes: 1 << 20, // 1 MiB — small but enough to keep one segment for these tests
        // Pin to PerEvent so each `append_event` resolves only after fsync;
        // these tests assert ACK-after-fsync semantics.
        sync_mode: beava_persistence::SyncMode::PerEvent,
    }
}

fn find_segment(dir: &std::path::Path) -> std::path::PathBuf {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("wal-") && s.ends_with(".log"))
                .unwrap_or(false)
        })
        .expect("at least one segment")
}

#[tokio::test(flavor = "current_thread")]
async fn append_returns_durable_lsn() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, handle) = WalSink::spawn(config_for_test(dir.path().to_path_buf())).unwrap();

    let lsn = sink.append_event(b"x".to_vec()).await.unwrap();
    assert_eq!(lsn, 1);

    sink.shutdown().await.unwrap();
    handle.await.unwrap();

    let seg = find_segment(dir.path());
    let r = WalReader::open(&seg).unwrap();
    let recs: Vec<_> = r.collect::<Result<_, _>>().unwrap();
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].lsn, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn append_record_at_least_raises_assigned_lsn() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, handle) = WalSink::spawn(config_for_test(dir.path().to_path_buf())).unwrap();

    let lsn = sink
        .append_record_at_least(
            beava_persistence::RecordType::RegistryBump,
            b"x".to_vec(),
            100,
        )
        .await
        .unwrap();
    assert_eq!(lsn, 100);

    let next = sink.append_event(b"p".to_vec()).await.unwrap();
    assert_eq!(next, 101);

    sink.shutdown().await.unwrap();
    handle.await.unwrap();

    let seg = find_segment(dir.path());
    let r = WalReader::open(&seg).unwrap();
    let recs: Vec<_> = r.collect::<Result<_, _>>().unwrap();
    assert_eq!(recs.len(), 2);
    assert_eq!(recs[0].lsn, 100);
    assert_eq!(recs[1].lsn, 101);
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_appends_get_distinct_lsns() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, handle) = WalSink::spawn(config_for_test(dir.path().to_path_buf())).unwrap();

    let mut tasks = Vec::new();
    for _ in 0..10 {
        let s = sink.clone();
        tasks.push(tokio::spawn(
            async move { s.append_event(b"p".to_vec()).await },
        ));
    }

    let mut lsns = Vec::new();
    for t in tasks {
        lsns.push(t.await.unwrap().unwrap());
    }
    lsns.sort_unstable();
    lsns.dedup();
    assert_eq!(lsns.len(), 10, "all distinct");

    sink.shutdown().await.unwrap();
    handle.await.unwrap();

    let seg = find_segment(dir.path());
    let r = WalReader::open(&seg).unwrap();
    let recs: Vec<_> = r.collect::<Result<_, _>>().unwrap();
    assert_eq!(recs.len(), 10);
}

#[tokio::test(flavor = "current_thread")]
async fn durable_lsn_watermark_monotonic() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, handle) = WalSink::spawn(config_for_test(dir.path().to_path_buf())).unwrap();

    let mut last = 0u64;
    for _ in 0..5 {
        let lsn = sink.append_event(b"p".to_vec()).await.unwrap();
        let durable = sink.durable_lsn();
        assert!(durable >= lsn, "watermark {durable} should be >= lsn {lsn}");
        assert!(lsn > last);
        last = lsn;
    }

    sink.shutdown().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn forced_fsync_interval() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config_for_test(dir.path().to_path_buf());
    cfg.fsync_interval_ms = 500;
    cfg.fsync_bytes = u64::MAX;
    let (sink, handle) = WalSink::spawn(cfg).unwrap();

    let t0 = Instant::now();
    let lsn = sink.append_event(b"x".to_vec()).await.unwrap();
    let elapsed = t0.elapsed();

    assert_eq!(lsn, 1);
    assert!(
        elapsed >= Duration::from_millis(400),
        "expected >= ~500ms coalesce, got {:?}",
        elapsed
    );
    assert!(
        elapsed < Duration::from_millis(2000),
        "expected < ~2s upper bound, got {:?}",
        elapsed
    );

    sink.shutdown().await.unwrap();
    handle.await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_flushes_pending() {
    let dir = tempfile::tempdir().unwrap();
    let (sink, handle) = WalSink::spawn(config_for_test(dir.path().to_path_buf())).unwrap();

    // Fire 5 appends concurrently; some may be mid-flight during shutdown.
    let sink_c = sink.clone();
    let tasks: Vec<_> = (0..5)
        .map(|_| {
            let s = sink_c.clone();
            tokio::spawn(async move { s.append_event(b"p".to_vec()).await })
        })
        .collect();

    for t in tasks {
        t.await.unwrap().unwrap();
    }

    sink.shutdown().await.unwrap();
    handle.await.unwrap();

    let seg = find_segment(dir.path());
    let r = WalReader::open(&seg).unwrap();
    let recs: Vec<_> = r.collect::<Result<_, _>>().unwrap();
    assert_eq!(recs.len(), 5);
}
