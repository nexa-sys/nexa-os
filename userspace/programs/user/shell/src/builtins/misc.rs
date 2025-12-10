//! Miscellaneous Builtin Commands
//!
//! Commands: times, trap, getopts, caller, mapfile/readarray, let, logout, compgen, complete, compopt, shift, variables, coproc, time

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::ShellState;
use std::io::{self, BufRead, Write};
use std::time::Instant;

/// Register miscellaneous builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register(
        "times",
        BuiltinDesc::new(
            builtin_times,
            "显示 shell 及其子进程的累积用户和系统时间",
            "打印 shell 及其所有子进程累积的用户和系统时间。\n\
         \n\
         返回状态:\n\
         总是成功。",
            "times",
            true,
        ),
    );

    registry.register(
        "trap",
        BuiltinDesc::new(
            builtin_trap,
            "在 shell 收到信号时执行命令",
            "当 shell 收到信号或其他条件时运行 ARG 中的命令。\n\
         \n\
         定义并激活当 shell 收到信号时执行的处理程序。\n\
         ARG 是在 shell 收到信号 SIGNAL_SPEC 时要执行的命令。\n\
         如果 ARG 缺失（且只有一个 SIGNAL_SPEC）或 `-'，\n\
         则每个指定信号被重置为其原始值。\n\
         如果 ARG 是空字符串，则 shell 和由它调用的命令忽略每个\n\
         SIGNAL_SPEC 指定的信号。\n\
         \n\
         选项:\n\
           -l    打印信号名称及其对应编号的列表\n\
           -p    为每个 SIGNAL_SPEC 显示与其关联的 trap 命令\n\
         \n\
         如果 SIGNAL_SPEC 是 EXIT (0)，则在 shell 退出时执行 ARG。\n\
         如果 SIGNAL_SPEC 是 DEBUG，则在每个简单命令之后执行 ARG。\n\
         如果 SIGNAL_SPEC 是 RETURN，则每当执行完以 `.' 或 `source'\n\
         内建命令执行的 shell 函数或脚本时执行 ARG。\n\
         如果 SIGNAL_SPEC 是 ERR，则每当命令以非零退出状态结束时执行 ARG。\n\
         \n\
         如果没有参数，trap 为所有设置了处理程序的信号打印\n\
         与每个信号关联的命令列表。\n\
         \n\
         如果 SIGNAL_SPEC 无效则返回失败；否则 trap 返回成功。",
            "trap [-lp] [[参数] 信号说明符 ...]",
            false, // Cannot be disabled
        ),
    );

    registry.register(
        "getopts",
        BuiltinDesc::new(
            builtin_getopts,
            "解析命令的位置参数",
            "getopts 用于 shell 脚本来解析位置参数。\n\
         \n\
         OPTSTRING 包含要识别的选项字母；如果一个字母后面跟着冒号，\n\
         该选项需要一个参数，应该用空白符与选项字母分开。\n\
         \n\
         每次调用时，getopts 将下一个选项放入 shell 变量 $name，\n\
         如果 name 不存在则初始化它，并将下一个要处理的参数的索引放入\n\
         shell 变量 OPTIND。OPTIND 初始化为 1。当选项需要参数时，\n\
         getopts 将参数放入 shell 变量 OPTARG。\n\
         \n\
         getopts 正常报告错误。如果 OPTSTRING 的第一个字符是冒号，\n\
         则使用静默错误报告。\n\
         \n\
         getopts 如果找到了选项则返回成功；如果遇到选项结尾或发生错误则返回失败。",
            "getopts 选项字符串 名称 [参数 ...]",
            true,
        ),
    );

    registry.register(
        "caller",
        BuiltinDesc::new(
            builtin_caller,
            "返回活动子程序调用的上下文",
            "返回任何活动子程序调用（shell 函数或以 `.' 或 `source'\n\
         内建命令执行的脚本）的上下文。\n\
         \n\
         不带 EXPR，caller 显示当前子程序调用的行号和源文件名。\n\
         如果给出了非负整数作为 EXPR，caller 显示对应调用栈中\n\
         该位置的行号、子程序名和源文件。这些额外信息可用于提供堆栈跟踪。\n\
         \n\
         值 EXPR 表示调用堆栈中在当前调用之前的帧数。\n\
         \n\
         如果 shell 没有在子程序调用中执行或 EXPR 不对应\n\
         调用堆栈中的有效位置则返回失败。",
            "caller [表达式]",
            true,
        ),
    );

    registry.register("mapfile", BuiltinDesc::new(
        builtin_mapfile,
        "从标准输入读取行到索引数组变量",
        "从标准输入读取行到索引数组变量 ARRAY，\n\
         或从文件描述符 FD（如果提供了 -u 选项）。\n\
         \n\
         选项:\n\
           -d DELIM   使用 DELIM 来终止行，而非换行\n\
           -n COUNT   最多复制 COUNT 行。如果 COUNT 是 0，复制所有行\n\
           -O ORIGIN  从索引 ORIGIN 开始将行赋值给 ARRAY。默认索引是 0\n\
           -s COUNT   丢弃读取的前 COUNT 行\n\
           -t         从每行读取中移除尾随的 DELIM（默认换行符）\n\
           -u FD      从文件描述符 FD 读取而非标准输入\n\
           -C CALLBACK 每当读取 QUANTUM 行时，用数组索引调用 CALLBACK\n\
                      和即将赋值的下一行\n\
           -c QUANTUM 指定每次调用 CALLBACK 之间的行数\n\
         \n\
         参数:\n\
           ARRAY      存储结果的数组变量名\n\
                      如果没有提供，默认数组名是 MAPFILE\n\
         \n\
         如果没有提供 -c 使用 5000 作为默认量子。\n\
         当使用 CALLBACK 时，它用 QUANTUM 行的数组索引和\n\
         即将赋值给该索引的行调用。\n\
         \n\
         如果成功则返回 0，如果给出无效选项或 ARRAY 无效或不可赋值则返回非零。",
        "mapfile [-d 分隔符] [-n 计数] [-O 起始] [-s 计数] [-t] [-u fd] [-C 回调] [-c 量子] [数组]",
        true,
    ));

    // readarray is an alias for mapfile
    registry.register("readarray", BuiltinDesc::new(
        builtin_mapfile,
        "从标准输入读取行到索引数组变量",
        "这是 `mapfile' 命令的同义词。参见 `help mapfile'。",
        "readarray [-d 分隔符] [-n 计数] [-O 起始] [-s 计数] [-t] [-u fd] [-C 回调] [-c 量子] [数组]",
        true,
    ));

    registry.register(
        "let",
        BuiltinDesc::new(
            builtin_let,
            "计算算术表达式",
            "计算算术表达式。\n\
         \n\
         EXPRESSION 中的每个 ARG 是要被计算的算术表达式。\n\
         表达式遵循 shell 算术规则。如果最后一个 ARG 计算结果为 0，\n\
         则 let 返回 1；否则返回 0。\n\
         \n\
         等价于:\n\
           (( 表达式 ))\n\
         \n\
         如果最后一个 ARG 计算为非零则返回成功，否则返回失败。",
            "let 参数 [参数 ...]",
            true,
        ),
    );

    registry.register(
        "logout",
        BuiltinDesc::new(
            builtin_logout,
            "退出登录 shell",
            "退出登录 shell。\n\
         \n\
         以状态 N 退出登录 shell。非登录 shell 返回错误。\n\
         \n\
         如果不是登录 shell 则返回失败。",
            "logout [n]",
            true,
        ),
    );

    registry.register("compgen", BuiltinDesc::new(
        builtin_compgen,
        "根据选项显示可能的补全",
        "根据 OPTION 生成匹配 WORD 的可能补全。\n\
         \n\
         选项:\n\
           -a    别名\n\
           -b    shell 内建命令\n\
           -c    所有命令（别名、内建、函数、关键字、外部命令）\n\
           -d    目录\n\
           -e    导出的 shell 变量\n\
           -f    文件\n\
           -j    作业名称\n\
           -k    shell 保留字\n\
           -s    服务名\n\
           -u    用户名\n\
           -v    shell 变量\n\
           -A 动作  使用 ACTION 来生成可能的补全\n\
           -G 模式  用于扩展的文件名\n\
           -W 词列表  用于生成可能补全的词列表\n\
           -P 前缀  每个补全结果前添加的前缀\n\
           -S 后缀  每个补全结果后添加的后缀\n\
           -X 过滤器  应用于补全列表的过滤器模式\n\
         \n\
         返回成功，除非给出无效选项或没有生成任何匹配。",
        "compgen [-abcdefgjksuv] [-o 选项] [-A 动作] [-G 全局模式] [-W 词语列表] [-F 函数] [-C 命令] [-X 过滤模式] [-P 前缀] [-S 后缀] [词语]",
        true,
    ));

    registry.register("complete", BuiltinDesc::new(
        builtin_complete,
        "指定如何补全参数",
        "为每个 NAME 指定如何补全参数。\n\
         \n\
         选项:\n\
           -p        以可重用作输入的格式打印现有补全规范\n\
           -r        为每个 NAME 移除补全规范，或如果没有给出 NAME 则移除所有\n\
           -D        将完成设置应用于\"默认\"命令补全\n\
           -E        将完成设置应用于\"空\"命令补全\n\
           -I        将完成设置应用于初始单词补全\n\
         \n\
         使用时，-D 选项优先于任何对特定 NAME 的补全规范。\n\
         \n\
         其他选项同 compgen。\n\
         \n\
         如果 NAME 没有提供则返回成功，除非给出无效选项。",
        "complete [-abcdefgjksuv] [-pr] [-DEI] [-o 选项] [-A 动作] [-G 全局模式] [-W 词语列表] [-F 函数] [-C 命令] [-X 过滤模式] [-P 前缀] [-S 后缀] [名称 ...]",
        true,
    ));

    registry.register(
        "compopt",
        BuiltinDesc::new(
            builtin_compopt,
            "修改补全选项",
            "修改当前可编程补全的补全选项或对于 NAME 的选项（如果提供了）。\n\
         \n\
         使用 `+o' 而非 `-o' 来关闭指定选项。\n\
         \n\
         选项:\n\
           -o 选项  设置补全选项 OPTION\n\
           -D      改变\"默认\"命令补全的选项\n\
           -E      改变\"空\"命令补全的选项\n\
           -I      改变初始单词补全的选项\n\
         \n\
         有效的补全选项: bashdefault, default, dirnames, filenames,\n\
         noquote, nosort, nospace, plusdirs\n\
         \n\
         如果成功则返回 0，如果给出无效选项或 NAME 没有补全规范则返回非零。",
            "compopt [-o|+o 选项] [-DEI] [名称 ...]",
            true,
        ),
    );

    registry.register(
        "shift",
        BuiltinDesc::new(
            builtin_shift,
            "移动位置参数",
            "将位置参数向左移动。\n\
         \n\
         将位置参数 $N+1, $N+2 ... 重命名为 $1, $2 ...。\n\
         如果没有给出 N，则假定为 1。\n\
         \n\
         退出状态:\n\
         如果 N 是非负数且小于等于 $# 则返回成功。",
            "shift [n]",
            true,
        ),
    );

    registry.register(
        "variables",
        BuiltinDesc::new(
            builtin_variables,
            "显示 shell 变量信息",
            "显示一些 shell 变量的名称和含义。\n\
         \n\
         这是一个帮助命令，显示 shell 使用的特殊变量列表。",
            "variables",
            true,
        ),
    );

    registry.register(
        "coproc",
        BuiltinDesc::new(
            builtin_coproc,
            "创建协进程",
            "创建一个协进程，命名为 NAME。\n\
         \n\
         在后台执行 COMMAND，将其 stdin 和 stdout 连接到数组变量 NAME。\n\
         协进程的 stdin 是 NAME[1]，stdout 是 NAME[0]。\n\
         \n\
         如果没有提供 NAME，默认为 COPROC。\n\
         \n\
         协进程的 PID 存储在 NAME_PID 中。\n\
         \n\
         如果成功启动协进程则返回 0。",
            "coproc [名称] 命令 [重定向]",
            true,
        ),
    );

    registry.register(
        "time",
        BuiltinDesc::new(
            builtin_time,
            "计时命令执行",
            "报告流水线执行消耗的时间。\n\
         \n\
         执行 PIPELINE 并打印实际时间、用户 CPU 时间和系统 CPU 时间\n\
         用于执行 PIPELINE 的概要。\n\
         \n\
         选项:\n\
           -p    以便携的 POSIX 格式打印计时概要\n\
         \n\
         TIMEFORMAT 变量的值被用作输出格式。\n\
         \n\
         返回 PIPELINE 的退出状态。",
            "time [-p] 流水线",
            true,
        ),
    );
}

/// times builtin - display process times
fn builtin_times(_state: &mut ShellState, _args: &[&str]) -> BuiltinResult {
    #[cfg(target_family = "unix")]
    {
        // Get process times using libc
        let mut tms = libc::tms {
            tms_utime: 0,
            tms_stime: 0,
            tms_cutime: 0,
            tms_cstime: 0,
        };

        unsafe {
            libc::times(&mut tms);
        }

        // Get clock ticks per second
        let ticks_per_sec = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;

        // Calculate times in seconds
        let user_time = tms.tms_utime as f64 / ticks_per_sec;
        let sys_time = tms.tms_stime as f64 / ticks_per_sec;
        let child_user = tms.tms_cutime as f64 / ticks_per_sec;
        let child_sys = tms.tms_cstime as f64 / ticks_per_sec;

        // Format as minutes:seconds
        let format_time = |t: f64| {
            let mins = (t / 60.0) as u64;
            let secs = t % 60.0;
            format!("{}m{:.3}s", mins, secs)
        };

        println!("{} {}", format_time(user_time), format_time(sys_time));
        println!("{} {}", format_time(child_user), format_time(child_sys));
    }

    #[cfg(not(target_family = "unix"))]
    {
        println!("0m0.000s 0m0.000s");
        println!("0m0.000s 0m0.000s");
    }

    Ok(0)
}

/// trap builtin - trap signals
fn builtin_trap(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut list_signals = false;
    let mut print_traps = false;
    let mut remaining_args: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-l" => list_signals = true,
            "-p" => print_traps = true,
            _ => remaining_args.push(*arg),
        }
    }

    if list_signals {
        // Print signal list
        println!(" 1) SIGHUP    2) SIGINT    3) SIGQUIT   4) SIGILL    5) SIGTRAP");
        println!(" 6) SIGABRT   7) SIGBUS    8) SIGFPE    9) SIGKILL  10) SIGUSR1");
        println!("11) SIGSEGV  12) SIGUSR2  13) SIGPIPE  14) SIGALRM  15) SIGTERM");
        println!("16) SIGSTKFLT 17) SIGCHLD  18) SIGCONT  19) SIGSTOP  20) SIGTSTP");
        println!("21) SIGTTIN  22) SIGTTOU  23) SIGURG   24) SIGXCPU  25) SIGXFSZ");
        println!("26) SIGVTALRM 27) SIGPROF  28) SIGWINCH 29) SIGIO   30) SIGPWR");
        println!("31) SIGSYS");
        return Ok(0);
    }

    if print_traps || remaining_args.is_empty() {
        // Print current traps
        let traps = state.get_traps();
        for (signal, action) in traps {
            println!("trap -- '{}' {}", action, signal);
        }
        return Ok(0);
    }

    // Set trap
    if remaining_args.len() == 1 {
        // Reset signal to default
        let signal = remaining_args[0];
        state.reset_trap(signal);
    } else {
        // Set trap action
        let action = remaining_args[0];
        for signal in &remaining_args[1..] {
            state.set_trap(signal, action);
        }
    }

    Ok(0)
}

/// getopts builtin - parse positional parameters
fn builtin_getopts(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.len() < 2 {
        return Err("getopts: 用法: getopts 选项字符串 名称 [参数 ...]".to_string());
    }

    let optstring = args[0];
    let name = args[1];
    let params: Vec<&str> = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        // Use positional parameters (not fully implemented)
        Vec::new()
    };

    // Get current OPTIND
    let optind: usize = state
        .get_var("OPTIND")
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    if optind > params.len() {
        // No more options
        state.set_var(name, "?");
        return Ok(1);
    }

    let current = params.get(optind - 1).copied().unwrap_or("");

    if !current.starts_with('-') || current == "-" || current == "--" {
        // Not an option
        state.set_var(name, "?");
        return Ok(1);
    }

    let opt_char = current.chars().nth(1).unwrap_or('?');

    // Check if option is valid
    let silent = optstring.starts_with(':');
    let optstring_clean = if silent { &optstring[1..] } else { optstring };

    if let Some(pos) = optstring_clean.find(opt_char) {
        // Valid option
        state.set_var(name, &opt_char.to_string());

        // Check if it requires an argument
        if optstring_clean.chars().nth(pos + 1) == Some(':') {
            // Needs argument
            let arg = if current.len() > 2 {
                // Argument attached to option
                &current[2..]
            } else if optind < params.len() {
                // Argument is next parameter
                state.set_var("OPTIND", (optind + 2).to_string());
                params[optind]
            } else {
                // Missing argument
                if silent {
                    state.set_var("OPTARG", &opt_char.to_string());
                    state.set_var(name, ":");
                } else {
                    eprintln!("getopts: 选项需要参数 -- {}", opt_char);
                    state.set_var(name, "?");
                }
                return Ok(0);
            };
            state.set_var("OPTARG", arg);
        }

        state.set_var("OPTIND", (optind + 1).to_string());
        Ok(0)
    } else {
        // Invalid option
        if silent {
            state.set_var("OPTARG", &opt_char.to_string());
        } else {
            eprintln!("getopts: 非法选项 -- {}", opt_char);
        }
        state.set_var(name, "?");
        state.set_var("OPTIND", (optind + 1).to_string());
        Ok(0)
    }
}

/// caller builtin - return context of subroutine call
fn builtin_caller(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let level: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(0);

    if let Some((line, name, file)) = state.get_caller_info(level) {
        if args.is_empty() {
            // Without expression: just line and file
            println!("{} {}", line, file);
        } else {
            // With expression: line, name, file
            println!("{} {} {}", line, name, file);
        }
        Ok(0)
    } else {
        Ok(1)
    }
}

/// mapfile/readarray builtin - read lines into array
fn builtin_mapfile(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut delimiter = '\n';
    let mut count: Option<usize> = None;
    let mut origin: usize = 0;
    let mut skip: usize = 0;
    let mut strip_delimiter = false;
    let mut fd: Option<i32> = None;
    let mut callback: Option<&str> = None;
    let mut quantum: usize = 5000;
    let mut array_name = "MAPFILE";

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match *arg {
            "-d" => {
                if let Some(d) = iter.next() {
                    delimiter = d.chars().next().unwrap_or('\n');
                } else {
                    return Err("mapfile: -d: 需要选项参数".to_string());
                }
            }
            "-n" => {
                if let Some(n) = iter.next() {
                    count = Some(n.parse().map_err(|_| format!("mapfile: {}: 无效数字", n))?);
                } else {
                    return Err("mapfile: -n: 需要选项参数".to_string());
                }
            }
            "-O" => {
                if let Some(o) = iter.next() {
                    origin = o.parse().map_err(|_| format!("mapfile: {}: 无效数字", o))?;
                } else {
                    return Err("mapfile: -O: 需要选项参数".to_string());
                }
            }
            "-s" => {
                if let Some(s) = iter.next() {
                    skip = s.parse().map_err(|_| format!("mapfile: {}: 无效数字", s))?;
                } else {
                    return Err("mapfile: -s: 需要选项参数".to_string());
                }
            }
            "-t" => strip_delimiter = true,
            "-u" => {
                if let Some(f) = iter.next() {
                    fd = Some(
                        f.parse()
                            .map_err(|_| format!("mapfile: {}: 无效文件描述符", f))?,
                    );
                } else {
                    return Err("mapfile: -u: 需要选项参数".to_string());
                }
            }
            "-C" => {
                if let Some(c) = iter.next() {
                    callback = Some(*c);
                } else {
                    return Err("mapfile: -C: 需要选项参数".to_string());
                }
            }
            "-c" => {
                if let Some(q) = iter.next() {
                    quantum = q.parse().map_err(|_| format!("mapfile: {}: 无效数字", q))?;
                } else {
                    return Err("mapfile: -c: 需要选项参数".to_string());
                }
            }
            arg if arg.starts_with('-') => {
                return Err(format!("mapfile: {}: 无效选项", arg));
            }
            _ => array_name = *arg,
        }
    }

    // Read from stdin (or specified fd)
    let stdin = io::stdin();
    let mut lines_read = 0;
    let mut skipped = 0;
    let mut idx = origin;

    // Create array in state
    state.clear_array(array_name);

    for line in stdin.lock().lines() {
        let mut line = line.map_err(|e| format!("mapfile: {}", e))?;

        // Skip first N lines
        if skipped < skip {
            skipped += 1;
            continue;
        }

        // Check count limit
        if let Some(max) = count {
            if max > 0 && lines_read >= max {
                break;
            }
        }

        // Strip delimiter if requested
        if strip_delimiter && line.ends_with(delimiter) {
            line.pop();
        }

        // Store in array
        state.set_array_element(array_name, idx, &line);
        idx += 1;
        lines_read += 1;

        // Call callback if specified
        if let Some(_cb) = callback {
            if lines_read % quantum == 0 {
                // Would invoke callback here
            }
        }
    }

    Ok(0)
}

/// let builtin - evaluate arithmetic expression
fn builtin_let(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        return Err("let: 用法: let 表达式 [表达式 ...]".to_string());
    }

    let mut last_result = 0i64;

    for expr in args {
        last_result = evaluate_arithmetic(state, expr)?;
    }

    // let returns 1 if last expression is 0, 0 otherwise
    if last_result == 0 {
        Ok(1)
    } else {
        Ok(0)
    }
}

/// logout builtin - exit login shell
fn builtin_logout(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if !state.is_login_shell() {
        return Err("logout: 非登录 shell: 使用 `exit'".to_string());
    }

    let code = args
        .first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(0);

    std::process::exit(code);
}

/// compgen builtin - generate completions
fn builtin_compgen(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut word = "";
    let mut gen_aliases = false;
    let mut gen_builtins = false;
    let mut gen_commands = false;
    let mut gen_directories = false;
    let mut gen_exports = false;
    let mut gen_files = false;
    let mut gen_jobs = false;
    let mut gen_keywords = false;
    let mut gen_users = false;
    let mut gen_variables = false;
    let mut word_list: Option<&str> = None;
    let mut prefix = "";
    let mut suffix = "";

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match *arg {
            "-a" => gen_aliases = true,
            "-b" => gen_builtins = true,
            "-c" => gen_commands = true,
            "-d" => gen_directories = true,
            "-e" => gen_exports = true,
            "-f" => gen_files = true,
            "-j" => gen_jobs = true,
            "-k" => gen_keywords = true,
            "-u" => gen_users = true,
            "-v" => gen_variables = true,
            "-W" => {
                if let Some(w) = iter.next() {
                    word_list = Some(*w);
                }
            }
            "-P" => {
                if let Some(p) = iter.next() {
                    prefix = *p;
                }
            }
            "-S" => {
                if let Some(s) = iter.next() {
                    suffix = *s;
                }
            }
            "-A" | "-G" | "-F" | "-C" | "-X" | "-o" => {
                // Skip argument for these options
                iter.next();
            }
            arg if arg.starts_with('-') => {
                return Err(format!("compgen: {}: 无效选项", arg));
            }
            _ => word = *arg,
        }
    }

    let mut completions: Vec<String> = Vec::new();

    // Generate completions based on options
    if gen_aliases {
        for (name, _) in state.list_aliases() {
            if name.starts_with(word) {
                completions.push(name.to_string());
            }
        }
    }

    if gen_builtins {
        let builtins = [
            "cd",
            "pwd",
            "echo",
            "exit",
            "export",
            "alias",
            "set",
            "unset",
            "source",
            "type",
            "help",
            "history",
            "jobs",
            "bg",
            "fg",
            "kill",
            "wait",
            "trap",
            "umask",
            "ulimit",
            "shopt",
            "declare",
            "local",
            "readonly",
            "let",
            "test",
            "true",
            "false",
            "break",
            "continue",
            "return",
            "eval",
            "exec",
            "command",
            "builtin",
            "read",
            "printf",
            "pushd",
            "popd",
            "dirs",
            "hash",
            "enable",
            "getopts",
            "caller",
            "mapfile",
            "readarray",
            "compgen",
            "complete",
            "compopt",
            "bind",
            "times",
            "logout",
            "suspend",
            "disown",
            "fc",
        ];
        for name in builtins {
            if name.starts_with(word) {
                completions.push(name.to_string());
            }
        }
    }

    if gen_keywords {
        let keywords = [
            "if", "then", "else", "elif", "fi", "case", "esac", "for", "while", "until", "do",
            "done", "in", "function", "select", "time", "coproc", "{", "}", "[[", "]]", "!", "[[",
        ];
        for kw in keywords {
            if kw.starts_with(word) {
                completions.push(kw.to_string());
            }
        }
    }

    if gen_variables {
        for (name, _) in state.all_vars() {
            if name.starts_with(word) {
                completions.push(name.to_string());
            }
        }
    }

    if gen_exports {
        for (name, _) in state.exported_vars() {
            if name.starts_with(word) {
                completions.push(name.to_string());
            }
        }
    }

    if let Some(words) = word_list {
        for w in words.split_whitespace() {
            if w.starts_with(word) {
                completions.push(w.to_string());
            }
        }
    }

    if gen_files || gen_directories || gen_commands {
        // Would scan filesystem here
        // For now, just note that file completion needs fs access
    }

    // Print completions with prefix/suffix
    for comp in completions {
        println!("{}{}{}", prefix, comp, suffix);
    }

    Ok(0)
}

/// complete builtin - specify completion behavior
fn builtin_complete(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut print_all = false;
    let mut remove = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-p" => print_all = true,
            "-r" => remove = true,
            "-D" | "-E" | "-I" => {} // Default/empty/initial completions
            arg if arg.starts_with('-') => {} // Other options handled
            _ => names.push(*arg),
        }
    }

    if print_all || (names.is_empty() && !remove) {
        // Print completion specs
        let specs = state.get_completion_specs();
        for (name, spec) in specs {
            println!("complete {} {}", spec, name);
        }
        return Ok(0);
    }

    if remove {
        if names.is_empty() {
            state.clear_completion_specs();
        } else {
            for name in names {
                state.remove_completion_spec(name);
            }
        }
        return Ok(0);
    }

    // Set completion spec for names
    // Parse options and create spec string
    let spec = args
        .iter()
        .take_while(|a| a.starts_with('-'))
        .copied()
        .collect::<Vec<_>>()
        .join(" ");

    for name in names {
        state.set_completion_spec(name, &spec);
    }

    Ok(0)
}

/// compopt builtin - modify completion options
fn builtin_compopt(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut set_options: Vec<&str> = Vec::new();
    let mut unset_options: Vec<&str> = Vec::new();
    let mut names: Vec<&str> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "-o" => {
                if let Some(opt) = iter.next() {
                    set_options.push(*opt);
                }
            }
            "+o" => {
                if let Some(opt) = iter.next() {
                    unset_options.push(*opt);
                }
            }
            "-D" | "-E" | "-I" => {} // Default/empty/initial
            arg if arg.starts_with('-') || arg.starts_with('+') => {
                return Err(format!("compopt: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    // Modify completion options
    for name in &names {
        for opt in &set_options {
            state.set_completion_option(name, opt, true);
        }
        for opt in &unset_options {
            state.set_completion_option(name, opt, false);
        }
    }

    Ok(0)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Evaluate arithmetic expression (simplified)
fn evaluate_arithmetic(state: &mut ShellState, expr: &str) -> Result<i64, String> {
    // Remove outer quotes if present
    let expr = expr.trim_matches('"').trim_matches('\'');

    // Handle assignment
    if let Some((var, value_expr)) = expr.split_once('=') {
        let var = var.trim();
        if !value_expr.contains('=') {
            let value = evaluate_arithmetic(state, value_expr.trim())?;
            state.set_var(var, value.to_string());
            return Ok(value);
        }
    }

    // Handle increment/decrement
    if expr.ends_with("++") {
        let var = &expr[..expr.len() - 2];
        let val: i64 = state.get_var(var).and_then(|s| s.parse().ok()).unwrap_or(0);
        state.set_var(var, (val + 1).to_string());
        return Ok(val);
    }
    if expr.ends_with("--") {
        let var = &expr[..expr.len() - 2];
        let val: i64 = state.get_var(var).and_then(|s| s.parse().ok()).unwrap_or(0);
        state.set_var(var, (val - 1).to_string());
        return Ok(val);
    }
    if expr.starts_with("++") {
        let var = &expr[2..];
        let val: i64 = state.get_var(var).and_then(|s| s.parse().ok()).unwrap_or(0);
        let new_val = val + 1;
        state.set_var(var, new_val.to_string());
        return Ok(new_val);
    }
    if expr.starts_with("--") {
        let var = &expr[2..];
        let val: i64 = state.get_var(var).and_then(|s| s.parse().ok()).unwrap_or(0);
        let new_val = val - 1;
        state.set_var(var, new_val.to_string());
        return Ok(new_val);
    }

    // Try to parse as number
    if let Ok(n) = expr.parse::<i64>() {
        return Ok(n);
    }

    // Try as variable reference
    if let Some(val) = state.get_var(expr) {
        if let Ok(n) = val.parse::<i64>() {
            return Ok(n);
        }
    }

    // Simple binary operations
    for op in [
        "**", "<=", ">=", "==", "!=", "&&", "||", "<<", ">>", "+", "-", "*", "/", "%", "<", ">",
        "&", "|", "^",
    ] {
        if let Some((left, right)) = expr.split_once(op) {
            let left_val = evaluate_arithmetic(state, left.trim())?;
            let right_val = evaluate_arithmetic(state, right.trim())?;

            return Ok(match op {
                "+" => left_val + right_val,
                "-" => left_val - right_val,
                "*" => left_val * right_val,
                "/" => {
                    if right_val == 0 {
                        return Err("let: 除数为零".to_string());
                    }
                    left_val / right_val
                }
                "%" => {
                    if right_val == 0 {
                        return Err("let: 除数为零".to_string());
                    }
                    left_val % right_val
                }
                "**" => left_val.pow(right_val as u32),
                "<" => {
                    if left_val < right_val {
                        1
                    } else {
                        0
                    }
                }
                ">" => {
                    if left_val > right_val {
                        1
                    } else {
                        0
                    }
                }
                "<=" => {
                    if left_val <= right_val {
                        1
                    } else {
                        0
                    }
                }
                ">=" => {
                    if left_val >= right_val {
                        1
                    } else {
                        0
                    }
                }
                "==" => {
                    if left_val == right_val {
                        1
                    } else {
                        0
                    }
                }
                "!=" => {
                    if left_val != right_val {
                        1
                    } else {
                        0
                    }
                }
                "&&" => {
                    if left_val != 0 && right_val != 0 {
                        1
                    } else {
                        0
                    }
                }
                "||" => {
                    if left_val != 0 || right_val != 0 {
                        1
                    } else {
                        0
                    }
                }
                "&" => left_val & right_val,
                "|" => left_val | right_val,
                "^" => left_val ^ right_val,
                "<<" => left_val << right_val,
                ">>" => left_val >> right_val,
                _ => return Err(format!("let: 不支持的运算符: {}", op)),
            });
        }
    }

    // Unary operations
    if expr.starts_with('!') {
        let val = evaluate_arithmetic(state, &expr[1..])?;
        return Ok(if val == 0 { 1 } else { 0 });
    }
    if expr.starts_with('~') {
        let val = evaluate_arithmetic(state, &expr[1..])?;
        return Ok(!val);
    }
    if expr.starts_with('-') && expr.len() > 1 {
        let val = evaluate_arithmetic(state, &expr[1..])?;
        return Ok(-val);
    }
    if expr.starts_with('+') && expr.len() > 1 {
        return evaluate_arithmetic(state, &expr[1..]);
    }

    // Parentheses
    if expr.starts_with('(') && expr.ends_with(')') {
        return evaluate_arithmetic(state, &expr[1..expr.len() - 1]);
    }

    Err(format!("let: {}: 语法错误", expr))
}

/// shift builtin - shift positional parameters
fn builtin_shift(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let n = if args.is_empty() {
        1
    } else {
        match args[0].parse::<usize>() {
            Ok(n) => n,
            Err(_) => return Err(format!("shift: {}: 需要数字参数", args[0])),
        }
    };

    state
        .shift_positional_params(n)
        .map(|_| 0)
        .map_err(|e| e.to_string())
}

/// variables builtin - display shell variable information
fn builtin_variables(_state: &mut ShellState, _args: &[&str]) -> BuiltinResult {
    println!("Shell 变量:");
    println!();
    println!("  特殊参数:");
    println!("    $0          shell 或脚本的名称");
    println!("    $1-$9       位置参数");
    println!("    $#          位置参数的数量");
    println!("    $@          所有位置参数（作为单独的词）");
    println!("    $*          所有位置参数（作为单个词）");
    println!("    $?          最后执行的命令的退出状态");
    println!("    $$          当前 shell 的进程 ID");
    println!("    $!          最后后台命令的进程 ID");
    println!("    $_          上一条命令的最后一个参数");
    println!();
    println!("  常用环境变量:");
    println!("    HOME        当前用户的主目录");
    println!("    PATH        命令搜索路径");
    println!("    PWD         当前工作目录");
    println!("    OLDPWD      上一个工作目录");
    println!("    SHELL       当前 shell 的路径");
    println!("    USER        当前用户名");
    println!("    IFS         内部字段分隔符");
    println!();
    println!("  Shell 变量:");
    println!("    BASH_VERSION   shell 版本");
    println!("    HISTFILE       历史记录文件路径");
    println!("    HISTSIZE       历史记录条目数量");
    println!("    PS1            主提示符");
    println!("    PS2            续行提示符");
    println!("    RANDOM         0-32767 之间的随机数");
    println!("    LINENO         当前行号");
    println!("    SECONDS        shell 启动以来的秒数");
    Ok(0)
}

/// coproc builtin - create coprocess
fn builtin_coproc(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        return Err("coproc: 需要命令".to_string());
    }

    // Parse name and command
    let (name, cmd_start) = if args.len() > 1 && !args[0].contains('/') && !args[0].starts_with('.')
    {
        // First arg might be a name if it's not a command path
        (args[0], 1)
    } else {
        ("COPROC", 0)
    };

    if cmd_start >= args.len() {
        return Err("coproc: 需要命令".to_string());
    }

    let command = args[cmd_start..].join(" ");

    // Note: Full coprocess implementation would require pipe creation and
    // process forking, which is complex in this context
    // For now, provide a simplified implementation that just runs the command in background

    println!("coproc: 协进程功能尚未完全实现");
    println!("coproc: 将在后台运行: {}", command);

    // Store coproc name
    state.set_var(format!("{}_PID", name), "0");

    Ok(0)
}

/// time builtin - time command execution
fn builtin_time(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let posix_format = args.first().map(|a| *a == "-p").unwrap_or(false);
    let cmd_start = if posix_format { 1 } else { 0 };

    if cmd_start >= args.len() {
        // No command, just show empty time
        if posix_format {
            println!("real 0.00");
            println!("user 0.00");
            println!("sys 0.00");
        } else {
            println!("\nreal\t0m0.000s");
            println!("user\t0m0.000s");
            println!("sys\t0m0.000s");
        }
        return Ok(0);
    }

    let command = args[cmd_start..].join(" ");

    // Note: Full time implementation would need to fork and exec
    // For now, return a message
    println!("time: 计时功能需要在命令行解析器中实现");
    println!("time: 命令: {}", command);

    Ok(0)
}
