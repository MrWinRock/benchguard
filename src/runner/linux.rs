use std::{collections::HashMap, time::Duration};

use crate::error::BenchguardError;

#[cfg(target_os = "linux")]
use std::{
    fs, io,
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::Instant,
};

#[cfg(target_os = "linux")]
use super::CommandSpec;
#[cfg(target_os = "linux")]
use crate::domain::Sample;

#[cfg(target_os = "linux")]
const POLL_INTERVAL: Duration = Duration::from_millis(5);
const TERMINATION_GRACE: Duration = Duration::from_millis(100);
#[cfg(target_os = "linux")]
const GROUP_CLEANUP_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcStat {
    pid: i32,
    state: char,
    parent_pid: i32,
    process_group: i32,
    user_ticks: u64,
    system_ticks: u64,
    start_ticks: u64,
    rss_pages: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ProcessIdentity {
    pid: i32,
    start_ticks: u64,
}

#[derive(Debug, Default)]
struct SampledTreeMetrics {
    cpu_ticks_by_process: HashMap<ProcessIdentity, u64>,
    peak_rss_pages: u64,
}

impl SampledTreeMetrics {
    fn observe(
        &mut self,
        process_group: i32,
        processes: &[ProcStat],
    ) -> Result<(), BenchguardError> {
        let mut current_rss_pages = 0_u64;

        for process in processes
            .iter()
            .filter(|process| process.process_group == process_group)
        {
            let cpu_ticks = process
                .user_ticks
                .checked_add(process.system_ticks)
                .ok_or(BenchguardError::NumericOverflow)?;
            let identity = ProcessIdentity {
                pid: process.pid,
                start_ticks: process.start_ticks,
            };
            self.cpu_ticks_by_process
                .entry(identity)
                .and_modify(|retained| *retained = (*retained).max(cpu_ticks))
                .or_insert(cpu_ticks);
            current_rss_pages = current_rss_pages
                .checked_add(process.rss_pages)
                .ok_or(BenchguardError::NumericOverflow)?;
        }

        self.peak_rss_pages = self.peak_rss_pages.max(current_rss_pages);
        Ok(())
    }

    fn total_cpu_ticks(&self) -> Result<u64, BenchguardError> {
        self.cpu_ticks_by_process
            .values()
            .try_fold(0_u64, |total, ticks| {
                total
                    .checked_add(*ticks)
                    .ok_or(BenchguardError::NumericOverflow)
            })
    }

    fn peak_rss_pages(&self) -> u64 {
        self.peak_rss_pages
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeoutAction {
    KeepLeaderWaitable,
    ReapAndComplete,
    SendTerm,
    SendKillAndReap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeoutPhase {
    Running,
    TermSent { at: Duration },
    KillSent,
}

#[derive(Debug, Clone, Copy)]
struct TimeoutController {
    deadline: Option<Duration>,
    phase: TimeoutPhase,
}

impl TimeoutController {
    fn new(deadline: Option<Duration>) -> Self {
        Self {
            deadline,
            phase: TimeoutPhase::Running,
        }
    }

    fn next_action(&mut self, elapsed: Duration, leader_exited: bool) -> TimeoutAction {
        match self.phase {
            TimeoutPhase::Running => {
                if leader_exited {
                    TimeoutAction::ReapAndComplete
                } else if self.deadline.is_some_and(|deadline| elapsed >= deadline) {
                    self.phase = TimeoutPhase::TermSent { at: elapsed };
                    TimeoutAction::SendTerm
                } else {
                    TimeoutAction::KeepLeaderWaitable
                }
            }
            TimeoutPhase::TermSent { at } => {
                if elapsed.saturating_sub(at) >= TERMINATION_GRACE {
                    self.phase = TimeoutPhase::KillSent;
                    TimeoutAction::SendKillAndReap
                } else {
                    TimeoutAction::KeepLeaderWaitable
                }
            }
            TimeoutPhase::KillSent => TimeoutAction::ReapAndComplete,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcStatParseError;

fn parse_proc_stat(input: &str) -> Result<ProcStat, ProcStatParseError> {
    let open = input.find('(').ok_or(ProcStatParseError)?;
    let close = input.rfind(')').ok_or(ProcStatParseError)?;
    if close <= open {
        return Err(ProcStatParseError);
    }

    let pid = parse_field(input[..open].trim())?;
    let fields = input[close + 1..].split_whitespace().collect::<Vec<_>>();
    if fields.len() <= 21 {
        return Err(ProcStatParseError);
    }

    Ok(ProcStat {
        pid,
        state: fields[0].chars().next().ok_or(ProcStatParseError)?,
        parent_pid: parse_field(fields[1])?,
        process_group: parse_field(fields[2])?,
        user_ticks: parse_field(fields[11])?,
        system_ticks: parse_field(fields[12])?,
        start_ticks: parse_field(fields[19])?,
        rss_pages: parse_field(fields[21])?,
    })
}

fn parse_field<T: std::str::FromStr>(field: &str) -> Result<T, ProcStatParseError> {
    field.parse().map_err(|_| ProcStatParseError)
}

fn ticks_to_nanoseconds(ticks: u64, ticks_per_second: u64) -> Result<u64, BenchguardError> {
    if ticks_per_second == 0 {
        return Err(BenchguardError::NumericOverflow);
    }
    let nanoseconds = u128::from(ticks)
        .checked_mul(1_000_000_000)
        .ok_or(BenchguardError::NumericOverflow)?
        / u128::from(ticks_per_second);
    u64::try_from(nanoseconds).map_err(|_| BenchguardError::NumericOverflow)
}

fn pages_to_bytes(pages: u64, page_size: u64) -> Result<u64, BenchguardError> {
    pages
        .checked_mul(page_size)
        .ok_or(BenchguardError::NumericOverflow)
}

fn has_live_group_descendant(processes: &[ProcStat], pgid: i32) -> bool {
    processes
        .iter()
        .any(|process| process.process_group == pgid && process.pid != pgid && process.state != 'Z')
}

#[derive(Debug, Default)]
struct GroupCleanupOwnership {
    leader_reaped: bool,
    released: bool,
}

impl GroupCleanupOwnership {
    fn mark_leader_reaped(&mut self) {
        self.leader_reaped = true;
    }

    fn release(&mut self) {
        self.released = true;
    }

    fn needs_group_kill(&self) -> bool {
        !self.released
    }

    fn needs_leader_reap(&self) -> bool {
        !self.released && !self.leader_reaped
    }
}

/// Runs one command in a separate Linux session and process group.
///
/// Linux CPU and peak resident memory are sampled process-group aggregates.
/// Every 5 ms, the sampler retains the greatest cumulative user+system tick
/// count observed for each process identity and the greatest aggregate current
/// RSS. Very short-lived processes between samples and descendants that
/// deliberately leave the group may be missed. This 5 ms interval and
/// process-group scope are fixed v0.1 behavior.
#[cfg(target_os = "linux")]
pub(super) fn platform_run_once(
    spec: &CommandSpec,
    timeout: Option<Duration>,
) -> Result<Sample, BenchguardError> {
    let ticks_per_second = sysconf_value(libc::_SC_CLK_TCK, "read clock tick frequency")?;
    let page_size = sysconf_value(libc::_SC_PAGESIZE, "read memory page size")?;
    let started = Instant::now();
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    // SAFETY: this closure runs in the forked child before exec and calls only
    // the async-signal-safe setsid(2), returning the OS error without touching
    // parent-owned state.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }

    let child = command
        .spawn()
        .map_err(|source| BenchguardError::CommandLaunch { source })?;
    let mut group = ChildProcessGroup::new(child)?;
    let mut metrics = SampledTreeMetrics::default();
    let mut timeout = TimeoutController::new(timeout);

    loop {
        metrics.observe(group.pgid, &read_proc_stats()?)?;
        let leader_exited = group.exit_observed()?;

        match timeout.next_action(started.elapsed(), leader_exited) {
            TimeoutAction::KeepLeaderWaitable => thread::sleep(POLL_INTERVAL),
            TimeoutAction::ReapAndComplete => {
                let wall_ns = u64::try_from(started.elapsed().as_nanos())
                    .map_err(|_| BenchguardError::NumericOverflow)?;
                let cpu_ns = ticks_to_nanoseconds(metrics.total_cpu_ticks()?, ticks_per_second)?;
                let peak_memory_bytes = pages_to_bytes(metrics.peak_rss_pages(), page_size)?;

                // All fallible metric conversions happen while the exited
                // leader remains waitable, pinning its PID/PGID. If one fails,
                // ChildProcessGroup::drop still owns group termination.
                let status = group.terminate_remaining_and_reap()?;
                let exit_code = status.code().unwrap_or(-1);
                if !status.success() {
                    group.release();
                    return Err(BenchguardError::CommandFailed { exit_code });
                }
                let sample = Sample {
                    wall_ns,
                    cpu_ns,
                    peak_memory_bytes,
                    exit_code,
                };
                group.release();
                return Ok(sample);
            }
            TimeoutAction::SendTerm => group.signal(libc::SIGTERM)?,
            TimeoutAction::SendKillAndReap => {
                group.kill_and_reap()?;
                return Err(BenchguardError::Timeout);
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn sysconf_value(name: libc::c_int, operation: &'static str) -> Result<u64, BenchguardError> {
    // SAFETY: sysconf reads process-wide configuration and has no pointer
    // arguments or ownership requirements.
    let value = unsafe { libc::sysconf(name) };
    u64::try_from(value).map_err(|_| measurement_error(operation, io::Error::last_os_error()))
}

#[cfg(target_os = "linux")]
fn read_proc_stats() -> Result<Vec<ProcStat>, BenchguardError> {
    let entries =
        fs::read_dir("/proc").map_err(|source| measurement_error("read /proc", source))?;
    Ok(entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .bytes()
                .all(|byte| byte.is_ascii_digit())
        })
        .filter_map(|entry| fs::read_to_string(entry.path().join("stat")).ok())
        .filter_map(|stat| parse_proc_stat(&stat).ok())
        .collect())
}

#[cfg(target_os = "linux")]
struct ChildProcessGroup {
    child: Child,
    pgid: i32,
    status: Option<ExitStatus>,
    ownership: GroupCleanupOwnership,
}

#[cfg(target_os = "linux")]
impl ChildProcessGroup {
    fn new(mut child: Child) -> Result<Self, BenchguardError> {
        let pgid = match i32::try_from(child.id()) {
            Ok(pgid) => pgid,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(BenchguardError::NumericOverflow);
            }
        };
        Ok(Self {
            child,
            pgid,
            status: None,
            ownership: GroupCleanupOwnership::default(),
        })
    }

    fn exit_observed(&self) -> Result<bool, BenchguardError> {
        loop {
            // SAFETY: zero is the required initial state for WNOHANG to report
            // no child by leaving si_pid at zero.
            let mut siginfo: libc::siginfo_t = unsafe { std::mem::zeroed() };
            // SAFETY: the leader is this process's owned, unreaped child.
            // WNOWAIT observes exit without consuming its wait status, keeping
            // the PID/PGID unavailable for reuse until the final group signal.
            let result = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id(),
                    &mut siginfo,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if result == 0 {
                // SAFETY: successful waitid initialized the siginfo child
                // fields; si_pid is zero when WNOHANG observed no exit.
                return Ok(unsafe { siginfo.si_pid() } != 0);
            }
            let source = io::Error::last_os_error();
            if source.kind() != io::ErrorKind::Interrupted {
                return Err(measurement_error("observe benchmark process exit", source));
            }
        }
    }

    fn signal(&self, signal: libc::c_int) -> Result<(), BenchguardError> {
        // SAFETY: pgid is the positive child PID from the isolated setsid
        // session; its negation addresses only that process group.
        let result = unsafe { libc::kill(-self.pgid, signal) };
        if result == 0 {
            return Ok(());
        }
        let source = io::Error::last_os_error();
        if source.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(measurement_error("signal benchmark process group", source))
        }
    }

    fn reap(&mut self) -> Result<ExitStatus, BenchguardError> {
        if let Some(status) = self.status {
            return Ok(status);
        }
        let status = self
            .child
            .wait()
            .map_err(|source| measurement_error("reap benchmark process", source))?;
        self.status = Some(status);
        self.ownership.mark_leader_reaped();
        Ok(status)
    }

    fn kill_and_reap(&mut self) -> Result<(), BenchguardError> {
        self.signal(libc::SIGKILL)?;
        self.wait_for_no_live_descendants()?;
        self.reap()?;
        self.ownership.release();
        Ok(())
    }

    fn terminate_remaining_and_reap(&mut self) -> Result<ExitStatus, BenchguardError> {
        self.signal(libc::SIGKILL)?;
        self.wait_for_no_live_descendants()?;
        self.reap()
    }

    fn wait_for_no_live_descendants(&self) -> Result<(), BenchguardError> {
        let started = Instant::now();
        loop {
            let processes = read_proc_stats()?;
            let live_descendant_exists = has_live_group_descendant(&processes, self.pgid);
            if !live_descendant_exists {
                return Ok(());
            }
            if started.elapsed() >= GROUP_CLEANUP_TIMEOUT {
                return Err(measurement_error(
                    "confirm benchmark process-group descendant cleanup",
                    io::Error::new(
                        io::ErrorKind::TimedOut,
                        "process group still had live descendants after SIGKILL",
                    ),
                ));
            }
            thread::sleep(POLL_INTERVAL);
        }
    }

    fn release(&mut self) {
        self.ownership.release();
    }
}

#[cfg(target_os = "linux")]
impl Drop for ChildProcessGroup {
    fn drop(&mut self) {
        if self.ownership.needs_group_kill() {
            let _ = self.signal(libc::SIGKILL);
        }
        if self.ownership.needs_leader_reap() {
            let _ = self.child.wait();
        }
    }
}

#[cfg(target_os = "linux")]
fn measurement_error(operation: &'static str, source: io::Error) -> BenchguardError {
    BenchguardError::Measurement { operation, source }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        TimeoutAction, TimeoutController, has_live_group_descendant, pages_to_bytes,
        parse_proc_stat, ticks_to_nanoseconds,
    };

    fn parse(line: &str) -> super::ProcStat {
        parse_proc_stat(line).expect("test stat line should parse")
    }

    // Catches parsing fields relative to the first ')' in comm instead of the
    // final delimiter, and off-by-one indices for the Linux stat fields.
    #[test]
    fn parses_proc_stat_with_spaces_and_parentheses_in_command_name() {
        let stat = parse("123 (worker ) name) S 1 123 123 0 0 0 0 0 0 0 7 3 0 0 0 0 1 0 456 0 8");

        assert_eq!(stat.pid, 123);
        assert_eq!(stat.state, 'S');
        assert_eq!(stat.parent_pid, 1);
        assert_eq!(stat.process_group, 123);
        assert_eq!(stat.user_ticks, 7);
        assert_eq!(stat.system_ticks, 3);
        assert_eq!(stat.start_ticks, 456);
        assert_eq!(stat.rss_pages, 8);
    }

    // Catches considering a killed zombie descendant live forever, or
    // declaring cleanup complete while a runnable group descendant remains.
    #[test]
    fn group_cleanup_waits_for_live_but_not_zombie_descendants() {
        let leader = parse("123 (leader) Z 1 123 123 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0 1 0 0");
        let live_child = parse("124 (child) S 1 123 123 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0 1 0 0");
        let zombie_child = parse("124 (child) Z 1 123 123 0 0 0 0 0 0 0 0 0 0 0 0 0 1 0 1 0 0");

        assert!(has_live_group_descendant(
            &[leader.clone(), live_child],
            123
        ));
        assert!(!has_live_group_descendant(&[leader, zombie_child], 123));
    }

    // Catches summing cumulative CPU counters at every poll, dropping CPU from
    // an exited descendant, or conflating a reused PID with the old process.
    #[test]
    fn sampled_tree_retains_each_process_maximum_without_poll_overcounting() {
        let mut metrics = super::SampledTreeMetrics::default();
        metrics
            .observe(
                42,
                &[
                    parse("42 (leader) S 1 42 42 0 0 0 0 0 0 0 10 2 0 0 0 0 1 0 100 0 4"),
                    parse("43 (child) S 42 42 42 0 0 0 0 0 0 0 5 1 0 0 0 0 1 0 200 0 6"),
                    parse("99 (unrelated) S 1 99 99 0 0 0 0 0 0 0 90 10 0 0 0 0 1 0 900 0 100"),
                ],
            )
            .unwrap();
        metrics
            .observe(
                42,
                &[
                    parse("42 (leader) S 1 42 42 0 0 0 0 0 0 0 15 3 0 0 0 0 1 0 100 0 2"),
                    parse("43 (reused) S 42 42 42 0 0 0 0 0 0 0 2 1 0 0 0 0 1 0 300 0 7"),
                ],
            )
            .unwrap();

        assert_eq!(metrics.total_cpu_ticks().unwrap(), 27);
        assert_eq!(metrics.peak_rss_pages(), 10);
    }

    // Catches using decimal memory units, truncating before converting clock
    // ticks, accepting an invalid clock frequency, or silently overflowing.
    #[test]
    fn metric_units_convert_to_integer_nanoseconds_and_bytes() {
        assert_eq!(ticks_to_nanoseconds(250, 100).unwrap(), 2_500_000_000);
        assert_eq!(pages_to_bytes(10, 4096).unwrap(), 40_960);
        assert!(ticks_to_nanoseconds(1, 0).is_err());
        assert!(pages_to_bytes(u64::MAX, 4096).is_err());
    }

    // Catches treating leader exit after SIGTERM as cleanup completion while a
    // descendant in the process group may still require SIGKILL.
    #[test]
    fn timeout_waits_full_grace_then_kills_even_if_leader_exited() {
        let mut timeout = TimeoutController::new(Some(Duration::from_millis(50)));

        assert_eq!(
            timeout.next_action(Duration::from_millis(49), false),
            TimeoutAction::KeepLeaderWaitable
        );
        assert_eq!(
            timeout.next_action(Duration::from_millis(50), false),
            TimeoutAction::SendTerm
        );
        assert_eq!(
            timeout.next_action(Duration::from_millis(149), true),
            TimeoutAction::KeepLeaderWaitable
        );
        assert_eq!(
            timeout.next_action(Duration::from_millis(150), true),
            TimeoutAction::SendKillAndReap
        );
    }

    // Catches a disabled timeout preventing ordinary completion.
    #[test]
    fn no_timeout_completes_when_the_leader_exits() {
        let mut timeout = TimeoutController::new(None);

        assert_eq!(
            timeout.next_action(Duration::from_secs(1), true),
            TimeoutAction::ReapAndComplete
        );
    }

    // Catches classifying a command as timed out when the no-reap exit probe
    // already observed its completion on the first poll at the deadline.
    #[test]
    fn observed_completion_wins_over_reaching_the_deadline() {
        let mut timeout = TimeoutController::new(Some(Duration::from_millis(50)));

        assert_eq!(
            timeout.next_action(Duration::from_millis(50), true),
            TimeoutAction::ReapAndComplete
        );
    }

    // Catches coupling process-group ownership to leader reaping: descendants
    // still need a kill if an error interrupts the timeout grace period.
    #[test]
    fn reaping_the_leader_does_not_release_group_cleanup_ownership() {
        let mut ownership = super::GroupCleanupOwnership::default();

        ownership.mark_leader_reaped();
        assert!(ownership.needs_group_kill());
        assert!(!ownership.needs_leader_reap());

        ownership.release();
        assert!(!ownership.needs_group_kill());
        assert!(!ownership.needs_leader_reap());
    }

    // Catches reaping an exited leader while the timeout grace period still
    // owns its numeric PID/PGID. The leader must remain waitable until the
    // final process-group signal decision has been made.
    #[test]
    fn timeout_keeps_exited_leader_waitable_until_final_group_signal() {
        let mut timeout = TimeoutController::new(Some(Duration::from_millis(50)));

        assert_eq!(
            timeout.next_action(Duration::from_millis(50), false),
            TimeoutAction::SendTerm
        );
        assert_eq!(
            timeout.next_action(Duration::from_millis(149), true),
            TimeoutAction::KeepLeaderWaitable
        );
        assert_eq!(
            timeout.next_action(Duration::from_millis(150), true),
            TimeoutAction::SendKillAndReap
        );
    }

    // Catches implementing the Linux exit probe with Child::try_wait(), which
    // would consume the wait status and release the PID before group cleanup.
    #[cfg(target_os = "linux")]
    #[test]
    fn linux_exit_probe_preserves_a_waitable_leader() {
        use std::{io, mem, os::unix::process::CommandExt, process::Command, thread};

        let mut command = Command::new("/bin/true");
        // SAFETY: the child calls only async-signal-safe setsid before exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        let child = command.spawn().unwrap();
        let mut group = super::ChildProcessGroup::new(child).unwrap();
        let observed = (0..100).any(|_| {
            if group.exit_observed().unwrap() {
                true
            } else {
                thread::sleep(Duration::from_millis(5));
                false
            }
        });
        assert!(observed, "leader exit was not observed");

        // SAFETY: siginfo is zero-initialized for waitid, and the child PID is
        // still owned by this process. WNOWAIT must leave it waitable again.
        let mut siginfo: libc::siginfo_t = unsafe { mem::zeroed() };
        // SAFETY: waitid receives a valid child PID and writable siginfo.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                group.child.id(),
                &mut siginfo,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        assert_eq!(result, 0);
        // SAFETY: successful waitid initialized the siginfo child fields.
        assert_eq!(unsafe { siginfo.si_pid() }, group.pgid);

        assert!(group.reap().unwrap().success());
        group.release();
    }
}
