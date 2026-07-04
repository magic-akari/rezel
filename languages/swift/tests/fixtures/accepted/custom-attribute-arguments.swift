// Pinned Swift 6.3.3 swift-frontend boundaries for custom attribute arguments.
@lifetime(borrow source)
func borrowed(source: borrowing Int) {}

@lifetime(result: copy source)
func copied(source: borrowing Int) {}

@lifetime(&source)
func inherited(source: inout Int) {}

@main::available(foo: bar)
func moduleSelected() {}
