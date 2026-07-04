// Pinned Swift 6.3.3 swift-frontend boundary for module selectors.
import struct ModuleSelectorTestingKit::A

@main::available var selectedValue

struct CreatesDeclExpectation {
    #main::myMacro()
}

func selected(_ value: Swift::Any) -> Swift::Bool {
    _ = Swift::print
    _ = value.Swift::self
    _ = #main::myMacro()
    _ = \main::Foo.BarKit::bar
    return Swift::true
}
