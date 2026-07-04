func statementConditionTrailingClosures() {
  if test { x in
    x
  } {}

  switch value {
  case _ where self.withLookahead { $0.shouldParsePatternBinding(introducer: introducer) }:
    break
  }
}
