#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    Automatic,
    Manual,
}

impl SyncMode {
    #[must_use]
    pub fn from_dropdown_index(index: u32) -> Self {
        match index {
            0 => Self::Manual,
            1 => Self::Automatic,
            _ => Self::Manual,
        }
    }

    #[must_use]
    pub fn dropdown_index(self) -> u32 {
        match self {
            Self::Manual => 0,
            Self::Automatic => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeState {
    Idle,
    OneTimeSyncing,
    Monitoring,
    Previewing,
    StoppingSync,
    StoppingPreview,
    StoppingMonitor,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlInput {
    pub mode: SyncMode,
    pub runtime: RuntimeState,
    pub authenticated: bool,
    pub client_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandButtonModel {
    pub visible: bool,
    pub sensitive: bool,
    pub icon: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlButtons {
    pub sync: CommandButtonModel,
    pub preview: CommandButtonModel,
}

#[must_use]
pub fn controls_for(input: ControlInput) -> ControlButtons {
    let can_start =
        input.authenticated && input.client_ready && matches!(input.runtime, RuntimeState::Idle);
    match input.mode {
        SyncMode::Automatic => automatic_controls(input.runtime, can_start),
        SyncMode::Manual => manual_controls(input.runtime, can_start),
    }
}

fn automatic_controls(runtime: RuntimeState, can_start: bool) -> ControlButtons {
    let sync = match runtime {
        RuntimeState::Monitoring => CommandButtonModel {
            visible: true,
            sensitive: true,
            icon: "media-playback-stop-symbolic",
            label: "停止",
        },
        RuntimeState::StoppingMonitor => CommandButtonModel {
            visible: true,
            sensitive: false,
            icon: "process-stop-symbolic",
            label: "正在停止",
        },
        _ => CommandButtonModel {
            visible: true,
            sensitive: can_start,
            icon: "media-playback-start-symbolic",
            label: "自动同步",
        },
    };
    ControlButtons {
        sync,
        preview: CommandButtonModel {
            visible: false,
            sensitive: false,
            icon: "view-list-symbolic",
            label: "预览",
        },
    }
}

fn manual_controls(runtime: RuntimeState, can_start: bool) -> ControlButtons {
    let sync = match runtime {
        RuntimeState::OneTimeSyncing => CommandButtonModel {
            visible: true,
            sensitive: true,
            icon: "media-playback-stop-symbolic",
            label: "停止",
        },
        RuntimeState::StoppingSync => CommandButtonModel {
            visible: true,
            sensitive: false,
            icon: "process-stop-symbolic",
            label: "停止",
        },
        _ => CommandButtonModel {
            visible: true,
            sensitive: can_start,
            icon: "view-refresh-symbolic",
            label: "同步",
        },
    };
    let preview = match runtime {
        RuntimeState::Previewing => CommandButtonModel {
            visible: true,
            sensitive: true,
            icon: "media-playback-stop-symbolic",
            label: "停止",
        },
        RuntimeState::StoppingSync => CommandButtonModel {
            visible: true,
            sensitive: false,
            icon: "view-list-symbolic",
            label: "预览",
        },
        RuntimeState::StoppingPreview => CommandButtonModel {
            visible: true,
            sensitive: false,
            icon: "process-stop-symbolic",
            label: "停止",
        },
        _ => CommandButtonModel {
            visible: true,
            sensitive: can_start,
            icon: "view-list-symbolic",
            label: "预览",
        },
    };
    ControlButtons { sync, preview }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(mode: SyncMode, runtime: RuntimeState) -> ControlInput {
        ControlInput {
            mode,
            runtime,
            authenticated: true,
            client_ready: true,
        }
    }

    #[test]
    fn automatic_idle_shows_single_start_button_and_no_preview() {
        let controls = controls_for(ready(SyncMode::Automatic, RuntimeState::Idle));

        assert_eq!(controls.sync.label, "自动同步");
        assert_eq!(controls.sync.icon, "media-playback-start-symbolic");
        assert!(controls.sync.visible);
        assert!(controls.sync.sensitive);
        assert!(!controls.preview.visible);
    }

    #[test]
    fn automatic_monitoring_turns_sync_button_into_stop_button() {
        let controls = controls_for(ready(SyncMode::Automatic, RuntimeState::Monitoring));

        assert_eq!(controls.sync.label, "停止");
        assert_eq!(controls.sync.icon, "media-playback-stop-symbolic");
        assert!(controls.sync.sensitive);
        assert!(!controls.preview.visible);
    }

    #[test]
    fn manual_idle_shows_sync_and_preview_buttons() {
        let controls = controls_for(ready(SyncMode::Manual, RuntimeState::Idle));

        assert_eq!(controls.sync.label, "同步");
        assert_eq!(controls.preview.label, "预览");
        assert!(controls.sync.visible);
        assert!(controls.preview.visible);
        assert!(controls.sync.sensitive);
        assert!(controls.preview.sensitive);
    }

    #[test]
    fn manual_one_time_syncing_turns_sync_button_into_stop_button() {
        let controls = controls_for(ready(SyncMode::Manual, RuntimeState::OneTimeSyncing));

        assert_eq!(controls.sync.label, "停止");
        assert_eq!(controls.sync.icon, "media-playback-stop-symbolic");
        assert!(controls.sync.sensitive);
        assert!(controls.preview.visible);
        assert!(!controls.preview.sensitive);
    }

    #[test]
    fn manual_previewing_turns_preview_button_into_stop_button() {
        let controls = controls_for(ready(SyncMode::Manual, RuntimeState::Previewing));

        assert_eq!(controls.preview.label, "停止");
        assert_eq!(controls.preview.icon, "media-playback-stop-symbolic");
        assert!(controls.preview.sensitive);
        assert!(!controls.sync.sensitive);
    }

    #[test]
    fn unauthenticated_account_cannot_start_any_mode() {
        let controls = controls_for(ControlInput {
            mode: SyncMode::Manual,
            runtime: RuntimeState::Idle,
            authenticated: false,
            client_ready: true,
        });

        assert!(!controls.sync.sensitive);
        assert!(!controls.preview.sensitive);
    }

    #[test]
    fn sync_mode_maps_to_dropdown_indices() {
        assert_eq!(SyncMode::from_dropdown_index(0), SyncMode::Manual);
        assert_eq!(SyncMode::from_dropdown_index(1), SyncMode::Automatic);
        assert_eq!(SyncMode::from_dropdown_index(99), SyncMode::Manual);
        assert_eq!(SyncMode::Manual.dropdown_index(), 0);
        assert_eq!(SyncMode::Automatic.dropdown_index(), 1);
    }

    #[test]
    fn in_progress_and_blocked_states_disable_starts() {
        let previewing = controls_for(ready(SyncMode::Manual, RuntimeState::Previewing));
        assert_eq!(previewing.preview.label, "停止");
        assert!(!previewing.sync.sensitive);
        assert!(previewing.preview.sensitive);

        let stopping_sync = controls_for(ready(SyncMode::Manual, RuntimeState::StoppingSync));
        assert_eq!(stopping_sync.sync.label, "停止");
        assert_eq!(stopping_sync.sync.icon, "process-stop-symbolic");
        assert!(!stopping_sync.sync.sensitive);
        assert_eq!(stopping_sync.preview.label, "预览");
        assert_eq!(stopping_sync.preview.icon, "view-list-symbolic");
        assert!(!stopping_sync.preview.sensitive);

        let stopping_preview = controls_for(ready(SyncMode::Manual, RuntimeState::StoppingPreview));
        assert_eq!(stopping_preview.sync.label, "同步");
        assert_eq!(stopping_preview.sync.icon, "view-refresh-symbolic");
        assert!(!stopping_preview.sync.sensitive);
        assert_eq!(stopping_preview.preview.label, "停止");
        assert_eq!(stopping_preview.preview.icon, "process-stop-symbolic");
        assert!(!stopping_preview.preview.sensitive);

        let stopping_monitor =
            controls_for(ready(SyncMode::Automatic, RuntimeState::StoppingMonitor));
        assert_eq!(stopping_monitor.sync.label, "正在停止");
        assert_eq!(stopping_monitor.sync.icon, "process-stop-symbolic");
        assert!(!stopping_monitor.sync.sensitive);

        let blocked = controls_for(ready(SyncMode::Manual, RuntimeState::Blocked));
        assert!(!blocked.sync.sensitive);
        assert!(!blocked.preview.sensitive);
    }
}
