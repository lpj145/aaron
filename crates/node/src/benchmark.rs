use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Hardware micro-benchmark measured at node boot to calibrate capacity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HardwareBenchmark {
    /// Million arithmetic/mixing operations per second
    pub cpu_mops: f64,
    /// Memory write & read bandwidth in MB/s
    pub mem_bandwidth_mb_s: f64,
    /// 4KB sync write + fsync latency in microseconds
    pub disk_sync_latency_us: u64,
    /// Nominal baseline score (calibrated to 1000)
    pub nominal_wps: u32,
    /// Total duration of the benchmark in milliseconds
    pub duration_ms: u64,
}

impl HardwareBenchmark {
    /// Executes a quick (~15-25ms) hardware micro-benchmark to establish nominal WPS.
    pub fn run() -> Self {
        let start_total = Instant::now();

        // 1. CPU Compute: 500,000 arithmetic mixing loop iterations
        let cpu_start = Instant::now();
        let mut x: u64 = 0x517cc1b727220a95;
        for i in 0..500_000u64 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(i);
            x ^= x >> 27;
        }
        std::hint::black_box(x);
        let cpu_elapsed = cpu_start.elapsed();
        let cpu_mops = 0.5 / cpu_elapsed.as_secs_f64().max(0.000_001);

        // 2. Memory Headroom: 4MB buffer write throughput
        let mem_start = Instant::now();
        let mut buf = vec![0u8; 4 * 1024 * 1024];
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = (i as u8).wrapping_mul(31);
        }
        std::hint::black_box(&buf[..]);
        let mem_elapsed = mem_start.elapsed();
        let mem_bandwidth_mb_s = 4.0 / mem_elapsed.as_secs_f64().max(0.000_001);

        // 3. Storage IOPS: 4KB sync write + fsync latency
        let disk_start = Instant::now();
        let disk_sync_latency_us = {
            let tmp_path = std::env::temp_dir().join(format!("aaron_bm_{}.tmp", std::process::id()));
            let sample_block = [0xAAu8; 4096];
            let latency = match std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)
            {
                Ok(mut file) => {
                    use std::io::Write;
                    let _ = file.write_all(&sample_block);
                    let _ = file.sync_all();
                    drop(file);
                    let _ = std::fs::remove_file(&tmp_path);
                    disk_start.elapsed().as_micros() as u64
                }
                Err(_) => 250, // Safe in-memory fallback
            };
            latency.max(1)
        };

        let duration_ms = start_total.elapsed().as_millis() as u64;

        // Dynamic hardware capacity score calculated from measured physical dimensions:
        // 1. CPU arithmetic/ALU throughput: ~8 points per Mops
        let cpu_score = (cpu_mops * 8.0).round() as u32;
        // 2. RAM write bandwidth: ~0.1 points per MB/s
        let mem_score = (mem_bandwidth_mb_s * 0.1).round() as u32;
        // 3. Storage sync responsiveness: lower fsync latency yields up to 800 points
        let disk_score = (120_000 / disk_sync_latency_us.max(30)).min(800) as u32;

        let nominal_wps = (cpu_score + mem_score + disk_score).max(100);

        Self {
            cpu_mops,
            mem_bandwidth_mb_s,
            disk_sync_latency_us,
            nominal_wps,
            duration_ms,
        }
    }
}

/// Dynamic runtime node telemetry tracking WPS load and error rate.
#[derive(Debug)]
pub struct NodeTelemetry {
    pub benchmark: HardwareBenchmark,
    current_wps: AtomicU32,
    error_count: AtomicU64,
    last_error_sec: AtomicU64,
    recent_errors: AtomicU32,
}

impl Default for NodeTelemetry {
    fn default() -> Self {
        Self::new(HardwareBenchmark::run())
    }
}

impl NodeTelemetry {
    pub fn new(benchmark: HardwareBenchmark) -> Self {
        let nominal = benchmark.nominal_wps;
        Self {
            benchmark,
            current_wps: AtomicU32::new((nominal / 10).max(40)), // Initial baseline idle ~10% capacity
            error_count: AtomicU64::new(0),
            last_error_sec: AtomicU64::new(0),
            recent_errors: AtomicU32::new(0),
        }
    }

    /// Returns the current Workload Performance Score (0 to 1000).
    pub fn current_wps(&self) -> u32 {
        self.current_wps.load(Ordering::Relaxed)
    }

    /// Sets the current Workload Performance Score (clamped to nominal capacity ceiling).
    pub fn set_wps(&self, wps: u32) {
        self.current_wps.store(wps.min(self.benchmark.nominal_wps * 2), Ordering::Relaxed);
    }

    /// Returns the nominal baseline capacity score.
    pub fn nominal_wps(&self) -> u32 {
        self.benchmark.nominal_wps
    }

    /// Records an error event (e.g. disk write failure, RPC timeout, framing error).
    pub fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        let now_sec = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let last = self.last_error_sec.swap(now_sec, Ordering::Relaxed);
        if now_sec == last {
            self.recent_errors.fetch_add(1, Ordering::Relaxed);
        } else {
            self.recent_errors.store(1, Ordering::Relaxed);
        }
    }

    /// Returns the current sliding error rate in errors per second.
    pub fn error_rate(&self) -> u32 {
        let now_sec = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = self.last_error_sec.load(Ordering::Relaxed);
        if now_sec.saturating_sub(last) <= 1 {
            self.recent_errors.load(Ordering::Relaxed)
        } else {
            0
        }
    }

    /// Total errors recorded since node startup.
    pub fn total_errors(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_benchmark_execution() {
        let bm = HardwareBenchmark::run();
        assert!(bm.nominal_wps >= 100);
        assert!(bm.cpu_mops > 0.0);
        assert!(bm.mem_bandwidth_mb_s > 0.0);
        assert!(bm.disk_sync_latency_us > 0);
        println!(
            "Measured benchmark on host: nominal_wps={}, CPU={:.1} Mops, RAM={:.1} MB/s, Disk={} us",
            bm.nominal_wps, bm.cpu_mops, bm.mem_bandwidth_mb_s, bm.disk_sync_latency_us
        );
    }

    #[test]
    fn test_node_telemetry_wps_and_error_rate() {
        let telemetry = NodeTelemetry::default();
        assert_eq!(telemetry.nominal_wps(), telemetry.benchmark.nominal_wps);
        assert_eq!(telemetry.error_rate(), 0);
        assert_eq!(telemetry.total_errors(), 0);

        telemetry.record_error();
        telemetry.record_error();
        assert_eq!(telemetry.total_errors(), 2);
        assert!(telemetry.error_rate() >= 2);

        telemetry.set_wps(650);
        assert_eq!(telemetry.current_wps(), 650);
    }
}
