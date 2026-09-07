//! CPU, memory, disk and temperature, for the four rings on the system card.

use sysinfo::{Components, Disks, System};

/// One ring: how full it is, and the reading written under it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reading {
    /// 0.0 to 1.0. The ring draws this; the label spells it out.
    pub fraction: f64,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub cpu: Reading,
    pub memory: Reading,
    pub disk: Reading,
    pub temperature: Reading,
}

/// Samples the machine. Held across ticks because CPU use is the difference
/// between two samples, and a fresh `System` has nothing to compare against.
pub struct Monitor {
    system: System,
    disks: Disks,
    components: Components,
}

impl Default for Monitor {
    fn default() -> Self {
        Self::new()
    }
}

impl Monitor {
    pub fn new() -> Self {
        Monitor {
            system: System::new(),
            disks: Disks::new_with_refreshed_list(),
            components: Components::new_with_refreshed_list(),
        }
    }

    pub fn sample(&mut self) -> Stats {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.disks.refresh(false);
        self.components.refresh(false);

        Stats {
            cpu: percentage(f64::from(self.system.global_cpu_usage()) / 100.0),
            memory: percentage(ratio(
                self.system.used_memory(),
                self.system.total_memory(),
            )),
            disk: percentage(self.root_disk_used()),
            temperature: self.hottest(),
        }
    }

    /// Whichever mount holds `/`, falling back to the largest one so a system
    /// that mounts root oddly still shows something true.
    fn root_disk_used(&self) -> f64 {
        let root = self
            .disks
            .list()
            .iter()
            .find(|disk| disk.mount_point() == std::path::Path::new("/"))
            .or_else(|| {
                self.disks
                    .list()
                    .iter()
                    .max_by_key(|disk| disk.total_space())
            });

        root.map_or(0.0, |disk| {
            ratio(
                disk.total_space().saturating_sub(disk.available_space()),
                disk.total_space(),
            )
        })
    }

    /// The hottest component, which is the one worth worrying about. Scaled
    /// against 100°C so the ring means the same thing as the others.
    fn hottest(&self) -> Reading {
        let hottest = self
            .components
            .list()
            .iter()
            .filter_map(sysinfo::Component::temperature)
            .filter(|value| value.is_finite())
            .fold(f64::NEG_INFINITY, |a, b| a.max(f64::from(b)));

        if hottest.is_finite() {
            Reading {
                fraction: (hottest / 100.0).clamp(0.0, 1.0),
                text: format!("{hottest:.0}°"),
            }
        } else {
            Reading {
                fraction: 0.0,
                text: String::from("--"),
            }
        }
    }
}

fn ratio(used: u64, total: u64) -> f64 {
    if total == 0 {
        return 0.0;
    }
    used as f64 / total as f64
}

fn percentage(fraction: f64) -> Reading {
    let fraction = fraction.clamp(0.0, 1.0);
    Reading {
        fraction,
        text: format!("{:.0}%", fraction * 100.0),
    }
}

#[cfg(test)]
mod tests;
