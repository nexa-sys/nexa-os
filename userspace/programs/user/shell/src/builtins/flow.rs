//! Flow Control Builtin Commands
//!
//! Commands: exit, return, break, continue, test/[

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::{FlowControl, ShellState};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Register flow control builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register(
        "exit",
        BuiltinDesc::new(
            builtin_exit,
            "退出 shell",
            "退出 shell。\n\
         \n\
         使用状态 N 退出 shell。如果 N 被省略，则退出状态是\n\
         最后执行的命令的状态。",
            "exit [n]",
            false, // Cannot be disabled
        ),
    );

    registry.register(
        "return",
        BuiltinDesc::new(
            builtin_return,
            "从函数返回",
            "使函数或脚本以指定的返回值退出。\n\
         \n\
         使 shell 函数退出并返回 N 值。如果 N 被省略，返回值是\n\
         函数中最后执行的命令的状态。",
            "return [n]",
            false,
        ),
    );

    registry.register(
        "break",
        BuiltinDesc::new(
            builtin_break,
            "退出 for, while 或 until 循环",
            "退出 FOR、WHILE 或 UNTIL 循环。\n\
         \n\
         退出封闭的 FOR、WHILE 或 UNTIL 循环。如果指定了 N，\n\
         则退出 N 层封闭循环。",
            "break [n]",
            false,
        ),
    );

    registry.register(
        "continue",
        BuiltinDesc::new(
            builtin_continue,
            "继续 for, while 或 until 循环的下一次迭代",
            "继续 FOR、WHILE 或 UNTIL 循环的下一次迭代。\n\
         \n\
         继续封闭的 FOR、WHILE 或 UNTIL 循环的下一次迭代。\n\
         如果指定了 N，则继续第 N 层封闭循环。",
            "continue [n]",
            false,
        ),
    );

    registry.register(
        "test",
        BuiltinDesc::new(
            builtin_test,
            "求条件表达式的值",
            "根据 EXPR 求值并返回成功或失败的退出状态。\n\
         \n\
         表达式可以是一元或二元表达式。一元表达式通常用于检测文件的状态。\n\
         也有字符串运算符和数值比较运算符。\n\
         \n\
         文件运算符:\n\
           -e FILE    如果文件存在则为真\n\
           -f FILE    如果文件存在且为普通文件则为真\n\
           -d FILE    如果文件存在且为目录则为真\n\
           -r FILE    如果文件存在且可读则为真\n\
           -w FILE    如果文件存在且可写则为真\n\
           -x FILE    如果文件存在且可执行则为真\n\
           -s FILE    如果文件存在且大小大于零则为真\n\
           -L FILE    如果文件存在且为符号链接则为真\n\
           -b FILE    如果文件存在且为块设备则为真\n\
           -c FILE    如果文件存在且为字符设备则为真\n\
           -p FILE    如果文件存在且为命名管道则为真\n\
           -S FILE    如果文件存在且为套接字则为真\n\
         \n\
         字符串运算符:\n\
           -z STRING  如果字符串长度为零则为真\n\
           -n STRING  如果字符串长度非零则为真\n\
           STRING     如果字符串非空则为真\n\
           S1 = S2    如果字符串相等则为真\n\
           S1 != S2   如果字符串不相等则为真\n\
           S1 < S2    如果 S1 按字典序排在 S2 之前则为真\n\
           S1 > S2    如果 S1 按字典序排在 S2 之后则为真\n\
         \n\
         数值运算符:\n\
           N1 -eq N2  如果 N1 等于 N2 则为真\n\
           N1 -ne N2  如果 N1 不等于 N2 则为真\n\
           N1 -lt N2  如果 N1 小于 N2 则为真\n\
           N1 -le N2  如果 N1 小于等于 N2 则为真\n\
           N1 -gt N2  如果 N1 大于 N2 则为真\n\
           N1 -ge N2  如果 N1 大于等于 N2 则为真\n\
         \n\
         其他运算符:\n\
           ! EXPR     如果 EXPR 为假则为真\n\
           EXPR1 -a EXPR2    如果 EXPR1 和 EXPR2 都为真则为真\n\
           EXPR1 -o EXPR2    如果 EXPR1 或 EXPR2 为真则为真",
            "test [表达式]",
            true,
        ),
    );

    registry.register(
        "[",
        BuiltinDesc::new(
            builtin_bracket,
            "求条件表达式的值",
            "这是 `test' 内建命令的同义词，但最后一个参数必须是\n\
         `]' 字面量，以匹配开始的 `['。",
            "[ 表达式 ]",
            true,
        ),
    );

    registry.register(
        "true",
        BuiltinDesc::new(
            builtin_true,
            "返回成功的结果",
            "返回成功的退出状态（0）。",
            "true",
            true,
        ),
    );

    registry.register(
        "false",
        BuiltinDesc::new(
            builtin_false,
            "返回失败的结果",
            "返回不成功的退出状态（1）。",
            "false",
            true,
        ),
    );

    registry.register(
        ":",
        BuiltinDesc::new(
            builtin_colon,
            "空命令",
            "无效果；命令什么也不做。\n\
         \n\
         退出状态:\n\
         总是成功。",
            ":",
            false,
        ),
    );
}

/// exit builtin - exit the shell
fn builtin_exit(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let code = args
        .first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(state.last_exit_status);

    std::process::exit(code);
}

/// return builtin - return from function
fn builtin_return(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if state.function_depth == 0 {
        return Err("return: 只能从函数或脚本中调用 `return'".to_string());
    }

    let code = args
        .first()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(state.last_exit_status);

    state.flow_control = Some(FlowControl::Return(code));
    Ok(code)
}

/// break builtin - break out of loops
fn builtin_break(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if state.loop_depth == 0 {
        return Err("break: 只有在 FOR、WHILE 或 UNTIL 循环中才有效".to_string());
    }

    let levels = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);

    if levels == 0 {
        return Err("break: 循环数必须大于 0".to_string());
    }

    state.flow_control = Some(FlowControl::Break(levels.min(state.loop_depth)));
    Ok(0)
}

/// continue builtin - continue loop
fn builtin_continue(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if state.loop_depth == 0 {
        return Err("continue: 只有在 FOR、WHILE 或 UNTIL 循环中才有效".to_string());
    }

    let levels = args
        .first()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(1);

    if levels == 0 {
        return Err("continue: 循环数必须大于 0".to_string());
    }

    state.flow_control = Some(FlowControl::Continue(levels.min(state.loop_depth)));
    Ok(0)
}

/// test builtin - evaluate conditional expression
fn builtin_test(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        return Ok(1); // Empty test is false
    }

    match evaluate_test_expression(args) {
        Ok(result) => Ok(if result { 0 } else { 1 }),
        Err(e) => Err(format!("test: {}", e)),
    }
}

/// [ builtin - same as test but requires ]
fn builtin_bracket(_state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        return Err("[: 缺少 `]'".to_string());
    }

    // Check for closing ]
    if args.last() != Some(&"]") {
        return Err("[: 缺少 `]'".to_string());
    }

    // Remove the closing ]
    let test_args = &args[..args.len() - 1];

    if test_args.is_empty() {
        return Ok(1);
    }

    match evaluate_test_expression(test_args) {
        Ok(result) => Ok(if result { 0 } else { 1 }),
        Err(e) => Err(format!("[: {}", e)),
    }
}

/// true builtin
fn builtin_true(_state: &mut ShellState, _args: &[&str]) -> BuiltinResult {
    Ok(0)
}

/// false builtin
fn builtin_false(_state: &mut ShellState, _args: &[&str]) -> BuiltinResult {
    Ok(1)
}

/// : (colon) builtin - null command
fn builtin_colon(_state: &mut ShellState, _args: &[&str]) -> BuiltinResult {
    Ok(0)
}

// ============================================================================
// Test Expression Evaluation
// ============================================================================

fn evaluate_test_expression(args: &[&str]) -> Result<bool, String> {
    let mut iter = args.iter().peekable();
    evaluate_or_expression(&mut iter)
}

fn evaluate_or_expression(
    iter: &mut std::iter::Peekable<std::slice::Iter<&str>>,
) -> Result<bool, String> {
    let mut result = evaluate_and_expression(iter)?;

    while iter.peek() == Some(&&"-o") {
        iter.next(); // consume -o
        let right = evaluate_and_expression(iter)?;
        result = result || right;
    }

    Ok(result)
}

fn evaluate_and_expression(
    iter: &mut std::iter::Peekable<std::slice::Iter<&str>>,
) -> Result<bool, String> {
    let mut result = evaluate_not_expression(iter)?;

    while iter.peek() == Some(&&"-a") {
        iter.next(); // consume -a
        let right = evaluate_not_expression(iter)?;
        result = result && right;
    }

    Ok(result)
}

fn evaluate_not_expression(
    iter: &mut std::iter::Peekable<std::slice::Iter<&str>>,
) -> Result<bool, String> {
    if iter.peek() == Some(&&"!") {
        iter.next(); // consume !
        Ok(!evaluate_primary_expression(iter)?)
    } else {
        evaluate_primary_expression(iter)
    }
}

fn evaluate_primary_expression(
    iter: &mut std::iter::Peekable<std::slice::Iter<&str>>,
) -> Result<bool, String> {
    let first = match iter.next() {
        Some(s) => *s,
        None => return Ok(false),
    };

    // Check for parenthesized expression
    if first == "(" {
        let result = evaluate_or_expression(iter)?;
        if iter.next() != Some(&")") {
            return Err("缺少 `)'".to_string());
        }
        return Ok(result);
    }

    // Unary file tests
    if first.starts_with('-') && first.len() == 2 {
        let op = first.chars().nth(1).unwrap();
        let operand = iter
            .next()
            .ok_or_else(|| format!("{}: 期望一元表达式", first))?;

        return match op {
            'e' => Ok(Path::new(operand).exists()),
            'f' => Ok(fs::metadata(operand).map(|m| m.is_file()).unwrap_or(false)),
            'd' => Ok(fs::metadata(operand).map(|m| m.is_dir()).unwrap_or(false)),
            'r' => Ok(is_readable(operand)),
            'w' => Ok(is_writable(operand)),
            'x' => Ok(is_executable(operand)),
            's' => Ok(fs::metadata(operand).map(|m| m.len() > 0).unwrap_or(false)),
            'L' | 'h' => Ok(fs::symlink_metadata(operand)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false)),
            'b' => Ok(is_block_device(operand)),
            'c' => Ok(is_char_device(operand)),
            'p' => Ok(is_fifo(operand)),
            'S' => Ok(is_socket(operand)),
            'z' => Ok(operand.is_empty()),
            'n' => Ok(!operand.is_empty()),
            _ => Err(format!("{}: 未知的一元运算符", first)),
        };
    }

    // Check for binary operators
    if let Some(&&op) = iter.peek() {
        match op {
            "=" | "==" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                return Ok(first == *right);
            }
            "!=" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                return Ok(first != *right);
            }
            "<" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                return Ok(first < *right);
            }
            ">" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                return Ok(first > *right);
            }
            "-eq" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                let n1: i64 = first
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", first))?;
                let n2: i64 = right
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", right))?;
                return Ok(n1 == n2);
            }
            "-ne" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                let n1: i64 = first
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", first))?;
                let n2: i64 = right
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", right))?;
                return Ok(n1 != n2);
            }
            "-lt" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                let n1: i64 = first
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", first))?;
                let n2: i64 = right
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", right))?;
                return Ok(n1 < n2);
            }
            "-le" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                let n1: i64 = first
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", first))?;
                let n2: i64 = right
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", right))?;
                return Ok(n1 <= n2);
            }
            "-gt" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                let n1: i64 = first
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", first))?;
                let n2: i64 = right
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", right))?;
                return Ok(n1 > n2);
            }
            "-ge" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                let n1: i64 = first
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", first))?;
                let n2: i64 = right
                    .parse()
                    .map_err(|_| format!("{}: 需要整数表达式", right))?;
                return Ok(n1 >= n2);
            }
            "-nt" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                return Ok(is_newer_than(first, right));
            }
            "-ot" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                return Ok(is_older_than(first, right));
            }
            "-ef" => {
                iter.next();
                let right = iter.next().ok_or("缺少右操作数")?;
                return Ok(is_same_file(first, right));
            }
            _ => {}
        }
    }

    // Single string: true if non-empty
    Ok(!first.is_empty())
}

// ============================================================================
// File Test Helpers
// ============================================================================

fn is_readable(path: &str) -> bool {
    fs::metadata(path).is_ok()
    // TODO: actually check permissions using nix or libc
}

fn is_writable(path: &str) -> bool {
    fs::metadata(path).is_ok()
    // TODO: actually check permissions
}

fn is_executable(path: &str) -> bool {
    fs::metadata(path)
        .map(|m| m.mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn is_block_device(path: &str) -> bool {
    fs::metadata(path)
        .map(|m| (m.mode() & 0o170000) == 0o60000)
        .unwrap_or(false)
}

fn is_char_device(path: &str) -> bool {
    fs::metadata(path)
        .map(|m| (m.mode() & 0o170000) == 0o20000)
        .unwrap_or(false)
}

fn is_fifo(path: &str) -> bool {
    fs::metadata(path)
        .map(|m| (m.mode() & 0o170000) == 0o10000)
        .unwrap_or(false)
}

fn is_socket(path: &str) -> bool {
    fs::metadata(path)
        .map(|m| (m.mode() & 0o170000) == 0o140000)
        .unwrap_or(false)
}

fn is_newer_than(file1: &str, file2: &str) -> bool {
    let m1 = fs::metadata(file1).and_then(|m| m.modified()).ok();
    let m2 = fs::metadata(file2).and_then(|m| m.modified()).ok();
    match (m1, m2) {
        (Some(t1), Some(t2)) => t1 > t2,
        _ => false,
    }
}

fn is_older_than(file1: &str, file2: &str) -> bool {
    let m1 = fs::metadata(file1).and_then(|m| m.modified()).ok();
    let m2 = fs::metadata(file2).and_then(|m| m.modified()).ok();
    match (m1, m2) {
        (Some(t1), Some(t2)) => t1 < t2,
        _ => false,
    }
}

fn is_same_file(file1: &str, file2: &str) -> bool {
    let m1 = fs::metadata(file1).ok();
    let m2 = fs::metadata(file2).ok();
    match (m1, m2) {
        (Some(s1), Some(s2)) => s1.dev() == s2.dev() && s1.ino() == s2.ino(),
        _ => false,
    }
}
