use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::core::http_client::HttpClient;
use twilight_model::channel::message::Message;

use crate::commands::{Arguments, BotCommand};

// For registration and discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMetadata {
    pub name: String,
    pub description: String,
    pub category: String,
    pub usage: String,
    pub examples: Vec<String>,
    pub aliases: Vec<String>,
}

// For managing all bot commands
pub struct CommandRegistry {
    commands: RwLock<HashMap<String, Arc<dyn BotCommand>>>,
    metadata: RwLock<HashMap<String, CommandMetadata>>,
    command_aliases: RwLock<HashMap<String, String>>,
    command_categories: RwLock<HashMap<String, Vec<String>>>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: RwLock::new(HashMap::new()),
            metadata: RwLock::new(HashMap::new()),
            command_aliases: RwLock::new(HashMap::new()),
            command_categories: RwLock::new(HashMap::new()),
        }
    }

    // Register a command with the registry
    pub fn register<T>(&self, command: T) -> Result<()>
    where
        T: BotCommand + Send + Sync + 'static,
    {
        let command_arc = Arc::new(command);
        let name = command_arc.name().to_string();

        let metadata = CommandMetadata {
            name: name.clone(),
            description: command_arc.description().to_string(),
            category: command_arc.category().to_string(),
            usage: command_arc.usage().to_string(),
            examples: command_arc
                .examples()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            aliases: command_arc
                .aliases()
                .iter()
                .map(|s| s.to_string())
                .collect(),
        };

        {
            let mut commands = self.commands.write().unwrap();
            commands.insert(name.clone(), command_arc.clone());
        }
        {
            let mut metadata_map = self.metadata.write().unwrap();
            metadata_map.insert(name.clone(), metadata);
        }
        {
            let mut aliases = self.command_aliases.write().unwrap();
            for alias in command_arc.aliases() {
                aliases.insert(alias.to_string(), name.clone());
            }
        }
        {
            let mut categories = self.command_categories.write().unwrap();
            let category = command_arc.category();
            categories
                .entry(category.to_string())
                .or_insert_with(Vec::new)
                .push(name.clone());
        }

        Ok(())
    }

    // Get a command name or alias
    pub fn get_command(&self, name: &str) -> Option<Arc<dyn BotCommand>> {
        let commands = self.commands.read().unwrap();
        let aliases = self.command_aliases.read().unwrap();

        let binding = name.to_string();
        let actual_name = aliases.get(name).unwrap_or(&binding);
        commands.get(actual_name).cloned()
    }

    // Get command metadata
    pub fn get_metadata(&self, name: &str) -> Option<CommandMetadata> {
        let aliases = self.command_aliases.read().unwrap();
        let binding = name.to_string();
        let actual_name = aliases.get(name).unwrap_or(&binding);
        self.metadata.read().unwrap().get(actual_name).cloned()
    }

    // Get commands category
    pub fn get_commands_by_category(&self, category: &str) -> Vec<CommandMetadata> {
        let categories = self.command_categories.read().unwrap();
        let metadata = self.metadata.read().unwrap();

        if let Some(commands) = categories.get(category) {
            commands
                .iter()
                .filter_map(|name| metadata.get(name))
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    // Get all categories
    pub fn get_categories(&self) -> Vec<String> {
        self.command_categories
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    // Execute a command
    pub async fn execute_command(
        &self,
        name: &str,
        http: &Arc<HttpClient>,
        msg: &Message,
        args: Arguments,
    ) -> Result<()> {
        if let Some(command) = self.get_command(name) {
            command.execute(http, msg, args).await
        } else {
            Err(anyhow::anyhow!("Command not found: {}", name))
        }
    }

    pub fn command_exists(&self, name: &str) -> bool {
        let aliases = self.command_aliases.read().unwrap();
        let binding = name.to_string();
        let actual_name = aliases.get(name).unwrap_or(&binding);
        self.commands.read().unwrap().contains_key(actual_name)
    }

    pub fn command_count(&self) -> usize {
        self.commands.read().unwrap().len()
    }
}

static GLOBAL_REGISTRY: once_cell::sync::Lazy<CommandRegistry> =
    once_cell::sync::Lazy::new(CommandRegistry::new);

pub fn get_registry() -> &'static CommandRegistry {
    &GLOBAL_REGISTRY
}

#[macro_export]
macro_rules! register_commands {
    ($registry:expr, $($command:expr),* $(,)?) => {
        {
            $(
                $registry.register($command)?;
            )*
            Ok::<(), anyhow::Error>(())
        }
    };
}
