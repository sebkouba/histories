use histories_core::{parse_chatgpt_export, parse_codex, parse_pi};
use std::{collections::HashMap, path::PathBuf};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn parses_scrubbed_codex_fixture() {
    let conversation = parse_codex(&fixture("codex-current.jsonl"), &HashMap::new())
        .unwrap()
        .unwrap();
    let clean = conversation
        .clean_messages()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();

    assert_eq!(conversation.external_id, "fixture-codex");
    assert_eq!(clean, ["Review the release", "The release is ready"]);
    assert_eq!(conversation.messages.len(), 3);
}

#[test]
fn parses_only_the_active_pi_fixture_branch() {
    let conversation = parse_pi(&fixture("pi-branched.jsonl")).unwrap().unwrap();
    let clean = conversation
        .clean_messages()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();

    assert_eq!(clean, ["Choose a branch", "Active answer"]);
    assert!(
        !conversation
            .messages
            .iter()
            .any(|message| message.content == "Abandoned answer")
    );
}

#[test]
fn parses_only_the_current_chatgpt_fixture_branch() {
    let conversations = parse_chatgpt_export(&fixture("chatgpt-current.json")).unwrap();
    let clean = conversations[0]
        .clean_messages()
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>();

    assert_eq!(clean, ["Choose a branch", "Active answer"]);
    assert_eq!(conversations[0].title, "Fixture conversation");
}
