#[path = "identifier_2_4_10.rs"]
mod profile;

pub(crate) fn is_start(value: u32) -> bool {
    if value < 0x80 {
        let value = u8::try_from(value).expect("ASCII code points fit in u8");
        return value.is_ascii_alphabetic() || value == b'_';
    }
    in_ranges(profile::IDENTIFIER_START, value)
}

pub(crate) fn is_part(value: u32) -> bool {
    if value < 0x80 {
        let value = u8::try_from(value).expect("ASCII code points fit in u8");
        return value.is_ascii_alphanumeric() || value == b'_';
    }
    in_ranges(profile::IDENTIFIER_PART, value)
}

fn in_ranges(ranges: &[(u32, u32)], value: u32) -> bool {
    let index = ranges.partition_point(|&(_, end)| end < value);
    ranges
        .get(index)
        .is_some_and(|&(start, end)| start <= value && value <= end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_profile_matches_kotlin_2_4_10() {
        assert_eq!(profile::KOTLIN_VERSION, "2.4.10");
        assert_eq!(
            profile::TABLE_SHA256,
            "cdef078f3a09da5203cb3e7fbaf15e2b2e83fad78019142770dcccef5fc7c6be"
        );
        assert_eq!(profile::IDENTIFIER_START.len(), 610);
        assert_eq!(profile::IDENTIFIER_PART.len(), 651);
        assert!(is_start(u32::from('_')));
        assert!(is_start(u32::from('λ')));
        assert!(is_part(0x11066));
        assert!(!is_start(0x11066));
    }
}
