//! CPU range allocation for warm-pool cgroup pinning.

use crate::error::{ConfigError, FcError};

/// CPU range allocator for warm-pool slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarmPoolCpuAllocator {
    /// First host CPU id eligible for warm-pool slots.
    pub first_cpu: u32,
    /// Number of contiguous CPUs assigned to each slot.
    pub cpus_per_slot: u32,
}

pub(super) fn build_warm_pool_cpu_ranges(
    target_ready: usize,
    allocator: Option<WarmPoolCpuAllocator>,
) -> Result<Vec<String>, FcError> {
    let available_cpu_count = std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(1);
    build_warm_pool_cpu_ranges_for_available(target_ready, allocator, available_cpu_count)
}

fn build_warm_pool_cpu_ranges_for_available(
    target_ready: usize,
    allocator: Option<WarmPoolCpuAllocator>,
    available_cpu_count: u32,
) -> Result<Vec<String>, FcError> {
    let Some(allocator) = allocator else {
        return Ok(Vec::new());
    };
    if allocator.cpus_per_slot == 0 {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "warm_pool.cpu_allocator.cpus_per_slot",
            reason: "must be > 0".into(),
        }));
    }
    let target_ready = u32::try_from(target_ready).map_err(|_| {
        FcError::Config(ConfigError::InvalidValue {
            field: "warm_pool.target_ready",
            reason: "does not fit in u32".into(),
        })
    })?;
    let required = target_ready
        .checked_mul(allocator.cpus_per_slot)
        .ok_or_else(|| {
            FcError::Config(ConfigError::InvalidValue {
                field: "warm_pool.cpu_allocator",
                reason: "target_ready * cpus_per_slot overflowed".into(),
            })
        })?;
    let end_exclusive = allocator.first_cpu.checked_add(required).ok_or_else(|| {
        FcError::Config(ConfigError::InvalidValue {
            field: "warm_pool.cpu_allocator",
            reason: "first_cpu + requested CPUs overflowed".into(),
        })
    })?;
    if end_exclusive > available_cpu_count {
        return Err(FcError::Config(ConfigError::InvalidValue {
            field: "warm_pool.cpu_allocator",
            reason: format!(
                "requires CPUs {}..{} but host exposes {available_cpu_count} CPUs",
                allocator.first_cpu, end_exclusive
            ),
        }));
    }

    let mut ranges = Vec::with_capacity(target_ready as usize);
    for slot in 0..target_ready {
        let start = allocator.first_cpu + slot * allocator.cpus_per_slot;
        let end = start + allocator.cpus_per_slot - 1;
        if start == end {
            ranges.push(start.to_string());
        } else {
            ranges.push(format!("{start}-{end}"));
        }
    }
    Ok(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_disjoint_slot_ranges() {
        let ranges = build_warm_pool_cpu_ranges_for_available(
            2,
            Some(WarmPoolCpuAllocator {
                first_cpu: 0,
                cpus_per_slot: 2,
            }),
            4,
        )
        .unwrap();

        assert_eq!(ranges, vec!["0-1", "2-3"]);
    }

    #[test]
    fn rejects_insufficient_host_cpus() {
        let err = build_warm_pool_cpu_ranges_for_available(
            2,
            Some(WarmPoolCpuAllocator {
                first_cpu: 1,
                cpus_per_slot: 2,
            }),
            4,
        )
        .expect_err("slot ranges must fit inside available CPUs");

        assert!(matches!(
            err,
            FcError::Config(ConfigError::InvalidValue {
                field: "warm_pool.cpu_allocator",
                ..
            })
        ));
    }
}
