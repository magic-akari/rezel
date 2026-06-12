fn invalid(value: Option<i32>) {
    if let Some(inner) = value && 0..inner {
        let _ = inner;
    }
}
