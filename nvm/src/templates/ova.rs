//! OVA/OVF Import/Export Support
//!
//! Handles Open Virtual Appliance (OVA) and Open Virtualization Format (OVF)
//! for interoperability with VMware, VirtualBox, etc.

use super::*;
use std::path::Path;
use std::io::{Read, Write};

/// OVA importer
pub struct OvaImporter {
    /// Temporary extraction directory
    temp_dir: std::path::PathBuf,
}

impl OvaImporter {
    pub fn new() -> Self {
        Self {
            temp_dir: std::env::temp_dir().join("nvm-ova-import"),
        }
    }

    /// Import OVA file
    pub fn import(&self, ova_path: &Path, library: &TemplateLibrary) -> Result<ImportResult, TemplateError> {
        // Create temp directory
        std::fs::create_dir_all(&self.temp_dir)?;

        // Extract OVA (tar archive)
        let extracted = self.extract_ova(ova_path)?;

        // Find and parse OVF descriptor
        let ovf_path = extracted
            .iter()
            .find(|p| p.extension().map_or(false, |e| e == "ovf"))
            .ok_or_else(|| TemplateError::Import("No OVF descriptor found".to_string()))?;

        let ovf = self.parse_ovf(ovf_path)?;

        // Create template from OVF
        let template_id = format!("tmpl-{}", uuid::Uuid::new_v4());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // Convert disks
        let mut converted_disks = Vec::new();
        let mut warnings = Vec::new();
        let mut template_disks = Vec::new();

        for disk in &ovf.disks {
            let src_path = self.temp_dir.join(&disk.file_ref);
            if src_path.exists() {
                let target_name = format!("{}.qcow2", disk.id);
                converted_disks.push(target_name.clone());
                
                template_disks.push(TemplateDisk {
                    id: disk.id.clone(),
                    name: disk.id.clone(),
                    size: disk.capacity,
                    format: DiskFormat::Qcow2,
                    path: target_name,
                    bootable: disk.boot_order == Some(1),
                    bus: DiskBus::Virtio,
                    checksum: None,
                });
            } else {
                warnings.push(format!("Disk file not found: {}", disk.file_ref));
            }
        }

        let template = Template {
            id: template_id.clone(),
            name: ovf.name.clone(),
            description: ovf.description.clone().unwrap_or_default(),
            version: ovf.version.clone().unwrap_or_else(|| "1.0".to_string()),
            os_type: map_os_type(&ovf.os_type),
            os_variant: ovf.os_type.clone(),
            arch: Architecture::X86_64,
            default_config: TemplateConfig {
                vcpus: ovf.vcpus,
                min_vcpus: 1,
                max_vcpus: 64,
                memory_mb: ovf.memory_mb,
                min_memory_mb: 512,
                max_memory_mb: 262144,
                ..Default::default()
            },
            disks: template_disks,
            format: TemplateFormat::Ova,
            created_at: now,
            updated_at: now,
            size: 0,
            checksum: None,
            properties: ovf.properties.clone(),
            tags: vec!["imported".to_string(), "ova".to_string()],
            public: false,
            owner: None,
        };

        library.add(template)?;

        // Cleanup
        let _ = std::fs::remove_dir_all(&self.temp_dir);

        Ok(ImportResult {
            template_id,
            name: ovf.name,
            warnings,
            converted_disks,
        })
    }

    fn extract_ova(&self, ova_path: &Path) -> Result<Vec<std::path::PathBuf>, TemplateError> {
        // OVA is a tar archive
        let file = std::fs::File::open(ova_path)?;
        let mut archive = tar::Archive::new(file);

        let mut extracted = Vec::new();
        
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = self.temp_dir.join(entry.path()?);
            
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            
            entry.unpack(&path)?;
            extracted.push(path);
        }

        Ok(extracted)
    }

    fn parse_ovf(&self, ovf_path: &Path) -> Result<OvfDescriptor, TemplateError> {
        let content = std::fs::read_to_string(ovf_path)?;
        
        // Simple XML parsing (in production use proper XML parser)
        // This is a simplified parser for demonstration
        let descriptor = OvfDescriptor {
            name: extract_xml_value(&content, "Name")
                .unwrap_or_else(|| "Imported VM".to_string()),
            description: extract_xml_value(&content, "Description"),
            version: extract_xml_value(&content, "Version"),
            os_type: extract_xml_value(&content, "OperatingSystemType")
                .unwrap_or_else(|| "Linux".to_string()),
            vcpus: extract_xml_value(&content, "VirtualQuantity")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2),
            memory_mb: extract_xml_value(&content, "VirtualQuantity")
                .and_then(|v| v.parse().ok())
                .unwrap_or(2048),
            disks: vec![],
            networks: vec![],
            properties: std::collections::HashMap::new(),
        };

        Ok(descriptor)
    }
}

/// OVF descriptor (parsed)
#[derive(Debug, Clone)]
pub struct OvfDescriptor {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub os_type: String,
    pub vcpus: u32,
    pub memory_mb: u64,
    pub disks: Vec<OvfDisk>,
    pub networks: Vec<OvfNetwork>,
    pub properties: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct OvfDisk {
    pub id: String,
    pub file_ref: String,
    pub capacity: u64,
    pub format: String,
    pub boot_order: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct OvfNetwork {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
}

/// OVA exporter
pub struct OvaExporter;

impl OvaExporter {
    pub fn new() -> Self {
        Self
    }

    /// Export template as OVA
    pub fn export(
        &self,
        template: &Template,
        library: &TemplateLibrary,
        options: &ExportOptions,
    ) -> Result<std::path::PathBuf, TemplateError> {
        let output_path = std::path::PathBuf::from(&options.output_path);
        
        // Create OVF descriptor
        let ovf_content = self.generate_ovf(template)?;
        
        // Create OVA tar archive
        let ova_file = std::fs::File::create(&output_path)?;
        let mut builder = tar::Builder::new(ova_file);

        // Add OVF descriptor
        let ovf_name = format!("{}.ovf", template.name);
        let ovf_bytes = ovf_content.as_bytes();
        let mut header = tar::Header::new_gnu();
        header.set_path(&ovf_name)?;
        header.set_size(ovf_bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, ovf_bytes)?;

        // Add disk images
        let template_path = library.template_path(&template.id);
        for disk in &template.disks {
            let disk_path = template_path.join(&disk.path);
            if disk_path.exists() {
                builder.append_path_with_name(&disk_path, &disk.path)?;
            }
        }

        // Generate manifest if requested
        if options.include_checksum {
            let mf_content = self.generate_manifest(template)?;
            let mf_name = format!("{}.mf", template.name);
            let mf_bytes = mf_content.as_bytes();
            let mut header = tar::Header::new_gnu();
            header.set_path(&mf_name)?;
            header.set_size(mf_bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append(&header, mf_bytes)?;
        }

        builder.finish()?;
        
        Ok(output_path)
    }

    fn generate_ovf(&self, template: &Template) -> Result<String, TemplateError> {
        let mut ovf = String::new();
        
        ovf.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>
<Envelope xmlns="http://schemas.dmtf.org/ovf/envelope/2"
          xmlns:ovf="http://schemas.dmtf.org/ovf/envelope/2"
          xmlns:rasd="http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_ResourceAllocationSettingData"
          xmlns:vssd="http://schemas.dmtf.org/wbem/wscim/1/cim-schema/2/CIM_VirtualSystemSettingData">
"#);

        // References section (disk files)
        ovf.push_str("  <References>\n");
        for disk in &template.disks {
            ovf.push_str(&format!(
                "    <File ovf:id=\"{}\" ovf:href=\"{}\"/>\n",
                disk.id, disk.path
            ));
        }
        ovf.push_str("  </References>\n");

        // Disk section
        ovf.push_str("  <DiskSection>\n");
        ovf.push_str("    <Info>Virtual disk information</Info>\n");
        for disk in &template.disks {
            ovf.push_str(&format!(
                "    <Disk ovf:diskId=\"{}\" ovf:capacity=\"{}\" ovf:format=\"{}\"/>\n",
                disk.id,
                disk.size,
                match disk.format {
                    DiskFormat::Vmdk => "http://www.vmware.com/interfaces/specifications/vmdk.html#streamOptimized",
                    _ => "http://www.qemu.org/interfaces/specifications/qcow2.html",
                }
            ));
        }
        ovf.push_str("  </DiskSection>\n");

        // Virtual System
        ovf.push_str(&format!("  <VirtualSystem ovf:id=\"{}\">\n", template.id));
        ovf.push_str(&format!("    <Name>{}</Name>\n", template.name));
        ovf.push_str(&format!("    <Info>{}</Info>\n", template.description));
        
        // OS section
        ovf.push_str(&format!(
            "    <OperatingSystemSection ovf:id=\"{}\">\n",
            match template.os_type {
                OsType::Linux => "101",
                OsType::Windows => "1",
                _ => "0",
            }
        ));
        ovf.push_str(&format!("      <Description>{}</Description>\n", template.os_variant));
        ovf.push_str("    </OperatingSystemSection>\n");

        // Hardware section
        ovf.push_str("    <VirtualHardwareSection>\n");
        ovf.push_str("      <Info>Virtual hardware requirements</Info>\n");
        
        // CPU
        ovf.push_str(&format!(
            "      <Item>
        <rasd:ElementName>CPU</rasd:ElementName>
        <rasd:ResourceType>3</rasd:ResourceType>
        <rasd:VirtualQuantity>{}</rasd:VirtualQuantity>
      </Item>\n",
            template.default_config.vcpus
        ));

        // Memory
        ovf.push_str(&format!(
            "      <Item>
        <rasd:ElementName>Memory</rasd:ElementName>
        <rasd:ResourceType>4</rasd:ResourceType>
        <rasd:VirtualQuantity>{}</rasd:VirtualQuantity>
        <rasd:AllocationUnits>byte * 2^20</rasd:AllocationUnits>
      </Item>\n",
            template.default_config.memory_mb
        ));

        ovf.push_str("    </VirtualHardwareSection>\n");
        ovf.push_str("  </VirtualSystem>\n");
        ovf.push_str("</Envelope>\n");

        Ok(ovf)
    }

    fn generate_manifest(&self, template: &Template) -> Result<String, TemplateError> {
        let mut mf = String::new();
        
        for disk in &template.disks {
            if let Some(checksum) = &disk.checksum {
                mf.push_str(&format!("SHA256({}) = {}\n", disk.path, checksum));
            }
        }
        
        Ok(mf)
    }
}

fn map_os_type(os_str: &str) -> OsType {
    let lower = os_str.to_lowercase();
    if lower.contains("linux") || lower.contains("ubuntu") || lower.contains("centos") 
        || lower.contains("debian") || lower.contains("fedora") || lower.contains("rhel") {
        OsType::Linux
    } else if lower.contains("windows") {
        OsType::Windows
    } else if lower.contains("freebsd") {
        OsType::FreeBsd
    } else {
        OsType::Other
    }
}

fn extract_xml_value(xml: &str, tag: &str) -> Option<String> {
    let start_tag = format!("<{}>", tag);
    let end_tag = format!("</{}>", tag);
    
    xml.find(&start_tag)
        .and_then(|start| {
            let content_start = start + start_tag.len();
            xml[content_start..].find(&end_tag)
                .map(|end| xml[content_start..content_start + end].trim().to_string())
        })
}

impl Default for OvaImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for OvaExporter {
    fn default() -> Self {
        Self::new()
    }
}
