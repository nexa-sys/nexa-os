//! Vim Script parser (placeholder module)
//!
//! This module provides AST parsing for Vim Script.
//! Currently the interpreter uses direct line-by-line parsing.

/// AST node types
#[derive(Debug, Clone)]
pub enum Node {
    /// A program is a list of statements
    Program(Vec<Statement>),
}

/// Statement types
#[derive(Debug, Clone)]
pub enum Statement {
    /// Let assignment: let var = expr
    Let { name: String, value: Expression },
    /// Unlet: unlet var
    Unlet { name: String },
    /// If statement
    If {
        condition: Expression,
        then_branch: Vec<Statement>,
        else_branch: Option<Vec<Statement>>,
    },
    /// While loop
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    /// For loop
    For {
        variable: String,
        iterable: Expression,
        body: Vec<Statement>,
    },
    /// Function definition
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Statement>,
    },
    /// Return statement
    Return(Option<Expression>),
    /// Break statement
    Break,
    /// Continue statement
    Continue,
    /// Echo statement
    Echo(Vec<Expression>),
    /// Call statement
    Call(Expression),
    /// Execute statement
    Execute(Expression),
    /// Set option
    Set(String, Option<Expression>),
    /// Mapping
    Map {
        mode: String,
        lhs: String,
        rhs: String,
        noremap: bool,
    },
    /// Autocommand
    AutoCmd {
        event: String,
        pattern: String,
        command: String,
    },
    /// Raw command
    Command(String),
}

/// Expression types
#[derive(Debug, Clone)]
pub enum Expression {
    /// Integer literal
    Integer(i64),
    /// Float literal
    Float(f64),
    /// String literal
    String(String),
    /// Variable reference
    Variable(String),
    /// List literal
    List(Vec<Expression>),
    /// Dictionary literal
    Dict(Vec<(Expression, Expression)>),
    /// Function call
    Call {
        function: Box<Expression>,
        args: Vec<Expression>,
    },
    /// Binary operation
    Binary {
        left: Box<Expression>,
        operator: String,
        right: Box<Expression>,
    },
    /// Unary operation
    Unary {
        operator: String,
        operand: Box<Expression>,
    },
    /// Ternary operation
    Ternary {
        condition: Box<Expression>,
        then_expr: Box<Expression>,
        else_expr: Box<Expression>,
    },
    /// Index access
    Index {
        object: Box<Expression>,
        index: Box<Expression>,
    },
    /// Member access
    Member {
        object: Box<Expression>,
        member: String,
    },
}
