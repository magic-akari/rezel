macro_rules! sink {
    ($($token:tt)*) => {};
}

fn rejected() {
    sink!(2em);
}
