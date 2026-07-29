use std::{
    env, fs,
    path::Path,
    process, thread,
    time::{Duration, Instant},
};

fn main() {
    match env::args().nth(1).as_deref() {
        Some("--version") => {
            thread::sleep(Duration::from_millis(100));
            println!("my-app 1.0.0");
        }
        Some("echo-args") => println!("{}", env::args().nth(2).unwrap()),
        Some("assert-exact-args")
            if env::args().skip(1).eq([
                "assert-exact-args",
                "two words",
                "",
                "embedded \"quote\"",
                "trailing backslashes \\\\",
            ]) => {}
        Some("assert-exact-args") => process::exit(65),
        Some("sleep-ms") => {
            let ms: u64 = env::args().nth(2).unwrap().parse().unwrap();
            thread::sleep(Duration::from_millis(ms));
        }
        Some("vary-sleep-ms") => {
            let state_path = env::args().nth(2).unwrap();
            let first_ms: u64 = env::args().nth(3).unwrap().parse().unwrap();
            let subsequent_ms: u64 = env::args().nth(4).unwrap().parse().unwrap();
            let count = fs::read_to_string(&state_path)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0);
            fs::write(state_path, (count + 1).to_string()).unwrap();
            thread::sleep(Duration::from_millis(if count == 0 {
                first_ms
            } else {
                subsequent_ms
            }));
        }
        Some("allocate-mib") => {
            let mib: usize = env::args().nth(2).unwrap().parse().unwrap();
            let mut bytes = vec![0_u8; mib * 1024 * 1024];
            bytes.iter_mut().step_by(4096).for_each(|byte| *byte = 1);
            thread::sleep(Duration::from_millis(100));
            std::hint::black_box(bytes);
        }
        #[cfg(windows)]
        Some("commit-untouched-mib") => {
            let mib: usize = env::args().nth(2).unwrap().parse().unwrap();
            let byte_count = mib.checked_mul(1024 * 1024).unwrap();
            // SAFETY: null requests an OS-selected base; the size and flags
            // describe an ordinary private read/write reservation.
            let allocation = unsafe {
                VirtualAlloc(
                    std::ptr::null_mut(),
                    byte_count,
                    MEM_COMMIT | MEM_RESERVE,
                    PAGE_READWRITE,
                )
            };
            assert!(!allocation.is_null());
            thread::sleep(Duration::from_millis(100));
            std::hint::black_box(allocation);
            // SAFETY: allocation is the exact base returned by VirtualAlloc;
            // MEM_RELEASE requires a zero size.
            assert_ne!(unsafe { VirtualFree(allocation, 0, MEM_RELEASE) }, 0);
        }
        Some("spawn-allocator") => {
            let mib = env::args().nth(2).unwrap_or_else(|| "32".to_owned());
            let status = std::process::Command::new(env::current_exe().unwrap())
                .args(["allocate-mib", &mib])
                .status()
                .unwrap();
            process::exit(status.code().unwrap_or(1));
        }
        Some("burn-cpu-ms") => {
            let milliseconds: u64 = env::args().nth(2).unwrap().parse().unwrap();
            let started = Instant::now();
            let mut value = 1_u64;
            while started.elapsed() < Duration::from_millis(milliseconds) {
                value = std::hint::black_box(
                    value
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1),
                );
            }
            std::hint::black_box(value);
        }
        Some("spawn-cpu-burner") => {
            let milliseconds = env::args().nth(2).unwrap_or_else(|| "300".to_owned());
            let status = std::process::Command::new(env::current_exe().unwrap())
                .args(["burn-cpu-ms", &milliseconds])
                .status()
                .unwrap();
            process::exit(status.code().unwrap_or(1));
        }
        Some("spawn-child") => {
            let status = std::process::Command::new(env::current_exe().unwrap())
                .args(["sleep-ms", "10"])
                .status()
                .unwrap();
            process::exit(status.code().unwrap_or(1));
        }
        Some("spawn-sleeper") => {
            let mut child = std::process::Command::new(env::current_exe().unwrap())
                .args(["sleep-ms", "30000"])
                .spawn()
                .unwrap();
            if let Some(pid_path) = env::args().nth(2) {
                publish_pid_atomically(Path::new(&pid_path), child.id()).unwrap();
            }
            child.wait().unwrap();
        }
        Some("spawn-sleeper-and-exit") => {
            let child = std::process::Command::new(env::current_exe().unwrap())
                .args(["sleep-ms", "30000"])
                .spawn()
                .unwrap();
            if let Some(pid_path) = env::args().nth(2) {
                publish_pid_atomically(Path::new(&pid_path), child.id()).unwrap();
            }
            let exit_code = env::args()
                .nth(3)
                .map(|value| value.parse().unwrap())
                .unwrap_or(0);
            process::exit(exit_code);
        }
        Some("verbose-exit") => {
            let code: i32 = env::args().nth(2).unwrap().parse().unwrap();
            println!("fixture stdout must stay private");
            eprintln!("fixture stderr must stay private");
            process::exit(code);
        }
        Some("verbose-sleep-ms") => {
            let ms: u64 = env::args().nth(2).unwrap().parse().unwrap();
            println!("fixture stdout must stay private");
            eprintln!("fixture stderr must stay private");
            thread::sleep(Duration::from_millis(ms));
        }
        #[cfg(windows)]
        Some("signal-event-handle") => {
            let handle: usize = env::args().nth(2).unwrap().parse().unwrap();
            // SAFETY: the test supplies its sentinel event value. SetEvent
            // fails harmlessly when that handle was excluded from inheritance.
            unsafe {
                SetEvent(handle as *mut std::ffi::c_void);
            }
        }
        Some("exit") => {
            let code: i32 = env::args().nth(2).unwrap().parse().unwrap();
            process::exit(code);
        }
        _ => process::exit(64),
    }
}

#[cfg(windows)]
const MEM_COMMIT: u32 = 0x0000_1000;
#[cfg(windows)]
const MEM_RESERVE: u32 = 0x0000_2000;
#[cfg(windows)]
const MEM_RELEASE: u32 = 0x0000_8000;
#[cfg(windows)]
const PAGE_READWRITE: u32 = 0x04;

#[cfg(windows)]
unsafe extern "system" {
    fn VirtualAlloc(
        address: *mut std::ffi::c_void,
        size: usize,
        allocation_type: u32,
        protect: u32,
    ) -> *mut std::ffi::c_void;
    fn VirtualFree(address: *mut std::ffi::c_void, size: usize, free_type: u32) -> i32;
    fn SetEvent(event: *mut std::ffi::c_void) -> i32;
}

fn publish_pid_atomically(path: &Path, pid: u32) -> std::io::Result<()> {
    let mut temporary_name = path.as_os_str().to_owned();
    temporary_name.push(format!(".{}.tmp", process::id()));
    let temporary_path = Path::new(&temporary_name);
    fs::write(temporary_path, pid.to_string())?;
    fs::rename(temporary_path, path)
}
