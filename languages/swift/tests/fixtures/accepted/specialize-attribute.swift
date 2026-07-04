// Pinned Swift 6.3.3 swift-frontend boundaries for specialize attributes.
@_specialize(exported: true, kind: full, availability: iOS, introduced: 15.4; where T == Swift.Int)
public func available<T>(_ value: T) {}

@_specialize(where T == Int, U == Float)
func generic<T, U>() {}

@_specialize(where T: _Trivial, U: _Trivial(32), V: _TrivialAtMost(64, 8), W: _TrivialStride(16), X: _BridgeObject)
func layouts<T, U, V, W, X>() {}

@specialized(where Array<T> == Int)
func specialized<T>() {}

@_specialize(target: _appendElementAssumeUniqueAndCapacity(_:newElement:), spi: Private, where T == Swift::Int)
func targeted<T>() {}

@_specialize(target: _makeUniqueAndReserveCapacityIfNotUnique(), where T == Swift::Int)
func zeroArgumentTarget<T>() {}
