//! Navigation Builtin Commands
//!
//! Commands: cd, pwd, dirs, pushd, popd

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::ShellState;
use std::fs;

/// Register navigation builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register("cd", BuiltinDesc::new(
        builtin_cd,
        "改变 shell 工作目录",
        "改变当前目录至 DIR。默认的 DIR 目录是 HOME 环境变量的值。\n\
         \n\
         变量 CDPATH 定义了搜索包含 DIR 的目录的搜索路径。\n\
         CDPATH 中用冒号分隔的备选目录名称。空目录名表示当前目录。\n\
         如果 DIR 以 / 开头，则不使用 CDPATH。\n\
         \n\
         选项:\n\
           -L    强制跟随符号链接: 在处理 .. 之后解析 DIR 中的符号链接\n\
           -P    使用物理目录结构而不跟随符号链接: 在处理 .. 之前解析 DIR 中的符号链接\n\
           -      等同于 cd \"$OLDPWD\"\n\
         \n\
         默认情况下跟随符号链接，就像指定了 -L 一样。\n\
         如果成功更改了目录则返回 0，否则返回非零值。",
        "cd [-L|[-P [-e]] [-@]] [目录]",
        true,
    ));

    registry.register("pwd", BuiltinDesc::new(
        builtin_pwd,
        "打印当前/工作目录的名称",
        "打印当前工作目录的绝对路径名。\n\
         \n\
         选项:\n\
           -L    打印 $PWD 的值，即使它包含符号链接\n\
           -P    打印物理目录，不带符号链接\n\
         \n\
         默认情况下，`pwd' 的行为就像指定了 `-L'。\n\
         如果成功打印了当前目录则返回 0，否则返回非零值。",
        "pwd [-LP]",
        true,
    ));

    registry.register("pushd", BuiltinDesc::new(
        builtin_pushd,
        "向目录栈添加目录",
        "将当前目录保存到目录栈顶部，然后切换到 DIR。\n\
         如果没有参数，pushd 将交换栈顶的两个目录。\n\
         \n\
         选项:\n\
           -n    不改变目录的情况下操作目录栈\n\
         \n\
         参数:\n\
           +N    将第 N 个目录（从 dirs 打印的列表左边数起，从零开始）旋转到栈顶\n\
           -N    将第 N 个目录（从 dirs 打印的列表右边数起，从零开始）旋转到栈顶\n\
           dir   将 DIR 添加到目录栈顶部\n\
         \n\
         如果成功更改了目录则返回 0，否则返回非零值。",
        "pushd [-n] [+N | -N | 目录]",
        true,
    ));

    registry.register("popd", BuiltinDesc::new(
        builtin_popd,
        "从目录栈移除目录",
        "从目录栈移除最顶部的目录，并切换到新的栈顶目录。\n\
         \n\
         选项:\n\
           -n    不改变目录的情况下从栈中移除目录\n\
         \n\
         参数:\n\
           +N    移除第 N 个目录（从 dirs 打印的列表左边数起，从零开始）\n\
           -N    移除第 N 个目录（从 dirs 打印的列表右边数起，从零开始）\n\
         \n\
         如果成功更改了目录则返回 0，否则返回非零值。",
        "popd [-n] [+N | -N]",
        true,
    ));

    registry.register("dirs", BuiltinDesc::new(
        builtin_dirs,
        "显示目录栈",
        "显示当前记忆的目录列表。目录是通过 pushd 命令添加到列表中的。\n\
         \n\
         选项:\n\
           -c    清除目录栈，删除所有条目\n\
           -l    使用完整路径名显示目录（不使用 ~ 替换 $HOME）\n\
           -p    每行一个目录打印目录栈\n\
           -v    每行一个目录打印目录栈，并加上栈位置前缀\n\
         \n\
         参数:\n\
           +N    显示第 N 个目录（从 dirs 无选项时打印的列表左边数起，从零开始）\n\
           -N    显示第 N 个目录（从 dirs 无选项时打印的列表右边数起，从零开始）\n\
         \n\
         如果成功则返回 0，除非提供了无效选项或发生错误。",
        "dirs [-clpv] [+N] [-N]",
        true,
    ));
}

/// cd builtin - change directory
fn builtin_cd(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut follow_symlinks = true;
    let mut target: Option<&str> = None;

    // Parse arguments
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match *arg {
            "-L" => follow_symlinks = true,
            "-P" => follow_symlinks = false,
            "-" => {
                // cd to OLDPWD
                target = state.get_var("OLDPWD");
                if target.is_none() {
                    return Err("cd: OLDPWD 未设置".to_string());
                }
            }
            "--" => {
                // Next arg is the directory
                target = iter.next().copied();
                break;
            }
            arg if arg.starts_with('-') => {
                return Err(format!("cd: {}: 无效选项", arg));
            }
            _ => {
                target = Some(*arg);
            }
        }
    }

    // Default to HOME if no target specified
    let home_dir = state.get_var("HOME").unwrap_or("/root").to_string();
    let target = match target {
        Some(t) => t,
        None => &home_dir,
    };

    // Resolve the path
    let resolved = state.resolve_path(target);

    // Check if it's a valid directory
    match fs::metadata(&resolved) {
        Ok(meta) if meta.is_dir() => {
            // If following symlinks, canonicalize
            let final_path = if follow_symlinks {
                fs::canonicalize(&resolved).unwrap_or(resolved)
            } else {
                resolved
            };
            
            // Check if cd - (to old directory)
            let oldpwd = state.get_var("OLDPWD").unwrap_or("").to_string();
            let was_cd_minus = target == oldpwd;
            
            state.set_cwd(&final_path);
            
            // If cd -, print the new directory
            if was_cd_minus {
                println!("{}", final_path.display());
            }
            
            Ok(0)
        }
        Ok(_) => Err(format!("cd: {}: 不是目录", target)),
        Err(e) => Err(format!("cd: {}: {}", target, e)),
    }
}

/// pwd builtin - print working directory
fn builtin_pwd(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut physical = false;

    for arg in args {
        match *arg {
            "-L" => physical = false,
            "-P" => physical = true,
            arg if arg.starts_with('-') => {
                return Err(format!("pwd: {}: 无效选项", arg));
            }
            _ => {}
        }
    }

    let path = if physical {
        // Get physical path without symlinks
        fs::canonicalize(state.cwd())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| state.cwd_str().to_string())
    } else {
        state.cwd_str().to_string()
    };

    println!("{}", path);
    Ok(0)
}

/// pushd builtin - push directory onto stack
fn builtin_pushd(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut no_change = false;
    let mut target: Option<&str> = None;

    for arg in args {
        match *arg {
            "-n" => no_change = true,
            arg if arg.starts_with('+') || arg.starts_with('-') => {
                // Numeric rotation
                if let Ok(n) = arg[1..].parse::<i32>() {
                    let n = if arg.starts_with('-') { -n } else { n };
                    if let Some(dir) = state.rotate_dir_stack(n) {
                        if !no_change {
                            state.set_cwd(&dir);
                        }
                        print_dir_stack(state, false, false);
                        return Ok(0);
                    } else {
                        return Err(format!("pushd: {}: 目录栈索引越界", arg));
                    }
                }
                target = Some(arg);
            }
            _ => target = Some(arg),
        }
    }

    if let Some(dir) = target {
        // Push current directory and switch to new one
        let resolved = state.resolve_path(dir);
        match fs::metadata(&resolved) {
            Ok(meta) if meta.is_dir() => {
                let old_cwd = state.cwd().to_path_buf();
                if !no_change {
                    state.set_cwd(&resolved);
                }
                state.push_dir(old_cwd);
                print_dir_stack(state, false, false);
                Ok(0)
            }
            Ok(_) => Err(format!("pushd: {}: 不是目录", dir)),
            Err(e) => Err(format!("pushd: {}: {}", dir, e)),
        }
    } else {
        // No argument: swap top two directories
        if let Some(top) = state.pop_dir() {
            let current = state.cwd().to_path_buf();
            if !no_change {
                state.set_cwd(&top);
            }
            state.push_dir(current);
            print_dir_stack(state, false, false);
            Ok(0)
        } else {
            Err("pushd: 目录栈中没有其他目录".to_string())
        }
    }
}

/// popd builtin - pop directory from stack
fn builtin_popd(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut no_change = false;

    for arg in args {
        match *arg {
            "-n" => no_change = true,
            arg if arg.starts_with('+') || arg.starts_with('-') => {
                // Numeric removal - simplified, just handle +0/-0
                return Err(format!("popd: {}: 目录栈索引越界", arg));
            }
            arg => return Err(format!("popd: {}: 无效参数", arg)),
        }
    }

    if let Some(dir) = state.pop_dir() {
        if !no_change {
            state.set_cwd(&dir);
        }
        print_dir_stack(state, false, false);
        Ok(0)
    } else {
        Err("popd: 目录栈为空".to_string())
    }
}

/// dirs builtin - display directory stack
fn builtin_dirs(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut clear = false;
    let mut long_format = false;
    let mut per_line = false;
    let mut with_index = false;

    for arg in args {
        match *arg {
            "-c" => clear = true,
            "-l" => long_format = true,
            "-p" => per_line = true,
            "-v" => {
                per_line = true;
                with_index = true;
            }
            arg if arg.starts_with('+') || arg.starts_with('-') => {
                // Show specific entry
                if let Ok(n) = arg[1..].parse::<i32>() {
                    let n = if arg.starts_with('-') { -n } else { n };
                    let stack = state.dir_stack();
                    let idx = if n >= 0 {
                        n as usize
                    } else {
                        stack.len().saturating_sub((-n) as usize)
                    };
                    if idx < stack.len() {
                        println!("{}", format_dir(stack[idx], state, long_format));
                        return Ok(0);
                    } else {
                        return Err(format!("dirs: {}: 目录栈索引越界", arg));
                    }
                }
                return Err(format!("dirs: {}: 无效参数", arg));
            }
            arg if arg.starts_with('-') => {
                return Err(format!("dirs: {}: 无效选项", arg));
            }
            _ => {}
        }
    }

    if clear {
        while state.pop_dir().is_some() {}
        return Ok(0);
    }

    print_dir_stack_opts(state, long_format, per_line, with_index);
    Ok(0)
}

/// Helper: print directory stack
fn print_dir_stack(state: &ShellState, long_format: bool, per_line: bool) {
    print_dir_stack_opts(state, long_format, per_line, false);
}

fn print_dir_stack_opts(state: &ShellState, long_format: bool, per_line: bool, with_index: bool) {
    let stack = state.dir_stack();
    
    if with_index {
        for (i, dir) in stack.iter().enumerate() {
            println!("{:2}  {}", i, format_dir(dir, state, long_format));
        }
    } else if per_line {
        for dir in &stack {
            println!("{}", format_dir(dir, state, long_format));
        }
    } else {
        let formatted: Vec<String> = stack
            .iter()
            .map(|d| format_dir(d, state, long_format))
            .collect();
        println!("{}", formatted.join(" "));
    }
}

/// Helper: format directory for display
fn format_dir(dir: &std::path::Path, state: &ShellState, long_format: bool) -> String {
    let path_str = dir.to_str().unwrap_or("?");
    
    if long_format {
        path_str.to_string()
    } else {
        // Replace HOME with ~
        if let Some(home) = state.get_var("HOME") {
            if path_str == home {
                return "~".to_string();
            }
            if let Some(rest) = path_str.strip_prefix(home) {
                if rest.starts_with('/') {
                    return format!("~{}", rest);
                }
            }
        }
        path_str.to_string()
    }
}
