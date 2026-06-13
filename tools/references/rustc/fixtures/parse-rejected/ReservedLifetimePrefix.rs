macro_rules! sink {
    ($($token:tt)*) => {};
}

fn rejected() {
    sink!('prefix#lt);
}
