//! Prometheus metrics export
//!
//! This module provides functions to export metrics in Prometheus text format.

use std::fmt::Write;

use crate::tracing::metrics::Metrics;

/// Export metrics in Prometheus text format
pub fn export_prometheus(metrics: &Metrics) -> String {
    let mut output = String::new();

    // Export counters
    for (name, value) in metrics.all_counters() {
        let _ = writeln!(&mut output, "# TYPE {name} counter\n{name} {value}");
    }

    // Export gauges
    for (name, value) in metrics.all_gauges() {
        let _ = writeln!(&mut output, "# TYPE {name} gauge\n{name} {value}");
    }

    // Export histograms
    for (name, stats) in metrics.all_histograms() {
        let _ = writeln!(&mut output, "# TYPE {name} histogram");
        let _ = writeln!(&mut output, "{name}_count {}", stats.count);
        let _ = writeln!(&mut output, "{name}_sum {}", stats.sum);

        // Export bucket values (simplified - using percentiles as buckets)
        let _ = writeln!(
            &mut output,
            "{name}_bucket{{le=\"{}\"}} {}",
            stats.p50,
            stats.count / 2
        );
        let _ = writeln!(
            &mut output,
            "{name}_bucket{{le=\"{}\"}} {}",
            stats.p90,
            (stats.count * 9) / 10
        );
        let _ = writeln!(
            &mut output,
            "{name}_bucket{{le=\"{}\"}} {}",
            stats.p95,
            (stats.count * 95) / 100
        );
        let _ = writeln!(
            &mut output,
            "{name}_bucket{{le=\"{}\"}} {}",
            stats.p99,
            (stats.count * 99) / 100
        );
        let _ = writeln!(&mut output, "{name}_bucket{{le=\"+Inf\"}} {}", stats.count);
    }

    output
}

/// Export global metrics in Prometheus format
pub fn prometheus_format() -> String {
    export_prometheus(crate::tracing::metrics::global())
}

/// Alias for export_prometheus
pub fn to_prometheus_format(metrics: &Metrics) -> String {
    export_prometheus(metrics)
}

/// Format a Push Gateway URL
pub fn format_push_gateway_url(base_url: &str, job: &str, instance: Option<&str>) -> String {
    let mut url = format!("{}/metrics/job/{}", base_url.trim_end_matches('/'), job);
    if let Some(inst) = instance {
        url.push_str(&format!("/instance/{inst}"));
    }
    url
}

/// Create a JSON snapshot of metrics for logging
pub fn create_metrics_snapshot(metrics: &Metrics) -> serde_json::Value {
    serde_json::json!({
        "counters": metrics.all_counters(),
        "gauges": metrics.all_gauges(),
        "histograms": metrics.all_histograms().into_iter().map(|(k, v)| {
            (k, serde_json::json!({
                "count": v.count,
                "sum": v.sum,
                "mean": v.mean,
                "min": v.min,
                "max": v.max,
                "p50": v.p50,
                "p90": v.p90,
                "p95": v.p95,
                "p99": v.p99,
            }))
        }).collect::<serde_json::Map<_, _>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tracing::metrics::Metrics;

    #[test]
    fn test_export_prometheus() {
        let metrics = Metrics::new();

        // Add some test metrics
        let counter = metrics.counter("test_counter");
        counter.add(42);

        let gauge = metrics.gauge("test_gauge");
        gauge.set(100);

        let histogram = metrics.histogram("test_histogram");
        histogram.observe(1.0);
        histogram.observe(2.0);
        histogram.observe(3.0);

        let output = export_prometheus(&metrics);

        // Check counter
        assert!(output.contains("# TYPE test_counter counter"));
        assert!(output.contains("test_counter 42"));

        // Check gauge
        assert!(output.contains("# TYPE test_gauge gauge"));
        assert!(output.contains("test_gauge 100"));

        // Check histogram
        assert!(output.contains("# TYPE test_histogram histogram"));
        assert!(output.contains("test_histogram_count 3"));
        assert!(output.contains("test_histogram_sum 6"));
    }

    #[test]
    fn test_format_push_gateway_url() {
        assert_eq!(
            format_push_gateway_url("http://localhost:9091", "my_job", None),
            "http://localhost:9091/metrics/job/my_job"
        );

        assert_eq!(
            format_push_gateway_url("http://localhost:9091/", "my_job", Some("instance1")),
            "http://localhost:9091/metrics/job/my_job/instance/instance1"
        );
    }

    #[test]
    fn test_create_metrics_snapshot() {
        let metrics = Metrics::new();
        metrics.counter("requests").add(10);
        metrics.gauge("connections").set(5);

        let snapshot = create_metrics_snapshot(&metrics);

        assert_eq!(snapshot["counters"]["requests"], 10);
        assert_eq!(snapshot["gauges"]["connections"], 5);
    }
}
