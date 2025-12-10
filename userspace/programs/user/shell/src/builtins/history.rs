//! History Builtin Commands
//!
//! Commands: history, fc

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::ShellState;
use std::io::{self, Write};

/// Register history builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register(
        "history",
        BuiltinDesc::new(
            builtin_history,
            "显示或操作历史记录列表",
            "显示带行号的历史记录列表，在每个修改过的条目前加上 `*'。\n\
         参数 N 限制只列出最后 N 个条目。\n\
         \n\
         选项:\n\
           -c    删除所有历史记录条目\n\
           -d 偏移量  删除位于偏移位置 OFFSET 的历史记录条目。\n\
                 负偏移量从历史记录末尾计数\n\
           -a    将当前会话的历史记录行追加到历史文件\n\
           -n    从历史文件中读取尚未读取的历史行\n\
           -r    读取历史文件并将其内容追加到历史列表\n\
           -w    将当前历史写入历史文件\n\
           -p    对每个 ARG 进行历史扩展并显示结果，但不存储到历史列表\n\
           -s    将 ARG 作为单个条目追加到历史列表\n\
         \n\
         如果给出了 FILENAME，则用作历史文件。否则，\n\
         如果 $HISTFILE 有值则使用它，否则使用 ~/.bash_history。\n\
         \n\
         如果没有遇到错误则返回成功。",
            "history [-c] [-d 偏移量] [n] 或 history -anrw [文件名] 或 history -ps 参数 [参数 ...]",
            true,
        ),
    );

    registry.register(
        "fc",
        BuiltinDesc::new(
            builtin_fc,
            "显示或执行历史列表中的命令",
            "fc 用于以交互方式编辑和重新执行来自历史列表的命令。\n\
         \n\
         FIRST 和 LAST 可以是数字来指定范围，或者 FIRST 可以是字符串\n\
         表示以该字符串开头的最近命令。\n\
         \n\
           -e ENAME  选择用于编辑的编辑器。默认为 FCEDIT，然后 EDITOR，\n\
                     然后 vi\n\
           -l        列出行而非编辑\n\
           -n        列出时省略行号\n\
           -r        逆序列出或执行\n\
         \n\
         使用 `fc -s [pat=rep ...] [命令]' 格式，\n\
         在应用旧的=新的替换后重新执行命令。\n\
         \n\
         有用的别名是 r='fc -s'，这样输入 `r cc' 就能运行最后一个\n\
         以 `cc' 开头的命令，输入 `r' 重新执行上一条命令。\n\
         \n\
         如果历史扩展和执行成功则返回 0，否则返回非零值。",
            "fc [-e 编辑器名] [-lnr] [起始] [终止] 或 fc -s [模式=替换串] [命令]",
            true,
        ),
    );
}

/// history builtin - display or manipulate history
fn builtin_history(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut clear = false;
    let mut delete_offset: Option<i32> = None;
    let mut append_file = false;
    let mut read_new = false;
    let mut read_file = false;
    let mut write_file = false;
    let mut expand_args = false;
    let mut store_args = false;
    let mut count: Option<usize> = None;
    let mut extra_args: Vec<&str> = Vec::new();

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match *arg {
            "-c" => clear = true,
            "-d" => {
                if let Some(offset) = iter.next() {
                    delete_offset = Some(
                        offset
                            .parse()
                            .map_err(|_| format!("history: {}: 需要数字参数", offset))?,
                    );
                } else {
                    return Err("history: -d: 需要选项参数".to_string());
                }
            }
            "-a" => append_file = true,
            "-n" => read_new = true,
            "-r" => read_file = true,
            "-w" => write_file = true,
            "-p" => expand_args = true,
            "-s" => store_args = true,
            arg if arg.starts_with('-') && arg.len() > 1 => {
                return Err(format!("history: {}: 无效选项", arg));
            }
            arg => {
                // Could be a count or filename
                if let Ok(n) = arg.parse::<usize>() {
                    count = Some(n);
                } else {
                    extra_args.push(arg);
                }
            }
        }
    }

    // Handle clear
    if clear {
        state.clear_history();
        return Ok(0);
    }

    // Handle delete
    if let Some(offset) = delete_offset {
        match state.delete_history_entry(offset) {
            Ok(()) => return Ok(0),
            Err(e) => return Err(format!("history: {}", e)),
        }
    }

    // Handle file operations
    let histfile = extra_args
        .first()
        .copied()
        .or_else(|| state.get_var("HISTFILE"))
        .unwrap_or("~/.shell_history");
    let histfile = state.resolve_path(histfile);

    if append_file {
        return state
            .append_history_to_file(&histfile)
            .map(|_| 0)
            .map_err(|e| format!("history: {}", e));
    }

    if read_file {
        return state
            .read_history_from_file(&histfile)
            .map(|_| 0)
            .map_err(|e| format!("history: {}", e));
    }

    if read_new {
        return state
            .read_new_history_from_file(&histfile)
            .map(|_| 0)
            .map_err(|e| format!("history: {}", e));
    }

    if write_file {
        return state
            .write_history_to_file(&histfile)
            .map(|_| 0)
            .map_err(|e| format!("history: {}", e));
    }

    // Handle -p (expand and print)
    if expand_args {
        for arg in extra_args {
            let expanded = state.expand_history(arg);
            println!("{}", expanded);
        }
        return Ok(0);
    }

    // Handle -s (store as single entry)
    if store_args {
        let command = extra_args.join(" ");
        state.add_history(&command);
        return Ok(0);
    }

    // Display history
    let history = state.get_history();
    let start = if let Some(n) = count {
        history.len().saturating_sub(n)
    } else {
        0
    };

    for (i, entry) in history.iter().enumerate().skip(start) {
        println!("{:5}  {}", i + 1, entry);
    }

    Ok(0)
}

/// fc builtin - fix command (display or re-execute history)
fn builtin_fc(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut list_mode = false;
    let mut no_numbers = false;
    let mut reverse = false;
    let mut editor: Option<&str> = None;
    let mut substitute_mode = false;
    let mut range_args: Vec<&str> = Vec::new();

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match *arg {
            "-l" => list_mode = true,
            "-n" => no_numbers = true,
            "-r" => reverse = true,
            "-s" => substitute_mode = true,
            "-e" => {
                if let Some(ed) = iter.next() {
                    editor = Some(*ed);
                } else {
                    return Err("fc: -e: 需要选项参数".to_string());
                }
            }
            arg if arg.starts_with('-') && arg.len() > 1 => {
                return Err(format!("fc: {}: 无效选项", arg));
            }
            _ => range_args.push(*arg),
        }
    }

    // Substitute mode: fc -s [old=new] [command]
    if substitute_mode {
        let (substitutions, cmd_pattern) = parse_fc_substitutions(&range_args);

        // Find matching command
        let history = state.get_history();
        let cmd = if let Some(pattern) = cmd_pattern {
            history
                .iter()
                .rev()
                .find(|h| h.starts_with(pattern))
                .map(|s| s.clone())
        } else {
            history.last().cloned()
        };

        let Some(mut cmd) = cmd else {
            return Err("fc: 没有找到匹配的命令".to_string());
        };

        // Apply substitutions
        for (old, new) in substitutions {
            cmd = cmd.replace(&old, &new);
        }

        println!("{}", cmd);
        // Execute the command - this needs to be handled by the main shell
        return Err(format!("FC_EXEC:{}", cmd));
    }

    let history = state.get_history();
    if history.is_empty() {
        return Err("fc: 历史记录为空".to_string());
    }

    // Parse range
    let (first, last) = parse_fc_range(&range_args, history.len());

    // List mode
    if list_mode {
        let range: Box<dyn Iterator<Item = usize>> = if reverse {
            Box::new((first..=last).rev())
        } else {
            Box::new(first..=last)
        };

        for i in range {
            if i > 0 && i <= history.len() {
                if no_numbers {
                    println!("{}", history[i - 1]);
                } else {
                    println!("{:5}  {}", i, history[i - 1]);
                }
            }
        }
        return Ok(0);
    }

    // Edit mode - get editor
    let _editor = editor
        .or_else(|| state.get_var("FCEDIT"))
        .or_else(|| state.get_var("EDITOR"))
        .unwrap_or("vi");

    // Collect commands to edit
    let mut commands = Vec::new();
    let range: Box<dyn Iterator<Item = usize>> = if reverse {
        Box::new((first..=last).rev())
    } else {
        Box::new(first..=last)
    };

    for i in range {
        if i > 0 && i <= history.len() {
            commands.push(history[i - 1].clone());
        }
    }

    if commands.is_empty() {
        return Err("fc: 没有找到匹配的命令".to_string());
    }

    // For now, just execute the last command without editing
    // Full implementation would open editor
    let cmd = commands.last().unwrap();
    println!("{}", cmd);
    Err(format!("FC_EXEC:{}", cmd))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Parse fc substitutions (old=new pairs)
fn parse_fc_substitutions<'a>(args: &[&'a str]) -> (Vec<(String, String)>, Option<&'a str>) {
    let mut substitutions = Vec::new();
    let mut cmd_pattern = None;

    for arg in args {
        if let Some((old, new)) = arg.split_once('=') {
            substitutions.push((old.to_string(), new.to_string()));
        } else {
            cmd_pattern = Some(*arg);
        }
    }

    (substitutions, cmd_pattern)
}

/// Parse fc range arguments
fn parse_fc_range(args: &[&str], history_len: usize) -> (usize, usize) {
    match args.len() {
        0 => {
            // Default: last 16 commands for listing, last command for editing
            let last = history_len;
            let first = last.saturating_sub(15);
            (first.max(1), last)
        }
        1 => {
            // Single argument - could be number or string
            let arg = args[0];
            if let Ok(n) = arg.parse::<i32>() {
                let idx = if n < 0 {
                    (history_len as i32 + n + 1).max(1) as usize
                } else {
                    n.max(1) as usize
                };
                (idx, idx)
            } else {
                // String search - find matching command
                (history_len, history_len)
            }
        }
        _ => {
            // Two arguments - range
            let first = parse_fc_index(args[0], history_len);
            let last = parse_fc_index(args[1], history_len);
            (first.min(last), first.max(last))
        }
    }
}

/// Parse a single fc index
fn parse_fc_index(arg: &str, history_len: usize) -> usize {
    if let Ok(n) = arg.parse::<i32>() {
        if n < 0 {
            (history_len as i32 + n + 1).max(1) as usize
        } else {
            n.max(1) as usize
        }
    } else {
        // String search would find the index
        history_len
    }
}
