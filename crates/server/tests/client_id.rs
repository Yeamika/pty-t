use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use pty_t_server::session::{allocate_client_id, ClientInfo, TermSize};
use tokio::sync::mpsc;

#[test]
fn duplicate_client_id_gets_random_replacement() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut clients = HashMap::new();
    clients.insert(
        "0".to_string(),
        ClientInfo::new(
            1,
            tx,
            TermSize { cols: 80, rows: 24 },
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345),
        ),
    );

    let id = allocate_client_id(&clients, "0");
    assert_ne!(id, "0");
    assert!(id.starts_with("client-"));
}
