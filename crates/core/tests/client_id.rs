use std::collections::HashMap;

use pty_t_core::session::{allocate_client_id, ClientInfo, TermSize};

#[test]
fn duplicate_client_id_gets_random_replacement() {
    let mut clients = HashMap::new();
    clients.insert(
        "0".to_string(),
        ClientInfo::new(1, TermSize { cols: 80, rows: 24 }),
    );

    let id = allocate_client_id(&clients, "0");
    assert_ne!(id, "0");
    assert!(id.starts_with("client-"));
}
