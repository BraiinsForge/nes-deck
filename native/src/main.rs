use retrodeck_native::{audio, canvas, fbdev, wayland};
use std::env;
use std::ffi::{CString, OsStr, c_char, c_int, c_void};
use std::mem;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::ptr;

type ClObject = *mut c_void;
type ClFixnum = isize;
type EclFixedFunction = unsafe extern "C" fn() -> ClObject;
type EclOneArgumentFunction = unsafe extern "C" fn(ClObject) -> ClObject;
type EclTwoArgumentFunction = unsafe extern "C" fn(ClObject, ClObject) -> ClObject;
type EclThreeArgumentFunction = unsafe extern "C" fn(ClObject, ClObject, ClObject) -> ClObject;
type EclFiveArgumentFunction =
    unsafe extern "C" fn(ClObject, ClObject, ClObject, ClObject, ClObject) -> ClObject;

const ECL_NIL: ClObject = 1usize as ClObject;
const FIXNUM_TAG: usize = 3;
const DEFAULT_STARTUP: &str = "/mnt/data/nes-deck/lisp/startup.lisp";
const ABI_VERSION: ClFixnum = 7;

const LOAD_STARTUP: &str = r#"
(handler-case
    (load cl-user::*retrodeck-startup-path* :verbose nil :print nil)
  (error (condition)
    (format *error-output* "retrodeck: failed to load ~A: ~A~%"
            cl-user::*retrodeck-startup-path* condition)
    nil))
"#;

const RUN_MAIN: &str = r#"
(handler-case
    (let* ((package (or (find-package "RETRODECK")
                        (error "The RETRODECK package is missing")))
           (symbol (or (find-symbol "MAIN" package)
                       (error "RETRODECK:MAIN is missing"))))
      (funcall symbol))
  (error (condition)
    (format *error-output* "retrodeck: Lisp orchestrator failed: ~A~%" condition)
    1))
"#;

unsafe extern "C" {
    fn cl_boot(argc: c_int, argv: *mut *mut c_char) -> c_int;
    fn cl_shutdown();
    fn ecl_make_symbol(name: *const c_char, package: *const c_char) -> ClObject;
    fn ecl_def_c_function(symbol: ClObject, function: EclFixedFunction, arguments: c_int);
    fn ecl_make_integer(value: ClFixnum) -> ClObject;
    fn ecl_cons(car: ClObject, cdr: ClObject) -> ClObject;
    fn ecl_make_simple_base_string(value: *const c_char, length: ClFixnum) -> ClObject;
    fn ecl_base_string_pointer_safe(value: ClObject) -> *mut c_char;
    fn ecl_length(value: ClObject) -> ClFixnum;
    fn ecl_find_package(name: *const c_char) -> ClObject;
    fn ecl_make_package(
        name: ClObject,
        nicknames: ClObject,
        use_list: ClObject,
        local_nicknames: ClObject,
    ) -> ClObject;
    fn ecl_defparameter(symbol: ClObject, value: ClObject);
    fn si_string_to_object(arguments: ClFixnum, string: ClObject, ...) -> ClObject;
    fn si_safe_eval(arguments: ClFixnum, form: ClObject, environment: ClObject, ...) -> ClObject;
}

struct Ecl;

impl Ecl {
    fn boot() -> Result<Self, String> {
        let mut arguments = env::args_os()
            .map(|argument| CString::new(argument.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "process arguments cannot contain NUL".to_owned())?;
        let count = arguments.len();
        let mut pointers = arguments
            .iter_mut()
            .map(|argument| argument.as_ptr().cast_mut())
            .collect::<Vec<_>>();
        pointers.push(ptr::null_mut());

        if unsafe { cl_boot(count as c_int, pointers.as_mut_ptr()) } == 0 {
            return Err("ECL failed to start".to_owned());
        }
        Ok(Self)
    }

    fn register_primitives(&self) -> Result<(), String> {
        let package_name = c_string("RETRODECK.NATIVE")?;
        if unsafe { ecl_find_package(package_name.as_ptr()) } == ECL_NIL {
            let name = unsafe { ecl_make_simple_base_string(package_name.as_ptr(), -1) };
            unsafe { ecl_make_package(name, ECL_NIL, ECL_NIL, ECL_NIL) };
        }

        let abi_name = c_string("ABI-VERSION")?;
        let abi = unsafe { ecl_make_symbol(abi_name.as_ptr(), package_name.as_ptr()) };
        unsafe { ecl_def_c_function(abi, native_abi_version, 0) };

        let play_name = c_string("PLAY-TONES")?;
        let play = unsafe { ecl_make_symbol(play_name.as_ptr(), package_name.as_ptr()) };
        // ECL 26.5.5 defines cl_objectfn_fixed as the C old-style
        // cl_object (*)(), then dispatches the argument count registered here.
        let callback = unsafe {
            mem::transmute::<EclFiveArgumentFunction, EclFixedFunction>(native_play_tones)
        };
        unsafe { ecl_def_c_function(play, callback, 5) };

        for (name, function) in [
            (
                "CANVAS-FILL-RECT",
                native_canvas_fill_rect as EclFiveArgumentFunction,
            ),
            (
                "CANVAS-DRAW-GLYPH",
                native_canvas_draw_glyph as EclFiveArgumentFunction,
            ),
            (
                "CANVAS-DRAW-RASTER",
                native_canvas_draw_raster as EclFiveArgumentFunction,
            ),
        ] {
            let name = c_string(name)?;
            let symbol = unsafe { ecl_make_symbol(name.as_ptr(), package_name.as_ptr()) };
            let callback =
                unsafe { mem::transmute::<EclFiveArgumentFunction, EclFixedFunction>(function) };
            unsafe { ecl_def_c_function(symbol, callback, 5) };
        }

        let name = c_string("RASTER-LOAD-COVER")?;
        let symbol = unsafe { ecl_make_symbol(name.as_ptr(), package_name.as_ptr()) };
        let callback = unsafe {
            mem::transmute::<EclTwoArgumentFunction, EclFixedFunction>(native_raster_load_cover)
        };
        unsafe { ecl_def_c_function(symbol, callback, 2) };

        let name = c_string("RASTER-LOAD-PNG")?;
        let symbol = unsafe { ecl_make_symbol(name.as_ptr(), package_name.as_ptr()) };
        let callback = unsafe {
            mem::transmute::<EclThreeArgumentFunction, EclFixedFunction>(native_raster_load_png)
        };
        unsafe { ecl_def_c_function(symbol, callback, 3) };

        for (name, function) in [
            ("AUDIO-ACTIVE-P", native_audio_active as EclFixedFunction),
            ("STOP-AUDIO", native_stop_audio as EclFixedFunction),
            ("FINISH-AUDIO", native_finish_audio as EclFixedFunction),
            ("RASTER-CLEAR", native_raster_clear as EclFixedFunction),
            (
                "FBDEV-PRESENT-CANVAS",
                native_fbdev_present_canvas as EclFixedFunction,
            ),
            ("FBDEV-OPEN", native_fbdev_open as EclFixedFunction),
            ("FBDEV-CLOSE", native_fbdev_close as EclFixedFunction),
            ("FBDEV-SIZE", native_fbdev_size as EclFixedFunction),
            (
                "WAYLAND-PRESENT-CANVAS",
                native_wayland_present_canvas as EclFixedFunction,
            ),
            (
                "WAYLAND-OPEN-WIDGET",
                native_wayland_open_widget as EclFixedFunction,
            ),
            ("WAYLAND-CLOSE", native_wayland_close as EclFixedFunction),
            (
                "WAYLAND-NEXT-TOUCH",
                native_wayland_next_touch as EclFixedFunction,
            ),
            ("WAYLAND-SIZE", native_wayland_size as EclFixedFunction),
            (
                "WAYLAND-SHUTDOWN-P",
                native_wayland_shutdown as EclFixedFunction,
            ),
        ] {
            let name = c_string(name)?;
            let symbol = unsafe { ecl_make_symbol(name.as_ptr(), package_name.as_ptr()) };
            unsafe { ecl_def_c_function(symbol, function, 0) };
        }

        for (name, function) in [
            (
                "CANVAS-CLEAR",
                native_canvas_clear as EclOneArgumentFunction,
            ),
            (
                "FBDEV-PRESENT-SOLID",
                native_fbdev_present_solid as EclOneArgumentFunction,
            ),
            (
                "WAYLAND-PRESENT-SOLID",
                native_wayland_present_solid as EclOneArgumentFunction,
            ),
            (
                "WAYLAND-DISPATCH",
                native_wayland_dispatch as EclOneArgumentFunction,
            ),
        ] {
            let name = c_string(name)?;
            let symbol = unsafe { ecl_make_symbol(name.as_ptr(), package_name.as_ptr()) };
            let callback =
                unsafe { mem::transmute::<EclOneArgumentFunction, EclFixedFunction>(function) };
            unsafe { ecl_def_c_function(symbol, callback, 1) };
        }
        Ok(())
    }

    fn load(&self, path: &Path) -> Result<(), String> {
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "the Lisp startup path cannot contain NUL".to_owned())?;
        let path_value = unsafe { ecl_make_simple_base_string(path.as_ptr(), -1) };
        let variable_name = c_string("*RETRODECK-STARTUP-PATH*")?;
        let package_name = c_string("CL-USER")?;
        let variable = unsafe { ecl_make_symbol(variable_name.as_ptr(), package_name.as_ptr()) };
        unsafe { ecl_defparameter(variable, path_value) };

        if self.evaluate(LOAD_STARTUP) == ECL_NIL {
            return Err("Common Lisp startup failed".to_owned());
        }
        Ok(())
    }

    fn run(&self) -> Result<u8, String> {
        decode_exit_code(self.evaluate(RUN_MAIN))
    }

    fn evaluate(&self, source: &str) -> ClObject {
        let source = CString::new(source).expect("embedded Lisp contains no NUL");
        let string = unsafe { ecl_make_simple_base_string(source.as_ptr(), -1) };
        let form = unsafe { si_string_to_object(1, string) };
        unsafe { si_safe_eval(3, form, ECL_NIL, ECL_NIL) }
    }
}

impl Drop for Ecl {
    fn drop(&mut self) {
        fbdev::close();
        wayland::close();
        audio::stop();
        unsafe { cl_shutdown() };
    }
}

unsafe extern "C" fn native_abi_version() -> ClObject {
    unsafe { ecl_make_integer(ABI_VERSION) }
}

unsafe extern "C" fn native_play_tones(
    first_frequency: ClObject,
    first_duration_ms: ClObject,
    second_frequency: ClObject,
    second_duration_ms: ClObject,
    volume_percent: ClObject,
) -> ClObject {
    let result = (|| {
        audio::play_tones(
            decode_i32(first_frequency, "first tone frequency")?,
            decode_i32(first_duration_ms, "first tone duration")?,
            decode_i32(second_frequency, "second tone frequency")?,
            decode_i32(second_duration_ms, "second tone duration")?,
            decode_i32(volume_percent, "menu sound volume")?,
        )
    })();
    let status = match result {
        Ok(audio::PlayOutcome::Started) => 1,
        Ok(audio::PlayOutcome::Busy) => 2,
        Err(error) => {
            eprintln!("retrodeck: {error}");
            0
        }
    };
    unsafe { ecl_make_integer(status) }
}

unsafe extern "C" fn native_audio_active() -> ClObject {
    unsafe { ecl_make_integer(if audio::active() { 1 } else { 0 }) }
}

unsafe extern "C" fn native_stop_audio() -> ClObject {
    audio::stop();
    unsafe { ecl_make_integer(0) }
}

unsafe extern "C" fn native_finish_audio() -> ClObject {
    audio::finish();
    unsafe { ecl_make_integer(0) }
}

unsafe extern "C" fn native_canvas_clear(color: ClObject) -> ClObject {
    let result = (|| {
        canvas::clear(decode_color(color, "canvas clear color")?);
        Ok(())
    })();
    native_status(result)
}

unsafe extern "C" fn native_canvas_fill_rect(
    x: ClObject,
    y: ClObject,
    width: ClObject,
    height: ClObject,
    color: ClObject,
) -> ClObject {
    let result = (|| {
        canvas::fill_rect(
            decode_i32(x, "canvas rectangle x")?,
            decode_i32(y, "canvas rectangle y")?,
            decode_u32(width, "canvas rectangle width")?,
            decode_u32(height, "canvas rectangle height")?,
            decode_color(color, "canvas rectangle color")?,
        );
        Ok(())
    })();
    native_status(result)
}

unsafe extern "C" fn native_canvas_draw_glyph(
    x: ClObject,
    y: ClObject,
    character: ClObject,
    scale: ClObject,
    color: ClObject,
) -> ClObject {
    let result = (|| {
        let character = u8::try_from(decode_u32(character, "canvas glyph character")?)
            .map_err(|_| "canvas glyph character is out of range".to_owned())?;
        let scale = decode_u32(scale, "canvas glyph scale")?;
        if scale == 0 {
            return Err("canvas glyph scale must be positive".to_owned());
        }
        canvas::draw_glyph(
            decode_i32(x, "canvas glyph x")?,
            decode_i32(y, "canvas glyph y")?,
            character,
            scale,
            decode_color(color, "canvas glyph color")?,
        );
        Ok(())
    })();
    native_status(result)
}

unsafe extern "C" fn native_raster_clear() -> ClObject {
    canvas::clear_rasters();
    unsafe { ecl_make_integer(1) }
}

unsafe extern "C" fn native_raster_load_cover(path: ClObject, background: ClObject) -> ClObject {
    let result = (|| {
        canvas::load_cover_raster(
            &decode_path(path, "cover raster path")?,
            decode_color(background, "cover raster background")?,
        )
    })();
    native_handle(result)
}

unsafe extern "C" fn native_raster_load_png(
    path: ClObject,
    width: ClObject,
    height: ClObject,
) -> ClObject {
    let result = (|| {
        canvas::load_png_raster(
            &decode_path(path, "PNG raster path")?,
            decode_u32(width, "PNG raster width")?,
            decode_u32(height, "PNG raster height")?,
        )
    })();
    native_handle(result)
}

unsafe extern "C" fn native_canvas_draw_raster(
    handle: ClObject,
    x: ClObject,
    y: ClObject,
    width: ClObject,
    height: ClObject,
) -> ClObject {
    let result = (|| {
        canvas::draw_raster(
            decode_u32(handle, "canvas raster handle")?,
            decode_i32(x, "canvas raster x")?,
            decode_i32(y, "canvas raster y")?,
            decode_u32(width, "canvas raster width")?,
            decode_u32(height, "canvas raster height")?,
        )
    })();
    native_status(result)
}

unsafe extern "C" fn native_fbdev_open() -> ClObject {
    native_status(fbdev::open())
}

unsafe extern "C" fn native_fbdev_close() -> ClObject {
    fbdev::close();
    unsafe { ecl_make_integer(0) }
}

unsafe extern "C" fn native_fbdev_present_canvas() -> ClObject {
    native_status(canvas::with_pixels(fbdev::present_rgba))
}

unsafe extern "C" fn native_fbdev_present_solid(color: ClObject) -> ClObject {
    let result = (|| fbdev::present_solid(decode_color(color, "fbdev solid color")?))();
    native_status(result)
}

unsafe extern "C" fn native_fbdev_size() -> ClObject {
    let Some((width, height)) = fbdev::size() else {
        return ECL_NIL;
    };
    make_fixnum_list(&[width as ClFixnum, height as ClFixnum])
}

unsafe extern "C" fn native_wayland_open_widget() -> ClObject {
    native_status(wayland::open_widget())
}

unsafe extern "C" fn native_wayland_close() -> ClObject {
    wayland::close();
    unsafe { ecl_make_integer(0) }
}

unsafe extern "C" fn native_wayland_present_canvas() -> ClObject {
    native_status(canvas::with_pixels(wayland::present_rgba))
}

unsafe extern "C" fn native_wayland_present_solid(color: ClObject) -> ClObject {
    let result = (|| wayland::present_solid(decode_color(color, "Wayland solid color")?))();
    native_status(result)
}

unsafe extern "C" fn native_wayland_dispatch(timeout_ms: ClObject) -> ClObject {
    let result = (|| {
        let timeout_ms = decode_u32(timeout_ms, "Wayland dispatch timeout")?;
        wayland::dispatch(timeout_ms)
    })();
    let value = match result {
        Ok(count) => match ClFixnum::try_from(count) {
            Ok(count) => count,
            Err(_) => {
                eprintln!("retrodeck: Wayland dispatch count is out of range");
                -1
            }
        },
        Err(error) => {
            eprintln!("retrodeck: {error}");
            -1
        }
    };
    unsafe { ecl_make_integer(value) }
}

unsafe extern "C" fn native_wayland_next_touch() -> ClObject {
    let Some(report) = wayland::next_touch() else {
        return ECL_NIL;
    };
    make_fixnum_list(&[
        report.x as ClFixnum,
        report.y as ClFixnum,
        boolean_fixnum(report.down),
        boolean_fixnum(report.pressed),
        boolean_fixnum(report.released),
    ])
}

unsafe extern "C" fn native_wayland_size() -> ClObject {
    let Some((width, height)) = wayland::size() else {
        return ECL_NIL;
    };
    make_fixnum_list(&[width as ClFixnum, height as ClFixnum])
}

unsafe extern "C" fn native_wayland_shutdown() -> ClObject {
    unsafe { ecl_make_integer(boolean_fixnum(wayland::shutdown_requested())) }
}

fn native_status(result: Result<(), String>) -> ClObject {
    let status = match result {
        Ok(()) => 1,
        Err(error) => {
            eprintln!("retrodeck: {error}");
            0
        }
    };
    unsafe { ecl_make_integer(status) }
}

fn native_handle(result: Result<u32, String>) -> ClObject {
    let handle = match result {
        Ok(handle) if u64::from(handle) <= (ClFixnum::MAX as u64 >> 2) => handle as ClFixnum,
        Ok(_) => {
            eprintln!("retrodeck: native raster handle is out of ECL fixnum range");
            0
        }
        Err(error) => {
            eprintln!("retrodeck: {error}");
            0
        }
    };
    unsafe { ecl_make_integer(handle) }
}

fn make_fixnum_list(values: &[ClFixnum]) -> ClObject {
    let mut list = ECL_NIL;
    for value in values.iter().rev() {
        unsafe {
            list = ecl_cons(ecl_make_integer(*value), list);
        }
    }
    list
}

fn boolean_fixnum(value: bool) -> ClFixnum {
    if value { 1 } else { 0 }
}

fn c_string(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "an internal ECL name contains NUL".to_owned())
}

fn decode_path(object: ClObject, name: &str) -> Result<PathBuf, String> {
    let length = unsafe { ecl_length(object) };
    if length < 0 {
        return Err(format!("{name} has an invalid length"));
    }
    let pointer = unsafe { ecl_base_string_pointer_safe(object) }.cast::<u8>();
    if pointer.is_null() {
        return Err(format!("{name} is unavailable"));
    }
    let bytes = unsafe { std::slice::from_raw_parts(pointer, length as usize) };
    if bytes.contains(&0) {
        return Err(format!("{name} cannot contain NUL"));
    }
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

fn decode_fixnum(object: ClObject) -> Option<ClFixnum> {
    let tagged = object as usize;
    (tagged & 3 == FIXNUM_TAG).then_some((tagged as isize) >> 2)
}

fn decode_i32(object: ClObject, name: &str) -> Result<c_int, String> {
    let value = decode_fixnum(object).ok_or_else(|| format!("{name} must be an integer"))?;
    c_int::try_from(value).map_err(|_| format!("{name} is out of range"))
}

fn decode_u32(object: ClObject, name: &str) -> Result<u32, String> {
    let value = decode_fixnum(object).ok_or_else(|| format!("{name} must be an integer"))?;
    u32::try_from(value).map_err(|_| format!("{name} is out of range"))
}

fn decode_color(object: ClObject, name: &str) -> Result<u32, String> {
    let color = decode_u32(object, name)?;
    (color <= 0x00ff_ffff)
        .then_some(color)
        .ok_or_else(|| format!("{name} is out of range"))
}

fn decode_exit_code(object: ClObject) -> Result<u8, String> {
    let value = decode_fixnum(object)
        .ok_or_else(|| "RETRODECK:MAIN must return an integer exit status".to_owned())?;
    u8::try_from(value).map_err(|_| "RETRODECK:MAIN returned an invalid exit status".to_owned())
}

fn startup_path() -> Result<PathBuf, String> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    match (arguments.next(), arguments.next()) {
        (None, None) => Ok(PathBuf::from(DEFAULT_STARTUP)),
        (Some(path), None) => Ok(path.into()),
        _ => Err("usage: retrodeck-native [STARTUP.LISP]".to_owned()),
    }
}

fn run() -> Result<u8, String> {
    let startup = startup_path()?;
    let ecl = Ecl::boot()?;
    ecl.register_primitives()?;
    ecl.load(&startup)?;
    ecl.run()
}

fn main() -> ExitCode {
    match run() {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            eprintln!("retrodeck: {error}");
            ExitCode::FAILURE
        }
    }
}
