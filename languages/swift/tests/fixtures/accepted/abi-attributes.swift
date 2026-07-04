@abi(func stableFunction() -> Int)
func currentFunction() -> Int { 0 }

struct ABIContainer {
  @abi(init())
  init() {}

  @abi(subscript(index: Int) -> Int)
  subscript(index: Int) -> Int { index }
}
