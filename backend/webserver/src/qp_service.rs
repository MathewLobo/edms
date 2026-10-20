use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Volatile transport headers stripped during duplicate comparison.
pub const VOLATILE_HEADERS: &[&str] = &[
    "date",
    "age",
    "cf-ray",
    "cf-cache-status",
    "x-request-id",
    "request-id",
    "x-runtime",
    "x-response-time",
    "server-timing",
    "nel",
    "report-to",
    "x-ratelimit-reset",
    "x-ratelimit-remaining",
    "set-cookie",
    "alt-svc",
    "expires",
    "last-modified",
    "etag",
];

/// Recursively sorts all JSON object keys alphabetically to produce canonical JSON.
/// Does not alter primitives, arrays, or string values.
pub fn normalize(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map
                .iter()
                .map(|(k, v)| (k.clone(), normalize(v)))
                .collect();
            let mut result = Map::new();
            for (k, v) in sorted {
                result.insert(k, v);
            }
            Value::Object(result)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(normalize).collect()),
        other => other.clone(),
    }
}

/// Unwraps any legacy or recursive {"body": ..., "ok": true} wrappers.
pub fn unwrap_nested_body(val: &Value) -> Value {
    let mut cur = val;
    while let Value::Object(map) = cur {
        if map.contains_key("body") && map.contains_key("ok") {
            if let Some(inner) = map.get("body") {
                cur = inner;
                continue;
            }
        }
        break;
    }
    cur.clone()
}

/// Parses and canonicalizes a JSON string into a compact, normalized representation.
/// If parsing fails or the input is empty, returns the trimmed string.
pub fn canonical(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(v) => {
            let unwrapped = unwrap_nested_body(&v);
            serde_json::to_string(&normalize(&unwrapped)).unwrap_or_else(|_| trimmed.to_string())
        }
        Err(_) => trimmed.to_string(),
    }
}

/// Strips volatile transport headers from header JSON.
pub fn sanitize_header_value(val: &mut Value) {
    match val {
        Value::Object(map) => {
            if map.contains_key("response_headers") || map.contains_key("request_headers") {
                if let Some(req_h) = map.get_mut("request_headers") {
                    sanitize_header_value(req_h);
                }
                if let Some(res_h) = map.get_mut("response_headers") {
                    sanitize_header_value(res_h);
                }
            } else {
                map.retain(|k, _| !VOLATILE_HEADERS.contains(&k.to_ascii_lowercase().as_str()));
            }
        }
        _ => {}
    }
}

/// Canonicalizes headers JSON and strips dynamic transport headers.
pub fn canonical_headers(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(mut v) => {
            sanitize_header_value(&mut v);
            serde_json::to_string(&normalize(&v)).unwrap_or_else(|_| trimmed.to_string())
        }
        Err(_) => trimmed.to_string(),
    }
}

/// Extracts query parameter data string.
pub fn extract_qp_data(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
        if let Some(data) = val.get("data").and_then(|d| d.as_str()) {
            return extract_qp_data(data);
        }
    }

    let query_str = if let Some(pos) = trimmed.find('?') {
        &trimmed[pos + 1..]
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") || trimmed.starts_with('/') {
        return String::new();
    } else if trimmed.contains('=') {
        trimmed
    } else {
        return String::new();
    };

    let mut pairs: Vec<&str> = query_str.split('&').filter(|s| !s.is_empty()).collect();
    pairs.sort();
    pairs.join("&")
}

/// Generates a canonical SHA-256 hash key for a request snapshot.
pub fn make_canonical_key(
    method: &str,
    endpoint: &str,
    q: &str,
    p: &str,
    h: &str,
    qp_data: &str,
) -> String {
    let cq = canonical(q);
    let cp = canonical(p);
    let ch = canonical_headers(h);

    let mut hasher = Sha256::new();
    for part in &[method, endpoint, &cq, &cp, &ch, qp_data] {
        let prefix = format!("{}:{}|", part.len(), part);
        hasher.update(prefix.as_bytes());
    }

    hex::encode(hasher.finalize())
}

/// Thread-safe in-memory store for tracking seen request snapshot hash keys.
#[derive(Clone, Default)]
pub struct DuplicateStore {
    seen: Arc<Mutex<HashSet<String>>>,
}

impl DuplicateStore {
    pub fn new() -> Self {
        Self {
            seen: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Checks if the key is stored, inserting it if new.
    pub fn add_if_new(&self, key: &str) -> bool {
        let mut set = self.seen.lock().unwrap();
        set.insert(key.to_string())
    }

    /// Removes a key from the set.
    pub fn remove(&self, key: &str) {
        let mut set = self.seen.lock().unwrap();
        set.remove(key);
    }

    /// Checks if a key exists in the store without inserting it.
    pub fn contains(&self, key: &str) -> bool {
        let set = self.seen.lock().unwrap();
        set.contains(key)
    }

    /// Returns the total count of keys currently in the store.
    pub fn count(&self) -> usize {
        let set = self.seen.lock().unwrap();
        set.len()
    }

    /// Rebuilds the store on startup from storage files.
    pub fn rebuild_from_storage(&self, storage_root: &Path) -> usize {
        let candidates = [
            storage_root.join("storage").join("globalEQPData"),
            storage_root.join("globalEQPData"),
            std::path::PathBuf::from("/app/storage/globalEQPData"),
            std::path::PathBuf::from("/app/edms_root/storage/globalEQPData"),
            std::path::PathBuf::from("/app/edms_root/globalEQPData"),
        ];

        let mut loaded = 0;
        let mut set = self.seen.lock().unwrap();

        for eqp_root in &candidates {
            if !eqp_root.exists() {
                continue;
            }
            if let Ok(entries) = fs::read_dir(eqp_root) {
                for entry in entries.flatten() {
                    if !entry.path().is_dir() {
                        continue;
                    }
                    let eid = entry.file_name().to_string_lossy().to_string();
                    let folder = entry.path();
                    let numbers = list_existing_qp_numbers(&folder, &eid);

                    for num in numbers {
                        if let Some((q, p, h, qp_data)) = read_qp_quadruple(&folder, &eid, num) {
                            let (method, _) = extract_method_and_endpoint(&q);
                            let key = make_canonical_key(&method, &eid, &q, &p, &h, &qp_data);
                            set.insert(key);
                            loaded += 1;
                        }
                    }
                }
            }
        }

        loaded
    }
}

/// Helper to parse method and endpoint URL out of request JSON.
pub fn extract_method_and_endpoint(q_json: &str) -> (String, String) {
    if let Ok(v) = serde_json::from_str::<Value>(q_json) {
        let m = v.get("method").and_then(|s| s.as_str()).unwrap_or("GET").to_uppercase();
        let u = v.get("url").or_else(|| v.get("endpoint_str")).and_then(|s| s.as_str()).unwrap_or("").to_string();
        (m, u)
    } else {
        ("GET".to_string(), String::new())
    }
}

/// Reads the Q, P, H, and QP-data file contents for a specific QP number from disk.
pub fn read_qp_quadruple(folder: &Path, eid: &str, num: i32) -> Option<(String, String, String, String)> {
    let req_path = folder.join(format!("{eid}-request-{num}.json"));
    let res_path = folder.join(format!("{eid}-response-{num}.json"));
    let hdr_path = folder.join(format!("{eid}-headers-{num}.json"));

    let qp_file_1 = folder.join(format!("query-param-{num}.json"));
    let qp_file_2 = folder.join(format!("{eid}-query-param-{num}.json"));

    let q = fs::read_to_string(&req_path).ok()?;
    let p = fs::read_to_string(&res_path).unwrap_or_else(|_| "{}".to_string());
    let h = fs::read_to_string(&hdr_path).unwrap_or_else(|_| "{}".to_string());

    let raw_qp = fs::read_to_string(&qp_file_1)
        .or_else(|_| fs::read_to_string(&qp_file_2))
        .unwrap_or_default();
    let qp_data = extract_qp_data(&raw_qp);

    Some((q, p, h, qp_data))
}

/// Reads the Q, P, H file contents for a specific QP number from disk.
pub fn read_qp_triple(folder: &Path, eid: &str, num: i32) -> Option<(String, String, String)> {
    let (q, p, h, _) = read_qp_quadruple(folder, eid, num)?;
    Some((q, p, h))
}

/// Scans the endpoint storage directory and returns all existing QP request numbers.
pub fn list_existing_qp_numbers(folder: &Path, eid: &str) -> Vec<i32> {
    let mut numbers = Vec::new();
    let prefix = format!("{eid}-request-");
    let suffix = ".json";

    if let Ok(entries) = fs::read_dir(folder) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(&prefix) && name_str.ends_with(suffix) {
                let num_str = &name_str[prefix.len()..name_str.len() - suffix.len()];
                if let Ok(n) = num_str.parse::<i32>() {
                    numbers.push(n);
                }
            }
        }
    }
    numbers.sort_unstable();
    numbers
}

/// Finds matching duplicate QP number if an identical snapshot already exists.
pub fn find_duplicate_qp(
    folder: &Path,
    eid: &str,
    method: &str,
    endpoint_str: &str,
    new_q: &str,
    new_p: &str,
    new_h: &str,
    exclude_request_number: Option<i32>,
) -> Option<i32> {
    if !folder.exists() {
        return None;
    }

    let qp_data = extract_qp_data(endpoint_str);
    let candidate_key = make_canonical_key(method, eid, new_q, new_p, new_h, &qp_data);
    let existing_numbers = list_existing_qp_numbers(folder, eid);

    for num in existing_numbers {
        if Some(num) == exclude_request_number {
            continue;
        }

        if let Some((q, p, h, qp_d)) = read_qp_quadruple(folder, eid, num) {
            let (m_ex, u_ex) = extract_method_and_endpoint(&q);
            let m_use = if m_ex.is_empty() { method } else { &m_ex };
            let qp_use = if !qp_d.is_empty() {
                qp_d
            } else if !u_ex.is_empty() {
                extract_qp_data(&u_ex)
            } else {
                qp_data.clone()
            };
            let existing_key = make_canonical_key(m_use, eid, &q, &p, &h, &qp_use);
            if candidate_key == existing_key {
                return Some(num);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_canonical_unwraps_nested_body() {
        let nested = r#"{"body":{"body":{}, "ok":true}, "ok":true}"#;
        let empty = r#"{}"#;
        assert_eq!(canonical(nested), canonical(empty));
    }

    #[test]
    fn test_extract_qp_data_url_without_query() {
        assert_eq!(extract_qp_data("https://dummyjson.com/users"), "");
        assert_eq!(extract_qp_data("https://dummyjson.com/users?limit=30&skip=0"), "limit=30&skip=0");
        assert_eq!(extract_qp_data("https://dummyjson.com/users?skip=0&limit=30"), "limit=30&skip=0");
    }
    use super::*;

    #[test]
    fn test_canonical_key_same_despite_key_order() {
        let q1 = r#"{"url":"https://api.test/users","method":"GET","body":{"b":2,"a":1}}"#;
        let q2 = r#"{"method":"GET","body":{"a":1,"b":2},"url":"https://api.test/users"}"#;
        let p = r#"{"status":200,"body":{"total":10}}"#;
        let h = r#"{"content-type":"application/json"}"#;
        let qp = "sort=listPrice,displayName";

        let key1 = make_canonical_key("GET", "/users", q1, p, h, qp);
        let key2 = make_canonical_key("GET", "/users", q2, p, h, qp);

        assert_eq!(key1, key2);
    }

    #[test]
    fn test_canonical_key_differs_on_single_character() {
        let q = r#"{"method":"GET","url":"/users"}"#;
        let p1 = r#"{"status":200,"body":{"total":10}}"#;
        let p2 = r#"{"status":200,"body":{"total":11}}"#;
        let h = r#"{}"#;
        let qp = "sort=price";

        let key1 = make_canonical_key("GET", "/users", q, p1, h, qp);
        let key2 = make_canonical_key("GET", "/users", q, p2, h, qp);

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_canonical_headers_ignores_timestamp_and_volatile_headers() {
        let h1 = r#"{
            "request_headers": {"Authorization": "Bearer 123"},
            "response_headers": {
                "Content-Type": "application/json",
                "Date": "Sun, 19 Oct 2026 05:22:15 GMT",
                "CF-RAY": "8d3e9102-DFW",
                "Age": "0"
            }
        }"#;

        let h2 = r#"{
            "request_headers": {"Authorization": "Bearer 123"},
            "response_headers": {
                "Content-Type": "application/json",
                "Date": "Sun, 19 Oct 2026 05:22:30 GMT",
                "CF-RAY": "8d3e9999-DFW",
                "Age": "15"
            }
        }"#;

        let key1 = make_canonical_key("GET", "/users", r#"{"q":1}"#, r#"{"status":200}"#, h1, "");
        let key2 = make_canonical_key("GET", "/users", r#"{"q":1}"#, r#"{"status":200}"#, h2, "");

        assert_eq!(key1, key2, "Volatile headers like Date/CF-RAY should not cause duplicate detection failure");
    }

    #[test]
    fn test_canonical_headers_still_differs_on_meaningful_header() {
        let h1 = r#"{"response_headers":{"Content-Type":"application/json","Date":"Sun, 19 Oct 2026 05:22:15 GMT"}}"#;
        let h2 = r#"{"response_headers":{"Content-Type":"text/html","Date":"Sun, 19 Oct 2026 05:22:15 GMT"}}"#;

        let key1 = make_canonical_key("GET", "/users", r#"{"q":1}"#, r#"{"status":200}"#, h1, "");
        let key2 = make_canonical_key("GET", "/users", r#"{"q":1}"#, r#"{"status":200}"#, h2, "");

        assert_ne!(key1, key2, "Meaningful header differences (Content-Type) must still produce different keys");
    }

    #[test]
    fn test_extract_qp_data_ignores_user_metadata() {
        let json_a = r#"{
            "data": "sort=listPrice,displayName",
            "user": {
                "key": ["sort"],
                "value": ["listPrice,displayName"],
                "helpstring": ["sort by price"]
            }
        }"#;

        let json_b = r#"{
            "data": "sort=listPrice,displayName",
            "user": {
                "key": ["diff_key"],
                "value": ["junk"],
                "helpstring": ["first page"]
            }
        }"#;

        let data_a = extract_qp_data(json_a);
        let data_b = extract_qp_data(json_b);

        assert_eq!(data_a, "sort=listPrice,displayName");
        assert_eq!(data_b, "sort=listPrice,displayName");
        assert_eq!(data_a, data_b);

        let q = r#"{"method":"GET"}"#;
        let p = r#"{"status":200}"#;
        let h = r#"{}"#;
        let key_a = make_canonical_key("GET", "/products", q, p, h, &data_a);
        let key_b = make_canonical_key("GET", "/products", q, p, h, &data_b);

        assert_eq!(key_a, key_b);
    }

    #[test]
    fn test_extract_qp_data_from_url() {
        let url = "/ccadmin/v1/products/?sort=listPrice,displayName";
        assert_eq!(extract_qp_data(url), "sort=listPrice,displayName");
    }

    #[test]
    fn test_duplicate_store_add_and_rollback() {
        let store = DuplicateStore::new();
        let key = "test_key_123";

        assert!(store.add_if_new(key));
        assert!(!store.add_if_new(key));

        store.remove(key);
        assert!(store.add_if_new(key));
    }
}
