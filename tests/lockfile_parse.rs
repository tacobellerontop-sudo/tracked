use ascend_lite::riot::lockfile::{parse, LockfileError};

#[test]
fn parses_valid_lockfile() {
    let l = parse("Riot Client:12345:52114:XyAbCdZq:https\r\n").unwrap();
    assert_eq!(l.name, "Riot Client");
    assert_eq!(l.pid, 12345);
    assert_eq!(l.port, 52114);
    assert_eq!(l.password.expose(), "XyAbCdZq");
    assert_eq!(l.protocol, "https");
}

#[test]
fn debug_output_hides_password() {
    let l = parse("Riot Client:1:2:supersecret:https").unwrap();
    assert!(!format!("{l:?}").contains("supersecret"));
}

#[test]
fn rejects_wrong_field_counts() {
    assert_eq!(parse(""), Err(LockfileError::FieldCount(1)));
    assert_eq!(parse("a:1:2:pw"), Err(LockfileError::FieldCount(4)));
    assert_eq!(parse("a:1:2:pw:https:extra"), Err(LockfileError::FieldCount(6)));
}

#[test]
fn rejects_bad_numbers() {
    assert_eq!(parse("a:x:2:pw:https"), Err(LockfileError::BadPid));
    assert_eq!(parse("a:-1:2:pw:https"), Err(LockfileError::BadPid));
    assert_eq!(parse("a:1:port:pw:https"), Err(LockfileError::BadPort));
    assert_eq!(parse("a:1:70000:pw:https"), Err(LockfileError::BadPort));
    assert_eq!(parse("a:1:0:pw:https"), Err(LockfileError::BadPort));
}

#[test]
fn rejects_empty_fields() {
    assert_eq!(parse(":1:2:pw:https"), Err(LockfileError::EmptyField("name")));
    assert_eq!(parse("a:1:2::https"), Err(LockfileError::EmptyField("password")));
    assert_eq!(parse("a:1:2:pw:"), Err(LockfileError::EmptyField("protocol")));
}
