//! Guest metrics request handler.

use std::fs;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use m80_proto::{
    write_frame, Envelope, GuestCpuMetrics, GuestMemMetrics, MetricsRequest, MetricsResponse,
    RawEnvelope, PAYLOAD_KIND_METRICS_REQUEST,
};

static REQUESTS_TOTAL: AtomicU64 = AtomicU64::new(0);
static ERRORS_TOTAL: AtomicU64 = AtomicU64::new(0);

pub(super) fn is_metrics_kind(kind: &str) -> bool {
    kind == PAYLOAD_KIND_METRICS_REQUEST
}

pub(super) fn record_request() {
    REQUESTS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn record_error() {
    ERRORS_TOTAL.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn handle_metrics<R, W>(
    raw: RawEnvelope,
    _reader: R,
    writer: &mut W,
) -> anyhow::Result<crate::connection::ConnectionOutcome>
where
    R: BufRead,
    W: Write,
{
    let request_id = raw.request_id.clone();
    if let Err(e) = raw.decode::<MetricsRequest>() {
        record_error();
        anyhow::bail!("malformed metrics_request payload: {e:#}");
    }
    let response = collect_metrics().inspect_err(|_| {
        record_error();
    })?;
    let out_env = match request_id {
        Some(request_id) => Envelope::with_request_id(response, request_id),
        None => Envelope::new(response),
    };
    write_frame(writer, &out_env)?;
    writer.flush()?;
    Ok(crate::connection::ConnectionOutcome::Continue)
}

fn collect_metrics() -> anyhow::Result<MetricsResponse> {
    collect_metrics_from_paths(Path::new("/proc/stat"), Path::new("/proc/meminfo"))
}

fn collect_metrics_from_paths(
    stat_path: &Path,
    meminfo_path: &Path,
) -> anyhow::Result<MetricsResponse> {
    let stat = fs::read_to_string(stat_path)?;
    let meminfo = fs::read_to_string(meminfo_path)?;
    Ok(MetricsResponse {
        cpu: parse_proc_stat(&stat)?,
        mem: parse_proc_meminfo(&meminfo)?,
        requests_total: REQUESTS_TOTAL.load(Ordering::Relaxed),
        errors_total: ERRORS_TOTAL.load(Ordering::Relaxed),
    })
}

fn parse_proc_stat(input: &str) -> anyhow::Result<GuestCpuMetrics> {
    let cpu_line = input
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| anyhow::anyhow!("missing aggregate cpu line in /proc/stat"))?;
    let mut fields = cpu_line.split_whitespace();
    let label = fields.next().unwrap_or_default();
    if label != "cpu" {
        anyhow::bail!("unexpected aggregate cpu label: {label}");
    }
    let mut ticks = [0u64; 10];
    for (idx, slot) in ticks.iter_mut().enumerate() {
        *slot = fields
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing cpu tick field {idx}"))?
            .parse()?;
    }
    let total_ticks = ticks.iter().copied().sum();
    Ok(GuestCpuMetrics {
        user_ticks: ticks[0],
        nice_ticks: ticks[1],
        system_ticks: ticks[2],
        idle_ticks: ticks[3],
        iowait_ticks: ticks[4],
        irq_ticks: ticks[5],
        softirq_ticks: ticks[6],
        steal_ticks: ticks[7],
        guest_ticks: ticks[8],
        guest_nice_ticks: ticks[9],
        total_ticks,
    })
}

fn parse_proc_meminfo(input: &str) -> anyhow::Result<GuestMemMetrics> {
    Ok(GuestMemMetrics {
        mem_total_bytes: meminfo_kib(input, "MemTotal")? * 1024,
        mem_available_bytes: meminfo_kib(input, "MemAvailable")? * 1024,
        mem_free_bytes: meminfo_kib(input, "MemFree")? * 1024,
        buffers_bytes: meminfo_kib(input, "Buffers")? * 1024,
        cached_bytes: meminfo_kib(input, "Cached")? * 1024,
        swap_total_bytes: meminfo_kib(input, "SwapTotal")? * 1024,
        swap_free_bytes: meminfo_kib(input, "SwapFree")? * 1024,
    })
}

fn meminfo_kib(input: &str, key: &str) -> anyhow::Result<u64> {
    let prefix = format!("{key}:");
    let line = input
        .lines()
        .find(|line| line.starts_with(&prefix))
        .ok_or_else(|| anyhow::anyhow!("missing {key} in /proc/meminfo"))?;
    let mut fields = line.split_whitespace();
    let label = fields.next().unwrap_or_default();
    if label != prefix {
        anyhow::bail!("unexpected meminfo label for {key}: {label}");
    }
    fields
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing value for {key}"))?
        .parse()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_stat_parser_reports_total_ticks() {
        let cpu = parse_proc_stat("cpu  1 2 3 4 5 6 7 8 9 10\ncpu0 1 2 3 4\n").unwrap();

        assert_eq!(cpu.user_ticks, 1);
        assert_eq!(cpu.system_ticks, 3);
        assert_eq!(cpu.idle_ticks, 4);
        assert_eq!(cpu.total_ticks, 55);
    }

    #[test]
    fn proc_meminfo_parser_reports_bytes() {
        let mem = parse_proc_meminfo(
            "MemTotal:       1024 kB\n\
             MemFree:         128 kB\n\
             MemAvailable:    512 kB\n\
             Buffers:          16 kB\n\
             Cached:           32 kB\n\
             SwapTotal:        64 kB\n\
             SwapFree:          8 kB\n",
        )
        .unwrap();

        assert_eq!(mem.mem_total_bytes, 1024 * 1024);
        assert_eq!(mem.mem_available_bytes, 512 * 1024);
        assert_eq!(mem.cached_bytes, 32 * 1024);
        assert_eq!(mem.swap_free_bytes, 8 * 1024);
    }
}
