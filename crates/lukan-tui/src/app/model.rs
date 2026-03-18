use super::helpers::build_welcome_banner;
use super::*;

impl App {
    /// Open the interactive model picker
    pub(super) async fn open_model_picker(&mut self) {
        let models = match ConfigManager::get_models().await {
            Ok(m) => m,
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load models: {e}"),
                ));
                return;
            }
        };

        if models.is_empty() {
            self.messages.push(ChatMessage::new(
                "system",
                "No models available. Run 'lukan setup' to configure providers.",
            ));
            return;
        }

        let current = format!(
            "{}:{}",
            self.config.config.provider,
            self.config.effective_model().unwrap_or_default()
        );

        // Pre-select the current model
        let selected = models.iter().position(|m| *m == current).unwrap_or(0);

        self.model_picker = Some(ModelPicker {
            models,
            selected,
            current,
        });
    }

    /// Switch to the selected model from the picker.
    /// For codex models, opens the reasoning effort picker first.
    pub(super) async fn select_model(&mut self, idx: usize) {
        let picker = self.model_picker.as_ref().unwrap();
        let entry = picker.models[idx].clone();

        let Some((provider_str, _model_name)) = entry.split_once(':') else {
            self.messages.push(ChatMessage::new(
                "system",
                format!("Invalid model format: {entry}"),
            ));
            return;
        };

        // For codex models, show reasoning effort picker first
        if provider_str == "openai-codex" {
            let current_effort = self.provider.reasoning_effort().unwrap_or("medium");
            let default_idx = match current_effort {
                "low" => 0,
                "high" => 2,
                "extra_high" => 3,
                _ => 1, // medium
            };
            self.reasoning_picker = Some(ReasoningPicker {
                model_entry: entry,
                levels: vec![
                    ("low", "Low", "Fast responses with lighter reasoning"),
                    (
                        "medium",
                        "Medium (default)",
                        "Balances speed and reasoning depth",
                    ),
                    (
                        "high",
                        "High",
                        "Greater reasoning depth for complex problems",
                    ),
                    ("extra_high", "Extra high", "Maximum reasoning depth"),
                ],
                selected: default_idx,
            });
            self.model_picker = None;
            return;
        }

        // Non-codex: switch immediately
        self.apply_model_switch(&entry).await;
    }

    /// Set the selected model as the default (persisted to config.json) and switch to it.
    pub(super) async fn set_default_model(&mut self, idx: usize) {
        let picker = self.model_picker.as_ref().unwrap();
        let entry = picker.models[idx].clone();

        let Some((provider_str, model_name)) = entry.split_once(':') else {
            self.messages.push(ChatMessage::new(
                "system",
                format!("Invalid model format: {entry}"),
            ));
            return;
        };

        // Update config and persist
        let provider_name: ProviderName =
            match serde_json::from_value(serde_json::Value::String(provider_str.to_string())) {
                Ok(p) => p,
                Err(_) => {
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Unknown provider: {provider_str}"),
                    ));
                    return;
                }
            };

        self.config.config.provider = provider_name;
        self.config.config.model = Some(model_name.to_string());

        if let Err(e) = ConfigManager::save(&self.config.config).await {
            self.messages.push(ChatMessage::new(
                "system",
                format!("Failed to save config: {e}"),
            ));
            return;
        }

        // Switch to the model
        self.apply_model_switch(&entry).await;
        self.messages.push(ChatMessage::new(
            "system",
            format!("Default model set to {entry}"),
        ));
    }

    /// Apply the model switch after all selections are done.
    pub(super) async fn apply_model_switch(&mut self, entry: &str) {
        self.apply_model_switch_with_effort(entry, None).await;
    }

    /// Apply the model switch, optionally setting reasoning effort.
    pub(super) async fn apply_model_switch_with_effort(
        &mut self,
        entry: &str,
        reasoning_effort: Option<&str>,
    ) {
        let Some((provider_str, model_name)) = entry.split_once(':') else {
            self.messages.push(ChatMessage::new(
                "system",
                format!("Invalid model format: {entry}"),
            ));
            return;
        };

        let provider_name: ProviderName =
            match serde_json::from_value(serde_json::Value::String(provider_str.to_string())) {
                Ok(p) => p,
                Err(_) => {
                    self.messages.push(ChatMessage::new(
                        "system",
                        format!("Unknown provider: {provider_str}"),
                    ));
                    return;
                }
            };

        let credentials = match CredentialsManager::load().await {
            Ok(c) => c,
            Err(e) => {
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Failed to load credentials: {e}"),
                ));
                return;
            }
        };

        let mut new_config = self.config.clone();
        new_config.config.provider = provider_name;
        new_config.config.model = Some(model_name.to_string());
        new_config.credentials = credentials;

        match create_provider(&new_config) {
            Ok(new_provider) => {
                if let Some(effort) = reasoning_effort {
                    new_provider.set_reasoning_effort(effort);
                }
                self.provider = Arc::from(new_provider);
                self.config = new_config;
                // Swap provider in existing agent to preserve history,
                // or reset if no agent exists yet.
                if let Some(agent) = self.agent.as_mut() {
                    agent.swap_provider(Arc::clone(&self.provider));
                }
                if let Some(banner) = self.messages.iter_mut().find(|m| m.role == "banner") {
                    let new_banner = build_welcome_banner(
                        self.provider.name(),
                        &self
                            .config
                            .effective_model()
                            .unwrap_or_else(|| "(no model selected)".to_string()),
                    );
                    banner.content = sanitize_for_display(&new_banner);
                }
                let effort_label = reasoning_effort
                    .map(|e| format!(" (reasoning: {e})"))
                    .unwrap_or_default();
                self.messages.push(ChatMessage::new(
                    "system",
                    format!("Switched to {entry}{effort_label}"),
                ));

                // Notify daemon so it uses the new model for agent creation
                if let Some(ref daemon) = self.daemon_tx {
                    let _ = daemon.send(&crate::ws_client::OutMessage::SetModel {
                        model: entry.to_string(),
                    });
                }
            }
            Err(e) => {
                self.messages
                    .push(ChatMessage::new("system", format!("Failed to switch: {e}")));
            }
        }
    }
}
