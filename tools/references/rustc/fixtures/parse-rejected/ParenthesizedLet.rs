fn invalid(value: Option<i32>) {
    if (let Some(inner) = value) {
        let _ = inner;
    }
}
