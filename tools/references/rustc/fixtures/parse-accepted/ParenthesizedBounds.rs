#![allow(dead_code)]

fn generic<T: (?Sized) + (Send)>() {}

fn predicate<T>()
where
    T: (?Sized) + (Send),
{
}

trait Associated: (Send) {
    type Value: (?Sized) + (Send);
}

fn higher_ranked<T: (for<'a> Fn(&'a u8))>() {}

fn higher_ranked_subject<T>()
where
    for<'a> &'a T: IntoIterator,
{
}

type Object = dyn (Send) + (Sync);
type Callback = dyn Send + (for<'a> Fn(&'a u8));
fn associated_constraint<T: Iterator<Item: (Copy)>>() {}

fn opaque() -> impl (Copy) + (Send) {
    0u8
}
