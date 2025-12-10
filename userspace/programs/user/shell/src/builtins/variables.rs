//! Variable Management Builtin Commands
//!
//! Commands: export, unset, set, declare/typeset, readonly, alias, unalias

use crate::builtins::{BuiltinDesc, BuiltinRegistry, BuiltinResult};
use crate::state::ShellState;

/// Register variable builtins
pub fn register(registry: &mut BuiltinRegistry) {
    registry.register(
        "export",
        BuiltinDesc::new(
            builtin_export,
            "为子进程设置导出属性",
            "标记每个 NAME 以便在后续命令的环境中自动导出。\n\
         如果提供了 VALUE，则在导出前赋值。\n\
         \n\
         选项:\n\
           -f    引用 shell 函数\n\
           -n    移除每个 NAME 的导出属性\n\
           -p    显示所有导出的变量和函数的列表\n\
         \n\
         参数 `--' 禁用进一步的选项处理。\n\
         \n\
         如果 NAME 不是有效的 shell 变量名则返回失败，否则返回成功。",
            "export [-fn] [名称[=值] ...] 或 export -p",
            true,
        ),
    );

    registry.register(
        "unset",
        BuiltinDesc::new(
            builtin_unset,
            "取消变量或函数的定义",
            "对于每个 NAME，移除对应的变量或函数。\n\
         \n\
         选项:\n\
           -f    将每个 NAME 视为 shell 函数\n\
           -v    将每个 NAME 视为 shell 变量\n\
           -n    将每个 NAME 视为名称引用，只取消变量本身而非其引用的变量\n\
         \n\
         如果没有选项，unset 首先尝试取消一个变量，如果失败则再尝试取消函数。\n\
         \n\
         某些变量不能被取消；参见 `readonly'。\n\
         \n\
         如果 NAME 是只读的则返回失败，否则返回成功。",
            "unset [-f] [-v] [-n] [名称 ...]",
            true,
        ),
    );

    registry.register(
        "set",
        BuiltinDesc::new(
            builtin_set,
            "设置或取消 shell 选项和位置参数的值",
            "改变 shell 选项和位置参数的值，或显示 shell 变量的名称和值。\n\
         \n\
         选项:\n\
           -a    标记已修改或创建的变量以便导出\n\
           -b    立即通知作业终止\n\
           -e    如果命令退出状态非零，立即退出\n\
           -f    禁用文件名扩展（通配符）\n\
           -h    在查找命令时记住它们的位置\n\
           -n    读取命令但不执行它们\n\
           -o 选项名  设置对应于选项名的变量:\n\
                 allexport    与 -a 相同\n\
                 errexit      与 -e 相同\n\
                 hashall      与 -h 相同\n\
                 ignoreeof    shell 不会在读取 EOF 时退出\n\
                 noglob       与 -f 相同\n\
                 notify       与 -b 相同\n\
                 nounset      与 -u 相同\n\
                 verbose      与 -v 相同\n\
                 xtrace       与 -x 相同\n\
           -u    将未设置的变量视为扩展时的错误\n\
           -v    读取时打印 shell 输入行\n\
           -x    执行时打印命令及其参数\n\
         \n\
         使用 + 而非 - 会关闭指定选项。还可以使用 -o 和 +o\n\
         没有选项名来显示当前选项设置。\n\
         \n\
         返回成功，除非给出无效选项。",
            "set [-abefhkmnptuvxBCEHPT] [-o 选项名] [--] [-] [参数 ...]",
            true,
        ),
    );

    registry.register(
        "declare",
        BuiltinDesc::new(
            builtin_declare,
            "设置变量的值和属性",
            "声明变量和给它们设置属性。如果没有给出 NAME，\n\
         显示所有变量的属性和值。\n\
         \n\
         选项:\n\
           -a    将 NAME 设为索引数组（如果支持）\n\
           -A    将 NAME 设为关联数组（如果支持）\n\
           -f    限制显示为函数定义和函数名\n\
           -i    使 NAME 具有 `integer' 属性\n\
           -l    在赋值时将 NAME 的值转换为小写\n\
           -n    使 NAME 成为对另一个变量的名称引用\n\
           -r    使 NAME 为只读\n\
           -t    使 NAME 具有 `trace' 属性\n\
           -u    在赋值时将 NAME 的值转换为大写\n\
           -x    标记 NAME 以便导出\n\
         \n\
         使用 `+' 而非 `-' 可关闭给定属性。\n\
         \n\
         返回成功，除非给出无效选项或出现变量赋值错误。",
            "declare [-aAfFgiIlnrtux] [名称[=值] ...]",
            true,
        ),
    );

    // typeset is an alias for declare
    registry.register(
        "typeset",
        BuiltinDesc::new(
            builtin_declare,
            "设置变量的值和属性",
            "与 `declare' 等同。参见 `help declare'。",
            "typeset [-aAfFgiIlnrtux] 名称[=值] ...",
            true,
        ),
    );

    registry.register(
        "readonly",
        BuiltinDesc::new(
            builtin_readonly,
            "标记变量或函数为只读",
            "标记每个 NAME 为只读；NAME 的值不能被后续赋值改变。\n\
         如果提供了 VALUE，则在标记只读前赋值。\n\
         \n\
         选项:\n\
           -a    引用索引数组变量\n\
           -A    引用关联数组变量\n\
           -f    引用 shell 函数\n\
           -p    显示所有只读变量的列表\n\
         \n\
         参数 `--' 禁用进一步的选项处理。\n\
         \n\
         返回成功，除非给出无效选项或 NAME 无效。",
            "readonly [-aAf] [名称[=值] ...] 或 readonly -p",
            true,
        ),
    );

    registry.register(
        "alias",
        BuiltinDesc::new(
            builtin_alias,
            "定义或显示别名",
            "alias 不带参数或带 -p 选项在标准输出上以\n\
         alias NAME=VALUE 的格式打印别名列表，使其可以重用作输入。\n\
         \n\
         否则，定义一个别名，使 VALUE 成为 NAME 的值。\n\
         VALUE 中的尾随空格会使下一个词在扩展别名时也进行别名检查。\n\
         \n\
         选项:\n\
           -p    以可重用格式打印所有定义的别名\n\
         \n\
         如果提供了 NAME 且未提供 VALUE，则打印该别名的值。\n\
         除非给出的 NAME 没有定义别名，否则返回成功。",
            "alias [-p] [名称[=值] ... ]",
            true,
        ),
    );

    registry.register(
        "unalias",
        BuiltinDesc::new(
            builtin_unalias,
            "移除别名定义",
            "从定义的别名列表中移除每个 NAME。\n\
         \n\
         选项:\n\
           -a    移除所有别名定义\n\
         \n\
         返回成功，除非 NAME 未定义为别名。",
            "unalias [-a] 名称 [名称 ...]",
            true,
        ),
    );

    registry.register(
        "local",
        BuiltinDesc::new(
            builtin_local,
            "定义局部变量",
            "创建局部变量。\n\
         \n\
         创建名为 NAME 的局部变量，并赋值为 VALUE。\n\
         OPTION 可以是 `declare' 接受的任何选项。\n\
         \n\
         局部变量只能在函数内使用；使 NAME 仅在该函数及其子函数内可见。\n\
         \n\
         返回成功，除非在函数外使用 local 或给出无效选项。",
            "local [选项] 名称[=值] ...",
            true,
        ),
    );
}

/// export builtin
fn builtin_export(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut show_all = false;
    let mut remove_export = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-p" => show_all = true,
            "-n" => remove_export = true,
            "-f" => {} // Functions not implemented
            "--" => {}
            arg if arg.starts_with('-') => {
                return Err(format!("export: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    if show_all || names.is_empty() {
        // Show exported variables
        let exported = state.exported_vars();
        for (name, value) in exported {
            println!("declare -x {}=\"{}\"", name, value);
        }
        return Ok(0);
    }

    for arg in names {
        if let Some((name, value)) = arg.split_once('=') {
            // export NAME=VALUE
            state.set_var(name, value);
            if !remove_export {
                state.export_var(name);
            }
        } else {
            // export NAME
            if remove_export {
                // -n: remove export flag (not implemented, just ignore)
            } else {
                state.export_var(arg);
            }
        }
    }

    Ok(0)
}

/// unset builtin
fn builtin_unset(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-f" | "-v" | "-n" => {} // All treat as variable for now
            arg if arg.starts_with('-') => {
                return Err(format!("unset: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    let mut success = true;
    for name in names {
        if let Err(e) = state.unset_var(name) {
            eprintln!("unset: {}", e);
            success = false;
        }
    }

    if success {
        Ok(0)
    } else {
        Ok(1)
    }
}

/// set builtin
fn builtin_set(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if args.is_empty() {
        // Show all variables
        let mut vars = state.all_vars();
        vars.sort_by_key(|(name, _)| *name);
        for (name, var) in vars {
            println!("{}={}", name, var.value);
        }
        return Ok(0);
    }

    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        let (enable, flags) = if arg.starts_with('-') && *arg != "--" {
            (true, &arg[1..])
        } else if arg.starts_with('+') {
            (false, &arg[1..])
        } else if *arg == "--" {
            break;
        } else {
            // Positional parameters (not implemented)
            break;
        };

        for flag in flags.chars() {
            match flag {
                'a' => state.options.allexport = enable,
                'b' => state.options.notify = enable,
                'e' => state.options.errexit = enable,
                'f' => state.options.noglob = enable,
                'h' => state.options.hashall = enable,
                'u' => state.options.nounset = enable,
                'v' => state.options.verbose = enable,
                'x' => state.options.xtrace = enable,
                'C' => state.options.noclobber = enable,
                'o' => {
                    if let Some(opt_name) = iter.next() {
                        match *opt_name {
                            "allexport" => state.options.allexport = enable,
                            "errexit" => state.options.errexit = enable,
                            "hashall" => state.options.hashall = enable,
                            "ignoreeof" => state.options.ignoreeof = enable,
                            "noglob" => state.options.noglob = enable,
                            "notify" => state.options.notify = enable,
                            "nounset" => state.options.nounset = enable,
                            "verbose" => state.options.verbose = enable,
                            "xtrace" => state.options.xtrace = enable,
                            _ => {
                                return Err(format!("set: {}: 无效选项名", opt_name));
                            }
                        }
                    } else {
                        // Show options
                        println!(
                            "allexport\t{}",
                            if state.options.allexport { "on" } else { "off" }
                        );
                        println!(
                            "errexit\t\t{}",
                            if state.options.errexit { "on" } else { "off" }
                        );
                        println!(
                            "hashall\t\t{}",
                            if state.options.hashall { "on" } else { "off" }
                        );
                        println!(
                            "ignoreeof\t{}",
                            if state.options.ignoreeof { "on" } else { "off" }
                        );
                        println!(
                            "noglob\t\t{}",
                            if state.options.noglob { "on" } else { "off" }
                        );
                        println!(
                            "notify\t\t{}",
                            if state.options.notify { "on" } else { "off" }
                        );
                        println!(
                            "nounset\t\t{}",
                            if state.options.nounset { "on" } else { "off" }
                        );
                        println!(
                            "verbose\t\t{}",
                            if state.options.verbose { "on" } else { "off" }
                        );
                        println!(
                            "xtrace\t\t{}",
                            if state.options.xtrace { "on" } else { "off" }
                        );
                    }
                }
                _ => {
                    return Err(format!("set: -{}: 无效选项", flag));
                }
            }
        }
    }

    Ok(0)
}

/// declare/typeset builtin
fn builtin_declare(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut show_all = false;
    let mut make_readonly = false;
    let mut make_export = false;
    let mut make_integer = false;
    let mut make_lowercase = false;
    let mut make_uppercase = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        if arg.starts_with('-') || arg.starts_with('+') {
            let enable = arg.starts_with('-');
            for flag in arg[1..].chars() {
                match flag {
                    'r' => {
                        if enable {
                            make_readonly = true
                        }
                    }
                    'x' => make_export = enable,
                    'i' => make_integer = enable,
                    'l' => make_lowercase = enable,
                    'u' => make_uppercase = enable,
                    'p' => show_all = true,
                    'a' | 'A' | 'f' | 'g' | 'I' | 'n' | 't' | 'F' => {
                        // Not implemented, ignore
                    }
                    _ => {
                        return Err(format!("declare: -{}: 无效选项", flag));
                    }
                }
            }
        } else {
            names.push(*arg);
        }
    }

    if show_all || names.is_empty() {
        // Show variables
        let vars = state.all_vars();
        for (name, var) in vars {
            let mut attrs = String::new();
            if var.attrs.exported {
                attrs.push('x');
            }
            if var.attrs.readonly {
                attrs.push('r');
            }
            if var.attrs.integer {
                attrs.push('i');
            }
            if var.attrs.lowercase {
                attrs.push('l');
            }
            if var.attrs.uppercase {
                attrs.push('u');
            }

            if attrs.is_empty() {
                println!("{}={}", name, var.value);
            } else {
                println!("declare -{} {}=\"{}\"", attrs, name, var.value);
            }
        }
        return Ok(0);
    }

    for arg in names {
        let (name, value) = if let Some((n, v)) = arg.split_once('=') {
            (n, Some(v))
        } else {
            (arg, None)
        };

        if let Some(v) = value {
            state.set_var(name, v);
        } else if state.get_var(name).is_none() {
            state.set_var(name, "");
        }

        // Apply attributes
        if let Some(var) = state.variables_mut().get_mut(name) {
            if make_export {
                var.attrs.exported = true;
            }
            if make_integer {
                var.attrs.integer = true;
            }
            if make_lowercase {
                var.attrs.lowercase = true;
                var.attrs.uppercase = false;
            }
            if make_uppercase {
                var.attrs.uppercase = true;
                var.attrs.lowercase = false;
            }
        }

        if make_readonly {
            let _ = state.set_readonly(name);
        }
    }

    Ok(0)
}

/// readonly builtin
fn builtin_readonly(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut show_all = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-p" => show_all = true,
            "-a" | "-A" | "-f" => {} // Not implemented
            "--" => {}
            arg if arg.starts_with('-') => {
                return Err(format!("readonly: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    if show_all || names.is_empty() {
        // Show readonly variables
        let vars = state.all_vars();
        for (name, var) in vars {
            if var.attrs.readonly {
                println!("declare -r {}=\"{}\"", name, var.value);
            }
        }
        return Ok(0);
    }

    for arg in names {
        if let Some((name, value)) = arg.split_once('=') {
            state.set_var(name, value);
            if let Err(e) = state.set_readonly(name) {
                eprintln!("readonly: {}", e);
            }
        } else {
            if let Err(e) = state.set_readonly(arg) {
                eprintln!("readonly: {}", e);
            }
        }
    }

    Ok(0)
}

/// alias builtin
fn builtin_alias(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut show_all = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-p" => show_all = true,
            arg if arg.starts_with('-') => {
                return Err(format!("alias: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    if show_all || names.is_empty() {
        // Show aliases
        for (name, value) in state.list_aliases() {
            println!("alias {}='{}'", name, value);
        }
        return Ok(0);
    }

    let mut success = true;
    for arg in names {
        if let Some((name, value)) = arg.split_once('=') {
            // Define alias
            state.set_alias(name, value);
        } else {
            // Show specific alias
            if let Some(value) = state.get_alias(arg) {
                println!("alias {}='{}'", arg, value);
            } else {
                eprintln!("alias: {}: 未找到", arg);
                success = false;
            }
        }
    }

    if success {
        Ok(0)
    } else {
        Ok(1)
    }
}

/// unalias builtin
fn builtin_unalias(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    let mut clear_all = false;
    let mut names: Vec<&str> = Vec::new();

    for arg in args {
        match *arg {
            "-a" => clear_all = true,
            arg if arg.starts_with('-') => {
                return Err(format!("unalias: {}: 无效选项", arg));
            }
            _ => names.push(*arg),
        }
    }

    if clear_all {
        state.clear_aliases();
        return Ok(0);
    }

    if names.is_empty() {
        return Err("unalias: 用法: unalias [-a] 名称 [名称 ...]".to_string());
    }

    let mut success = true;
    for name in names {
        if !state.unset_alias(name) {
            eprintln!("unalias: {}: 未找到", name);
            success = false;
        }
    }

    if success {
        Ok(0)
    } else {
        Ok(1)
    }
}

/// local builtin
fn builtin_local(state: &mut ShellState, args: &[&str]) -> BuiltinResult {
    if state.function_depth == 0 {
        return Err("local: 只能在函数内使用".to_string());
    }

    // For now, local acts like declare within a function
    builtin_declare(state, args)
}
