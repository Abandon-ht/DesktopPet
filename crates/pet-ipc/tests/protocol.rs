use pet_ipc::{
    frame,
    server::{Command, serve},
};
use serde_json::{Value, json};
use std::io::Cursor;
fn request(sequence: u64, kind: &str, payload: Value) -> Value {
    json!({"protocol_version":1,"session_id":"test","sequence":sequence,
        "request_id":format!("r{sequence}"),"type":kind,"payload":payload})
}
fn input(messages: &[Value]) -> Cursor<Vec<u8>> {
    Cursor::new(
        messages
            .iter()
            .map(|v| format!("{v}\n"))
            .collect::<String>()
            .into_bytes(),
    )
}
#[test]
fn duplicate_request_is_applied_once_and_rejection_keeps_connection() {
    let hello = request(1, "hello", json!({}));
    let first = request(2, "desktop", json!({"type":"set_visible","payload":false}));
    let mut retry = first.clone();
    retry["sequence"] = json!(3);
    let bad = request(
        4,
        "desktop",
        json!({"type":"execute_script","payload":"bad"}),
    );
    let mut output = Vec::new();
    let mut applications = 0;
    serve(
        &mut input(&[hello, first, retry, bad, request(5, "ping", json!({}))]),
        &mut output,
        |c| {
            if matches!(c, Command::Desktop(_)) {
                applications += 1;
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(applications, 1);
    let replies: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(replies[1]["payload"], replies[2]["payload"]);
    assert_eq!(replies[3]["payload"]["accepted"], false);
    assert_eq!(replies[4]["type"], "pong");
}
#[test]
fn wrong_identity_order_and_version_fail_before_applying_commands() {
    for key in ["session_id", "sequence", "protocol_version"] {
        let mut bad = request(2, "desktop", json!({"type":"set_visible","payload":true}));
        bad[key] = if key == "session_id" {
            json!("old-session")
        } else {
            json!(99)
        };
        let mut applications = 0;
        assert!(
            serve(
                &mut input(&[request(1, "hello", json!({})), bad]),
                &mut Vec::new(),
                |c| {
                    if matches!(c, Command::Desktop(_)) {
                        applications += 1;
                    }
                    Ok(())
                }
            )
            .is_err()
        );
        assert_eq!(applications, 0);
    }
}
#[test]
fn frames_are_bounded_and_truncated_input_is_rejected() {
    assert!(frame(&mut Cursor::new(vec![b'x'; pet_ipc::MAX_FRAME + 1])).is_err());
    assert!(frame(&mut Cursor::new(b"{}".as_slice())).is_err());
    assert!(frame(&mut Cursor::new(Vec::<u8>::new())).unwrap().is_none());
}
#[test]
fn request_id_cannot_be_reused_for_another_command() {
    let first = request(2, "desktop", json!({"type":"set_visible","payload":false}));
    let mut second = first.clone();
    second["sequence"] = json!(3);
    second["payload"]["payload"] = json!(true);
    let error = serve(
        &mut input(&[request(1, "hello", json!({})), first, second]),
        &mut Vec::new(),
        |_| Ok(()),
    )
    .unwrap_err();
    assert!(error.to_string().contains("request_id_reused"));
}
