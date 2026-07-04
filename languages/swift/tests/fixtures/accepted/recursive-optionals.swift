// Pinned Swift 6.3.3 swift-frontend acceptance boundary for recursive type suffixes.
func optionalThenIuo() -> Int?! {}
func iuoThenOptional() -> Int!? {}
func doubleIuo() -> Int!! {}
func functionReturningOptional() -> () -> Int? {}
func optionalFunction() -> (() -> Int)? {}

let someOptional: some P?
let anyOptional: any P?
let someIuo: some P!
let composedOptional: P & Q?
let someComposition: some P & Q
let suppressedOptional: ~Copyable?
let someSuppressedOptional: some ~Copyable?
func packOptional<each T>(_ value: repeat each T?) {}
