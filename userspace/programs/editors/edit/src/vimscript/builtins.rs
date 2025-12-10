//! Built-in Vim Script functions

use super::Value;
use std::io;

/// Call a built-in function
pub fn call_builtin(name: &str, args: &[Value]) -> Option<io::Result<Value>> {
    match name {
        // String functions
        "strlen" => Some(strlen(args)),
        "strwidth" => Some(strlen(args)),        // Simplified
        "strdisplaywidth" => Some(strlen(args)), // Simplified
        "len" => Some(len(args)),
        "empty" => Some(empty(args)),
        "tolower" => Some(tolower(args)),
        "toupper" => Some(toupper(args)),
        "trim" => Some(trim(args)),
        "substitute" => Some(substitute(args)),
        "split" => Some(split(args)),
        "join" => Some(join(args)),
        "escape" => Some(escape(args)),
        "shellescape" => Some(shellescape(args)),
        "fnameescape" => Some(fnameescape(args)),
        "fnamemodify" => Some(fnamemodify(args)),
        "strpart" => Some(strpart(args)),
        "strcharpart" => Some(strpart(args)), // Simplified
        "stridx" => Some(stridx(args)),
        "strridx" => Some(strridx(args)),
        "matchstr" => Some(matchstr(args)),
        "match" => Some(match_fn(args)),
        "matchend" => Some(matchend(args)),
        "matchlist" => Some(matchlist(args)),
        "printf" => Some(printf(args)),
        "tr" => Some(tr(args)),
        "repeat" => Some(repeat(args)),

        // List functions
        "add" => Some(add(args)),
        "extend" => Some(extend(args)),
        "insert" => Some(insert(args)),
        "remove" => Some(remove(args)),
        "reverse" => Some(reverse(args)),
        "sort" => Some(sort(args)),
        "uniq" => Some(uniq(args)),
        "filter" => Some(filter(args)),
        "map" => Some(map_fn(args)),
        "copy" => Some(copy(args)),
        "deepcopy" => Some(deepcopy(args)),
        "get" => Some(get(args)),
        "index" => Some(index(args)),
        "count" => Some(count(args)),
        "range" => Some(range(args)),
        "flatten" => Some(flatten(args)),

        // Dictionary functions
        "has_key" => Some(has_key(args)),
        "keys" => Some(keys(args)),
        "values" => Some(values(args)),
        "items" => Some(items(args)),

        // Type functions
        "type" => Some(type_fn(args)),
        "typename" => Some(typename(args)),
        "string" => Some(string(args)),
        "str2nr" => Some(str2nr(args)),
        "str2float" => Some(str2float(args)),
        "nr2char" => Some(nr2char(args)),
        "char2nr" => Some(char2nr(args)),
        "float2nr" => Some(float2nr(args)),
        "abs" => Some(abs(args)),
        "floor" => Some(floor(args)),
        "ceil" => Some(ceil(args)),
        "round" => Some(round(args)),
        "sqrt" => Some(sqrt(args)),
        "pow" => Some(pow(args)),
        "log" => Some(log(args)),
        "exp" => Some(exp(args)),
        "sin" => Some(sin(args)),
        "cos" => Some(cos(args)),
        "tan" => Some(tan(args)),

        // Comparison functions
        "min" => Some(min(args)),
        "max" => Some(max(args)),

        // Test functions
        "exists" => Some(exists(args)),
        "has" => Some(has(args)),
        "executable" => Some(executable(args)),
        "filereadable" => Some(filereadable(args)),
        "filewritable" => Some(filewritable(args)),
        "isdirectory" => Some(isdirectory(args)),

        // File functions
        "glob" => Some(glob(args)),
        "globpath" => Some(globpath(args)),
        "readfile" => Some(readfile(args)),
        "writefile" => Some(writefile(args)),
        "delete" => Some(delete(args)),
        "rename" => Some(rename(args)),
        "mkdir" => Some(mkdir(args)),
        "getcwd" => Some(getcwd(args)),
        "expand" => Some(expand(args)),
        "resolve" => Some(resolve(args)),
        "simplify" => Some(simplify(args)),

        // Utility functions
        "system" => Some(system(args)),
        "systemlist" => Some(systemlist(args)),
        "localtime" => Some(localtime(args)),
        "strftime" => Some(strftime(args)),

        // Input functions
        "input" => Some(input(args)),
        "inputlist" => Some(inputlist(args)),
        "confirm" => Some(confirm(args)),

        // Miscellaneous
        "eval" => Some(eval(args)),
        "execute" => Some(execute(args)),
        "function" => Some(function(args)),
        "funcref" => Some(function(args)),
        "call" => Some(call(args)),

        // Unknown function
        _ => None,
    }
}

// String functions

fn strlen(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    Ok(Value::Integer(s.len() as i64))
}

fn len(args: &[Value]) -> io::Result<Value> {
    let v = args.get(0).unwrap_or(&Value::Null);
    Ok(Value::Integer(v.len() as i64))
}

fn empty(args: &[Value]) -> io::Result<Value> {
    let v = args.get(0).unwrap_or(&Value::Null);
    Ok(Value::Integer(if v.is_empty() { 1 } else { 0 }))
}

fn tolower(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    Ok(Value::String(s.to_lowercase()))
}

fn toupper(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    Ok(Value::String(s.to_uppercase()))
}

fn trim(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let chars = args.get(1).map(|v| v.to_string());

    let result = if let Some(chars) = chars {
        let chars: Vec<char> = chars.chars().collect();
        s.trim_matches(|c| chars.contains(&c)).to_string()
    } else {
        s.trim().to_string()
    };

    Ok(Value::String(result))
}

fn substitute(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let pat = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let sub = args.get(2).map(|v| v.to_string()).unwrap_or_default();
    let flags = args.get(3).map(|v| v.to_string()).unwrap_or_default();

    let result = if flags.contains('g') {
        s.replace(&pat, &sub)
    } else {
        s.replacen(&pat, &sub, 1)
    };

    Ok(Value::String(result))
}

fn split(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let pat = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| " ".to_string());

    let parts: Vec<Value> = s
        .split(&pat)
        .filter(|s| !s.is_empty())
        .map(|s| Value::String(s.to_string()))
        .collect();

    Ok(Value::List(parts))
}

fn join(args: &[Value]) -> io::Result<Value> {
    let list = args.get(0).cloned().unwrap_or(Value::List(Vec::new()));
    let sep = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| " ".to_string());

    if let Value::List(items) = list {
        let strings: Vec<String> = items.iter().map(|v| v.to_string()).collect();
        Ok(Value::String(strings.join(&sep)))
    } else {
        Ok(Value::String(list.to_string()))
    }
}

fn escape(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let chars = args.get(1).map(|v| v.to_string()).unwrap_or_default();

    let mut result = String::new();
    for ch in s.chars() {
        if chars.contains(ch) {
            result.push('\\');
        }
        result.push(ch);
    }

    Ok(Value::String(result))
}

fn shellescape(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    Ok(Value::String(format!("'{}'", s.replace('\'', "'\\''"))))
}

fn fnameescape(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let mut result = String::new();
    for ch in s.chars() {
        if " \t\n*?[{`$\\%#'\"|!<".contains(ch) {
            result.push('\\');
        }
        result.push(ch);
    }
    Ok(Value::String(result))
}

fn fnamemodify(args: &[Value]) -> io::Result<Value> {
    let fname = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let mods = args.get(1).map(|v| v.to_string()).unwrap_or_default();

    let mut result = fname.clone();

    for m in mods.split(':').skip(1) {
        match m {
            "p" => {
                // Full path
                if !result.starts_with('/') {
                    if let Ok(cwd) = std::env::current_dir() {
                        result = format!("{}/{}", cwd.display(), result);
                    }
                }
            }
            "h" => {
                // Head (directory part)
                if let Some(pos) = result.rfind('/') {
                    result = result[..pos].to_string();
                }
            }
            "t" => {
                // Tail (filename part)
                if let Some(pos) = result.rfind('/') {
                    result = result[pos + 1..].to_string();
                }
            }
            "r" => {
                // Root (remove extension)
                if let Some(pos) = result.rfind('.') {
                    result = result[..pos].to_string();
                }
            }
            "e" => {
                // Extension
                if let Some(pos) = result.rfind('.') {
                    result = result[pos + 1..].to_string();
                } else {
                    result = String::new();
                }
            }
            _ => {}
        }
    }

    Ok(Value::String(result))
}

fn strpart(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let start = args.get(1).map(|v| v.to_int()).unwrap_or(0) as usize;
    let len = args.get(2).map(|v| v.to_int() as usize);

    let chars: Vec<char> = s.chars().collect();
    let end = len
        .map(|l| (start + l).min(chars.len()))
        .unwrap_or(chars.len());
    let start = start.min(chars.len());

    Ok(Value::String(chars[start..end].iter().collect()))
}

fn stridx(args: &[Value]) -> io::Result<Value> {
    let haystack = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let needle = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let start = args.get(2).map(|v| v.to_int()).unwrap_or(0) as usize;

    let result = haystack[start..]
        .find(&needle)
        .map(|pos| (start + pos) as i64)
        .unwrap_or(-1);

    Ok(Value::Integer(result))
}

fn strridx(args: &[Value]) -> io::Result<Value> {
    let haystack = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let needle = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let start = args.get(2).map(|v| v.to_int() as usize);

    let search_str = start
        .map(|s| &haystack[..s.min(haystack.len())])
        .unwrap_or(&haystack);

    let result = search_str
        .rfind(&needle)
        .map(|pos| pos as i64)
        .unwrap_or(-1);

    Ok(Value::Integer(result))
}

fn matchstr(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let pat = args.get(1).map(|v| v.to_string()).unwrap_or_default();

    // Simplified: just find the pattern
    if let Some(pos) = s.find(&pat) {
        Ok(Value::String(s[pos..pos + pat.len()].to_string()))
    } else {
        Ok(Value::String(String::new()))
    }
}

fn match_fn(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let pat = args.get(1).map(|v| v.to_string()).unwrap_or_default();

    Ok(Value::Integer(s.find(&pat).map(|p| p as i64).unwrap_or(-1)))
}

fn matchend(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let pat = args.get(1).map(|v| v.to_string()).unwrap_or_default();

    Ok(Value::Integer(
        s.find(&pat).map(|p| (p + pat.len()) as i64).unwrap_or(-1),
    ))
}

fn matchlist(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let pat = args.get(1).map(|v| v.to_string()).unwrap_or_default();

    if s.contains(&pat) {
        Ok(Value::List(vec![Value::String(pat)]))
    } else {
        Ok(Value::List(Vec::new()))
    }
}

fn printf(args: &[Value]) -> io::Result<Value> {
    let fmt = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let mut result = fmt;

    for (i, arg) in args.iter().skip(1).enumerate() {
        let placeholder = format!("%{}", i + 1);
        result = result.replace(&placeholder, &arg.to_string());

        // Also handle simple %s, %d patterns
        if result.contains("%s") {
            result = result.replacen("%s", &arg.to_string(), 1);
        } else if result.contains("%d") {
            result = result.replacen("%d", &arg.to_int().to_string(), 1);
        }
    }

    Ok(Value::String(result))
}

fn tr(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let from = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    let to = args.get(2).map(|v| v.to_string()).unwrap_or_default();

    let from_chars: Vec<char> = from.chars().collect();
    let to_chars: Vec<char> = to.chars().collect();

    let result: String = s
        .chars()
        .map(|c| {
            from_chars
                .iter()
                .position(|&fc| fc == c)
                .and_then(|i| to_chars.get(i).copied())
                .unwrap_or(c)
        })
        .collect();

    Ok(Value::String(result))
}

fn repeat(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let count = args.get(1).map(|v| v.to_int()).unwrap_or(0) as usize;
    Ok(Value::String(s.repeat(count)))
}

// List functions

fn add(args: &[Value]) -> io::Result<Value> {
    let mut list = match args.get(0) {
        Some(Value::List(l)) => l.clone(),
        _ => return Ok(Value::List(Vec::new())),
    };

    if let Some(item) = args.get(1) {
        list.push(item.clone());
    }

    Ok(Value::List(list))
}

fn extend(args: &[Value]) -> io::Result<Value> {
    let mut list1 = match args.get(0) {
        Some(Value::List(l)) => l.clone(),
        _ => return Ok(Value::List(Vec::new())),
    };

    if let Some(Value::List(l2)) = args.get(1) {
        list1.extend(l2.clone());
    }

    Ok(Value::List(list1))
}

fn insert(args: &[Value]) -> io::Result<Value> {
    let mut list = match args.get(0) {
        Some(Value::List(l)) => l.clone(),
        _ => return Ok(Value::List(Vec::new())),
    };

    let item = args.get(1).cloned().unwrap_or(Value::Null);
    let idx = args.get(2).map(|v| v.to_int()).unwrap_or(0) as usize;

    list.insert(idx.min(list.len()), item);
    Ok(Value::List(list))
}

fn remove(args: &[Value]) -> io::Result<Value> {
    let mut list = match args.get(0) {
        Some(Value::List(l)) => l.clone(),
        _ => return Ok(Value::Null),
    };

    let idx = args.get(1).map(|v| v.to_int()).unwrap_or(0) as usize;

    if idx < list.len() {
        Ok(list.remove(idx))
    } else {
        Ok(Value::Null)
    }
}

fn reverse(args: &[Value]) -> io::Result<Value> {
    let mut list = match args.get(0) {
        Some(Value::List(l)) => l.clone(),
        _ => return Ok(Value::List(Vec::new())),
    };

    list.reverse();
    Ok(Value::List(list))
}

fn sort(args: &[Value]) -> io::Result<Value> {
    let mut list = match args.get(0) {
        Some(Value::List(l)) => l.clone(),
        _ => return Ok(Value::List(Vec::new())),
    };

    list.sort_by(|a, b| {
        use std::cmp::Ordering;
        match a.compare(b) {
            x if x < 0 => Ordering::Less,
            x if x > 0 => Ordering::Greater,
            _ => Ordering::Equal,
        }
    });

    Ok(Value::List(list))
}

fn uniq(args: &[Value]) -> io::Result<Value> {
    let list = match args.get(0) {
        Some(Value::List(l)) => l.clone(),
        _ => return Ok(Value::List(Vec::new())),
    };

    let mut result = Vec::new();
    for item in list {
        if !result.contains(&item) {
            result.push(item);
        }
    }

    Ok(Value::List(result))
}

fn filter(args: &[Value]) -> io::Result<Value> {
    // Simplified filter - just returns list as-is
    Ok(args.get(0).cloned().unwrap_or(Value::List(Vec::new())))
}

fn map_fn(args: &[Value]) -> io::Result<Value> {
    // Simplified map - just returns list as-is
    Ok(args.get(0).cloned().unwrap_or(Value::List(Vec::new())))
}

fn copy(args: &[Value]) -> io::Result<Value> {
    Ok(args.get(0).cloned().unwrap_or(Value::Null))
}

fn deepcopy(args: &[Value]) -> io::Result<Value> {
    Ok(args.get(0).cloned().unwrap_or(Value::Null))
}

fn get(args: &[Value]) -> io::Result<Value> {
    let container = args.get(0).unwrap_or(&Value::Null);
    let key = args.get(1).unwrap_or(&Value::Null);
    let default = args.get(2).cloned().unwrap_or(Value::Null);

    match container {
        Value::List(l) => {
            let idx = key.to_int() as usize;
            Ok(l.get(idx).cloned().unwrap_or(default))
        }
        Value::Dict(d) => Ok(d.get(&key.to_string()).cloned().unwrap_or(default)),
        _ => Ok(default),
    }
}

fn index(args: &[Value]) -> io::Result<Value> {
    let list = match args.get(0) {
        Some(Value::List(l)) => l,
        _ => return Ok(Value::Integer(-1)),
    };

    let item = args.get(1).unwrap_or(&Value::Null);

    for (i, v) in list.iter().enumerate() {
        if v == item {
            return Ok(Value::Integer(i as i64));
        }
    }

    Ok(Value::Integer(-1))
}

fn count(args: &[Value]) -> io::Result<Value> {
    let list = match args.get(0) {
        Some(Value::List(l)) => l,
        _ => return Ok(Value::Integer(0)),
    };

    let item = args.get(1).unwrap_or(&Value::Null);
    let count = list.iter().filter(|v| *v == item).count();

    Ok(Value::Integer(count as i64))
}

fn range(args: &[Value]) -> io::Result<Value> {
    let end = args.get(0).map(|v| v.to_int()).unwrap_or(0);
    let start = args.get(1).map(|v| v.to_int()).unwrap_or(0);
    let step = args.get(2).map(|v| v.to_int()).unwrap_or(1);

    let mut list = Vec::new();
    let mut i = start;

    if step > 0 {
        while i <= end {
            list.push(Value::Integer(i));
            i += step;
        }
    } else if step < 0 {
        while i >= end {
            list.push(Value::Integer(i));
            i += step;
        }
    }

    Ok(Value::List(list))
}

fn flatten(args: &[Value]) -> io::Result<Value> {
    let list = match args.get(0) {
        Some(Value::List(l)) => l.clone(),
        _ => return Ok(Value::List(Vec::new())),
    };

    let maxdepth = args.get(1).map(|v| v.to_int()).unwrap_or(-1);

    fn flatten_recursive(list: &[Value], depth: i64, maxdepth: i64) -> Vec<Value> {
        let mut result = Vec::new();
        for item in list {
            if let Value::List(inner) = item {
                if maxdepth < 0 || depth < maxdepth {
                    result.extend(flatten_recursive(inner, depth + 1, maxdepth));
                } else {
                    result.push(item.clone());
                }
            } else {
                result.push(item.clone());
            }
        }
        result
    }

    Ok(Value::List(flatten_recursive(&list, 0, maxdepth)))
}

// Dictionary functions

fn has_key(args: &[Value]) -> io::Result<Value> {
    let dict = match args.get(0) {
        Some(Value::Dict(d)) => d,
        _ => return Ok(Value::Integer(0)),
    };

    let key = args.get(1).map(|v| v.to_string()).unwrap_or_default();
    Ok(Value::Integer(if dict.contains_key(&key) { 1 } else { 0 }))
}

fn keys(args: &[Value]) -> io::Result<Value> {
    let dict = match args.get(0) {
        Some(Value::Dict(d)) => d,
        _ => return Ok(Value::List(Vec::new())),
    };

    let keys: Vec<Value> = dict.keys().map(|k| Value::String(k.clone())).collect();
    Ok(Value::List(keys))
}

fn values(args: &[Value]) -> io::Result<Value> {
    let dict = match args.get(0) {
        Some(Value::Dict(d)) => d,
        _ => return Ok(Value::List(Vec::new())),
    };

    let values: Vec<Value> = dict.values().cloned().collect();
    Ok(Value::List(values))
}

fn items(args: &[Value]) -> io::Result<Value> {
    let dict = match args.get(0) {
        Some(Value::Dict(d)) => d,
        _ => return Ok(Value::List(Vec::new())),
    };

    let items: Vec<Value> = dict
        .iter()
        .map(|(k, v)| Value::List(vec![Value::String(k.clone()), v.clone()]))
        .collect();
    Ok(Value::List(items))
}

// Type functions

fn type_fn(args: &[Value]) -> io::Result<Value> {
    let v = args.get(0).unwrap_or(&Value::Null);
    Ok(Value::Integer(v.type_number()))
}

fn typename(args: &[Value]) -> io::Result<Value> {
    let v = args.get(0).unwrap_or(&Value::Null);
    Ok(Value::String(v.type_name().to_string()))
}

fn string(args: &[Value]) -> io::Result<Value> {
    let v = args.get(0).unwrap_or(&Value::Null);
    Ok(Value::String(v.to_string()))
}

fn str2nr(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let base = args.get(1).map(|v| v.to_int()).unwrap_or(10) as u32;

    let n = i64::from_str_radix(s.trim(), base).unwrap_or(0);
    Ok(Value::Integer(n))
}

fn str2float(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let n = s.trim().parse::<f64>().unwrap_or(0.0);
    Ok(Value::Float(n))
}

fn nr2char(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_int()).unwrap_or(0) as u32;
    let ch = char::from_u32(n).unwrap_or('\0');
    Ok(Value::String(ch.to_string()))
}

fn char2nr(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let n = s.chars().next().map(|c| c as i64).unwrap_or(0);
    Ok(Value::Integer(n))
}

fn float2nr(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Integer(n as i64))
}

fn abs(args: &[Value]) -> io::Result<Value> {
    match args.get(0) {
        Some(Value::Integer(n)) => Ok(Value::Integer(n.abs())),
        Some(Value::Float(n)) => Ok(Value::Float(n.abs())),
        _ => Ok(Value::Integer(0)),
    }
}

fn floor(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Float(n.floor()))
}

fn ceil(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Float(n.ceil()))
}

fn round(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Float(n.round()))
}

fn sqrt(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Float(n.sqrt()))
}

fn pow(args: &[Value]) -> io::Result<Value> {
    let x = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    let y = args.get(1).map(|v| v.to_float()).unwrap_or(1.0);
    Ok(Value::Float(x.powf(y)))
}

fn log(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(1.0);
    Ok(Value::Float(n.ln()))
}

fn exp(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Float(n.exp()))
}

fn sin(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Float(n.sin()))
}

fn cos(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Float(n.cos()))
}

fn tan(args: &[Value]) -> io::Result<Value> {
    let n = args.get(0).map(|v| v.to_float()).unwrap_or(0.0);
    Ok(Value::Float(n.tan()))
}

// Comparison

fn min(args: &[Value]) -> io::Result<Value> {
    match args.get(0) {
        Some(Value::List(l)) if !l.is_empty() => {
            let mut min_val = &l[0];
            for item in l.iter().skip(1) {
                if item.compare(min_val) < 0 {
                    min_val = item;
                }
            }
            Ok(min_val.clone())
        }
        _ => Ok(Value::Integer(0)),
    }
}

fn max(args: &[Value]) -> io::Result<Value> {
    match args.get(0) {
        Some(Value::List(l)) if !l.is_empty() => {
            let mut max_val = &l[0];
            for item in l.iter().skip(1) {
                if item.compare(max_val) > 0 {
                    max_val = item;
                }
            }
            Ok(max_val.clone())
        }
        _ => Ok(Value::Integer(0)),
    }
}

// Test functions

fn exists(args: &[Value]) -> io::Result<Value> {
    // Simplified - always return false for now
    let _ = args;
    Ok(Value::Integer(0))
}

fn has(args: &[Value]) -> io::Result<Value> {
    let feature = args.get(0).map(|v| v.to_string()).unwrap_or_default();

    let supported = matches!(
        feature.as_str(),
        "unix" | "nvim" | "vim_starting" | "syntax" | "autocmd" | "eval"
    );

    Ok(Value::Integer(if supported { 1 } else { 0 }))
}

fn executable(args: &[Value]) -> io::Result<Value> {
    let _ = args;
    Ok(Value::Integer(0)) // Simplified
}

fn filereadable(args: &[Value]) -> io::Result<Value> {
    let path = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let readable = std::path::Path::new(&path).is_file();
    Ok(Value::Integer(if readable { 1 } else { 0 }))
}

fn filewritable(args: &[Value]) -> io::Result<Value> {
    filereadable(args) // Simplified
}

fn isdirectory(args: &[Value]) -> io::Result<Value> {
    let path = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let is_dir = std::path::Path::new(&path).is_dir();
    Ok(Value::Integer(if is_dir { 1 } else { 0 }))
}

// File functions

fn glob(args: &[Value]) -> io::Result<Value> {
    let _ = args;
    Ok(Value::String(String::new())) // Simplified
}

fn globpath(args: &[Value]) -> io::Result<Value> {
    let _ = args;
    Ok(Value::String(String::new())) // Simplified
}

fn readfile(args: &[Value]) -> io::Result<Value> {
    let path = args.get(0).map(|v| v.to_string()).unwrap_or_default();

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let lines: Vec<Value> = content
                .lines()
                .map(|l| Value::String(l.to_string()))
                .collect();
            Ok(Value::List(lines))
        }
        Err(_) => Ok(Value::List(Vec::new())),
    }
}

fn writefile(args: &[Value]) -> io::Result<Value> {
    let lines = match args.get(0) {
        Some(Value::List(l)) => l,
        _ => return Ok(Value::Integer(-1)),
    };

    let path = args.get(1).map(|v| v.to_string()).unwrap_or_default();

    let content: String = lines
        .iter()
        .map(|l| l.to_string())
        .collect::<Vec<_>>()
        .join("\n");

    match std::fs::write(&path, content) {
        Ok(_) => Ok(Value::Integer(0)),
        Err(_) => Ok(Value::Integer(-1)),
    }
}

fn delete(args: &[Value]) -> io::Result<Value> {
    let path = args.get(0).map(|v| v.to_string()).unwrap_or_default();

    match std::fs::remove_file(&path) {
        Ok(_) => Ok(Value::Integer(0)),
        Err(_) => Ok(Value::Integer(-1)),
    }
}

fn rename(args: &[Value]) -> io::Result<Value> {
    let from = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let to = args.get(1).map(|v| v.to_string()).unwrap_or_default();

    match std::fs::rename(&from, &to) {
        Ok(_) => Ok(Value::Integer(0)),
        Err(_) => Ok(Value::Integer(-1)),
    }
}

fn mkdir(args: &[Value]) -> io::Result<Value> {
    let path = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let recursive = args.get(1).map(|v| v.to_string()).unwrap_or_default() == "p";

    let result = if recursive {
        std::fs::create_dir_all(&path)
    } else {
        std::fs::create_dir(&path)
    };

    match result {
        Ok(_) => Ok(Value::Integer(0)),
        Err(_) => Ok(Value::Integer(-1)),
    }
}

fn getcwd(_args: &[Value]) -> io::Result<Value> {
    match std::env::current_dir() {
        Ok(path) => Ok(Value::String(path.to_string_lossy().to_string())),
        Err(_) => Ok(Value::String(String::new())),
    }
}

fn expand(args: &[Value]) -> io::Result<Value> {
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();

    let expanded = match s.as_str() {
        "%" => "[No Name]".to_string(), // Current file
        "%:p" => std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default(),
        "%:t" => "[No Name]".to_string(),
        "%:h" => ".".to_string(),
        "~" | "$HOME" => std::env::var("HOME").unwrap_or_default(),
        _ if s.starts_with('$') => std::env::var(&s[1..]).unwrap_or_default(),
        _ => s,
    };

    Ok(Value::String(expanded))
}

fn resolve(args: &[Value]) -> io::Result<Value> {
    let path = args.get(0).map(|v| v.to_string()).unwrap_or_default();

    match std::fs::canonicalize(&path) {
        Ok(p) => Ok(Value::String(p.to_string_lossy().to_string())),
        Err(_) => Ok(Value::String(path)),
    }
}

fn simplify(args: &[Value]) -> io::Result<Value> {
    let path = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    // Simplified: just return as-is
    Ok(Value::String(path))
}

// Utility functions

fn system(args: &[Value]) -> io::Result<Value> {
    let _ = args;
    // System commands not supported in this environment
    Ok(Value::String(String::new()))
}

fn systemlist(args: &[Value]) -> io::Result<Value> {
    let _ = args;
    Ok(Value::List(Vec::new()))
}

fn localtime(_args: &[Value]) -> io::Result<Value> {
    // Return a placeholder timestamp
    Ok(Value::Integer(0))
}

fn strftime(args: &[Value]) -> io::Result<Value> {
    let fmt = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    // Simplified
    Ok(Value::String(fmt))
}

// Input functions

fn input(args: &[Value]) -> io::Result<Value> {
    let prompt = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    print!("{}", prompt);
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    Ok(Value::String(line.trim_end().to_string()))
}

fn inputlist(args: &[Value]) -> io::Result<Value> {
    let list = match args.get(0) {
        Some(Value::List(l)) => l,
        _ => return Ok(Value::Integer(-1)),
    };

    for (i, item) in list.iter().enumerate() {
        println!("{}: {}", i, item);
    }

    print!("Type number: ");
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    let choice = line.trim().parse::<i64>().unwrap_or(-1);
    Ok(Value::Integer(choice))
}

fn confirm(args: &[Value]) -> io::Result<Value> {
    let msg = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    let choices = args
        .get(1)
        .map(|v| v.to_string())
        .unwrap_or_else(|| "&Yes\n&No".to_string());
    let default = args.get(2).map(|v| v.to_int()).unwrap_or(0);

    println!("{}", msg);

    for (i, choice) in choices.split('\n').enumerate() {
        println!("{}: {}", i + 1, choice.trim_start_matches('&'));
    }

    print!("Choice (default {}): ", default);
    let _ = std::io::Write::flush(&mut std::io::stdout());

    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    let choice = line.trim().parse::<i64>().unwrap_or(default);
    Ok(Value::Integer(choice))
}

// Miscellaneous

fn eval(args: &[Value]) -> io::Result<Value> {
    // Would need access to VimScript interpreter
    let s = args.get(0).map(|v| v.to_string()).unwrap_or_default();

    // Try to parse as a simple value
    if let Ok(n) = s.parse::<i64>() {
        return Ok(Value::Integer(n));
    }
    if let Ok(n) = s.parse::<f64>() {
        return Ok(Value::Float(n));
    }

    Ok(Value::String(s))
}

fn execute(_args: &[Value]) -> io::Result<Value> {
    // Would need access to editor
    Ok(Value::String(String::new()))
}

fn function(args: &[Value]) -> io::Result<Value> {
    let name = args.get(0).map(|v| v.to_string()).unwrap_or_default();
    Ok(Value::Funcref(name))
}

fn call(args: &[Value]) -> io::Result<Value> {
    // Would need access to VimScript interpreter
    let _ = args;
    Ok(Value::Null)
}
