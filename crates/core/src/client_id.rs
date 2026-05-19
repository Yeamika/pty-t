use crate::types::ClientInfo;
use std::collections::HashMap;

pub fn allocate_client_id(clients: &HashMap<String, ClientInfo>, requested: &str) -> String {
    let requested = requested.trim();
    if !requested.is_empty() && !clients.contains_key(requested) {
        return requested.to_string();
    }

    loop {
        let id = format!("client-{:016x}", rand::random::<u64>());
        if !clients.contains_key(&id) {
            return id;
        }
    }
}
