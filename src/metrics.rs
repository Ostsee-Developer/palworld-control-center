use sysinfo::{Disks, System};

pub struct SystemMetrics {
    system: System,
    disks: Disks,
    demo: bool,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub disk_percent: f64,
    pub memory_used_gib: f64,
    pub memory_total_gib: f64,
    pub disk_free_gib: f64,
}

impl SystemMetrics {
    pub fn new(demo: bool) -> Self {
        Self {
            system: System::new_all(),
            disks: Disks::new_with_refreshed_list(),
            demo,
            cpu_percent: 0.0,
            memory_percent: 0.0,
            disk_percent: 0.0,
            memory_used_gib: 0.0,
            memory_total_gib: 0.0,
            disk_free_gib: 0.0,
        }
    }

    pub fn refresh(&mut self) {
        if self.demo {
            self.cpu_percent = 38.0;
            self.memory_percent = 64.0;
            self.disk_percent = 42.0;
            self.memory_used_gib = 10.3;
            self.memory_total_gib = 16.0;
            self.disk_free_gib = 287.4;
            return;
        }

        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.disks.refresh(true);

        self.cpu_percent = f64::from(self.system.global_cpu_usage()).clamp(0.0, 100.0);
        let total_memory = self.system.total_memory();
        let used_memory = self.system.used_memory();
        self.memory_percent = percentage(used_memory, total_memory);
        self.memory_used_gib = bytes_to_gib(used_memory);
        self.memory_total_gib = bytes_to_gib(total_memory);

        let (total_disk, available_disk) =
            self.disks
                .list()
                .iter()
                .fold((0_u64, 0_u64), |(total, available), disk| {
                    (
                        total.saturating_add(disk.total_space()),
                        available.saturating_add(disk.available_space()),
                    )
                });
        self.disk_percent = percentage(total_disk.saturating_sub(available_disk), total_disk);
        self.disk_free_gib = bytes_to_gib(available_disk);
    }
}
fn percentage(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
    }
}

fn bytes_to_gib(value: u64) -> f64 {
    value as f64 / 1_073_741_824.0
}

#[cfg(test)]
mod tests {
    use super::percentage;

    #[test]
    fn percentage_handles_zero_and_clamps() {
        assert_eq!(percentage(1, 0), 0.0);
        assert_eq!(percentage(25, 100), 25.0);
        assert_eq!(percentage(125, 100), 100.0);
    }
}
