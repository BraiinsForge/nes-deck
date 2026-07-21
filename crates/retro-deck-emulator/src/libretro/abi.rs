//! Minimal raw declarations for libretro API version 1.

#![allow(
    dead_code,
    reason = "ABI declarations are consumed incrementally by the private Rust host port"
)]

use std::ffi::{c_char, c_uint, c_void};
use std::ptr;

pub(super) const API_VERSION: c_uint = 1;

pub(super) const DEVICE_JOYPAD: c_uint = 1;
pub(super) const DEVICE_KEYBOARD: c_uint = 3;
pub(super) const DEVICE_TYPE_SHIFT: c_uint = 8;
pub(super) const DEVICE_MASK: c_uint = (1 << DEVICE_TYPE_SHIFT) - 1;

pub(super) const MEMORY_SAVE_RAM: c_uint = 0;
pub(super) const MEMORY_RTC: c_uint = 1;

pub(super) const PIXEL_FORMAT_XRGB8888: c_uint = 1;
pub(super) const PIXEL_FORMAT_RGB565: c_uint = 2;

pub(super) const LANGUAGE_ENGLISH: c_uint = 0;

pub(super) const ENVIRONMENT_GET_CAN_DUPE: c_uint = 3;
pub(super) const ENVIRONMENT_SET_PERFORMANCE_LEVEL: c_uint = 8;
pub(super) const ENVIRONMENT_GET_SYSTEM_DIRECTORY: c_uint = 9;
pub(super) const ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
pub(super) const ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
pub(super) const ENVIRONMENT_GET_VARIABLE: c_uint = 15;
pub(super) const ENVIRONMENT_SET_VARIABLES: c_uint = 16;
pub(super) const ENVIRONMENT_GET_VARIABLE_UPDATE: c_uint = 17;
pub(super) const ENVIRONMENT_GET_RUMBLE_INTERFACE: c_uint = 23;
pub(super) const ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
pub(super) const ENVIRONMENT_GET_CONTENT_DIRECTORY: c_uint = 30;
pub(super) const ENVIRONMENT_GET_SAVE_DIRECTORY: c_uint = 31;
pub(super) const ENVIRONMENT_SET_SYSTEM_AV_INFO: c_uint = 32;
pub(super) const ENVIRONMENT_SET_SUBSYSTEM_INFO: c_uint = 34;
pub(super) const ENVIRONMENT_SET_CONTROLLER_INFO: c_uint = 35;
pub(super) const ENVIRONMENT_SET_GEOMETRY: c_uint = 37;
pub(super) const ENVIRONMENT_GET_LANGUAGE: c_uint = 39;
pub(super) const ENVIRONMENT_SET_CORE_OPTIONS: c_uint = 53;
pub(super) const ENVIRONMENT_SET_CORE_OPTIONS_INTL: c_uint = 54;
pub(super) const ENVIRONMENT_SET_CORE_OPTIONS_DISPLAY: c_uint = 55;
pub(super) const ENVIRONMENT_GET_CORE_OPTIONS_VERSION: c_uint = 52;
pub(super) const ENVIRONMENT_SET_CORE_OPTIONS_V2: c_uint = 67;
pub(super) const ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL: c_uint = 68;

const ENVIRONMENT_EXPERIMENTAL: c_uint = 0x1_0000;
pub(super) const ENVIRONMENT_SET_MEMORY_MAPS: c_uint = 0x24 | ENVIRONMENT_EXPERIMENTAL;
pub(super) const ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS: c_uint = 0x2a | ENVIRONMENT_EXPERIMENTAL;
pub(super) const ENVIRONMENT_GET_INPUT_BITMASKS: c_uint = 0x33 | ENVIRONMENT_EXPERIMENTAL;

pub(super) const fn device_subclass(base: c_uint, identifier: c_uint) -> c_uint {
    ((identifier + 1) << DEVICE_TYPE_SHIFT) | base
}

pub(super) type EnvironmentCallback = unsafe extern "C" fn(c_uint, *mut c_void) -> bool;
pub(super) type VideoRefreshCallback = unsafe extern "C" fn(*const c_void, c_uint, c_uint, usize);
pub(super) type AudioSampleCallback = unsafe extern "C" fn(i16, i16);
pub(super) type AudioSampleBatchCallback = unsafe extern "C" fn(*const i16, usize) -> usize;
pub(super) type InputPollCallback = unsafe extern "C" fn();
pub(super) type InputStateCallback = unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint) -> i16;
pub(super) type LogPrintf = unsafe extern "C" fn(c_uint, *const c_char, ...);

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(super) struct SystemInfo {
    pub(super) library_name: *const c_char,
    pub(super) library_version: *const c_char,
    pub(super) valid_extensions: *const c_char,
    pub(super) need_fullpath: bool,
    pub(super) block_extract: bool,
}

impl Default for SystemInfo {
    fn default() -> Self {
        Self {
            library_name: ptr::null(),
            library_version: ptr::null(),
            valid_extensions: ptr::null(),
            need_fullpath: false,
            block_extract: false,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct GameGeometry {
    pub(super) base_width: c_uint,
    pub(super) base_height: c_uint,
    pub(super) max_width: c_uint,
    pub(super) max_height: c_uint,
    pub(super) aspect_ratio: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct SystemTiming {
    pub(super) frames_per_second: f64,
    pub(super) sample_rate: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct SystemAvInfo {
    pub(super) geometry: GameGeometry,
    pub(super) timing: SystemTiming,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(super) struct Variable {
    pub(super) key: *const c_char,
    pub(super) value: *const c_char,
}

impl Default for Variable {
    fn default() -> Self {
        Self {
            key: ptr::null(),
            value: ptr::null(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(super) struct GameInfo {
    pub(super) path: *const c_char,
    pub(super) data: *const c_void,
    pub(super) size: usize,
    pub(super) metadata: *const c_char,
}

impl Default for GameInfo {
    fn default() -> Self {
        Self {
            path: ptr::null(),
            data: ptr::null(),
            size: 0,
            metadata: ptr::null(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LogCallback {
    pub(super) log: Option<LogPrintf>,
}

unsafe extern "C" {
    pub(super) fn retro_set_environment(callback: EnvironmentCallback);
    pub(super) fn retro_set_video_refresh(callback: VideoRefreshCallback);
    pub(super) fn retro_set_audio_sample(callback: AudioSampleCallback);
    pub(super) fn retro_set_audio_sample_batch(callback: AudioSampleBatchCallback);
    pub(super) fn retro_set_input_poll(callback: InputPollCallback);
    pub(super) fn retro_set_input_state(callback: InputStateCallback);

    pub(super) fn retro_init();
    pub(super) fn retro_deinit();
    pub(super) fn retro_api_version() -> c_uint;
    pub(super) fn retro_get_system_info(info: *mut SystemInfo);
    pub(super) fn retro_get_system_av_info(info: *mut SystemAvInfo);
    pub(super) fn retro_set_controller_port_device(port: c_uint, device: c_uint);
    pub(super) fn retro_load_game(game: *const GameInfo) -> bool;
    pub(super) fn retro_unload_game();
    pub(super) fn retro_run();
    pub(super) fn retro_get_memory_data(identifier: c_uint) -> *mut c_void;
    pub(super) fn retro_get_memory_size(identifier: c_uint) -> usize;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn constants_match_libretro_api_version_one() {
        assert_eq!(API_VERSION, 1);
        assert_eq!(DEVICE_JOYPAD, 1);
        assert_eq!(DEVICE_KEYBOARD, 3);
        assert_eq!(device_subclass(DEVICE_JOYPAD, 1), 0x201);
        assert_eq!(device_subclass(DEVICE_JOYPAD, 3), 0x401);
        assert_eq!(MEMORY_SAVE_RAM, 0);
        assert_eq!(MEMORY_RTC, 1);
        assert_eq!(PIXEL_FORMAT_XRGB8888, 1);
        assert_eq!(PIXEL_FORMAT_RGB565, 2);
        assert_eq!(ENVIRONMENT_GET_VARIABLE, 15);
        assert_eq!(ENVIRONMENT_SET_MEMORY_MAPS, 0x1_0024);
        assert_eq!(ENVIRONMENT_GET_INPUT_BITMASKS, 0x1_0033);
        assert_eq!(ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL, 68);
    }

    #[test]
    fn pointer_structs_have_c_field_order_and_alignment() {
        let pointer = size_of::<*const c_void>();
        assert_eq!(offset_of!(SystemInfo, library_name), 0);
        assert_eq!(offset_of!(SystemInfo, library_version), pointer);
        assert_eq!(offset_of!(SystemInfo, valid_extensions), pointer * 2);
        assert_eq!(offset_of!(SystemInfo, need_fullpath), pointer * 3);
        assert_eq!(offset_of!(SystemInfo, block_extract), pointer * 3 + 1);
        assert_eq!(align_of::<SystemInfo>(), pointer);
        assert_eq!(size_of::<SystemInfo>(), pointer * 4);

        assert_eq!(offset_of!(Variable, key), 0);
        assert_eq!(offset_of!(Variable, value), pointer);
        assert_eq!(size_of::<Variable>(), pointer * 2);

        assert_eq!(offset_of!(GameInfo, path), 0);
        assert_eq!(offset_of!(GameInfo, data), pointer);
        assert_eq!(offset_of!(GameInfo, size), pointer * 2);
        assert_eq!(offset_of!(GameInfo, metadata), pointer * 3);
        assert_eq!(size_of::<GameInfo>(), pointer * 4);
        assert_eq!(size_of::<LogCallback>(), pointer);
    }

    #[test]
    fn audiovisual_structs_match_the_c_layout() {
        assert_eq!(size_of::<GameGeometry>(), 20);
        assert_eq!(align_of::<GameGeometry>(), align_of::<c_uint>());
        assert_eq!(offset_of!(GameGeometry, aspect_ratio), 16);
        assert_eq!(size_of::<SystemTiming>(), 16);
        assert_eq!(offset_of!(SystemAvInfo, geometry), 0);
        assert_eq!(offset_of!(SystemAvInfo, timing), 24);
        assert_eq!(size_of::<SystemAvInfo>(), 40);
    }

    #[test]
    fn callback_types_are_single_function_pointers() {
        let pointer = size_of::<*const c_void>();
        assert_eq!(size_of::<EnvironmentCallback>(), pointer);
        assert_eq!(size_of::<VideoRefreshCallback>(), pointer);
        assert_eq!(size_of::<AudioSampleCallback>(), pointer);
        assert_eq!(size_of::<AudioSampleBatchCallback>(), pointer);
        assert_eq!(size_of::<InputPollCallback>(), pointer);
        assert_eq!(size_of::<InputStateCallback>(), pointer);
        assert_eq!(size_of::<LogPrintf>(), pointer);
    }
}
