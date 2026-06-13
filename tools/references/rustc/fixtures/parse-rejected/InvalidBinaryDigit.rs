macro_rules! sink {
    ($($token:tt)*) => {};
}

fn rejected() {
    sink!(0b0102);
}
