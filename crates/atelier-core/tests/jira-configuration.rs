use atelier_core::{
    application::jira::{JiraFailure, configure, issue_identity},
    contracts::{JiraConfiguration, JiraConfigurationUpdate, JiraMode},
};

fn update(previous: &JiraConfiguration) -> JiraConfigurationUpdate {
    JiraConfigurationUpdate {
        expected_revision: previous.revision,
        mode: previous.mode,
        url: previous.url.clone(),
        email: previous.email.clone(),
        query: previous.query.clone(),
        token: None,
        clear_token: false,
    }
}
#[test]
fn credential_scope_and_key_rotations_protect_stale_jira_forms() {
    let initial = JiraConfiguration::default();
    let mut body = update(&initial);
    body.url = "https://synthetic.example.test".into();
    body.email = "synthetic@example.test".into();
    body.token = Some("synthetic-one".into());
    let (first, token) = configure(&initial, body, |url| Ok(url.into())).unwrap();
    assert_eq!(token.as_deref(), Some("synthetic-one"));
    let stale = update(&first);
    let mut rotation = update(&first);
    rotation.token = Some("synthetic-two".into());
    let (second, token) = configure(&first, rotation, |url| Ok(url.into())).unwrap();
    assert_eq!(token.as_deref(), Some("synthetic-two"));
    assert!(matches!(
        configure(&second, stale, |url| Ok(url.into())),
        Err(JiraFailure::Conflict)
    ));
    let (_, token) = configure(&second, update(&second), |url| Ok(url.into())).unwrap();
    assert!(token.is_none());
    let mut changed = update(&second);
    changed.url = "https://another.example.test".into();
    let (_, token) = configure(&second, changed, |url| Ok(url.into())).unwrap();
    assert_eq!(token.as_deref(), Some(""));
    let mut changed = update(&second);
    changed.mode = JiraMode::DataCenter;
    let (_, token) = configure(&second, changed, |url| Ok(url.into())).unwrap();
    assert_eq!(token.as_deref(), Some(""));
    let mut invalid = update(&second);
    invalid.clear_token = true;
    invalid.token = Some("synthetic-three".into());
    assert!(configure(&second, invalid, |url| Ok(url.into())).is_err());
    assert!(issue_identity("12345", "SYN_TEST-12"));
    for (id, key) in [
        ("", "SYN-1"),
        ("synthetic-token", "SYN-1"),
        ("12", "../SYN-1"),
        ("12", "SYN-1/../2"),
        ("12", "SYN-x"),
    ] {
        assert!(!issue_identity(id, key));
    }
}

#[test]
fn descriptions_preserve_order_unicode_and_plain_text_without_rendering_markup() {
    use atelier_core::application::jira::description;
    use serde_json::json;
    assert_eq!(description(&json!(null)).unwrap(), "");
    assert_eq!(
        description(&json!("<script>é😀</script>")).unwrap(),
        "<script>é😀</script>"
    );
    let doc = json!({"type":"doc","version":1,"content":[{"type":"heading","content":[{"type":"text","text":"Title"}]},{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"é😀"},{"type":"hardBreak"},{"type":"mention","attrs":{"text":"@Synthetic"}},{"type":"emoji","attrs":{"shortName":":ok:"}}]}]}]}]});
    assert_eq!(description(&doc).unwrap(), "Title\né😀\n@Synthetic:ok:");
    for invalid in [
        json!(true),
        json!({"type":"doc","version":2,"content":[]}),
        json!({"type":"doc","version":1,"content":[{"type":"text"}]}),
        json!("😀".repeat(10001)),
        json!("a\0b"),
        json!({"type":"doc","version":1,"content":vec![json!({"type":"paragraph"});4096]}),
    ] {
        assert!(description(&invalid).is_err());
    }
}
