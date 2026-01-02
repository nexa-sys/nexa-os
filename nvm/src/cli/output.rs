//! CLI Output Formatting

use super::OutputFormat;
use serde::Serialize;
use std::io::{self, Write};

/// Output formatter
pub struct OutputFormatter {
    format: OutputFormat,
    color: bool,
}

impl OutputFormatter {
    pub fn new(format: OutputFormat) -> Self {
        Self {
            format,
            color: atty::is(atty::Stream::Stdout),
        }
    }

    pub fn with_color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    /// Print data in configured format
    pub fn print<T: Serialize + TablePrint>(&self, data: &T) -> io::Result<()> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        
        match self.format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(data)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                writeln!(handle, "{}", json)?;
            }
            OutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(data)
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
                writeln!(handle, "{}", yaml)?;
            }
            OutputFormat::Table => {
                data.print_table(&mut handle, self.color)?;
            }
            OutputFormat::Csv => {
                data.print_csv(&mut handle)?;
            }
            OutputFormat::Plain => {
                data.print_plain(&mut handle)?;
            }
        }
        
        Ok(())
    }

    /// Print success message
    pub fn success(&self, msg: &str) {
        if self.color {
            println!("\x1b[32m✓\x1b[0m {}", msg);
        } else {
            println!("OK: {}", msg);
        }
    }

    /// Print error message
    pub fn error(&self, msg: &str) {
        if self.color {
            eprintln!("\x1b[31m✗\x1b[0m {}", msg);
        } else {
            eprintln!("ERROR: {}", msg);
        }
    }

    /// Print warning message
    pub fn warning(&self, msg: &str) {
        if self.color {
            println!("\x1b[33m⚠\x1b[0m {}", msg);
        } else {
            println!("WARNING: {}", msg);
        }
    }

    /// Print info message
    pub fn info(&self, msg: &str) {
        if self.color {
            println!("\x1b[34mℹ\x1b[0m {}", msg);
        } else {
            println!("INFO: {}", msg);
        }
    }
}

/// Trait for table-printable types
pub trait TablePrint {
    fn print_table<W: Write>(&self, w: &mut W, color: bool) -> io::Result<()>;
    fn print_csv<W: Write>(&self, w: &mut W) -> io::Result<()>;
    fn print_plain<W: Write>(&self, w: &mut W) -> io::Result<()>;
}

/// Generic table printer for vectors
impl<T: TableRow> TablePrint for Vec<T> {
    fn print_table<W: Write>(&self, w: &mut W, color: bool) -> io::Result<()> {
        if self.is_empty() {
            writeln!(w, "No items found")?;
            return Ok(());
        }

        let headers = T::headers();
        let rows: Vec<Vec<String>> = self.iter().map(|r| r.row()).collect();
        
        // Calculate column widths
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                if i < widths.len() {
                    widths[i] = widths[i].max(cell.len());
                }
            }
        }

        // Print header
        for (i, header) in headers.iter().enumerate() {
            if color {
                write!(w, "\x1b[1m{:width$}\x1b[0m", header, width = widths[i] + 2)?;
            } else {
                write!(w, "{:width$}", header, width = widths[i] + 2)?;
            }
        }
        writeln!(w)?;

        // Print separator
        for width in &widths {
            write!(w, "{:-<width$}  ", "", width = *width)?;
        }
        writeln!(w)?;

        // Print rows
        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                write!(w, "{:width$}", cell, width = widths.get(i).copied().unwrap_or(10) + 2)?;
            }
            writeln!(w)?;
        }

        Ok(())
    }

    fn print_csv<W: Write>(&self, w: &mut W) -> io::Result<()> {
        let headers = T::headers();
        writeln!(w, "{}", headers.join(","))?;
        
        for item in self {
            let row = item.row();
            writeln!(w, "{}", row.join(","))?;
        }
        
        Ok(())
    }

    fn print_plain<W: Write>(&self, w: &mut W) -> io::Result<()> {
        for item in self {
            let row = item.row();
            writeln!(w, "{}", row.join("\t"))?;
        }
        Ok(())
    }
}

/// Trait for types that can be represented as table rows
pub trait TableRow {
    fn headers() -> Vec<&'static str>;
    fn row(&self) -> Vec<String>;
}

/// Implementation for VM info
impl TableRow for super::commands::vm::VmInfo {
    fn headers() -> Vec<&'static str> {
        vec!["ID", "NAME", "STATUS", "VCPUS", "MEMORY", "NODE"]
    }
    
    fn row(&self) -> Vec<String> {
        vec![
            self.id.clone(),
            self.name.clone(),
            self.status.clone(),
            self.vcpus.to_string(),
            format!("{}MB", self.memory_mb),
            self.node.clone(),
        ]
    }
}

/// Single item printer
impl<T: Serialize> TablePrint for T
where
    T: SingleItemPrint,
{
    fn print_table<W: Write>(&self, w: &mut W, color: bool) -> io::Result<()> {
        self.print_single(w, color)
    }

    fn print_csv<W: Write>(&self, w: &mut W) -> io::Result<()> {
        let json = serde_json::to_string(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        writeln!(w, "{}", json)
    }

    fn print_plain<W: Write>(&self, w: &mut W) -> io::Result<()> {
        self.print_single(w, false)
    }
}

/// Trait for single item printing
pub trait SingleItemPrint: Serialize {
    fn print_single<W: Write>(&self, w: &mut W, color: bool) -> io::Result<()> {
        let value = serde_json::to_value(self)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        
        if let serde_json::Value::Object(map) = value {
            for (key, val) in map {
                let val_str = match val {
                    serde_json::Value::String(s) => s,
                    serde_json::Value::Null => "N/A".to_string(),
                    other => other.to_string(),
                };
                
                if color {
                    writeln!(w, "\x1b[1m{:20}\x1b[0m {}", key, val_str)?;
                } else {
                    writeln!(w, "{:20} {}", key, val_str)?;
                }
            }
        }
        
        Ok(())
    }
}

// Implement for VM details
impl SingleItemPrint for super::commands::vm::VmDetails {}
impl SingleItemPrint for super::commands::cluster::ClusterStatus {}
impl SingleItemPrint for super::commands::system::SystemInfo {}
impl SingleItemPrint for super::commands::system::LicenseInfo {}
impl SingleItemPrint for super::commands::system::UpdateInfo {}

// TableRow implementations for list types
impl TableRow for super::commands::storage::PoolInfo {
    fn headers() -> Vec<&'static str> {
        vec!["NAME", "TYPE", "TOTAL", "USED", "STATUS"]
    }
    fn row(&self) -> Vec<String> {
        vec![
            self.name.clone(),
            self.pool_type.clone(),
            format!("{}GB", self.total_gb),
            format!("{}GB", self.used_gb),
            self.status.clone(),
        ]
    }
}

impl TableRow for super::commands::storage::VolumeInfo {
    fn headers() -> Vec<&'static str> {
        vec!["ID", "NAME", "POOL", "SIZE", "FORMAT"]
    }
    fn row(&self) -> Vec<String> {
        vec![
            self.id.clone(),
            self.name.clone(),
            self.pool.clone(),
            format!("{}GB", self.size_gb),
            self.format.clone(),
        ]
    }
}

impl TableRow for super::commands::network::NetworkInfo {
    fn headers() -> Vec<&'static str> {
        vec!["NAME", "TYPE", "CIDR", "STATUS"]
    }
    fn row(&self) -> Vec<String> {
        vec![
            self.name.clone(),
            self.network_type.clone(),
            self.cidr.clone().unwrap_or_default(),
            self.status.clone(),
        ]
    }
}

impl TableRow for super::commands::cluster::NodeInfo {
    fn headers() -> Vec<&'static str> {
        vec!["ID", "HOSTNAME", "STATUS", "ROLE"]
    }
    fn row(&self) -> Vec<String> {
        vec![
            self.id.clone(),
            self.hostname.clone(),
            self.status.clone(),
            self.role.clone(),
        ]
    }
}

impl TableRow for super::commands::backup::BackupInfo {
    fn headers() -> Vec<&'static str> {
        vec!["ID", "VM_ID", "TYPE", "SIZE", "CREATED"]
    }
    fn row(&self) -> Vec<String> {
        vec![
            self.id.clone(),
            self.vm_id.clone(),
            self.backup_type.clone(),
            format!("{:.1}GB", self.size_gb),
            self.created_at.to_string(),
        ]
    }
}

impl TableRow for super::commands::user::UserInfo {
    fn headers() -> Vec<&'static str> {
        vec!["USERNAME", "ROLES", "ENABLED"]
    }
    fn row(&self) -> Vec<String> {
        vec![
            self.username.clone(),
            self.roles.join(","),
            self.enabled.to_string(),
        ]
    }
}
