// Pinned Swift 6.3.3 swift-frontend boundary for inline array type sugar.
typealias ThreeIntegers = [3 of Int]
typealias NestedInlineArray = [[3 of Int] of Int]

func consumeInlineArray(_ value: [Int of _]) {}
