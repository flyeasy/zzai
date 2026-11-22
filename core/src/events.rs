//! 事件协议 v1（DESIGN §3.1 硬规则 4、D-006）
//!
//! 一条事件 = 单行 JSON：
//! {"schema":1,"type":"...","seq":n,"session":n,"generation":n,"payload":{...}}

use serde_json::{json, Value};

pub const SCHEMA: u32 = 1;

pub fn envelope(kind: &str, seq: u64, session: u64, generation: u64, payload: Value) -> String {
    json!({
        "schema": SCHEMA,
        "type": kind,
        "seq": seq,
        "session": session,
        "generation": generation,
        "payload": payload,
    })
    .to_string()
}

pub fn state_payload(phase: &str) -> Value {
    json!({ "phase": phase })
}

pub fn error_payload(code: &str, message: &str) -> Value {
    json!({ "code": code, "message": message })
}

pub fn result_payload(task_id: &str, text: &str, partial: bool) -> Value {
    json!({
        "task_id": task_id,
        "text": text,
        "partial": partial,
        "segments": [],
    })
}
