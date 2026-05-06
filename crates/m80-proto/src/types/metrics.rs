//! Guest metrics payloads carried over the host↔guest wire.

use serde::{Deserialize, Serialize};

use crate::types::Payload;

/// Wire `kind` for [`MetricsRequest`].
pub const PAYLOAD_KIND_METRICS_REQUEST: &str = "metrics_request";
/// Wire `kind` for [`MetricsResponse`].
pub const PAYLOAD_KIND_METRICS_RESPONSE: &str = "metrics_response";

/// Request guest-side metrics from `m80-guestd`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsRequest {}

/// Fixed-shape guest metrics response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricsResponse {
    /// CPU counters read from `/proc/stat`.
    pub cpu: GuestCpuMetrics,
    /// Memory gauges read from `/proc/meminfo`.
    pub mem: GuestMemMetrics,
    /// Guestd request frames accepted since daemon start.
    pub requests_total: u64,
    /// Guestd request frames that produced a handler-level error since daemon start.
    pub errors_total: u64,
}

/// Guest CPU counters in Linux clock ticks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestCpuMetrics {
    /// User-mode ticks.
    pub user_ticks: u64,
    /// Niced user-mode ticks.
    pub nice_ticks: u64,
    /// Kernel-mode ticks.
    pub system_ticks: u64,
    /// Idle ticks.
    pub idle_ticks: u64,
    /// I/O wait ticks.
    pub iowait_ticks: u64,
    /// IRQ ticks.
    pub irq_ticks: u64,
    /// Soft IRQ ticks.
    pub softirq_ticks: u64,
    /// Stolen time ticks.
    pub steal_ticks: u64,
    /// Guest ticks.
    pub guest_ticks: u64,
    /// Niced guest ticks.
    pub guest_nice_ticks: u64,
    /// Sum of every reported CPU tick field.
    pub total_ticks: u64,
}

/// Guest memory gauges in bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestMemMetrics {
    /// Total guest memory.
    pub mem_total_bytes: u64,
    /// Kernel estimate of memory available without swapping.
    pub mem_available_bytes: u64,
    /// Free guest memory.
    pub mem_free_bytes: u64,
    /// Buffer cache bytes.
    pub buffers_bytes: u64,
    /// Page cache bytes.
    pub cached_bytes: u64,
    /// Total swap bytes.
    pub swap_total_bytes: u64,
    /// Free swap bytes.
    pub swap_free_bytes: u64,
}

impl Payload for MetricsRequest {
    const KIND: &'static str = PAYLOAD_KIND_METRICS_REQUEST;
}

impl Payload for MetricsResponse {
    const KIND: &'static str = PAYLOAD_KIND_METRICS_RESPONSE;
}
