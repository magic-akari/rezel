macro_rules! sink {
    ($($token:tt)*) => {};
}

fn rejected() {
    sink!(0x80.0);
}
