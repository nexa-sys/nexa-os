//! NexaOS Edit - A Vim-like text editor with Vim Script support
//!
//! This is a modal text editor inspired by Vim, featuring:
//! - Normal, Insert, Visual, and Command-line modes
//! - Basic Vim Script support for configuration and scripting
//! - Syntax highlighting (basic)
//! - Search and replace
//! - Multiple buffers
//! - Undo/redo support

mod buffer;
mod editor;
mod input;
mod mode;
mod render;
mod terminal;
mod vimscript;

use std::env;
use std::process;

use editor::Editor;

fn print_usage() {
    println!("edit - A Vim-like text editor for NexaOS");
    println!();
    println!("Usage: edit [OPTIONS] [file...]");
    println!();
    println!("Options:");
    println!("  -h, --help     Show this help message");
    println!("  -v, --version  Show version information");
    println!("  -c <command>   Execute Vim command after loading files");
    println!("  -S <script>    Source Vim Script file");
    println!("  +<line>        Start at specified line number");
    println!();
    println!("In Normal mode:");
    println!("  h/j/k/l        Move cursor left/down/up/right");
    println!("  i/a/o          Enter Insert mode");
    println!("  v/V            Enter Visual mode");
    println!("  :              Enter Command-line mode");
    println!("  /              Search forward");
    println!("  ?              Search backward");
    println!("  u              Undo");
    println!("  Ctrl-R         Redo");
    println!("  dd             Delete line");
    println!("  yy             Yank (copy) line");
    println!("  p/P            Paste after/before");
    println!();
    println!("Command-line mode commands:");
    println!("  :w [file]      Write (save) file");
    println!("  :q             Quit");
    println!("  :wq/:x         Write and quit");
    println!("  :q!            Quit without saving");
    println!("  :e <file>      Edit (open) file");
    println!("  :set <opt>     Set option");
    println!("  :source <file> Execute Vim Script");
    println!();
    println!("Vim Script support:");
    println!("  - Variables (let, unlet)");
    println!("  - Conditionals (if/elseif/else/endif)");
    println!("  - Loops (while/endwhile, for/endfor)");
    println!("  - Functions (function/endfunction)");
    println!("  - Built-in functions (strlen, substitute, etc.)");
    println!("  - Mappings (map, nmap, imap, etc.)");
    println!("  - Autocommands (autocmd)");
}

fn print_version() {
    println!("edit version 0.1.0");
    println!("NexaOS Vim-like Editor with Vim Script support");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    let mut files: Vec<String> = Vec::new();
    let mut commands: Vec<String> = Vec::new();
    let mut scripts: Vec<String> = Vec::new();
    let mut start_line: Option<usize> = None;
    
    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        
        if arg == "-h" || arg == "--help" {
            print_usage();
            process::exit(0);
        } else if arg == "-v" || arg == "--version" {
            print_version();
            process::exit(0);
        } else if arg == "-c" {
            i += 1;
            if i < args.len() {
                commands.push(args[i].clone());
            } else {
                eprintln!("edit: -c requires an argument");
                process::exit(1);
            }
        } else if arg == "-S" {
            i += 1;
            if i < args.len() {
                scripts.push(args[i].clone());
            } else {
                eprintln!("edit: -S requires an argument");
                process::exit(1);
            }
        } else if arg.starts_with('+') {
            if let Ok(line) = arg[1..].parse::<usize>() {
                start_line = Some(line);
            }
        } else if !arg.starts_with('-') {
            files.push(arg.clone());
        }
        
        i += 1;
    }
    
    // Create editor instance
    let mut editor = match Editor::new() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("edit: failed to initialize editor: {}", e);
            process::exit(1);
        }
    };
    
    // Load Vim Script files
    for script in &scripts {
        if let Err(e) = editor.source_script(script) {
            eprintln!("edit: error sourcing '{}': {}", script, e);
        }
    }
    
    // Try to load user config
    if let Ok(home) = env::var("HOME") {
        let vimrc = format!("{}/.editrc", home);
        let _ = editor.source_script(&vimrc);
    }
    
    // Load files
    if files.is_empty() {
        // Start with empty buffer
        editor.new_buffer();
    } else {
        for file in &files {
            if let Err(e) = editor.open_file(file) {
                // File doesn't exist - create new buffer with this path
                // This allows creating new files by specifying their path
                if e.kind() == std::io::ErrorKind::NotFound {
                    editor.new_buffer_with_path(file);
                } else {
                    eprintln!("edit: error opening '{}': {}", file, e);
                }
            }
        }
    }
    
    // Ensure at least one buffer exists
    if !editor.has_buffers() {
        editor.new_buffer();
    }
    
    // Jump to line if specified
    if let Some(line) = start_line {
        editor.goto_line(line);
    }
    
    // Execute startup commands
    for cmd in &commands {
        if let Err(e) = editor.execute_command(cmd) {
            eprintln!("edit: error executing '{}': {}", cmd, e);
        }
    }
    
    // Run the main editor loop
    match editor.run() {
        Ok(_) => {}
        Err(e) => {
            eprintln!("edit: {}", e);
            process::exit(1);
        }
    }
}
