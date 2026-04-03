use crate::models::DbShard;
use std::collections::BTreeMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

fn hash<T: Hash>(item: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    item.hash(&mut hasher);
    hasher.finish()
}

pub struct ConsistentHash {
    ring: BTreeMap<u64, String>,
    replicas: usize,
}

impl ConsistentHash {
    pub fn new(replicas: usize) -> Self {
        Self {
            ring: BTreeMap::new(),
            replicas,
        }
    }

    pub fn add_node(&mut self, node: &str) {
        for i in 0..self.replicas {
            let virtual_node = format!("{}#{}", node, i);
            let h = hash(&virtual_node);
            self.ring.insert(h, node.to_string());
        }
    }

    pub fn get_node(&self, key: &str) -> Option<&String> {
        let h = hash(&key);

        self.ring
            .range(h..)
            .next()
            .or_else(|| self.ring.iter().next())
            .map(|(_, v)| v)
    }
}

pub struct Snowflake {
    machine_id: u16,
    sequence: u16,
    last_timestamp: u64,
}

impl Snowflake {
    pub fn new(machine_id: u16) -> Self {
        Self {
            machine_id,
            sequence: 0,
            last_timestamp: 0,
        }
    }

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }

    pub fn generate(&mut self) -> u64 {
        let mut ts = Self::now();

        if ts == self.last_timestamp {
            self.sequence += 1;

            if self.sequence > 4095 {
                while ts <= self.last_timestamp {
                    ts = Self::now();
                }
                self.sequence = 0;
            }
        } else {
            self.sequence = 0;
        }

        self.last_timestamp = ts;

        (ts << 22) | ((self.machine_id as u64) << 12) | self.sequence as u64
    }
}

const BASE62: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub fn encode_base62(mut num: u64) -> String {
    if num == 0 {
        return "0".to_string();
    }

    let mut result = String::new();

    while num > 0 {
        let rem = (num % 62) as usize;
        result.push(BASE62[rem] as char);
        num /= 62;
    }

    result.chars().rev().collect()
}

pub fn unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

pub fn select_node_index(key: &str, ring: &ConsistentHash) -> Option<usize> {
    let node = ring.get_node(key)?;
    node.strip_prefix("redis")
        .and_then(|id| id.parse::<usize>().ok())
}

pub fn select_db_node<'a>(
    key: &str,
    ring: &ConsistentHash,
    db_nodes: &'a [DbShard],
) -> Option<&'a DbShard> {
    let index = select_node_index(key, ring)?;
    db_nodes.get(index)
}

pub fn fallback_indices(key: &str, ring: &ConsistentHash, total_nodes: usize) -> Vec<usize> {
    if total_nodes == 0 {
        return Vec::new();
    }

    let start = select_node_index(key, ring).unwrap_or(0) % total_nodes;
    (0..total_nodes)
        .map(|offset| (start + offset) % total_nodes)
        .collect()
}
