use std::collections::HashMap;

use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::config::ConfigToml;
use codex_core::config::SubagentToml;
use tempfile::TempDir;

#[test]
fn resolves_subagents_and_overrides_enabled_state() {
    let cfg = ConfigToml {
        subagents: Some(HashMap::from([
            (
                "reviewer".to_string(),
                SubagentToml {
                    display_name: Some("Reviewer".to_string()),
                    description: Some("Focus on code review feedback".to_string()),
                    system_prompt: Some("Provide review comments".to_string()),
                    enabled: Some(true),
                    tools: vec!["apply_patch".to_string()],
                    context: vec!["diff".to_string()],
                    triggers: vec!["/review".to_string()],
                },
            ),
            (
                "tester".to_string(),
                SubagentToml {
                    system_prompt: Some("Run the test suite".to_string()),
                    ..Default::default()
                },
            ),
        ])),
        ..ConfigToml::default()
    };

    let overrides = ConfigOverrides {
        subagent_toggles: HashMap::from([(String::from("reviewer"), false)]),
        ..ConfigOverrides::default()
    };

    let home = TempDir::new().expect("create temp dir");
    let config =
        Config::load_from_base_config_with_overrides(cfg, overrides, home.path().to_path_buf())
            .expect("load config");

    let reviewer = config
        .subagents
        .get("reviewer")
        .expect("reviewer subagent exists");
    assert!(!reviewer.enabled);
    assert_eq!(
        reviewer.system_prompt.as_deref(),
        Some("Provide review comments")
    );
    assert_eq!(reviewer.allowed_tools, vec!["apply_patch".to_string()]);
    assert_eq!(reviewer.context_sources, vec!["diff".to_string()]);
    assert_eq!(reviewer.triggers, vec!["/review".to_string()]);

    let tester = config
        .subagents
        .get("tester")
        .expect("tester subagent exists");
    assert!(tester.enabled);
    assert_eq!(
        tester.system_prompt.as_deref().unwrap_or_default(),
        "Run the test suite"
    );
}
