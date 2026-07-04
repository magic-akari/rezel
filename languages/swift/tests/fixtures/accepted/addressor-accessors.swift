// Pinned Swift 6.3.3 swift-frontend boundary for addressor accessors.
struct Addressors {
  var value: Int {
    unsafeAddress { fatalError() }
    unsafeMutableAddress { fatalError() }
  }
}
