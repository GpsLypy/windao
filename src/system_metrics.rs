use std::{collections::HashMap, time::Duration};

#[derive(Clone, Copy)]
pub struct Memory {
    pub used: u64,
    pub total: u64,
}

impl Memory {
    pub fn percent(self) -> f32 {
        self.used as f32 / self.total as f32 * 100.0
    }
}

#[derive(Clone, Copy, Default)]
pub struct Traffic {
    pub received: f64,
    pub sent: f64,
}

#[derive(Clone, Copy, Default)]
pub struct Sample {
    pub cpu_percent: Option<f32>,
    pub memory: Option<Memory>,
    pub traffic: Option<Traffic>,
}

#[derive(Clone, Copy)]
struct CpuTimes {
    idle: u64,
    kernel: u64,
    user: u64,
}

impl CpuTimes {
    fn usage_since(self, previous: Self) -> Option<f32> {
        let total = self
            .kernel
            .checked_sub(previous.kernel)?
            .checked_add(self.user.checked_sub(previous.user)?)?;
        let idle = self.idle.checked_sub(previous.idle)?;
        let busy = total.checked_sub(idle)?;
        (total > 0).then(|| busy as f32 / total as f32 * 100.0)
    }
}

#[derive(Clone, Copy)]
struct Octets {
    received: u64,
    sent: u64,
}

type Interfaces = HashMap<u64, Octets>;

fn traffic_since(current: &Interfaces, previous: &Interfaces, interval: Duration) -> Traffic {
    let seconds = interval.as_secs_f64();
    current
        .iter()
        .filter_map(|(id, counters)| {
            let previous = previous.get(id)?;
            Some((
                counters.received.checked_sub(previous.received)?,
                counters.sent.checked_sub(previous.sent)?,
            ))
        })
        .fold(Traffic::default(), |mut traffic, (received, sent)| {
            traffic.received += received as f64 / seconds;
            traffic.sent += sent as f64 / seconds;
            traffic
        })
}

#[cfg(windows)]
pub use windows::Sampler;

#[cfg(windows)]
mod windows {
    use std::{ptr, slice, time::Instant};

    use windows_sys::Win32::{
        Foundation::{FILETIME, NO_ERROR},
        NetworkManagement::{
            IpHelper::{
                FreeMibTable, GetIfTable2, IF_TYPE_SOFTWARE_LOOPBACK, IF_TYPE_TUNNEL, MIB_IF_ROW2,
                MIB_IF_TABLE2,
            },
            Ndis::IfOperStatusUp,
        },
        System::{
            SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
            Threading::GetSystemTimes,
        },
    };

    use super::{CpuTimes, Interfaces, Memory, Octets, Sample, traffic_since};

    #[derive(Default)]
    pub struct Sampler {
        previous_cpu: Option<CpuTimes>,
        previous_network: Option<(Instant, Interfaces)>,
    }

    impl Sampler {
        pub fn poll(&mut self, now: Instant) -> Sample {
            let cpu = cpu_times();
            let cpu_percent = cpu
                .zip(self.previous_cpu)
                .and_then(|(current, previous)| current.usage_since(previous));
            self.previous_cpu = cpu;

            let network = network_counters();
            let traffic = network.as_ref().zip(self.previous_network.as_ref()).map(
                |(current, (previous_time, previous))| {
                    traffic_since(current, previous, now.duration_since(*previous_time))
                },
            );
            self.previous_network = network.map(|counters| (now, counters));

            Sample {
                cpu_percent,
                memory: physical_memory(),
                traffic,
            }
        }
    }

    fn cpu_times() -> Option<CpuTimes> {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // The kernel writes three cumulative FILETIME values to these outputs.
        let success = unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) };

        fn ticks(time: FILETIME) -> u64 {
            (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
        }

        (success != 0).then(|| CpuTimes {
            idle: ticks(idle),
            kernel: ticks(kernel),
            user: ticks(user),
        })
    }

    fn physical_memory() -> Option<Memory> {
        let mut info = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // Windows reads dwLength and fills the fields on success.
        let success = unsafe { GlobalMemoryStatusEx(&mut info) };
        (success != 0 && info.ullTotalPhys > 0).then(|| Memory {
            used: info.ullTotalPhys.saturating_sub(info.ullAvailPhys),
            total: info.ullTotalPhys,
        })
    }

    struct InterfaceTable(*mut MIB_IF_TABLE2);

    impl InterfaceTable {
        fn rows(&self) -> &[MIB_IF_ROW2] {
            // Windows allocates NumEntries rows at Table, whose header includes padding.
            unsafe {
                slice::from_raw_parts(
                    ptr::addr_of!((*self.0).Table).cast(),
                    (*self.0).NumEntries as usize,
                )
            }
        }
    }

    impl Drop for InterfaceTable {
        fn drop(&mut self) {
            // The pointer came from GetIfTable2 and this guard owns the allocation.
            unsafe { FreeMibTable(self.0.cast()) };
        }
    }

    fn network_counters() -> Option<Interfaces> {
        let mut table = ptr::null_mut();
        // This out pointer receives a Windows allocation on success.
        if unsafe { GetIfTable2(&mut table) } != NO_ERROR || table.is_null() {
            return None;
        }
        let table = InterfaceTable(table);
        Some(
            table
                .rows()
                .iter()
                .filter(|row| {
                    row.OperStatus == IfOperStatusUp
                        && !matches!(row.Type, IF_TYPE_SOFTWARE_LOOPBACK | IF_TYPE_TUNNEL)
                })
                .map(|row| {
                    // The union's Value field is the numeric interface identifier (NET_LUID).
                    (
                        unsafe { row.InterfaceLuid.Value },
                        Octets {
                            received: row.InOctets,
                            sent: row.OutOctets,
                        },
                    )
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_excludes_idle_from_the_combined_kernel_and_user_time() {
        let previous = CpuTimes {
            idle: 100,
            kernel: 130,
            user: 30,
        };
        let current = CpuTimes {
            idle: 160,
            kernel: 210,
            user: 50,
        };
        assert_eq!(current.usage_since(previous), Some(40.0));
        assert_eq!(current.usage_since(current), None);
        assert_eq!(previous.usage_since(current), None);
    }

    #[test]
    fn network_rate_ignores_counter_resets_and_interface_changes() {
        let previous = Interfaces::from([
            (
                1,
                Octets {
                    received: 1000,
                    sent: 1000,
                },
            ),
            (
                2,
                Octets {
                    received: 1000,
                    sent: 1000,
                },
            ),
            (
                3,
                Octets {
                    received: 1000,
                    sent: 1000,
                },
            ),
        ]);
        let current = Interfaces::from([
            (
                1,
                Octets {
                    received: 1600,
                    sent: 1020,
                },
            ),
            (
                2,
                Octets {
                    received: 1,
                    sent: 1,
                },
            ),
            (
                4,
                Octets {
                    received: 10_000,
                    sent: 10_000,
                },
            ),
        ]);
        let traffic = traffic_since(&current, &previous, Duration::from_millis(2000));
        assert_eq!((traffic.received, traffic.sent), (300.0, 10.0));
    }

    #[test]
    fn memory_load_uses_physical_capacity() {
        assert_eq!(Memory { used: 3, total: 4 }.percent(), 75.0);
    }
}
