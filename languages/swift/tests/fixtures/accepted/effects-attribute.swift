// Pinned Swift 6.3.3 swift-frontend boundaries for opaque @_effects payloads.
@_effects(notEscaping self.value**)
func first() {}

@_effects(escaping self.value**.class*.value** => return.value**)
func second() {}
