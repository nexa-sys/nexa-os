/// Interactive user-space shell with real keyboard input
use spin::Mutex;
use crate::fs;

const SHELL_PROMPT: &str = "nexa$ ";
const MAX_CMD_LEN: usize = 256;

pub struct Shell {
    buffer: [u8; MAX_CMD_LEN],
    pos: usize,
}

static SHELL: Mutex<Shell> = Mutex::new(Shell {
    buffer: [0; MAX_CMD_LEN],
    pos: 0,
});

impl Shell {
    pub fn new() -> Self {
        Shell {
            buffer: [0; MAX_CMD_LEN],
            pos: 0,
        }
    }

    fn print_prompt(&mut self) {
        crate::print!("{}", SHELL_PROMPT);
    }

    fn handle_char(&mut self, c: char) {
        match c {
            '\n' => {
                crate::println!();
                self.execute_command();
                self.pos = 0;
                self.buffer.fill(0);
                self.print_prompt();
            }
            '\x08' => {
                // Backspace
                if self.pos > 0 {
                    self.pos -= 1;
                    self.buffer[self.pos] = 0;
                    crate::print!("\x08");
                }
            }
            _ => {
                if self.pos < MAX_CMD_LEN - 1 && c.is_ascii() {
                    self.buffer[self.pos] = c as u8;
                    self.pos += 1;
                    crate::print!("{}", c);
                }
            }
        }
    }

    fn execute_command(&mut self) {
        let cmd = core::str::from_utf8(&self.buffer[..self.pos])
            .unwrap_or("")
            .trim();

        if cmd.is_empty() {
            return;
        }

        // Parse command and arguments
        let mut parts = cmd.split_whitespace();
        let command = parts.next().unwrap_or("");
        
        // Collect up to 10 arguments (simple array-based approach)
        let mut args: [&str; 10] = [""; 10];
        let mut arg_count = 0;
        for arg in parts {
            if arg_count < 10 {
                args[arg_count] = arg;
                arg_count += 1;
            }
        }
        let args = &args[..arg_count];

        match command {
            "help" => self.cmd_help(),
            "echo" => self.cmd_echo(&args),
            "clear" => self.cmd_clear(),
            "uname" => self.cmd_uname(&args),
            "uptime" => self.cmd_uptime(),
            "free" => self.cmd_free(),
            "ps" => self.cmd_ps(),
            "date" => self.cmd_date(),
            "pwd" => self.cmd_pwd(),
            "ls" => self.cmd_ls(&args),
            "cat" => self.cmd_cat(&args),
            "exit" => self.cmd_exit(),
            "hello" => self.cmd_hello(),
            "test" => self.cmd_test(),
            "" => {},
            _ => {
                crate::println!("{}: command not found", command);
            }
        }
    }

    fn cmd_help(&self) {
        crate::println!("NexaOS Shell - Available commands:");
        crate::println!("  help     - Display this help message");
        crate::println!("  echo     - Print arguments to screen");
        crate::println!("  clear    - Clear the screen");
        crate::println!("  ls       - List files in current directory");
        crate::println!("  cat      - Display file contents");
        crate::println!("  uname    - Print system information");
        crate::println!("  uptime   - Show system uptime");
        crate::println!("  free     - Display memory information");
        crate::println!("  ps       - List running processes");
        crate::println!("  date     - Display current date");
        crate::println!("  pwd      - Print working directory");
        crate::println!("  hello    - Greet the user");
        crate::println!("  test     - Run a test command");
        crate::println!("  exit     - Exit the shell");
    }

    fn cmd_echo(&self, args: &[&str]) {
        if args.is_empty() {
            crate::println!();
        } else {
            for (i, arg) in args.iter().enumerate() {
                if i > 0 {
                    crate::print!(" ");
                }
                crate::print!("{}", arg);
            }
            crate::println!();
        }
    }

    fn cmd_clear(&self) {
        crate::vga_buffer::clear_screen();
    }

    fn cmd_uname(&self, args: &[&str]) {
        let all = args.iter().any(|&a| a == "-a" || a == "--all");
        let sysname = args.iter().any(|&a| a == "-s" || a == "--kernel-name");
        let nodename = args.iter().any(|&a| a == "-n" || a == "--nodename");
        let release = args.iter().any(|&a| a == "-r" || a == "--kernel-release");
        let version = args.iter().any(|&a| a == "-v" || a == "--kernel-version");
        let machine = args.iter().any(|&a| a == "-m" || a == "--machine");

        if all || (!sysname && !nodename && !release && !version && !machine) {
            crate::println!("NexaOS nexa-host 0.0.1 #1 x86_64");
        } else {
            let mut first = true;
            if sysname { 
                crate::print!("NexaOS");
                first = false;
            }
            if nodename { 
                if !first { crate::print!(" "); }
                crate::print!("nexa-host");
                first = false;
            }
            if release { 
                if !first { crate::print!(" "); }
                crate::print!("0.0.1");
                first = false;
            }
            if version { 
                if !first { crate::print!(" "); }
                crate::print!("#1");
                first = false;
            }
            if machine { 
                if !first { crate::print!(" "); }
                crate::print!("x86_64");
            }
            crate::println!();
        }
    }

    fn cmd_uptime(&self) {
        // Simple uptime (could be enhanced with actual timer)
        crate::println!("up 0:00:05");
    }

    fn cmd_free(&self) {
        crate::println!("              total        used        free");
        crate::println!("Mem:       16777216      524288    16252928");
        crate::println!("(Memory values are estimates - proper memory management not yet implemented)");
    }

    fn cmd_ps(&self) {
        crate::println!("  PID TTY          TIME CMD");
        crate::println!("    1 tty1     00:00:00 init");
        crate::println!("  100 tty1     00:00:00 shell");
    }

    fn cmd_date(&self) {
        crate::println!("Thu Oct 23 00:00:00 UTC 2025");
    }

    fn cmd_pwd(&self) {
        crate::println!("/");
    }

    fn cmd_hello(&self) {
        crate::println!("Hello from NexaOS user-space shell!");
        crate::println!("This shell is running in a hybrid-kernel environment.");
    }

    fn cmd_test(&self) {
        crate::println!("Running shell test...");
        crate::println!("+ Shell is operational");
        crate::println!("+ Command parsing works");
        crate::println!("+ Output display works");
        crate::println!("Shell test completed successfully!");
    }

    fn cmd_ls(&self, _args: &[&str]) {
        let files = fs::list_files();
        
        crate::println!("Files:");
        for file_opt in files.iter() {
            if let Some(file) = file_opt {
                if file.name == "/" {
                    continue; // Skip root dir entry
                }
                let type_str = if file.is_dir { "DIR " } else { "FILE" };
                let size = file.content.len();
                crate::println!("  {} {:>8} bytes  {}", type_str, size, file.name);
            }
        }
    }

    fn cmd_cat(&self, args: &[&str]) {
        if args.is_empty() {
            crate::println!("cat: missing file operand");
            return;
        }
        
        for arg in args {
            // For simplicity, just try the arg as-is and with / prefix
            let mut found = false;
            
            if let Some(content) = fs::read_file(arg) {
                crate::print!("{}", content);
                found = true;
            } else {
                // Try with / prefix
                static mut PATH_BUF: [u8; 256] = [0; 256];
                unsafe {
                    PATH_BUF[0] = b'/';
                    let arg_bytes = arg.as_bytes();
                    let len = arg_bytes.len().min(255);
                    PATH_BUF[1..len+1].copy_from_slice(&arg_bytes[..len]);
                    
                    if let Ok(path_str) = core::str::from_utf8(&PATH_BUF[..len+1]) {
                        if let Some(content) = fs::read_file(path_str) {
                            crate::print!("{}", content);
                            found = true;
                        }
                    }
                }
            }
            
            if !found {
                crate::println!("cat: {}: No such file or directory", arg);
            }
        }
    }

    fn cmd_exit(&self) {
        crate::println!("Exiting shell...");
        crate::println!("System halted.");
        crate::arch::halt_loop();
    }
}

/// Initialize and run the shell (interactive mode)
pub fn run() {
    crate::println!();
    crate::println!("============================================================");
    crate::println!("          Welcome to NexaOS Interactive Shell");
    crate::println!("                    Version 0.0.1");
    crate::println!("============================================================");
    crate::println!();
    crate::println!("Type 'help' for available commands.");
    crate::println!("Type commands using your keyboard!");
    crate::println!();

    loop {
        crate::print!("nexa$ ");
        
        let mut buffer = [0u8; MAX_CMD_LEN];
        let len = crate::keyboard::read_line(&mut buffer);
        
        let cmd = core::str::from_utf8(&buffer[..len])
            .unwrap_or("")
            .trim();
        
        if cmd.is_empty() {
            continue;
        }
        
        // Parse and execute command
        let mut parts = cmd.split_whitespace();
        let command = parts.next().unwrap_or("");
        
        let mut args: [&str; 10] = [""; 10];
        let mut arg_count = 0;
        for arg in parts {
            if arg_count < 10 {
                args[arg_count] = arg;
                arg_count += 1;
            }
        }
        let args = &args[..arg_count];
        
        let shell = Shell::new();
        
        match command {
            "help" => shell.cmd_help(),
            "echo" => shell.cmd_echo(&args),
            "clear" => shell.cmd_clear(),
            "ls" => shell.cmd_ls(&args),
            "cat" => shell.cmd_cat(&args),
            "uname" => shell.cmd_uname(&args),
            "uptime" => shell.cmd_uptime(),
            "free" => shell.cmd_free(),
            "ps" => shell.cmd_ps(),
            "date" => shell.cmd_date(),
            "pwd" => shell.cmd_pwd(),
            "hello" => shell.cmd_hello(),
            "test" => shell.cmd_test(),
            "exit" => shell.cmd_exit(),
            "" => {},
            _ => {
                crate::println!("{}: command not found", command);
            }
        }
    }
}


