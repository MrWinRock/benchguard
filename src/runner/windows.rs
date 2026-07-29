use std::{
    env,
    ffi::OsStr,
    fs, io, mem,
    os::windows::ffi::OsStrExt,
    path::Path,
    ptr, thread,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, GENERIC_READ, GENERIC_WRITE, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, IsProcessInJob,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION,
            JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAndIoAccountingInformation, JobObjectBasicProcessIdList,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject,
        },
        ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::{
            CREATE_SUSPENDED, CreateProcessW, DeleteProcThreadAttributeList,
            EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, INFINITE,
            InitializeProcThreadAttributeList, OpenProcess, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            PROCESS_INFORMATION, PROCESS_QUERY_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_VM_READ,
            ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess,
            UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

use super::CommandSpec;
use crate::{domain::Sample, error::BenchguardError};

const HUNDRED_NS_TO_NS: u64 = 100;
const MAX_FINITE_WAIT_MS: u32 = INFINITE - 1;
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);
const CLEANUP_POLL_INTERVAL: Duration = Duration::from_millis(1);
const METRIC_POLL_INTERVAL: Duration = Duration::from_millis(5);

pub(super) fn platform_run_once(
    spec: &CommandSpec,
    timeout: Option<Duration>,
) -> Result<Sample, BenchguardError> {
    // Peak memory is the greatest aggregate current working set sampled from
    // active Job Object members every 5 ms. A process that starts and exits
    // entirely between samples, or exits between enumeration and query, may
    // be missed. CPU time remains the Job Object's cumulative accounting.
    let started = Instant::now();
    let job = create_kill_on_close_job()?;
    let application_name = resolve_application_name(spec.program.as_os_str())?;
    let mut command_line = build_command_line(spec)?;
    let null_device = open_inheritable_null_device()?;
    let inherited_handles = [null_device.raw()];
    let attribute_list = ProcThreadAttributeList::with_handles(&inherited_handles)?;
    let startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: size_u32::<STARTUPINFOEXW>()?,
            dwFlags: STARTF_USESTDHANDLES,
            hStdInput: null_device.raw(),
            hStdOutput: null_device.raw(),
            hStdError: null_device.raw(),
            ..Default::default()
        },
        lpAttributeList: attribute_list.raw(),
    };
    let mut process_info = PROCESS_INFORMATION::default();

    // SAFETY: application_name and command_line are live, NUL-terminated UTF-16
    // buffers. The command line is writable as required by CreateProcessW.
    // Security/environment/current-directory pointers are null to inherit the
    // caller defaults. The extended startup attribute list whitelists only the
    // null stdio handle. startup and process_info are valid structures.
    let created = unsafe {
        CreateProcessW(
            application_name.as_ptr(),
            command_line.as_mut_ptr(),
            ptr::null(),
            ptr::null(),
            1,
            CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
            ptr::null(),
            ptr::null(),
            &startup.StartupInfo,
            &mut process_info,
        )
    };
    if created == 0 {
        return Err(BenchguardError::CommandLaunch {
            source: io::Error::last_os_error(),
        });
    }

    // SAFETY: successful CreateProcessW guarantees both returned handles are
    // valid and transfers ownership to the caller.
    let mut child = unsafe { CreatedProcess::from_process_info(process_info) };

    // SAFETY: job and process are valid owned handles. The process remains
    // suspended, so it cannot create an untracked descendant before assignment.
    if unsafe { AssignProcessToJobObject(job.raw(), child.process.raw()) } == 0 {
        let source = io::Error::last_os_error();
        let _ = child.terminate_unassigned_bounded();
        return Err(measurement_error(
            "assign benchmark process to Windows Job Object",
            source,
        ));
    }
    child.job_owns_cleanup();
    let mut execution = JobExecution::new(job, child);
    execution.resume()?;

    match execution.wait(started, timeout)? {
        WaitOutcome::Completed => {
            let exit_code = execution.exit_code()?;
            if exit_code != 0 {
                return Err(BenchguardError::CommandFailed { exit_code });
            }
            let (cpu_ns, peak_memory_bytes) = execution.metrics()?;
            let wall_ns = duration_ns(started.elapsed())?;
            execution.finish();
            Ok(Sample {
                wall_ns,
                cpu_ns,
                peak_memory_bytes,
                exit_code,
            })
        }
        WaitOutcome::TimedOut => {
            execution.terminate_and_reap()?;
            Err(BenchguardError::Timeout)
        }
    }
}

struct ProcThreadAttributeList {
    _storage: Vec<usize>,
    raw: *mut std::ffi::c_void,
}

impl ProcThreadAttributeList {
    fn with_handles(handles: &[HANDLE]) -> Result<Self, BenchguardError> {
        let mut byte_count = 0_usize;
        // SAFETY: a null first call is the documented size query; byte_count
        // receives the required allocation size for one attribute.
        unsafe {
            InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut byte_count);
        }
        if byte_count == 0 {
            return Err(last_measurement_error(
                "size Windows process thread attribute list",
            ));
        }

        let word_count = byte_count.div_ceil(mem::size_of::<usize>());
        let mut storage = vec![0_usize; word_count];
        let raw = storage.as_mut_ptr().cast();
        // SAFETY: storage is aligned and at least byte_count bytes long.
        if unsafe { InitializeProcThreadAttributeList(raw, 1, 0, &mut byte_count) } == 0 {
            return Err(last_measurement_error(
                "initialize Windows process thread attribute list",
            ));
        }
        let list = Self {
            _storage: storage,
            raw,
        };
        let handles_size = mem::size_of_val(handles);
        // SAFETY: list is initialized for one attribute; handles is a live
        // array of inheritable handles and the API copies the attribute value
        // into list for the subsequent CreateProcessW call.
        if unsafe {
            UpdateProcThreadAttribute(
                list.raw,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                handles_size,
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(last_measurement_error(
                "restrict Windows benchmark inherited handles",
            ));
        }
        Ok(list)
    }

    fn raw(&self) -> *mut std::ffi::c_void {
        self.raw
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        // SAFETY: raw is initialized exactly once and remains backed by
        // _storage until after this destructor returns.
        unsafe {
            DeleteProcThreadAttributeList(self.raw);
        }
    }
}

fn open_inheritable_null_device() -> Result<OwnedHandle, BenchguardError> {
    let security = SECURITY_ATTRIBUTES {
        nLength: size_u32::<SECURITY_ATTRIBUTES>()?,
        lpSecurityDescriptor: ptr::null_mut(),
        bInheritHandle: 1,
    };
    let name = [u16::from(b'N'), u16::from(b'U'), u16::from(b'L'), 0];
    // SAFETY: name is a NUL-terminated UTF-16 device path and security is a
    // valid inheritable descriptor. The returned handle is owned by the caller.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            &security,
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(last_measurement_error(
            "open null device for benchmark output",
        ));
    }
    OwnedHandle::new(raw)
        .ok_or_else(|| last_measurement_error("open null device for benchmark output"))
}

fn create_kill_on_close_job() -> Result<OwnedHandle, BenchguardError> {
    // SAFETY: null security attributes and name request an unnamed Job Object
    // with default security. A non-null result transfers ownership to us.
    let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    let job =
        OwnedHandle::new(raw).ok_or_else(|| last_measurement_error("create Windows Job Object"))?;
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

    // SAFETY: limits points to the correctly sized structure for the requested
    // information class and job is a live Job Object handle.
    if unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            ptr::from_ref(&limits).cast(),
            size_u32::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()?,
        )
    } == 0
    {
        return Err(last_measurement_error(
            "configure Windows Job Object cleanup",
        ));
    }
    Ok(job)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitOutcome {
    Completed,
    TimedOut,
}

struct JobExecution {
    job: Option<OwnedHandle>,
    child: CreatedProcess,
    cleanup_required: bool,
    peak_working_set_bytes: u64,
}

impl JobExecution {
    fn new(job: OwnedHandle, child: CreatedProcess) -> Self {
        Self {
            job: Some(job),
            child,
            cleanup_required: true,
            peak_working_set_bytes: 0,
        }
    }

    fn resume(&mut self) -> Result<(), BenchguardError> {
        let thread = self
            .child
            .thread
            .take()
            .expect("primary thread is owned until resume");
        // SAFETY: thread is the suspended primary thread returned by
        // CreateProcessW. u32::MAX is the documented failure result.
        if unsafe { ResumeThread(thread.raw()) } == u32::MAX {
            return Err(last_measurement_error(
                "resume benchmark process primary thread",
            ));
        }
        Ok(())
    }

    fn wait(
        &mut self,
        started: Instant,
        timeout: Option<Duration>,
    ) -> Result<WaitOutcome, BenchguardError> {
        loop {
            self.observe_resident_memory()?;
            let wait_millis = match timeout {
                None => duration_to_wait_millis(METRIC_POLL_INTERVAL),
                Some(limit) => {
                    let remaining = limit.saturating_sub(started.elapsed());
                    duration_to_wait_millis(remaining.min(METRIC_POLL_INTERVAL))
                }
            };

            // SAFETY: process is a live process handle and wait_millis is a
            // valid Windows wait duration.
            match unsafe { WaitForSingleObject(self.child.process.raw(), wait_millis) } {
                WAIT_OBJECT_0 => return Ok(WaitOutcome::Completed),
                WAIT_TIMEOUT => {
                    if timeout.is_some_and(|limit| started.elapsed() >= limit) {
                        return Ok(WaitOutcome::TimedOut);
                    }
                }
                WAIT_FAILED => {
                    return Err(last_measurement_error("wait for benchmark process"));
                }
                result => {
                    return Err(measurement_error(
                        "wait for benchmark process",
                        io::Error::other(format!("unexpected Windows wait result {result}")),
                    ));
                }
            }
        }
    }

    fn observe_resident_memory(&mut self) -> Result<(), BenchguardError> {
        let mut aggregate_working_set = 0_u64;
        for pid in query_job_process_ids(self.job_raw())? {
            let Some(process) = open_process_for_memory_sampling(pid)? else {
                continue;
            };
            let mut belongs_to_job = 0;
            // SAFETY: process and job are live handles and belongs_to_job is
            // a writable BOOL. This closes the PID-reuse race after listing.
            if unsafe { IsProcessInJob(process.raw(), self.job_raw(), &mut belongs_to_job) } == 0 {
                return Err(last_measurement_error(
                    "confirm Windows resident-memory sample Job Object membership",
                ));
            }
            if belongs_to_job == 0 {
                continue;
            }

            let mut counters = PROCESS_MEMORY_COUNTERS {
                cb: size_u32::<PROCESS_MEMORY_COUNTERS>()?,
                ..Default::default()
            };
            // SAFETY: process has query/read access and counters is the exact
            // writable structure size supplied to K32GetProcessMemoryInfo.
            if unsafe {
                K32GetProcessMemoryInfo(
                    process.raw(),
                    &mut counters,
                    size_u32::<PROCESS_MEMORY_COUNTERS>()?,
                )
            } == 0
            {
                let memory_query_error = io::Error::last_os_error();
                // A listed process may exit between enumeration and this
                // query. Exited members are outside this 5 ms sample.
                let wait_result = unsafe { WaitForSingleObject(process.raw(), 0) };
                if resolve_memory_query_failure(memory_query_error, wait_result)? {
                    continue;
                }
            }
            aggregate_working_set = aggregate_working_set
                .checked_add(
                    u64::try_from(counters.WorkingSetSize)
                        .map_err(|_| BenchguardError::NumericOverflow)?,
                )
                .ok_or(BenchguardError::NumericOverflow)?;
        }
        self.peak_working_set_bytes = self.peak_working_set_bytes.max(aggregate_working_set);
        Ok(())
    }

    fn exit_code(&self) -> Result<i32, BenchguardError> {
        let mut exit_code = 0_u32;
        // SAFETY: process is a live process handle and exit_code is writable.
        if unsafe { GetExitCodeProcess(self.child.process.raw(), &mut exit_code) } == 0 {
            return Err(last_measurement_error("read benchmark process exit code"));
        }
        Ok(exit_code as i32)
    }

    fn metrics(&self) -> Result<(u64, u64), BenchguardError> {
        let accounting = query_job_information::<JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION>(
            self.job_raw(),
            JobObjectBasicAndIoAccountingInformation,
            "query Windows Job Object CPU accounting",
        )?;
        let user_100ns = u64::try_from(accounting.BasicInfo.TotalUserTime)
            .map_err(|_| BenchguardError::NumericOverflow)?;
        let kernel_100ns = u64::try_from(accounting.BasicInfo.TotalKernelTime)
            .map_err(|_| BenchguardError::NumericOverflow)?;
        let cpu_100ns = user_100ns
            .checked_add(kernel_100ns)
            .ok_or(BenchguardError::NumericOverflow)?;
        let cpu_ns = cpu_100ns
            .checked_mul(HUNDRED_NS_TO_NS)
            .ok_or(BenchguardError::NumericOverflow)?;
        Ok((cpu_ns, self.peak_working_set_bytes))
    }

    fn terminate_and_reap(&mut self) -> Result<(), BenchguardError> {
        // SAFETY: job is a live Job Object handle. Terminating the job covers
        // the leader and every descendant that inherited membership.
        if unsafe { TerminateJobObject(self.job_raw(), 1) } == 0 {
            let source = io::Error::last_os_error();
            self.close_job_for_kill_on_close();
            if wait_for_handle_bounded(self.child.process.raw(), CLEANUP_TIMEOUT).unwrap_or(false) {
                self.cleanup_required = false;
            }
            return Err(measurement_error(
                "terminate timed-out Windows Job Object",
                source,
            ));
        }

        let started = Instant::now();
        loop {
            let accounting =
                match query_job_information::<JOBOBJECT_BASIC_AND_IO_ACCOUNTING_INFORMATION>(
                    self.job_raw(),
                    JobObjectBasicAndIoAccountingInformation,
                    "confirm timed-out Windows Job Object cleanup",
                ) {
                    Ok(accounting) => accounting,
                    Err(error) => {
                        self.close_job_for_kill_on_close();
                        let _ = wait_for_handle_bounded(self.child.process.raw(), CLEANUP_TIMEOUT);
                        return Err(error);
                    }
                };
            if accounting.BasicInfo.ActiveProcesses == 0 {
                let leader_exited = wait_for_handle_bounded(
                    self.child.process.raw(),
                    CLEANUP_TIMEOUT.saturating_sub(started.elapsed()),
                )
                .map_err(|source| measurement_error("reap timed-out benchmark process", source))?;
                self.close_job_for_kill_on_close();
                if !leader_exited {
                    return Err(measurement_error(
                        "reap timed-out benchmark process",
                        cleanup_timeout_error(
                            "leader remained active after Job Object reached zero processes",
                        ),
                    ));
                }
                self.cleanup_required = false;
                return Ok(());
            }
            if started.elapsed() >= CLEANUP_TIMEOUT {
                self.close_job_for_kill_on_close();
                let _ = wait_for_handle_bounded(self.child.process.raw(), CLEANUP_TIMEOUT);
                return Err(measurement_error(
                    "confirm timed-out Windows Job Object cleanup",
                    cleanup_timeout_error(
                        "Job Object still had active processes after termination",
                    ),
                ));
            }
            thread::sleep(CLEANUP_POLL_INTERVAL);
        }
    }

    fn finish(&mut self) {
        self.cleanup_required = false;
        self.close_job_for_kill_on_close();
    }

    fn job_raw(&self) -> HANDLE {
        self.job
            .as_ref()
            .expect("Job handle is owned during execution")
            .raw()
    }

    fn close_job_for_kill_on_close(&mut self) {
        drop(self.job.take());
    }
}

fn open_process_for_memory_sampling(pid: u32) -> Result<Option<OwnedHandle>, BenchguardError> {
    // SAFETY: OpenProcess receives a concrete Job Object member PID. Query,
    // VM-read, and synchronization rights cover both the memory query and the
    // exit-race confirmation performed on this same handle.
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if raw.is_null() {
        let source = io::Error::last_os_error();
        if source.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            return Ok(None);
        }
        return Err(measurement_error(
            "open Windows Job Object process for resident-memory sampling",
            source,
        ));
    }
    Ok(Some(
        OwnedHandle::new(raw).expect("OpenProcess returned a non-null handle"),
    ))
}

fn resolve_memory_query_failure(
    memory_query_error: io::Error,
    exit_probe_result: u32,
) -> Result<bool, BenchguardError> {
    match exit_probe_result {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_FAILED => Err(measurement_error(
            "query Windows process working set after exit confirmation failed",
            memory_query_error,
        )),
        _ => Err(measurement_error(
            "query Windows process working set",
            memory_query_error,
        )),
    }
}

impl Drop for JobExecution {
    fn drop(&mut self) {
        if self.cleanup_required {
            if let Some(job) = self.job.as_ref() {
                // SAFETY: job remains a live owned handle during this call.
                unsafe {
                    TerminateJobObject(job.raw(), 1);
                }
            }
            self.close_job_for_kill_on_close();
            let _ = wait_for_handle_bounded(self.child.process.raw(), CLEANUP_TIMEOUT);
        }
    }
}

struct CreatedProcess {
    process: OwnedHandle,
    thread: Option<OwnedHandle>,
    terminate_process_on_drop: bool,
}

impl CreatedProcess {
    unsafe fn from_process_info(info: PROCESS_INFORMATION) -> Self {
        Self {
            // SAFETY: caller upholds CreateProcessW's successful-handle
            // guarantee and transfers each handle exactly once.
            process: unsafe { OwnedHandle::from_valid(info.hProcess) },
            // SAFETY: same guarantee applies independently to hThread.
            thread: Some(unsafe { OwnedHandle::from_valid(info.hThread) }),
            terminate_process_on_drop: true,
        }
    }

    fn job_owns_cleanup(&mut self) {
        self.terminate_process_on_drop = false;
    }

    fn terminate_unassigned_bounded(&mut self) -> Result<(), io::Error> {
        // SAFETY: process is an owned process handle. This path runs before
        // successful Job Object assignment, so direct termination is required.
        let termination_error = (unsafe { TerminateProcess(self.process.raw(), 1) } == 0)
            .then(io::Error::last_os_error);
        let exited = wait_for_handle_bounded(self.process.raw(), CLEANUP_TIMEOUT)?;
        if exited {
            self.terminate_process_on_drop = false;
        }
        if let Some(source) = termination_error {
            Err(source)
        } else if exited {
            Ok(())
        } else {
            Err(cleanup_timeout_error(
                "unassigned process remained active after termination",
            ))
        }
    }
}

impl Drop for CreatedProcess {
    fn drop(&mut self) {
        if self.terminate_process_on_drop {
            let _ = self.terminate_unassigned_bounded();
        }
    }
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(raw: HANDLE) -> Option<Self> {
        (!raw.is_null()).then_some(Self(raw))
    }

    unsafe fn from_valid(raw: HANDLE) -> Self {
        debug_assert!(!raw.is_null());
        Self(raw)
    }

    fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: OwnedHandle is constructed only from an owned, non-null
        // HANDLE and cannot be cloned, so this is its unique closing path.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn query_job_process_ids(job: HANDLE) -> Result<Vec<u32>, BenchguardError> {
    let mut capacity = 16_usize;
    loop {
        let bytes = mem::size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>()
            .checked_add(
                capacity
                    .saturating_sub(1)
                    .checked_mul(mem::size_of::<usize>())
                    .ok_or(BenchguardError::NumericOverflow)?,
            )
            .ok_or(BenchguardError::NumericOverflow)?;
        let word_count = bytes.div_ceil(mem::size_of::<usize>());
        let mut storage = vec![0_usize; word_count];
        let list = storage
            .as_mut_ptr()
            .cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>();
        let buffer_size = u32::try_from(bytes).map_err(|_| BenchguardError::NumericOverflow)?;

        // SAFETY: storage is usize-aligned and large enough for the fixed
        // header plus capacity flexible-array process IDs.
        let succeeded = unsafe {
            QueryInformationJobObject(
                job,
                JobObjectBasicProcessIdList,
                list.cast(),
                buffer_size,
                ptr::null_mut(),
            )
        };
        // SAFETY: QueryInformationJobObject initializes the fixed header even
        // when it reports that the flexible array needs a larger buffer.
        let assigned = unsafe { (*list).NumberOfAssignedProcesses as usize };
        // SAFETY: same initialized header contains the number actually copied.
        let listed = unsafe { (*list).NumberOfProcessIdsInList as usize };
        if succeeded == 0 {
            let source = io::Error::last_os_error();
            if source.raw_os_error() == Some(ERROR_MORE_DATA as i32) || assigned > capacity {
                capacity = assigned.max(capacity.saturating_mul(2));
                continue;
            }
            return Err(measurement_error(
                "enumerate Windows Job Object processes",
                source,
            ));
        }
        if listed > capacity {
            capacity = listed;
            continue;
        }

        // SAFETY: listed is bounded by the allocated flexible-array capacity.
        let process_ids =
            unsafe { std::slice::from_raw_parts((*list).ProcessIdList.as_ptr(), listed) };
        return process_ids
            .iter()
            .map(|&pid| u32::try_from(pid).map_err(|_| BenchguardError::NumericOverflow))
            .collect();
    }
}

fn query_job_information<T: Default>(
    job: HANDLE,
    class: i32,
    operation: &'static str,
) -> Result<T, BenchguardError> {
    let mut information = T::default();
    // SAFETY: information is a writable value with the exact size supplied to
    // QueryInformationJobObject for the caller-selected information class.
    if unsafe {
        QueryInformationJobObject(
            job,
            class,
            ptr::from_mut(&mut information).cast(),
            size_u32::<T>()?,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(last_measurement_error(operation));
    }
    Ok(information)
}

fn build_command_line(spec: &CommandSpec) -> Result<Vec<u16>, BenchguardError> {
    let mut command_line = Vec::new();
    for (index, argument) in std::iter::once(spec.program.as_os_str())
        .chain(spec.args.iter().map(|argument| argument.as_os_str()))
        .enumerate()
    {
        if index != 0 {
            command_line.push(u16::from(b' '));
        }
        append_quoted_argument(&mut command_line, argument)?;
    }
    command_line.push(0);
    Ok(command_line)
}

fn resolve_application_name(program: &OsStr) -> Result<Vec<u16>, BenchguardError> {
    if program.is_empty() {
        return Err(BenchguardError::CommandLaunch {
            source: io::Error::new(io::ErrorKind::InvalidInput, "benchmark program is empty"),
        });
    }
    let program_path = Path::new(program);
    if program_path.components().count() != 1 || program_path.file_name() != Some(program) {
        return nul_terminated(program);
    }

    let search_path = env::var_os("PATH").unwrap_or_default();
    let mut names = Vec::with_capacity(2);
    if program_path.extension().is_none() {
        let mut executable_name = program.to_os_string();
        executable_name.push(".exe");
        names.push(executable_name);
    }
    names.push(program.to_os_string());

    for directory in
        env::split_paths(&search_path).filter(|directory| !directory.as_os_str().is_empty())
    {
        for name in &names {
            let candidate = directory.join(name);
            if fs::metadata(&candidate).is_ok_and(|metadata| metadata.is_file()) {
                return nul_terminated(candidate.as_os_str());
            }
        }
    }

    Err(BenchguardError::CommandLaunch {
        source: io::Error::new(
            io::ErrorKind::NotFound,
            format!("benchmark program {:?} was not found in PATH", program),
        ),
    })
}

fn append_quoted_argument(
    command_line: &mut Vec<u16>,
    argument: &OsStr,
) -> Result<(), BenchguardError> {
    let wide = argument.encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(invalid_command_nul());
    }
    let requires_quotes = wide.is_empty()
        || wide
            .iter()
            .any(|unit| [b' ', b'\t', b'"'].map(u16::from).contains(unit));
    if !requires_quotes {
        command_line.extend(wide);
        return Ok(());
    }

    command_line.push(u16::from(b'"'));
    let mut backslashes = 0_usize;
    for unit in wide {
        if unit == u16::from(b'\\') {
            backslashes += 1;
        } else if unit == u16::from(b'"') {
            command_line.extend(std::iter::repeat_n(
                u16::from(b'\\'),
                backslashes
                    .checked_mul(2)
                    .and_then(|count| count.checked_add(1))
                    .ok_or(BenchguardError::NumericOverflow)?,
            ));
            command_line.push(unit);
            backslashes = 0;
        } else {
            command_line.extend(std::iter::repeat_n(u16::from(b'\\'), backslashes));
            command_line.push(unit);
            backslashes = 0;
        }
    }
    command_line.extend(std::iter::repeat_n(
        u16::from(b'\\'),
        backslashes
            .checked_mul(2)
            .ok_or(BenchguardError::NumericOverflow)?,
    ));
    command_line.push(u16::from(b'"'));
    Ok(())
}

fn nul_terminated(value: &OsStr) -> Result<Vec<u16>, BenchguardError> {
    let mut wide = value.encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(invalid_command_nul());
    }
    wide.push(0);
    Ok(wide)
}

fn invalid_command_nul() -> BenchguardError {
    BenchguardError::CommandLaunch {
        source: io::Error::new(
            io::ErrorKind::InvalidInput,
            "benchmark command contains an interior NUL",
        ),
    }
}

fn duration_to_wait_millis(duration: Duration) -> u32 {
    if duration.is_zero() {
        return 0;
    }
    let rounded_up = duration
        .as_millis()
        .saturating_add(u128::from(duration.subsec_nanos() % 1_000_000 != 0));
    u32::try_from(rounded_up.min(u128::from(MAX_FINITE_WAIT_MS)))
        .expect("bounded Windows wait duration fits u32")
}

fn wait_for_handle_bounded(handle: HANDLE, timeout: Duration) -> Result<bool, io::Error> {
    // SAFETY: callers provide a live synchronization handle and the converted
    // timeout is always finite.
    match unsafe { WaitForSingleObject(handle, duration_to_wait_millis(timeout)) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        result => Err(io::Error::other(format!(
            "unexpected Windows wait result {result}"
        ))),
    }
}

fn cleanup_timeout_error(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, message)
}

fn duration_ns(duration: Duration) -> Result<u64, BenchguardError> {
    u64::try_from(duration.as_nanos()).map_err(|_| BenchguardError::NumericOverflow)
}

fn size_u32<T>() -> Result<u32, BenchguardError> {
    u32::try_from(mem::size_of::<T>()).map_err(|_| BenchguardError::NumericOverflow)
}

fn last_measurement_error(operation: &'static str) -> BenchguardError {
    measurement_error(operation, io::Error::last_os_error())
}

fn measurement_error(operation: &'static str, source: io::Error) -> BenchguardError {
    BenchguardError::Measurement { operation, source }
}

#[cfg(test)]
mod tests {
    use super::{
        CLEANUP_TIMEOUT, CreatedProcess, JobExecution, OwnedHandle, build_command_line,
        create_kill_on_close_job, duration_to_wait_millis, open_process_for_memory_sampling,
        resolve_application_name, resolve_memory_query_failure, size_u32, wait_for_handle_bounded,
    };
    use crate::error::BenchguardError;
    use crate::runner::CommandSpec;
    use std::{
        io,
        process::Command,
        ptr, thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{DuplicateHandle, ERROR_ACCESS_DENIED, HANDLE},
        System::{
            JobObjects::AssignProcessToJobObject,
            Threading::{
                CREATE_SUSPENDED, CreateProcessW, GetCurrentProcess, PROCESS_INFORMATION,
                PROCESS_SYNCHRONIZE, STARTUPINFOW, TerminateProcess,
            },
        },
    };

    const CLEANUP_SCENARIO: &str = "BENCHGUARD_WINDOWS_CLEANUP_SCENARIO";
    const JOB_QUERY_ACCESS: u32 = 4;

    // Catches omitting empty arguments, failing to escape embedded quotes, or
    // failing to double trailing backslashes before a closing quote.
    #[test]
    fn command_line_encoding_preserves_windows_argument_boundaries() {
        let encoded = build_command_line(&CommandSpec::new(
            "program.exe",
            [
                "two words",
                "",
                "embedded \"quote\"",
                "trailing backslashes \\\\",
            ],
        ))
        .unwrap();

        assert_eq!(
            String::from_utf16(&encoded[..encoded.len() - 1]).unwrap(),
            "program.exe \"two words\" \"\" \"embedded \\\"quote\\\"\" \
             \"trailing backslashes \\\\\\\\\""
        );
        assert_eq!(encoded.last(), Some(&0));
    }

    // Catches sub-millisecond timeouts being truncated to an immediate poll,
    // or a finite wait being accidentally encoded as INFINITE.
    #[test]
    fn wait_duration_rounds_up_and_stays_finite() {
        assert_eq!(duration_to_wait_millis(Duration::ZERO), 0);
        assert_eq!(duration_to_wait_millis(Duration::from_nanos(1)), 1);
        assert_eq!(duration_to_wait_millis(Duration::from_millis(50)), 50);
        assert_eq!(
            duration_to_wait_millis(Duration::MAX),
            super::MAX_FINITE_WAIT_MS
        );
    }

    // Catches a failed TerminateJobObject call flowing into Drop, where a
    // second ignored failure used to be followed by an infinite process wait.
    #[test]
    fn failed_job_termination_does_not_hang_cleanup() {
        assert_cleanup_subprocess_exits("job-termination-failure");
    }

    // Catches pre-assignment cleanup waiting forever after TerminateProcess
    // fails for a process handle without PROCESS_TERMINATE access.
    #[test]
    fn failed_pre_assignment_termination_does_not_hang_cleanup() {
        assert_cleanup_subprocess_exits("process-termination-failure");
    }

    // Catches opening sampled processes without SYNCHRONIZE even though the
    // memory-query exit-race path waits on that exact handle.
    #[test]
    fn memory_sample_process_handle_can_confirm_process_exit() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runner::windows::tests::cleanup_failure_subprocess",
                "--nocapture",
            ])
            .env(CLEANUP_SCENARIO, "memory-sample-handle")
            .spawn()
            .unwrap();
        let process = open_process_for_memory_sampling(child.id())
            .unwrap()
            .expect("live test process should open");

        assert!(child.wait().unwrap().success());
        assert!(wait_for_handle_bounded(process.raw(), Duration::ZERO).unwrap());
    }

    // Catches WaitForSingleObject overwriting GetLastError after a failed
    // memory query and reporting the wait error instead of the root failure.
    #[test]
    fn failed_exit_probe_preserves_the_memory_query_error() {
        let error = resolve_memory_query_failure(
            io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32),
            windows_sys::Win32::Foundation::WAIT_FAILED,
        )
        .unwrap_err();

        let BenchguardError::Measurement { operation, source } = error else {
            panic!("unexpected error variant");
        };
        assert!(operation.contains("exit confirmation failed"));
        assert_eq!(source.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32));
    }

    #[test]
    fn cleanup_failure_subprocess() {
        match std::env::var(CLEANUP_SCENARIO).ok().as_deref() {
            None => {}
            Some("job-termination-failure") => job_termination_failure_scenario(),
            Some("process-termination-failure") => process_termination_failure_scenario(),
            Some("memory-sample-handle") => thread::sleep(Duration::from_millis(100)),
            Some(other) => panic!("unknown cleanup test scenario {other}"),
        }
    }

    fn assert_cleanup_subprocess_exits(scenario: &str) {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runner::windows::tests::cleanup_failure_subprocess",
                "--nocapture",
            ])
            .env(CLEANUP_SCENARIO, scenario)
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "cleanup subprocess failed: {status}");
                return;
            }
            if started.elapsed() >= Duration::from_secs(5) {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("{scenario} did not complete within the cleanup bound");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn job_termination_failure_scenario() {
        let job = create_kill_on_close_job().unwrap();
        let mut child = create_suspended_ping();
        // SAFETY: both handles are valid and the process is still suspended.
        assert_ne!(
            unsafe { AssignProcessToJobObject(job.raw(), child.process.raw()) },
            0
        );
        child.job_owns_cleanup();
        let restricted_job = duplicate_with_access(job.raw(), JOB_QUERY_ACCESS);
        drop(job);
        let mut execution = JobExecution::new(restricted_job, child);
        execution.resume().unwrap();

        let error = execution.terminate_and_reap().unwrap_err();
        assert_access_denied(error);
        drop(execution);
    }

    fn process_termination_failure_scenario() {
        let safety_job = create_kill_on_close_job().unwrap();
        let info = create_suspended_ping_info();
        // SAFETY: successful CreateProcessW returned valid owned handles.
        let full_process = unsafe { OwnedHandle::from_valid(info.hProcess) };
        // SAFETY: same contract independently covers the primary thread.
        let primary_thread = unsafe { OwnedHandle::from_valid(info.hThread) };
        // SAFETY: both handles are valid and the process is suspended.
        assert_ne!(
            unsafe { AssignProcessToJobObject(safety_job.raw(), full_process.raw()) },
            0
        );
        let restricted_process = duplicate_with_access(full_process.raw(), PROCESS_SYNCHRONIZE);
        let child = CreatedProcess {
            process: restricted_process,
            thread: Some(primary_thread),
            terminate_process_on_drop: true,
        };

        drop(child);

        // SAFETY: full_process retains PROCESS_TERMINATE and synchronization
        // rights solely as a test cleanup backstop.
        assert_ne!(unsafe { TerminateProcess(full_process.raw(), 1) }, 0);
        assert!(wait_for_handle_bounded(full_process.raw(), CLEANUP_TIMEOUT).unwrap());
        drop(safety_job);
    }

    fn create_suspended_ping() -> CreatedProcess {
        let info = create_suspended_ping_info();
        // SAFETY: the helper returns PROCESS_INFORMATION only after a
        // successful CreateProcessW call and transfers both handles.
        unsafe { CreatedProcess::from_process_info(info) }
    }

    fn create_suspended_ping_info() -> PROCESS_INFORMATION {
        let spec = CommandSpec::new("ping", ["-n", "30", "127.0.0.1"]);
        let application_name = resolve_application_name(spec.program.as_os_str()).unwrap();
        let mut command_line = build_command_line(&spec).unwrap();
        let startup = STARTUPINFOW {
            cb: size_u32::<STARTUPINFOW>().unwrap(),
            ..Default::default()
        };
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: all buffers and structures meet CreateProcessW's contracts.
        let created = unsafe {
            CreateProcessW(
                application_name.as_ptr(),
                command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_SUSPENDED,
                ptr::null(),
                ptr::null(),
                &startup,
                &mut info,
            )
        };
        assert_ne!(created, 0, "{}", io::Error::last_os_error());
        info
    }

    fn duplicate_with_access(source: HANDLE, access: u32) -> OwnedHandle {
        let mut duplicate = ptr::null_mut();
        // SAFETY: source is a live handle in the current process and duplicate
        // receives ownership of the requested restricted handle.
        let duplicated = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                source,
                GetCurrentProcess(),
                &mut duplicate,
                access,
                0,
                0,
            )
        };
        assert_ne!(duplicated, 0, "{}", io::Error::last_os_error());
        OwnedHandle::new(duplicate).expect("DuplicateHandle returned null")
    }

    fn assert_access_denied(error: BenchguardError) {
        match error {
            BenchguardError::Measurement { source, .. } => {
                assert_eq!(source.raw_os_error(), Some(ERROR_ACCESS_DENIED as i32))
            }
            other => panic!("expected measurement error, got {other:?}"),
        }
    }
}
