//! Vim Script value types

use std::collections::HashMap;
use std::fmt;

/// Vim Script value type
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Null/void value
    Null,
    /// Integer value
    Integer(i64),
    /// Floating point value
    Float(f64),
    /// String value
    String(String),
    /// List value
    List(Vec<Value>),
    /// Dictionary value
    Dict(HashMap<String, Value>),
    /// Funcref (function reference)
    Funcref(String),
    /// Special values (v:true, v:false, v:null, v:none)
    Special(SpecialValue),
}

/// Special Vim values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialValue {
    True,
    False,
    Null,
    None,
}

impl Value {
    /// Check if value is truthy (non-zero, non-empty)
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Integer(n) => *n != 0,
            Value::Float(n) => *n != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::List(l) => !l.is_empty(),
            Value::Dict(d) => !d.is_empty(),
            Value::Funcref(_) => true,
            Value::Special(s) => matches!(s, SpecialValue::True),
        }
    }
    
    /// Convert to integer
    pub fn to_int(&self) -> i64 {
        match self {
            Value::Null => 0,
            Value::Integer(n) => *n,
            Value::Float(n) => *n as i64,
            Value::String(s) => s.parse().unwrap_or(0),
            Value::List(l) => l.len() as i64,
            Value::Dict(d) => d.len() as i64,
            Value::Funcref(_) => 0,
            Value::Special(s) => match s {
                SpecialValue::True => 1,
                _ => 0,
            },
        }
    }
    
    /// Convert to float
    pub fn to_float(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Integer(n) => *n as f64,
            Value::Float(n) => *n,
            Value::String(s) => s.parse().unwrap_or(0.0),
            Value::List(l) => l.len() as f64,
            Value::Dict(d) => d.len() as f64,
            Value::Funcref(_) => 0.0,
            Value::Special(s) => match s {
                SpecialValue::True => 1.0,
                _ => 0.0,
            },
        }
    }
    
    /// Get type name
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "v:t_none",
            Value::Integer(_) => "v:t_number",
            Value::Float(_) => "v:t_float",
            Value::String(_) => "v:t_string",
            Value::List(_) => "v:t_list",
            Value::Dict(_) => "v:t_dict",
            Value::Funcref(_) => "v:t_func",
            Value::Special(_) => "v:t_special",
        }
    }
    
    /// Get type number (for type() function)
    pub fn type_number(&self) -> i64 {
        match self {
            Value::Integer(_) => 0,
            Value::String(_) => 1,
            Value::Funcref(_) => 2,
            Value::List(_) => 3,
            Value::Dict(_) => 4,
            Value::Float(_) => 5,
            Value::Special(_) => 6,
            Value::Null => 7,
        }
    }
    
    /// Compare two values (for sorting)
    pub fn compare(&self, other: &Value) -> i32 {
        match (self, other) {
            (Value::Integer(a), Value::Integer(b)) => {
                if a < b { -1 } else if a > b { 1 } else { 0 }
            }
            (Value::Float(a), Value::Float(b)) => {
                if a < b { -1.0 as i32 } else if a > b { 1 } else { 0 }
            }
            (Value::String(a), Value::String(b)) => {
                a.cmp(b) as i32
            }
            _ => {
                let a = self.to_float();
                let b = other.to_float();
                if a < b { -1 } else if a > b { 1 } else { 0 }
            }
        }
    }
    
    /// Get list item by index
    pub fn get_index(&self, index: i64) -> Option<Value> {
        match self {
            Value::List(l) => {
                let idx = if index < 0 {
                    (l.len() as i64 + index) as usize
                } else {
                    index as usize
                };
                l.get(idx).cloned()
            }
            Value::String(s) => {
                let idx = if index < 0 {
                    (s.len() as i64 + index) as usize
                } else {
                    index as usize
                };
                s.chars().nth(idx).map(|c| Value::String(c.to_string()))
            }
            Value::Dict(d) => d.get(&index.to_string()).cloned(),
            _ => None,
        }
    }
    
    /// Get dictionary item by key
    pub fn get_key(&self, key: &str) -> Option<Value> {
        match self {
            Value::Dict(d) => d.get(key).cloned(),
            _ => None,
        }
    }
    
    /// Get length
    pub fn len(&self) -> usize {
        match self {
            Value::String(s) => s.len(),
            Value::List(l) => l.len(),
            Value::Dict(d) => d.len(),
            _ => 0,
        }
    }
    
    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "v:null"),
            Value::Integer(n) => write!(f, "{}", n),
            Value::Float(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "{}", s),
            Value::List(l) => {
                write!(f, "[")?;
                for (i, item) in l.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    match item {
                        Value::String(s) => write!(f, "'{}'", s)?,
                        _ => write!(f, "{}", item)?,
                    }
                }
                write!(f, "]")
            }
            Value::Dict(d) => {
                write!(f, "{{")?;
                for (i, (k, v)) in d.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "'{}': ", k)?;
                    match v {
                        Value::String(s) => write!(f, "'{}'", s)?,
                        _ => write!(f, "{}", v)?,
                    }
                }
                write!(f, "}}")
            }
            Value::Funcref(name) => write!(f, "function('{}')", name),
            Value::Special(s) => match s {
                SpecialValue::True => write!(f, "v:true"),
                SpecialValue::False => write!(f, "v:false"),
                SpecialValue::Null => write!(f, "v:null"),
                SpecialValue::None => write!(f, "v:none"),
            },
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Integer(if b { 1 } else { 0 })
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Integer(n)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Value::Integer(n as i64)
    }
}

impl From<usize> for Value {
    fn from(n: usize) -> Self {
        Value::Integer(n as i64)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Float(n)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(v: Vec<T>) -> Self {
        Value::List(v.into_iter().map(|x| x.into()).collect())
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(v) => v.into(),
            None => Value::Null,
        }
    }
}
