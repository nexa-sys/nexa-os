//! Template Library - Storage and management of VM templates

use super::*;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;

/// Template library
pub struct TemplateLibrary {
    /// Templates by ID
    templates: RwLock<HashMap<TemplateId, Template>>,
    /// Storage path
    storage_path: PathBuf,
    /// Index file path
    index_path: PathBuf,
}

impl TemplateLibrary {
    /// Create new template library
    pub fn new(storage_path: PathBuf) -> Self {
        let index_path = storage_path.join("index.json");
        
        let lib = Self {
            templates: RwLock::new(HashMap::new()),
            storage_path,
            index_path,
        };
        
        // Load existing index
        let _ = lib.load_index();
        
        lib
    }

    /// Load index from disk
    fn load_index(&self) -> Result<(), TemplateError> {
        if !self.index_path.exists() {
            return Ok(());
        }
        
        let content = std::fs::read_to_string(&self.index_path)?;
        let templates: HashMap<TemplateId, Template> = serde_json::from_str(&content)
            .map_err(|e| TemplateError::Serialization(e.to_string()))?;
        
        *self.templates.write() = templates;
        Ok(())
    }

    /// Save index to disk
    fn save_index(&self) -> Result<(), TemplateError> {
        std::fs::create_dir_all(&self.storage_path)?;
        
        let templates = self.templates.read();
        let content = serde_json::to_string_pretty(&*templates)
            .map_err(|e| TemplateError::Serialization(e.to_string()))?;
        
        std::fs::write(&self.index_path, content)?;
        Ok(())
    }

    /// List all templates
    pub fn list(&self) -> Vec<Template> {
        self.templates.read().values().cloned().collect()
    }

    /// Get template by ID
    pub fn get(&self, id: &str) -> Option<Template> {
        self.templates.read().get(id).cloned()
    }

    /// Search templates
    pub fn search(&self, query: &TemplateQuery) -> Vec<Template> {
        self.templates
            .read()
            .values()
            .filter(|t| {
                // Name/description search
                if let Some(text) = &query.text {
                    let text_lower = text.to_lowercase();
                    if !t.name.to_lowercase().contains(&text_lower)
                        && !t.description.to_lowercase().contains(&text_lower)
                    {
                        return false;
                    }
                }
                
                // OS type filter
                if let Some(os) = &query.os_type {
                    if t.os_type != *os {
                        return false;
                    }
                }
                
                // Architecture filter
                if let Some(arch) = &query.arch {
                    if t.arch != *arch {
                        return false;
                    }
                }
                
                // Tag filter
                if let Some(tags) = &query.tags {
                    for tag in tags {
                        if !t.tags.contains(tag) {
                            return false;
                        }
                    }
                }
                
                // Public filter
                if let Some(public) = query.public {
                    if t.public != public {
                        return false;
                    }
                }
                
                true
            })
            .cloned()
            .collect()
    }

    /// Add template
    pub fn add(&self, template: Template) -> Result<TemplateId, TemplateError> {
        let id = template.id.clone();
        
        if self.templates.read().contains_key(&id) {
            return Err(TemplateError::AlreadyExists(id));
        }
        
        // Create template directory
        let template_dir = self.storage_path.join(&id);
        std::fs::create_dir_all(&template_dir)?;
        
        // Save template metadata
        let meta_path = template_dir.join("metadata.json");
        let meta_content = serde_json::to_string_pretty(&template)
            .map_err(|e| TemplateError::Serialization(e.to_string()))?;
        std::fs::write(meta_path, meta_content)?;
        
        self.templates.write().insert(id.clone(), template);
        self.save_index()?;
        
        Ok(id)
    }

    /// Update template metadata
    pub fn update(&self, id: &str, updates: TemplateUpdate) -> Result<(), TemplateError> {
        let mut templates = self.templates.write();
        
        let template = templates.get_mut(id)
            .ok_or_else(|| TemplateError::NotFound(id.to_string()))?;
        
        if let Some(name) = updates.name {
            template.name = name;
        }
        if let Some(desc) = updates.description {
            template.description = desc;
        }
        if let Some(tags) = updates.tags {
            template.tags = tags;
        }
        if let Some(public) = updates.public {
            template.public = public;
        }
        if let Some(props) = updates.properties {
            template.properties.extend(props);
        }
        
        template.updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        drop(templates);
        self.save_index()?;
        
        Ok(())
    }

    /// Delete template
    pub fn delete(&self, id: &str) -> Result<(), TemplateError> {
        if self.templates.write().remove(id).is_none() {
            return Err(TemplateError::NotFound(id.to_string()));
        }
        
        // Delete template directory
        let template_dir = self.storage_path.join(id);
        if template_dir.exists() {
            std::fs::remove_dir_all(&template_dir)?;
        }
        
        self.save_index()?;
        Ok(())
    }

    /// Deploy template to create VM
    pub fn deploy(&self, id: &str, options: DeployOptions) -> Result<String, TemplateError> {
        let template = self.get(id)
            .ok_or_else(|| TemplateError::NotFound(id.to_string()))?;
        
        // Generate VM ID
        let vm_id = format!("vm-{}", uuid::Uuid::new_v4());
        
        // In production: copy disks, create VM config, etc.
        // This is a placeholder
        
        Ok(vm_id)
    }

    /// Create template from existing VM
    pub fn create_from_vm(&self, vm_id: &str, options: CreateTemplateOptions) -> Result<TemplateId, TemplateError> {
        let template_id = format!("tmpl-{}", uuid::Uuid::new_v4());
        
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        
        let template = Template {
            id: template_id.clone(),
            name: options.name,
            description: options.description.unwrap_or_default(),
            version: "1.0".to_string(),
            os_type: options.os_type.unwrap_or(OsType::Linux),
            os_variant: options.os_variant.unwrap_or_default(),
            arch: Architecture::X86_64,
            default_config: TemplateConfig::default(),
            disks: Vec::new(), // Would be populated from VM
            format: TemplateFormat::Native,
            created_at: now,
            updated_at: now,
            size: 0,
            checksum: None,
            properties: options.properties,
            tags: options.tags,
            public: false,
            owner: options.owner,
        };
        
        self.add(template)?;
        Ok(template_id)
    }

    /// Get template storage path
    pub fn template_path(&self, id: &str) -> PathBuf {
        self.storage_path.join(id)
    }
}

/// Template query
#[derive(Debug, Clone, Default)]
pub struct TemplateQuery {
    pub text: Option<String>,
    pub os_type: Option<OsType>,
    pub arch: Option<Architecture>,
    pub tags: Option<Vec<String>>,
    pub public: Option<bool>,
    pub owner: Option<String>,
}

/// Template update
#[derive(Debug, Clone, Default)]
pub struct TemplateUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    pub public: Option<bool>,
    pub properties: Option<HashMap<String, String>>,
}

/// Create template options
#[derive(Debug, Clone)]
pub struct CreateTemplateOptions {
    pub name: String,
    pub description: Option<String>,
    pub os_type: Option<OsType>,
    pub os_variant: Option<String>,
    pub tags: Vec<String>,
    pub properties: HashMap<String, String>,
    pub owner: Option<String>,
}

impl Default for TemplateLibrary {
    fn default() -> Self {
        Self::new(PathBuf::from("/var/lib/nvm/templates"))
    }
}
