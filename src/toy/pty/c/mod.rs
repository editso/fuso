pub mod bindings {

    #[repr(C)]
    #[cfg(not(windows))]
    pub struct PtyProcess {
        pub pid: ::std::os::raw::c_int,
        pub pty: ::std::os::raw::c_int,
        pub argv: *const *mut ::std::os::raw::c_char,
        pub environs: *const *const ::std::os::raw::c_char,
        pub work: *const ::std::os::raw::c_char,
    }

    #[repr(C)]
    #[cfg(windows)]
    pub struct PtyProcess {
        pub pid: ::std::os::raw::c_int,
        pub pty: *const ::std::os::raw::c_void,
        pub cmdline: *const ::std::os::raw::c_char,
        pub environs: *const ::std::os::raw::c_ushort,
        pub work_dir: *const ::std::os::raw::c_char,

        pub wait: *const ::std::os::raw::c_void,
        pub handle: *const ::std::os::raw::c_void,
        pub startup_info: *const ::std::os::raw::c_void,
        pub pipe_input_name: *const u8,
        pub pipe_output_name: *const u8,

        pub exit_cb: *const extern "C" fn(*mut PtyProcess),
        pub exit_cb_data: *const ::std::os::raw::c_void,
    }

    extern "C" {
        pub fn pty_exit(process: *mut PtyProcess);
        pub fn pty_spawn(process: *mut PtyProcess) -> ::std::os::raw::c_int;
    }
}

#[cfg(windows)]
pub mod windows_ext {

    use std::os::windows;

    use crate::error;

    type DWORD = ::std::os::raw::c_ulong;

    const ENABLE_ECHO_INPUT: DWORD = 0x0004;
    const ENABLE_LINE_INPUT: DWORD = 0x0002;

    pub trait ConsoleExt {
        fn set_console_mode(&self, mode: DWORD) -> error::Result<()>;

        fn enable_input_mode(&self) -> error::Result<()> {
            self.set_console_mode(!(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT))
        }
    }

    impl<T> ConsoleExt for T
    where
        T: std::os::windows::io::AsRawHandle,
    {
        fn set_console_mode(&self, mode: DWORD) -> error::Result<()> {
            #[link(name = "kernel32")]
            extern "C" {
                fn GetLastError() -> DWORD;

                /// https://learn.microsoft.com/en-us/windows/console/setconsolemode
                fn SetConsoleMode(handle: windows::raw::HANDLE, mode: DWORD) -> bool;

                /// https://learn.microsoft.com/en-us/windows/console/getconsolemode
                fn GetConsoleMode(handle: windows::raw::HANDLE, mode: *mut DWORD) -> bool;
            }

            let handle = self.as_raw_handle();

            unsafe {
                let mut old_mode = DWORD::default();

                if !GetConsoleMode(handle, &mut old_mode as *mut DWORD) {
                    return Err(std::io::Error::from_raw_os_error(GetLastError() as _).into());
                }

                if !SetConsoleMode(handle, old_mode & mode) {
                    return Err(std::io::Error::from_raw_os_error(GetLastError() as _).into());
                }

                Ok(())
            }
        }
    }
}
