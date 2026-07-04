func ifconfigExprInExpr(baseExpr: MyStruct) {
  globalFunc(
    baseExpr
    #if CONDITION_1
      .methodOne()
    #else
      .methodTwo()
    #endif
  )
}

func ifconfigExprInCondition(baseExpr: MyStruct) {
  if baseExpr
    #if CONDITION_1
      .isReady
    #else
      .isFallback
    #endif
  {}
}
