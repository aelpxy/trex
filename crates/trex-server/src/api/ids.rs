use uuid::Uuid;

pub const SESSION: &str = "sess";
pub const USER: &str = "user";
pub const WORKSPACE: &str = "ws";
pub const PROJECT: &str = "proj";
pub const CREDIT_ENTRY: &str = "cred";

pub fn encode(prefix: &str, id: Uuid) -> String {
    format!("{prefix}_{}", id.simple())
}

pub fn decode(prefix: &str, value: &str) -> Option<Uuid> {
    let raw = value.strip_prefix(prefix)?.strip_prefix('_')?;
    Uuid::try_parse(raw).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_prefixed_ids() {
        let id = Uuid::now_v7();
        let encoded = encode(SESSION, id);
        assert!(encoded.starts_with("sess_"));
        assert_eq!(decode(SESSION, &encoded), Some(id));
        assert_eq!(decode("run", &encoded), None);
        assert_eq!(decode(SESSION, "sess_nope"), None);
    }
}
