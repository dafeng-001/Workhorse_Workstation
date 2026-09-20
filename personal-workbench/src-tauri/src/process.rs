use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX
#[cfg(windows)]
const SEM_FLAGS: u32 = 0x0001 | 0x0002 | 0x8000;

pub fn suppress_error_dialogs() {
    #[cfg(windows)]
    unsafe {
        extern "system" {
            fn SetErrorMode(u_mode: u32) -> u32;
        }
        SetErrorMode(SEM_FLAGS);
    }
}

fn base_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Never block on credential / ssh prompts.
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_ASKPASS", "echo");
    cmd.env("GCM_INTERACTIVE", "never");
    cmd.env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Run a program with a hard timeout. Kills the child if it exceeds `timeout`.
pub fn run_capture_timeout(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout: Duration,
) -> anyhow::Result<std::process::Output> {
    use std::io::Read;
    use std::process::Child;

    let mut cmd = base_command(program);
    cmd.args(args);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child: Child = cmd.spawn()?;

    // Drain pipes on helper threads so the child cannot block on a full pipe.
    let mut out_pipe = child.stdout.take();
    let mut err_pipe = child.stderr.take();
    let out_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = out_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_h = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = err_pipe.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let start = Instant::now();
    let status = loop {
        match child.try_wait()? {
            Some(st) => break st,
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    anyhow::bail!("timeout after {:?}", timeout);
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };

    let stdout = out_h.join().unwrap_or_default();
    let stderr = err_h.join().unwrap_or_default();
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

pub fn run_capture(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
) -> anyhow::Result<std::process::Output> {
    run_capture_timeout(program, args, cwd, Duration::from_secs(8))
}

/// Launch a GUI process fully hidden (no taskbar flash).
#[cfg(windows)]
pub fn spawn_hidden_gui(program: &Path, args: &[&str], cwd: Option<&Path>) -> anyhow::Result<()> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    const STARTF_USESHOWWINDOW: u32 = 0x0000_0001;
    const SW_HIDE: u16 = 0;

    #[repr(C)]
    struct StartupInfoW {
        cb: u32,
        lp_reserved: *mut u16,
        lp_desktop: *mut u16,
        lp_title: *mut u16,
        dw_x: u32,
        dw_y: u32,
        dw_x_size: u32,
        dw_y_size: u32,
        dw_x_count_chars: u32,
        dw_y_count_chars: u32,
        dw_fill_attribute: u32,
        dw_flags: u32,
        w_show_window: u16,
        cb_reserved2: u16,
        lp_reserved2: *mut u8,
        h_std_input: *mut c_void,
        h_std_output: *mut c_void,
        h_std_error: *mut c_void,
    }

    #[repr(C)]
    struct ProcessInformation {
        h_process: *mut c_void,
        h_thread: *mut c_void,
        dw_process_id: u32,
        dw_thread_id: u32,
    }

    extern "system" {
        fn CreateProcessW(
            lp_application_name: *const u16,
            lp_command_line: *mut u16,
            lp_process_attributes: *mut c_void,
            lp_thread_attributes: *mut c_void,
            b_inherit_handles: i32,
            dw_creation_flags: u32,
            lp_environment: *mut c_void,
            lp_current_directory: *const u16,
            lp_startup_info: *mut StartupInfoW,
            lp_process_information: *mut ProcessInformation,
        ) -> i32;
        fn CloseHandle(h: *mut c_void) -> i32;
    }

    let app: Vec<u16> = program.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut cmdline = format!("\"{}\"", program.display());
    for a in args {
        cmdline.push(' ');
        if a.contains(' ') {
            cmdline.push_str(&format!("\"{a}\""));
        } else {
            cmdline.push_str(a);
        }
    }
    let mut cl: Vec<u16> = cmdline.encode_utf16().chain(Some(0)).collect();
    let cwd_w: Option<Vec<u16>> = cwd.map(|c| c.as_os_str().encode_wide().chain(Some(0)).collect());

    let mut si = StartupInfoW {
        cb: std::mem::size_of::<StartupInfoW>() as u32,
        lp_reserved: std::ptr::null_mut(),
        lp_desktop: std::ptr::null_mut(),
        lp_title: std::ptr::null_mut(),
        dw_x: 0,
        dw_y: 0,
        dw_x_size: 0,
        dw_y_size: 0,
        dw_x_count_chars: 0,
        dw_y_count_chars: 0,
        dw_fill_attribute: 0,
        dw_flags: STARTF_USESHOWWINDOW,
        w_show_window: SW_HIDE,
        cb_reserved2: 0,
        lp_reserved2: std::ptr::null_mut(),
        h_std_input: std::ptr::null_mut(),
        h_std_output: std::ptr::null_mut(),
        h_std_error: std::ptr::null_mut(),
    };
    let mut pi = ProcessInformation {
        h_process: std::ptr::null_mut(),
        h_thread: std::ptr::null_mut(),
        dw_process_id: 0,
        dw_thread_id: 0,
    };

    let ok = unsafe {
        CreateProcessW(
            app.as_ptr(),
            cl.as_mut_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            CREATE_NO_WINDOW,
            std::ptr::null_mut(),
            cwd_w.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null()),
            &mut si,
            &mut pi,
        )
    };
    if ok == 0 {
        anyhow::bail!("CreateProcessW failed");
    }
    unsafe {
        if !pi.h_process.is_null() {
            CloseHandle(pi.h_process);
        }
        if !pi.h_thread.is_null() {
            CloseHandle(pi.h_thread);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn spawn_hidden_gui(program: &Path, args: &[&str], cwd: Option<&Path>) -> anyhow::Result<()> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Some(d) = cwd {
        cmd.current_dir(d);
    }
    cmd.spawn()?;
    Ok(())
}

/// Launch elevated (UAC). Returns true if the user approved and process started.
#[cfg(windows)]
pub fn spawn_elevated(program: &Path, args: &[&str], cwd: Option<&Path>) -> bool {
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    struct ShellExecuteInfoW {
        cb_size: u32,
        f_mask: u32,
        hwnd: *mut std::ffi::c_void,
        lp_verb: *const u16,
        lp_file: *const u16,
        lp_parameters: *mut u16,
        lp_directory: *const u16,
        n_show: i32,
        hinst_app: *mut std::ffi::c_void,
        lp_id_list: *mut std::ffi::c_void,
        lp_class: *const u16,
        hkey_class: *mut std::ffi::c_void,
        dw_hot_key: u32,
        h_icon: *mut std::ffi::c_void,
        h_process: *mut std::ffi::c_void,
    }

    const SEE_MASK_NOCLOSEPROCESS: u32 = 0x0000_0040;
    const SW_HIDE: i32 = 0;

    extern "system" {
        fn ShellExecuteExW(p_exec_info: *mut ShellExecuteInfoW) -> i32;
        fn CloseHandle(h: *mut std::ffi::c_void) -> i32;
    }

    let file: Vec<u16> = program.as_os_str().encode_wide().chain(Some(0)).collect();
    let verb: Vec<u16> = std::ffi::OsStr::new("runas")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut params_s = args.join(" ");
    // quote args with spaces
    params_s = args
        .iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let mut params: Vec<u16> = params_s.encode_utf16().chain(Some(0)).collect();
    let dir_w: Option<Vec<u16>> =
        cwd.map(|c| c.as_os_str().encode_wide().chain(Some(0)).collect());

    let mut sei = ShellExecuteInfoW {
        cb_size: std::mem::size_of::<ShellExecuteInfoW>() as u32,
        f_mask: SEE_MASK_NOCLOSEPROCESS,
        hwnd: std::ptr::null_mut(),
        lp_verb: verb.as_ptr(),
        lp_file: file.as_ptr(),
        lp_parameters: params.as_mut_ptr(),
        lp_directory: dir_w
            .as_ref()
            .map(|d| d.as_ptr())
            .unwrap_or(std::ptr::null()),
        n_show: SW_HIDE,
        hinst_app: std::ptr::null_mut(),
        lp_id_list: std::ptr::null_mut(),
        lp_class: std::ptr::null(),
        hkey_class: std::ptr::null_mut(),
        dw_hot_key: 0,
        h_icon: std::ptr::null_mut(),
        h_process: std::ptr::null_mut(),
    };

    let ok = unsafe { ShellExecuteExW(&mut sei) };
    if ok == 0 {
        return false;
    }
    // Wait briefly for install to finish (service install is quick).
    if !sei.h_process.is_null() {
        extern "system" {
            fn WaitForSingleObject(h: *mut std::ffi::c_void, ms: u32) -> u32;
        }
        unsafe {
            WaitForSingleObject(sei.h_process, 15_000);
            CloseHandle(sei.h_process);
        }
    }
    true
}

#[cfg(not(windows))]
pub fn spawn_elevated(_program: &Path, _args: &[&str], _cwd: Option<&Path>) -> bool {
    false
}

/// Open a path with the shell without flashing a window.
pub fn open_path(path: &str) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("explorer.exe");
        cmd.arg(path);
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.spawn()?;
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(path);
        cmd.spawn()?;
    }
    Ok(())
}
