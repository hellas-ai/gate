//! Simple metrics collection for WASM and native environments
//!
//! This module provides basic metrics collection that works in both WASM
//! and native environments by using atomic operations and tracing events.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

/// A counter metric that can only increase
#[derive(Clone)]
pub struct Counter {
    name: String,
    value: Arc<AtomicU64>,
}

impl Counter {
    /// Create a new counter
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Increment the counter by 1
    pub fn increment(&self) {
        self.add(1);
    }

    /// Add a value to the counter
    pub fn add(&self, value: u64) {
        let old = self.value.fetch_add(value, Ordering::Relaxed);
        debug!(
            metric = "counter",
            name = %self.name,
            value = value,
            total = old + value,
            "Counter incremented"
        );
    }

    /// Get the current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// A gauge metric that can increase or decrease
#[derive(Clone)]
pub struct Gauge {
    name: String,
    value: Arc<AtomicI64>,
}

impl Gauge {
    /// Create a new gauge
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: Arc::new(AtomicI64::new(0)),
        }
    }

    /// Set the gauge value
    pub fn set(&self, value: i64) {
        self.value.store(value, Ordering::Relaxed);
        debug!(
            metric = "gauge",
            name = %self.name,
            value = value,
            "Gauge set"
        );
    }

    /// Increment the gauge by 1
    pub fn increment(&self) {
        let value = self.value.fetch_add(1, Ordering::Relaxed) + 1;
        debug!(
            metric = "gauge",
            name = %self.name,
            value = value,
            "Gauge incremented"
        );
    }

    /// Decrement the gauge by 1
    pub fn decrement(&self) {
        let value = self.value.fetch_sub(1, Ordering::Relaxed) - 1;
        debug!(
            metric = "gauge",
            name = %self.name,
            value = value,
            "Gauge decremented"
        );
    }

    /// Get the current value
    pub fn get(&self) -> i64 {
        self.value.load(Ordering::Relaxed)
    }
}

/// Statistics for a histogram
#[derive(Debug, Clone)]
pub struct HistogramStats {
    pub count: u64,
    pub sum: f64,
    pub mean: f64,
    pub min: f64,
    pub max: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
}

/// A histogram metric for recording durations
#[derive(Clone)]
pub struct Histogram {
    name: String,
    observations: Arc<RwLock<Vec<f64>>>,
}

impl Histogram {
    /// Create a new histogram
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            observations: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Record an observation
    pub fn observe(&self, value: f64) {
        if let Ok(mut observations) = self.observations.write() {
            observations.push(value);
            debug!(
                metric = "histogram",
                name = %self.name,
                value = value,
                "Histogram observation recorded"
            );
        }
    }

    /// Record a duration
    pub fn observe_duration(&self, duration: Duration) {
        self.observe(duration.as_secs_f64());
    }

    /// Get statistics for the histogram
    pub fn stats(&self) -> Option<HistogramStats> {
        let observations = self.observations.read().ok()?;
        if observations.is_empty() {
            return None;
        }

        let mut sorted = observations.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let count = sorted.len() as u64;
        let sum: f64 = sorted.iter().sum();
        let mean = sum / count as f64;
        let min = *sorted.first().unwrap();
        let max = *sorted.last().unwrap();

        let percentile = |p: f64| -> f64 {
            let index = ((count as f64 - 1.0) * p / 100.0) as usize;
            sorted[index]
        };

        Some(HistogramStats {
            count,
            sum,
            mean,
            min,
            max,
            p50: percentile(50.0),
            p90: percentile(90.0),
            p95: percentile(95.0),
            p99: percentile(99.0),
        })
    }

    /// Clear all observations
    pub fn clear(&self) {
        if let Ok(mut observations) = self.observations.write() {
            observations.clear();
        }
    }
}

/// Global metrics registry
pub struct Metrics {
    counters: RwLock<HashMap<String, Counter>>,
    gauges: RwLock<HashMap<String, Gauge>>,
    histograms: RwLock<HashMap<String, Histogram>>,
}

impl Metrics {
    /// Create a new metrics registry
    pub fn new() -> Self {
        Self {
            counters: RwLock::new(HashMap::new()),
            gauges: RwLock::new(HashMap::new()),
            histograms: RwLock::new(HashMap::new()),
        }
    }

    /// Get or create a counter
    pub fn counter(&self, name: &str) -> Counter {
        if let Ok(counters) = self.counters.read()
            && let Some(counter) = counters.get(name)
        {
            return counter.clone();
        }

        let counter = Counter::new(name);
        if let Ok(mut counters) = self.counters.write() {
            counters.insert(name.to_string(), counter.clone());
        }
        counter
    }

    /// Get or create a gauge
    pub fn gauge(&self, name: &str) -> Gauge {
        if let Ok(gauges) = self.gauges.read()
            && let Some(gauge) = gauges.get(name)
        {
            return gauge.clone();
        }

        let gauge = Gauge::new(name);
        if let Ok(mut gauges) = self.gauges.write() {
            gauges.insert(name.to_string(), gauge.clone());
        }
        gauge
    }

    /// Get or create a histogram
    pub fn histogram(&self, name: &str) -> Histogram {
        if let Ok(histograms) = self.histograms.read()
            && let Some(histogram) = histograms.get(name)
        {
            return histogram.clone();
        }

        let histogram = Histogram::new(name);
        if let Ok(mut histograms) = self.histograms.write() {
            histograms.insert(name.to_string(), histogram.clone());
        }
        histogram
    }

    /// Log all metrics
    pub fn log_all(&self) {
        info!("Logging all metrics");

        // Log counters
        if let Ok(counters) = self.counters.read() {
            for (name, counter) in counters.iter() {
                info!(
                    metric_type = "counter",
                    metric_name = %name,
                    value = counter.get(),
                    "Metric value"
                );
            }
        }

        // Log gauges
        if let Ok(gauges) = self.gauges.read() {
            for (name, gauge) in gauges.iter() {
                info!(
                    metric_type = "gauge",
                    metric_name = %name,
                    value = gauge.get(),
                    "Metric value"
                );
            }
        }

        // Log histograms
        if let Ok(histograms) = self.histograms.read() {
            for (name, histogram) in histograms.iter() {
                if let Some(stats) = histogram.stats() {
                    info!(
                        metric_type = "histogram",
                        metric_name = %name,
                        count = stats.count,
                        sum = stats.sum,
                        mean = stats.mean,
                        min = stats.min,
                        max = stats.max,
                        p50 = stats.p50,
                        p90 = stats.p90,
                        p95 = stats.p95,
                        p99 = stats.p99,
                        "Metric value"
                    );
                }
            }
        }
    }

    /// Get all counters
    pub fn all_counters(&self) -> HashMap<String, u64> {
        self.counters
            .read()
            .ok()
            .map(|counters| counters.iter().map(|(k, v)| (k.clone(), v.get())).collect())
            .unwrap_or_default()
    }

    /// Get all gauges
    pub fn all_gauges(&self) -> HashMap<String, i64> {
        self.gauges
            .read()
            .ok()
            .map(|gauges| gauges.iter().map(|(k, v)| (k.clone(), v.get())).collect())
            .unwrap_or_default()
    }

    /// Get all histogram stats
    pub fn all_histograms(&self) -> HashMap<String, HistogramStats> {
        self.histograms
            .read()
            .ok()
            .map(|histograms| {
                histograms
                    .iter()
                    .filter_map(|(k, v)| v.stats().map(|stats| (k.clone(), stats)))
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}

// Global metrics instance
static GLOBAL_METRICS: OnceLock<Metrics> = OnceLock::new();

/// Get the global metrics instance, initializing if needed
fn global_metrics() -> &'static Metrics {
    GLOBAL_METRICS.get_or_init(Metrics::new)
}

/// Get or create a global counter
pub fn counter(name: &str) -> Counter {
    global_metrics().counter(name)
}

/// Get or create a global gauge
pub fn gauge(name: &str) -> Gauge {
    global_metrics().gauge(name)
}

/// Get or create a global histogram
pub fn histogram(name: &str) -> Histogram {
    global_metrics().histogram(name)
}

/// Log all global metrics
pub fn log_all_metrics() {
    global_metrics().log_all();
}

/// Get the global metrics instance
pub fn global() -> &'static Metrics {
    global_metrics()
}

/// Timer guard for recording durations
pub struct Timer {
    histogram: Histogram,
    start: Instant,
}

impl Timer {
    /// Create a new timer
    pub fn new(histogram: Histogram) -> Self {
        Self {
            histogram,
            start: Instant::now(),
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        self.histogram.observe_duration(self.start.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let counter = Counter::new("test_counter");
        assert_eq!(counter.get(), 0);

        counter.increment();
        assert_eq!(counter.get(), 1);

        counter.add(5);
        assert_eq!(counter.get(), 6);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new("test_gauge");
        assert_eq!(gauge.get(), 0);

        gauge.set(10);
        assert_eq!(gauge.get(), 10);

        gauge.increment();
        assert_eq!(gauge.get(), 11);

        gauge.decrement();
        assert_eq!(gauge.get(), 10);
    }

    #[test]
    fn test_histogram() {
        let histogram = Histogram::new("test_histogram");

        histogram.observe(1.0);
        histogram.observe(2.0);
        histogram.observe(3.0);
        histogram.observe(4.0);
        histogram.observe(5.0);

        let stats = histogram.stats().unwrap();
        assert_eq!(stats.count, 5);
        assert_eq!(stats.sum, 15.0);
        assert_eq!(stats.mean, 3.0);
        assert_eq!(stats.min, 1.0);
        assert_eq!(stats.max, 5.0);
        assert_eq!(stats.p50, 3.0);
    }

    #[test]
    fn test_global_metrics() {
        let counter1 = counter("global_counter");
        let counter2 = counter("global_counter");

        counter1.increment();
        assert_eq!(counter2.get(), 1); // Same instance

        let gauge1 = gauge("global_gauge");
        let gauge2 = gauge("global_gauge");

        gauge1.set(42);
        assert_eq!(gauge2.get(), 42); // Same instance
    }
}
