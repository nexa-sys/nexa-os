//! Shell Configuration Builtin Commands
//!
//! Commands: shopt, bind, ulimit, umask

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::ShellState;

/// Register configuration builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register(
        "shopt",
        BuiltinDesc::new(
            builtin_shopt,
            "设置和取消 shell 选项",
            "切换控制可选 shell 行为的选项值。\n\
         没有选项或使用 `-p' 选项时，显示所有可设置选项的列表，\n\
         并指出每个选项是否被设置；如果提供了 OPTNAME，\n\
         则将输出限制为这些选项。\n\
         \n\
         选项:\n\
           -o    将 OPTNAME 限制为那些为 `set -o' 定义的选项\n\
           -p    以可重用作输入的格式显示每个 shell 选项\n\
           -q    抑制普通输出；返回状态指示 OPTNAME 是否被设置\n\
           -s    启用（设置）每个 OPTNAME\n\
           -u    禁用（取消）每个 OPTNAME\n\
         \n\
         如果使用 OPTNAME 调用且没有 -s 或 -u，则显示该选项的当前状态。\n\
         \n\
         如果 OPTNAME 启用则返回成功，如果给出无效选项或\n\
         OPTNAME 被禁用则返回失败。",
            "shopt [-pqsu] [-o] [选项名 ...]",
            true,
        ),
    );

    registry.register("bind", BuiltinDesc::new(
        builtin_bind,
        "设置 Readline 键绑定和变量",
        "设置 Readline 键绑定和变量。\n\
         \n\
         使用语法绑定 KEYSEQ 到 shell 命令 COMMAND，\n\
         像用户从终端键入 COMMAND 一样。\n\
         \n\
         选项:\n\
           -m KEYMAP     使用 KEYMAP 作为后续绑定的键映射\n\
           -l            列出函数名称\n\
           -P            列出函数名称和绑定\n\
           -p            以可重用作输入的格式列出函数和绑定\n\
           -S            列出能启动宏的键序列及其值\n\
           -s            以可重用作输入的格式列出键序列和绑定\n\
           -V            列出变量名称和值\n\
           -v            以可重用作输入的格式列出变量名称和值\n\
           -q FUNCTION   查询哪些键调用指定的 FUNCTION\n\
           -u FUNCTION   解绑绑定到 FUNCTION 的所有键\n\
           -r KEYSEQ     移除 KEYSEQ 的绑定\n\
           -f FILENAME   从 FILENAME 读取键绑定\n\
           -x KEYSEQ:SHELL-COMMAND  当 KEYSEQ 被输入时执行 SHELL-COMMAND\n\
           -X            以可重用作输入的格式列出通过 -x 绑定的键序列和命令\n\
         \n\
         如果没有给出无效选项则返回成功。",
        "bind [-lpvsPSVX] [-m 键映射] [-f 文件名] [-q 名称] [-u 名称] [-r 键序列] [-x 键序列:shell-命令] [键序列:readline-函数 或 readline-命令]",
        true,
    ));

    registry.register(
        "ulimit",
        BuiltinDesc::new(
            builtin_ulimit,
            "修改 shell 资源限制",
            "提供控制 shell 启动的进程可用资源的功能，\n\
         在允许此类控制的系统上。\n\
         \n\
         选项:\n\
           -S    使用 `软' 资源限制\n\
           -H    使用 `硬' 资源限制\n\
           -a    报告所有当前限制\n\
           -b    套接字缓冲区大小\n\
           -c    创建的核心文件的最大大小\n\
           -d    进程数据段的最大大小\n\
           -e    最大调度优先级（'nice'）\n\
           -f    shell 及其子进程写入的文件的最大大小\n\
           -i    等待中的信号的最大数量\n\
           -k    为此进程分配的最大 kqueue 数\n\
           -l    进程可以锁定到内存中的最大大小\n\
           -m    最大驻留集大小\n\
           -n    打开的文件描述符的最大数量\n\
           -p    管道缓冲区大小\n\
           -q    POSIX 消息队列中的最大字节数\n\
           -r    最大实时调度优先级\n\
           -s    最大栈大小\n\
           -t    最大 CPU 时间量（秒）\n\
           -u    用户可用的最大进程数\n\
           -v    进程可用的虚拟内存量\n\
           -x    文件锁的最大数量\n\
           -P    伪终端的最大数量\n\
           -R    进程在被阻塞前可运行的最大实时微秒数\n\
           -T    线程的最大数量\n\
         \n\
         不是所有选项在所有平台上都可用。\n\
         \n\
         如果给出了 LIMIT，它是特定资源的新值；特殊的 LIMIT 值\n\
         `soft'、`hard' 和 `unlimited' 分别代表当前软限制、\n\
         当前硬限制和无限制。\n\
         否则，打印指定资源的当前值。\n\
         如果没有给出选项，假定 -f。\n\
         \n\
         值以 1024 字节为增量，除了 -t 使用秒，-p 使用 512 字节块，\n\
         -u 是未缩放的进程数。\n\
         \n\
         返回成功，除非给出无效选项或发生错误。",
            "ulimit [-SHabcdefiklmnpqrstuvxPRT] [限制]",
            true,
        ),
    );

    registry.register(
        "umask",
        BuiltinDesc::new(
            builtin_umask,
            "显示或设置文件模式掩码",
            "设置用户文件创建掩码为 MODE。如果 MODE 被省略，\n\
         打印当前掩码值。\n\
         \n\
         如果 MODE 以数字开头，则被解释为八进制数；\n\
         否则是类似于 chmod(1) 接受的符号模式掩码。\n\
         \n\
         选项:\n\
           -p    如果 MODE 被省略，以可重用作输入的格式输出\n\
           -S    以符号形式输出掩码；否则输出八进制数\n\
         \n\
         如果成功改变了 MODE 或没有给出 MODE 参数则返回成功，\n\
         否则返回失败。",
            "umask [-p] [-S] [模式]",
            true,
        ),
    );
}

/// shopt builtin - set and unset shell options
fn builtin_shopt(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut set_mode = false;
    let mut unset_mode = false;
    let mut quiet = false;
    let mut print_reusable = false;
    let mut use_set_options = false;
    let mut optnames: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-s" => set_mode = true,
            "-u" => unset_mode = true,
            "-q" => quiet = true,
            "-p" => print_reusable = true,
            "-o" => use_set_options = true,
            arg if arg.starts_with('-') => {
                return Err(format!("shopt: {}: 无效选项", arg));
            }
            _ => optnames.push(*arg),
        }
    }

    // Get shopt options list
    let shopts = state.get_shopts();

    // If setting/unsetting
    if set_mode || unset_mode {
        for name in &optnames {
            if use_set_options {
                // -o options are handled by set builtin
                if set_mode {
                    state.set_option(name, true)?;
                } else {
                    state.set_option(name, false)?;
                }
            } else {
                if !shopts.contains_key(*name) {
                    return Err(format!("shopt: {}: 无效的 shell 选项名", name));
                }
                state.set_shopt(name, set_mode);
            }
        }
        return Ok(0);
    }

    // Display mode
    let display_list: Vec<_> = if optnames.is_empty() {
        if use_set_options {
            // Show set -o options
            state.get_set_options().into_iter().collect()
        } else {
            shopts.into_iter().collect()
        }
    } else {
        optnames
            .iter()
            .filter_map(|name| {
                if use_set_options {
                    state.get_set_option(name).map(|v| (name.to_string(), v))
                } else {
                    shopts.get(*name).map(|v| (name.to_string(), *v))
                }
            })
            .collect()
    };

    if quiet {
        // Check if all specified options are set
        for (_, enabled) in &display_list {
            if !enabled {
                return Ok(1);
            }
        }
        return Ok(0);
    }

    // Print options
    for (name, enabled) in display_list {
        if print_reusable {
            println!("shopt {} {}", if enabled { "-s" } else { "-u" }, name);
        } else {
            println!("{:<20} {}", name, if enabled { "on" } else { "off" });
        }
    }

    Ok(0)
}

/// bind builtin - set readline key bindings
fn builtin_bind(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut list_functions = false;
    let mut list_bindings = false;
    let mut list_variables = false;
    let mut list_macros = false;

    for arg in args {
        match *arg {
            "-l" => list_functions = true,
            "-P" | "-p" => list_bindings = true,
            "-V" | "-v" => list_variables = true,
            "-S" | "-s" => list_macros = true,
            "-m" | "-q" | "-u" | "-r" | "-f" | "-x" | "-X" => {
                // These require additional arguments - not fully implemented
            }
            arg if arg.starts_with('-') => {
                return Err(format!("bind: {}: 无效选项", arg));
            }
            _ => {} // Key binding spec
        }
    }

    if list_functions {
        // List readline function names
        let functions = [
            "accept-line",
            "backward-char",
            "backward-delete-char",
            "backward-kill-line",
            "backward-kill-word",
            "backward-word",
            "beginning-of-history",
            "beginning-of-line",
            "call-last-kbd-macro",
            "capitalize-word",
            "clear-screen",
            "complete",
            "delete-char",
            "delete-horizontal-space",
            "digit-argument",
            "downcase-word",
            "dump-functions",
            "dump-macros",
            "dump-variables",
            "end-of-history",
            "end-of-line",
            "exchange-point-and-mark",
            "forward-char",
            "forward-search-history",
            "forward-word",
            "history-search-backward",
            "history-search-forward",
            "insert-comment",
            "kill-line",
            "kill-region",
            "kill-word",
            "next-history",
            "previous-history",
            "quoted-insert",
            "re-read-init-file",
            "redraw-current-line",
            "reverse-search-history",
            "self-insert",
            "set-mark",
            "transpose-chars",
            "transpose-words",
            "undo",
            "universal-argument",
            "unix-line-discard",
            "unix-word-rubout",
            "upcase-word",
            "yank",
            "yank-last-arg",
            "yank-pop",
        ];
        for func in functions {
            println!("{}", func);
        }
        return Ok(0);
    }

    if list_bindings {
        // Show common key bindings
        println!("\"\\C-a\": beginning-of-line");
        println!("\"\\C-e\": end-of-line");
        println!("\"\\C-f\": forward-char");
        println!("\"\\C-b\": backward-char");
        println!("\"\\C-d\": delete-char");
        println!("\"\\C-h\": backward-delete-char");
        println!("\"\\C-k\": kill-line");
        println!("\"\\C-u\": unix-line-discard");
        println!("\"\\C-w\": unix-word-rubout");
        println!("\"\\C-l\": clear-screen");
        println!("\"\\C-p\": previous-history");
        println!("\"\\C-n\": next-history");
        println!("\"\\C-r\": reverse-search-history");
        println!("\"\\e[A\": previous-history");
        println!("\"\\e[B\": next-history");
        println!("\"\\e[C\": forward-char");
        println!("\"\\e[D\": backward-char");
        return Ok(0);
    }

    if list_variables {
        // Show readline variables
        println!("bell-style audible");
        println!("bind-tty-special-chars on");
        println!("blink-matching-paren on");
        println!("colored-completion-prefix off");
        println!("colored-stats off");
        println!("comment-begin #");
        println!("completion-display-width -1");
        println!("completion-ignore-case off");
        println!("completion-map-case off");
        println!("completion-prefix-display-length 0");
        println!("completion-query-items 100");
        println!("convert-meta on");
        println!("disable-completion off");
        println!("editing-mode emacs");
        println!("echo-control-characters on");
        println!("enable-bracketed-paste on");
        println!("enable-keypad off");
        println!("expand-tilde off");
        println!("history-preserve-point off");
        println!("history-size 500");
        println!("horizontal-scroll-mode off");
        println!("input-meta on");
        println!("keyseq-timeout 500");
        println!("mark-directories on");
        println!("mark-modified-lines off");
        println!("mark-symlinked-directories off");
        println!("match-hidden-files on");
        println!("menu-complete-display-prefix off");
        println!("output-meta on");
        println!("page-completions on");
        println!("print-completions-horizontally off");
        println!("revert-all-at-newline off");
        println!("show-all-if-ambiguous off");
        println!("show-all-if-unmodified off");
        println!("show-mode-in-prompt off");
        println!("skip-completed-text off");
        println!("visible-stats off");
        return Ok(0);
    }

    if list_macros {
        // No macros defined by default
        return Ok(0);
    }

    // Without options, bind expects a key binding spec
    if args.is_empty() {
        return Err("bind: 用法: bind [-lpvsPSVX] [-m 键映射] [-f 文件名] [-q 名称] [-u 名称] [-r 键序列] [-x 键序列:shell-命令] [键序列:readline-函数]".to_string());
    }

    Ok(0)
}

/// ulimit builtin - modify shell resource limits
fn builtin_ulimit(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut show_all = false;
    let mut soft = true;
    let mut hard = false;
    let mut resource = 'f'; // Default: file size
    let mut limit_value: Option<&str> = None;

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match *arg {
            "-S" => {
                soft = true;
                hard = false;
            }
            "-H" => {
                hard = true;
                soft = false;
            }
            "-a" => show_all = true,
            "-c" => resource = 'c',
            "-d" => resource = 'd',
            "-e" => resource = 'e',
            "-f" => resource = 'f',
            "-i" => resource = 'i',
            "-l" => resource = 'l',
            "-m" => resource = 'm',
            "-n" => resource = 'n',
            "-p" => resource = 'p',
            "-q" => resource = 'q',
            "-r" => resource = 'r',
            "-s" => resource = 's',
            "-t" => resource = 't',
            "-u" => resource = 'u',
            "-v" => resource = 'v',
            "-x" => resource = 'x',
            "-P" => resource = 'P',
            "-R" => resource = 'R',
            "-T" => resource = 'T',
            arg if arg.starts_with('-') => {
                return Err(format!("ulimit: {}: 无效选项", arg));
            }
            _ => limit_value = Some(*arg),
        }
    }

    if show_all {
        // Display all limits
        println!("core file size          (blocks, -c) unlimited");
        println!("data seg size           (kbytes, -d) unlimited");
        println!("scheduling priority             (-e) 0");
        println!("file size               (blocks, -f) unlimited");
        println!("pending signals                 (-i) 127890");
        println!("max locked memory       (kbytes, -l) 65536");
        println!("max memory size         (kbytes, -m) unlimited");
        println!("open files                      (-n) 1024");
        println!("pipe size            (512 bytes, -p) 8");
        println!("POSIX message queues     (bytes, -q) 819200");
        println!("real-time priority              (-r) 0");
        println!("stack size              (kbytes, -s) 8192");
        println!("cpu time               (seconds, -t) unlimited");
        println!("max user processes              (-u) 127890");
        println!("virtual memory          (kbytes, -v) unlimited");
        println!("file locks                      (-x) unlimited");
        return Ok(0);
    }

    // Get or set specific limit
    let (desc, _unit) = match resource {
        'c' => ("core file size (blocks)", "blocks"),
        'd' => ("data seg size (kbytes)", "kbytes"),
        'e' => ("scheduling priority", ""),
        'f' => ("file size (blocks)", "blocks"),
        'i' => ("pending signals", ""),
        'l' => ("max locked memory (kbytes)", "kbytes"),
        'm' => ("max memory size (kbytes)", "kbytes"),
        'n' => ("open files", ""),
        'p' => ("pipe size (512 bytes)", "512 bytes"),
        'q' => ("POSIX message queues (bytes)", "bytes"),
        'r' => ("real-time priority", ""),
        's' => ("stack size (kbytes)", "kbytes"),
        't' => ("cpu time (seconds)", "seconds"),
        'u' => ("max user processes", ""),
        'v' => ("virtual memory (kbytes)", "kbytes"),
        'x' => ("file locks", ""),
        'P' => ("pseudoterminals", ""),
        'R' => ("real-time timeout (microseconds)", "microseconds"),
        'T' => ("threads", ""),
        _ => ("unknown", ""),
    };

    if let Some(value) = limit_value {
        // Set limit
        let _limit = match value {
            "unlimited" => None,
            "soft" => None, // Use soft limit
            "hard" => None, // Use hard limit
            _ => value.parse::<u64>().ok(),
        };

        // Actually setting limits requires platform-specific code
        #[cfg(target_family = "unix")]
        {
            // Would use setrlimit here
            println!("ulimit: 设置限制功能尚未完全实现");
        }
        Ok(0)
    } else {
        // Show limit
        #[cfg(target_family = "unix")]
        {
            // Would use getrlimit here
            println!("{}: unlimited", desc);
        }
        #[cfg(not(target_family = "unix"))]
        {
            println!("{}: 此平台不支持", desc);
        }
        Ok(0)
    }
}

/// umask builtin - display or set file mode mask
fn builtin_umask(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut symbolic = false;
    let mut print_format = false;
    let mut mode_arg: Option<&str> = None;

    for arg in args {
        match *arg {
            "-S" => symbolic = true,
            "-p" => print_format = true,
            arg if arg.starts_with('-') => {
                return Err(format!("umask: {}: 无效选项", arg));
            }
            _ => mode_arg = Some(*arg),
        }
    }

    if let Some(mode) = mode_arg {
        // Set umask
        let mask = if mode.chars().all(|c| c.is_ascii_digit()) {
            // Octal mode
            u32::from_str_radix(mode, 8).map_err(|_| format!("umask: {}: 无效的八进制数", mode))?
        } else {
            // Symbolic mode (simplified)
            parse_symbolic_umask(mode, state.get_umask())?
        };

        state.set_umask(mask);
        Ok(0)
    } else {
        // Display umask
        let mask = state.get_umask();

        if symbolic {
            // Symbolic format: u=rwx,g=rwx,o=rwx
            let u = format_mask_part((mask >> 6) & 7);
            let g = format_mask_part((mask >> 3) & 7);
            let o = format_mask_part(mask & 7);
            if print_format {
                println!("umask -S u={},g={},o={}", u, g, o);
            } else {
                println!("u={},g={},o={}", u, g, o);
            }
        } else {
            // Octal format
            if print_format {
                println!("umask {:04o}", mask);
            } else {
                println!("{:04o}", mask);
            }
        }
        Ok(0)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Format a permission mask part (e.g., 7 -> "rwx", 5 -> "r-x")
fn format_mask_part(bits: u32) -> String {
    let inverted = 7 - bits; // umask is inverted permissions
    let mut s = String::with_capacity(3);
    s.push(if inverted & 4 != 0 { 'r' } else { '-' });
    s.push(if inverted & 2 != 0 { 'w' } else { '-' });
    s.push(if inverted & 1 != 0 { 'x' } else { '-' });
    s
}

/// Parse symbolic umask mode (simplified)
fn parse_symbolic_umask(mode: &str, current: u32) -> Result<u32, String> {
    let mut result = current;

    for part in mode.split(',') {
        let mut chars = part.chars().peekable();

        // Parse who: u, g, o, a
        let mut who = 0u32;
        while let Some(&c) = chars.peek() {
            match c {
                'u' => {
                    who |= 0o700;
                    chars.next();
                }
                'g' => {
                    who |= 0o070;
                    chars.next();
                }
                'o' => {
                    who |= 0o007;
                    chars.next();
                }
                'a' => {
                    who |= 0o777;
                    chars.next();
                }
                _ => break,
            }
        }
        if who == 0 {
            who = 0o777; // Default to all
        }

        // Parse operator: +, -, =
        let op = chars
            .next()
            .ok_or_else(|| format!("umask: {}: 无效的符号模式", mode))?;

        // Parse permissions: r, w, x
        let mut perm = 0u32;
        for c in chars {
            match c {
                'r' => perm |= 0o444,
                'w' => perm |= 0o222,
                'x' => perm |= 0o111,
                _ => return Err(format!("umask: {}: 无效的符号模式", mode)),
            }
        }

        // Apply based on who
        perm &= who;

        match op {
            '+' => result &= !perm, // Adding permission = removing from mask
            '-' => result |= perm,  // Removing permission = adding to mask
            '=' => {
                result &= !who; // Clear who bits
                result |= who & !perm; // Set mask for unspecified permissions
            }
            _ => return Err(format!("umask: {}: 无效的符号模式", mode)),
        }
    }

    Ok(result & 0o777)
}
