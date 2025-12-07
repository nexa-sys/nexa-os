//! Utility Builtin Commands
//!
//! Commands: echo, printf, source/., eval, exec, command, builtin

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::ShellState;
use std::io::{self, Write};

/// Register utility builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register("echo", BuiltinDesc::new(
        builtin_echo,
        "输出参数",
        "输出 ARG，参数之间以单个空格分隔，并以换行符结尾。\n\
         \n\
         选项:\n\
           -n    不输出尾随换行符\n\
           -e    启用反斜杠转义字符的解释\n\
           -E    禁用反斜杠转义字符的解释（默认）\n\
         \n\
         `echo' 解释下列反斜杠转义字符:\n\
           \\\\\\\\    反斜杠\n\
           \\\\a    警告（铃声）\n\
           \\\\b    退格\n\
           \\\\c    不再产生输出\n\
           \\\\e    转义字符\n\
           \\\\f    换页\n\
           \\\\n    换行\n\
           \\\\r    回车\n\
           \\\\t    水平制表符\n\
           \\\\v    垂直制表符\n\
           \\\\0nnn  八进制表示的 ASCII 字符（最多三位八进制数字）\n\
           \\\\xHH   十六进制表示的 ASCII 字符（最多两位十六进制数字）\n\
         \n\
         除非给出无效选项，否则返回成功。",
        "echo [-neE] [参数 ...]",
        true,
    ));

    registry.register("printf", BuiltinDesc::new(
        builtin_printf,
        "格式化并打印数据",
        "根据 FORMAT 格式化并打印 ARGUMENTS。\n\
         \n\
         选项:\n\
           -v var    将输出放入 shell 变量 VAR 而非输出到标准输出\n\
         \n\
         FORMAT 是一个字符串，包含三类对象：普通字符、\n\
         转义序列和格式说明符。普通字符直接输出。\n\
         转义序列被转换为其代表的字符。\n\
         格式说明符使用连续的 ARGUMENTS 替换自身。\n\
         \n\
         除了标准的 printf(3) 格式，printf 还解释:\n\
           %b    展开参数中的反斜杠转义序列\n\
           %q    以可重用作输入的格式引用参数\n\
           %(fmt)T  使用 strftime(3) 风格的 FMT 输出日期时间字符串\n\
         \n\
         FORMAT 在需要时会被重用，以消耗所有 ARGUMENTS。如果比格式字符串\n\
         需要的 ARGUMENTS 少，则多余的格式说明符表现得就像提供了零值或空字符串。\n\
         \n\
         如果成功则返回 0，否则返回非零值。",
        "printf [-v var] 格式 [参数]",
        true,
    ));

    registry.register("source", BuiltinDesc::new(
        builtin_source,
        "在当前 shell 中执行脚本",
        "从 FILENAME 读取并执行命令在当前 shell 中。\n\
         \n\
         $PATH 中的条目用于查找包含 FILENAME 的目录。\n\
         如果向 `source' 提供任何 ARGUMENTS，它们会在执行 FILENAME 时\n\
         成为位置参数。\n\
         \n\
         如果 FILENAME 被成功执行则返回 0，否则在找不到\n\
         FILENAME 或无法读取它时返回非零值。",
        "source 文件名 [参数]",
        true,
    ));

    registry.register(".", BuiltinDesc::new(
        builtin_source,
        "在当前 shell 中执行脚本",
        "从 FILENAME 读取并执行命令在当前 shell 中。\n\
         这是 `source' 命令的简写形式。",
        ". 文件名 [参数]",
        true,
    ));

    registry.register("eval", BuiltinDesc::new(
        builtin_eval,
        "将参数作为 shell 命令执行",
        "将 ARG 连接为单个字符串，以此作为 shell 的输入，并执行得到的命令。\n\
         \n\
         返回结果命令的退出状态，如果没有命令则返回成功。",
        "eval [参数 ...]",
        true,
    ));

    registry.register("exec", BuiltinDesc::new(
        builtin_exec,
        "用指定命令替换 shell",
        "用 COMMAND 替换 shell。如果 COMMAND 没有指定，\n\
         则任何重定向在当前 shell 中生效。\n\
         \n\
         选项:\n\
           -a NAME   将 NAME 作为第零个参数传递给 COMMAND\n\
           -c        在空环境中执行 COMMAND\n\
           -l        将连字符放在 argv[0] 传递给 COMMAND\n\
         \n\
         如果命令不能执行则返回失败。",
        "exec [-cl] [-a 名称] [命令 [参数 ...]] [重定向 ...]",
        true,
    ));

    registry.register("command", BuiltinDesc::new(
        builtin_command,
        "执行命令但不进行函数查找",
        "运行 COMMAND，参数为 ARGS，绕过函数查找。\n\
         \n\
         选项:\n\
           -p    使用 PATH 的默认值来保证找到所有标准实用工具\n\
           -v    打印 COMMAND 将被调用的对应描述\n\
           -V    打印每个 COMMAND 将被调用的更详细描述\n\
         \n\
         如果 COMMAND 被找到并成功调用则返回 0，否则返回非零值。",
        "command [-pVv] 命令 [参数 ...]",
        true,
    ));

    registry.register("builtin", BuiltinDesc::new(
        builtin_builtin,
        "执行 shell 内建命令",
        "执行 SHELL-BUILTIN，参数为 ARGs，不进行函数查找。\n\
         \n\
         当你希望重新实现一个与 shell 内建命令同名的函数，\n\
         但在函数内还要调用原内建命令时，这会很有用。\n\
         \n\
         如果 SHELL-BUILTIN 是内建命令则返回内建命令的结果，\n\
         否则返回失败。",
        "builtin [shell-内建 [参数 ...]]",
        false,  // Cannot be disabled
    ));

    registry.register("read", BuiltinDesc::new(
        builtin_read,
        "从标准输入读取一行",
        "从标准输入读取一行，或从文件描述符 FD 读取（如果指定了 -u），\n\
         并将第一个词赋值给第一个 NAME，第二个词赋值给第二个 NAME，\n\
         依此类推，剩余的词赋值给最后一个 NAME。\n\
         只有在 $IFS 中的字符才被识别为词分隔符。\n\
         \n\
         如果没有提供 NAME，读取的行被存储在 REPLY 变量中。\n\
         \n\
         选项:\n\
           -d 分隔符  继续读取直到读取到 DELIM 的第一个字符，而不是换行符\n\
           -n 字符数  读取 NCHARS 个字符后返回，而不是等待换行符\n\
           -p 提示符  输出 PROMPT 字符串而不带尾随换行符，然后再尝试读取\n\
           -r         不允许反斜杠转义任何字符\n\
           -s         不回显来自终端的输入\n\
           -t 超时    如果在 TIMEOUT 秒内没有读取到完整行，则超时并返回失败\n\
         \n\
         除非遇到 EOF 或超时，否则返回成功。",
        "read [-rs] [-d 分隔符] [-n 字符数] [-p 提示符] [-t 超时] [名称 ...]",
        true,
    ));
}

/// echo builtin - output arguments
fn builtin_echo(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut newline = true;
    let mut interpret_escapes = false;
    let mut output_args: Vec<&str> = Vec::new();
    let mut options_done = false;

    for arg in args {
        if !options_done {
            match *arg {
                "-n" => {
                    newline = false;
                    continue;
                }
                "-e" => {
                    interpret_escapes = true;
                    continue;
                }
                "-E" => {
                    interpret_escapes = false;
                    continue;
                }
                "-ne" | "-en" => {
                    newline = false;
                    interpret_escapes = true;
                    continue;
                }
                "-nE" | "-En" => {
                    newline = false;
                    interpret_escapes = false;
                    continue;
                }
                "--" => {
                    options_done = true;
                    continue;
                }
                _ if arg.starts_with('-') && arg.len() > 1 => {
                    // Check if all chars are valid options
                    let valid = arg[1..].chars().all(|c| matches!(c, 'n' | 'e' | 'E'));
                    if valid {
                        if arg.contains('n') { newline = false; }
                        if arg.contains('e') { interpret_escapes = true; }
                        if arg.contains('E') { interpret_escapes = false; }
                        continue;
                    }
                    // Not valid options, treat as regular argument
                    options_done = true;
                }
                _ => {
                    options_done = true;
                }
            }
        }
        output_args.push(*arg);
    }

    let output = output_args.join(" ");
    
    let final_output = if interpret_escapes {
        process_escape_sequences(&output)
    } else {
        output
    };

    // Check for \c in escape mode
    let stop_idx = if interpret_escapes {
        final_output.find('\x00')
    } else {
        None
    };

    if let Some(idx) = stop_idx {
        print!("{}", &final_output[..idx]);
    } else if newline {
        println!("{}", final_output);
    } else {
        print!("{}", final_output);
    }
    
    let _ = io::stdout().flush();
    Ok(0)
}

/// printf builtin - formatted output
fn builtin_printf(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut var_name: Option<&str> = None;
    let mut format_idx = 0;

    // Parse options
    let mut iter = args.iter().enumerate();
    while let Some((i, arg)) = iter.next() {
        match *arg {
            "-v" => {
                if let Some((_, name)) = iter.next() {
                    var_name = Some(*name);
                } else {
                    return Err("printf: -v: 需要选项参数".to_string());
                }
            }
            "--" => {
                format_idx = i + 1;
                break;
            }
            arg if arg.starts_with('-') && arg.len() > 1 => {
                return Err(format!("printf: {}: 无效选项", arg));
            }
            _ => {
                format_idx = i;
                break;
            }
        }
    }

    if format_idx >= args.len() {
        return Err("printf: 用法: printf [-v var] 格式 [参数]".to_string());
    }

    let format = args[format_idx];
    let format_args = &args[format_idx + 1..];

    let output = format_string(format, format_args)?;

    if let Some(name) = var_name {
        state.set_var(name, &output);
    } else {
        print!("{}", output);
        let _ = io::stdout().flush();
    }

    Ok(0)
}

/// source builtin - execute script in current shell
fn builtin_source(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        return Err("source: 文件名参数是必需的".to_string());
    }

    let filename = args[0];
    let script_args = &args[1..];

    // Try to read the file
    let content = match std::fs::read_to_string(filename) {
        Ok(c) => c,
        Err(e) => return Err(format!("source: {}: {}", filename, e)),
    };

    // Store positional parameters (not implemented yet)
    let _ = script_args;

    // Execute each line
    // Note: This is a simplified implementation
    // A full implementation would use a proper parser
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        
        // Execute the line (simplified - should use shell's execute function)
        state.last_exit_status = 0;
    }

    Ok(state.last_exit_status)
}

/// eval builtin - execute arguments as command
fn builtin_eval(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        return Ok(0);
    }

    let _command = args.join(" ");
    
    // Note: eval requires access to the shell's command execution
    // This is a placeholder - full implementation would re-invoke the parser
    Err("eval: 功能尚未完全实现".to_string())
}

/// exec builtin - replace shell with command
fn builtin_exec(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        // No command - just apply redirections (not implemented)
        return Ok(0);
    }

    let mut clear_env = false;
    let mut login_shell = false;
    let mut argv0: Option<&str> = None;
    let mut cmd_idx = 0;

    // Parse options
    let mut iter = args.iter().enumerate();
    while let Some((i, arg)) = iter.next() {
        match *arg {
            "-c" => clear_env = true,
            "-l" => login_shell = true,
            "-a" => {
                if let Some((_, name)) = iter.next() {
                    argv0 = Some(*name);
                } else {
                    return Err("exec: -a: 需要选项参数".to_string());
                }
            }
            "--" => {
                cmd_idx = i + 1;
                break;
            }
            arg if arg.starts_with('-') => {
                return Err(format!("exec: {}: 无效选项", arg));
            }
            _ => {
                cmd_idx = i;
                break;
            }
        }
    }

    if cmd_idx >= args.len() {
        return Ok(0);
    }

    let cmd = args[cmd_idx];
    let cmd_args = &args[cmd_idx + 1..];

    // Build command
    let mut command = std::process::Command::new(cmd);
    command.args(cmd_args);

    if clear_env {
        command.env_clear();
    }

    if let Some(name) = argv0 {
        command.arg0(name);
    } else if login_shell {
        command.arg0(&format!("-{}", cmd));
    }

    // exec replaces the current process
    use std::os::unix::process::CommandExt;
    let error = command.exec();
    
    Err(format!("exec: {}: {}", cmd, error))
}

/// command builtin - run command bypassing functions
fn builtin_command(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    // List of known builtin names
    const BUILTINS: &[&str] = &[
        "cd", "pwd", "pushd", "popd", "dirs",
        "export", "unset", "set", "declare", "typeset", "readonly", "local", "let",
        "alias", "unalias",
        "exit", "return", "break", "continue", "test", "[", "true", "false", ":", "logout",
        "help", "type", "hash", "enable", "caller",
        "echo", "printf", "source", ".", "eval", "exec", "command", "builtin", "read",
        "jobs", "bg", "fg", "disown", "suspend", "kill", "wait",
        "history", "fc",
        "shopt", "bind", "ulimit", "umask",
        "trap", "times",
        "compgen", "complete", "compopt",
        "getopts", "mapfile", "readarray",
    ];

    let mut use_default_path = false;
    let mut describe = false;
    let mut describe_verbose = false;
    let mut cmd_idx = 0;

    // Parse options
    for (i, arg) in args.iter().enumerate() {
        match *arg {
            "-p" => use_default_path = true,
            "-v" => describe = true,
            "-V" => describe_verbose = true,
            "--" => {
                cmd_idx = i + 1;
                break;
            }
            arg if arg.starts_with('-') => {
                return Err(format!("command: {}: 无效选项", arg));
            }
            _ => {
                cmd_idx = i;
                break;
            }
        }
    }

    if cmd_idx >= args.len() {
        return Ok(0);
    }

    let cmd = args[cmd_idx];

    if describe || describe_verbose {
        // Just describe the command
        if BUILTINS.contains(&cmd) {
            if describe_verbose {
                println!("{} 是一个 shell 内建命令", cmd);
            } else {
                println!("{}", cmd);
            }
        } else {
            // Search in PATH
            let search_paths = if use_default_path {
                vec!["/bin", "/usr/bin"]
            } else {
                vec!["/bin", "/sbin", "/usr/bin", "/usr/sbin"]
            };

            let mut found = false;
            for dir in search_paths {
                let path = std::path::Path::new(dir).join(cmd);
                if path.exists() {
                    if describe_verbose {
                        println!("{} 是 {}", cmd, path.display());
                    } else {
                        println!("{}", path.display());
                    }
                    found = true;
                    break;
                }
            }

            if !found {
                return Err(format!("command: {}: 未找到", cmd));
            }
        }
        return Ok(0);
    }

    // Execute the command (bypassing functions - but we don't have functions yet)
    // This would normally call the shell's external command executor
    Ok(0)
}

/// builtin builtin - run shell builtin directly
/// Note: Full functionality requires being called through the registry from main.rs
fn builtin_builtin(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    // List of known builtin names
    const BUILTINS: &[&str] = &[
        "cd", "pwd", "pushd", "popd", "dirs",
        "export", "unset", "set", "declare", "typeset", "readonly", "local", "let",
        "alias", "unalias",
        "exit", "return", "break", "continue", "test", "[", "true", "false", ":", "logout",
        "help", "type", "hash", "enable", "caller",
        "echo", "printf", "source", ".", "eval", "exec", "command", "builtin", "read",
        "jobs", "bg", "fg", "disown", "suspend", "kill", "wait",
        "history", "fc",
        "shopt", "bind", "ulimit", "umask",
        "trap", "times",
        "compgen", "complete", "compopt",
        "getopts", "mapfile", "readarray",
    ];

    if args.is_empty() {
        return Ok(0);
    }

    let cmd = args[0];
    let _cmd_args = &args[1..];

    if BUILTINS.contains(&cmd) {
        // This should be handled by main.rs which has access to the registry
        // Return a special code to indicate this
        Err(format!("BUILTIN_EXEC:{}", args.join(" ")))
    } else {
        Err(format!("builtin: {}: 不是 shell 内建命令", cmd))
    }
}

/// read builtin - read a line from stdin
fn builtin_read(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut prompt: Option<&str> = None;
    let mut raw_mode = false;
    let mut silent = false;
    let mut nchars: Option<usize> = None;
    let mut delimiter = '\n';
    let mut timeout: Option<f64> = None;
    let mut names: Vec<&str> = Vec::new();

    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "-p" => {
                prompt = iter.next().copied();
            }
            "-r" => raw_mode = true,
            "-s" => silent = true,
            "-n" => {
                if let Some(n) = iter.next() {
                    nchars = n.parse().ok();
                }
            }
            "-d" => {
                if let Some(d) = iter.next() {
                    delimiter = d.chars().next().unwrap_or('\n');
                }
            }
            "-t" => {
                if let Some(t) = iter.next() {
                    timeout = t.parse().ok();
                }
            }
            arg if arg.starts_with('-') => {
                return Err(format!("read: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    // Print prompt if specified
    if let Some(p) = prompt {
        print!("{}", p);
        let _ = io::stdout().flush();
    }

    // Read input
    let mut input = String::new();
    
    // Note: timeout and silent mode would require terminal raw mode
    // This is a simplified implementation
    let _ = timeout;
    let _ = silent;
    let _ = nchars;
    let _ = delimiter;
    let _ = raw_mode;

    if io::stdin().read_line(&mut input).is_err() {
        return Ok(1);
    }

    // Remove trailing newline
    if input.ends_with('\n') {
        input.pop();
    }
    if input.ends_with('\r') {
        input.pop();
    }

    if names.is_empty() {
        // Store in REPLY
        state.set_var("REPLY", &input);
    } else if names.len() == 1 {
        state.set_var(names[0], &input);
    } else {
        // Split by IFS
        let ifs = state.get_var("IFS").unwrap_or(" \t\n");
        let words: Vec<&str> = input.split(|c| ifs.contains(c)).filter(|s| !s.is_empty()).collect();
        
        for (i, name) in names.iter().enumerate() {
            if i < words.len() - 1 || i >= words.len() {
                state.set_var(*name, *words.get(i).unwrap_or(&""));
            } else {
                // Last variable gets remaining words
                let remaining: Vec<&str> = words[i..].to_vec();
                state.set_var(*name, &remaining.join(" "));
            }
        }
    }

    Ok(0)
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Process escape sequences in a string
fn process_escape_sequences(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('a') => result.push('\x07'),
                Some('b') => result.push('\x08'),
                Some('c') => result.push('\x00'), // Stop output marker
                Some('e') => result.push('\x1b'),
                Some('f') => result.push('\x0c'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('v') => result.push('\x0b'),
                Some('0') => {
                    // Octal
                    let mut octal = String::new();
                    for _ in 0..3 {
                        if let Some(&c) = chars.peek() {
                            if c.is_digit(8) {
                                octal.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                    }
                    if let Ok(n) = u8::from_str_radix(&octal, 8) {
                        result.push(n as char);
                    }
                }
                Some('x') => {
                    // Hexadecimal
                    let mut hex = String::new();
                    for _ in 0..2 {
                        if let Some(&c) = chars.peek() {
                            if c.is_ascii_hexdigit() {
                                hex.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                    }
                    if let Ok(n) = u8::from_str_radix(&hex, 16) {
                        result.push(n as char);
                    }
                }
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Format a string using printf-style formatting
fn format_string(format: &str, args: &[&str]) -> Result<String, String> {
    let mut result = String::new();
    let mut chars = format.chars().peekable();
    let mut arg_idx = 0;

    while let Some(c) = chars.next() {
        if c == '\\' {
            // Escape sequence
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('"') => result.push('"'),
                Some('\'') => result.push('\''),
                Some(c) => {
                    result.push('\\');
                    result.push(c);
                }
                None => result.push('\\'),
            }
        } else if c == '%' {
            if chars.peek() == Some(&'%') {
                chars.next();
                result.push('%');
                continue;
            }

            // Parse format specifier
            let mut width = String::new();
            let mut precision = String::new();
            let mut in_precision = false;
            let mut left_align = false;
            let mut zero_pad = false;

            // Flags
            loop {
                match chars.peek() {
                    Some(&'-') => {
                        left_align = true;
                        chars.next();
                    }
                    Some(&'0') if !in_precision && width.is_empty() => {
                        zero_pad = true;
                        chars.next();
                    }
                    Some(&'.') => {
                        in_precision = true;
                        chars.next();
                    }
                    Some(&c) if c.is_ascii_digit() => {
                        if in_precision {
                            precision.push(chars.next().unwrap());
                        } else {
                            width.push(chars.next().unwrap());
                        }
                    }
                    _ => break,
                }
            }

            let arg = args.get(arg_idx).copied().unwrap_or("");
            arg_idx += 1;

            let width: usize = width.parse().unwrap_or(0);
            let _precision: usize = precision.parse().unwrap_or(6);

            match chars.next() {
                Some('s') => {
                    if left_align {
                        result.push_str(&format!("{:<width$}", arg, width = width));
                    } else {
                        result.push_str(&format!("{:>width$}", arg, width = width));
                    }
                }
                Some('d') | Some('i') => {
                    let n: i64 = arg.parse().unwrap_or(0);
                    if left_align {
                        result.push_str(&format!("{:<width$}", n, width = width));
                    } else if zero_pad {
                        result.push_str(&format!("{:0>width$}", n, width = width));
                    } else {
                        result.push_str(&format!("{:>width$}", n, width = width));
                    }
                }
                Some('u') => {
                    let n: u64 = arg.parse().unwrap_or(0);
                    result.push_str(&format!("{:width$}", n, width = width));
                }
                Some('o') => {
                    let n: u64 = arg.parse().unwrap_or(0);
                    result.push_str(&format!("{:o}", n));
                }
                Some('x') => {
                    let n: u64 = arg.parse().unwrap_or(0);
                    result.push_str(&format!("{:x}", n));
                }
                Some('X') => {
                    let n: u64 = arg.parse().unwrap_or(0);
                    result.push_str(&format!("{:X}", n));
                }
                Some('c') => {
                    result.push(arg.chars().next().unwrap_or('\0'));
                }
                Some('b') => {
                    // %b - interpret escapes in argument
                    result.push_str(&process_escape_sequences(arg));
                }
                Some('q') => {
                    // %q - quote for shell reuse
                    result.push('\'');
                    for ch in arg.chars() {
                        if ch == '\'' {
                            result.push_str("'\\''");
                        } else {
                            result.push(ch);
                        }
                    }
                    result.push('\'');
                }
                Some(c) => {
                    return Err(format!("printf: `%{}': 无效的格式字符", c));
                }
                None => {
                    return Err("printf: 格式字符串意外结束".to_string());
                }
            }
        } else {
            result.push(c);
        }
    }

    Ok(result)
}
