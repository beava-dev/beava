use std::{
    iter,
    net::SocketAddr,
    sync::{
        atomic::{self, AtomicBool},
        Arc,
    },
    time::{Duration, Instant},
};

use beava_core::wire::{decode_frame, encode_frame, Frame, CT_MSGPACK, OP_PING};
use bytes::{Bytes, BytesMut};
use futures::{stream::FuturesUnordered, StreamExt};
use hdrhistogram::Histogram;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use crate::{
    blast_shape::{self, build_pool, BlastShape, BlastShapeConfig},
    harness::production::load_pipeline,
};

// 3 significant decimals
const HIST_SIGFIGS: u8 = 3;

// 60 max RTT
const HIST_MAX_US: u64 = 60_000_000;

// 16Kb max frame size
const MAX_FRAME_BYTES: u32 = 1 << 14;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Server TCP address (wire-protocol port; default 8081)
    #[arg(long, default_value = "127.0.0.1:8081")]
    addr: SocketAddr,

    /// Number of concurrent TCP connections
    #[arg(long, default_value_t = 1)]
    connections: usize,

    /// Target requests per second across all connections combined (0 = max rate)
    #[arg(long, default_value_t = 0)]
    rps: u64,

    #[arg(long)]
    pipeline: Option<String>,

    #[arg(long, default_value_t = 1.0)]
    zipf_alpha: f64,

    #[arg(long, default_value_t = 1_000_000)]
    zipf_cardinality: u64,

    /// Measurement window duration (after warmup)
    #[arg(long, value_parser = humantime::parse_duration, default_value = "30s")]
    duration: Duration,

    /// Warmup period excluded from measurements
    #[arg(long, value_parser = humantime::parse_duration, default_value = "5s")]
    warmup: Duration,
}

impl Args {
    fn load_pipeline(&self) -> anyhow::Result<Option<Vec<Vec<Bytes>>>> {
        let Some(pipeline) = &self.pipeline else {
            return Ok(None);
        };

        tracing::info!(pipeline, "loading");
        let pipeline_cfg = load_pipeline(pipeline)?;
        let blast_shape_cfg = BlastShapeConfig {
            pipeline: &pipeline_cfg,
            event_names_for_mixed: &[],
            wire_format: blast_shape::WireFormat::Msgpack,
            seed: 69420,
        };

        let frames: Vec<_> = build_pool(
            BlastShape::Zipfian {
                alpha: self.zipf_alpha,
                cardinality: self.zipf_cardinality,
            },
            &blast_shape_cfg,
            1 << 14,
        )?
        .chunks(self.connections)
        .map(Vec::from)
        .collect();

        Ok(Some(frames))
    }

    pub fn exec(self) -> anyhow::Result<()> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_io()
            .enable_time()
            .build()?;

        runtime.block_on(run(self))
    }
}

async fn run(args: Args) -> Result<(), anyhow::Error> {
    tracing::info!(
        addr = args.addr.to_string(),
        connections = args.connections,
        rps = args.rps,
        warmup = ?args.warmup,
        duration = ?args.duration,
        "benchmarking with fixed schedule"
    );

    let frames_from_pipeline = args.load_pipeline()?;
    let end = Instant::now() + args.warmup + args.duration;
    let interval = (args.rps > 0)
        .then(|| Duration::from_secs_f64(args.connections as f64 / args.rps as f64))
        .filter(|d| !d.is_zero());

    let recording = Arc::new(AtomicBool::new(false));
    {
        let rec = recording.clone();
        let warmup = args.warmup;
        tokio::spawn(async move {
            tokio::time::sleep(warmup).await;
            rec.store(true, atomic::Ordering::Release);
            tracing::info!("warmup done!");
        });
    }

    let mut workers = FuturesUnordered::new();
    if let Some(frames) = frames_from_pipeline {
        for frames in frames {
            let worker = FixedScheduleWorker {
                remote_addr: args.addr,
                end,
                frames: frames.into_iter().cycle(),
                interval,
                recording: recording.clone(),
            };
            workers.push(tokio::spawn(worker.run()));
        }
    } else {
        for _ in 0..args.connections {
            let ping = {
                let mut bytes = BytesMut::new();
                let frame = Frame::new(OP_PING, CT_MSGPACK, Bytes::new());
                encode_frame(&frame, &mut bytes);
                bytes.freeze()
            };
            let worker = FixedScheduleWorker {
                remote_addr: args.addr,
                end,
                frames: iter::repeat_with(move || ping.clone()),
                interval,
                recording: recording.clone(),
            };
            workers.push(tokio::spawn(worker.run()));
        }
    }

    let mut samples = Histogram::<u64>::new_with_bounds(1, HIST_MAX_US, HIST_SIGFIGS)?;
    while let Some(worker) = workers.next().await {
        samples.add(worker??)?;
    }

    print_samples(samples, args.duration);
    Ok(())
}

struct FixedScheduleWorker<F> {
    remote_addr: SocketAddr,
    end: Instant,
    frames: F,
    interval: Option<Duration>,
    recording: Arc<AtomicBool>,
}

impl<F> FixedScheduleWorker<F> {
    async fn run(mut self) -> anyhow::Result<Histogram<u64>>
    where
        F: Iterator<Item = Bytes> + Send + 'static,
    {
        let mut hist = Histogram::<u64>::new_with_bounds(1, HIST_MAX_US, HIST_SIGFIGS)?;

        let stream = TcpStream::connect(self.remote_addr).await?;
        stream.set_nodelay(true)?;

        let local_addr = stream.local_addr()?;
        let (mut read_half, mut write_half) = tokio::io::split(stream);

        tracing::info!(addr = local_addr.to_string(), "connected");

        // The sender forwards the *scheduled* send time through this channel so
        // the receiver can compute latency that includes any queuing delay.
        let (ts_tx, mut ts_rx) = tokio::sync::mpsc::channel::<Instant>(4096);

        let sender = tokio::spawn(async move {
            let mut ticker = self.interval.map(|iv| {
                // MissedTickBehavior::Burst fires any overdue ticks immediately on
                // the next tick() call, preserving the fixed schedule
                let mut t = tokio::time::interval(iv);
                t.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
                t
            });
            while Instant::now() < self.end {
                let Some(frame) = self.frames.next() else {
                    break;
                };
                let scheduled = match ticker.as_mut() {
                    Some(t) => {
                        let ts = t.tick().await.into_std();
                        if ts >= self.end {
                            // Stop if the tick fired past the benchmark window.
                            break;
                        }
                        ts
                    }
                    None => Instant::now(),
                };

                if let Err(err) = ts_tx.send(scheduled).await {
                    tracing::error!(error = %err, "closed channel");
                    break;
                }
                if let Err(err) = write_half.write_all(&frame).await {
                    tracing::error!(error = %err, "failed frame write");
                    break;
                }
            }
            tracing::info!(addr = local_addr.to_string(), "finished writing");

            write_half
        });

        // For a single TCP stream, responses always arrive in the same order as
        // requests, so a simple FIFO match is correct and sufficient.
        let mut buf = BytesMut::with_capacity(8 * 1024);
        'outer: while let Some(scheduled) = ts_rx.recv().await {
            loop {
                match decode_frame(&mut buf, MAX_FRAME_BYTES) {
                    Ok(Some(_)) => {
                        break;
                    }
                    Ok(None) => {
                        if read_half.read_buf(&mut buf).await.unwrap_or(0) == 0 {
                            break 'outer;
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            if self.recording.load(atomic::Ordering::Acquire) {
                let us = (scheduled.elapsed().as_micros() as u64).clamp(1, HIST_MAX_US);
                hist.record(us).ok();
            }
        }
        tracing::info!(addr = local_addr.to_string(), "finished reading");

        sender.abort();
        match sender.await {
            Ok(mut write_half) => {
                write_half.shutdown().await.ok();
                tracing::info!(addr = local_addr.to_string(), "disconnected");
            }
            Err(err) => {
                if !err.is_cancelled() {
                    return Err(err.into());
                }
                tracing::info!(addr = local_addr.to_string(), "aborted");
            }
        }

        Ok(hist)
    }
}

fn print_samples(samples: Histogram<u64>, bench_duration: Duration) {
    let count = samples.len();
    let throughput = count as f64 / bench_duration.as_secs_f64();
    println!("samples:    {count}");
    println!("throughput: {throughput:.0} req/s");
    println!("min:        {:.3} ms", samples.min() as f64 / 1_000.0);
    println!("mean:       {:.3} ms", samples.mean() / 1_000.0);
    println!(
        "p50:        {:.3} ms",
        samples.value_at_quantile(0.50) as f64 / 1_000.0
    );
    println!(
        "p90:        {:.3} ms",
        samples.value_at_quantile(0.90) as f64 / 1_000.0
    );
    println!(
        "p99:        {:.3} ms",
        samples.value_at_quantile(0.99) as f64 / 1_000.0
    );
    println!(
        "p99.9:      {:.3} ms",
        samples.value_at_quantile(0.999) as f64 / 1_000.0
    );
    println!("max:        {:.3} ms", samples.max() as f64 / 1_000.0);
}
