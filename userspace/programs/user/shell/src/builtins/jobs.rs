//! Job Control Builtin Commands
//!
//! Commands: jobs, bg, fg, disown, suspend, kill, wait

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::ShellState;
use std::process;

/// Register job control builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register("jobs", BuiltinDesc::new(
        builtin_jobs,
        "显示当前作业",
        "显示当前作业的状态。\n\
         \n\
         选项:\n\
           -l    除正常信息外还列出进程 ID\n\
           -n    只列出自上次通知以来状态发生变化的进程\n\
           -p    只列出进程 ID\n\
           -r    只列出正在运行的作业\n\
           -s    只列出已停止的作业\n\
         \n\
         如果给出了 JOBSPEC，则只输出该作业的信息。\n\
         如果没有选项，则显示所有活动作业的状态。\n\
         \n\
         如果给出了 JOBSPEC 且它存在则返回成功，否则返回失败。",
        "jobs [-lnprs] [任务说明符 ...] 或 jobs -x 命令 [参数]",
        true,
    ));

    registry.register("bg", BuiltinDesc::new(
        builtin_bg,
        "将作业移至后台",
        "将每个 JOBSPEC 移至后台，就像以 `&' 启动它一样。\n\
         如果 JOB_SPEC 没有提供，则使用 shell 认为的当前作业。\n\
         \n\
         如果成功启用了作业控制且 JOB_SPEC 存在则返回成功，\n\
         否则返回非零值。",
        "bg [任务说明符 ...]",
        true,
    ));

    registry.register("fg", BuiltinDesc::new(
        builtin_fg,
        "将作业移至前台",
        "将 JOB_SPEC 移至前台，使它成为当前作业。\n\
         如果 JOB_SPEC 没有提供，则使用 shell 认为的当前作业。\n\
         \n\
         如果成功启用了作业控制且 JOB_SPEC 存在则返回前台命令的状态，\n\
         否则返回非零值。",
        "fg [任务说明符]",
        true,
    ));

    registry.register("disown", BuiltinDesc::new(
        builtin_disown,
        "从作业表中移除作业",
        "从活动作业表中移除每个 JOBSPEC 参数。\n\
         \n\
         选项:\n\
           -a    如果未给出 JOBSPEC，则移除所有作业\n\
           -h    标记每个 JOBSPEC，使其在 shell 接收到 SIGHUP 时\n\
                 不会向作业发送 SIGHUP\n\
           -r    移除正在运行的作业\n\
         \n\
         如果既没有给出选项也没有给出 JOBSPEC，则使用当前作业。\n\
         \n\
         如果所有 JOBSPEC 都被移除则返回成功，否则返回失败。",
        "disown [-h] [-ar] [任务说明符 ... | pid ...]",
        true,
    ));

    registry.register("suspend", BuiltinDesc::new(
        builtin_suspend,
        "挂起 shell 的执行",
        "挂起执行此 shell，直到它收到 SIGCONT 信号。\n\
         登录 shell 或以 -f 选项使用的 shell 无法被挂起。\n\
         \n\
         选项:\n\
           -f    即使是登录 shell 也强制挂起\n\
         \n\
         如果不是登录 shell 则返回成功，否则返回失败。",
        "suspend [-f]",
        true,
    ));

    registry.register("kill", BuiltinDesc::new(
        builtin_kill,
        "向作业发送信号",
        "向由 PID 或 JOBSPEC 标识的进程发送 SIGSPEC 或 SIGNUM 指定的信号。\n\
         \n\
         选项:\n\
           -s sig    SIG 是信号名称\n\
           -n sig    SIG 是信号编号\n\
           -l        列出信号名称；如果跟着参数，则列出该信号对应的名称\n\
           -L        同 -l\n\
         \n\
         如果没有给出 SIGSPEC，则默认为 SIGTERM。\n\
         \n\
         如果成功发送了信号则返回成功，否则返回失败。",
        "kill [-s 信号说明符 | -n 信号编号 | -信号说明符] pid | 任务说明符 ...",
        false,  // Cannot be disabled (important for signal handling)
    ));

    registry.register("wait", BuiltinDesc::new(
        builtin_wait,
        "等待作业完成",
        "等待由 ID 标识的每个进程（可以是进程 ID 或作业说明符），\n\
         并报告其终止状态。\n\
         \n\
         选项:\n\
           -f    等待 ID 终止，而非在它改变状态时返回\n\
           -n    等待任何单个进程，并返回其退出状态\n\
           -p VAR  将触发返回的进程 ID 存入 VAR\n\
         \n\
         如果没有给出 ID，则等待所有当前活动的子进程，返回状态为零。\n\
         如果 ID 是作业说明符，则等待该作业中所有进程。\n\
         \n\
         如果每个 ID 都是有效进程或作业则返回 ID 的退出状态，\n\
         否则返回失败。",
        "wait [-fn] [-p 变量] [id ...]",
        true,
    ));
}

/// jobs builtin - display job status
fn builtin_jobs(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut show_pids = false;
    let mut long_format = false;
    let mut running_only = false;
    let mut stopped_only = false;

    for arg in args {
        match *arg {
            "-l" => long_format = true,
            "-p" => show_pids = true,
            "-n" => {} // Changed since last notification - not implemented
            "-r" => running_only = true,
            "-s" => stopped_only = true,
            arg if arg.starts_with('-') => {
                return Err(format!("jobs: {}: 无效选项", arg));
            }
            _ => {} // Job spec - not implemented
        }
    }

    // Get jobs from state
    let jobs = state.list_jobs();
    
    if jobs.is_empty() {
        // No jobs - silently succeed
        return Ok(0);
    }

    for job in jobs {
        // Filter by state if requested
        if running_only && job.status != "Running" {
            continue;
        }
        if stopped_only && job.status != "Stopped" {
            continue;
        }

        if show_pids {
            println!("{}", job.pid);
        } else if long_format {
            println!("[{}]{}\t{}\t{}\t{}", 
                job.job_id,
                if job.is_current { "+" } else if job.is_previous { "-" } else { " " },
                job.pid,
                job.status,
                job.command
            );
        } else {
            println!("[{}]{}\t{}\t{}", 
                job.job_id,
                if job.is_current { "+" } else if job.is_previous { "-" } else { " " },
                job.status,
                job.command
            );
        }
    }

    Ok(0)
}

/// bg builtin - move job to background
fn builtin_bg(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let job_spec = args.first().copied();
    
    match state.resume_job(job_spec, false) {
        Ok(job_id) => {
            println!("[{}] 继续", job_id);
            Ok(0)
        }
        Err(e) => Err(format!("bg: {}", e)),
    }
}

/// fg builtin - move job to foreground
fn builtin_fg(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let job_spec = args.first().copied();
    
    match state.resume_job(job_spec, true) {
        Ok(job_id) => {
            if let Some(job) = state.get_job(job_id) {
                println!("{}", job.command);
            }
            // Wait for job to complete (in foreground)
            if let Some(status) = state.wait_for_job(job_id) {
                Ok(status)
            } else {
                Ok(0)
            }
        }
        Err(e) => Err(format!("fg: {}", e)),
    }
}

/// disown builtin - remove job from table
fn builtin_disown(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut remove_all = false;
    let mut no_hup = false;
    let mut running_only = false;
    let mut job_specs: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-a" => remove_all = true,
            "-h" => no_hup = true,
            "-r" => running_only = true,
            arg if arg.starts_with('-') => {
                return Err(format!("disown: {}: 无效选项", arg));
            }
            _ => job_specs.push(*arg),
        }
    }

    if remove_all {
        state.disown_all_jobs(no_hup, running_only);
        return Ok(0);
    }

    if job_specs.is_empty() {
        // Disown current job
        match state.disown_current_job(no_hup) {
            Ok(_) => Ok(0),
            Err(e) => Err(format!("disown: {}", e)),
        }
    } else {
        let mut success = true;
        for spec in job_specs {
            if let Err(e) = state.disown_job(spec, no_hup) {
                eprintln!("disown: {}: {}", spec, e);
                success = false;
            }
        }
        if success { Ok(0) } else { Ok(1) }
    }
}

/// suspend builtin - suspend shell execution
fn builtin_suspend(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut force = false;

    for arg in args {
        match *arg {
            "-f" => force = true,
            arg if arg.starts_with('-') => {
                return Err(format!("suspend: {}: 无效选项", arg));
            }
            _ => return Err(format!("suspend: {}: 无效参数", arg)),
        }
    }

    // Check if login shell
    if state.is_login_shell() && !force {
        return Err("suspend: 无法挂起登录 shell".to_string());
    }

    // Send SIGSTOP to self
    #[cfg(target_family = "unix")]
    unsafe {
        libc::kill(libc::getpid(), libc::SIGSTOP);
    }

    Ok(0)
}

/// kill builtin - send signal to process
fn builtin_kill(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        return Err("kill: 用法: kill [-s 信号说明符 | -n 信号编号 | -信号说明符] pid | 任务说明符 ...".to_string());
    }

    let mut list_signals = false;
    let mut signal = 15i32; // SIGTERM
    let mut targets: Vec<&str> = Vec::new();

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match *arg {
            "-l" | "-L" => {
                list_signals = true;
            }
            "-s" => {
                if let Some(sig_name) = iter.next() {
                    signal = parse_signal_name(sig_name)?;
                } else {
                    return Err("kill: -s: 需要选项参数".to_string());
                }
            }
            "-n" => {
                if let Some(sig_num) = iter.next() {
                    signal = sig_num.parse().map_err(|_| format!("kill: {}: 无效信号说明符", sig_num))?;
                } else {
                    return Err("kill: -n: 需要选项参数".to_string());
                }
            }
            arg if arg.starts_with('-') && arg.len() > 1 => {
                // Could be -SIG or -NUM
                let sig = &arg[1..];
                if let Ok(num) = sig.parse::<i32>() {
                    signal = num;
                } else {
                    signal = parse_signal_name(sig)?;
                }
            }
            _ => targets.push(*arg),
        }
    }

    if list_signals {
        print_signal_list();
        return Ok(0);
    }

    if targets.is_empty() {
        return Err("kill: 用法: kill [-s 信号说明符 | -n 信号编号 | -信号说明符] pid | 任务说明符 ...".to_string());
    }

    let mut success = true;
    for target in targets {
        let pid = if target.starts_with('%') {
            // Job spec
            match state.get_job_pid(target) {
                Some(p) => p,
                None => {
                    eprintln!("kill: {}: 没有这个作业", target);
                    success = false;
                    continue;
                }
            }
        } else {
            // PID
            match target.parse::<i32>() {
                Ok(p) => p,
                Err(_) => {
                    eprintln!("kill: {}: 参数必须是进程或作业 ID", target);
                    success = false;
                    continue;
                }
            }
        };

        #[cfg(target_family = "unix")]
        {
            if unsafe { libc::kill(pid, signal) } != 0 {
                let err = std::io::Error::last_os_error();
                eprintln!("kill: ({}) - {}", pid, err);
                success = false;
            }
        }
        #[cfg(not(target_family = "unix"))]
        {
            eprintln!("kill: 此平台不支持");
            success = false;
        }
    }

    if success { Ok(0) } else { Ok(1) }
}

/// wait builtin - wait for job completion
fn builtin_wait(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut wait_any = false;
    let mut var_name: Option<&str> = None;
    let mut ids: Vec<&str> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "-f" => {} // Wait for termination - default behavior
            "-n" => wait_any = true,
            "-p" => {
                if let Some(name) = iter.next() {
                    var_name = Some(*name);
                } else {
                    return Err("wait: -p: 需要选项参数".to_string());
                }
            }
            arg if arg.starts_with('-') => {
                return Err(format!("wait: {}: 无效选项", arg));
            }
            _ => ids.push(*arg),
        }
    }

    if ids.is_empty() {
        // Wait for all children
        let status = state.wait_all_jobs();
        return Ok(status);
    }

    if wait_any {
        // Wait for any single process
        let (pid, status) = state.wait_any_job();
        if let Some(name) = var_name {
            state.set_var(name, pid.to_string());
        }
        return Ok(status);
    }

    // Wait for specific jobs/pids
    let mut last_status = 0;
    for id in ids {
        if id.starts_with('%') {
            // Job spec
            if let Some(job_id) = state.parse_job_spec(id) {
                if let Some(status) = state.wait_for_job(job_id) {
                    last_status = status;
                    if let Some(name) = var_name {
                        if let Some(pid) = state.get_job_pid(id) {
                            state.set_var(name, pid.to_string());
                        }
                    }
                } else {
                    eprintln!("wait: {}: 没有这个作业", id);
                    last_status = 127;
                }
            } else {
                eprintln!("wait: {}: 没有这个作业", id);
                last_status = 127;
            }
        } else {
            // PID
            if let Ok(pid) = id.parse::<i32>() {
                let status = state.wait_for_pid(pid);
                last_status = status;
                if let Some(name) = var_name {
                    state.set_var(name, pid.to_string());
                }
            } else {
                eprintln!("wait: {}: 不是有效的进程 ID", id);
                last_status = 127;
            }
        }
    }

    Ok(last_status)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse signal name to number
fn parse_signal_name(name: &str) -> Result<i32, String> {
    let name_upper = name.to_uppercase();
    let name = name_upper.strip_prefix("SIG").unwrap_or(&name_upper);
    
    match name {
        "HUP" => Ok(1),
        "INT" => Ok(2),
        "QUIT" => Ok(3),
        "ILL" => Ok(4),
        "TRAP" => Ok(5),
        "ABRT" | "IOT" => Ok(6),
        "BUS" => Ok(7),
        "FPE" => Ok(8),
        "KILL" => Ok(9),
        "USR1" => Ok(10),
        "SEGV" => Ok(11),
        "USR2" => Ok(12),
        "PIPE" => Ok(13),
        "ALRM" => Ok(14),
        "TERM" => Ok(15),
        "STKFLT" => Ok(16),
        "CHLD" | "CLD" => Ok(17),
        "CONT" => Ok(18),
        "STOP" => Ok(19),
        "TSTP" => Ok(20),
        "TTIN" => Ok(21),
        "TTOU" => Ok(22),
        "URG" => Ok(23),
        "XCPU" => Ok(24),
        "XFSZ" => Ok(25),
        "VTALRM" => Ok(26),
        "PROF" => Ok(27),
        "WINCH" => Ok(28),
        "IO" | "POLL" => Ok(29),
        "PWR" => Ok(30),
        "SYS" => Ok(31),
        _ => Err(format!("kill: {}: 无效信号说明符", name)),
    }
}

/// Print list of signals
fn print_signal_list() {
    const SIGNALS: &[(&str, i32)] = &[
        ("HUP", 1), ("INT", 2), ("QUIT", 3), ("ILL", 4),
        ("TRAP", 5), ("ABRT", 6), ("BUS", 7), ("FPE", 8),
        ("KILL", 9), ("USR1", 10), ("SEGV", 11), ("USR2", 12),
        ("PIPE", 13), ("ALRM", 14), ("TERM", 15), ("STKFLT", 16),
        ("CHLD", 17), ("CONT", 18), ("STOP", 19), ("TSTP", 20),
        ("TTIN", 21), ("TTOU", 22), ("URG", 23), ("XCPU", 24),
        ("XFSZ", 25), ("VTALRM", 26), ("PROF", 27), ("WINCH", 28),
        ("IO", 29), ("PWR", 30), ("SYS", 31),
    ];

    for (i, (name, num)) in SIGNALS.iter().enumerate() {
        print!("{:2}) SIG{:<8}", num, name);
        if (i + 1) % 4 == 0 {
            println!();
        }
    }
    if SIGNALS.len() % 4 != 0 {
        println!();
    }
}
