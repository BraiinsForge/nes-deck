use std::env;
use std::ffi::{CString, c_char, c_int, c_void};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::ptr;

type ClObject = *mut c_void;
type ClFixnum = isize;

const ECL_NIL: ClObject = 1usize as ClObject;
const FIXNUM_TAG: usize = 3;
const DEFAULT_STARTUP: &str = "/mnt/data/nes-deck/lisp/startup.lisp";
const ABI_VERSION: ClFixnum = 1;

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
    fn ecl_def_c_function(
        symbol: ClObject,
        function: unsafe extern "C" fn() -> ClObject,
        arguments: c_int,
    );
    fn ecl_make_integer(value: ClFixnum) -> ClObject;
    fn ecl_make_simple_base_string(value: *const c_char, length: ClFixnum) -> ClObject;
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

        let symbol_name = c_string("ABI-VERSION")?;
        let symbol = unsafe { ecl_make_symbol(symbol_name.as_ptr(), package_name.as_ptr()) };
        unsafe { ecl_def_c_function(symbol, native_abi_version, 0) };
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
        unsafe { cl_shutdown() };
    }
}

unsafe extern "C" fn native_abi_version() -> ClObject {
    unsafe { ecl_make_integer(ABI_VERSION) }
}

fn c_string(value: &str) -> Result<CString, String> {
    CString::new(value).map_err(|_| "an internal ECL name contains NUL".to_owned())
}

fn decode_exit_code(object: ClObject) -> Result<u8, String> {
    let tagged = object as usize;
    if tagged & 3 != FIXNUM_TAG {
        return Err("RETRODECK:MAIN must return an integer exit status".to_owned());
    }
    let value = (tagged as isize) >> 2;
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
