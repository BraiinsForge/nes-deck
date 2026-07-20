//! Managed external effects for the staged dashboard runtime.

use std::os::fd::AsFd as _;
use std::path::Path;
use std::process::Command as ProcessCommand;
use std::time::{Duration, Instant};

use retro_deck_audio::Volume;
use retro_deck_dashboard::{
    ExitHold, ExitHoldEvent, ExitPolicy, Intent, LaunchPlan, LaunchTarget, TerminalMode,
    parse_volume,
};
use retro_deck_platform::audio::{AudioGate, ToneCueWorker};
use retro_deck_platform::file::read_regular_bounded;
use retro_deck_platform::input::TouchscreenDevice;
use retro_deck_platform::process::{ManagedChild, ManagedChildExit};

use super::{
    APPLICATION, DashboardRuntime, VOLUME_STATE, desired_audio_gate, elapsed_milliseconds,
};

const CHILD_POLL: Duration = Duration::from_millis(40);
const AUDIO_HANDOFF_TIMEOUT: Duration = Duration::from_secs(2);
const VOLUME_ENVIRONMENT: &str = "RETRO_DECK_VOLUME_PERCENT";
const KEYMAP_ENVIRONMENT: &str = "RETRO_DECK_KEYMAP";
const EXIT_HINT_ENVIRONMENT: &str = "RETRO_DECK_EXIT_HINT";
const VOLUME_STATE_ENVIRONMENT: &str = "RETRO_DECK_VOLUME_STATE";

#[derive(Debug)]
pub(super) struct PendingLaunch {
    command: ProcessCommand,
    label: String,
    exit_policy: ExitPolicy,
    queued_at: Instant,
}

impl PendingLaunch {
    fn from_plan(plan: LaunchPlan<'_>, label: String) -> Self {
        let mut command = ProcessCommand::new(plan.program());
        command.current_dir("/");
        for name in [
            VOLUME_ENVIRONMENT,
            KEYMAP_ENVIRONMENT,
            EXIT_HINT_ENVIRONMENT,
            VOLUME_STATE_ENVIRONMENT,
        ] {
            command.env_remove(name);
        }
        if let Some(argument) = plan.argument() {
            command.arg(argument);
        }
        if let Some(percent) = plan.volume_percent() {
            command.env(VOLUME_ENVIRONMENT, percent.to_string());
        }
        if let Some(keymap) = plan.keymap() {
            command.env(KEYMAP_ENVIRONMENT, keymap.as_str());
        }
        if plan.exit_hint() {
            command.env(EXIT_HINT_ENVIRONMENT, "1");
        }
        if let Some(path) = plan.volume_state() {
            command.env(VOLUME_STATE_ENVIRONMENT, path);
        }
        Self {
            command,
            label,
            exit_policy: plan.exit_policy(),
            queued_at: Instant::now(),
        }
    }
}

impl DashboardRuntime {
    pub(super) fn queue_intent(&mut self, intent: Intent) {
        if self.pending_launch.is_some() {
            return;
        }
        match intent {
            Intent::Launch(index) => self.queue_catalog_launch(index),
            Intent::OpenTerminal => {
                let plan = LaunchPlan::from_target(
                    LaunchTarget::Terminal(TerminalMode::Shell),
                    self.model.volume(),
                    self.model.keymap(),
                );
                match plan {
                    Ok(plan) => self
                        .begin_audio_handoff(PendingLaunch::from_plan(plan, "terminal".to_owned())),
                    Err(error) => {
                        eprintln!("{APPLICATION}: cannot plan terminal launch: {error}");
                    }
                }
            }
            Intent::OpenWifi => self.open_wifi_editor(),
        }
    }

    fn queue_catalog_launch(&mut self, index: usize) {
        let Some(entry) = self.model.catalog().entry(index) else {
            eprintln!("{APPLICATION}: rejected missing catalog launch index {index}");
            return;
        };
        let label = entry.title().to_owned();
        let target = match LaunchTarget::from_entry(entry) {
            Ok(target) => target,
            Err(error) => {
                eprintln!("{APPLICATION}: cannot classify {label}: {error}");
                return;
            }
        };
        let plan = if matches!(target, LaunchTarget::Reboot) {
            LaunchPlan::confirmed_reboot()
        } else {
            match LaunchPlan::from_target(target, self.model.volume(), self.model.keymap()) {
                Ok(plan) => plan,
                Err(error) => {
                    eprintln!("{APPLICATION}: cannot plan {label}: {error}");
                    return;
                }
            }
        };
        let pending = PendingLaunch::from_plan(plan, label);
        self.begin_audio_handoff(pending);
    }

    fn begin_audio_handoff(&mut self, pending: PendingLaunch) {
        eprintln!(
            "{APPLICATION}: preparing managed launch for {}",
            pending.label
        );
        self.pending_launch = Some(pending);
        self.audio_gate = AudioGate::Hidden;
        if let Some(audio) = &self.audio {
            audio.set_gate(AudioGate::Hidden);
        }
    }

    pub(super) fn service_pending_launch(&mut self) -> bool {
        let Some(pending) = self.pending_launch.as_ref() else {
            return false;
        };
        let audio_released = self
            .audio
            .as_ref()
            .is_none_or(ToneCueWorker::device_released);
        if !audio_released {
            if pending.queued_at.elapsed() < AUDIO_HANDOFF_TIMEOUT {
                return true;
            }
            let Some(cancelled) = self.pending_launch.take() else {
                return true;
            };
            eprintln!(
                "{APPLICATION}: cancelled {} because menu audio did not release in time",
                cancelled.label
            );
            self.restore_audio_gate();
            self.dirty = true;
            return true;
        }

        let Some(pending) = self.pending_launch.take() else {
            return false;
        };
        self.run_pending_launch(pending);
        self.reload_child_volume();
        self.restore_audio_gate();
        self.dirty = true;
        true
    }

    fn run_pending_launch(&mut self, mut pending: PendingLaunch) {
        let mut touchscreen = match pending.exit_policy {
            ExitPolicy::SupervisorTouchHold => match TouchscreenDevice::discover() {
                Ok(touchscreen) => Some(touchscreen),
                Err(error) => {
                    eprintln!(
                        "{APPLICATION}: cannot start {} without supervised exit touch: {error}",
                        pending.label
                    );
                    return;
                }
            },
            ExitPolicy::ChildOwnsTouch | ExitPolicy::None => None,
        };
        let mut exit_hold = touchscreen
            .as_ref()
            .map(|touchscreen| ExitHold::new(touchscreen.state().down()));
        let mut child = match ManagedChild::spawn(&mut pending.command) {
            Ok(child) => child,
            Err(error) => {
                eprintln!("{APPLICATION}: cannot launch {}: {error}", pending.label);
                return;
            }
        };
        eprintln!(
            "{APPLICATION}: launched {} as {}",
            pending.label,
            child.program().display()
        );
        let child_started = Instant::now();

        loop {
            if let Err(error) = self.presentation.dispatch_nonblocking() {
                eprintln!(
                    "{APPLICATION}: display failed while supervising {}: {error}",
                    pending.label
                );
                return;
            }
            let now = Instant::now();
            if self.shutdown.requested() || self.presentation.shutdown_requested() {
                request_child_termination(&mut child, now, &pending.label, "dashboard shutdown");
            }
            match child.poll(now) {
                Ok(Some(exit)) => {
                    report_child_exit(&pending.label, exit);
                    return;
                }
                Ok(None) => {}
                Err(error) => {
                    eprintln!(
                        "{APPLICATION}: cannot supervise {}: {error}; forcing containment",
                        pending.label
                    );
                    return;
                }
            }

            let touch_available = match (touchscreen.as_mut(), exit_hold.as_mut()) {
                (Some(device), Some(hold)) => update_supervised_touch(
                    device,
                    hold,
                    &mut child,
                    now,
                    child_started,
                    &pending.label,
                ),
                _ => true,
            };
            if !touch_available {
                touchscreen = None;
                exit_hold = None;
            }
            self.discard_menu_input();

            let wait = if let Some(touchscreen) = &touchscreen {
                touchscreen.wait_readable_with(self.presentation.as_fd(), CHILD_POLL)
            } else {
                self.controllers
                    .wait_readable_with(self.presentation.as_fd(), CHILD_POLL)
            };
            if let Err(error) = wait {
                eprintln!(
                    "{APPLICATION}: wait failed while supervising {}: {error}; forcing containment",
                    pending.label
                );
                return;
            }
        }
    }

    pub(super) fn discard_menu_input(&mut self) {
        self.input_events.clear();
        let _stats = self.controllers.drain_into(&mut self.input_events);
        self.input_events.clear();
        let _reports = self.presentation.take_touch_reports();
        self.touch.cancel();
        self.wifi_touch.cancel();
    }

    fn reload_child_volume(&mut self) {
        let bytes = match read_regular_bounded(
            Path::new(VOLUME_STATE),
            retro_deck_dashboard::MAXIMUM_PREFERENCE_BYTES,
        ) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("{APPLICATION}: cannot reload child volume: {error}");
                return;
            }
        };
        let volume = match parse_volume(&bytes) {
            Ok(volume) => volume,
            Err(error) => {
                eprintln!("{APPLICATION}: cannot reload child volume: {error}");
                return;
            }
        };
        if !self.model.adopt_volume(volume) {
            return;
        }
        if let Some(audio) = &self.audio {
            let Some(volume) = Volume::new(volume.percent()) else {
                return;
            };
            audio.set_volume(volume);
        }
        eprintln!(
            "{APPLICATION}: managed child updated game volume to {}%",
            volume.percent()
        );
    }

    fn restore_audio_gate(&mut self) {
        let requested =
            desired_audio_gate(self.presentation.visible(), self.model.volume().is_muted());
        self.audio_gate = requested;
        if let Some(audio) = &self.audio {
            audio.set_gate(requested);
        }
    }
}

fn update_supervised_touch(
    touchscreen: &mut TouchscreenDevice,
    exit_hold: &mut ExitHold,
    child: &mut ManagedChild,
    now: Instant,
    child_started: Instant,
    label: &str,
) -> bool {
    let state = match touchscreen.drain() {
        Ok(state) => state,
        Err(error) => {
            eprintln!(
                "{APPLICATION}: supervised touch failed for {label}: {error}; stopping child"
            );
            request_child_termination(child, now, label, "touch failure");
            return false;
        }
    };
    let point = state.point();
    match exit_hold.update(
        state.down(),
        point.x(),
        point.y(),
        elapsed_milliseconds(child_started),
    ) {
        Some(ExitHoldEvent::Started) => {
            eprintln!("{APPLICATION}: exit hold started for {label}");
        }
        Some(ExitHoldEvent::Cancelled) => {
            eprintln!("{APPLICATION}: exit hold cancelled for {label}");
        }
        Some(ExitHoldEvent::Completed) => {
            request_child_termination(child, now, label, "touch hold");
        }
        None => {}
    }
    true
}

fn request_child_termination(child: &mut ManagedChild, now: Instant, label: &str, reason: &str) {
    match child.request_termination(now) {
        Ok(true) => eprintln!("{APPLICATION}: stopping {label} after {reason}"),
        Ok(false) => {}
        Err(error) => eprintln!("{APPLICATION}: cannot stop {label} after {reason}: {error}"),
    }
}

fn report_child_exit(label: &str, exit: ManagedChildExit) {
    eprintln!(
        "{APPLICATION}: {label} ended with {} ({:?})",
        exit.status(),
        exit.cause()
    );
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::path::Path;
    use std::process::Command as ProcessCommand;

    use retro_deck_config::System;
    use retro_deck_dashboard::{
        ExitPolicy, Keymap, LaunchPlan, LaunchTarget, TerminalMode, VolumeState,
    };

    use super::{
        EXIT_HINT_ENVIRONMENT, KEYMAP_ENVIRONMENT, PendingLaunch, VOLUME_ENVIRONMENT,
        VOLUME_STATE_ENVIRONMENT,
    };

    #[test]
    fn pending_commands_own_fixed_program_arguments_and_environment() {
        let Some(volume) = VolumeState::new(55, 55).ok() else {
            return;
        };
        let plan = LaunchPlan::from_target(
            LaunchTarget::Emulator {
                system: System::Nes,
                content: Path::new("/mnt/data/roms/nes/test.nes"),
            },
            volume,
            Keymap::Czech,
        );
        let Some(plan) = plan.ok() else {
            return;
        };
        let pending = PendingLaunch::from_plan(plan, "TEST".to_owned());
        assert_eq!(
            pending.command.get_program(),
            OsStr::new("/mnt/data/nes-deck/nes-deck")
        );
        assert_eq!(
            pending.command.get_args().collect::<Vec<_>>(),
            [OsStr::new("/mnt/data/roms/nes/test.nes")]
        );
        assert_eq!(pending.command.get_current_dir(), Some(Path::new("/")));
        assert_eq!(
            environment_value(&pending.command, VOLUME_ENVIRONMENT),
            Some(OsStr::new("55"))
        );
        assert_eq!(
            environment_value(&pending.command, EXIT_HINT_ENVIRONMENT),
            Some(OsStr::new("1"))
        );
        assert!(environment_removed(&pending.command, KEYMAP_ENVIRONMENT));
        assert!(environment_removed(
            &pending.command,
            VOLUME_STATE_ENVIRONMENT
        ));
        assert_eq!(pending.exit_policy, ExitPolicy::SupervisorTouchHold);

        let terminal = LaunchPlan::from_target(
            LaunchTarget::Terminal(TerminalMode::Lisp),
            volume,
            Keymap::Czech,
        );
        let Some(terminal) = terminal.ok() else {
            return;
        };
        let terminal = PendingLaunch::from_plan(terminal, "LISP".to_owned());
        assert_eq!(
            terminal.command.get_args().collect::<Vec<_>>(),
            [OsStr::new("lisp")]
        );
        assert_eq!(
            environment_value(&terminal.command, KEYMAP_ENVIRONMENT),
            Some(OsStr::new("cz"))
        );
        assert!(environment_removed(&terminal.command, VOLUME_ENVIRONMENT));
    }

    fn environment_value<'command>(
        command: &'command ProcessCommand,
        name: &str,
    ) -> Option<&'command OsStr> {
        command
            .get_envs()
            .find_map(|(key, value)| (key == name).then_some(value).flatten())
    }

    fn environment_removed(command: &ProcessCommand, name: &str) -> bool {
        command
            .get_envs()
            .any(|(key, value)| key == name && value.is_none())
    }
}
