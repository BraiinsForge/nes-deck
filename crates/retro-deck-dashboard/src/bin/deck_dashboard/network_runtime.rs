//! Read-only network status lifecycle for dashboard presentation.

use std::time::Duration;

use retro_deck_dashboard::{
    NetworkStatusConfig, NetworkStatusPoll, NetworkStatusWorker, NetworkStatusWorkerReport, Screen,
};

use super::{APPLICATION, DashboardRuntime};

const WIRELESS_INTERFACE: &str = "wlan0";
const WIREGUARD_INTERFACE: &str = "wg0";
const WIFI_STATUS: &str = "/var/run/deck-wifi/status";
const NETWORK_REFRESH: Duration = Duration::from_secs(2);

impl DashboardRuntime {
    pub(super) fn service_network_status(&mut self) {
        let mut disconnected = false;
        loop {
            let Some(worker) = self.network_worker.as_ref() else {
                return;
            };
            match worker.try_update() {
                NetworkStatusPoll::Updated(status) => {
                    if self.network_failure_reported {
                        eprintln!("{APPLICATION}: read-only network status recovered");
                        self.network_failure_reported = false;
                    }
                    if status != self.network_status {
                        self.network_status = status;
                        self.dirty |= self.model.screen() == Screen::Settings;
                    }
                }
                NetworkStatusPoll::Failed(error) => {
                    if !self.network_failure_reported {
                        eprintln!(
                            "{APPLICATION}: {error}; preserving the previous visible snapshot"
                        );
                        self.network_failure_reported = true;
                    }
                }
                NetworkStatusPoll::Empty => break,
                NetworkStatusPoll::Disconnected => {
                    disconnected = true;
                    break;
                }
            }
        }
        if disconnected {
            self.finish_network_worker();
        }
    }

    pub(super) fn finish_network_worker(&mut self) {
        let Some(worker) = self.network_worker.take() else {
            return;
        };
        report_network_shutdown(worker.shutdown());
    }
}

pub(super) fn start_network_worker() -> Option<NetworkStatusWorker> {
    let config = match NetworkStatusConfig::new(
        WIRELESS_INTERFACE,
        WIREGUARD_INTERFACE,
        WIFI_STATUS,
        NETWORK_REFRESH,
    ) {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "{APPLICATION}: cannot configure read-only network status: {error}; status remains unavailable"
            );
            return None;
        }
    };
    match NetworkStatusWorker::spawn(config) {
        Ok(worker) => Some(worker),
        Err(error) => {
            eprintln!(
                "{APPLICATION}: cannot start read-only network status: {error}; status remains unavailable"
            );
            None
        }
    }
}

fn report_network_shutdown(report: NetworkStatusWorkerReport) {
    if report.panicked {
        eprintln!("{APPLICATION}: read-only network status worker panicked during shutdown");
    }
    if report.failures != 0 || report.dropped_updates != 0 {
        eprintln!(
            "{APPLICATION}: network status stopped after {} failure(s), with {} update(s) omitted",
            report.failures, report.dropped_updates
        );
    }
}
