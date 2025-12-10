//! Information Builtin Commands
//!
//! Commands: help, type, hash, enable

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::ShellState;
use std::fs;
use std::path::Path;

/// Search paths for external commands
const SEARCH_PATHS: &[&str] = &["/bin", "/sbin", "/usr/bin", "/usr/sbin"];

/// Register info builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register(
        "help",
        BuiltinDesc::new(
            builtin_help,
            "显示内建命令的帮助信息",
            "显示有关内建命令的帮助信息。\n\
         \n\
         如果指定了 PATTERN，则给出所有匹配 PATTERN 的命令的详细帮助，\n\
         否则打印所有内建命令的列表。\n\
         \n\
         选项:\n\
           -d    输出每个主题的简短描述\n\
           -m    以类似 man 手册格式显示用法\n\
           -s    只输出每个匹配 PATTERN 的主题的简短用法\n\
         \n\
         参数:\n\
           PATTERN  指定要显示帮助的模式\n\
         \n\
         退出状态:\n\
         返回成功，除非 PATTERN 未找到或给出无效选项。",
            "help [-dms] [模式 ...]",
            false, // Cannot be disabled
        ),
    );

    registry.register(
        "type",
        BuiltinDesc::new(
            builtin_type,
            "显示命令类型的信息",
            "对于每个 NAME，指示如果作为命令名，它将如何被解释。\n\
         \n\
         选项:\n\
           -a    显示所有包含名为 NAME 的可执行文件的位置；\n\
                 包括别名、内建命令和函数，但仅当没有使用 `-p' 选项时\n\
           -f    抑制 shell 函数查找\n\
           -P    为每个 NAME 强制搜索 PATH，即使 type -t name\n\
                 返回的不是 `file'\n\
           -p    如果 `type -t NAME' 返回 `file'，则返回\n\
                 将被执行的文件的名称\n\
           -t    输出单词: `alias', `keyword', `function', `builtin',\n\
                 或 `file'，分别代表 NAME 是别名、shell 保留字、\n\
                 shell 函数、shell 内建，或磁盘上的文件\n\
         \n\
         参数:\n\
           NAME  要解释的命令名\n\
         \n\
         如果所有 NAME 都被找到则返回成功，否则返回失败。",
            "type [-afptP] 名称 [名称 ...]",
            true,
        ),
    );

    registry.register(
        "hash",
        BuiltinDesc::new(
            builtin_hash,
            "记住或显示程序位置",
            "确定并记住每个 NAME 的完整路径名。如果不带参数，\n\
         则打印有关已记住命令的信息。\n\
         \n\
         选项:\n\
           -d    忘记每个 NAME 记住的位置\n\
           -l    以可重用作输入的格式显示\n\
           -p 路径名  将 NAME 的路径用作 PATHNAME\n\
           -r    忘记所有记住的位置\n\
           -t    打印每个 NAME 对应的已记住的完整路径名\n\
         \n\
         参数:\n\
           NAME  每个 NAME 被搜索于 $PATH 并添加到已记住命令的列表\n\
         \n\
         如果每个 NAME 都被找到则返回成功，否则返回失败。",
            "hash [-lr] [-p 路径名] [-dt] [名称 ...]",
            true,
        ),
    );

    registry.register(
        "enable",
        BuiltinDesc::new(
            builtin_enable,
            "启用和禁用内建命令",
            "启用和禁用内建 shell 命令。禁用某个内建命令后，\n\
         磁盘上同名的命令可以在不使用完整路径名的情况下执行，\n\
         即使 shell 通常在搜索磁盘上的命令之前先搜索内建命令。\n\
         \n\
         选项:\n\
           -a    打印所有内建命令列表，标明每个是否被启用\n\
           -n    禁用每个 NAME 或显示被禁用的内建命令列表\n\
           -p    以可重用作输入的格式打印列表\n\
           -s    仅打印 POSIX `special' 内建命令\n\
         \n\
         不使用选项，每个 NAME 被启用。\n\
         \n\
         如果 NAME 是内建命令或给出了无效选项则返回成功。",
            "enable [-a] [-dnps] [-f 文件名] [名称 ...]",
            false, // Cannot be disabled
        ),
    );
}

/// help builtin - display help information
/// Note: This is called from main.rs with access to the registry
pub fn builtin_help(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    // This function needs registry access, which is provided through a different mechanism
    // The actual implementation is in main.rs where the registry is available

    let mut short_desc = false;
    let mut short_usage = false;
    let mut patterns: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-d" => short_desc = true,
            "-s" => short_usage = true,
            "-m" => {} // Man format - treat as normal for now
            arg if arg.starts_with('-') => {
                return Err(format!("help: {}: 无效选项", arg));
            }
            _ => patterns.push(*arg),
        }
    }

    // Store parsed options for use by the caller
    // The actual help display is handled in main.rs
    if patterns.is_empty() {
        println!("NexaOS Shell, 版本 0.3.0");
        println!("这些 shell 命令是内部定义的。输入 `help' 以获取本列表。");
        println!("输入 `help 名称' 以得到有关命令 `名称' 的更多信息。");
        println!("使用 `info bash' 来获得关于 shell 的更多一般性信息。");
        println!();
        println!("内建命令:");
        println!("  导航: cd, pwd, pushd, popd, dirs");
        println!("  变量: export, unset, set, declare, typeset, readonly, local, let, shift");
        println!("  别名: alias, unalias");
        println!("  流程控制: exit, return, break, continue, test, [, true, false, :, logout");
        println!("  信息: help, type, hash, enable, caller, variables");
        println!("  实用: echo, printf, source, ., eval, exec, command, builtin, read");
        println!("  作业控制: jobs, bg, fg, disown, suspend, kill, wait, coproc");
        println!("  历史: history, fc");
        println!("  配置: shopt, bind, ulimit, umask");
        println!("  陷阱: trap, times, time");
        println!("  补全: compgen, complete, compopt");
        println!("  其他: getopts, mapfile, readarray");
        println!();
        println!("控制流结构 (需要在脚本或命令行中使用):");
        println!("  条件: if/then/elif/else/fi, case/esac, [[...]]");
        println!("  循环: for, while, until, select");
        println!("  分组: {{...}}, ((...)), function");
        println!();
        println!("使用 `help 名称' 获取特定命令的详细帮助。");
        Ok(0)
    } else {
        // For specific help, we need the registry
        // Return a special code to indicate main.rs should handle this
        Err(format!("HELP_PATTERN:{}", patterns.join(",")))
    }
}

/// type builtin - display command type
fn builtin_type(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    // List of known builtin names
    const BUILTINS: &[&str] = &[
        // Navigation
        "cd",
        "pwd",
        "pushd",
        "popd",
        "dirs",
        // Variables
        "export",
        "unset",
        "set",
        "declare",
        "typeset",
        "readonly",
        "local",
        "let",
        "shift",
        // Aliases
        "alias",
        "unalias",
        // Flow control
        "exit",
        "return",
        "break",
        "continue",
        "test",
        "[",
        "true",
        "false",
        ":",
        "logout",
        // Information
        "help",
        "type",
        "hash",
        "enable",
        "caller",
        "variables",
        // Utility
        "echo",
        "printf",
        "source",
        ".",
        "eval",
        "exec",
        "command",
        "builtin",
        "read",
        // Job control
        "jobs",
        "bg",
        "fg",
        "disown",
        "suspend",
        "kill",
        "wait",
        "coproc",
        // History
        "history",
        "fc",
        // Configuration
        "shopt",
        "bind",
        "ulimit",
        "umask",
        // Traps
        "trap",
        "times",
        "time",
        // Completion
        "compgen",
        "complete",
        "compopt",
        // Misc
        "getopts",
        "mapfile",
        "readarray",
    ];

    let mut show_all = false;
    let mut type_only = false;
    let mut show_path = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-a" => show_all = true,
            "-t" => type_only = true,
            "-p" | "-P" => show_path = true,
            "-f" => {} // Suppress function lookup - no functions yet
            arg if arg.starts_with('-') => {
                return Err(format!("type: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    if names.is_empty() {
        return Err("type: 用法: type [-afptP] 名称 [名称 ...]".to_string());
    }

    let mut all_found = true;

    for name in names {
        let mut found = false;

        // Check alias
        if let Some(value) = state.get_alias(name) {
            found = true;
            if type_only {
                println!("alias");
            } else {
                println!("{} 是 `{}' 的别名", name, value);
            }
            if !show_all {
                continue;
            }
        }

        // Check builtin
        if BUILTINS.contains(&name) && !show_path {
            found = true;
            if type_only {
                println!("builtin");
            } else {
                println!("{} 是一个 shell 内建命令", name);
            }
            if !show_all {
                continue;
            }
        }

        // Check hashed
        if let Some(path) = state.get_hashed(name) {
            found = true;
            if type_only {
                println!("file");
            } else if show_path {
                println!("{}", path.display());
            } else {
                println!("{} 已哈希 ({})", name, path.display());
            }
            if !show_all {
                continue;
            }
        }

        // Search PATH
        if let Some(path) = find_in_path(name) {
            found = true;
            if type_only {
                println!("file");
            } else if show_path {
                println!("{}", path.display());
            } else {
                println!("{} 是 {}", name, path.display());
            }
        }

        if !found {
            all_found = false;
            if !type_only {
                eprintln!("type: {}: 未找到", name);
            }
        }
    }

    if all_found {
        Ok(0)
    } else {
        Ok(1)
    }
}

/// hash builtin - manage command hash table
fn builtin_hash(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut delete = false;
    let mut list_format = false;
    let mut clear = false;
    let mut show_only = false;
    let mut pathname: Option<&str> = None;
    let mut names: Vec<&str> = Vec::new();

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match *arg {
            "-d" => delete = true,
            "-l" => list_format = true,
            "-r" => clear = true,
            "-t" => show_only = true,
            "-p" => {
                pathname = iter.next().copied();
                if pathname.is_none() {
                    return Err("hash: -p: 需要选项参数".to_string());
                }
            }
            arg if arg.starts_with('-') => {
                return Err(format!("hash: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    // Clear all hashes
    if clear {
        state.clear_hash();
        return Ok(0);
    }

    // No arguments: show hash table
    if names.is_empty() && pathname.is_none() {
        let hashed = state.list_hashed();
        if hashed.is_empty() {
            println!("hash: 哈希表为空");
        } else {
            if list_format {
                for (name, path) in hashed {
                    println!("builtin hash -p {} {}", path.display(), name);
                }
            } else {
                println!("命中\t命令");
                for (name, path) in hashed {
                    println!("   1\t{}", path.display());
                }
            }
        }
        return Ok(0);
    }

    // Process names
    let mut all_found = true;

    for name in names {
        if delete {
            if !state.unhash(name) {
                eprintln!("hash: {}: 未找到", name);
                all_found = false;
            }
        } else if show_only {
            if let Some(path) = state.get_hashed(name) {
                println!("{}", path.display());
            } else if let Some(path) = find_in_path(name) {
                println!("{}", path.display());
            } else {
                eprintln!("hash: {}: 未找到", name);
                all_found = false;
            }
        } else if let Some(p) = pathname {
            // Set specific path
            state.hash_command(name.to_string(), Path::new(p).to_path_buf());
        } else {
            // Find and hash
            if let Some(path) = find_in_path(name) {
                state.hash_command(name.to_string(), path);
            } else {
                eprintln!("hash: {}: 未找到", name);
                all_found = false;
            }
        }
    }

    if all_found {
        Ok(0)
    } else {
        Ok(1)
    }
}

/// enable builtin - enable/disable builtins
/// Note: Full functionality requires registry access from main.rs
fn builtin_enable(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    // List of known builtin names
    const BUILTINS: &[&str] = &[
        "cd",
        "pwd",
        "pushd",
        "popd",
        "dirs",
        "export",
        "unset",
        "set",
        "declare",
        "typeset",
        "readonly",
        "local",
        "let",
        "shift",
        "alias",
        "unalias",
        "exit",
        "return",
        "break",
        "continue",
        "test",
        "[",
        "true",
        "false",
        ":",
        "logout",
        "help",
        "type",
        "hash",
        "enable",
        "caller",
        "variables",
        "echo",
        "printf",
        "source",
        ".",
        "eval",
        "exec",
        "command",
        "builtin",
        "read",
        "jobs",
        "bg",
        "fg",
        "disown",
        "suspend",
        "kill",
        "wait",
        "coproc",
        "history",
        "fc",
        "shopt",
        "bind",
        "ulimit",
        "umask",
        "trap",
        "times",
        "time",
        "compgen",
        "complete",
        "compopt",
        "getopts",
        "mapfile",
        "readarray",
    ];

    let mut show_all = false;
    let mut disable = false;
    let mut print_format = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-a" => show_all = true,
            "-n" => disable = true,
            "-p" => print_format = true,
            "-s" => {} // Special builtins - all our builtins are special for now
            "-d" | "-f" => {
                // Dynamic loading not supported
                return Err(format!("enable: {}: 不支持此选项", arg));
            }
            arg if arg.starts_with('-') => {
                return Err(format!("enable: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    if names.is_empty() {
        // Show builtins (simplified - all builtins are always enabled)
        for name in BUILTINS {
            if print_format {
                if disable {
                    // No disabled builtins to show
                } else {
                    println!("enable {}", name);
                }
            } else if show_all {
                println!("enable {}", name);
            } else if !disable {
                println!("{}", name);
            }
        }
        Ok(0)
    } else {
        // Enable/disable specific builtins
        // Note: This simplified version doesn't actually disable builtins
        // Full implementation would modify the registry
        for name in names {
            if !BUILTINS.contains(&name) {
                eprintln!("enable: {}: 不是 shell 内建命令", name);
            } else if disable {
                eprintln!("enable: 内建命令禁用功能尚未实现");
            }
        }
        Ok(0)
    }
}

/// Helper: find command in PATH
fn find_in_path(cmd: &str) -> Option<std::path::PathBuf> {
    for dir in SEARCH_PATHS {
        let path = Path::new(dir).join(cmd);
        if fs::metadata(&path).is_ok() {
            return Some(path);
        }
    }
    None
}
