//! Versioned JSON output envelope for `m80 --json`.

use serde::Serialize;

use crate::request_id;

const ENVELOPE_VERSION: u16 = 1;

#[derive(Serialize)]
struct Envelope<'a, T: ?Sized> {
    version: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    data: &'a T,
}

pub(crate) fn to_pretty<T: Serialize + ?Sized>(data: &T) -> String {
    let envelope = Envelope {
        version: ENVELOPE_VERSION,
        request_id: request_id::current(),
        data,
    };
    serde_json::to_string_pretty(&envelope).expect("json envelope serialization")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_wraps_payload_with_version() {
        let payload = serde_json::json!({ "status": "ok" });
        let rendered = to_pretty(&payload);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(parsed["version"], 1);
        assert!(parsed.get("request_id").is_none());
        assert_eq!(parsed["data"]["status"], "ok");
    }

    #[test]
    fn envelope_includes_scoped_request_id() {
        let _scope = request_id::set("req-test".to_owned());
        let payload = serde_json::json!({ "status": "ok" });
        let rendered = to_pretty(&payload);
        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();

        assert_eq!(parsed["request_id"], "req-test");
    }
}
