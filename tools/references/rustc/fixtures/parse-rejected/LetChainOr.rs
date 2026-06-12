fn invalid(value: Option<i32>) {
    if let Some(inner) = value || inner > 0 {
        let _ = inner;
    }
}
