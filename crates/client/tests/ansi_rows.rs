#[test]
fn rows_formatted_preserves_ansi_color() {
    let mut parser = vt100::Parser::new(24, 80, 2000);
    parser.process(b"\x1b[31mRED\x1b[0m\n");

    let row = parser
        .screen()
        .rows_formatted(0, 80)
        .next()
        .expect("expected one row");
    let row = String::from_utf8(row).expect("formatted row must be utf8");

    assert!(row.contains("\x1b[31m"), "missing red escape code: {row:?}");
    assert!(row.contains("RED"), "missing text: {row:?}");
}
