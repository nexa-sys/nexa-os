//! env - Display environment variables or run a command in modified environment
//!
//! Usage:
//!   env [OPTION]... [-] [NAME=VALUE]... [COMMAND [ARG]...]

use std::env;
use std::process::{self, Command};

fn print_usage() {
    println!("env - Display environment variables or run a command in modified environment");
    println!();
    println!("Usage: env [OPTION]... [-] [NAME=VALUE]... [COMMAND [ARG]...]");
    println!();
    println!("Options:");
    println!("  -i, --ignore-environment  Start with an empty environment");
    println!("  -u, --unset=NAME          Remove variable from the environment");
    println!("  -0, --null                End each output line with NUL, not newline");
    println!("  -h, --help                Show this help message");
    println!();
    println!("Without COMMAND, print the environment.");
    println!("With COMMAND, run it with the modified environment.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let mut ignore_env = false;
    let mut null_terminated = false;
    let mut unset_vars: Vec<String> = Vec::new();
    let mut set_vars: Vec<(String, String)> = Vec::new();
    let mut command_args: Vec<&str> = Vec::new();
    let mut parsing_options = true;
    
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        
        if parsing_options {
            if arg == "-h" || arg == "--help" {
                print_usage();
                process::exit(0);
            } else if arg == "-i" || arg == "--ignore-environment" {
                ignore_env = true;
            } else if arg == "-0" || arg == "--null" {
                null_terminated = true;
            } else if arg == "-" {
                ignore_env = true;
            } else if arg == "-u" {
                i += 1;
                if i >= args.len() {
                    eprintln!("env: option '-u' requires an argument");
                    process::exit(125);
                }
                unset_vars.push(args[i].clone());
            } else if arg.starts_with("-u") {
                unset_vars.push(arg[2..].to_string());
            } else if arg.starts_with("--unset=") {
                unset_vars.push(arg[8..].to_string());
            } else if arg.starts_with('-') {
                eprintln!("env: unknown option: {}", arg);
                process::exit(125);
            } else if arg.contains('=') {
                // VAR=VALUE
                let parts: Vec<&str> = arg.splitn(2, '=').collect();
                set_vars.push((parts[0].to_string(), parts[1].to_string()));
            } else {
                // Start of command
                parsing_options = false;
                command_args.push(arg);
            }
        } else {
            command_args.push(arg);
        }
        i += 1;
    }

    // If no command, just print environment
    if command_args.is_empty() {
        let terminator = if null_terminated { '\0' } else { '\n' };
        
        if ignore_env {
            // Only print set_vars
            for (name, value) in &set_vars {
                print!("{}={}{}", name, value, terminator);
            }
        } else {
            // Print current environment minus unset, plus set
            for (key, value) in env::vars() {
                if unset_vars.contains(&key) {
                    continue;
                }
                // Check if overridden
                let final_value = set_vars.iter()
                    .find(|(k, _)| k == &key)
                    .map(|(_, v)| v.as_str())
                    .unwrap_or(&value);
                print!("{}={}{}", key, final_value, terminator);
            }
            
            // Print new variables
            for (name, value) in &set_vars {
                if env::var(name).is_err() {
                    print!("{}={}{}", name, value, terminator);
                }
            }
        }
        process::exit(0);
    }

    // Execute command with modified environment
    let program = command_args[0];
    let mut cmd = Command::new(program);
    
    if ignore_env {
        cmd.env_clear();
    } else {
        // Unset specified variables
        for var in &unset_vars {
            cmd.env_remove(var);
        }
    }
    
    // Set new variables
    for (name, value) in &set_vars {
        cmd.env(name, value);
    }
    
    // Add command arguments
    cmd.args(&command_args[1..]);
    
    // Execute
    match cmd.status() {
        Ok(status) => {
            process::exit(status.code().unwrap_or(1));
        }
        Err(e) => {
            eprintln!("env: {}: {}", program, e);
            process::exit(if e.kind() == std::io::ErrorKind::NotFound { 127 } else { 126 });
        }
    }
}
