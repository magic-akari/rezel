#![allow(dead_code)]

async fn modern(values: &[Option<i32>], fallback: Option<i32>) -> i32 {
    let Some(Some(first)) = values.first() else {
        return 0;
    };
    let mut iter = values.iter();

    if (values.len() > 1 || fallback.is_some())
        && let Some(Some(left)) = iter.next()
        && let Some(Some(right)) = iter.next()
        && left < right
    {
        let add = async |value: i32| -> i32 { value + const { 1 } };
        return add(*left).await;
    }

    while let Some(Some(value)) = iter.next() && *value < 10 {
        break;
    }

    match values.first() {
        Some(value) if let Some(inner) = value && *inner > 0 => *inner,
        _ => *first,
    }
}
