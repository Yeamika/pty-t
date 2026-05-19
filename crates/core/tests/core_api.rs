#[cfg(unix)]
#[test]
fn command_spec_sets_cwd_env_and_exposes_exit_code() {
    use pty_t_core::session::CommandSpec;
    use pty_t_core::PtyManager;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let cwd = std::env::temp_dir().join(format!("pty-t-core-api-{suffix}"));
    fs::create_dir_all(&cwd).unwrap();

    let command = CommandSpec::new("sh")
        .args([
            "-lc",
            r#"test "$SHELL_MANAGER_TEST" = ok && test "$(pwd)" = "$EXPECTED_CWD"; exit 7"#,
        ])
        .cwd(&cwd)
        .env("SHELL_MANAGER_TEST", "ok")
        .env("EXPECTED_CWD", cwd.to_string_lossy());
    let manager = PtyManager::default_shell(80, 24);
    manager.create_pty("main", command, None, None).unwrap();
    let detail = manager.detail("main").unwrap();

    assert_eq!(detail.pty, "main");
    assert_eq!(detail.cwd.as_deref(), Some(cwd.to_string_lossy().as_ref()));
    assert_eq!(detail.env.get("SHELL_MANAGER_TEST").unwrap(), "ok");
    assert!(detail.process_id.is_some());
    assert!(detail.created_at > 0);

    assert_eq!(manager.wait_exit_code("main").unwrap(), 7);
    let _ = fs::remove_dir_all(cwd);
}

#[cfg(unix)]
#[tokio::test]
async fn wait_exit_code_timeout_does_not_block_later_kill() {
    use pty_t_core::session::CommandSpec;
    use pty_t_core::PtyManager;
    use std::time::Duration;

    let manager = PtyManager::default_shell(80, 24);
    manager
        .create_pty(
            "main",
            CommandSpec::new("sh").args(["-lc", "sleep 5"]),
            None,
            None,
        )
        .unwrap();

    assert_eq!(
        manager
            .wait_exit_code_timeout("main", Duration::from_millis(20))
            .await
            .unwrap(),
        None
    );
    manager.kill_pty("main").unwrap();
}

#[cfg(unix)]
#[test]
fn output_history_defaults_to_one_mib_and_can_be_limited() {
    use pty_t_core::session::{CommandSpec, DEFAULT_OUTPUT_HISTORY_LIMIT};
    use pty_t_core::PtyManager;

    let manager = PtyManager::default_shell(80, 24);
    let session = manager
        .create_pty(
            "main",
            CommandSpec::new("sh").args(["-lc", "cat"]),
            None,
            None,
        )
        .unwrap();

    assert_eq!(session.output_history_limit(), DEFAULT_OUTPUT_HISTORY_LIMIT);

    session.set_output_history_limit(4);
    session.on_pty_output(b"abcdef");

    assert_eq!(session.output_history_limit(), 4);
    assert_eq!(session.output_history_len(), 4);
    assert!(!manager.snapshot_pty("main").unwrap().is_empty());

    session.set_output_history_limit(0);
    assert_eq!(session.output_history_len(), 0);

    manager.kill_pty("main").unwrap();
}
