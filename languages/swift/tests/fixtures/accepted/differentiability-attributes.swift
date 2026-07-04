@differentiable(reverse, wrt: self)
func differentiableMap() {}

@differentiable(reverse, wrt: (self, initialResult))
func differentiableReduce() {}

@differentiable(wrt: x where T: Differentiable)
func generic<T>(_ x: T) {}

@derivative(of: Self.other)
func firstDerivative() {}

@derivative(of: Foo.Self.other)
func qualifiedDerivative() {}

@derivative(of: ??)
func operatorDerivative() {}

@transpose(of: S.instanceMethod, wrt: self)
func transposeMember() {}

@transpose(of: Float.-, wrt: (0, 1))
func transposeOperator() {}

@derivative(of: Swift::Foo.Swift::Bar.Swift::baz(), wrt: quux)
func moduleSelectedDerivative() {}

@derivative(of: Any.method())
func anyDerivative() {}

@derivative(of: Foo.Self<Int>.method())
func genericSelfDerivative() {}

@derivative(of: Foo.$name.method())
func dollarNameDerivative() {}

@derivative(of: Foo.Swift::Any<Int>.method())
func moduleSelectedAnyDerivative() {}
