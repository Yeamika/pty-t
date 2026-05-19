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
