// Pinned Swift 6.3.3 swift-frontend parameter modifier/name boundaries.
func modified(_const _ map: String) {}
func stacked(isolated _const _ map: String) {}
func names(_const map: String, isolated: String, isolated _const: String) {}
func types(_ a: _const borrowing String, _ b: borrowing _const String) {}

let closure = { (_const _ x: Int) in x }

enum E {
  case value(_const Int)
  case labeled(_const: Int)
}
