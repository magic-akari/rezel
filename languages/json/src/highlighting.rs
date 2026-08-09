use rezel_common::NodePropSource;

pub(crate) fn json_highlighting() -> NodePropSource {
    #[cfg(feature = "highlight")]
    {
        let tags = rezel_highlight::tags();
        rezel_highlight::style_tags([
            ("String", rezel_highlight::TagSet::from(tags.string)),
            ("Number", rezel_highlight::TagSet::from(tags.number)),
            ("True False", rezel_highlight::TagSet::from(tags.bool_)),
            (
                "PropertyName",
                rezel_highlight::TagSet::from(tags.property_name),
            ),
            ("Null", rezel_highlight::TagSet::from(tags.null)),
            (", :", rezel_highlight::TagSet::from(tags.separator)),
            ("[ ]", rezel_highlight::TagSet::from(tags.square_bracket)),
            ("{ }", rezel_highlight::TagSet::from(tags.brace)),
        ])
        .expect("the pinned JSON highlight selectors are valid")
    }

    #[cfg(not(feature = "highlight"))]
    {
        rezel_common::group_prop().source(|_| None)
    }
}
