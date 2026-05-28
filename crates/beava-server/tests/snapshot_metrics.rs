//! Snapshot instrumentation exposed on the admin /metrics endpoint.

#![cfg(feature = "testing")]

use beava_server::testing::{TestServer, TestServerBuilder};

async fn fetch_metrics(ts: &TestServer) -> String {
    reqwest::get(format!("{}/metrics", ts.admin_url()))
        .await
        .expect("/metrics request")
        .text()
        .await
        .expect("/metrics body")
}

fn scrape_metric_value<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(name) {
            match rest.chars().next() {
                Some(' ') | Some('\t') | Some('{') => {}
                _ => continue,
            }
            return trimmed.split_whitespace().last();
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn metrics_endpoint_exposes_last_snapshot_stats() {
    let ts = TestServerBuilder::new()
        .snapshot_interval_ms(60_000)
        .spawn()
        .await
        .expect("spawn");

    let initial = fetch_metrics(&ts).await;
    for help in [
        "# HELP beava_snapshot_last_duration_seconds",
        "# HELP beava_snapshot_last_bytes",
        "# HELP beava_snapshot_last_fsync_seconds",
    ] {
        assert!(
            initial.contains(help),
            "metrics body must contain `{help}`; got:\n{initial}"
        );
    }

    ts.force_snapshot_now().await.expect("force snapshot");

    let after = fetch_metrics(&ts).await;
    let bytes = scrape_metric_value(&after, "beava_snapshot_last_bytes")
        .expect("snapshot bytes metric")
        .parse::<u64>()
        .expect("snapshot bytes value");
    let duration = scrape_metric_value(&after, "beava_snapshot_last_duration_seconds")
        .expect("snapshot duration metric")
        .parse::<f64>()
        .expect("snapshot duration value");
    let fsync = scrape_metric_value(&after, "beava_snapshot_last_fsync_seconds")
        .expect("snapshot fsync metric")
        .parse::<f64>()
        .expect("snapshot fsync value");

    assert!(bytes > 0, "forced snapshot should report bytes > 0");
    assert!(duration.is_finite() && duration >= 0.0);
    assert!(fsync.is_finite() && fsync >= 0.0);

    ts.shutdown().await.ok();
}
