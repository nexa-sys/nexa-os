//! Interactive Shell Mode

use super::{CliResult, CliError, OutputFormat};
use super::output::OutputFormatter;
use std::io::{self, Write, BufRead};

/// Interactive shell
pub struct Shell {
    prompt: String,
    formatter: OutputFormatter,
    history: Vec<String>,
}

impl Shell {
    pub fn new() -> Self {
        Self {
            prompt: "nvm> ".to_string(),
            formatter: OutputFormatter::new(OutputFormat::Table),
            history: Vec::new(),
        }
    }

    /// Run interactive shell
    pub fn run(&mut self) -> CliResult<()> {
        println!("NVM Interactive Shell");
        println!("Type 'help' for available commands, 'exit' to quit\n");

        let stdin = io::stdin();
        let mut stdout = io::stdout();

        loop {
            print!("{}", self.prompt);
            stdout.flush()?;

            let mut line = String::new();
            if stdin.lock().read_line(&mut line)? == 0 {
                break;
            }

            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            self.history.push(line.to_string());

            match self.execute(line) {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => {
                    self.formatter.error(&e.to_string());
                }
            }
        }

        Ok(())
    }

    /// Execute a shell command
    fn execute(&mut self, line: &str) -> CliResult<bool> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(false);
        }

        let cmd = parts[0];
        let args = &parts[1..];

        match cmd {
            "exit" | "quit" | "q" => {
                println!("Goodbye!");
                return Ok(true);
            }
            "help" | "?" => {
                self.print_help();
            }
            "history" => {
                for (i, cmd) in self.history.iter().enumerate() {
                    println!("{:4}  {}", i + 1, cmd);
                }
            }
            "clear" => {
                print!("\x1B[2J\x1B[1;1H");
            }
            "vm" => {
                self.handle_vm(args)?;
            }
            "storage" => {
                self.handle_storage(args)?;
            }
            "network" => {
                self.handle_network(args)?;
            }
            "cluster" => {
                self.handle_cluster(args)?;
            }
            "backup" => {
                self.handle_backup(args)?;
            }
            "user" => {
                self.handle_user(args)?;
            }
            "system" => {
                self.handle_system(args)?;
            }
            _ => {
                self.formatter.error(&format!("Unknown command: {}. Type 'help' for available commands.", cmd));
            }
        }

        Ok(false)
    }

    fn print_help(&self) {
        println!("Available commands:");
        println!();
        println!("  VM Management:");
        println!("    vm list                    - List all VMs");
        println!("    vm info <id>               - Show VM details");
        println!("    vm create <name> ...       - Create new VM");
        println!("    vm start <id>              - Start VM");
        println!("    vm stop <id>               - Stop VM");
        println!("    vm restart <id>            - Restart VM");
        println!("    vm delete <id>             - Delete VM");
        println!("    vm snapshot <id> <name>    - Create snapshot");
        println!("    vm clone <id> <name>       - Clone VM");
        println!("    vm migrate <id> <node>     - Migrate VM");
        println!("    vm console <id>            - Open console");
        println!();
        println!("  Storage:");
        println!("    storage pools              - List storage pools");
        println!("    storage volumes            - List volumes");
        println!("    storage create <pool> ...  - Create volume");
        println!();
        println!("  Network:");
        println!("    network list               - List networks");
        println!("    network create <name> ...  - Create network");
        println!();
        println!("  Cluster:");
        println!("    cluster status             - Show cluster status");
        println!("    cluster nodes              - List cluster nodes");
        println!("    cluster join <addr> <tok>  - Join cluster");
        println!();
        println!("  Backup:");
        println!("    backup list                - List backups");
        println!("    backup create <vm> <tgt>   - Create backup");
        println!("    backup restore <id>        - Restore backup");
        println!();
        println!("  System:");
        println!("    system info                - Show system info");
        println!("    system license             - Show license info");
        println!("    system update              - Check for updates");
        println!();
        println!("  Shell:");
        println!("    help, ?                    - Show this help");
        println!("    history                    - Show command history");
        println!("    clear                      - Clear screen");
        println!("    exit, quit, q              - Exit shell");
    }

    fn handle_vm(&self, args: &[&str]) -> CliResult<()> {
        if args.is_empty() {
            self.formatter.error("Usage: vm <list|info|start|stop|...>");
            return Ok(());
        }

        match args[0] {
            "list" | "ls" => {
                let vms = super::commands::vm::list(OutputFormat::Table, false, None)?;
                self.formatter.print(&vms)?;
            }
            "info" if args.len() > 1 => {
                let vm = super::commands::vm::info(args[1])?;
                self.formatter.print(&vm)?;
            }
            "start" if args.len() > 1 => {
                super::commands::vm::start(args[1])?;
                self.formatter.success(&format!("VM {} started", args[1]));
            }
            "stop" if args.len() > 1 => {
                let force = args.get(2) == Some(&"--force");
                super::commands::vm::stop(args[1], force)?;
                self.formatter.success(&format!("VM {} stopped", args[1]));
            }
            "restart" if args.len() > 1 => {
                super::commands::vm::restart(args[1])?;
                self.formatter.success(&format!("VM {} restarted", args[1]));
            }
            "delete" if args.len() > 1 => {
                super::commands::vm::delete(args[1], false)?;
                self.formatter.success(&format!("VM {} deleted", args[1]));
            }
            "snapshot" if args.len() > 2 => {
                let snap_id = super::commands::vm::snapshot(args[1], args[2], None)?;
                self.formatter.success(&format!("Snapshot {} created", snap_id));
            }
            "clone" if args.len() > 2 => {
                let vm_id = super::commands::vm::clone(args[1], args[2], true)?;
                self.formatter.success(&format!("VM cloned: {}", vm_id));
            }
            "migrate" if args.len() > 2 => {
                super::commands::vm::migrate(args[1], args[2], true)?;
                self.formatter.success(&format!("VM {} migrated to {}", args[1], args[2]));
            }
            "console" if args.len() > 1 => {
                let console = super::commands::vm::console(args[1], "vnc")?;
                self.formatter.info(&format!("Console URL: {}", console.url));
            }
            _ => {
                self.formatter.error("Invalid vm command. Type 'help' for usage.");
            }
        }

        Ok(())
    }

    fn handle_storage(&self, args: &[&str]) -> CliResult<()> {
        match args.first().copied() {
            Some("pools") | Some("pool") => {
                let pools = super::commands::storage::list_pools()?;
                self.formatter.print(&pools)?;
            }
            Some("volumes") | Some("vol") => {
                let vols = super::commands::storage::list_volumes(None)?;
                self.formatter.print(&vols)?;
            }
            _ => {
                self.formatter.error("Usage: storage <pools|volumes|create>");
            }
        }
        Ok(())
    }

    fn handle_network(&self, args: &[&str]) -> CliResult<()> {
        match args.first().copied() {
            Some("list") | Some("ls") => {
                let nets = super::commands::network::list()?;
                self.formatter.print(&nets)?;
            }
            _ => {
                self.formatter.error("Usage: network <list|create>");
            }
        }
        Ok(())
    }

    fn handle_cluster(&self, args: &[&str]) -> CliResult<()> {
        match args.first().copied() {
            Some("status") => {
                let status = super::commands::cluster::status()?;
                self.formatter.print(&status)?;
            }
            Some("nodes") => {
                let nodes = super::commands::cluster::list_nodes()?;
                self.formatter.print(&nodes)?;
            }
            _ => {
                self.formatter.error("Usage: cluster <status|nodes|join|leave>");
            }
        }
        Ok(())
    }

    fn handle_backup(&self, args: &[&str]) -> CliResult<()> {
        match args.first().copied() {
            Some("list") | Some("ls") => {
                let backups = super::commands::backup::list(None)?;
                self.formatter.print(&backups)?;
            }
            _ => {
                self.formatter.error("Usage: backup <list|create|restore>");
            }
        }
        Ok(())
    }

    fn handle_user(&self, args: &[&str]) -> CliResult<()> {
        match args.first().copied() {
            Some("list") | Some("ls") => {
                let users = super::commands::user::list()?;
                self.formatter.print(&users)?;
            }
            _ => {
                self.formatter.error("Usage: user <list|create|passwd>");
            }
        }
        Ok(())
    }

    fn handle_system(&self, args: &[&str]) -> CliResult<()> {
        match args.first().copied() {
            Some("info") => {
                let info = super::commands::system::info()?;
                self.formatter.print(&info)?;
            }
            Some("license") => {
                let lic = super::commands::system::license()?;
                self.formatter.print(&lic)?;
            }
            Some("update") => {
                let upd = super::commands::system::update_check()?;
                self.formatter.print(&upd)?;
            }
            _ => {
                self.formatter.error("Usage: system <info|license|update>");
            }
        }
        Ok(())
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}
