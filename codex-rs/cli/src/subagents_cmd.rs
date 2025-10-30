use anyhow::Context;
use anyhow::anyhow;
use clap::ArgAction;
use clap::Parser;
use clap::ValueEnum;
use clap::ValueHint;
use codex_common::CliConfigOverrides;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::config::SubagentDefinition;
use codex_exec::Cli as ExecCli;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;
use toml::Value as TomlValue;

#[derive(Debug, Parser)]
pub struct SubagentsCli {
    #[clap(flatten)]
    pub config_overrides: CliConfigOverrides,

    #[clap(flatten)]
    pub toggles: SubagentToggleArgs,

    #[command(subcommand)]
    pub command: SubagentCommand,
}

impl SubagentsCli {
    pub async fn run(
        mut self,
        root_overrides: CliConfigOverrides,
        codex_linux_sandbox_exe: Option<PathBuf>,
    ) -> anyhow::Result<()> {
        // Root-level overrides should have the lowest precedence.
        self.config_overrides
            .raw_overrides
            .splice(0..0, root_overrides.raw_overrides);

        match self.command {
            SubagentCommand::List => {
                let config = load_config(&self.config_overrides, &self.toggles).await?;
                print_subagent_list(&config.subagents);
            }
            SubagentCommand::Show(args) => {
                let config = load_config(&self.config_overrides, &self.toggles).await?;
                show_subagent(&config.subagents, args)?;
            }
            SubagentCommand::Run(args) => {
                run_subagent(
                    args,
                    &self.config_overrides,
                    &self.toggles,
                    codex_linux_sandbox_exe,
                )
                .await?;
            }
        }

        Ok(())
    }
}

#[derive(Debug, Default, Clone, Parser)]
pub struct SubagentToggleArgs {
    /// Enable a subagent for this invocation.
    #[arg(long = "enable", value_name = "SUBAGENT", action = ArgAction::Append)]
    enable: Vec<String>,

    /// Disable a subagent for this invocation.
    #[arg(long = "disable", value_name = "SUBAGENT", action = ArgAction::Append)]
    disable: Vec<String>,
}

impl SubagentToggleArgs {
    fn to_map(&self) -> HashMap<String, bool> {
        let mut toggles = HashMap::new();
        for name in &self.enable {
            toggles.insert(name.clone(), true);
        }
        for name in &self.disable {
            toggles.insert(name.clone(), false);
        }
        toggles
    }

    fn to_cli_overrides(&self) -> Vec<String> {
        let mut overrides = Vec::new();
        for name in &self.enable {
            overrides.push(format!("subagents.{name}.enabled=true"));
        }
        for name in &self.disable {
            overrides.push(format!("subagents.{name}.enabled=false"));
        }
        overrides
    }
}

#[derive(Debug, Parser)]
pub enum SubagentCommand {
    /// List available subagents and their status.
    List,
    /// Show detailed configuration for a subagent.
    Show(SubagentShowArgs),
    /// Run a subagent using the non-interactive executor.
    Run(SubagentRunArgs),
}

#[derive(Debug, Parser)]
pub struct SubagentShowArgs {
    #[arg(value_name = "SUBAGENT")]
    pub name: String,

    #[arg(long = "format", value_enum, default_value_t = SubagentShowFormat::Human)]
    pub format: SubagentShowFormat,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum SubagentShowFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Parser)]
pub struct SubagentRunArgs {
    #[arg(value_name = "SUBAGENT")]
    pub name: String,

    #[arg(value_name = "PROMPT", value_hint = ValueHint::Other)]
    pub prompt: Option<String>,

    #[arg(long = "json", default_value_t = false)]
    pub json: bool,

    #[arg(long = "oss", default_value_t = false)]
    pub oss: bool,

    #[arg(long = "full-auto", default_value_t = false)]
    pub full_auto: bool,

    #[arg(
        long = "dangerously-bypass-approvals-and-sandbox",
        alias = "yolo",
        default_value_t = false
    )]
    pub dangerously_bypass_approvals_and_sandbox: bool,

    #[arg(long = "sandbox", short = 's', value_enum)]
    pub sandbox_mode: Option<codex_common::SandboxModeCliArg>,

    #[arg(long = "cd", short = 'C', value_name = "DIR")]
    pub cwd: Option<PathBuf>,

    #[arg(long = "profile", short = 'p')]
    pub config_profile: Option<String>,

    #[arg(long = "model", short = 'm')]
    pub model: Option<String>,
}

async fn load_config(
    overrides: &CliConfigOverrides,
    toggles: &SubagentToggleArgs,
) -> anyhow::Result<Config> {
    let kv_overrides = overrides.parse_overrides().map_err(|e| anyhow!(e))?;
    let structured = ConfigOverrides {
        subagent_toggles: toggles.to_map(),
        ..ConfigOverrides::default()
    };
    Config::load_with_cli_overrides(kv_overrides, structured)
        .await
        .map_err(anyhow::Error::from)
}

fn print_subagent_list(subagents: &BTreeMap<String, SubagentDefinition>) {
    if subagents.is_empty() {
        println!("No subagents are configured.");
        return;
    }

    println!("{:<24} {:<10} Display Name", "Name", "Enabled");
    for (name, definition) in subagents {
        let status = if definition.enabled { "yes" } else { "no" };
        let display_name = definition.display_name.as_deref().unwrap_or("—");
        println!("{name:<24} {status:<10} {display_name}");
    }
}

fn show_subagent(
    subagents: &BTreeMap<String, SubagentDefinition>,
    args: SubagentShowArgs,
) -> anyhow::Result<()> {
    let Some(definition) = subagents.get(&args.name) else {
        return Err(anyhow!("unknown subagent `{}`", args.name));
    };

    match args.format {
        SubagentShowFormat::Human => {
            println!("Name: {}", args.name);
            if let Some(display) = &definition.display_name {
                println!("Display name: {display}");
            }
            if let Some(description) = &definition.description {
                println!("Description: {description}");
            }
            println!("Enabled: {}", definition.enabled);
            if let Some(prompt) = &definition.system_prompt {
                println!("System prompt:\n{prompt}");
            }
            if definition.allowed_tools.is_empty() {
                println!("Allowed tools: (inherit default)");
            } else {
                println!("Allowed tools: {}", definition.allowed_tools.join(", "));
            }
            if definition.context_sources.is_empty() {
                println!("Context sources: (none)");
            } else {
                println!("Context sources: {}", definition.context_sources.join(", "));
            }
            if definition.triggers.is_empty() {
                println!("Triggers: (manual)");
            } else {
                println!("Triggers: {}", definition.triggers.join(", "));
            }
        }
        SubagentShowFormat::Json => {
            let payload = json!({
                "name": args.name,
                "display_name": definition.display_name,
                "description": definition.description,
                "enabled": definition.enabled,
                "system_prompt": definition.system_prompt,
                "allowed_tools": definition.allowed_tools,
                "context_sources": definition.context_sources,
                "triggers": definition.triggers,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }

    Ok(())
}

async fn run_subagent(
    args: SubagentRunArgs,
    overrides: &CliConfigOverrides,
    toggles: &SubagentToggleArgs,
    codex_linux_sandbox_exe: Option<PathBuf>,
) -> anyhow::Result<()> {
    let config = load_config(overrides, toggles).await?;
    let Some(definition) = config.subagents.get(&args.name) else {
        return Err(anyhow!("unknown subagent `{}`", args.name));
    };

    let mut argv = Vec::new();
    argv.push("codex-subagent-run".to_string());
    if args.json {
        argv.push("--json".to_string());
    }
    if args.oss {
        argv.push("--oss".to_string());
    }
    if args.full_auto {
        argv.push("--full-auto".to_string());
    }
    if args.dangerously_bypass_approvals_and_sandbox {
        argv.push("--dangerously-bypass-approvals-and-sandbox".to_string());
    }
    if let Some(mode) = args.sandbox_mode {
        argv.push("--sandbox".to_string());
        let name = mode
            .to_possible_value()
            .context("sandbox flag missing possible value")?
            .get_name()
            .to_string();
        argv.push(name);
    }
    if let Some(model) = &args.model {
        argv.push("-m".to_string());
        argv.push(model.clone());
    }
    if let Some(profile) = &args.config_profile {
        argv.push("-p".to_string());
        argv.push(profile.clone());
    }
    if let Some(cwd) = &args.cwd {
        argv.push("-C".to_string());
        argv.push(cwd.display().to_string());
    }
    if let Some(prompt) = &args.prompt {
        argv.push(prompt.clone());
    }

    let mut exec_cli = ExecCli::parse_from(argv);

    // Start with any user-provided overrides.
    exec_cli
        .config_overrides
        .raw_overrides
        .extend(overrides.raw_overrides.clone());

    // Apply toggle overrides next so they take precedence.
    exec_cli
        .config_overrides
        .raw_overrides
        .extend(toggles.to_cli_overrides());

    // Ensure the requested subagent is enabled and inject its prompt if present.
    exec_cli
        .config_overrides
        .raw_overrides
        .push(format!("subagents.{}.enabled=true", args.name));

    if let Some(prompt) = &definition.system_prompt {
        let encoded = TomlValue::String(prompt.clone()).to_string();
        exec_cli
            .config_overrides
            .raw_overrides
            .push(format!("base_instructions={encoded}"));
    }

    codex_exec::run_main(exec_cli, codex_linux_sandbox_exe).await?;
    Ok(())
}
