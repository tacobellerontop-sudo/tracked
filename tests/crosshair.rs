use tracked::crosshair::{fmt_num, CrosshairCode, CrosshairError};

const SAMPLES: &[&str] = &[
    "0;P;c;5;h;0;0l;4;0o;2;0a;1;0f;0;1b;0",
    "0;s;1;P;c;1;o;1;d;1;z;3;0b;0;1b;0",
    "0;P;c;8;u;00FFAAFF;h;0;f;0;0l;3;0v;5;0g;1;0o;1;0a;1;0f;0;1b;0;A;c;1;S;s;0",
    "0",
];

#[test]
fn round_trips_exactly() {
    for code in SAMPLES {
        let parsed = CrosshairCode::parse(code).unwrap();
        assert_eq!(&parsed.to_string(), code);
        assert_eq!(CrosshairCode::parse(&parsed.to_string()).unwrap(), parsed);
    }
}

#[test]
fn tolerates_whitespace_and_trailing_semicolon() {
    let c = CrosshairCode::parse("  0;P;c;5;\n").unwrap();
    assert_eq!(c.to_string(), "0;P;c;5");
}

#[test]
fn typed_primary_view() {
    let c = CrosshairCode::parse(SAMPLES[0]).unwrap();
    let p = c.primary();
    assert_eq!(p.color_index, 5);
    assert_eq!(p.color, [0, 255, 255, 255]);
    assert!(!p.outlines);
    assert_eq!(p.inner.length, 4.0);
    assert_eq!(p.inner.offset, 2.0);
    assert_eq!(p.inner.opacity, 1.0);
    assert!(p.inner.show, "defaults on");
    assert!(!p.outer.show);
    assert_eq!(p.inner.thickness, 2.0, "default");

    let custom = CrosshairCode::parse(SAMPLES[2]).unwrap().primary();
    assert_eq!(custom.color, [0x00, 0xFF, 0xAA, 0xFF]);
}

#[test]
fn edits_round_trip_through_typed_view() {
    let mut c = CrosshairCode::parse(SAMPLES[0]).unwrap();
    let mut p = c.primary();
    p.center_dot = true;
    p.inner.length = 7.0;
    p.outer.opacity = 0.5;
    c.set_primary(&p);
    let reparsed = CrosshairCode::parse(&c.to_string()).unwrap();
    assert_eq!(reparsed.primary(), p);
    assert_eq!(reparsed.get("P", "0l"), Some("7"));
    assert_eq!(reparsed.get("P", "0f"), Some("0"), "unknown keys are preserved");
}

#[test]
fn rejects_malformed_codes() {
    assert_eq!(CrosshairCode::parse(""), Err(CrosshairError::Empty));
    assert_eq!(CrosshairCode::parse("1;P;c;5"), Err(CrosshairError::BadVersion));
    assert_eq!(
        CrosshairCode::parse("0;P;c"),
        Err(CrosshairError::MissingValue("c".into()))
    );
    assert_eq!(CrosshairCode::parse("0;P;;5"), Err(CrosshairError::EmptyToken));
    assert_eq!(
        CrosshairCode::parse("0;P;c;5;P;c;1"),
        Err(CrosshairError::DuplicateSection("P".into()))
    );
}

#[test]
fn number_formatting() {
    assert_eq!(fmt_num(2.0), "2");
    assert_eq!(fmt_num(0.35), "0.35");
    assert_eq!(fmt_num(0.0), "0");
    assert_eq!(fmt_num(0.8), "0.8");
}
