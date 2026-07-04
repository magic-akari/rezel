protocol P {}

func contextualAnyTypes(_ value: any P) {
  _ = (any P).self
  _ = (any ~Copyable).self
  _ = [any P & ~Copyable]()
  _ = any~Copyable
}
