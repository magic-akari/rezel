#![forbid(unsafe_code)]

use rezel_common::{ParseErrorKind, SyntaxNode};
use rezel_lr::ParseLimits;

// SwiftSyntax 60e8eb850721, Tests/SwiftParserTest/DeclarationTests.swift.
const DECLARATION_CASES: &[(&str, &str)] = &[
    ("line 21", "import Foundation"),
    ("line 31", "struct Foo {}"),
    ("line 35", "func foo() {}"),
    ("line 352", "z\n\nvar x: Double = z"),
];

const CALLABLE_NAME_CASES: &[(&str, &str)] = &[
    (
        "OperatorsTests.swift:114",
        "func == (x: Man, y: Five) -> ManIsFive {}",
    ),
    (
        "OperatorsTests.swift:124",
        "func => (x: TheDevilIsSix, y: GodIsSeven) -> TheDevilIsSixThenGodIsSeven {}",
    ),
    (
        "OperatorsTests.swift:163",
        "postfix func *!* (x: LOOK) -> LOOKBang {}\nprefix func *!* (x: LOOKBang) {}",
    ),
    (
        "OperatorsTests.swift:248",
        "func &(x: Man, y: Man) -> Man { return x }",
    ),
    (
        "stdlib/public/Concurrency/AsyncThrowingStream.swift:218",
        "storage.yield(value)",
    ),
    (
        "stdlib/public/core/Integers.swift:941",
        "static prefix func ~ (_ x: Self) -> Self",
    ),
];

// SwiftSyntax 60e8eb850721, operator and precedence-group parser cases.
const OPERATOR_DECLARATION_CASES: &[(&str, &str, &[&str])] = &[
    (
        "OperatorDeclDesignatedTypesTests.swift:71",
        "prefix operator ^^ : PrefixMagicOperatorProtocol\ninfix operator <*< : MediumPrecedence, InfixMagicOperatorProtocol\npostfix operator ^^ : PostfixMagicOperatorProtocol",
        &[
            "OperatorDeclaration",
            "OperatorName",
            "OperatorPrecedenceAndTypes",
            "OperatorDesignatedType",
        ],
    ),
    (
        "OperatorDeclDesignatedTypesTests.swift:161",
        "infix operator <*<<< : MediumPrecedence, &",
        &[
            "OperatorDeclaration",
            "OperatorPrecedenceAndTypes",
            "OperatorDesignatedType",
        ],
    ),
    (
        "OperatorDeclDesignatedTypesTests.swift:189",
        "infix operator <*<>*> : AdditionPrecedence,",
        &["OperatorDeclaration", "OperatorPrecedenceAndTypes"],
    ),
    (
        "DeclarationTests.swift:650",
        "precedencegroup FooGroup {\n  higherThan: Group1, Group2\n  lowerThan: Group3, Group4\n  associativity: left\n  assignment: false\n}",
        &[
            "PrecedenceGroupDeclaration",
            "PrecedenceGroupAssociativity",
            "PrecedenceGroupAssignment",
            "PrecedenceGroupRelation",
            "PrecedenceGroupName",
        ],
    ),
    (
        "OperatorsTests.swift:53",
        "infix operator => : FatArrow\nprecedencegroup FatArrow {\n  associativity: right\n  higherThan: AssignmentPrecedence\n}\nprecedencegroup AssignmentPrecedence {\n  assignment: true\n}",
        &[
            "OperatorDeclaration",
            "PrecedenceGroupDeclaration",
            "PrecedenceGroupAssignment",
        ],
    ),
    (
        "OperatorsTests.swift:68",
        "precedencegroup DefaultPrecedence {}",
        &["PrecedenceGroupDeclaration"],
    ),
    (
        "OperatorDeclTests.swift:512,523",
        "precedencegroup BangBangBang {\n  associativity: none\n  associativity: left\n}\nprecedencegroup CaretCaretCaret {\n  assignment: true\n  assignment: false\n}",
        &[
            "PrecedenceGroupDeclaration",
            "PrecedenceGroupAssociativity",
            "PrecedenceGroupAssignment",
        ],
    ),
];

// `OperatorLike` accepts postfix-lexed operator names; the mise-pinned Swift
// 6.3.3 `swift-frontend -parse` oracle accepts these no-trivia boundaries.
const GLUED_OPERATOR_NAME_CASE: &str = "infix operator& : P\ninfix operator~ : P";

const BINDING_PATTERN_CASES: &[(&str, &str)] = &[
    ("recursive tuple smoke", "let (head, (left, right)) = value"),
    (
        "DeclarationTests.swift:3884",
        "let (x: Int, using: String) = (x: 42, using: \"\")",
    ),
    (
        "PatternWithoutVariablesTests.swift:52",
        "var (_, _) = (1, 2)",
    ),
    (
        "stdlib/public/Concurrency/ContinuousClock.swift:47",
        "let (secHi, secLo) = s.multipliedFullWidth(by: 1_000_000_000_000_000_000)",
    ),
];

// SwiftSyntax 60e8eb850721, translated/DeprecatedWhereTests.swift. These are
// the eight accepted-source groups that contain a Swift 4 generic where clause
// before the closing angle bracket.
const DEPRECATED_GENERIC_WHERE_CASES: &[(&str, &str)] = &[
    (
        "DeprecatedWhereTests.swift:39",
        "func f2<T: Mashable>(x: T) {}\nfunc f3<T where T: Womparable>(x: T) {}\nfunc f4<T>(x: T) -> Int { return 2 }\nfunc f5<T>(x: T) where T: Equatable {}",
    ),
    (
        "DeprecatedWhereTests.swift:55",
        "func f12<T: Mashable where T: Womparable>(x: T) {}\nfunc f13<T: Mashable>(x: T) -> Int { return 2 }\nfunc f14<T: Mashable>(x: T) where T: Equatable {}\nfunc f23<T where T: Womparable>(x: T) -> Int { return 2 }\nfunc f24<T where T: Womparable>(x: T) where T: Equatable {}\nfunc f34<T>(x: T) -> Int where T: Equatable { return 2 }",
    ),
    (
        "DeprecatedWhereTests.swift:75",
        "func f123<T: Mashable where T: Womparable>(x: T) -> Int { return 2 }\nfunc f124<T: Mashable where T: Womparable>(x: T) where T: Equatable {}\nfunc f234<T where T: Womparable>(x: T) -> Int where T: Equatable { return 2 }",
    ),
    (
        "DeprecatedWhereTests.swift:89",
        "func f1234<T: Mashable where T: Womparable>(x: T) -> Int where T: Equatable { return 2 }",
    ),
    (
        "DeprecatedWhereTests.swift:108",
        "struct S1<T: Mashable> {}\nstruct S2<T where T: Womparable> {}\nstruct S3<T> where T: Equatable {}",
    ),
    (
        "DeprecatedWhereTests.swift:122",
        "struct S12<T: Mashable where T: Womparable> {}\nstruct S13<T: Mashable> where T: Equatable {}\nstruct S23<T where T: Womparable> where T: Equatable {}",
    ),
    (
        "DeprecatedWhereTests.swift:136",
        "struct S123<T: Mashable where T: Womparable> where T: Equatable {}",
    ),
    (
        "DeprecatedWhereTests.swift:146",
        "protocol ProtoA {}\nprotocol ProtoB {}\nprotocol ProtoC {}\nprotocol ProtoD {}\nfunc testCombinedConstraints<T: ProtoA & ProtoB where T: ProtoC>(x: T) {}\nfunc testCombinedConstraints<T: ProtoA & ProtoB where T: ProtoC>(x: T) where T: ProtoD {}",
    ),
];

const MATCHING_PATTERN_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/AsyncStreamBuffer.swift:137",
        "switch limit { case .bufferingOldest(let limit): return limit }",
    ),
    (
        "stdlib/public/RuntimeModule/Address.swift:29",
        "switch representation { case let .sixteenBit(addr): return UInt64(addr) }",
    ),
    (
        "stdlib/public/RuntimeModule/BacktraceFormatter.swift:488",
        "if case let .columns(columns) = $0 { return columns.count }",
    ),
    (
        "stdlib/public/RuntimeModule/Backtrace.swift:56",
        "switch value { case .sixteenBit(_): break }",
    ),
    (
        "MatchingPatternsTests.swift:158",
        "switch e { case is A<Int>.C<Int>: break }",
    ),
];

// SwiftSyntax 60e8eb850721 matching-pattern contexts that recurse into
// tuple/call arguments and postfix optional patterns.
const RECURSIVE_MATCHING_PATTERN_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/core/CollectionDifference.swift:126",
        "switch changes { case (.remove(_, _, _), .insert(_, _, _)): () }",
    ),
    (
        "stdlib/public/core/PrefixWhile.swift:190",
        "switch pair { case (.pastEnd, _): () }",
    ),
    (
        "stdlib/public/core/StringStorageBridge.swift:67",
        "switch pair { case (_cocoaUTF8Encoding, _): () }",
    ),
    (
        "stdlib/public/core/StringWordBreaking.swift:273",
        "switch pair { case (.newlineCRLF, _), (_, .newlineCRLF): () }",
    ),
    (
        "SwiftIfConfig/IfConfigRegionState.swift:44",
        "switch pair { case (true, _): () }",
    ),
    (
        "MatchingPatternsTests.swift:545",
        "switch op1 { case _?: break }",
    ),
    (
        "MatchingPatternsTests.swift:557",
        "switch op2 { case _?: break }",
    ),
    (
        "PatternWithoutVariablesTests.swift:76",
        "switch (a, 42) { case let (_, x): _ = x; break }",
    ),
];

// Fixed SwiftSyntax 60e8eb850721 binding-introducer pattern witnesses.
const BINDING_INTRODUCER_PATTERN_CASES: &[(&str, &str)] = &[
    ("PatternTests.swift:54", "if case let E<Int>.e(y) = x {}"),
    ("PatternTests.swift:106", "if case let (y[0], z) = x {}"),
    ("PatternTests.swift:150", "if case let y[z] = x {}"),
    (
        "MatchingPatternsTests.swift:405",
        "switch value { case .Payload(let x): () }",
    ),
    (
        "PatternWithoutVariablesTests.swift:89",
        "switch (a, 42) { case let (_, x): _ = x; break }",
    ),
    (
        "stdlib/public/core/DebuggerSupport.swift:167",
        "switch value { case let x?: () }",
    ),
    ("ordinary matching reference", "switch 0 { case x: () }"),
];

const GENERIC_DISAMBIGUATION_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/AsyncStreamBuffer.swift:138",
        "if count < limit {}",
    ),
    ("GenericDisambiguationTests.swift:69", "(a < b, c > d)"),
    ("GenericDisambiguationTests.swift:78", "(a < b, c > (d))"),
    ("GenericDisambiguationTests.swift:99", "generic<Int>(0)"),
    ("GenericDisambiguationTests.swift:107", "A<A<B>>.c()"),
    ("GenericDisambiguationTests.swift:216", "A<>.c()"),
    ("ordinary generic member", "A<Int>.member"),
    ("commented generic member", "A<Int>/* comment */.member"),
];

const PREFIX_OPERATOR_LINES: &str = "let x: () = ()\n!()\n!(())\n!(x)\n!x";

const MULTILINE_OPERATOR_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/AsyncStream.swift:339",
        "let storage: _AsyncStreamCriticalStorage<Optional<() async -> Element?>>\n  = .create(produce)",
    ),
    (
        "stdlib/public/Concurrency/CooperativeExecutor.swift:23",
        "return MemoryLayout<(Int, Int)>.size\n  < MemoryLayout<CooperativeExecutor.Timestamp>.size",
    ),
    (
        "SwiftSyntax Sources/SwiftSyntaxMacroExpansion/MacroSystem.swift:1506",
        "return node.leadingTrivia.description\n  + self\n  + node.trailingTrivia.description",
    ),
    (
        "stdlib/public/Concurrency/DispatchExecutor.swift:103",
        "if components.seconds < 0\n  || components.seconds == 0 && components.attoseconds < 0 {}",
    ),
    (
        "stdlib/public/core/StringObject.swift:266-267",
        "let payload = UInt64(truncatingIfNeeded: discriminatedObjectRawBits)\n  & _StringObject.Nibbles.largeAddressMask",
    ),
    (
        "stdlib/public/core/UTF8.swift:212-213",
        "let top5bits = _buffer._storage\n  & 0b0__0111__0011_0000__0000_0000__0000_0000",
    ),
    ("InvalidTests.swift:694", PREFIX_OPERATOR_LINES),
];

const CLOSURE_BODY_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/AsyncStream.swift:342",
        "return await withTaskCancellationHandler { return nil } onCancel: {}",
    ),
    (
        "stdlib/public/Concurrency/AsyncSequence.swift:256",
        "return try await !contains { try await !predicate($0) }",
    ),
    (
        "ExpressionTests.swift:3813",
        "let (ids, (actions, tracking)) = state.withCriticalRegion { ($0.valueObservers(for: keyPath), $0.didSet(keyPath: keyPath)) }",
    ),
    (
        "SwiftSyntax Sources/SwiftLexicalLookup/QualifiedLookup/DeclName.swift:96",
        "let argumentList = arguments.map({ ($0?.name ?? \"_\") + \":\" }).joined(separator: \"\")",
    ),
    ("discard-assignment body smoke", "{ _ = value }"),
];

const TYPED_CLOSURE_PARAMETER_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/MainActor.swift:149",
        "withoutActuallyEscaping(operation) { (_ fn: @escaping YesActor) throws -> T in fn() }",
    ),
    ("ExpressionTests.swift:3706", "{ (_ x: MyType) in }"),
    (
        "ExpressionTests.swift:3720",
        "{ (@_noImplicitCopy _ x: Int) -> () in }",
    ),
];

const CLOSURE_SIGNATURE_EFFECT_CASES: &[(&str, &str, &str, usize, usize)] = &[
    (
        "ExpressionTests.swift:4017",
        "try foo { (a, b) throws(S) in 1 }",
        "ClosureParameter",
        1,
        0,
    ),
    (
        "ExpressionTests.swift:4025",
        "try foo { a, b throws(S) in 1 }",
        "ClosureShorthandParameter",
        1,
        0,
    ),
    (
        "ClosureMissingInTests.swift:66",
        "let closure = { x, _ -> Int in x }",
        "ClosureShorthandParameter",
        0,
        1,
    ),
    (
        "stdlib/public/Concurrency/Deque/Deque+Collection.swift:337",
        "storage.update { handle -> (_Slot, Element) in handle }",
        "ClosureShorthandParameter",
        0,
        1,
    ),
    (
        "stdlib/public/core/Dictionary.swift:498",
        "merge { _, _ throws(_MergeError) in throw error }",
        "ClosureShorthandParameter",
        1,
        0,
    ),
    (
        "stdlib/public/core/Span/MutableSpan.swift:396",
        "body { buffer throws(E) -> Result in buffer }",
        "ClosureShorthandParameter",
        1,
        1,
    ),
];

const OPERATOR_REFERENCE_CASES: &[(&str, &str, &str)] = &[
    ("ExpressionTests.swift:1762", "reduce(0, +)", "+"),
    (
        "stdlib/public/Concurrency/AsyncSequence.swift:465",
        "return try await self.min(by: <)",
        "<",
    ),
    (
        "stdlib/public/Concurrency/PriorityQueue.swift:210",
        "self.init(compare: >)",
        ">",
    ),
];

const RANGE_OPERATOR_CASES: &[(&str, &str, &str)] = &[
    (
        "stdlib/public/Concurrency/Deque/Deque+Collection.swift:100",
        "unsafe target[..<c]._rebased()._initialize(from: segments.first)",
        "PrefixOperatorExpression",
    ),
    (
        "stdlib/public/RuntimeModule/FramePointerUnwinder.swift:71",
        "let beforeIndex = withoutUnderscore[...beforeIndexNdx]",
        "PrefixOperatorExpression",
    ),
    (
        "stdlib/public/SwiftOnoneSupport/SwiftOnoneSupport.swift:365",
        "(0..<0)._prespecializeCollection(index: 0, range: (0..<0))",
        "SequenceExpression",
    ),
    (
        "stdlib/public/SwiftOnoneSupport/SwiftOnoneSupport.swift:372",
        "(0...0)._prespecializeClosedRange()",
        "SequenceExpression",
    ),
    (
        "stdlib/public/core/Diffing.swift:113",
        "result.append(contentsOf: self[currentIndex...])",
        "PostfixOperatorExpression",
    ),
];

const TUPLE_MEMBER_CASES: &[(&str, &str, usize)] = &[
    ("LexerTests.swift:121", "x.1.0", 2),
    ("LexerTests.swift:1433", "x.13.1", 2),
    (
        "ExpressionInterpretedAsVersionTupleTests.swift:20",
        "1.2.3.4",
        2,
    ),
    (
        "stdlib/public/RuntimeModule/Base64.swift:160",
        "return output.0",
        1,
    ),
];

const INVALID_IMPLICIT_TUPLE_MEMBER: (&str, &str) = ("RecoveryTests.swift:1972", ".42");

const AMPERSAND_OPERATOR_CASES: &[(&str, &str, &str)] = &[
    ("ExpressionTests.swift:70", "(&y[0])", "InOutExpression"),
    (
        "stdlib/public/Concurrency/Task.swift:453",
        "Kind(rawValue: bits & 0xFF)!",
        "SequenceExpression",
    ),
    (
        "stdlib/public/RuntimeModule/Elf.swift:302",
        "var theCrc = ~crc",
        "PrefixOperatorExpression",
    ),
    (
        "stdlib/public/RuntimeModule/ImageSource.swift:219",
        "let roundedExtra = (extra + 0xffff) & ~0xffff",
        "PrefixOperatorExpression",
    ),
];

const IDENTIFIER_CASES: &[(&str, &str)] = &[
    ("IdentifiersTests.swift:45", "你好.שלום.வணக்கம்.Γειά.привет()"),
    (
        "IdentifiersTests.swift:62",
        "// Combining characters can be used within identifiers.\nfunc s̈pin̈al_tap̈() {}",
    ),
    ("EscapedIdentifiersTests.swift:19", "func `protocol`() {}"),
    ("EscapedIdentifiersTests.swift:27", "`protocol`()"),
    (
        "EscapedIdentifiersTests.swift:43",
        "var `class` = `Type`.self",
    ),
    (
        "DollarIdentifierTests.swift:214",
        "func escapedDollarAnd() {\n  `$0` = 1\n  `$$` = 2\n  `$abc` = 3\n}",
    ),
    (
        "DollarIdentifierTests.swift:310",
        "let _ = S().$café // Okay",
    ),
];

const DECLARATION_SHELL_CASES: &[(&str, &str)] = &[
    ("DeclarationTests.swift:23", "@_spi(Private) import SwiftUI"),
    (
        "DeclarationTests.swift:25",
        "@_exported import class Foundation.Thread",
    ),
    (
        "DeclarationTests.swift:27",
        "@_private(sourceFile: \"YetAnotherFile.swift\") import Foundation",
    ),
    (
        "DeclarationTests.swift:37",
        "func foo() -> Slice<MinimalMutableCollection<T>> {}",
    ),
    (
        "DeclarationTests.swift:39",
        "func onEscapingAutoclosure(_ fn: @Sendable @autoclosure @escaping () -> Int) { }\nfunc onEscapingAutoclosure2(_ fn: @escaping @autoclosure @Sendable () -> Int) { }\nfunc bar(_ : String) async -> [[String]: Array<String>] {}\nfunc tupleMembersFunc() -> (Type.Inner, Type2.Inner2) {}\nfunc myFun<S: T & U>(var1: S) {\n  // do stuff\n}",
    ),
    ("DeclarationTests.swift:172", "class Foo {}"),
    (
        "DeclarationTests.swift:174",
        "@dynamicMemberLookup @available(swift 4.0)\npublic class MyClass {\n  let A: Int\n  let B: Double\n}",
    ),
    ("DeclarationTests.swift:221", "actor Foo {}"),
    ("DeclarationTests.swift:310", "protocol Foo {}"),
    ("DeclarationTests.swift:312", "protocol P { init() }"),
    (
        "DeclarationTests.swift:314",
        "protocol P {\n  associatedtype Foo: Bar where X.Y == Z.W.W.Self\n\n  var foo: Bool { get set }\n  subscript<R>(index: Int) -> R\n}",
    ),
    (
        "DeclarationTests.swift:369",
        "private unowned(unsafe) var foo: Int",
    ),
    (
        "DeclarationTests.swift:370",
        "unowned(unsafe) let unmanagedVar: Class = c",
    ),
    ("DeclarationTests.swift:389", "@Wrapper var café = 42"),
    (
        "DeclarationTests.swift:391",
        "var x: T {\n  get async {\n    foo()\n    bar()\n  }\n}",
    ),
    ("DeclarationTests.swift:640", "typealias Foo = Int"),
    (
        "DeclarationTests.swift:642",
        "typealias MyAlias = (_ a: Int, _ b: Double, _ c: Bool, _ d: String) -> Bool",
    ),
    (
        "DeclarationTests.swift:897",
        "enum Foo {\n  @preconcurrency case custom(@Sendable () throws -> Void)\n}",
    ),
    (
        "DeclarationTests.swift:905",
        "enum Content {\n  case keyPath(KeyPath<FocusedValues, Value?>)\n  case keyPath(KeyPath<FocusedValues, Binding<Value>?>)\n  case value(Value?)\n}",
    ),
    (
        "DeclarationTests.swift:1717",
        "struct S0 {\n  init!(int: Int) { }\n  init! (uint: UInt) { }\n  init !(float: Float) { }\n\n  init?(string: String) { }\n  init ?(double: Double) { }\n  init ? (char: Character) { }\n}",
    ),
    (
        "DeclarationTests.swift:806",
        "extension Int: @retroactive Identifiable {}",
    ),
    (
        "DeclarationTests.swift:814",
        "struct MyValue: @preconcurrency P {}",
    ),
    (
        "DeclarationTests.swift:820",
        "extension MyValue: @preconcurrency P {}",
    ),
    (
        "DeclarationTests.swift:828",
        "extension Int: nonisolated Q {}",
    ),
    (
        "DeclarationTests.swift:834",
        "extension Int: @MainActor P {}",
    ),
    (
        "DeclarationTests.swift:840",
        "extension Int: @preconcurrency nonisolated Q {}",
    ),
    (
        "DeclarationTests.swift:846",
        "extension Int: @unsafe nonisolated Q {}",
    ),
];

const STATEMENT_CASES: &[(&str, &str)] = &[
    ("StatementTests.swift:19", "if let baz {}"),
    (
        "ModuleSelectorTests.swift:1833",
        "let x = if y { 1 } else { 0 }",
    ),
    ("StatementTests.swift:132", "do {}"),
    ("StatementTests.swift:142", "do {} catch {}"),
    (
        "StatementTests.swift:1204",
        "do throws(any Error) {\n  throw myError\n}",
    ),
    (
        "StatementTests.swift:199",
        "switch x {\ncase .A, .B:\n  break\n}",
    ),
    (
        "SwitchTests.swift:523",
        "switch x {\ncase 0:\n  fallthrough\ncase 1:\n  fallthrough\ndefault:\n  fallthrough\n}",
    ),
    ("ForeachTests.swift:37", "for i in r {\n  sum = sum + i\n}"),
    (
        "ForeachAsyncTests.swift:60",
        "for await i in r {\n  sum = sum + i\n}",
    ),
    (
        "StatementTests.swift:1239",
        "for try await unsafe x in e { }",
    ),
    ("RecoveryTests.swift:3250", "guard foo else {}"),
    (
        "SwiftParser/Declarations.swift:195",
        "repeat {\n  lookahead.consumeAnyToken()\n} while lookahead.atStartOfDeclaration(allowInitDecl: allowInitDecl, requiresDecl: requiresDecl)",
    ),
    ("TryTests.swift:295", "while true { break }"),
    (
        "Parser+EntryTests.swift:44",
        "defer { buffer.deallocate() }",
    ),
    ("ThenStatementTests.swift:631", "throw then"),
    (
        "StatementTests.swift:712",
        "var x: Int {\n  _read {\n    yield &x\n  }\n}",
    ),
    ("StatementTests.swift:860", "discard self"),
];

// SwiftSyntax 60e8eb850721, feature-enabled parser cases. These are kept
// separate from the ordinary statement matrix because they deliberately select
// the experimental DoExprSyntax and ThenStmtSyntax roles.
const DO_EXPRESSION_CASES: &[(&str, &str)] = &[
    ("DoExpressionTests.swift:23", "let x = do { 5 }"),
    ("DoExpressionTests.swift:36", "let x = do { 5 } catch { 0 }"),
    (
        "DoExpressionTests.swift:117",
        "y = do { 5 } catch { 0 } as Int",
    ),
    ("DoExpressionTests.swift:151", "do {\n  ()\n  then 5\n}"),
    ("DoExpressionTests.swift:233", "return do { 5 }"),
];

const THEN_STATEMENT_CASES: &[(&str, &str)] = &[
    ("ThenStatementTests.swift:24", "then 0"),
    (
        "ThenStatementTests.swift:118",
        "then if .random() { 0 } else { 1 }",
    ),
    ("ThenStatementTests.swift:141", "then ~1"),
    ("ThenStatementTests.swift:156", "then /.../"),
    ("ThenStatementTests.swift:201", "a: then 0"),
    ("ThenStatementTests.swift:501", "then try 0"),
];

// SwiftSyntax 60e8eb850721, dedicated differentiation-attribute parsers.
const DIFFERENTIABILITY_ATTRIBUTE_CASES: &[(&str, &str)] = &[
    (
        "AttributeTests.swift:243",
        "func f(input: @differentiable(reverse, wrt: value) (Int) -> Int) {}",
    ),
    (
        "AttributeTests.swift:257",
        "@differentiable(reverse, wrt: self)\nfunc differentiableMap() {}",
    ),
    (
        "AttributeTests.swift:269",
        "@differentiable(reverse, wrt: (self, initialResult))\nfunc differentiableReduce() {}",
    ),
    (
        "DeclarationTests.swift:699",
        "@differentiable(wrt: x where T: D)\nfunc generic<T>(_ x: T) {}",
    ),
    (
        "AttributeTests.swift:297",
        "@derivative(of: Self.other)\nfunc firstDerivative() {}",
    ),
    (
        "AttributeTests.swift:306",
        "@derivative(of: Foo.Self.other)\nfunc qualifiedDerivative() {}",
    ),
    (
        "stdlib/public/Differentiation/OptionalDifferentiation.swift:74",
        "@derivative(of: ??)\nfunc operatorDerivative() {}",
    ),
    (
        "AttributeTests.swift:319",
        "@transpose(of: S.instanceMethod, wrt: self)\nfunc transposeMember() {}",
    ),
    (
        "AttributeTests.swift:346",
        "@transpose(of: Float.-, wrt: (0, 1))\nfunc transposeOperator() {}",
    ),
    (
        "ModuleSelectorTests.swift:1752",
        "@derivative(of: Swift::Foo.Swift::Bar.Swift::baz(), wrt: quux)\nfunc moduleSelectedDerivative() {}",
    ),
];

// SwiftSyntax 60e8eb850721, AttributeTests.swift `testABIAttribute`.
const ABI_ATTRIBUTE_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/Task+PriorityEscalation.swift:123",
        r"@abi(
  func withTaskPriorityEscalationHandler<T, E>(
    operation: () async throws(E) -> T,
    onPriorityEscalated handler: @Sendable (TaskPriority, TaskPriority) -> Void,
    isolation: isolated (any Actor)?
  ) async throws(E) -> T
)
public func current<T, E>() {}",
    ),
    (
        "AttributeTests.swift:989",
        "@abi(func fn() -> Int)\nfunc fn1() -> Int {}",
    ),
    (
        "AttributeTests.swift:1017",
        "@abi(associatedtype AssocTy)\nassociatedtype AssocTy",
    ),
    ("AttributeTests.swift:1023", "@abi(deinit)\ndeinit {}"),
    (
        "AttributeTests.swift:1029",
        "enum E { @abi(case someCase) case someCase }",
    ),
    ("AttributeTests.swift:1037", "@abi(func fn())\nfunc fn()"),
    ("AttributeTests.swift:1043", "@abi(init())\ninit() {}"),
    (
        "AttributeTests.swift:1049",
        "@abi(subscript(i: Int) -> Element)\nsubscript(i: Int) -> Element {}",
    ),
    (
        "AttributeTests.swift:1055",
        "@abi(typealias Typealias = @escaping () -> Void)\ntypealias Typealias = () -> Void",
    ),
    ("AttributeTests.swift:1061", "@abi(let c1, c2)\nlet c1, c2"),
    ("AttributeTests.swift:1067", "@abi(var v1, v2)\nvar v1, v2"),
    (
        "AttributeTests.swift:1117",
        "@abi(associatedtype AssocTy = T)\nassociatedtype AssocTy",
    ),
    ("AttributeTests.swift:1123", "@abi(deinit {})\ndeinit {}"),
    (
        "AttributeTests.swift:1129",
        "enum E { @abi(case someCase = 42) case someCase }",
    ),
    ("AttributeTests.swift:1137", "@abi(func fn() {})\nfunc fn()"),
    ("AttributeTests.swift:1143", "@abi(init() {})\ninit() {}"),
    (
        "AttributeTests.swift:1149",
        "@abi(subscript(i: Int) -> Element { get {} set {} })\nsubscript(i: Int) -> Element {}",
    ),
    (
        "AttributeTests.swift:1155",
        "@abi(let c1 = 1, c2 = 2)\nlet c1, c2",
    ),
    (
        "AttributeTests.swift:1161",
        "@abi(var v1 = 1, v2 = 2)\nvar v1, v2",
    ),
    (
        "AttributeTests.swift:1167",
        "@abi(var v3 { get {} set {} })\nvar v3",
    ),
];

// SwiftSyntax 60e8eb850721, feature-enabled UsingDeclarationTests.
const USING_DECLARATION_CASES: &[(&str, &str)] = &[
    ("DeclarationTests.swift:3684", "using @MainActor"),
    ("DeclarationTests.swift:3697", "using nonisolated"),
    ("DeclarationTests.swift:3705", "using @Test"),
    ("DeclarationTests.swift:3719", "using test"),
    (
        "DeclarationTests.swift:3727",
        "using @warn(DiagGroupID, as: warning)",
    ),
    (
        "DeclarationTests.swift:3760",
        "using @diagnose(DiagGroupID, as: error)",
    ),
    ("DeclarationTests.swift:3886", "do { using @MainActor }"),
];

const IF_CONFIG_CASES: &[(&str, &str)] = &[
    (
        "Parser+EntryTests.swift:79",
        "#if FLAG\nfunc test() {}\n#endif",
    ),
    (
        "SwiftParser/Directives.swift:13",
        "#if compiler(>=6)\n@_spi(RawSyntax) internal import SwiftSyntax\n#else\n@_spi(RawSyntax) import SwiftSyntax\n#endif",
    ),
    (
        "IfconfigExprTests.swift:146",
        "func emptyElse(baseExpr: MyStruct) {\n  baseExpr\n#if CONDITION_1\n    .methodOne()\n#elseif CONDITION_2\n    // OK. Do nothing.\n#endif\n}",
    ),
    (
        "IfconfigExprTests.swift:187",
        "func nestedIfConfig(baseExpr: MyStruct) {\n  baseExpr\n#if CONDITION_1\n  #if CONDITION_2\n    .methodOne()\n  #endif\n  #if CONDITION_1\n    .methodTwo()\n  #endif\n#else\n  .unknownMethod1()\n  #if CONDITION_2\n    .unknownMethod2()\n  #endif\n#endif\n}",
    ),
    (
        "IfconfigExprTests.swift:206",
        "func ifconfigExprInExpr(baseExpr: MyStruct) {\n  globalFunc(\n    baseExpr\n#if CONDITION_1\n      .methodOne()\n#else\n      .methodTwo()\n#endif\n  )\n}",
    ),
    (
        "IfconfigExprTests.swift:225",
        "#if canImport(A, _version: 2)\nlet a = 1\n#endif",
    ),
    (
        "IfconfigExprTests.swift:621",
        "#if compiler(>=10.0) && hasGreeble(blah)\n#endif",
    ),
    (
        "stdlib/public/Concurrency/TaskLocal.swift:17",
        "#if $Macros && hasAttribute(attached)\n#endif",
    ),
    (
        "stdlib/public/Platform/Platform.swift:316",
        "#if os(Linux) || os(FreeBSD) || os(OpenBSD) || os(PS4) || os(Android) || os(Cygwin) || os(Haiku) || os(WASI)\n#endif",
    ),
    (
        "stdlib/public/RuntimeModule/ByteSwapping.swift:49",
        "#if _endian(little)\n#endif",
    ),
];

const SHORTHAND_CLOSURE_SIGNATURE: &str = "{ [weak self, weak weakB = b] foo in\n  return 0\n}";

const EXPRESSION_CASES: &[(&str, &str)] = &[
    ("ExpressionTests.swift:30", "a ? b : c ? d : e"),
    (
        "ExpressionTests.swift:49",
        "{ @MainActor (a: Int) async -> Int in print(\"hi\") }",
    ),
    ("ExpressionTests.swift:55", SHORTHAND_CLOSURE_SIGNATURE),
    ("ExpressionTests.swift:165", "await a()"),
    ("ExpressionTests.swift:1660", "consume msg"),
    ("ExpressionTests.swift:1670", "let b = (borrow self).buffer"),
    (
        "ExpressionTests.swift:2796",
        "func f() { let x = unsafe y }",
    ),
    (
        "AsyncPrefixSequence.swift:106",
        "return try await baseIterator.next()",
    ),
    ("AsyncPrefixSequence.swift:105", "remaining &-= 1"),
    ("Integers.swift:1408", "return 0..<1"),
    (
        "OperatorsTests.swift:134",
        "Man() == Five() => TheDevil() == Six() => God() == Seven()",
    ),
    (
        "ExpressionTests.swift:908",
        "[Dictionary<String, Int>: Int]()",
    ),
    ("StatementTests.swift:752", "native[key, isUnique: true]"),
    (
        "IfconfigExprTests.swift:107",
        "base.optionalMember?.optionalMethod()![idx]",
    ),
    (
        "ExpressionTests.swift:2257",
        "let values = [1, 2, 3]\nlet lookup = [1: 2, 3: 4]\nlet empty = [:]",
    ),
    ("ExpressionTests.swift:63", "let values = map({ [$0] })"),
    (
        "ExpressionTests.swift:2769",
        "switch Bool.random() { case true: 0 case false: 1 } as? Int",
    ),
];

const SIGNATURE_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/Actor.swift:74",
        "public func _defaultActorInitialize(_ actor: AnyObject)",
    ),
    (
        "SwiftSyntax ConsecutiveStatementsTests.swift:73",
        "func test(i: inout Int, j: inout Int) {}",
    ),
    (
        "stdlib/public/Concurrency/AsyncCompactMapSequence.swift:143",
        "public mutating func next(isolation actor: isolated (any Actor)?) async throws(Failure) -> ElementOfResult? {}",
    ),
    (
        "stdlib/public/Concurrency/AsyncDropWhileSequence.swift:47",
        "public __consuming func drop(while predicate: @Sendable @escaping (Element) async -> Bool) -> AsyncDropWhileSequence<Self> {}",
    ),
    (
        "stdlib/public/Concurrency/AsyncStream.swift:195",
        "public func yield(_ value: sending Element) -> YieldResult {}",
    ),
    (
        "DeclarationTests.swift:1897",
        "func const(_const _ map: String) {}",
    ),
    (
        "DeclarationTests.swift:1909",
        "func isolatedConst(isolated _const _ map: String) {}",
    ),
    (
        "DeclarationTests.swift:3436",
        "func const(_const x y: String) {}",
    ),
    (
        "TypeTests.swift:436",
        "func foo1(_ a: _const borrowing String) {}",
    ),
    (
        "TypeTests.swift:437",
        "func foo2(_ a: borrowing _const String) {}",
    ),
    ("ExpressionTests.swift:3698", "_ = { (_const x: Int) in }"),
];

const TRAILING_CLOSURE_CASES: &[(&str, &str)] = &[
    ("TrailingClosuresTests.swift:28", "foo { 42 }\nb: { \"\" }"),
    ("TrailingClosuresTests.swift:37", "foo { 42 } b: { \"\" }"),
    ("TrailingClosuresTests.swift:28", "foo { 42 }\nbar()"),
    (
        "TrailingClosuresTests.swift:151",
        "multiple_trailing_with_defaults(duration: 42) {} completion: {}",
    ),
    (
        "TrailingClosuresTests.swift:96",
        "let _ = s[true] { 21 } v: { 42 }",
    ),
];

const SPECIAL_NAME_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/Deque/_DequeSlot.swift:27",
        "internal static var zero: Self { get { return Self(at: 0) } }",
    ),
    (
        "stdlib/public/Concurrency/Deque/Deque+ExpressibleByArrayLiteral.swift:28",
        "self.init(elements)",
    ),
    (
        "stdlib/public/Concurrency/AsyncStreamBuffer.swift:34",
        "os_unfair_lock.self",
    ),
    (
        "stdlib/public/Concurrency/PlatformExecutorCooperative.swift:20",
        "public static var mainExecutor: any MainExecutor { get { executor } }",
    ),
];

const PROPERTY_AND_WHERE_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/AsyncCompactMapSequence.swift:163",
        "extension AsyncCompactMapSequence: @unchecked Sendable where Base: Sendable, Base.Element: Sendable, ElementOfResult: Sendable { }",
    ),
    (
        "stdlib/public/Concurrency/AsyncSequence.swift:84",
        "associatedtype Failure: Error = any Error where AsyncIterator.Failure == Failure",
    ),
    (
        "stdlib/public/Concurrency/AsyncFlatMapSequence.swift:80",
        "public func flatMap<SegmentOfResult: AsyncSequence>(_ transform: @Sendable @escaping (Element) async -> SegmentOfResult) -> AsyncFlatMapSequence<Self, SegmentOfResult> where SegmentOfResult.Failure == Failure { }",
    ),
    (
        "stdlib/public/Concurrency/Deque/_DequeSlot.swift:27",
        "internal static var zero: Self { Self(at: 0) }",
    ),
    (
        "stdlib/public/Concurrency/UnimplementedExecutor.swift:40",
        "public var isMainExecutor: Bool { true }",
    ),
    (
        "stdlib/public/Concurrency/AsyncCompactMapSequence.swift:162",
        "extension AsyncCompactMapSequence: @unchecked Sendable\n  where Base: Sendable,\n        Base.Element: Sendable,\n        ElementOfResult: Sendable { }",
    ),
    (
        "stdlib/public/Concurrency/AsyncFlatMapSequence.swift:78",
        "extension AsyncSequence {\n  func flatMap() -> AsyncFlatMapSequence<Self, SegmentOfResult>\n    where SegmentOfResult.Failure == Failure\n  {}\n}",
    ),
    (
        "stdlib/public/Concurrency/AsyncSequence.swift:84",
        "associatedtype Failure: Error = any Error\n    where AsyncIterator.Failure == Failure",
    ),
    (
        "TrailingCommaTests.swift:337",
        "struct T: P1, P2, where P1: Equatable, P2: Equatable, { }",
    ),
    (
        "stdlib/public/Concurrency/CheckedContinuation.swift:28",
        "class CheckedContinuationCanary {\n  static func create()\n    -> CheckedContinuationCanary { fatalError() }\n}",
    ),
    (
        "stdlib/public/Concurrency/DispatchExecutor.swift:99",
        "func clamp(_ components: (seconds: Int64, attoseconds: Int64))\n  -> (seconds: Int64, attoseconds: Int64) {\n  return components\n}",
    ),
];

const ACCESSOR_DISAMBIGUATION_CASES: &[(&str, &str, &str)] = &[
    (
        "DeclarationTests.swift:393",
        "var x: T { get async { foo(); bar() } }",
        "AccessorBlock",
    ),
    (
        "DeclarationTests.swift:418",
        "var foo: Int {\n  @available(swift 5.0)\n  func myFun() -> Int { return 42 }\n  return myFun()\n}",
        "GetterCodeBlock",
    ),
    (
        "DeclarationTests.swift:430",
        "var foo: Int { mutating set { test += 1 } }",
        "AccessorBlock",
    ),
    (
        "DeclarationTests.swift:3898",
        "var value = initialValue { @_accessorBlock get }",
        "AccessorBlock",
    ),
    (
        "DeclarationTests.swift:3935",
        "var x: Int = foo()\n{ @available(*, deprecated) didSet {} }",
        "AccessorBlock",
    ),
    ("empty accessor smoke", "var value: Int {}", "AccessorBlock"),
];

const ACCESSOR_SPECIFIERS: &[&str] = &[
    "get",
    "set",
    "didSet",
    "willSet",
    "unsafeAddress",
    "addressWithOwner",
    "addressWithNativeOwner",
    "unsafeMutableAddress",
    "mutableAddressWithOwner",
    "mutableAddressWithNativeOwner",
    "_read",
    "read",
    "_modify",
    "modify",
    "init",
    "borrow",
    "mutate",
];

const POUND_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/Deque/Compatibility.swift:37",
        "guard #available(macOS 10.15, iOS 13, watchOS 6, tvOS 13, *) else {}",
    ),
    (
        "AvailabilityQueryUnavailabilityTests.swift:22",
        "if #unavailable(OSX 10.51) {}",
    ),
    (
        "AvailabilityQueryUnavailabilityTests.swift:481",
        "if 1 != 2, #unavailable(iOS 8.0) {}",
    ),
    (
        "ExpressionTests.swift:1005",
        "#fancyMacro<Arg1, Arg2>(hello: \"me\")",
    ),
    ("ExpressionTests.swift:1788", "#file == $0.path"),
    (
        "stdlib/public/Concurrency/Clock.swift:71",
        "func measure(isolation: isolated (any Actor)? = #isolation) {}",
    ),
    (
        "SwiftSyntaxMacrosGenericTestSupport/Assertions.swift:132",
        "func location(fileID: StaticString = #fileID, line: UInt = #line) {}",
    ),
];

const MACRO_DECLARATION_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/Actor.swift:100",
        "@freestanding(expression)\npublic macro isolation<T>() -> T = Builtin.IsolationMacro",
    ),
    (
        "stdlib/public/Concurrency/TaskLocal.swift:27",
        "@attached(accessor)\n@attached(peer, names: prefixed(`$`))\npublic macro TaskLocal() =\n  #externalMacro(module: \"SwiftMacros\", type: \"TaskLocalMacro\")",
    ),
    (
        "stdlib/public/core/Macros.swift:25",
        "public macro externalMacro<T>(module: String, type: String) -> T =\n  Builtin.ExternalMacro",
    ),
    (
        "DeclarationTests.swift:2500",
        "macro m3(a b: Int) -> Int = A.M3",
    ),
    ("DeclarationTests.swift:2502", "macro m5<T: P>(_: T)"),
    (
        "SwiftSyntax cases/00079.swift",
        "@attached(extension)\nmacro m()",
    ),
    (
        "SwiftSyntax cases/00068.swift",
        "@attached(member, names: named(init))\nmacro m()",
    ),
    (
        "SwiftSyntax cases/00070.swift",
        "@attached(member, names: named(subscript))\nmacro m()",
    ),
];

const MODERN_TYPE_CASES: &[(&str, &str)] = &[
    (
        "stdlib/public/Concurrency/Actor.swift:108",
        "func extractIsolation<each Arg, Result>(_ fn: @escaping @isolated(any) (repeat each Arg) async throws -> Result) -> (any Actor)? {}",
    ),
    (
        "VariadicGenericsTests.swift:304",
        "func f1<each T>() -> repeat each T {}",
    ),
    (
        "VariadicGenericsTests.swift:123",
        "func zip<each T, each U>(_ first: repeat each T, with second: repeat each U) -> (repeat (each T, each U)) {}",
    ),
    (
        "VariadicGenericsTests.swift:365",
        "typealias Alias<each T> = (repeat each T)",
    ),
    ("DeclarationTests.swift:2753", "struct Hello: ~Copyable {}"),
    ("DeclarationTests.swift:2765", "let _: any ~Copyable = 0"),
    (
        "stdlib/public/Cxx/CxxSpan.swift:15",
        "func unsafeBitCast<T: ~Escapable & ~Copyable, U>(_ x: consuming T, to type: U.Type) -> U {}",
    ),
    (
        "stdlib/public/Synchronization/Cell.swift:18",
        "struct Cell<Value: ~Copyable>: ~Copyable {}",
    ),
];

#[test]
fn strict_parser_accepts_focused_swiftsyntax_cases() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in DECLARATION_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("DeclarationTests.swift {origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in CALLABLE_NAME_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in BINDING_PATTERN_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in MATCHING_PATTERN_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in IDENTIFIER_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in DECLARATION_SHELL_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in STATEMENT_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in IF_CONFIG_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in EXPRESSION_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in SIGNATURE_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in TRAILING_CLOSURE_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in SPECIAL_NAME_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in PROPERTY_AND_WHERE_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in POUND_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in MACRO_DECLARATION_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
    for &(origin, source) in MODERN_TYPE_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
}

#[test]
fn if_config_conditions_follow_the_boolean_grammar() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    for source in [
        "#if !(FLAG && os(Linux)) || compiler(>=10.0)\n#endif",
        "#if canImport(A, _version: 2) && $Macros\n#endif",
    ] {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}");
        });
    }

    // TSPL only admits conjunction, disjunction, and negation at the condition
    // level. Swift 6.3.3 diagnoses this as "expected '&&' or '||' expression".
    assert!(parser.parse("#if a + b\n#endif").is_err());
}

#[test]
fn code_block_if_configs_preserve_the_enclosing_item_flavor() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        (
            "stdlib/public/Platform/Platform.swift:316 getter",
            "var value: Int {\n#if os(Linux) || os(WASI)\nreturn 1\n#else\nreturn 0\n#endif\n}",
            "GetterCodeBlockItem",
            &["IfConfigIfClause", "IfConfigElseClause", "PoundEndif"][..],
        ),
        (
            "stdlib/public/RuntimeModule/ByteSwapping.swift:49 initializer",
            "init(littleEndian value: Self) {\n#if _endian(little)\nself = value\n#else\nself = value.byteSwapped\n#endif\n}",
            "CodeBlockItem",
            &["IfConfigIfClause", "IfConfigElseClause", "PoundEndif"][..],
        ),
        (
            "stdlib/public/Concurrency/Executor.swift:153 ordinary block",
            "func f() {\n#if os(WASI) || !$Embedded\nreturn\n#endif\n}",
            "CodeBlockItem",
            &["IfConfigIfClause", "PoundEndif"][..],
        ),
    ];

    for (origin, source, item_name, expected_children) in cases {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{origin}: {error}\n{source}"));
        let directive = first_descendant_named(&tree.top_node(), "IfConfigDeclaration").unwrap();
        assert!(
            directive.node_type().is_name("Declaration"),
            "{origin}: {tree}"
        );
        assert_eq!(
            direct_child_names(&directive),
            expected_children,
            "{origin}: {tree}"
        );
        assert_eq!(
            directive
                .parent()
                .map(|parent| parent.node_type().name().to_owned()),
            Some(item_name.to_owned()),
            "{origin}: {tree}"
        );
    }

    // SwiftSyntax forwards the current code-item context through nested
    // clauses instead of returning to the ordinary top-level item parser.
    let nested = parser
        .parse("var value: Int {\n#if OUTER\n#if INNER\nreturn 1\n#endif\n#endif\n}")
        .unwrap();
    assert_eq!(
        named_node_count(&nested.top_node(), "IfConfigDeclaration"),
        2,
        "{nested}"
    );
}

#[test]
fn postfix_if_configs_remain_owned_by_their_base_expression() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let source = IF_CONFIG_CASES
        .iter()
        .find_map(|(origin, source)| (*origin == "IfconfigExprTests.swift:206").then_some(*source))
        .unwrap();
    let tree = parser.parse(source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(source).unwrap();
        panic!("{error}\n{source}\n{recovered}");
    });
    let expression = first_descendant_named(&tree.top_node(), "PostfixIfConfigExpression").unwrap();
    assert!(expression.node_type().is_name("Expression"), "{tree}");
    assert_eq!(
        direct_child_names(&expression),
        ["DeclReferenceExpression", "IfConfigDeclaration"],
        "{tree}"
    );
    let directive = expression.child_by_name("IfConfigDeclaration").unwrap();
    assert!(directive.node_type().is_name("Declaration"), "{tree}");
    assert_eq!(
        direct_child_names(&directive),
        ["IfConfigIfClause", "IfConfigElseClause", "PoundEndif"],
        "{tree}"
    );

    // SwiftSyntax permits nested rootless postfix-ifconfig expressions and
    // keeps every nested directive inside the enclosing expression.
    let nested_source = IF_CONFIG_CASES
        .iter()
        .find_map(|(origin, source)| (*origin == "IfconfigExprTests.swift:187").then_some(*source))
        .unwrap();
    let nested = parser.parse(nested_source).unwrap();
    assert_eq!(
        named_node_count(&nested.top_node(), "PostfixIfConfigExpression"),
        4,
        "{nested}"
    );

    // A declaration body doesn't satisfy the expression-only first-body
    // boundary, so the same directive remains an ordinary code-block item.
    let ordinary = parser
        .parse("func f() {\nbase\n#if FLAG\nlet local = 1\n#endif\n}")
        .unwrap();
    assert_eq!(
        named_node_count(&ordinary.top_node(), "PostfixIfConfigExpression"),
        0,
        "{ordinary}"
    );
    assert_eq!(
        code_block_items(&ordinary.top_node()).len(),
        2,
        "{ordinary}"
    );

    // SwiftSyntax carries the enclosing condition flavor into the dotted
    // branches rather than ending the condition before the directive.
    let condition = parser
        .parse("func f(base: S) {\nif base\n#if FLAG\n.isReady\n#else\n.isFallback\n#endif\n{}\n}")
        .unwrap();
    assert_eq!(
        named_node_count(&condition.top_node(), "PostfixIfConfigExpression"),
        1,
        "{condition}"
    );

    // TSPL requires the first postfix-ifconfig branch to contain an
    // expression. In an argument position it cannot fall back to a separate
    // declaration item.
    assert!(
        parser
            .parse("func f() { call(base\n#if FLAG\n#else\n.member\n#endif\n) }")
            .is_err()
    );
}

#[test]
fn strict_parser_accepts_feature_enabled_do_and_then_cases() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in DO_EXPRESSION_CASES.iter().chain(THEN_STATEMENT_CASES) {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
}

#[test]
fn do_and_then_nodes_keep_their_official_cst_roles() {
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse("let value = do {\n  then 1\n} catch {\n  then 0\n}")
        .unwrap();
    let do_expression = first_descendant_named(&tree.top_node(), "DoExpression").unwrap();
    assert_eq!(
        direct_child_names(&do_expression),
        ["do", "CodeBlock", "CatchClause"],
        "{tree}"
    );
    assert!(do_expression.node_type().is_name("Expression"));
    assert_eq!(named_node_count(&do_expression, "ThenStatement"), 2);
    let then_statement = first_descendant_named(&do_expression, "ThenStatement").unwrap();
    assert_eq!(
        direct_child_names(&then_statement),
        ["then", "IntegerLiteralExpression"],
        "{tree}"
    );
    assert!(then_statement.node_type().is_name("Statement"));

    let typed_throws = rezel_lang_swift::parser()
        .with_strict(true)
        .parse("do throws(any Error) { throw error }")
        .unwrap();
    assert!(first_descendant_named(&typed_throws.top_node(), "DoStatement").is_some());
    assert!(first_descendant_named(&typed_throws.top_node(), "DoExpression").is_none());
}

#[test]
fn contextual_then_remains_an_identifier_in_expression_shapes() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for source in [
        "then.foo",
        "then()",
        "then + 2",
        "then=2",
        "let x = then",
        "then!",
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{source}: {error}\n{recovered}");
        });
        assert!(
            first_descendant_named(&tree.top_node(), "ThenStatement").is_none(),
            "{tree}"
        );
    }
}

#[test]
fn differentiability_attributes_keep_their_specialized_argument_roles() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in DIFFERENTIABILITY_ATTRIBUTE_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }

    let source = r"
@differentiable(reverse, wrt: (self, 1), where T: D)
func differentiable<T>() {}
@derivative(of: Foo.Self.other, wrt: self)
func derivative() {}
@transpose(of: Float.-, wrt: (0, 1))
func transpose() {}
";
    let tree = parser.parse(source).unwrap();
    assert_eq!(
        named_node_count(&tree.top_node(), "DifferentiableAttributeArguments"),
        1,
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "DerivativeAttributeArguments"),
        2,
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "DifferentiabilityWithRespectToArgument"),
        3,
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "DifferentiabilityArguments"),
        2,
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "DifferentiabilityArgument"),
        5,
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "GenericWhereClause"),
        1,
        "{tree}"
    );
}

#[test]
fn derivative_module_selected_name_preserves_type_and_decl_roles() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let module_selected = parser
        .parse(
            "@derivative(of: Swift::Foo.Swift::Bar.Swift::baz(), wrt: quux)\nfunc moduleSelected() {}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&module_selected.top_node(), "ModuleSelector"),
        3,
        "{module_selected}"
    );
    let derivative_arguments =
        first_descendant_named(&module_selected.top_node(), "DerivativeAttributeArguments")
            .unwrap();
    let original_name = derivative_arguments
        .child_by_name("MemberAccessExpression")
        .unwrap();
    assert_eq!(
        direct_child_names(&original_name),
        ["TypeExpression", ".", "DeclReferenceExpression"],
        "{module_selected}"
    );
    let base = original_name.child_by_name("TypeExpression").unwrap();
    assert_eq!(direct_child_names(&base), ["IdentifierType"], "{base}");
    assert_eq!(
        named_node_count(&base, "ModuleSelector"),
        2,
        "{module_selected}"
    );
    let final_name = original_name
        .child_by_name("DeclReferenceExpression")
        .unwrap();
    assert_eq!(
        direct_child_names(&final_name),
        ["ModuleSelector", "Identifier", "(", ")"],
        "{module_selected}"
    );
    assert_eq!(
        named_node_count(&module_selected.top_node(), "FunctionCallExpression"),
        0,
        "{module_selected}"
    );
}

#[test]
fn derivative_qualified_type_prefixes_follow_official_identifier_boundaries() {
    // Swift 6.3.3 accepts these as qualified declaration names. SwiftSyntax
    // parses every intermediate component as a type identifier while leaving
    // the final component for the declaration reference.
    let parser = rezel_lang_swift::parser().with_strict(true);
    let mut unexpected = Vec::new();
    for source in [
        "@derivative(of: Foo.Self<Int>.method())\nfunc derivative() {}",
        "@derivative(of: Foo.$name.method())\nfunc derivative() {}",
        "@derivative(of: Foo.`if`.method())\nfunc derivative() {}",
        "@derivative(of: Foo.Swift::$name.method())\nfunc derivative() {}",
        "@derivative(of: Foo.Swift::Any<Int>.method())\nfunc derivative() {}",
        "@derivative(of: Any.method())\nfunc derivative() {}",
    ] {
        let tree = match parser.parse(source) {
            Ok(tree) => tree,
            Err(error) => {
                let recovered = rezel_lang_swift::parser().parse(source).unwrap();
                unexpected.push(format!("{source}\n{error}\n{recovered}"));
                continue;
            }
        };
        let arguments =
            first_descendant_named(&tree.top_node(), "DerivativeAttributeArguments").unwrap();
        let original_name = arguments.child_by_name("MemberAccessExpression").unwrap();
        assert_eq!(
            direct_child_names(&original_name),
            ["TypeExpression", ".", "DeclReferenceExpression"],
            "{tree}"
        );
    }

    // These spellings are rejected by the same frontend. They distinguish
    // contextual type identifiers from lowercase `self`, reserved keywords,
    // dollar literals, the special root `Any`, and a line-broken module target.
    for source in [
        "@derivative(of: Foo.self.method())\nfunc derivative() {}",
        "@derivative(of: Foo.if.method())\nfunc derivative() {}",
        "@derivative(of: Foo.$0.method())\nfunc derivative() {}",
        "@derivative(of: Foo.Swift::self.method())\nfunc derivative() {}",
        "@derivative(of: Foo.Swift::$0.method())\nfunc derivative() {}",
        "@derivative(of: Foo.Any.method())\nfunc derivative() {}",
        "@derivative(of: Any.Foo.method())\nfunc derivative() {}",
        "@derivative(of: Any<Int>.method())\nfunc derivative() {}",
        "@derivative(of: Foo.Swift::\nBar.method())\nfunc derivative() {}",
    ] {
        if let Ok(tree) = parser.parse(source) {
            unexpected.push(format!("{source}\n{tree}"));
        }
    }
    assert!(
        unexpected.is_empty(),
        "unexpected boundaries: {unexpected:#?}"
    );
}

#[test]
fn builtin_differentiability_attribute_names_must_be_unqualified() {
    // SwiftSyntax always treats qualified or escaped spellings as custom
    // attributes, even when their final component is compiler-known.
    let custom = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(
            "@Module.differentiable(reverse, wrt: self)\nfunc qualified() {}\n@`derivative`(of: value)\nfunc escaped() {}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&custom.top_node(), "DifferentiableAttributeArguments"),
        0,
        "{custom}"
    );
    assert_eq!(
        named_node_count(&custom.top_node(), "DerivativeAttributeArguments"),
        0,
        "{custom}"
    );
    assert_eq!(
        named_node_count(&custom.top_node(), "LabeledExpression"),
        3,
        "{custom}"
    );
}

#[test]
fn differentiability_kind_specifiers_follow_the_official_token_set() {
    // SwiftSyntax 60e8eb850721, generated KindSpecifierOptions.
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(
            "@differentiable(_forward)\nfunc forward() {}\n@differentiable(_linear)\nfunc linear() {}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&tree.top_node(), "ForwardDifferentiabilityKeyword"),
        1,
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "LinearDifferentiabilityKeyword"),
        1,
        "{tree}"
    );
}

#[test]
fn derivative_accessors_follow_the_qualified_decl_name_boundary() {
    // SwiftSyntax Attributes.swift parses an accessor only after the complete
    // qualified declaration name. A bare dotted `get` remains the name.
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (source, expected) in [
        (
            "@derivative(of: foo().get)\nfunc derivative() {}",
            ["(", "of", ":", "DeclReferenceExpression", ".", "get", ")"],
        ),
        (
            "@derivative(of: Foo.foo(x:).set)\nfunc derivative() {}",
            ["(", "of", ":", "MemberAccessExpression", ".", "set", ")"],
        ),
        (
            "@transpose(of: Float.-._modify)\nfunc transpose() {}",
            [
                "(",
                "of",
                ":",
                "MemberAccessExpression",
                ".",
                "_modify",
                ")",
            ],
        ),
    ] {
        let tree = parser.parse(source).unwrap();
        let arguments =
            first_descendant_named(&tree.top_node(), "DerivativeAttributeArguments").unwrap();
        assert_eq!(direct_child_names(&arguments), expected, "{tree}");
    }

    let member = parser
        .parse("@derivative(of: Foo.get)\nfunc derivative() {}")
        .unwrap();
    let arguments =
        first_descendant_named(&member.top_node(), "DerivativeAttributeArguments").unwrap();
    assert_eq!(
        direct_child_names(&arguments),
        ["(", "of", ":", "MemberAccessExpression", ")"],
        "{member}"
    );
}

#[test]
fn abi_attributes_accept_the_official_provider_declaration_matrix() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in ABI_ATTRIBUTE_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }
}

#[test]
fn abi_attribute_arguments_own_the_existing_declaration_roles() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        (
            "@abi(func fn() -> Int)\nfunc fn1() -> Int {}",
            "FunctionDeclaration",
        ),
        (
            "protocol P { @abi(associatedtype AssocTy) associatedtype AssocTy }",
            "AssociatedTypeDeclaration",
        ),
        (
            "class C { @abi(deinit) deinit {} }",
            "DeinitializerDeclaration",
        ),
        (
            "enum E { @abi(case someCase) case someCase }",
            "EnumCaseDeclaration",
        ),
        (
            "struct S { @abi(init()) init() {} }",
            "InitializerDeclaration",
        ),
        (
            "struct S { @abi(subscript(i: Int) -> Int) subscript(i: Int) -> Int { 0 } }",
            "SubscriptDeclaration",
        ),
        (
            "@abi(typealias Alias = () -> Void)\ntypealias Alias = () -> Void",
            "TypeAliasDeclaration",
        ),
        ("@abi(var value = 1)\nvar value = 1", "VariableDeclaration"),
    ];

    for (source, provider) in cases {
        let tree = parser.parse(source).unwrap();
        let arguments = first_descendant_named(&tree.top_node(), "ABIAttributeArguments").unwrap();
        assert_eq!(
            direct_child_names(&arguments),
            ["(", provider, ")"],
            "{tree}"
        );
    }
}

#[test]
fn abi_attribute_routing_is_exact_and_provider_kinds_are_restricted() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let custom = parser
        .parse(
            "@Module.abi(func: value)\nfunc qualified() {}\n@`abi`(func: value)\nfunc escaped() {}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&custom.top_node(), "ABIAttributeArguments"),
        0,
        "{custom}"
    );
    assert_eq!(
        named_node_count(&custom.top_node(), "LabeledExpression"),
        2,
        "{custom}"
    );

    for source in [
        "@abi(import Fnord)\nfunc invalid() {}",
        "@abi(struct Nested {})\nstruct Invalid {}",
    ] {
        assert!(parser.parse(source).is_err(), "{source}");
    }
}

#[test]
fn using_declarations_follow_the_feature_enabled_official_cases() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in USING_DECLARATION_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }

    for (source, specifier) in [
        ("using @MainActor", "Attribute"),
        ("using nonisolated", "Identifier"),
        ("using @warn(DiagGroupID, as: warning)", "Attribute"),
        ("using test", "Identifier"),
    ] {
        let tree = parser.parse(source).unwrap();
        let declaration = first_descendant_named(&tree.top_node(), "UsingDeclaration").unwrap();
        assert_eq!(
            direct_child_names(&declaration),
            ["using", specifier],
            "{tree}"
        );
    }
}

#[test]
fn contextual_using_preserves_line_and_declaration_prefix_boundaries() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let ordinary = parser
        .parse("nonisolated\nusing\nlet using = 42\nfunc using(x: Int) {}")
        .unwrap();
    assert_eq!(
        named_node_count(&ordinary.top_node(), "UsingDeclaration"),
        0,
        "{ordinary}"
    );

    for source in [
        "using\n@MainActor",
        "@MainActor\nusing @Test",
        "public using test",
    ] {
        assert!(parser.parse(source).is_err(), "{source}");
        let recovered = rezel_lang_swift::parser().parse(source).unwrap();
        assert_eq!(
            named_node_count(&recovered.top_node(), "UsingDeclaration"),
            0,
            "{source}\n{recovered}"
        );
    }

    let invalid_specifier = parser.parse("using func").unwrap();
    assert_eq!(
        named_node_count(&invalid_specifier.top_node(), "UsingDeclaration"),
        0,
        "{invalid_specifier}"
    );
    assert_eq!(
        named_node_count(&invalid_specifier.top_node(), "InvalidUsingDeclaration"),
        1,
        "{invalid_specifier}"
    );
}

type TypeSuffixLayer = (&'static str, &'static str);

struct TypeSuffixCase {
    origin: &'static str,
    source: &'static str,
    layers: &'static [TypeSuffixLayer],
}

struct SomeAnySuffixCase {
    source: &'static str,
    outer_name: &'static str,
    suffix: &'static str,
    specifier: &'static str,
}

const DOUBLE_OPTIONAL_LAYERS: &[TypeSuffixLayer] = &[("OptionalType", "?"), ("OptionalType", "?")];
const MIXED_OPTIONAL_THEN_IUO_LAYERS: &[TypeSuffixLayer] = &[
    ("ImplicitlyUnwrappedOptionalType", "!"),
    ("OptionalType", "?"),
];
const MIXED_IUO_THEN_OPTIONAL_LAYERS: &[TypeSuffixLayer] = &[
    ("OptionalType", "?"),
    ("ImplicitlyUnwrappedOptionalType", "!"),
];
const TRIPLE_OPTIONAL_LAYERS: &[TypeSuffixLayer] = &[
    ("OptionalType", "?"),
    ("OptionalType", "?"),
    ("OptionalType", "?"),
];
const DOUBLE_IUO_LAYERS: &[TypeSuffixLayer] = &[
    ("ImplicitlyUnwrappedOptionalType", "!"),
    ("ImplicitlyUnwrappedOptionalType", "!"),
];

const RECURSIVE_OPTIONAL_CASES: &[TypeSuffixCase] = &[
    TypeSuffixCase {
        origin: "SwiftSyntax 60e8eb850721, translated/TryTests.swift:435",
        source: "func producesDoubleOptional() throws -> Int?? { return 3 }\nlet _: String = try? producesDoubleOptional()",
        layers: DOUBLE_OPTIONAL_LAYERS,
    },
    TypeSuffixCase {
        origin: "SwiftSyntax 60e8eb850721, translated/TryTests.swift:566",
        source: "let _: Int?? = (try? produceAny()) as? Int",
        layers: DOUBLE_OPTIONAL_LAYERS,
    },
    TypeSuffixCase {
        origin: "SwiftSyntax 60e8eb850721, translated/TryTests.swift:641",
        source: "let _: Int??? = try? producer.produceDoubleOptionalInt()",
        layers: TRIPLE_OPTIONAL_LAYERS,
    },
    TypeSuffixCase {
        origin: "Swift 064859e41d68596f486c5d724401cb370f260409, stdlib/public/core/LazyCollection.swift:111",
        source: "func _customIndexOfEquatableElement(_ element: Element) -> Index?? {}",
        layers: DOUBLE_OPTIONAL_LAYERS,
    },
    TypeSuffixCase {
        origin: "pinned Swift 6.3.3 frontend fixture, optional then IUO",
        source: "func optionalThenIuo() -> Int?! {}",
        layers: MIXED_OPTIONAL_THEN_IUO_LAYERS,
    },
    TypeSuffixCase {
        origin: "pinned Swift 6.3.3 frontend fixture, IUO then optional",
        source: "func iuoThenOptional() -> Int!? {}",
        layers: MIXED_IUO_THEN_OPTIONAL_LAYERS,
    },
    TypeSuffixCase {
        origin: "pinned Swift 6.3.3 frontend fixture, consecutive IUOs",
        source: "func doubleIuo() -> Int!! {}",
        layers: DOUBLE_IUO_LAYERS,
    },
];

const SOME_ANY_SUFFIX_CASES: &[SomeAnySuffixCase] = &[
    SomeAnySuffixCase {
        source: "let x: some P?",
        outer_name: "OptionalType",
        suffix: "?",
        specifier: "some",
    },
    SomeAnySuffixCase {
        source: "let x: any P?",
        outer_name: "OptionalType",
        suffix: "?",
        specifier: "any",
    },
    SomeAnySuffixCase {
        source: "let x: some P!",
        outer_name: "ImplicitlyUnwrappedOptionalType",
        suffix: "!",
        specifier: "some",
    },
];

#[test]
fn recursive_optional_types_preserve_each_suffix_in_the_cst() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for case in RECURSIVE_OPTIONAL_CASES {
        let tree = parser.parse(case.source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(case.source).unwrap();
            panic!("{}: {error}\n{recovered}", case.origin);
        });
        let node = tree.top_node();
        assert_type_suffix_layers(&node, case.layers);
        for type_name in ["OptionalType", "ImplicitlyUnwrappedOptionalType"] {
            assert_eq!(
                named_node_count(&node, type_name),
                type_suffix_layer_count(case.layers, type_name),
                "{}: {tree}",
                case.origin
            );
        }
    }
}

#[test]
fn question_mark_expression_contexts_do_not_become_optional_types() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (source, expected_node) in [
        ("let coalesced = a ?? b", "SequenceExpression"),
        ("base?.member", "OptionalChainingExpression"),
        ("try? work()", "TrySuffix"),
        (
            "switch value { case _?: break }",
            "OptionalChainingExpression",
        ),
    ] {
        let tree = parser.parse(source).unwrap();
        assert!(
            first_descendant_named(&tree.top_node(), expected_node).is_some(),
            "{source}: {tree}"
        );
        assert_eq!(named_node_count(&tree.top_node(), "OptionalType"), 0);
    }
    let coalescing = parser.parse("let coalesced = a ?? b").unwrap();
    let operator = first_descendant_named(&coalescing.top_node(), "BinaryOperator").unwrap();
    assert!(operator.child_by_name("??").is_some());
}

#[test]
fn optional_type_suffix_function_association_matches_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let returning_optional = parser
        .parse("func functionReturningOptional() -> () -> Int? {}")
        .unwrap();
    let return_clause =
        first_descendant_named(&returning_optional.top_node(), "ReturnClause").unwrap();
    assert_type_children(
        &return_clause.child_by_name("FunctionType").unwrap(),
        &["TupleType", "->", "OptionalType"],
    );

    let optional_function = parser
        .parse("func optionalFunction() -> (() -> Int)? {}")
        .unwrap();
    let return_clause =
        first_descendant_named(&optional_function.top_node(), "ReturnClause").unwrap();
    let optional = return_clause.child_by_name("OptionalType").unwrap();
    assert_type_children(&optional, &["TupleType", "?"]);
    let function = optional
        .child_by_name("TupleType")
        .unwrap()
        .child_by_name("TupleTypeElement")
        .unwrap()
        .child_by_name("FunctionType")
        .unwrap();
    assert!(function.node_type().is_name("Type"));
}

#[test]
fn function_type_arrows_remain_expression_sequence_elements() {
    // Pinned SwiftSyntax 60e8eb850721, translated/TypeExprTests.swift:367-545.
    // `parseSequenceExpressionOperator` materializes the arrow and its type
    // effects as an expression element; it does not reinterpret the ordinary
    // tuple operands as a TypeExpression.
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (source, arrows, effects) in [
        ("_ = (Int) -> Int", 1, 0),
        ("_ = [(Int, Int) throws -> Int]()", 1, 1),
        ("_ = 2 + () -> (Int, Int).2", 1, 0),
        ("_ = [(Int) -> (Int) -> Int]()", 2, 0),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}");
        });
        assert_eq!(
            named_node_count(&tree.top_node(), "ArrowExpression"),
            arrows,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "FunctionEffect"),
            effects,
            "{tree}"
        );
        let sequence =
            first_descendant_with_child(&tree.top_node(), "SequenceExpression", "ArrowExpression")
                .unwrap_or_else(|| panic!("{tree}"));
        let arrow = sequence.child_by_name("ArrowExpression").unwrap();
        assert!(arrow.node_type().is_name("Expression"), "{tree}");
        assert_eq!(arrow.child_by_name("->").unwrap().name().as_ref(), "->");
        assert_eq!(named_node_count(&tree.top_node(), "TypeExpression"), 0);
    }

    // Pinned Swift 6.3.3 rejects `rethrows` as the leading expression-arrow
    // effect even though declarations admit it as a function effect.
    assert!(parser.parse("_ = [() rethrows -> Void]()").is_err());
}

#[test]
fn declaration_effects_and_initializer_returns_match_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax 60e8eb850721, DeclarationTests.swift:1947-1957.
    let reasync = parser
        .parse(
            "class MyType {\n  init(_ f: () async -> Void) reasync { await f() }\n  func foo(index: Int) reasync rethrows -> String { await f() }\n}",
        )
        .unwrap();
    let initializer = first_descendant_named(&reasync.top_node(), "InitializerDeclaration")
        .expect("reasync initializer");
    assert_eq!(
        direct_child_names(&initializer),
        [
            "init",
            "FunctionParameterClause",
            "FunctionEffect",
            "CodeBlock"
        ],
        "{reasync}"
    );
    let function = first_descendant_named(&reasync.top_node(), "FunctionDeclaration")
        .expect("reasync rethrows function");
    assert_eq!(
        direct_child_names(&function),
        [
            "func",
            "FunctionName",
            "FunctionParameterClause",
            "FunctionEffect",
            "FunctionEffect",
            "ReturnClause",
            "CodeBlock"
        ],
        "{reasync}"
    );

    // SwiftSyntax DeclarationTests.swift:3389 and translated/
    // InitDeinitTests.swift:102,442.
    for source in ["public init() -> Int", "struct S { init() -> S {} }"] {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{error}\n{source}"));
        let initializer = first_descendant_named(&tree.top_node(), "InitializerDeclaration")
            .expect("initializer declaration");
        assert!(
            initializer.child_by_name("ReturnClause").is_some(),
            "{tree}"
        );
    }
    let deinitializer = parser.parse("class C { deinit async {} }").unwrap();
    let deinitializer =
        first_descendant_named(&deinitializer.top_node(), "DeinitializerDeclaration").unwrap();
    assert_eq!(
        direct_child_names(&deinitializer),
        ["deinit", "FunctionEffect", "CodeBlock"],
        "{deinitializer}"
    );
}

#[test]
fn declaration_effect_lookahead_controls_newline_boundaries() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax parses function effects without a start-of-line restriction.
    // Swift 6.3.3 and accepted sources LocalTestingDistributedActorSystem.swift:49-51
    // and ImageMap+Darwin.swift:64-66 put declaration effects on a new line.
    for source in [
        "func asyncEffect()\n  async -> Int { 0 }",
        "func resolve<Act>(_ name: String)\n  throws -> Act? where Act: AnyObject {}",
        "func withDyldProcessInfo<T>(_ body: () throws -> T)\n  rethrows -> T { try body() }",
        "func commented()\n  /* before effect */ rethrows -> Int { 0 }",
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}")
        });
        let function = first_descendant_named(&tree.top_node(), "FunctionDeclaration").unwrap();
        assert!(function.child_by_name("FunctionEffect").is_some(), "{tree}");
        assert!(function.child_by_name("ReturnClause").is_some(), "{tree}");
    }

    // The marker is state-sensitive and matches whole identifiers. It must not
    // join an effect-like prefix or a new declaration to the preceding item.
    let prefix = parser.parse("func prefix()\nrethrowsValue").unwrap();
    assert_eq!(
        prefix.top_node().children_by_name("CodeBlockItem").len(),
        2,
        "{prefix}"
    );
    let separate = parser
        .parse("func first() {}\nasync let value = operation()")
        .unwrap();
    assert_eq!(
        separate.top_node().children_by_name("CodeBlockItem").len(),
        2,
        "{separate}"
    );

    // SwiftSyntax DeclarationTests.swift:358-361. The canonical `async`
    // terminal is also a declaration modifier, so only the signature-specific
    // line marker may suppress this separator.
    let async_bindings = parser
        .parse("async let a = fetch(\"1.jpg\")\nasync let b: Image = fetch(\"2.jpg\")")
        .unwrap();
    assert_eq!(
        async_bindings
            .top_node()
            .children_by_name("CodeBlockItem")
            .len(),
        2,
        "{async_bindings}"
    );
    assert_eq!(
        named_node_count(&async_bindings.top_node(), "VariableDeclaration"),
        2,
        "{async_bindings}"
    );
}

#[test]
fn declaration_only_effects_stay_out_of_type_and_accessor_contexts() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for source in [
        "_ = [() reasync -> Void]()",
        "_ = [() rethrows -> Void]()",
        "let closure = { () reasync in }",
        "var value: Int { get rethrows {} }",
        "class C { deinit reasync {} }",
        "func outOfOrder() throws async {}",
    ] {
        assert!(
            parser.parse(source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn initializer_bodies_parse_bare_init_as_calls() {
    // SwiftSyntax 60e8eb850721, Declarations.swift parses initializer bodies
    // with `allowInitDecl: false`; ExpressionTests.swift:3624-3639 fixes the
    // same context across an immediate `#if` clause.
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        (
            "InitDeinitTests.swift:380",
            "class Aaron { convenience init() { init(x: 1) } }",
            1,
        ),
        (
            "InitDeinitTests.swift:390",
            "class Theodosia: Aaron { init() { init(x: 2) } }",
            1,
        ),
        (
            "InitDeinitTests.swift:402",
            "struct AaronStruct { init(x: Int) {}\ninit() { init(x: 1) } }",
            2,
        ),
        (
            "InitDeinitTests.swift:413",
            "enum AaronEnum: Int { case A = 1\ninit(x: Int) { init(rawValue: x)! } }",
            1,
        ),
        (
            "ExpressionTests.swift:3628",
            "class C { init() {\n#if true\ninit()\n#endif\n} }",
            1,
        ),
    ];

    for (origin, source, declaration_count) in cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}")
        });
        assert_eq!(
            named_node_count(&tree.top_node(), "InitializerDeclaration"),
            declaration_count,
            "{origin}: {tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "FunctionCallExpression"),
            1,
            "{origin}: {tree}"
        );
        let call = first_descendant_named(&tree.top_node(), "FunctionCallExpression").unwrap();
        let reference = call.child_by_name("DeclReferenceExpression").unwrap();
        assert_eq!(direct_child_names(&reference), ["init"], "{origin}: {tree}");
    }

    let declaration = parser.parse("init() {}").unwrap();
    assert_eq!(
        named_node_count(&declaration.top_node(), "InitializerDeclaration"),
        1,
        "{declaration}"
    );
    assert_eq!(
        named_node_count(&declaration.top_node(), "FunctionCallExpression"),
        0,
        "{declaration}"
    );
}

#[test]
fn attributed_types_are_committed_as_type_expression_elements() {
    // Pinned SwiftSyntax 60e8eb850721, Expressions.swift
    // `parseAttributedTypeExprIfPresent`, translated/TypeExprTests.swift:488-532,
    // and ExpressionTests.swift:911-914. A successful full-type lookahead
    // commits `@`, `inout`, or `nonisolated(nonsending)` to TypeExpression;
    // its function arrow is not materialized as an ArrowExpression.
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        ("_ = @convention(c) () -> Int", 1, 0, 0),
        ("_ = 1 + (@convention(c) () -> Int).self", 1, 0, 0),
        ("_ = ((inout Int) -> Void).self", 0, 1, 0),
        ("_ = nonisolated(nonsending) () async -> Void", 1, 0, 1),
        ("call(@Sendable (Int) throws -> Void)", 1, 0, 0),
    ];

    for (source, function_types, arrow_expressions, nonisolated_arguments) in cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}");
        });
        let top = tree.top_node();
        assert_eq!(named_node_count(&top, "TypeExpression"), 1, "{tree}");
        assert_eq!(named_node_count(&top, "AttributedType"), 1, "{tree}");
        assert_eq!(
            named_node_count(&top, "FunctionType"),
            function_types,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&top, "ArrowExpression"),
            arrow_expressions,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&top, "NonisolatedSpecifierArgument"),
            nonisolated_arguments,
            "{tree}"
        );

        let type_expression = first_descendant_named(&top, "TypeExpression").unwrap();
        assert!(type_expression.node_type().is_name("Expression"), "{tree}");
        let attributed = type_expression.child_by_name("AttributedType").unwrap();
        assert!(attributed.node_type().is_name("Type"), "{tree}");
    }

    // ExpressionTests.swift:4093. The same `@Sendable` spelling in a closure
    // signature remains an Attribute; the type-expression marker is not
    // shiftable in that parser state.
    let closure = parser.parse("f { @Sendable (e: Int) in }").unwrap();
    assert_eq!(
        named_node_count(&closure.top_node(), "TypeExpression"),
        0,
        "{closure}"
    );
    assert_eq!(
        named_node_count(&closure.top_node(), "Attribute"),
        1,
        "{closure}"
    );
}

#[test]
fn old_ownership_operator_spellings_share_modern_expression_nodes() {
    // Pinned SwiftSyntax 60e8eb850721, translated/MoveExprTests.swift:21-54
    // and BorrowExprTests.swift:21-43. These cases explicitly enable
    // `oldOwnershipOperatorSpellings`; the mise-pinned Swift 6.3.3 frontend
    // accepts the same spellings with the corresponding experimental flag.
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        (
            "MoveExprTests.swift:21",
            "var global: Int = 5\nfunc testGlobal() { let _ = _move global }",
            "ConsumeExpression",
            "_move",
        ),
        (
            "BorrowExprTests.swift:21",
            "func useString(_ str: String) {}\nvar global: String = \"123\"\nfunc testGlobal() { useString(_borrow global) }",
            "BorrowExpression",
            "_borrow",
        ),
    ];

    for (origin, source, expression_name, keyword_name) in cases {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{origin}: {error}\n{source}"));
        let expression = first_descendant_named(&tree.top_node(), expression_name).unwrap();
        assert!(expression.node_type().is_name("Expression"), "{tree}");
        assert_eq!(
            direct_child_names(&expression),
            [keyword_name, "DeclReferenceExpression"],
            "{origin}: {tree}"
        );
    }

    // SwiftSyntax keeps call syntax on the ordinary identifier path even when
    // the old-spelling feature is enabled. A line break likewise prevents the
    // contextual prefix from claiming the following code item.
    for source in [
        "_move(value)",
        "_borrow(value)",
        "_move (value)",
        "_borrow (value)",
        "_move\nvalue",
        "_borrow /* line\nbreak */ value",
    ] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), "ConsumeExpression"),
            0,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "BorrowExpression"),
            0,
            "{tree}"
        );
    }

    for (source, expression_name) in [
        ("_move /* trivia */ value", "ConsumeExpression"),
        ("_borrow /* trivia */ value", "BorrowExpression"),
    ] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), expression_name),
            1,
            "{tree}"
        );
    }
}

#[test]
fn contextual_any_type_expressions_follow_the_prefix_boundary() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        (
            "ExpressionTypeTests.swift:19",
            "_ = (any Sequence<Int>).self",
            "IdentifierType",
        ),
        ("TypeTests.swift:321", "[any ~Copyable]()", "SuppressedType"),
        (
            "TypeTests.swift:325",
            "[any P & ~Copyable]()",
            "CompositionType",
        ),
        (
            "TypeTests.swift:359",
            "(any ~Copyable).self",
            "SuppressedType",
        ),
        (
            "SwiftSyntax Expressions.swift adjacent-tilde branch",
            "any~Copyable",
            "SuppressedType",
        ),
        (
            "SwiftSyntax exact tilde before comment trivia",
            "any~/* trivia */Copyable",
            "SuppressedType",
        ),
    ];

    for (origin, source, constraint) in cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let type_expression = first_descendant_named(&tree.top_node(), "TypeExpression").unwrap();
        assert!(
            type_expression.node_type().is_name("Expression"),
            "{origin}: {tree}"
        );
        assert_eq!(
            direct_child_names(&type_expression),
            ["SomeOrAnyType"],
            "{origin}: {tree}"
        );
        let some_or_any = type_expression.child_by_name("SomeOrAnyType").unwrap();
        assert!(some_or_any.node_type().is_name("Type"), "{origin}: {tree}");
        assert_eq!(
            direct_child_names(&some_or_any),
            ["any", constraint],
            "{origin}: {tree}"
        );
    }

    for source in [
        "any(value)",
        "any.member",
        "any + value",
        "any~>Copyable",
        "any\n~Copyable",
    ] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), "TypeExpression"),
            0,
            "{tree}"
        );
    }

    // stdlib/public/RuntimeModule/Elf.swift:1759. SwiftSyntax parses the right
    // type as part of each cast operator, including when `as` starts a line.
    let cast = parser
        .parse("_ = value\n  as any ElfSymbolTableProtocol\n  as? SymbolTable")
        .unwrap();
    let sequence = first_descendant_named(&cast.top_node(), "SequenceExpression").unwrap();
    assert_eq!(
        direct_child_names(&sequence),
        [
            "DiscardAssignmentExpression",
            "BinaryOperator",
            "DeclReferenceExpression",
            "CastOperator",
            "TypeExpression",
            "CastOperator",
            "TypeExpression",
        ],
        "{cast}"
    );
    let cast_types = sequence.children_by_name("TypeExpression");
    assert_eq!(cast_types.len(), 2, "{cast}");
    assert_eq!(
        direct_child_names(&cast_types[0]),
        ["SomeOrAnyType"],
        "{cast}"
    );
}

#[test]
fn some_any_type_suffixes_wrap_the_qualified_type() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    // SwiftSyntax 60e8eb850721, SomeAnyTypeTests.swift:39-69.
    for case in SOME_ANY_SUFFIX_CASES {
        let tree = parser.parse(case.source).unwrap();
        let annotation = first_descendant_named(&tree.top_node(), "TypeAnnotation").unwrap();
        let outer = annotation.child_by_name(case.outer_name).unwrap();
        assert_type_children(&outer, &["SomeOrAnyType", case.suffix]);
        assert_type_children(
            &outer.child_by_name("SomeOrAnyType").unwrap(),
            &[case.specifier, "IdentifierType"],
        );
    }
}

#[test]
fn member_metatype_and_optional_type_suffixes_share_one_postfix_layer() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax 60e8eb850721, TypeMemberTests.swift testBaseType.
    let generic_member = parser.parse("let x: X<T>.Y<U>").unwrap();
    let member = first_descendant_named(&generic_member.top_node(), "MemberType").unwrap();
    assert_type_children(
        &member,
        &[
            "IdentifierType",
            ".",
            "TypeIdentifierName",
            "GenericArgumentClause",
        ],
    );
    assert_type_children(
        &member.child_by_name("IdentifierType").unwrap(),
        &["TypeIdentifierName", "GenericArgumentClause"],
    );

    // SwiftSyntax translated/TypeExprTests.swift:636 and TypeMemberTests.swift
    // testBaseType alternate member and optional suffixes in the same loop.
    let alternating = parser.parse("let x: X?.Y!.Z").unwrap();
    let outer_member = first_descendant_named(&alternating.top_node(), "MemberType").unwrap();
    assert_type_children(
        &outer_member,
        &["ImplicitlyUnwrappedOptionalType", ".", "TypeIdentifierName"],
    );
    let iuo = outer_member
        .child_by_name("ImplicitlyUnwrappedOptionalType")
        .unwrap();
    assert_type_children(&iuo, &["MemberType", "!"]);
    let inner_member = iuo.child_by_name("MemberType").unwrap();
    assert_type_children(&inner_member, &["OptionalType", ".", "TypeIdentifierName"]);
    assert_type_children(
        &inner_member.child_by_name("OptionalType").unwrap(),
        &["IdentifierType", "?"],
    );

    // SwiftSyntax 60e8eb850721, TypeMetatypeTests.swift testBaseType.
    let metatype = parser.parse("let x: X.Type.Type").unwrap();
    let outer_metatype = first_descendant_named(&metatype.top_node(), "MetatypeType").unwrap();
    assert_type_children(&outer_metatype, &["MetatypeType", ".", "TypeKeyword"]);
    assert_type_children(
        &outer_metatype.child_by_name("MetatypeType").unwrap(),
        &["IdentifierType", ".", "TypeKeyword"],
    );

    let protocol_metatype = parser.parse("let x: X.Protocol").unwrap();
    let protocol_metatype =
        first_descendant_named(&protocol_metatype.top_node(), "MetatypeType").unwrap();
    assert_type_children(
        &protocol_metatype,
        &["IdentifierType", ".", "ProtocolKeyword"],
    );

    for source in ["let x: X.TypeName", "let x: X.ProtocolBuffer"] {
        let member = parser.parse(source).unwrap();
        let member = first_descendant_named(&member.top_node(), "MemberType").unwrap();
        assert_type_children(&member, &["IdentifierType", ".", "TypeIdentifierName"]);
    }

    // Metatype specialization is state-scoped and does not reserve these
    // spellings where an ordinary identifier is expected.
    parser.parse("let Type = 1\nlet Protocol = Type").unwrap();

    let single_stack_parser =
        rezel_lang_swift::parser()
            .with_strict(true)
            .with_limits(ParseLimits {
                max_actions: 50_000,
                max_stacks: 1,
                max_stack_depth: 1_024,
                max_buffer_records: 20_000,
                max_recovery_actions: 0,
            });
    for source in [
        "let metatype: Root.Type.Type",
        "let protocolMetatype: Root.Protocol",
        "let memberType: Root.TypeName",
        "let protocolMember: Root.ProtocolBuffer",
        r"let keyPathMetatype = \Root.Type.make()",
    ] {
        let default_tree = parser.parse(source).unwrap();
        let single_stack_tree = single_stack_parser.parse(source).unwrap();
        assert_eq!(
            default_tree.to_string(),
            single_stack_tree.to_string(),
            "{source}"
        );
    }
}

#[test]
fn composition_type_elements_keep_their_suffix_ownership() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    // SwiftSyntax 60e8eb850721, Sources/SwiftParser/Types.swift:163-169
    // parses each ordinary composition element through `parseSimpleType`.
    let tree = parser.parse("let x: P & Q?").unwrap();
    let composition = first_descendant_named(&tree.top_node(), "CompositionType").unwrap();
    assert_type_children(&composition, &["IdentifierType", "&", "OptionalType"]);
    assert_type_children(
        &composition.child_by_name("OptionalType").unwrap(),
        &["IdentifierType", "?"],
    );

    // SwiftSyntax 60e8eb850721, SomeAnyTypeTests.swift:73-108.
    let tree = parser.parse("let x: some P & Q").unwrap();
    let some_or_any = first_descendant_named(&tree.top_node(), "SomeOrAnyType").unwrap();
    assert_type_children(&some_or_any, &["some", "CompositionType"]);
    assert_type_children(
        &some_or_any.child_by_name("CompositionType").unwrap(),
        &["IdentifierType", "&", "IdentifierType"],
    );

    let tree = parser.parse("let x: some P? & Q").unwrap();
    let composition = first_descendant_named(&tree.top_node(), "CompositionType").unwrap();
    assert_type_children(&composition, &["OptionalType", "&", "IdentifierType"]);
    assert_type_children(
        &composition.child_by_name("OptionalType").unwrap(),
        &["SomeOrAnyType", "?"],
    );
}

#[test]
fn suppressed_type_suffix_ownership_matches_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    // SwiftSyntax 60e8eb850721, DeclarationTests.swift:2807-2815 and
    // SomeAnyTypeTests.swift:217-244 use distinct suffix ownership for
    // ordinary and some/any-suppressed types.
    let tree = parser.parse("let x: ~Copyable?").unwrap();
    let suppression = first_descendant_named(&tree.top_node(), "SuppressedType").unwrap();
    assert_type_children(&suppression, &["~", "OptionalType"]);
    assert_type_children(
        &suppression.child_by_name("OptionalType").unwrap(),
        &["IdentifierType", "?"],
    );

    let tree = parser.parse("let x: some ~Copyable?").unwrap();
    let optional = first_descendant_named(&tree.top_node(), "OptionalType").unwrap();
    assert_type_children(&optional, &["SomeOrAnyType", "?"]);
    let suppression = optional
        .child_by_name("SomeOrAnyType")
        .unwrap()
        .child_by_name("SuppressedType")
        .unwrap();
    assert_type_children(&suppression, &["~", "IdentifierType"]);
}

#[test]
fn pack_element_optional_suffix_ownership_matches_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    // SwiftSyntax Types.swift:22-34 parses `repeat` outside its scalar type;
    // `each` then owns any optional suffix on its pack element.
    let tree = parser
        .parse("func packOptional<each T>(_ value: repeat each T?) {}")
        .unwrap();
    let expansion = first_descendant_named(&tree.top_node(), "PackExpansionType").unwrap();
    assert_type_children(&expansion, &["repeat", "PackElementType"]);
    let element = expansion.child_by_name("PackElementType").unwrap();
    assert_type_children(&element, &["each", "OptionalType"]);
    assert_type_children(
        &element.child_by_name("OptionalType").unwrap(),
        &["IdentifierType", "?"],
    );
}

#[test]
fn pack_expression_keywords_preserve_contextual_boundaries() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax VariadicGenericsTests.swift:155-188 only treats `each` as a
    // pack element when it prefixes a separate sequence element.
    let calls = parser
        .parse("func test() { each(x)\neach (x)\neach\nx }")
        .unwrap();
    assert_eq!(
        named_node_count(&calls.top_node(), "FunctionCallExpression"),
        2,
        "{calls}"
    );
    assert_eq!(
        named_node_count(&calls.top_node(), "PackElementExpression"),
        0,
        "{calls}"
    );
    assert_eq!(code_block_items(&calls.top_node()).len(), 4, "{calls}");

    let label = parser
        .parse("func f(repeat value: Int) {}\nf(repeat: value)\nf(each: value)")
        .unwrap();
    assert_eq!(
        named_node_count(&label.top_node(), "ArgumentLabel"),
        2,
        "{label}"
    );
    assert_eq!(
        named_node_count(&label.top_node(), "PackExpansionExpression"),
        0,
        "{label}"
    );

    let element = parser.parse("func test() { each x }").unwrap();
    let element_node =
        first_descendant_named(&element.top_node(), "PackElementExpression").unwrap();
    assert_eq!(
        direct_child_names(&element_node),
        ["each", "DeclReferenceExpression"],
        "{element}"
    );

    // Swift stdlib/public/core/Flatten.swift:74-88 keeps the following call as
    // a separate code item after a newline-terminated repeat condition.
    let statement = parser
        .parse("func test() { repeat {} while true\nfatalError() }")
        .unwrap();
    assert_eq!(
        named_node_count(&statement.top_node(), "RepeatStatement"),
        1,
        "{statement}"
    );
    assert_eq!(
        named_node_count(&statement.top_node(), "PackExpansionExpression"),
        0,
        "{statement}"
    );
    assert_eq!(
        code_block_items(&statement.top_node()).len(),
        2,
        "{statement}"
    );

    let continued = parser
        .parse("func test() { repeat {} while first\n&& second\nfatalError() }")
        .unwrap();
    assert_eq!(
        named_node_count(&continued.top_node(), "SequenceExpression"),
        1,
        "{continued}"
    );
    assert_eq!(
        code_block_items(&continued.top_node()).len(),
        2,
        "{continued}"
    );

    let flatten_source = r"
public mutating func next() -> Element? {
  repeat {
    if _fastPath(_inner != nil) {
      let ret = _inner!.next()
      if _fastPath(ret != nil) {
        return ret
      }
    }
    let s = _base.next()
    if _slowPath(s == nil) {
      return nil
    }
    _inner = s!.makeIterator()
  }
  while true
  fatalError()
}
";
    let flatten = parser.parse(flatten_source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(flatten_source).unwrap();
        panic!("{error}\n{flatten_source}\n{recovered}")
    });
    assert_eq!(
        named_node_count(&flatten.top_node(), "RepeatStatement"),
        1,
        "{flatten}"
    );
    assert_eq!(
        named_node_count(&flatten.top_node(), "PackExpansionExpression"),
        0,
        "{flatten}"
    );
}

#[test]
fn pack_expansion_expressions_own_the_full_sequence() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax VariadicGenericsTests.swift:18-33 and 191-287 parse the
    // expansion pattern as a complete expression, not one outer sequence item.
    let simple = parser
        .parse(
            "func tuplify<each T>(_ t: repeat each T) -> (repeat each T) { return (repeat each t) }",
        )
        .unwrap();
    let expansion = first_descendant_named(&simple.top_node(), "PackExpansionExpression").unwrap();
    assert_eq!(
        direct_child_names(&expansion),
        ["repeat", "PackElementExpression"],
        "{simple}"
    );

    let sequence = parser
        .parse("func expand<each T>(_ t: repeat each T) { repeat x + each t + 10 }")
        .unwrap();
    let expansion =
        first_descendant_named(&sequence.top_node(), "PackExpansionExpression").unwrap();
    assert_eq!(
        direct_child_names(&expansion),
        ["repeat", "SequenceExpression"],
        "{sequence}"
    );
    assert_eq!(
        named_node_count(&expansion, "PackElementExpression"),
        1,
        "{sequence}"
    );

    for source in [
        "func zip<each T, each U>(_ first: repeat each T, with second: repeat each U) -> (repeat (each T, each U)) { return (repeat (each first, each second)) }",
        "func variadicMap<each T, each Result>(_ t: repeat each T, transform: repeat (each T) -> each Result) -> (repeat each Result) { return (repeat (each transform)(each t)) }",
        "func expand<each T>(_ t: repeat each T) { repeat (each t).member() }",
        "func expand<each T>(_ t: repeat each T) { repeat each t.member }",
    ] {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}")
        });
    }
}

fn direct_child_names(node: &SyntaxNode) -> Vec<String> {
    node.children()
        .map(|child| child.name().to_string())
        .collect()
}

fn code_block_items(node: &SyntaxNode) -> Vec<SyntaxNode> {
    first_descendant_named(node, "CodeBlock")
        .expect("containing code block")
        .children_by_name("CodeBlockItem")
}

fn contains_error(node: &SyntaxNode) -> bool {
    node.node_type().is_error() || node.children().any(|child| contains_error(&child))
}

fn first_catch_item(catch_clause: &SyntaxNode) -> SyntaxNode {
    catch_clause
        .child_by_name("CatchItemList")
        .expect("catch item list")
        .child_by_name("CatchItem")
        .expect("catch item")
}

fn switch_cases(node: &SyntaxNode) -> Vec<SyntaxNode> {
    first_descendant_named(node, "SwitchExpression")
        .expect("switch expression")
        .children_by_name("SwitchCase")
}

fn member_decl_reference(member: &SyntaxNode) -> SyntaxNode {
    let mut references = member.children_by_name("DeclReferenceExpression");
    references
        .pop()
        .expect("trailing member declaration reference")
}

fn strict_key_path(source: &str) -> SyntaxNode {
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(source)
        .unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}");
        });
    first_descendant_named(&tree.top_node(), "KeyPathExpression").expect("key-path expression")
}

fn assert_sequence_children(node: &SyntaxNode, expected: &[&str], operator_count: usize) {
    assert_eq!(node.name().as_ref(), "SequenceExpression", "{node}");
    assert_eq!(direct_child_names(node), expected, "{node}");
    assert_eq!(
        node.children_by_name("BinaryOperator").len(),
        operator_count,
        "{node}"
    );
}

fn named_node_count(node: &SyntaxNode, name: &str) -> usize {
    usize::from(node.node_type().is_name(name))
        + node
            .children()
            .map(|child| named_node_count(&child, name))
            .sum::<usize>()
}

fn first_descendant_named(node: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    if node.name().as_ref() == name {
        return Some(node.clone());
    }
    node.children()
        .find_map(|child| first_descendant_named(&child, name))
}

fn first_descendant_with_child(
    node: &SyntaxNode,
    name: &str,
    child_name: &str,
) -> Option<SyntaxNode> {
    if node.name().as_ref() == name && node.child_by_name(child_name).is_some() {
        return Some(node.clone());
    }
    node.children()
        .find_map(|child| first_descendant_with_child(&child, name, child_name))
}

fn assert_type_suffix_layers(node: &SyntaxNode, expected: &[TypeSuffixLayer]) {
    let mut current =
        first_descendant_named(node, expected[0].0).expect("outer recursive optional type");
    for (index, &(expected_name, suffix)) in expected.iter().enumerate() {
        assert_eq!(current.name().as_ref(), expected_name);
        let expected_child = expected
            .get(index + 1)
            .map_or("IdentifierType", |(name, _)| *name);
        assert_type_children(&current, &[expected_child, suffix]);
        current = current
            .child_by_name(expected_child)
            .expect("nested type suffix child");
    }
    assert_eq!(current.name().as_ref(), "IdentifierType");
    assert!(current.node_type().is_name("Type"));
}

fn type_suffix_layer_count(layers: &[TypeSuffixLayer], name: &str) -> usize {
    layers
        .iter()
        .filter(|(layer_name, _)| *layer_name == name)
        .count()
}

fn assert_type_children(node: &SyntaxNode, expected: &[&str]) {
    assert!(node.node_type().is_name("Type"), "{node} is not in Type");
    assert_eq!(direct_child_names(node), expected, "{node}");
}

fn assert_module_selector(selector: &SyntaxNode) {
    assert_eq!(selector.name().as_ref(), "ModuleSelector", "{selector}");
    assert_eq!(
        direct_child_names(selector),
        ["Identifier", "::"],
        "{selector}"
    );
}

#[test]
fn conditional_attribute_if_configs_preserve_attribute_list_ownership() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax 60e8eb850721, Tests/SwiftParserTest/Parser+EntryTests.swift:109.
    // The conditional block is one attribute-list element, not an ordinary
    // top-level IfConfigDeclaration preceding the function.
    let entry = parser
        .parse("#if FLAG\n@attr\n#endif\nfunc test() {}")
        .unwrap();
    let entry_items = entry.top_node().children_by_name("CodeBlockItem");
    assert_eq!(entry_items.len(), 1);
    assert_eq!(
        direct_child_names(&entry_items[0]),
        ["IfConfigDeclaration", "FunctionDeclaration"]
    );

    // SwiftSyntax Attributes.swift parses every clause through the same
    // attribute-list element grammar. Attributes in later clauses therefore
    // keep the conditional attached to the following declaration.
    let alternatives = parser
        .parse(
            "#if FIRST\n#elseif SECOND\n@_spi(Second)\n#else\n@_spi(Third)\n#endif\nfunc test() {}",
        )
        .unwrap();
    let alternative_items = alternatives.top_node().children_by_name("CodeBlockItem");
    assert_eq!(alternative_items.len(), 1);
    let alternative_if_config = alternative_items[0]
        .child_by_name("IfConfigDeclaration")
        .unwrap();
    assert_eq!(
        direct_child_names(&alternative_if_config),
        [
            "IfConfigIfClause",
            "IfConfigElseifClause",
            "IfConfigElseClause",
            "PoundEndif"
        ]
    );
    assert_eq!(named_node_count(&alternative_if_config, "Attribute"), 2);

    // SwiftSyntax 60e8eb850721, Tests/SwiftParserTest/DirectiveTests.swift:160.
    // This also fixes the previously missing BooleanLiteral condition role.
    let nested = parser
        .parse("#if true\n#if true\n@frozen\n#endif\n#endif\npublic struct S {}")
        .unwrap();
    let nested_items = nested.top_node().children_by_name("CodeBlockItem");
    assert_eq!(nested_items.len(), 1);
    assert_eq!(
        direct_child_names(&nested_items[0]),
        [
            "IfConfigDeclaration",
            "DeclarationModifier",
            "StructDeclaration"
        ]
    );
    let outer_if_config = nested_items[0]
        .child_by_name("IfConfigDeclaration")
        .unwrap();
    assert_eq!(named_node_count(&outer_if_config, "IfConfigDeclaration"), 2);
    let outer_clause = outer_if_config.child_by_name("IfConfigIfClause").unwrap();
    let inner_if_config = outer_clause.child_by_name("IfConfigDeclaration").unwrap();
    let inner_clause = inner_if_config.child_by_name("IfConfigIfClause").unwrap();
    assert_eq!(
        direct_child_names(&inner_clause),
        ["PoundIf", "IfConfigCondition", "Attribute"]
    );
    assert_eq!(
        inner_clause
            .child_by_name("IfConfigCondition")
            .unwrap()
            .child_by_name("IfConfigConditionToken")
            .unwrap()
            .children_by_name("BooleanLiteral")
            .len(),
        1
    );

    // SwiftSyntax 60e8eb850721, Sources/SwiftParser/Attributes.swift:25.
    // The source comment's nested empty #if is extended with @after to verify
    // that the parent attribute list resumes after its closing directive.
    let nested_empty = parser
        .parse("#if COND1\n  @attr\n  #if COND2\n  #endif\n  @after\n#endif\nfunc fn() {}")
        .unwrap();
    let nested_empty_items = nested_empty.top_node().children_by_name("CodeBlockItem");
    assert_eq!(nested_empty_items.len(), 1);
    assert_eq!(
        direct_child_names(&nested_empty_items[0]),
        ["IfConfigDeclaration", "FunctionDeclaration"]
    );
    let empty_outer = nested_empty_items[0]
        .child_by_name("IfConfigDeclaration")
        .unwrap();
    assert_eq!(named_node_count(&empty_outer, "IfConfigDeclaration"), 2);
    let empty_outer_clause = empty_outer.child_by_name("IfConfigIfClause").unwrap();
    assert_eq!(
        direct_child_names(&empty_outer_clause),
        [
            "PoundIf",
            "IfConfigCondition",
            "Attribute",
            "IfConfigDeclaration",
            "Attribute"
        ]
    );
    let empty_inner = empty_outer_clause
        .child_by_name("IfConfigDeclaration")
        .unwrap();
    let empty_inner_clause = empty_inner.child_by_name("IfConfigIfClause").unwrap();
    assert!(empty_inner_clause.children_by_name("Attribute").is_empty());
    assert!(
        empty_inner_clause
            .children_by_name("IfConfigDeclaration")
            .is_empty()
    );
}

#[test]
fn ordinary_if_configs_do_not_become_attribute_lists() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Empty attribute conditionals remain ordinary declarations and therefore
    // cannot share the following declaration's CodeBlockItem.
    let empty_top_level = parser.parse("#if FLAG\n#endif\nfunc test() {}").unwrap();
    let empty_top_level_items = empty_top_level.top_node().children_by_name("CodeBlockItem");
    assert_eq!(empty_top_level_items.len(), 2);
    assert_eq!(
        direct_child_names(&empty_top_level_items[0]),
        ["IfConfigDeclaration"]
    );
    assert_eq!(
        direct_child_names(&empty_top_level_items[1]),
        ["FunctionDeclaration"]
    );

    // A body declaration likewise keeps this as an ordinary ifconfig, with
    // its guarded function owned by the clause's nested CodeBlockItem.
    let ordinary = parser
        .parse("#if FLAG\nfunc guarded() {}\n#endif\nfunc following() {}")
        .unwrap();
    let ordinary_items = ordinary.top_node().children_by_name("CodeBlockItem");
    assert_eq!(ordinary_items.len(), 2);
    assert_eq!(
        direct_child_names(&ordinary_items[0]),
        ["IfConfigDeclaration"]
    );
    assert_eq!(
        direct_child_names(&ordinary_items[1]),
        ["FunctionDeclaration"]
    );
    let ordinary_if_config = ordinary_items[0]
        .child_by_name("IfConfigDeclaration")
        .unwrap();
    let ordinary_clause = ordinary_if_config
        .child_by_name("IfConfigIfClause")
        .unwrap();
    let guarded_item = ordinary_clause.child_by_name("CodeBlockItem").unwrap();
    assert!(guarded_item.child_by_name("FunctionDeclaration").is_some());
    assert!(
        ordinary_items[0]
            .child_by_name("FunctionDeclaration")
            .is_none()
    );

    // Pinned Swift 6.3.3 frontend rejects each malformed directive boundary.
    for source in [
        "#if FLAG\n@attr #endif\nfunc test() {}",
        "#if FLAG\n@attr\n#if INNER\n#endif #endif\n#endif\nfunc test() {}",
        "#if FLAG\n@_spi(X)\n#endif func f() {}",
    ] {
        assert!(
            parser.parse(source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn strict_parser_accepts_operator_and_precedence_group_declarations() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source, expected_nodes) in OPERATOR_DECLARATION_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let tree = tree.to_string();
        for expected_node in expected_nodes {
            assert!(tree.contains(expected_node), "{origin}: {tree}");
        }
    }

    let operator_tree = parser
        .parse(OPERATOR_DECLARATION_CASES[0].1)
        .unwrap()
        .to_string();
    assert!(
        !operator_tree.contains("DeclarationModifier"),
        "{operator_tree}"
    );

    parser.parse(GLUED_OPERATOR_NAME_CASE).unwrap();

    for source in [
        "@objc postfix operator ++: PrecedenceGroup",
        "mutating postfix operator --: UndefinedPrecedenceGroup",
    ] {
        assert!(
            parser.parse(source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn discard_assignments_remain_distinct_from_pattern_wildcards() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Swift stdlib/public/core/InputStream.swift:43.
    let discard_assignment = parser.parse("_ = result.removeLast()").unwrap().to_string();
    assert!(discard_assignment.contains("SequenceExpression"));
    assert!(discard_assignment.contains("DiscardAssignmentExpression"));

    // SwiftSyntax ConsecutiveStatementsTests.swift:81.
    let consecutive = parser.parse("_ = r; _ = s; _ = t").unwrap().to_string();
    assert_eq!(
        consecutive.matches("DiscardAssignmentExpression").count(),
        3,
        "{consecutive}"
    );

    // The mise-pinned Swift frontend accepts both ordinary-expression forms.
    for source in ["_", "foo(_)"] {
        let ordinary = parser.parse(source).unwrap().to_string();
        assert!(
            ordinary.contains("DiscardAssignmentExpression"),
            "{source}: {ordinary}"
        );
        assert!(
            !ordinary.contains("WildcardPattern"),
            "{source}: {ordinary}"
        );
    }

    // The same frontend rejects a binding-only argument in an ordinary
    // condition call. Pattern arguments belong to the matching entry point.
    assert!(parser.parse("if foo(let x) {}").is_err());

    for source in [
        "let _ = value",
        "switch value { case _: () }",
        "if case _ = value {}",
    ] {
        let pattern = parser.parse(source).unwrap().to_string();
        assert!(pattern.contains("WildcardPattern"), "{source}: {pattern}");
        assert!(
            !pattern.contains("DiscardAssignmentExpression"),
            "{source}: {pattern}"
        );
    }
}

#[test]
fn matching_patterns_recurse_through_postfix_arguments() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in RECURSIVE_MATCHING_PATTERN_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }

    let tuple = parser
        .parse(RECURSIVE_MATCHING_PATTERN_CASES[1].1)
        .unwrap()
        .to_string();
    assert!(tuple.contains("TupleExpression"), "{tuple}");
    assert!(tuple.contains("ConditionLabeledExpression"), "{tuple}");
    assert!(tuple.contains("WildcardPattern"), "{tuple}");
    assert!(!tuple.contains("DiscardAssignmentExpression"), "{tuple}");

    let call = parser
        .parse(RECURSIVE_MATCHING_PATTERN_CASES[0].1)
        .unwrap()
        .to_string();
    assert!(call.contains("FunctionCallExpression"), "{call}");
    assert!(call.contains("ConditionArgumentClause"), "{call}");
    assert!(call.contains("ConditionLabeledExpression"), "{call}");
    assert!(call.contains("WildcardPattern"), "{call}");
    assert!(!call.contains("DiscardAssignmentExpression"), "{call}");

    let optional = parser
        .parse(RECURSIVE_MATCHING_PATTERN_CASES[5].1)
        .unwrap()
        .to_string();
    assert!(
        optional.contains("OptionalChainingExpression"),
        "{optional}"
    );
    assert!(optional.contains("WildcardPattern"), "{optional}");
    assert!(
        !optional.contains("DiscardAssignmentExpression"),
        "{optional}"
    );

    // Minimal witness for the same recursive context in subscript arguments.
    let subscript = parser
        .parse("switch value { case subject[_]: () }")
        .unwrap()
        .to_string();
    assert!(subscript.contains("SubscriptCallExpression"), "{subscript}");
    assert!(
        subscript.contains("ConditionLabeledExpression"),
        "{subscript}"
    );
    assert!(subscript.contains("WildcardPattern"), "{subscript}");
    assert!(
        !subscript.contains("DiscardAssignmentExpression"),
        "{subscript}"
    );
}

#[test]
fn matching_and_binding_patterns_use_sequence_expression_fallback() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Swift 064859e41d68596f486c5d724401cb370f260409,
    // stdlib/public/RuntimeModule/CompactBacktrace.swift:639.
    let compact = parser
        .parse("switch instruction { case .pc_first ... .pc_last: break }")
        .unwrap();
    let compact_sequence =
        first_descendant_named(&compact.top_node(), "SequenceExpression").unwrap();
    assert_sequence_children(
        &compact_sequence,
        &[
            "ImplicitMemberExpression",
            "BinaryOperator",
            "ImplicitMemberExpression",
        ],
        1,
    );

    // Swift 064859e41d68596f486c5d724401cb370f260409,
    // stdlib/public/core/CharacterProperties.swift:86.
    let scalar = parser
        .parse("switch value { case 0x000A...0x000D: break }")
        .unwrap();
    let scalar_sequence = first_descendant_named(&scalar.top_node(), "SequenceExpression").unwrap();
    assert_sequence_children(
        &scalar_sequence,
        &[
            "IntegerLiteralExpression",
            "BinaryOperator",
            "IntegerLiteralExpression",
        ],
        1,
    );

    // Swift 064859e41d68596f486c5d724401cb370f260409,
    // stdlib/public/core/NFC.swift:436.
    let nfc = parser
        .parse("switch (x, y) { case (L.base ..< L.base &+ L.count, V.base ..< V.base &+ V.count): break }")
        .unwrap();
    let pattern = first_descendant_named(&nfc.top_node(), "SwitchCasePattern").unwrap();
    let tuple = pattern.child_by_name("TupleExpression").unwrap();
    let arguments = tuple.children_by_name("ConditionLabeledExpression");
    assert_eq!(arguments.len(), 2, "{nfc}");
    for argument in arguments {
        let sequence = argument.child_by_name("SequenceExpression").unwrap();
        assert_sequence_children(
            &sequence,
            &[
                "MemberAccessExpression",
                "BinaryOperator",
                "MemberAccessExpression",
                "BinaryOperator",
                "MemberAccessExpression",
            ],
            2,
        );
    }

    let binding = parser
        .parse("if case let lower ... upper = value {}")
        .unwrap();
    let binding_sequence =
        first_descendant_named(&binding.top_node(), "SequenceExpression").unwrap();
    assert_sequence_children(
        &binding_sequence,
        &["IdentifierPattern", "BinaryOperator", "IdentifierPattern"],
        1,
    );
}

#[test]
fn matching_pattern_assignments_remain_delimiters() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (source, pattern) in [
        ("if case let x = value {}", "ValueBindingPattern"),
        ("if case _ = value {}", "WildcardPattern"),
    ] {
        let tree = parser.parse(source).unwrap();
        let condition =
            first_descendant_named(&tree.top_node(), "MatchingPatternCondition").unwrap();
        assert_eq!(
            direct_child_names(&condition),
            ["case", "SwitchCasePattern", "=", "DeclReferenceExpression"],
            "{tree}"
        );
        assert!(
            first_descendant_named(&condition, pattern).is_some(),
            "{tree}"
        );
        assert_eq!(named_node_count(&condition, "BinaryOperator"), 0, "{tree}");
    }

    // Pinned SwiftSyntax 60e8eb850721, MatchingPatternsTests.swift:226 and
    // PatternWithoutVariablesTests.swift:76. `inout` is a value-binding
    // introducer here, not an identifier pattern or declaration binding.
    for (source, expected) in [
        (
            "if case inout .Naught(value) = n {}",
            ["inout", "FunctionCallExpression"],
        ),
        (
            "switch x { case inout .Bar: break }",
            ["inout", "ImplicitMemberExpression"],
        ),
    ] {
        let tree = parser.parse(source).unwrap();
        let binding = first_descendant_named(&tree.top_node(), "ValueBindingPattern").unwrap();
        assert_eq!(direct_child_names(&binding), expected, "{tree}");
    }
}

#[test]
fn binding_specifiers_follow_swiftsyntax_pattern_contexts() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax 60e8eb850721, translated/PatternWithoutVariablesTests.swift:20-60.
    // Variable declarations own the binding-specifier token directly.
    for specifier in ["inout", "_mutating", "_borrowing", "_consuming"] {
        let source = format!("{specifier} _ = 1");
        let tree = parser.parse(&source).unwrap_or_else(|error| {
            panic!("PatternWithoutVariablesTests: {error}\n{source}");
        });
        let declaration = first_descendant_named(&tree.top_node(), "VariableDeclaration").unwrap();
        assert_eq!(
            direct_child_names(&declaration),
            [specifier, "PatternBinding"],
            "{tree}"
        );
        assert_eq!(named_node_count(&tree.top_node(), "BindingSpecifier"), 0);
    }

    // SwiftSyntax 60e8eb850721, translated/MatchingPatternsTests.swift:55-605.
    for specifier in ["inout", "_mutating", "_borrowing", "_consuming"] {
        let source = format!("switch value {{ case {specifier} item: break }}");
        let tree = parser.parse(&source).unwrap_or_else(|error| {
            panic!("MatchingPatternsTests: {error}\n{source}");
        });
        let binding = first_descendant_named(&tree.top_node(), "ValueBindingPattern").unwrap();
        assert_eq!(
            direct_child_names(&binding),
            [specifier, "IdentifierPattern"],
            "{tree}"
        );
    }

    // Patterns.swift:279-291 makes `borrowing` and `_borrowing` bindings only
    // before an identifier or wildcard on the same line; the former has the
    // full boundary matrix in MatchingPatternsTests.swift:611-723.
    for source in [
        "switch 42 { case borrowing .foo(): break }",
        "switch 42 { case _borrowing .foo(): break }",
        "switch 42 { case borrowing (): break }",
        "switch 42 { case borrowing + borrowing: break }",
    ] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), "ValueBindingPattern"),
            0,
            "{tree}"
        );
        assert!(
            first_descendant_named(&tree.top_node(), "DeclReferenceExpression").is_some(),
            "{tree}"
        );
    }

    for (source, expected_pattern) in [
        (
            "switch 42 { case borrowing item: break }",
            "IdentifierPattern",
        ),
        (
            "switch value { case .payload(borrowing item): break }",
            "IdentifierPattern",
        ),
        (
            "switch value { case borrowing item.member: break }",
            "MemberAccessExpression",
        ),
    ] {
        let tree = parser.parse(source).unwrap();
        let binding = first_descendant_named(&tree.top_node(), "ValueBindingPattern").unwrap();
        assert_eq!(
            direct_child_names(&binding),
            ["borrowing", expected_pattern],
            "{tree}"
        );
    }

    // PatternWithoutVariablesTests.swift:89 keeps `_mutating` as an ordinary
    // assignment without its binding context; `_borrowing` shares that
    // expression fallback when no pattern target follows.
    for source in ["_mutating = 2", "_borrowing = 2"] {
        let ordinary = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&ordinary.top_node(), "VariableDeclaration"),
            0,
            "{ordinary}"
        );
    }
}

#[test]
fn optional_bindings_keep_self_in_the_identifier_pattern_context() {
    // Pinned SwiftSyntax 60e8eb850721, StatementTests.swift:43 and
    // translated/SelfRebindingTests.swift:19. SwiftParser also uses the
    // shorthand form in Sources/SwiftParser/SyntaxUtils.swift:63.
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        ("StatementTests.swift:43", "if let self = self {}"),
        ("SyntaxUtils.swift:63", "if let self {}"),
        (
            "SelfRebindingTests.swift:19",
            "guard let self = self else {}",
        ),
        ("Swift 6.3.3 var binding", "while var self = self {}"),
    ];

    for (origin, source) in cases {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{origin}: {error}\n{source}"));
        let condition =
            first_descendant_named(&tree.top_node(), "OptionalBindingCondition").unwrap();
        let pattern = condition.child_by_name("IdentifierPattern").unwrap();
        assert_eq!(direct_child_names(&pattern), ["self"], "{origin}: {tree}");
        assert_eq!(
            named_node_count(&condition, "DeclReferenceExpression"),
            usize::from(source.contains("= self")),
            "{origin}: {tree}"
        );
    }

    // The binding-introducer context recurses through tuple patterns instead
    // of adding a top-level-only exception for `self`.
    let tuple = parser.parse("if let (self, x) = value {}").unwrap();
    let condition = first_descendant_named(&tuple.top_node(), "OptionalBindingCondition").unwrap();
    assert_eq!(named_node_count(&condition, "TuplePattern"), 1, "{tuple}");
    assert_eq!(
        named_node_count(&condition, "IdentifierPattern"),
        2,
        "{tuple}"
    );

    // `self` is only a binding-introducer identifier pattern. The pinned
    // frontend rejects it in an ordinary variable declaration, and
    // `_borrowing` is not an optional-binding introducer.
    assert!(parser.parse("let self = self").is_err());
    assert!(parser.parse("if _borrowing value {}").is_err());
}

#[test]
fn parameter_modifiers_follow_swiftsyntax_name_boundaries() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let modified_cases: &[(&str, &str, &str, &[&str])] = &[
        (
            "DeclarationTests.swift:1897",
            "func const(_const _ map: String) {}",
            "FunctionParameter",
            &["_const"],
        ),
        (
            "DeclarationTests.swift:1909",
            "func isolatedConst(isolated _const _ map: String) {}",
            "FunctionParameter",
            &["isolated", "_const"],
        ),
        (
            "DeclarationTests.swift:3436",
            "func const(_const x y: String) {}",
            "FunctionParameter",
            &["_const"],
        ),
        (
            "SwiftSyntax parameter-modifier closure smoke",
            "_ = { (_const _ x: Int) in x }",
            "ClosureParameter",
            &["_const"],
        ),
        (
            "SwiftSyntax enum-case parameter path",
            "enum E { case value(_const Int) }",
            "EnumCaseParameter",
            &["_const"],
        ),
    ];

    for &(origin, source, parameter_kind, expected) in modified_cases {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{origin}: {error}\n{source}"));
        let parameter = first_descendant_named(&tree.top_node(), parameter_kind).unwrap();
        let spellings = parameter
            .children_by_name("DeclarationModifier")
            .iter()
            .flat_map(direct_child_names)
            .collect::<Vec<_>>();
        assert_eq!(spellings, expected, "{origin}: {tree}");
    }

    let name_cases = [
        "func const(_const map: String) {}",
        "func const(_const: String) {}",
        "func isolatedConst(isolated _const: String) {}",
        "_ = { (_const x: Int) in x }",
        "_ = { (isolated) in isolated }",
        "enum E { case value(_const: Int) }",
    ];
    for source in name_cases {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), "DeclarationModifier"),
            0,
            "{tree}"
        );
    }

    for source in [
        "func foo1(_ a: _const borrowing String) {}",
        "func foo2(_ a: borrowing _const String) {}",
    ] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), "AttributedType"),
            1,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "SimpleTypeSpecifier"),
            2,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "DeclarationModifier"),
            0,
            "{tree}"
        );
    }
}

#[test]
fn parameter_names_allow_wildcards_in_both_positions() {
    // Pinned Swift 064859e41d68, stdlib/public/core/ASCII.swift:55.
    // SwiftSyntax parses both names with the same argument-label routine.
    let parser = rezel_lang_swift::parser().with_strict(true);
    let tree = parser
        .parse("func decode(_ content: FromEncoding.EncodedScalar, from _: FromEncoding.Type) {}")
        .unwrap();
    let clause = first_descendant_named(&tree.top_node(), "FunctionParameterClause").unwrap();
    let names = clause
        .children_by_name("FunctionParameter")
        .iter()
        .map(|parameter| direct_child_names(&parameter.child_by_name("ParameterNames").unwrap()))
        .collect::<Vec<_>>();
    assert_eq!(names, [["_", "Identifier"], ["Identifier", "_"]], "{tree}");
}

#[test]
fn lifetime_type_specifiers_share_one_attributed_type_owner() {
    // Pinned SwiftSyntax 60e8eb850721, TypeTests.swift:441-518 and
    // DeclarationTests.swift:3383. These cases explicitly enable the
    // experimental `nonescapableTypes` parser feature. The mise-pinned Swift
    // 6.3.3 frontend does not accept them, even with the corresponding
    // compiler feature flag, so they are intentionally not oracle fixtures.
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        ("TypeTests.swift:441", "func foo() -> dependsOn(x) X", 1, 1),
        (
            "TypeTests.swift:443",
            "func foo() -> dependsOn(x, y) X",
            1,
            1,
        ),
        (
            "TypeTests.swift:471",
            "func foo() -> dependsOn(x) dependsOn(scoped y) X",
            2,
            1,
        ),
        (
            "TypeTests.swift:473",
            "func foo() -> dependsOn(scoped x) X",
            1,
            1,
        ),
        ("TypeTests.swift:516", "func foo() -> dependsOn(0) X", 1, 1),
        (
            "TypeTests.swift:518",
            "func foo() -> dependsOn(self) X",
            1,
            1,
        ),
        (
            "DeclarationTests.swift:3383",
            "init(_ ptr: UnsafeRawBufferPointer, _ a: borrowing Array<Int>) -> dependsOn(a) Self",
            1,
            2,
        ),
    ];

    for (origin, source, lifetime_specifiers, attributed_types) in cases {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{origin}: {error}\n{source}"));
        assert_eq!(
            named_node_count(&tree.top_node(), "LifetimeTypeSpecifier"),
            lifetime_specifiers,
            "{origin}: {tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "AttributedType"),
            attributed_types,
            "{origin}: {tree}"
        );
    }

    let many = parser.parse("func foo() -> dependsOn(x, y) X").unwrap();
    let lifetime = first_descendant_named(&many.top_node(), "LifetimeTypeSpecifier").unwrap();
    let arguments = lifetime.children_by_name("LifetimeSpecifierArgument");
    assert_eq!(arguments.len(), 2, "{many}");
    assert_eq!(
        direct_child_names(&arguments[0]),
        ["Identifier", ","],
        "{many}"
    );
    assert_eq!(direct_child_names(&arguments[1]), ["Identifier"], "{many}");

    let scoped = parser.parse("func foo() -> dependsOn(scoped x) X").unwrap();
    let lifetime = first_descendant_named(&scoped.top_node(), "LifetimeTypeSpecifier").unwrap();
    assert_eq!(
        direct_child_names(&lifetime),
        ["dependsOn", "(", "scoped", "LifetimeSpecifierArgument", ")"],
        "{scoped}"
    );

    for source in [
        "func ordinary(_ value: dependsOn) {}",
        "typealias dependsOn = Int",
    ] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), "LifetimeTypeSpecifier"),
            0,
            "{tree}"
        );
    }

    assert!(parser.parse("func foo() -> dependsOn(x,) X").is_err());

    // TypeTests.swift:535 diagnoses the newline-separated spelling. Our CST
    // parser may retain the later lines as separate top-level code items, but
    // must not commit `dependsOn` to the feature-gated specifier.
    let newline = parser.parse("func foo() -> dependsOn\n(0)\nX").unwrap();
    assert_eq!(
        named_node_count(&newline.top_node(), "LifetimeTypeSpecifier"),
        0,
        "{newline}"
    );
}

#[test]
fn enum_case_parameter_names_are_direct_identifier_or_wildcard_children() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases: &[(&str, &str, usize, &[&str])] = &[
        (
            "translated/EnumTests.swift:1460",
            "enum E { case a(_ x: Int) }",
            0,
            &["_", "Identifier", ":", "IdentifierType"],
        ),
        (
            "stdlib/public/core/UTF8SpanInternalHelpers.swift:169",
            "enum E { case error(_: Range<Int>) }",
            0,
            &["_", ":", "IdentifierType"],
        ),
        (
            "SwiftLexicalLookup/LookupResult.swift:20",
            "enum E { case lookForMembers(in: Syntax) }",
            0,
            &["Identifier", ":", "IdentifierType"],
        ),
        (
            "SwiftIfConfig/IfConfigDiagnostic.swift:25",
            "enum E { case unsupported(name: String, operator: TokenSyntax) }",
            1,
            &["Identifier", ":", "IdentifierType"],
        ),
    ];

    for (name, source, parameter_index, expected_children) in cases {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{name}: {error}\n{source}"));
        let clause = first_descendant_named(&tree.top_node(), "EnumCaseParameterClause").unwrap();
        let parameter = clause
            .children_by_name("EnumCaseParameter")
            .get(*parameter_index)
            .cloned()
            .unwrap();
        assert_eq!(
            direct_child_names(&parameter),
            *expected_children,
            "{name}: {tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "EnumCaseParameterLabel"),
            0,
            "{name}: {tree}"
        );
    }

    let two_parameters = parser
        .parse("enum E { case skipUntil(_ t1: TokenSpec, _ t2: TokenSpec) }")
        .unwrap();
    let clause =
        first_descendant_named(&two_parameters.top_node(), "EnumCaseParameterClause").unwrap();
    assert_eq!(clause.children_by_name("EnumCaseParameter").len(), 2);

    // CST construction preserves the label. Strict syntax validation owns the
    // frontend rule that excludes `inout` from `isArgumentLabel`.
    let inout_label = parser.parse("enum E { case f(inout: Int) }").unwrap();
    let parameter = first_descendant_named(&inout_label.top_node(), "EnumCaseParameter").unwrap();
    assert_eq!(
        direct_child_names(&parameter),
        ["Identifier", ":", "IdentifierType"],
        "{inout_label}"
    );
}

#[test]
fn enum_case_parameter_clauses_continue_across_newlines() {
    // SwiftSyntax 60e8eb850721, translated/EnumTests.swift:1470. The
    // SwiftSyntax parser accepts this parser-only boundary even though the
    // mise-pinned Swift 6.3.3 frontend diagnoses the clause as a property.
    let source = "enum E {\n  case a\n    (Int)\n}";
    let parser = rezel_lang_swift::parser().with_strict(true);
    let tree = parser.parse(source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(source).unwrap();
        panic!("{error}\n{source}\n{recovered}")
    });
    let element = first_descendant_named(&tree.top_node(), "EnumCaseElement").unwrap();
    assert_eq!(
        direct_child_names(&element),
        ["Identifier", "EnumCaseParameterClause"],
        "{tree}"
    );

    let separate = parser.parse("foo\n(Int)").unwrap();
    assert_eq!(
        separate.top_node().children_by_name("CodeBlockItem").len(),
        2,
        "{separate}"
    );
}

#[test]
fn value_generic_parameters_and_arguments_follow_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Swift stdlib/public/core/InlineArray.swift:101 and _InlineArray.swift:16.
    let declaration = parser
        .parse("struct InlineArray<let count: Int, Element: ~Copyable>: ~Copyable {}")
        .unwrap();
    let parameters = first_descendant_named(&declaration.top_node(), "GenericParameterClause")
        .unwrap()
        .children_by_name("GenericParameter");
    assert_eq!(parameters.len(), 2, "{declaration}");
    assert_eq!(
        direct_child_names(&parameters[0]),
        ["let", "TypeName", ":", "IdentifierType"],
        "{declaration}"
    );
    assert_eq!(
        direct_child_names(&parameters[1]),
        ["TypeName", ":", "SuppressedType"],
        "{declaration}"
    );

    // SwiftSyntax ValueGenericsTests.swift:127-169.
    parser.parse("let x = Generic<123>.self").unwrap();
    let argument_source = "let x = Generic<-123, Int>.self";
    let arguments = parser.parse(argument_source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(argument_source).unwrap();
        panic!("{error}\n{argument_source}\n{recovered}");
    });
    let argument_clause =
        first_descendant_named(&arguments.top_node(), "GenericArgumentClause").unwrap();
    assert_eq!(
        direct_child_names(&argument_clause),
        ["<", "PrefixOperatorExpression", ",", "IdentifierType", ">"],
        "{arguments}"
    );
    let negative = argument_clause
        .child_by_name("PrefixOperatorExpression")
        .unwrap();
    assert!(negative.node_type().is_name("Expression"), "{arguments}");
    assert_eq!(
        direct_child_names(&negative),
        ["-", "IntegerLiteralExpression"],
        "{arguments}"
    );
    assert!(
        negative
            .child_by_name("IntegerLiteralExpression")
            .unwrap()
            .node_type()
            .is_name("Expression"),
        "{arguments}"
    );

    // ValueGenericsTests.swift:169 uses lowercase `self` as a member type
    // name. SwiftSyntax's MemberTypeSyntax admits it there, while the root
    // IdentifierTypeSyntax name does not.
    let member_type_source = "let x: Generic<Int, -123>.self";
    let member_type = parser.parse(member_type_source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser()
            .parse(member_type_source)
            .unwrap();
        panic!("{error}\n{member_type_source}\n{recovered}");
    });
    let member = first_descendant_named(&member_type.top_node(), "MemberType").unwrap();
    assert_eq!(
        direct_child_names(&member),
        ["IdentifierType", ".", "TypeIdentifierName"],
        "{member_type}"
    );
    assert_eq!(
        direct_child_names(&member.child_by_name("IdentifierType").unwrap()),
        ["TypeIdentifierName", "GenericArgumentClause"],
        "{member_type}"
    );
    let member_name = member.child_by_name("TypeIdentifierName").unwrap();
    assert_eq!(direct_child_names(&member_name), ["self"], "{member_type}");

    for source in ["let x: 123", "let x: self"] {
        assert!(
            parser.parse(source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn inline_array_types_share_value_generic_argument_ownership() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        (
            "TypeTests.swift:779",
            "[3 of Int]",
            "IntegerLiteralExpression",
            "IdentifierType",
        ),
        (
            "TypeTests.swift:787",
            "[Int of _]",
            "IdentifierType",
            "IdentifierType",
        ),
        (
            "ExpressionTypeTests.swift:119",
            "let value: [@escaping () -> Int of Int]",
            "AttributedType",
            "IdentifierType",
        ),
        (
            "ExpressionTypeTests.swift:121",
            "let value: [sending P & Q of Int]",
            "AttributedType",
            "IdentifierType",
        ),
        (
            "ExpressionTypeTests.swift:126",
            "let value: [[3 of Int] of Int]",
            "InlineArrayType",
            "IdentifierType",
        ),
        (
            "TypeTests.swift:832",
            "let value: [3 of\nInt]",
            "IntegerLiteralExpression",
            "IdentifierType",
        ),
        (
            "TypeTests.swift:838",
            "let value: [\n3 of Int]",
            "IntegerLiteralExpression",
            "IdentifierType",
        ),
    ];

    for (origin, source, count, element) in cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let inline_array = first_descendant_named(&tree.top_node(), "InlineArrayType").unwrap();
        assert!(inline_array.node_type().is_name("Type"), "{origin}: {tree}");
        assert_eq!(
            direct_child_names(&inline_array),
            ["[", count, "of", element, "]"],
            "{origin}: {tree}"
        );
    }

    // SwiftSyntax TypeTests.swift:907/938 and
    // ExpressionTypeTests.swift:150 enable `LiteralExpressions`. The package
    // deliberately includes these feature-selected CST forms in its current
    // single syntax surface.
    for (origin, source) in [
        ("TypeTests.swift:907", "[(2 + 3) of Int]"),
        ("TypeTests.swift:938", "[(1 + 1) of [(2 + 1) of Int]]"),
    ] {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{origin}: {error}"));
        assert_eq!(
            named_node_count(&tree.top_node(), "InlineArrayType"),
            if origin.ends_with(":938") { 2 } else { 1 },
            "{origin}: {tree}"
        );
        assert!(
            first_descendant_named(&tree.top_node(), "TupleExpression").is_some(),
            "{origin}: {tree}"
        );
    }

    let generic = parser
        .parse("let value: InlineArray<(1 + 2), Int>")
        .unwrap();
    let arguments = first_descendant_named(&generic.top_node(), "GenericArgumentClause").unwrap();
    assert_eq!(
        direct_child_names(&arguments),
        ["<", "TupleExpression", ",", "IdentifierType", ">"],
        "{generic}"
    );

    let literal = parser.parse("[1, 2, 3]").unwrap();
    assert_eq!(named_node_count(&literal.top_node(), "InlineArrayType"), 0);
    assert_eq!(
        named_node_count(&literal.top_node(), "CollectionExpression"),
        1
    );

    // SwiftSyntax TypeTests.swift:testMultiline requires `of` to remain on the
    // count's line so an array literal cannot be reinterpreted later. Cover
    // both value and type counts because type lookahead may consume trivia.
    assert!(parser.parse("[3\nof Int]").is_err());
    assert!(parser.parse("[Int\nof Int]").is_err());
}

#[test]
fn deprecated_generic_where_clauses_belong_to_parameter_clauses() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source) in DEPRECATED_GENERIC_WHERE_CASES {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{origin}: {error}\n{source}"));
        assert!(
            named_node_count(&tree.top_node(), "GenericWhereClause") > 0,
            "{origin}: {tree}"
        );
    }

    let ownership = parser
        .parse("func f<T: Mashable where T: Womparable>(x: T) where T: Equatable {}")
        .unwrap();
    let parameters =
        first_descendant_named(&ownership.top_node(), "GenericParameterClause").unwrap();
    assert_eq!(
        direct_child_names(&parameters),
        ["<", "GenericParameter", "GenericWhereClause", ">"],
        "{ownership}"
    );
    assert_eq!(
        named_node_count(&ownership.top_node(), "GenericWhereClause"),
        2,
        "{ownership}"
    );
}

#[test]
fn value_generic_requirements_and_operator_names_follow_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax ValueGenericsTests.swift:190-208 permits integer expressions
    // on either side of a same-type requirement, but never after `:`.
    let requirements = parser
        .parse("extension Vector where N == 123, -123 == M {}")
        .unwrap();
    let requirement_nodes = first_descendant_named(&requirements.top_node(), "GenericWhereClause")
        .unwrap()
        .children_by_name("GenericRequirement");
    assert_eq!(requirement_nodes.len(), 2, "{requirements}");
    assert_eq!(
        direct_child_names(&requirement_nodes[0]),
        ["IdentifierType", "==", "IntegerLiteralExpression"],
        "{requirements}"
    );
    assert_eq!(
        direct_child_names(&requirement_nodes[1]),
        ["PrefixOperatorExpression", "==", "IdentifierType"],
        "{requirements}"
    );

    // ValueGenericsTests.swift:292 and SwiftSyntax's parseFuncDeclaration
    // exercise both contextual generic-parameter prefixes after an operator.
    for source in [
        "func *<let X: Int, let Y: Int>(l: A<X>, r: A<Y>) -> Int { l.int * r.int }",
        "func %%%<each T>(x: repeat each T) {}",
    ] {
        parser.parse(source).unwrap();
    }

    let invalid = "extension Vector where N: 123 {}";
    assert!(
        parser.parse(invalid).is_err(),
        "unexpectedly accepted {invalid}"
    );
}

#[test]
fn generic_requirement_comments_preserve_comma_continuations() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Swift 064859e41d68, StringProtocol.swift:27-31. Horizontal trivia
    // between the comma and a line comment does not turn the following
    // requirement into a separate member declaration.
    let official = parser
        .parse(
            "protocol P {\n  associatedtype UTF8View: /*Bidirectional*/Collection\n  where UTF8View.Element == UInt8, // Unicode.UTF8.CodeUnit\n        UTF8View.Index == Index\n}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&official.top_node(), "GenericRequirement"),
        2,
        "{official}"
    );
    assert_eq!(
        named_node_count(&official.top_node(), "LineComment"),
        1,
        "{official}"
    );

    let block_comment = parser
        .parse(
            "protocol P {\n  associatedtype View: Collection\n  where View.Element == UInt8, /* outer /* separator */ outer */\n        View.Index == Int\n}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&block_comment.top_node(), "GenericRequirement"),
        2,
        "{block_comment}"
    );
    assert_eq!(
        named_node_count(&block_comment.top_node(), "BlockComment"),
        2,
        "{block_comment}"
    );

    // SwiftSyntax TrailingCommaTests.swift:335. The continuation marker only
    // belongs to commas with a following requirement; an actual trailing
    // comma retains its original declaration-boundary behavior.
    let trailing = parser
        .parse("struct T: P1, P2, where P1: Equatable, P2: Equatable, { }")
        .unwrap();
    assert_eq!(
        named_node_count(&trailing.top_node(), "GenericRequirement"),
        2,
        "{trailing}"
    );
}

#[test]
fn matching_patterns_accept_ternary_expression_fallback() {
    // The mise-pinned Swift 6.3.3 frontend accepts both the ternary colon and
    // the enclosing switch-case colon in this expression-pattern position.
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse("switch value { case condition ? first : second: break }")
        .unwrap();
    let ternary = first_descendant_named(&tree.top_node(), "TernaryExpression").unwrap();
    assert_eq!(
        direct_child_names(&ternary),
        [
            "DeclReferenceExpression",
            "DeclReferenceExpression",
            ":",
            "DeclReferenceExpression",
        ],
        "{tree}"
    );
    let label = first_descendant_named(&tree.top_node(), "SwitchCaseLabel").unwrap();
    assert_eq!(
        direct_child_names(&label),
        ["case", "SwitchCaseItem", ":"],
        "{tree}"
    );
}

#[test]
fn switch_case_attributes_are_owned_by_their_labels() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source, label, body_item) in [
        (
            "SwiftSyntax 60e8eb850721 materialized 03090, SwitchTests.swift:781",
            "switch Whatever.Thing {\ncase .Thing:\n@unknown case _:\n  x = 0\n}",
            "case",
            "ExpressionStatement",
        ),
        (
            "SwiftSyntax 60e8eb850721 materialized 03091, SwitchTests.swift:793",
            "switch Whatever.Thing {\ncase .Thing:\n@unknown default:\n  x = 0\n}",
            "default",
            "ExpressionStatement",
        ),
        (
            "SwiftSyntax 60e8eb850721 materialized 03128, SwitchTests.swift:1279",
            "func testReturnBeforeUnknownDefault() {\n  switch x {\n  case 1:\n    return\n  @unknown default:\n    break\n  }\n}",
            "default",
            "BreakStatement",
        ),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            panic!("{origin}: {error}\n{source}");
        });
        let cases = switch_cases(&tree.top_node());
        assert_eq!(cases.len(), 2, "{origin}: {tree}");
        let attributed = &cases[1];
        assert_eq!(
            direct_child_names(attributed),
            ["Attribute", "SwitchCaseLabel", "SwitchCaseCodeBlockItem"],
            "{origin}: {tree}"
        );
        assert_eq!(
            direct_child_names(&attributed.child_by_name("Attribute").unwrap()),
            ["AttributeName"],
            "{origin}: {tree}"
        );
        assert!(
            attributed
                .child_by_name("SwitchCaseLabel")
                .unwrap()
                .child_by_name(label)
                .is_some(),
            "{origin}: {tree}"
        );
        assert_eq!(
            direct_child_names(&attributed.child_by_name("SwitchCaseCodeBlockItem").unwrap()),
            [body_item],
            "{origin}: {tree}"
        );
        if label == "default" && body_item == "BreakStatement" {
            assert_eq!(
                direct_child_names(&cases[0]),
                ["SwitchCaseLabel", "SwitchCaseCodeBlockItem"],
                "{origin}: {tree}"
            );
            assert_eq!(
                direct_child_names(&cases[0].child_by_name("SwitchCaseCodeBlockItem").unwrap()),
                ["ReturnStatement"],
                "{origin}: {tree}"
            );
        }
    }

    // Swift 6.3.3 `swiftc -frontend -parse` accepts an attributed declaration
    // inside the case body; it must not start a nested SwitchCase.
    let declaration = parser
        .parse("switch value {\ncase _:\n  @MainActor func local() {}\n}")
        .unwrap();
    let cases = switch_cases(&declaration.top_node());
    assert_eq!(cases.len(), 1, "{declaration}");
    assert_eq!(
        direct_child_names(&cases[0]),
        ["SwitchCaseLabel", "SwitchCaseCodeBlockItem"],
        "{declaration}"
    );
    assert_eq!(
        direct_child_names(&cases[0].child_by_name("SwitchCaseCodeBlockItem").unwrap()),
        ["Attribute", "FunctionDeclaration"],
        "{declaration}"
    );

    assert!(
        parser
            .parse("switch value { @unknown(flag) default: break }")
            .is_err()
    );
}

#[test]
fn conditional_switch_cases_follow_the_official_list_boundary() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax StatementTests.swift:191 and the ANTLR Swift 5 grammar both
    // model conditional switch cases as a recursive switch-case list.
    let conditional = parser
        .parse(
            "switch value {\ncase .a: break\n#if NEVER\n#elseif ENABLE_C\ncase .c: break\n#endif\n}",
        )
        .unwrap();
    let switch = first_descendant_named(&conditional.top_node(), "SwitchExpression").unwrap();
    let if_config = switch.child_by_name("IfConfigDeclaration").unwrap();
    let elseif_clause = if_config.child_by_name("IfConfigElseifClause").unwrap();
    assert!(
        elseif_clause.child_by_name("SwitchCase").is_some(),
        "{conditional}"
    );

    // SwiftSyntax Sources use this shape for resilient switch exhaustivity.
    let attributed = parser
        .parse("switch value {\n#if RESILIENT_LIBRARIES\n@unknown default: fatalError()\n#endif\n}")
        .unwrap();
    let clause = first_descendant_named(&attributed.top_node(), "IfConfigIfClause").unwrap();
    let nested_case = clause.child_by_name("SwitchCase").unwrap();
    assert!(
        nested_case.child_by_name("Attribute").is_some(),
        "{attributed}"
    );

    // A conditional declaration whose first clause contains ordinary code is
    // owned by the current case body, not by the outer case list.
    let body = parser
        .parse("switch value {\ncase .a:\n#if FLAG\nlet local = 1\n#endif\ncase .b: break\n}")
        .unwrap();
    let switch = first_descendant_named(&body.top_node(), "SwitchExpression").unwrap();
    assert!(
        switch.child_by_name("IfConfigDeclaration").is_none(),
        "{body}"
    );
    let first_case = switch.child_by_name("SwitchCase").unwrap();
    assert!(
        first_case
            .children_by_name("SwitchCaseCodeBlockItem")
            .iter()
            .any(|item| item.child_by_name("IfConfigDeclaration").is_some()),
        "{body}"
    );

    // StatementTests.swift:372: when the first clause is a diagnostic body,
    // a case in a later clause is not promoted to the outer switch list.
    assert!(
        parser
            .parse(
                "switch value {\ncase .a: break\n#if FLAG\n#warning(\"message\")\n#else\ncase .b: break\n#endif\n}",
            )
            .is_err()
    );
}

#[test]
fn switch_case_diagnostics_follow_list_and_body_contexts() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax StatementTests.swift:234 and 267.
    let list = parser
        .parse(
            "switch value {\n#warning(\"before\")\n#if FLAG\n#error(\"inside\")\n#endif\ncase .a: break\n}",
        )
        .unwrap();
    let switch = first_descendant_named(&list.top_node(), "SwitchExpression").unwrap();
    assert!(
        switch.child_by_name("MacroExpansionDeclaration").is_some(),
        "{list}"
    );
    let clause = switch
        .child_by_name("IfConfigDeclaration")
        .unwrap()
        .child_by_name("IfConfigIfClause")
        .unwrap();
    assert!(
        clause.child_by_name("MacroExpansionDeclaration").is_some(),
        "{list}"
    );

    // StatementTests.swift:327: after a case label, the same diagnostic is an
    // expression statement of that case body.
    let body = parser
        .parse("switch value {\ncase .a:\ncall()\n#warning(\"body\")\ncase .b: break\n}")
        .unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser()
                .parse("switch value {\ncase .a:\ncall()\n#warning(\"body\")\ncase .b: break\n}")
                .unwrap();
            panic!("{error}\n{recovered}")
        });
    let switch = first_descendant_named(&body.top_node(), "SwitchExpression").unwrap();
    assert_eq!(
        named_node_count(&switch, "MacroExpansionDeclaration"),
        0,
        "{body}"
    );
    let first_case = switch.child_by_name("SwitchCase").unwrap();
    assert_eq!(
        named_node_count(&first_case, "MacroExpansionExpression"),
        1,
        "{body}"
    );

    // StatementTests.swift:415: after a list-level if-config has closed, the
    // diagnostic is again a sibling case-list element.
    let after_if_config = parser
        .parse(
            "switch value {\ncase .a: break\n#if FLAG\ncase .b: break\n#endif\n#warning(\"after\")\ndefault: break\n}",
        )
        .unwrap();
    let switch = first_descendant_named(&after_if_config.top_node(), "SwitchExpression").unwrap();
    assert!(
        switch.child_by_name("IfConfigDeclaration").is_some()
            && switch.child_by_name("MacroExpansionDeclaration").is_some(),
        "{after_if_config}"
    );
}

#[test]
fn binding_introducer_patterns_separate_identifiers_from_references() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in BINDING_INTRODUCER_PATTERN_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }

    let generic = parser
        .parse(BINDING_INTRODUCER_PATTERN_CASES[0].1)
        .unwrap()
        .to_string();
    assert!(
        generic.contains("GenericSpecializationExpression(DeclReferenceExpression"),
        "{generic}"
    );
    assert!(
        generic.contains("ConditionLabeledExpression(IdentifierPattern"),
        "{generic}"
    );

    let tuple = parser
        .parse(BINDING_INTRODUCER_PATTERN_CASES[1].1)
        .unwrap()
        .to_string();
    assert!(
        tuple.contains("SubscriptCallExpression(DeclReferenceExpression"),
        "{tuple}"
    );
    assert!(
        tuple.contains("ConditionLabeledExpression(IdentifierPattern"),
        "{tuple}"
    );

    let subscript = parser
        .parse(BINDING_INTRODUCER_PATTERN_CASES[2].1)
        .unwrap()
        .to_string();
    assert!(
        subscript.contains("SubscriptCallExpression(DeclReferenceExpression"),
        "{subscript}"
    );
    assert!(
        subscript.contains("ConditionLabeledExpression(IdentifierPattern"),
        "{subscript}"
    );

    let nested_call = parser
        .parse(BINDING_INTRODUCER_PATTERN_CASES[3].1)
        .unwrap()
        .to_string();
    assert!(nested_call.contains("ValueBindingPattern"), "{nested_call}");
    assert!(nested_call.contains("IdentifierPattern"), "{nested_call}");

    let nested_tuple = parser
        .parse(BINDING_INTRODUCER_PATTERN_CASES[4].1)
        .unwrap()
        .to_string();
    assert!(nested_tuple.contains("WildcardPattern"), "{nested_tuple}");
    assert!(nested_tuple.contains("IdentifierPattern"), "{nested_tuple}");

    let optional = parser
        .parse(BINDING_INTRODUCER_PATTERN_CASES[5].1)
        .unwrap()
        .to_string();
    assert!(
        optional.contains("OptionalChainingExpression(IdentifierPattern"),
        "{optional}"
    );

    let ordinary = parser
        .parse(BINDING_INTRODUCER_PATTERN_CASES[6].1)
        .unwrap()
        .to_string();
    assert!(ordinary.contains("DeclReferenceExpression"), "{ordinary}");
    assert!(!ordinary.contains("IdentifierPattern"), "{ordinary}");

    let wildcard = parser.parse("switch 0 { case _: () }").unwrap().to_string();
    assert!(wildcard.contains("WildcardPattern"), "{wildcard}");
    assert!(
        !wildcard.contains("DiscardAssignmentExpression"),
        "{wildcard}"
    );
    let discard = parser.parse("_ = value").unwrap().to_string();
    assert!(discard.contains("DiscardAssignmentExpression"), "{discard}");
}

#[test]
fn matching_value_bindings_recurse_as_patterns() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax 60e8eb850721, translated/MatchingPatternsTests.swift:78-82.
    // parseMatchingPattern recursively re-enters the binding-introducer
    // context after every accepted binding specifier.
    let tree = parser
        .parse(
            "switch value {\ncase var var a: a += 1\ncase var let b: print(b)\ncase var (var c): c += 1\n}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&tree.top_node(), "ValueBindingPattern"),
        6,
        "{tree}"
    );

    let first_pattern = first_descendant_named(&tree.top_node(), "SwitchCasePattern").unwrap();
    let outer = first_pattern.child_by_name("ValueBindingPattern").unwrap();
    let inner = outer.child_by_name("ValueBindingPattern").unwrap();
    assert!(inner.child_by_name("IdentifierPattern").is_some(), "{tree}");
}

#[test]
fn binding_introducer_keyword_names_follow_upstream_context() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let self_binding = parser
        .parse("switch 0 { case let self: () }")
        .unwrap()
        .to_string();
    assert!(self_binding.contains("IdentifierPattern"), "{self_binding}");
    assert!(
        !self_binding.contains("DeclReferenceExpression"),
        "{self_binding}"
    );

    let self_member = parser
        .parse("switch 0 { case let self.member: () }")
        .unwrap()
        .to_string();
    assert!(
        self_member.contains("MemberAccessExpression(DeclReferenceExpression"),
        "{self_member}"
    );
    assert!(!self_member.contains("IdentifierPattern"), "{self_member}");

    let self_type = parser
        .parse("switch 0 { case let Self: () }")
        .unwrap()
        .to_string();
    assert!(self_type.contains("DeclReferenceExpression"), "{self_type}");
    assert!(!self_type.contains("IdentifierPattern"), "{self_type}");

    // The mise-pinned frontend accepts this dollar identifier in closure context.
    let dollar = parser
        .parse("let f = { switch 0 { case let $0: () } }")
        .unwrap()
        .to_string();
    assert!(
        dollar.contains("DeclReferenceExpression(DollarIdentifier"),
        "{dollar}"
    );
}

#[test]
fn dollar_prefixed_identifiers_follow_swiftsyntax_lexical_classification() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // translated/DebuggerTests.swift:27. A nonnumeric `$` spelling is an
    // ordinary Identifier in both pattern and reference positions.
    let debugger = parser
        .parse("var ($x0, $x1) = (4, 3)\nvar z = $x0 + $x1")
        .unwrap();
    assert_eq!(
        named_node_count(&debugger.top_node(), "DollarIdentifier"),
        0,
        "{debugger}"
    );
    assert_eq!(
        named_node_count(&debugger.top_node(), "IdentifierPattern"),
        3,
        "{debugger}"
    );

    let label = parser.parse("$label: if true { break $label }").unwrap();
    assert_eq!(
        named_node_count(&label.top_node(), "DollarIdentifier"),
        0,
        "{label}"
    );

    // translated/DollarIdentifierTests.swift:236. The same lexical class is
    // shared by declaration names and attributes instead of being admitted at
    // each grammar site as a DollarIdentifier exception.
    let declarations = parser
        .parse(
            "func $declareWithDollar() { var $foo = 1 }\n\
             switch 0 { @$dollar case _: break }",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&declarations.top_node(), "DollarIdentifier"),
        0,
        "{declarations}"
    );
    assert_eq!(
        named_node_count(&declarations.top_node(), "AttributeName"),
        1,
        "{declarations}"
    );

    // Purely numeric closure arguments retain the dedicated token kind.
    let shorthand = parser.parse("let f = { $0 }").unwrap();
    assert_eq!(
        named_node_count(&shorthand.top_node(), "DollarIdentifier"),
        1,
        "{shorthand}"
    );
}

#[test]
fn class_restriction_types_are_local_to_inheritance_clauses() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // translated/MetatypeObjectConversionTests.swift:28.
    let tree = parser
        .parse("protocol NonClassProto {}\nprotocol ClassConstrainedProto : class {}")
        .unwrap();
    let inherited = first_descendant_named(&tree.top_node(), "InheritedType").unwrap();
    assert_eq!(
        direct_child_names(&inherited),
        ["ClassRestrictionType"],
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "ClassRestrictionType"),
        1,
        "{tree}"
    );
}

#[test]
fn local_and_detailed_declaration_modifiers_follow_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Swift stdlib 064859e41d68, Distributed/DistributedActor.swift:373.
    let local = parser
        .parse("func f() { _local let localSelf = self }")
        .unwrap();
    let modifier = first_descendant_named(&local.top_node(), "DeclarationModifier").unwrap();
    assert_eq!(direct_child_names(&modifier), ["_local"], "{local}");
    assert_eq!(
        named_node_count(&local.top_node(), "VariableDeclaration"),
        1
    );

    // Swift 6.3.3's frontend also accepts a following modifier before the
    // declaration introducer. This guards the declaration-shaped lookahead,
    // rather than special-casing only `_local let`.
    assert!(
        parser
            .parse("_local private let value = 0\n_local final class C {}\n_local #memberwiseInit",)
            .is_ok()
    );

    // SwiftSyntax 60e8eb850721, DeclarationTests.swift:369-370 and :254.
    let detailed = parser
        .parse(
            "private(set) var value = 0\nunowned(unsafe) let object: AnyObject = source\nnonisolated(unsafe) let global = 0",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&detailed.top_node(), "ModifierDetail"),
        3,
        "{detailed}"
    );

    // `_local` remains an ordinary identifier when it is not followed by a
    // declaration, and plain modifiers cannot acquire an arbitrary detail.
    assert!(parser.parse("func f() { _local() }").is_ok());
    assert!(
        parser
            .parse("func f() { _local(value) let localSelf = self }")
            .is_err()
    );
    assert!(parser.parse("final(value) let item = 0").is_err());
}

#[test]
fn dotted_keyword_members_are_decl_reference_expressions() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source) in [
        (
            "Swift 064859e41d68596f486c5d724401cb370f260409, OutputStream.swift:339",
            "switch style { case .struct: break }",
        ),
        (
            "Swift 064859e41d68596f486c5d724401cb370f260409, ReflectionMirror.swift:171",
            "struct Reflection { var displayStyle: Any; init() { self.displayStyle = .class } }",
        ),
        (
            "SwiftSyntax 60e8eb850721, RegexLiteralLexer.swift:297",
            "switch result { case .continue: break }",
        ),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            panic!("{origin}: {error}\n{source}");
        });
        assert_eq!(
            named_node_count(&tree.top_node(), "MemberName"),
            0,
            "{tree}"
        );
        let member = first_descendant_named(&tree.top_node(), "ImplicitMemberExpression")
            .expect("upstream keyword member");
        assert_eq!(
            direct_child_names(&member),
            [".", "DeclReferenceExpression"],
            "{tree}"
        );
        let reference = member.child_by_name("DeclReferenceExpression").unwrap();
        assert_eq!(direct_child_names(&reference), ["Identifier"], "{tree}");
    }

    let normal = parser.parse("value.operator").unwrap();
    let member = first_descendant_named(&normal.top_node(), "MemberAccessExpression").unwrap();
    assert_eq!(
        direct_child_names(&member),
        ["DeclReferenceExpression", ".", "DeclReferenceExpression"],
        "{normal}"
    );
    assert_eq!(
        direct_child_names(&member_decl_reference(&member)),
        ["Identifier"],
        "{normal}"
    );
    assert_eq!(named_node_count(&normal.top_node(), "MemberName"), 0);

    let implicit = parser.parse("let value = .class").unwrap();
    let implicit_member =
        first_descendant_named(&implicit.top_node(), "ImplicitMemberExpression").unwrap();
    assert_eq!(
        direct_child_names(&implicit_member),
        [".", "DeclReferenceExpression"],
        "{implicit}"
    );

    // Pinned SwiftSyntax sources Parser.swift and Identifier.swift use `Self`
    // in both implicit and ordinary member-reference positions.
    let implicit_self = parser.parse("let value = .Self").unwrap();
    let implicit_self_member =
        first_descendant_named(&implicit_self.top_node(), "ImplicitMemberExpression").unwrap();
    let implicit_self_reference = implicit_self_member
        .child_by_name("DeclReferenceExpression")
        .unwrap();
    assert_eq!(direct_child_names(&implicit_self_reference), ["Self"]);

    let normal_self = parser.parse("let value = String.Self").unwrap();
    let normal_self_member =
        first_descendant_named(&normal_self.top_node(), "MemberAccessExpression").unwrap();
    assert_eq!(
        direct_child_names(&member_decl_reference(&normal_self_member)),
        ["Self"]
    );

    // Pinned Swift 6.3.3 frontend accepts lexer-classified `inout` after a
    // dot; SwiftSyntax remaps it to an identifier declaration reference.
    let implicit_inout = parser.parse("let value = .inout").unwrap();
    let inout_member =
        first_descendant_named(&implicit_inout.top_node(), "ImplicitMemberExpression").unwrap();
    let inout_reference = inout_member
        .child_by_name("DeclReferenceExpression")
        .unwrap();
    assert_eq!(direct_child_names(&inout_reference), ["Identifier"]);

    for (source, expected_child) in [("value.self", "self"), ("value.0", "IntegerLiteral")] {
        let tree = parser.parse(source).unwrap();
        let member = first_descendant_named(&tree.top_node(), "MemberAccessExpression").unwrap();
        let reference = member_decl_reference(&member);
        assert_eq!(direct_child_names(&reference), [expected_child], "{tree}");
        assert!(!reference.to_string().contains("MemberName"), "{tree}");
    }
}

#[test]
fn dotted_decl_name_arguments_do_not_mask_calls() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source, arguments, base_name) in [
        (
            "Swift 064859e41d68596f486c5d724401cb370f260409, CoreSymbolication.swift:382",
            "String.init(cString:)",
            1,
            "init",
        ),
        (
            "Swift 064859e41d68596f486c5d724401cb370f260409, DebuggerSupport.swift:147",
            "String.init(reflecting:)",
            1,
            "init",
        ),
        (
            "pinned Swift 6.3.3 frontend, dedicated Self declaration reference",
            "String.Self(cString:)",
            1,
            "Self",
        ),
        (
            "SwiftSyntax 60e8eb850721, Expressions.swift compound declaration-name shape",
            "Thing.method(first:_:)",
            2,
            "Identifier",
        ),
        (
            "comment trivia between declaration-name labels",
            "Thing.method(/* before */ first /* colon */ : /* next */ _:)",
            2,
            "Identifier",
        ),
        (
            "pinned Swift 6.3.3 frontend, raw member declaration reference",
            "Thing.`method`(label:)",
            1,
            "Identifier",
        ),
        (
            "pinned Swift 6.3.3 frontend, remapped keyword declaration reference",
            "Thing.class(label:)",
            1,
            "Identifier",
        ),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            panic!("{origin}: {error}\n{source}");
        });
        let member = first_descendant_named(&tree.top_node(), "MemberAccessExpression").unwrap();
        let reference = member_decl_reference(&member);
        assert_eq!(
            direct_child_names(&reference),
            [base_name, "DeclNameArguments"],
            "{tree}"
        );
        let labels = reference.child_by_name("DeclNameArguments").unwrap();
        assert_eq!(
            labels.children_by_name("DeclNameArgument").len(),
            arguments,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "FunctionCallExpression"),
            0,
            "{tree}"
        );
    }

    let generic = parser.parse("Thing.method(first:_:)<T>").unwrap();
    assert!(
        first_descendant_named(&generic.top_node(), "DeclNameArguments").is_some(),
        "{generic}"
    );

    for source in [
        "foo.bar()",
        "foo.bar(label: value)",
        "foo.bar(first: value, second: other)",
    ] {
        let tree = parser.parse(source).unwrap();
        let call = first_descendant_named(&tree.top_node(), "FunctionCallExpression").unwrap();
        let member = first_descendant_named(&call, "MemberAccessExpression").unwrap();
        assert!(
            member_decl_reference(&member)
                .child_by_name("DeclNameArguments")
                .is_none(),
            "{tree}"
        );
    }

    // CST construction preserves the call. Strict syntax validation owns the
    // frontend rule that rejects `inout` as an argument label.
    let inout_call = parser.parse("value.member(inout: value)").unwrap();
    let call = first_descendant_named(&inout_call.top_node(), "FunctionCallExpression").unwrap();
    let member = first_descendant_named(&call, "MemberAccessExpression").unwrap();
    assert!(
        member_decl_reference(&member)
            .child_by_name("DeclNameArguments")
            .is_none(),
        "{inout_call}"
    );
}

#[test]
fn unqualified_decl_name_arguments_do_not_mask_calls() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source, expected_children, arguments) in [
        (
            "SwiftSyntax Sources/SwiftCompilerPluginMessageHandling/StandardIOMessageConnection.swift:24",
            "_dup(_:)",
            &["Identifier", "DeclNameArguments"][..],
            1,
        ),
        (
            "SwiftSyntax Sources/SwiftSyntax/BumpPtrAllocator.swift:152",
            "test(_:)",
            &["Identifier", "DeclNameArguments"][..],
            1,
        ),
        (
            "SwiftSyntax Sources/SwiftSyntax/SourceLocation.swift:199",
            "computeLines(text:)",
            &["Identifier", "DeclNameArguments"][..],
            1,
        ),
        (
            "pinned Swift 6.3.3 module selector",
            "Swift::function(first:_:)",
            &["ModuleSelector", "Identifier", "DeclNameArguments"][..],
            2,
        ),
        (
            "pinned Swift 6.3.3 initializer reference",
            "init(_:)",
            &["init", "DeclNameArguments"][..],
            1,
        ),
        (
            "pinned Swift 6.3.3 Self reference",
            "Self(_:)",
            &["Self", "DeclNameArguments"][..],
            1,
        ),
        (
            "pinned Swift 6.3.3 self reference",
            "self(_:)",
            &["self", "DeclNameArguments"][..],
            1,
        ),
        (
            "pinned Swift 6.3.3 raw identifier reference",
            "`function`(label:)",
            &["Identifier", "DeclNameArguments"][..],
            1,
        ),
    ] {
        let tree = parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{origin}: {error}\n{source}"));
        let reference =
            first_descendant_named(&tree.top_node(), "DeclReferenceExpression").unwrap();
        assert_eq!(
            direct_child_names(&reference),
            expected_children,
            "{origin}: {tree}"
        );
        assert_eq!(
            reference
                .child_by_name("DeclNameArguments")
                .unwrap()
                .children_by_name("DeclNameArgument")
                .len(),
            arguments,
            "{origin}: {tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "FunctionCallExpression"),
            0,
            "{origin}: {tree}"
        );
    }

    for source in [
        "function()",
        "function(value)",
        "function(label: value)",
        "self(label: value)",
        "Self(label: value)",
        "init() { init(label: value) }",
    ] {
        let tree = parser.parse(source).unwrap();
        let call = first_descendant_named(&tree.top_node(), "FunctionCallExpression")
            .unwrap_or_else(|| panic!("{source}: {tree}"));
        let reference = first_descendant_named(&call, "DeclReferenceExpression").unwrap();
        assert!(
            reference.child_by_name("DeclNameArguments").is_none(),
            "{tree}"
        );
    }

    // SwiftSyntax's argument-label predicate excludes `inout` in both the
    // dotted and unqualified declaration-name forms.
    assert!(parser.parse("function(inout:)").is_err());
}

#[test]
fn key_path_roots_and_components_follow_swiftsyntax_ownership() {
    // Pinned SwiftSyntax 60e8eb850721, ExpressionTests.swift:199-313.
    let rootless = strict_key_path(r"\.?.foo");
    assert_eq!(
        direct_child_names(&rootless),
        ["\\", "KeyPathComponent", "KeyPathComponent"],
        "{rootless}"
    );
    let components = rootless.children_by_name("KeyPathComponent");
    assert_eq!(
        direct_child_names(&components[0]),
        [".", "KeyPathOptionalComponent"]
    );
    assert_eq!(
        direct_child_names(&components[1]),
        [".", "KeyPathPropertyComponent"]
    );

    let generic = strict_key_path(r"\Lens<[Int]>.[0]");
    assert_eq!(
        direct_child_names(&generic),
        ["\\", "IdentifierType", "KeyPathComponent"],
        "{generic}"
    );
    let generic_component = generic.child_by_name("KeyPathComponent").unwrap();
    assert_eq!(
        direct_child_names(&generic_component),
        [".", "KeyPathSubscriptComponent"]
    );

    let direct_subscript = strict_key_path(r"\ABCProtocol[100]");
    assert_eq!(
        direct_child_names(&direct_subscript),
        ["\\", "IdentifierType", "KeyPathComponent"],
        "{direct_subscript}"
    );
    assert_eq!(
        direct_child_names(&direct_subscript.child_by_name("KeyPathComponent").unwrap()),
        ["KeyPathSubscriptComponent"]
    );

    let tuple = strict_key_path(r"\(UnsafeRawPointer?, String).1");
    assert_eq!(
        direct_child_names(&tuple),
        ["\\", "TupleType", "KeyPathComponent"],
        "{tuple}"
    );
    let tuple_reference = first_descendant_named(&tuple, "DeclReferenceExpression").unwrap();
    assert_eq!(direct_child_names(&tuple_reference), ["IntegerLiteral"]);

    let optional = strict_key_path(r"\String?.!.count");
    assert_eq!(
        direct_child_names(&optional),
        ["\\", "OptionalType", "KeyPathComponent", "KeyPathComponent"],
        "{optional}"
    );
    let optional_root = optional.child_by_name("OptionalType").unwrap();
    assert_eq!(direct_child_names(&optional_root), ["IdentifierType", "?"]);

    let metatype = strict_key_path(r"\Foo.Type.init()");
    let metatype_root = metatype.child_by_name("MetatypeType").unwrap();
    assert_eq!(
        direct_child_names(&metatype_root),
        ["IdentifierType", ".", "TypeKeyword"]
    );
    let method = metatype.child_by_name("KeyPathComponent").unwrap();
    assert_eq!(direct_child_names(&method), [".", "KeyPathMethodComponent"]);

    let protocol = strict_key_path(r"\Foo.Protocol.member");
    let protocol_root = protocol.child_by_name("MetatypeType").unwrap();
    assert_eq!(
        direct_child_names(&protocol_root),
        ["IdentifierType", ".", "ProtocolKeyword"]
    );

    for source in [r"\Foo.TypeName.member", r"\Foo.ProtocolBuffer.member"] {
        let property_path = strict_key_path(source);
        assert_eq!(
            named_node_count(&property_path, "MetatypeType"),
            0,
            "{property_path}"
        );
        assert_eq!(
            property_path.children_by_name("KeyPathComponent").len(),
            2,
            "{property_path}"
        );
    }

    let repeated_optional = strict_key_path(r"\Optional.?!?!?!?");
    assert_eq!(
        repeated_optional.children_by_name("KeyPathComponent").len(),
        7,
        "{repeated_optional}"
    );
}

#[test]
fn key_path_methods_and_compound_names_remain_distinct() {
    // Pinned SwiftSyntax 60e8eb850721, ExpressionTests.swift:335-525 and
    // TrailingCommaTests.swift:70.
    for (source, components, arguments) in [
        (r"\Foo.method()", 1, 0),
        (r"\Foo.method(arg: 10)", 1, 1),
        (r"\Foo.bar[0,]", 2, 1),
        (r"\Foo.Bar.[2]", 2, 1),
    ] {
        let key_path = strict_key_path(source);
        assert_eq!(
            key_path.children_by_name("KeyPathComponent").len(),
            components,
            "{key_path}"
        );
        assert_eq!(
            named_node_count(&key_path, "LabeledExpression"),
            arguments,
            "{key_path}"
        );
    }

    for source in [r"\Foo.method(arg:)", r"\Foo.t(a:)(2)"] {
        let key_path = strict_key_path(source);
        let reference = first_descendant_named(&key_path, "DeclReferenceExpression").unwrap();
        assert!(
            reference.child_by_name("DeclNameArguments").is_some(),
            "{key_path}"
        );
    }

    let compound_property = strict_key_path(r"\Foo.method(arg:)");
    assert!(
        first_descendant_named(&compound_property, "KeyPathPropertyComponent").is_some(),
        "{compound_property}"
    );
    assert_eq!(
        named_node_count(&compound_property, "KeyPathMethodComponent"),
        0
    );

    let applied_method = strict_key_path(r"\Foo.t(a:)(2)");
    assert!(
        first_descendant_named(&applied_method, "KeyPathMethodComponent").is_some(),
        "{applied_method}"
    );

    let parser = rezel_lang_swift::parser().with_strict(true);
    let invalid_generic_method = parser.parse(r"\Foo.method<Int>()");
    assert!(
        invalid_generic_method.is_err(),
        "{}",
        invalid_generic_method.unwrap()
    );
}

#[test]
fn key_path_phase_boundary_preserves_following_operators() {
    // Pinned SwiftSyntax 60e8eb850721, ExpressionTests.swift:289-333 and
    // 751-812. Once an optional/subscript component is seen, a dotted
    // optional sequence belongs to the surrounding expression instead.
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (source, components) in [(r"\Foo?.?.bar.?.blah", 2), (r"\Foo?.?.?.blah", 1)] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}");
        });
        let top = tree.top_node();
        let sequence = first_descendant_named(&top, "SequenceExpression").unwrap();
        assert_eq!(
            sequence.children_by_name("BinaryOperator").len(),
            1,
            "{tree}"
        );
        let key_path = first_descendant_named(&sequence, "KeyPathExpression").unwrap();
        assert_eq!(
            key_path.children_by_name("KeyPathComponent").len(),
            components,
            "{tree}"
        );
    }

    for source in [r"\Optional.?!?!?!?.??!", r"\T.?.!", r"\T.abc[2].?"] {
        assert!(
            parser.parse(source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn key_path_components_are_selected_without_parallel_parse_stacks() {
    let parser = rezel_lang_swift::parser()
        .with_strict(true)
        .with_limits(ParseLimits {
            max_actions: 50_000,
            max_stacks: 1,
            max_stack_depth: 1_024,
            max_buffer_records: 20_000,
            max_recovery_actions: 0,
        });

    for source in [
        r"\ABCProtocol[100]",
        r"\Foo.Bar.[2]",
        r"\String?.!.count",
        r"\Foo.Type.init()",
        r"\Optional.?!?!?!?",
    ] {
        parser
            .parse(source)
            .unwrap_or_else(|error| panic!("{error}\n{source}"));
    }
}

#[test]
fn regex_literals_follow_swiftsyntax_delimiters_and_operator_boundaries() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let positive_cases = [
        (
            "RegexLiteralTests.swift:19",
            r"/(?<identifier>[[:alpha:]]\w*) = (?<hex>[0-9A-F]+)/",
        ),
        ("RegexLiteralTests.swift:273", "#//#"),
        ("RegexLiteralTests.swift:405", "#/\n abc\n/#"),
        (
            "ForwardSlashRegexSkippingTests.swift:19",
            r#"struct A { static let r = /test":"(.*?)"/ }"#,
        ),
        ("ForwardSlashRegexTests.swift:68", "_ = /abc/.self"),
        ("ForwardSlashRegexTests.swift:1785", "_ = /)/"),
        (
            "ForwardSlashRegexTests.swift:593",
            "func testThrow() throws { throw /x/ }",
        ),
    ];

    for (origin, source) in positive_cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let expression = first_descendant_named(&tree.top_node(), "RegexLiteralExpression")
            .unwrap_or_else(|| panic!("{origin}: {tree}"));
        assert_eq!(
            direct_child_names(&expression),
            ["RegexLiteral"],
            "{origin}: {tree}"
        );
    }

    for (origin, source) in [
        (
            "ForwardSlashRegexSkippingAllowedTests.swift:129",
            "func d() { _ = 1 / 2 + 3 * 4; _ = 1 / 2 / 3 / 4 }",
        ),
        (
            "ForwardSlashRegexTests.swift:21",
            "prefix operator /\nprefix operator ^/\nprefix operator /^/",
        ),
        ("operator reference", "foo(/)"),
        ("line comment", "let value = 1 // /not a regex/"),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            panic!("{origin}: {error}\n{source}");
        });
        assert_eq!(
            named_node_count(&tree.top_node(), "RegexLiteralExpression"),
            0,
            "{origin}: {tree}"
        );
    }
}

#[test]
fn source_file_shebang_is_a_leading_token_only() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax translated/HashbangMainTests.swift:19 and
    // translated/HashbangLibraryTests.swift:22.
    let source = "#!/usr/bin/swift\nlet x = 42\nx + x";
    let tree = parser.parse(source).unwrap();
    assert_eq!(
        direct_child_names(&tree.top_node()),
        ["Shebang", "CodeBlockItem", "CodeBlockItem"],
        "{tree}"
    );
    let shebang = tree.top_node().child_by_name("Shebang").unwrap();
    assert_eq!(
        usize::from(shebang.to()) - usize::from(shebang.from()),
        "#!/usr/bin/swift".len()
    );

    let eof = parser.parse("#!").unwrap();
    assert_eq!(direct_child_names(&eof.top_node()), ["Shebang"], "{eof}");

    // The generated token is root-position constrained. Leading trivia is
    // tolerated for recovery-friendly source ingestion, while a hashbang
    // after a source item cannot become Shebang.
    for source in [
        " #!/usr/bin/swift\nlet x = 42",
        "\n#!/usr/bin/swift\nlet x = 42",
        "// generated file\n#!/usr/bin/swift\nlet x = 42",
        "/* generated file */#!/usr/bin/swift\nlet x = 42",
    ] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(named_node_count(&tree.top_node(), "Shebang"), 1, "{tree}");
    }
    assert!(parser.parse("let x = 42\n#!/usr/bin/swift").is_err());
}

#[test]
fn custom_operators_follow_fixity_through_postfix_chains() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax 60e8eb850721, translated/OperatorsTests.swift:216.
    let prefix = parser.parse("var _: God = ^TheDevil()").unwrap();
    let prefix = first_descendant_named(&prefix.top_node(), "PrefixOperatorExpression").unwrap();
    assert_eq!(
        direct_child_names(&prefix),
        ["PrefixOperator", "FunctionCallExpression"],
        "{prefix}"
    );

    // SwiftSyntax 60e8eb850721,
    // translated/OptionalChainLvaluesTests.swift:72.
    let postfix = parser.parse("mutT?.mutS?.y++").unwrap();
    let postfix = first_descendant_named(&postfix.top_node(), "PostfixOperatorExpression").unwrap();
    assert_eq!(
        direct_child_names(&postfix),
        ["MemberAccessExpression"],
        "{postfix}"
    );

    // The same OperatorsTests case classifies whitespace on both sides as a
    // binary operator and retains the source-ordered sequence CST.
    let binary = parser.parse("var _: Man = TheDevil() ^ God()").unwrap();
    let sequence = first_descendant_named(&binary.top_node(), "SequenceExpression").unwrap();
    assert_sequence_children(
        &sequence,
        &[
            "FunctionCallExpression",
            "BinaryOperator",
            "FunctionCallExpression",
        ],
        1,
    );

    // SwiftSyntax GenericDisambiguationTests.swift:216. The empty angle
    // operator is postfix, after which ordinary member and call suffixes must
    // still recurse on the resulting expression.
    let chained = parser.parse("A<>.c()").unwrap();
    let call = first_descendant_named(&chained.top_node(), "FunctionCallExpression").unwrap();
    let member = call.child_by_name("MemberAccessExpression").unwrap();
    assert!(
        member.child_by_name("PostfixOperatorExpression").is_some(),
        "{chained}"
    );
}

#[test]
fn operator_regex_boundaries_follow_swiftsyntax_context() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // RegexLiteralTests.swift:1085, 1094, 1105, and 1115. Directive
    // newlines are structural even when the condition or first body item
    // starts with an operator-shaped bare regex delimiter.
    for source in [
        "#if /^ }}x/\n#endif",
        "#if true\n#else\n/^ }}x/\n#endif",
        "#if true\n#elseif /^ }}x/\n#endif",
        "#if true\n#endif\n/^ }}x/",
    ] {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}");
        });
    }

    // translated/ForwardSlashRegexTests.swift:742 and 781. A slash with
    // whitespace on both sides continues the preceding expression as a
    // binary operator instead of starting a new code item or regex literal.
    let division = parser
        .parse("let _: () -> Int = {\n0\n/ 1 /\n2\n}")
        .unwrap();
    assert_eq!(
        named_node_count(&division.top_node(), "RegexLiteralExpression"),
        0,
        "{division}"
    );
    assert_eq!(
        named_node_count(&division.top_node(), "BinaryOperator"),
        2,
        "{division}"
    );

    // translated/ForwardSlashRegexTests.swift:1313 and 1321. In a delimited
    // expression an unmatched right parenthesis terminates the operand, so
    // the two slashes are prefix/postfix operators rather than one regex.
    for source in ["_ = (/x)/", "_ = (/[(0)])/"] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), "RegexLiteralExpression"),
            0,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "PrefixOperatorExpression"),
            1,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "PostfixOperatorExpression"),
            1,
            "{tree}"
        );
    }

    // translated/ForwardSlashRegexSkippingAllowedTests.swift:163 and
    // translated/PrefixSlashTests.swift:28. The same delimiter context applies
    // recursively in a tuple and a call argument.
    let prefix_chain = parser.parse("(/E.e).foo(/0)").unwrap();
    assert_eq!(
        named_node_count(&prefix_chain.top_node(), "PrefixOperatorExpression"),
        2,
        "{prefix_chain}"
    );
    assert_eq!(
        named_node_count(&prefix_chain.top_node(), "RegexLiteralExpression"),
        0,
        "{prefix_chain}"
    );

    // SwiftSyntax RegexLiteralTests.swift:699 and 707. In expression-tail
    // context these slashes are operator spellings, not regex delimiters.
    for source in ["x /^ y/", "x !/^ y/"] {
        let tree = parser.parse(source).unwrap();
        let sequence = first_descendant_named(&tree.top_node(), "SequenceExpression").unwrap();
        assert_sequence_children(
            &sequence,
            &[
                "DeclReferenceExpression",
                "BinaryOperator",
                "PostfixOperatorExpression",
            ],
            1,
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "RegexLiteralExpression"),
            0,
            "{tree}"
        );
    }

    // RegexLiteralTests.swift:787. After an opening delimiter, `&` is the
    // prefix in-out operator and the slash begins the operand's regex.
    let inout = parser.parse("foo(&/^ }}x/)").unwrap();
    let inout_expression = first_descendant_named(&inout.top_node(), "InOutExpression").unwrap();
    assert_eq!(
        direct_child_names(&inout_expression),
        ["RegexLiteralExpression"],
        "{inout}"
    );

    // RegexLiteralTests.swift:1396. The question mark is the ternary
    // delimiter even though it is immediately followed by a regex slash.
    let ternary = parser.parse("let x = true ?/abc/ : /def/").unwrap();
    let ternary_expression =
        first_descendant_named(&ternary.top_node(), "TernaryExpression").unwrap();
    assert_eq!(
        named_node_count(&ternary_expression, "RegexLiteralExpression"),
        2,
        "{ternary}"
    );

    // translated/ForwardSlashRegexTests.swift:1873. A generic prefix
    // operator may be split from the following regex literal.
    let prefix = parser.parse("_ = ^^/0xG/").unwrap();
    let prefix_expression =
        first_descendant_named(&prefix.top_node(), "PrefixOperatorExpression").unwrap();
    assert_eq!(
        direct_child_names(&prefix_expression),
        ["PrefixOperator", "RegexLiteralExpression"],
        "{prefix}"
    );
}

#[test]
fn string_literals_follow_swiftsyntax_raw_multiline_and_interpolation_boundaries() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let positive_cases = [
        ("MultilineStringTests.swift:329", "_ = \"\"\"\n\n    \"\"\""),
        (
            "MultilineStringTests.swift:360",
            "_ = \"hello\\(\"\"\"\n  world\n  \"\"\")\"",
        ),
        (
            "MultilineStringTests.swift:490",
            "#\"\"\"\nThree \\#\nGamma\n\"\"\"#",
        ),
        (
            "MultilineStringTests.swift:393",
            concat!(
                "_ = \"\"\"\n",
                "    welcome\n",
                "    \\(\n",
                "      /*\n",
                "        ')' or '\"\"\"' in comment.\n",
                "        \"\"\"\n",
                "      */\n",
                "      \"to\\(\"\"\"\n",
                "           Swift\n",
                "           \"\"\")\"\n",
                "      // ) or \"\"\" in comment.\n",
                "    )\n",
                "    !\n",
                "    \"\"\"",
            ),
        ),
        ("RawStringTests.swift:115", "_ = #\"\"Zeta\"\"#"),
        (
            "RawStringTests.swift:236",
            "_ = \"interpolating \\(#\"\"\"false delimiter\"#)\"",
        ),
        (
            "RawStringTests.swift:254",
            "let foo = \"Interpolation\"\n_ = #\"\\b\\b \\#(foo)\\#(foo) Kappa\"#",
        ),
        ("RawStringTests.swift:165", "_ = #\"\u{200b}\"\u{200b}\"#"),
        (
            "RawStringTests.swift:291",
            "_ = #####\"This is a string\"#####",
        ),
        ("RawStringTests.swift:275", "#\"unused literal\"#"),
    ];

    for (origin, source) in positive_cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let expression = first_descendant_named(&tree.top_node(), "StringLiteralExpression")
            .unwrap_or_else(|| panic!("{origin}: {tree}"));
        assert_eq!(
            direct_child_names(&expression),
            ["StringLiteral"],
            "{origin}: {tree}"
        );
    }

    let mixed = parser.parse("_ = #/\"regex\"/#\n_ = #\"string\"#").unwrap();
    assert_eq!(
        named_node_count(&mixed.top_node(), "RegexLiteralExpression"),
        1,
        "{mixed}"
    );
    assert_eq!(
        named_node_count(&mixed.top_node(), "StringLiteralExpression"),
        1,
        "{mixed}"
    );
}

#[test]
fn generic_disambiguation_matches_upstream_choice() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in GENERIC_DISAMBIGUATION_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }

    let comparison = parser.parse("(a < b, c > d)").unwrap().to_string();
    assert!(comparison.contains("SequenceExpression"));
    assert!(!comparison.contains("GenericSpecializationExpression"));
    let generic = parser.parse("(a < b, c > (d))").unwrap().to_string();
    assert!(generic.contains("GenericSpecializationExpression"));

    let member = parser.parse("A<Int>.member").unwrap().to_string();
    assert!(
        member.contains("MemberAccessExpression(GenericSpecializationExpression"),
        "{member}"
    );
    assert!(!member.contains("SequenceExpression"), "{member}");
}

#[test]
fn empty_generic_arguments_follow_the_swiftsyntax_token_boundary() {
    // SwiftSyntax 60e8eb850721, VariadicGenericsTests.swift:539. Its generic
    // argument parser accepts an empty list, while expression disambiguation
    // explicitly excludes an immediately adjacent `<>` operator token.
    let source = "let _: G< > = G()\n\
let _: G<repeat each T> = G()\n\
let _: G<Int, repeat each T> = G()\n\
let _ = G< >.self\n\
let _ = G<repeat each T>.self\n\
let _ = G<Int, repeat each T>.self";
    let parser = rezel_lang_swift::parser().with_strict(true);
    let tree = parser.parse(source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(source).unwrap();
        panic!("VariadicGenericsTests.swift:539: {error}\n{source}\n{recovered}");
    });
    assert_eq!(
        named_node_count(&tree.top_node(), "GenericArgumentClause"),
        6,
        "{tree}"
    );
    let empty = first_descendant_named(&tree.top_node(), "GenericArgumentClause").unwrap();
    assert_eq!(
        empty.children_by_name("GenericArgument").len(),
        0,
        "{empty}"
    );

    let compact_type = parser.parse("let _: G<> = G()").unwrap();
    let compact_type_clause =
        first_descendant_named(&compact_type.top_node(), "GenericArgumentClause").unwrap();
    assert_eq!(
        compact_type_clause
            .children_by_name("GenericArgument")
            .len(),
        0,
        "{compact_type}"
    );
    let compact_expression = parser.parse("let _ = G<>.self").unwrap();
    assert_eq!(
        named_node_count(&compact_expression.top_node(), "GenericArgumentClause"),
        0,
        "{compact_expression}"
    );
    assert_eq!(
        named_node_count(&compact_expression.top_node(), "PostfixOperatorExpression"),
        1,
        "{compact_expression}"
    );

    let commented = parser.parse("let _ = G</*empty*/>.self").unwrap();
    let commented_clause =
        first_descendant_named(&commented.top_node(), "GenericArgumentClause").unwrap();
    assert_eq!(
        commented_clause.children_by_name("GenericArgument").len(),
        0,
        "{commented}"
    );
    assert_eq!(
        named_node_count(&commented_clause, "BlockComment"),
        1,
        "{commented}"
    );
}

#[test]
fn tuple_type_and_parameter_keyword_labels_are_identifiers() {
    // SwiftSyntax 60e8eb850721, Sources/SwiftParser/Expressions.swift:232.
    // Tuple type elements and declaration parameters both use
    // `parseArgumentLabel`, which remaps a lexer keyword to an identifier.
    let source = "mutating func parseUnresolvedAsExpr(\n  handle: TokenConsumptionHandle\n) -> (operator: RawExprSyntax, rhs: RawExprSyntax) {}";
    let parser = rezel_lang_swift::parser().with_strict(true);
    let tree = parser.parse(source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(source).unwrap();
        panic!("{error}\n{source}\n{recovered}")
    });
    assert_eq!(
        named_node_count(&tree.top_node(), "TupleTypeElementLabel"),
        2,
        "{tree}"
    );
    let first_label = first_descendant_named(&tree.top_node(), "TupleTypeElementLabel").unwrap();
    assert_eq!(
        direct_child_names(&first_label),
        ["Identifier", ":"],
        "{tree}"
    );

    let parameter = parser.parse("func f(operator: Int) {}").unwrap();
    let names = first_descendant_named(&parameter.top_node(), "ParameterNames").unwrap();
    assert_eq!(direct_child_names(&names), ["Identifier"], "{parameter}");

    assert!(parser.parse("func f($0: Int) {}").is_err());
    assert!(parser.parse("let x: ($0: Int, rhs: Int)").is_err());
}

#[test]
fn generic_disambiguation_stays_bounded_on_repeated_angles() {
    let limits = ParseLimits {
        max_actions: 50_000,
        max_stacks: 16,
        max_stack_depth: 1_024,
        max_buffer_records: 20_000,
        max_recovery_actions: 0,
    };
    let parser = rezel_lang_swift::parser()
        .with_strict(true)
        .with_limits(limits);

    let mut comparisons = String::from("if value");
    for _ in 0..128 {
        comparisons.push_str(" < value");
    }
    comparisons.push_str(" {}");
    parser.parse(&comparisons).unwrap();

    let mut generics = String::from("A");
    for _ in 0..64 {
        generics.push_str("<A");
    }
    for _ in 0..64 {
        generics.push('>');
    }
    generics.push_str("()");
    parser.parse(&generics).unwrap();
}

#[test]
fn newline_continuations_remain_bounded_at_eof_and_during_recovery() {
    let line_break = DECLARATION_CASES
        .iter()
        .find_map(|(origin, source)| (*origin == "line 352").then_some(*source))
        .expect("SwiftSyntax DeclarationTests.swift:352 fixture");
    let postfix_if_config = IF_CONFIG_CASES
        .iter()
        .find_map(|(origin, source)| (*origin == "IfconfigExprTests.swift:206").then_some(*source))
        .expect("SwiftSyntax IfconfigExprTests.swift:206 fixture");
    let generic_requirements = "protocol P {\n  associatedtype UTF8View: /*Bidirectional*/Collection\n  where UTF8View.Element == UInt8, // Unicode.UTF8.CodeUnit\n        UTF8View.Index == Index\n}";
    let cases = [
        ("ordinary code-item line break", line_break, "value\nlet ="),
        (
            "function effect after a line break",
            "func resolve<Act>(_ name: String)\n  throws -> Act? where Act: AnyObject {}",
            "func f()\nthrows ->",
        ),
        (
            "enum case parameter after a line break",
            "enum E {\n  case a\n    (Int)\n}",
            "enum E { case a\n(",
        ),
        (
            "postfix conditional-compilation suffix",
            postfix_if_config,
            "func f(base: S) {\n  base\n#if FLAG\n    .member()",
        ),
        (
            "generic requirement after a comma and line break",
            generic_requirements,
            "protocol P {\n  associatedtype View: Collection\n  where View.Element == UInt8,\n",
        ),
    ];

    // First establish that the intentionally truncated EOF cases are ordinary
    // syntax errors under the unrestricted public parser. This prevents a
    // resource limit from accidentally becoming their baseline behavior.
    let default_strict = rezel_lang_swift::parser().with_strict(true);
    for &(name, _, truncated) in &cases {
        let error = default_strict
            .parse(truncated)
            .expect_err("the incomplete EOF form must not parse strictly");
        assert_eq!(error.kind(), ParseErrorKind::Syntax, "{name}: {error}");
    }

    // These uniform ceilings are far below the defaults while leaving room
    // for the pinned SwiftSyntax fixtures. They make a non-progressing
    // zero-width continuation fail as a resource limit rather than spin.
    let strict_limits = ParseLimits {
        max_actions: 1_024,
        max_stacks: 16,
        max_stack_depth: 1_024,
        max_buffer_records: 20_000,
        max_recovery_actions: 0,
    };
    let strict = rezel_lang_swift::parser()
        .with_strict(true)
        .with_limits(strict_limits);
    let recovering = rezel_lang_swift::parser().with_limits(ParseLimits {
        max_recovery_actions: 32,
        ..strict_limits
    });

    for &(name, accepted, truncated) in &cases {
        strict
            .parse(accepted)
            .unwrap_or_else(|error| panic!("{name}: accepted fixture failed: {error}\n{accepted}"));

        let error = strict
            .parse(truncated)
            .expect_err("the incomplete EOF form must not parse strictly");
        assert_eq!(error.kind(), ParseErrorKind::Syntax, "{name}: {error}");

        let first = recovering
            .parse(truncated)
            .unwrap_or_else(|error| panic!("{name}: recovery exceeded its budget: {error}"));
        let second = recovering
            .parse(truncated)
            .unwrap_or_else(|error| panic!("{name}: second recovery exceeded its budget: {error}"));
        assert!(
            contains_error(&first.top_node()),
            "{name}: recovery tree omitted an error\n{first}"
        );
        assert_eq!(first.to_string(), second.to_string(), "{name}");
    }
}

#[test]
fn accessor_blocks_are_selected_without_parallel_parse_stacks() {
    let limits = ParseLimits {
        max_actions: 50_000,
        max_stacks: 1,
        max_stack_depth: 1_024,
        max_buffer_records: 20_000,
        max_recovery_actions: 0,
    };
    let parser = rezel_lang_swift::parser()
        .with_strict(true)
        .with_limits(limits);

    for &(origin, source, expected_node) in ACCESSOR_DISAMBIGUATION_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let tree = tree.to_string();
        assert!(
            tree.contains(expected_node),
            "{origin}: expected {expected_node}\n{source}\n{tree}"
        );
        let rejected_node = if expected_node == "AccessorBlock" {
            "GetterCodeBlock"
        } else {
            "AccessorBlock"
        };
        assert!(
            !tree.contains(rejected_node),
            "{origin}: unexpectedly contained {rejected_node}\n{source}\n{tree}"
        );
    }
}

#[test]
fn accessor_specifiers_and_modifiers_match_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax 60e8eb850721, generated AccessorSpecifierOptions. Keeping
    // this complete list together catches drift between the grammar and the
    // accessor-block lookahead scanner, including the longest legacy names.
    for specifier in ACCESSOR_SPECIFIERS {
        let source = format!("var value: Int {{ {specifier} {{}} }}");
        let tree = parser.parse(&source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(&source).unwrap();
            panic!("{specifier}: {error}\n{source}\n{recovered}");
        });
        assert_eq!(
            named_node_count(&tree.top_node(), "AccessorDeclaration"),
            1,
            "{specifier}: {tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "AccessorSpecifier"),
            1,
            "{specifier}: {tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "GetterCodeBlock"),
            0,
            "{specifier}: {tree}"
        );
    }

    // SwiftSyntax DeclarationTests.swift:3448-3457 and 3535-3544.
    let coroutine = parser
        .parse("var i: Int {\n  yielding borrow { yield _i }\n  yielding mutate { yield &_i }\n}")
        .unwrap();
    assert_eq!(
        named_node_count(&coroutine.top_node(), "AccessorDeclaration"),
        2,
        "{coroutine}"
    );
    assert_eq!(
        named_node_count(&coroutine.top_node(), "DeclarationModifier"),
        2,
        "{coroutine}"
    );

    let modifiers = parser
        .parse(
            "var value: Int {\n  __consuming consuming borrowing mutating nonmutating yielding get { 0 }\n}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&modifiers.top_node(), "DeclarationModifier"),
        6,
        "{modifiers}"
    );

    // Swift 064859e41d68, UnsafePointer.swift:286-288. The attribute belongs
    // to the addressor, not to an expression in an implicit getter body.
    let addressor = parser
        .parse(
            "var pointee: Pointee {\n  @_transparent unsafeAddress {\n    return unsafe self\n  }\n}",
        )
        .unwrap();
    let accessor = first_descendant_named(&addressor.top_node(), "AccessorDeclaration").unwrap();
    assert!(accessor.child_by_name("Attribute").is_some(), "{addressor}");
    assert!(
        accessor.child_by_name("AccessorSpecifier").is_some(),
        "{addressor}"
    );
}

#[test]
fn strict_parser_accepts_multiline_operator_cases() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in MULTILINE_OPERATOR_CASES {
        parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
    }

    let prefixes = parser.parse(PREFIX_OPERATOR_LINES).unwrap().to_string();
    assert_eq!(prefixes.matches("CodeBlockItem").count(), 5);
    assert_eq!(prefixes.matches("PrefixOperatorExpression").count(), 4);

    // A reserved ampersand without right trivia remains a prefix token and
    // starts a new code item instead of continuing the preceding call.
    let prefix_ampersand = parser
        .parse("func f(_ value: inout Int) {\n  use()\n  &value\n}")
        .unwrap();
    let body = first_descendant_named(&prefix_ampersand.top_node(), "CodeBlock").unwrap();
    assert_eq!(body.children_by_name("CodeBlockItem").len(), 2);
    assert_eq!(named_node_count(&body, "InOutExpression"), 1);
}

#[test]
fn multiline_ternary_delimiters_continue_the_current_code_item() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        (
            "Swift 064859e41d68596f486c5d724401cb370f260409, ClosedRange.swift:229-231",
            "enum End { case pastEnd; case inRange(Int) }\nfunc closedRange(_ x: Int, _ upperBound: Int) -> End {\n  return x == upperBound\n    ? .pastEnd\n    : .inRange(x + 1)\n}",
        ),
        (
            "Swift 064859e41d68596f486c5d724401cb370f260409, CollectionAlgorithms.swift:467-469",
            "func collectionAlgorithms(_ range: Range<Int>, by predicate: (Int) throws -> Bool) rethrows -> Int {\n  return try predicate(range.lowerBound)\n    ? range.lowerBound\n    : range.upperBound\n}",
        ),
        (
            "trivia before both ternary delimiters",
            "func comments(_ condition: Bool) -> Int {\n  return condition\n    // before question\n    ? 1\n    /* before colon */\n    : 2\n}",
        ),
        (
            "SwiftSyntax TokenSpecSet.BinaryOperatorLike: infixQuestionMark before comment trivia",
            "func questionRightTrivia(_ condition: Bool) -> Int {\n  return condition\n    ?/* operator right trivia */ 1\n    : 2\n}",
        ),
        (
            "SwiftSyntax TokenSpecSet.BinaryOperatorLike: infixQuestionMark before line-comment trivia",
            "func questionLineComment(_ condition: Bool) -> Int {\n  return condition\n    ?// operator right trivia\n    1\n    : 2\n}",
        ),
        (
            "SwiftSyntax TokenSpecSet.BinaryOperatorLike: infixQuestionMark without a right bound",
            "func questionWithoutRightBound(_ condition: Bool) -> Int {\n  return condition\n    ?first\n    :second\n}",
        ),
    ];
    for (origin, source) in cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{recovered}");
        });
        let body = first_descendant_named(&tree.top_node(), "CodeBlock").unwrap();
        assert_eq!(body.children_by_name("CodeBlockItem").len(), 1, "{tree}");
        assert_eq!(named_node_count(&body, "TernaryExpression"), 1, "{tree}");
    }

    let nested = parser
        .parse("func nested(_ first: Bool, _ second: Bool) -> Int {\n  return first\n    ? 1\n    : second\n      ? 2\n      : 3\n}")
        .unwrap();
    let outer = first_descendant_named(&nested.top_node(), "TernaryExpression").unwrap();
    assert_eq!(named_node_count(&outer, "TernaryExpression"), 2);
    assert_eq!(outer.children_by_name("TernaryExpression").len(), 1);

    let separated = parser.parse("foo\nbar").unwrap();
    assert_eq!(
        separated.top_node().children_by_name("CodeBlockItem").len(),
        2
    );
}

#[test]
fn closing_delimiters_continue_multiline_expressions() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Pinned SwiftSyntax 60e8eb850721, Sources/SwiftParser/Types.swift:1473.
    let call = parser
        .parse(
            "func f() {\n  return .attribute(\n    parseAttribute(argumentMode: .required) { parser in\n      return parser.value()\n    }\n    /* outer call */\n  )\n}",
        )
        .unwrap();
    let body = first_descendant_named(&call.top_node(), "CodeBlock").unwrap();
    assert_eq!(body.children_by_name("CodeBlockItem").len(), 1, "{call}");
    assert_eq!(
        named_node_count(&body, "FunctionCallExpression"),
        3,
        "{call}"
    );

    let collection = parser
        .parse("let values = [\n  items.map { $0 }\n]")
        .unwrap();
    let expression =
        first_descendant_named(&collection.top_node(), "CollectionExpression").unwrap();
    assert_eq!(
        direct_child_names(&expression),
        ["[", "CollectionElement", "]"],
        "{collection}"
    );
}

#[test]
fn leading_commas_continue_lists_across_lines() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Swift 064859e41d68, StringUTF8Validation.swift:54-57. A comma at the
    // start of a physical line continues the preceding inheritance list.
    let official = parser
        .parse(
            "internal struct EncodingError: Error, Sendable, Hashable\n  , RawRepresentable {\n  var rawValue: UInt8\n}",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&official.top_node(), "InheritedType"),
        4,
        "{official}"
    );

    // Trivia before the comma does not own the continuation and remains in
    // the CST. This exercises the same general list boundary, not a protocol
    // name or declaration-specific exception.
    let commented = parser
        .parse("struct S: P\n  // continued inheritance\n  , Q {}")
        .unwrap();
    assert_eq!(
        named_node_count(&commented.top_node(), "InheritedType"),
        2,
        "{commented}"
    );
    assert_eq!(
        named_node_count(&commented.top_node(), "LineComment"),
        1,
        "{commented}"
    );

    // Swift 6.3.3 rejects an orphan comma after an ordinary expression. The
    // layout classifier only suppresses the line break; the grammar must
    // still prove that a comma is valid in the surrounding list.
    assert!(parser.parse("func invalid() {\n  value\n  ,\n}").is_err());
}

#[test]
fn postfix_chains_continue_after_trailing_closures() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Pinned SwiftSyntax 60e8eb850721, Sources/SwiftOperators/OperatorTable.swift:141.
    // SwiftSyntax's postfix loop accepts the dotted suffix at the start of a
    // line after the first trailing-closure call has been formed.
    let chained = parser
        .parse(
            "dict.sorted { $0.key < $1.key }\n  .map { $0.value.description }\n  .joined(separator: \"\\n\")",
        )
        .unwrap();
    assert_eq!(
        chained.top_node().children_by_name("CodeBlockItem").len(),
        1,
        "{chained}"
    );
    assert_eq!(
        named_node_count(&chained.top_node(), "FunctionCallExpression"),
        3,
        "{chained}"
    );
    assert_eq!(
        named_node_count(&chained.top_node(), "TrailingClosureClause"),
        2,
        "{chained}"
    );

    let commented = parser
        .parse("items.map { $0 }\n  /* continue the same postfix chain */ .count")
        .unwrap();
    assert_eq!(
        commented.top_node().children_by_name("CodeBlockItem").len(),
        1,
        "{commented}"
    );

    // A period-starting operator remains a separate prefix expression rather
    // than being mistaken for a member suffix.
    let range = parser.parse("foo()\n...bar").unwrap();
    assert_eq!(
        range.top_node().children_by_name("CodeBlockItem").len(),
        2,
        "{range}"
    );
}

#[test]
fn catch_where_omits_the_pattern_without_consuming_identifier_prefixes() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    // SwiftSyntax 60e8eb850721 materialized 00948, StatementTests.swift:152.
    let omitted = parser
        .parse("do {}\ncatch where (error as NSError) == NSError() {}")
        .unwrap();
    let first_catch = first_descendant_named(&omitted.top_node(), "CatchClause").unwrap();
    assert_eq!(
        direct_child_names(&first_catch),
        ["catch", "CatchItemList", "CodeBlock"],
        "{omitted}"
    );
    let omitted_item = first_catch_item(&first_catch);
    assert_eq!(
        direct_child_names(&omitted_item),
        ["WhereClause"],
        "{omitted}"
    );
    assert!(omitted_item.child_by_name("IdentifierPattern").is_none());

    // SwiftSyntax 60e8eb850721 materialized 01738, ErrorsTests.swift:552.
    let empty = parser
        .parse("do {\n} catch where true {\n  let error2 = error\n} catch {\n}")
        .unwrap();
    let do_expression = first_descendant_named(&empty.top_node(), "DoExpression").unwrap();
    let catches = do_expression.children_by_name("CatchClause");
    assert_eq!(catches.len(), 2, "{empty}");
    assert_eq!(
        direct_child_names(&catches[1]),
        ["catch", "CodeBlock"],
        "{empty}"
    );
    assert!(catches[1].child_by_name("CatchItemList").is_none());

    let patterned = parser
        .parse("do {} catch let error where condition {}")
        .unwrap();
    let patterned_catch = first_descendant_named(&patterned.top_node(), "CatchClause").unwrap();
    assert_eq!(
        direct_child_names(&first_catch_item(&patterned_catch)),
        ["ValueBindingPattern", "WhereClause"],
        "{patterned}"
    );

    let identifier = parser.parse("do {} catch whereValue {}").unwrap();
    let identifier_catch = first_descendant_named(&identifier.top_node(), "CatchClause").unwrap();
    assert_eq!(
        direct_child_names(&first_catch_item(&identifier_catch)),
        ["IdentifierPattern"],
        "{identifier}"
    );
}

#[test]
fn catch_items_use_the_matching_pattern_context() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Swift 064859e41d685, stdlib/public/core/NativeDictionary.swift:790.
    let enum_case = parser
        .parse("do {} catch _MergeError.keyCollision {}")
        .unwrap();
    let enum_item =
        first_catch_item(&first_descendant_named(&enum_case.top_node(), "CatchClause").unwrap());
    assert_eq!(
        direct_child_names(&enum_item),
        ["MemberAccessExpression"],
        "{enum_case}"
    );

    // SwiftSyntax 60e8eb850721, materialized DoExpressionTests.swift:171.
    let typed_binding = parser
        .parse(
            "do {\n  0\n  then 1\n} catch _ where true {\n  2\n} catch let err as Err {\n  3\n  4\n  then 5\n} catch {\n  then 6\n}",
        )
        .unwrap();
    let do_expression = first_descendant_named(&typed_binding.top_node(), "DoExpression").unwrap();
    let catches = do_expression.children_by_name("CatchClause");
    assert_eq!(catches.len(), 3, "{typed_binding}");
    assert_eq!(
        direct_child_names(&first_catch_item(&catches[0])),
        ["WildcardPattern", "WhereClause"],
        "{typed_binding}"
    );
    let binding_item = first_catch_item(&catches[1]);
    assert_eq!(
        direct_child_names(&binding_item),
        ["ValueBindingPattern"],
        "{typed_binding}"
    );
    assert_eq!(
        named_node_count(&binding_item, "CastOperator"),
        1,
        "{typed_binding}"
    );
    assert!(catches[2].child_by_name("CatchItemList").is_none());

    // SwiftSyntax 60e8eb850721, materialized ErrorsTests.swift:577.
    let is_type = parser
        .parse("do { throw opaque_error() } catch is Error {}")
        .unwrap();
    let is_item =
        first_catch_item(&first_descendant_named(&is_type.top_node(), "CatchClause").unwrap());
    assert_eq!(direct_child_names(&is_item), ["IsTypePattern"], "{is_type}");
}

#[test]
fn newline_else_stays_with_its_if_expression() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    // Swift 064859e41d68596f486c5d724401cb370f260409,
    // stdlib/public/core/RangeReplaceableCollection.swift:640-645.
    let tree = parser
        .parse(
            "func removeAll(_ keepCapacity: Bool) {\n  if !keepCapacity {\n    self = Self()\n  }\n  else {\n    replaceSubrange(startIndex..<endIndex, with: EmptyCollection())\n  }\n}",
        )
        .unwrap();
    let items = code_block_items(&tree.top_node());
    assert_eq!(items.len(), 1, "{tree}");
    let if_expression = items[0]
        .child_by_name("ExpressionStatement")
        .unwrap()
        .child_by_name("IfExpression")
        .unwrap();
    assert_eq!(
        direct_child_names(&if_expression),
        ["if", "ConditionList", "CodeBlock", "else", "CodeBlock"],
        "{tree}"
    );
}

#[test]
fn newline_catch_stays_with_its_do_expression() {
    // SwiftSyntax 60e8eb850721 parses a statement-position `do` as DoExprSyntax
    // when DoExpressions is enabled; StatementTests.swift:143-149 supplies the
    // multiline catch boundary.
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse("func doCatch() {\n  do {\n\n  }\n  catch {\n\n  }\n}")
        .unwrap();
    let items = code_block_items(&tree.top_node());
    assert_eq!(items.len(), 1, "{tree}");
    let expression_statement = items[0].child_by_name("ExpressionStatement").unwrap();
    let do_expression = expression_statement.child_by_name("DoExpression").unwrap();
    assert_eq!(
        direct_child_names(&do_expression),
        ["do", "CodeBlock", "CatchClause"],
        "{tree}"
    );
    let catch_clause = do_expression.child_by_name("CatchClause").unwrap();
    assert_eq!(
        direct_child_names(&catch_clause),
        ["catch", "CodeBlock"],
        "{tree}"
    );
}

#[test]
fn keyword_continuations_recurse_and_cross_comment_trivia() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let nested = parser
        .parse(
            "func choose(_ first: Bool, _ second: Bool) {\n  if first {}\n  else if second {}\n  else {}\n}",
        )
        .unwrap();
    let items = code_block_items(&nested.top_node());
    assert_eq!(items.len(), 1, "{nested}");
    let outer = items[0]
        .child_by_name("ExpressionStatement")
        .unwrap()
        .child_by_name("IfExpression")
        .unwrap();
    assert_eq!(
        direct_child_names(&outer),
        ["if", "ConditionList", "CodeBlock", "else", "IfExpression"],
        "{nested}"
    );
    let inner = outer.child_by_name("IfExpression").unwrap();
    assert_eq!(
        direct_child_names(&inner),
        ["if", "ConditionList", "CodeBlock", "else", "CodeBlock"],
        "{nested}"
    );

    let comments = parser
        .parse(
            "func comments(_ condition: Bool) {\n  if condition {}\n  /* before else */\n  else {}\n  do {}\n  // before catch\n  catch {}\n}",
        )
        .unwrap();
    let comments_items = code_block_items(&comments.top_node());
    assert_eq!(comments_items.len(), 2, "{comments}");
    let comment_if = first_descendant_named(&comments_items[0], "IfExpression").unwrap();
    assert_eq!(
        direct_child_names(&comment_if),
        [
            "if",
            "ConditionList",
            "CodeBlock",
            "BlockComment",
            "else",
            "CodeBlock"
        ],
        "{comments}"
    );
    let comment_do = first_descendant_named(&comments_items[1], "DoExpression").unwrap();
    assert_eq!(
        direct_child_names(&comment_do),
        ["do", "CodeBlock", "LineComment", "CatchClause"],
        "{comments}"
    );
}

#[test]
fn keyword_continuation_prefixes_remain_separate_code_items() {
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(
            "func guards(_ condition: Bool) {\n  if condition {}\n  elsewhere\n  do {}\n  catching\n}",
        )
        .unwrap();
    let items = code_block_items(&tree.top_node());
    assert_eq!(items.len(), 4, "{tree}");
    for (item, expected) in items.iter().zip([
        "ExpressionStatement",
        "ExpressionStatement",
        "ExpressionStatement",
        "ExpressionStatement",
    ]) {
        assert_eq!(direct_child_names(item), [expected], "{tree}");
    }
}

#[test]
fn closure_bodies_do_not_become_shorthand_signatures() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in CLOSURE_BODY_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        assert!(!tree.to_string().contains("ClosureSignature"));
    }

    let signature = parser
        .parse(SHORTHAND_CLOSURE_SIGNATURE)
        .unwrap()
        .to_string();
    assert!(signature.contains("ClosureSignature"));
    assert!(signature.contains("ClosureShorthandParameter"));
}

#[test]
fn closure_capture_lists_may_be_empty_or_nonempty() {
    // SwiftSyntax 60e8eb850721, TypeTests.swift:71. Current Swift accepts an
    // empty capture list even though the older TSPL and ANTLR productions
    // still spell `capture-list-items` as nonempty.
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source, captures) in [
        (
            "TypeTests.swift:71",
            "simple { [] str in\n  print(str)\n}",
            0,
        ),
        ("TSPL nonempty capture", "simple { [value] in value }", 1),
        (
            "TSPL trailing capture comma",
            "simple { [weak self, value = initial,] in value }",
            2,
        ),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let capture = first_descendant_named(&tree.top_node(), "ClosureCaptureClause").unwrap();
        assert_eq!(
            capture.children_by_name("ClosureCaptureItem").len(),
            captures,
            "{origin}: {tree}"
        );
    }
}

#[test]
fn named_opaque_types_are_owned_by_result_contexts() {
    // SwiftSyntax 60e8eb850721, TypeTests.swift:171. Named opaque types are
    // parsed by `parseResultType`, not by the ordinary type grammar. Swift
    // 6.3.3's frontend rejects this older syntax, so it deliberately remains
    // a pinned SwiftSyntax parser case rather than a frontend-oracle fixture.
    let source = "func f2() -> <T: SignedInteger, U: SignedInteger> Int {\n}\n\
dynamic func lazyMapCollection<C: Collection, T>(_ collection: C, body: @escaping (C.Element) -> T)\n\
    -> <R: Collection where R.Element == T> R {\n  return collection.lazy.map { body($0) }\n}\n\
struct Boom<T: P> {\n  var prop1: Int = 5\n  var prop2: <U, V> (U, V) = (\"hello\", 5)\n}";
    let parser = rezel_lang_swift::parser().with_strict(true);
    let tree = parser.parse(source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(source).unwrap();
        panic!("TypeTests.swift:171: {error}\n{source}\n{recovered}");
    });
    assert_eq!(
        named_node_count(&tree.top_node(), "NamedOpaqueReturnType"),
        3,
        "{tree}"
    );
    for owner_name in ["ReturnClause", "TypeAnnotation"] {
        let owner =
            first_descendant_with_child(&tree.top_node(), owner_name, "NamedOpaqueReturnType")
                .unwrap_or_else(|| {
                    panic!("{owner_name} must directly own the named opaque type: {tree}")
                });
        assert!(
            owner.child_by_name("NamedOpaqueReturnType").is_some(),
            "{owner}"
        );
    }
    let named = first_descendant_named(&tree.top_node(), "NamedOpaqueReturnType").unwrap();
    assert!(named.node_type().is_name("Type"), "{named}");
    assert!(
        named.child_by_name("GenericParameterClause").is_some(),
        "{named}"
    );
    assert_eq!(
        named
            .children()
            .filter(|child| child.node_type().is_name("Type"))
            .count(),
        1,
        "{named}"
    );

    let ordinary = parser.parse("let value: G<T> = source").unwrap();
    assert_eq!(
        named_node_count(&ordinary.top_node(), "NamedOpaqueReturnType"),
        0,
        "{ordinary}"
    );

    // Initializers reuse SwiftSyntax's function-signature result path.
    let initializer = parser.parse("struct S { init() -> <T> T {} }").unwrap();
    assert_eq!(
        named_node_count(&initializer.top_node(), "NamedOpaqueReturnType"),
        1,
        "{initializer}"
    );
}

#[test]
fn strict_parser_accepts_typed_closure_parameters() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source) in TYPED_CLOSURE_PARAMETER_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let tree = tree.to_string();
        assert!(tree.contains("ClosureParameter"), "{origin}: {tree}");
    }
}

#[test]
fn closure_effects_and_returns_follow_both_parameter_clause_forms() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source, parameter_kind, effects, returns) in CLOSURE_SIGNATURE_EFFECT_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let signature = first_descendant_named(&tree.top_node(), "ClosureSignature").unwrap();
        assert!(
            first_descendant_named(&signature, parameter_kind).is_some(),
            "{origin}: {tree}"
        );
        assert_eq!(
            named_node_count(&signature, "FunctionEffect"),
            effects,
            "{origin}: {tree}"
        );
        assert_eq!(
            named_node_count(&signature, "ReturnClause"),
            returns,
            "{origin}: {tree}"
        );
    }
}

#[test]
fn comma_lists_require_an_element_before_a_trailing_comma() {
    // TSPL permits an empty list or a nonempty list with an optional trailing
    // comma, but never a comma by itself. The pinned Swift 6.3.3 frontend
    // confirms the same boundary for calls, parameters, tuples, and types.
    let parser = rezel_lang_swift::parser().with_strict(true);
    parser
        .parse(
            "func f(_ value: Int,) {}\nf(value,)\nlet tuple = (value,)\nlet typed: (Int,) = (value,)",
        )
        .unwrap();

    for source in [
        "f(,)",
        "func f(,) {}",
        "let value = (,)",
        "let value: (,) = other",
    ] {
        assert!(parser.parse(source).is_err(), "{source}");
    }
}

#[test]
fn strict_parser_accepts_operator_reference_arguments() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source, operator) in OPERATOR_REFERENCE_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let tree = tree.to_string();
        let reference = format!("DeclReferenceExpression(\"{operator}\")");
        assert!(tree.contains(&reference), "{origin}: {tree}");
    }
}

#[test]
fn range_operators_follow_upstream_fixity() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source, expected_node) in RANGE_OPERATOR_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let tree = tree.to_string();
        assert!(tree.contains(expected_node), "{origin}: {tree}");
    }
}

#[test]
fn tuple_members_follow_upstream_lexical_boundaries() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source, expected_members) in TUPLE_MEMBER_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let tree = tree.to_string();
        assert_eq!(
            tree.matches("MemberAccessExpression").count(),
            expected_members,
            "{origin}: {tree}"
        );
    }

    let (origin, source) = INVALID_IMPLICIT_TUPLE_MEMBER;
    assert!(parser.parse(source).is_err(), "{origin}: {source}");
}

#[test]
fn numeric_literal_underscores_follow_the_swift_lexer() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // Swift 064859e41d68: UTF16.swift:340, UTF8.swift:83, Unicode.swift:299.
    let official = parser
        .parse(
            "let values = (0b0__111_1111, 0b0_______________________11_1111__0000_0000, 0b11_00__0000)",
        )
        .unwrap();
    assert_eq!(
        named_node_count(&official.top_node(), "IntegerLiteralExpression"),
        3,
        "{official}"
    );

    // SwiftSyntax's lexer consumes `[digit-or-underscore]*` after the first
    // radix-valid digit. Cover every shared token helper with the smallest
    // matrix that distinguishes this from Java-style separator rules.
    let matrix = parser
        .parse("let values = (1__2_, 0o7__0_, 0xF__0_, 1__2.3__4_, 0xF__0.A__Bp1__0_, tuple.0__1)")
        .unwrap();
    assert_eq!(
        named_node_count(&matrix.top_node(), "IntegerLiteralExpression"),
        3,
        "{matrix}"
    );
    assert_eq!(
        named_node_count(&matrix.top_node(), "FloatLiteralExpression"),
        2,
        "{matrix}"
    );
    assert_eq!(
        named_node_count(&matrix.top_node(), "MemberAccessExpression"),
        1,
        "{matrix}"
    );

    for source in ["let value = 0b_1", "let value = 0o_7", "let value = 0x_F"] {
        assert!(
            parser.parse(source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn ampersand_and_bitwise_prefix_operators_follow_upstream_fixity() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for &(origin, source, expected_node) in AMPERSAND_OPERATOR_CASES {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let tree = tree.to_string();
        assert!(tree.contains(expected_node), "{origin}: {tree}");
    }

    let compact_binary = parser.parse("lhs&rhs").unwrap().to_string();
    assert!(compact_binary.contains("SequenceExpression"));
    for source in ["lhs &rhs", "lhs& rhs", "~ value"] {
        assert!(
            parser.parse(source).is_err(),
            "unexpectedly accepted {source}"
        );
    }
}

#[test]
fn scoped_import_module_selectors_preserve_import_path_components() {
    // SwiftSyntax 60e8eb850721, ModuleSelectorTests.swift:21.
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse("import struct ModuleSelectorTestingKit::A")
        .unwrap();
    let path = first_descendant_named(&tree.top_node(), "ImportPath").unwrap();
    let components = path.children_by_name("ImportPathComponent");
    assert_eq!(components.len(), 2, "{tree}");
    assert_eq!(
        direct_child_names(&components[0]),
        ["Identifier", "::"],
        "{tree}"
    );
    assert_eq!(direct_child_names(&components[1]), ["Identifier"], "{tree}");
    assert_eq!(named_node_count(&path, "ModuleSelector"), 0, "{tree}");
}

#[test]
fn module_selectors_are_owned_by_types_references_macros_and_attributes() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source, owner_name) in [
        (
            "ModuleSelectorTests.swift:89",
            "extension ModuleSelectorTestingKit::A {}",
            "IdentifierType",
        ),
        (
            "ModuleSelectorTests.swift:427",
            "_ = Swift::print",
            "DeclReferenceExpression",
        ),
        (
            "ModuleSelectorTests.swift:1005",
            "_ = #main::myMacro()",
            "MacroExpansionExpression",
        ),
        (
            "ModuleSelectorTests.swift:388",
            "@main::available(foo: bar) var use3",
            "Attribute",
        ),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let owner = first_descendant_named(&tree.top_node(), owner_name).unwrap();
        let selector = owner.child_by_name("ModuleSelector").unwrap();
        assert_module_selector(&selector);
        if owner_name == "Attribute" {
            assert_eq!(
                direct_child_names(&owner),
                [
                    "@",
                    "ModuleSelector",
                    "AttributeName",
                    "AttributeArgumentClause"
                ],
                "{tree}"
            );
            assert_eq!(
                direct_child_names(&owner.child_by_name("AttributeName").unwrap()),
                ["Identifier"],
                "{tree}"
            );
        }
    }
}

#[test]
fn module_selectors_recur_in_member_operator_and_key_path_names() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let member = parser.parse("_ = x.Swift::y").unwrap();
    let access = first_descendant_named(&member.top_node(), "MemberAccessExpression").unwrap();
    let reference = member_decl_reference(&access);
    assert_module_selector(&reference.child_by_name("ModuleSelector").unwrap());

    let operator = parser.parse("_ = myArray.reduce(0, Swift::+)").unwrap();
    assert_eq!(
        named_node_count(&operator.top_node(), "ModuleSelector"),
        1,
        "{operator}"
    );

    let key_path = strict_key_path(r"\main::Foo.BarKit::bar");
    assert_eq!(
        named_node_count(&key_path, "ModuleSelector"),
        2,
        "{key_path}"
    );
    let root = key_path.child_by_name("IdentifierType").unwrap();
    assert_module_selector(&root.child_by_name("ModuleSelector").unwrap());
}

#[test]
fn module_selector_trivia_and_qualified_keywords_follow_swiftsyntax() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for source in [
        "_ = Swift:: print",
        "_ = Swift ::print",
        "_ = Swift :: print",
        "_ = Swift\n::print",
        "_ = Swift\n:: print",
        "_ = Swift::nil",
        "_ = Swift::self",
        "_ = Swift::Self",
        "_ = Swift::Any",
        "_ = Swift::init",
        "_ = Swift::$foo",
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}");
        });
        assert_eq!(
            named_node_count(&tree.top_node(), "ModuleSelector"),
            1,
            "{tree}"
        );
    }
}

#[test]
fn module_selectors_in_attribute_arguments_keep_their_cst_role() {
    // SwiftSyntax 60e8eb850721, ModuleSelectorTests.swift:185.
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(
            "@_dynamicReplacement(for: ModuleSelectorTestingKit::negate())\nmutating func myNegate() {}",
        )
        .unwrap();
    let argument =
        first_descendant_with_child(&tree.top_node(), "AttributeArgument", "ModuleSelector")
            .unwrap();
    assert_module_selector(&argument.child_by_name("ModuleSelector").unwrap());

    // SwiftSyntax AttributeTests.swift:186 uses adjacent colons as an
    // Objective-C selector, not as a module selector.
    let objc_source = "@objc(:::x::)\nfunc f(_: Int, _: Int, _: Int, _: Int, _: Int) {}";
    let objc = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(objc_source)
        .unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(objc_source).unwrap();
            panic!("{error}\n{objc_source}\n{recovered}");
        });
    assert_eq!(
        named_node_count(&objc.top_node(), "ModuleSelector"),
        0,
        "{objc}"
    );
}

#[test]
fn custom_attribute_arguments_reuse_labeled_expressions() {
    // SwiftSyntax 60e8eb850721, AttributeTests.swift:1404-1420.
    let lifetime_source = r"
struct NE: ~Escapable {}

@lifetime(ne)
func derive1(ne: NE) -> NE { ne }

@lifetime(borrow ne)
func derive2(ne: borrowing NE) -> NE { ne }

@lifetime(ne1, n2)
func derive3(ne1: NE, ne2: NE) -> NE { ne1 }

@lifetime(borrow ne1, n2)
func derive4(ne1: NE, ne2: NE) -> NE { ne1 }

@lifetime(neOut: ne)
func derive5(ne: NE, neOut: inout NE) -> NE { neOut = ne }

@lifetime(neOut: borrow ne)
func derive6(ne: borrowing NE, neOut: inout NE) -> NE { neOut = ne }
";
    let lifetime = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(lifetime_source)
        .unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(lifetime_source).unwrap();
            panic!("{error}\n{lifetime_source}\n{recovered}");
        });
    assert_eq!(
        named_node_count(&lifetime.top_node(), "LabeledExpression"),
        8,
        "{lifetime}"
    );
    assert_eq!(
        named_node_count(&lifetime.top_node(), "BorrowExpression"),
        3,
        "{lifetime}"
    );

    // Accepted Swift stdlib forms cover the remaining prefix-expression and
    // labeled-expression boundaries used by the lifetime attribute.
    let stdlib_source = r"
@lifetime(&source)
func inherit(source: inout Int) {}

@lifetime(self: copy self)
func append() {}
";
    let stdlib = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(stdlib_source)
        .unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(stdlib_source).unwrap();
            panic!("{error}\n{stdlib_source}\n{recovered}");
        });
    assert_eq!(
        named_node_count(&stdlib.top_node(), "InOutExpression"),
        1,
        "{stdlib}"
    );
    assert_eq!(
        named_node_count(&stdlib.top_node(), "CopyExpression"),
        1,
        "{stdlib}"
    );

    // Compiler-known special syntax remains on its separate argument grammar,
    // while a module-selected name is always a custom attribute in SwiftSyntax.
    let split_source = r"
@available(iOS 13.0, *)
func builtin() {}
@main::available(foo: bar)
func custom() {}
";
    let split = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(split_source)
        .unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(split_source).unwrap();
            panic!("{error}\n{split_source}\n{recovered}");
        });
    assert_eq!(
        named_node_count(&split.top_node(), "LabeledExpression"),
        1,
        "{split}"
    );
    assert_eq!(
        named_node_count(&split.top_node(), "ModuleSelector"),
        1,
        "{split}"
    );

    // SwiftSyntax TrailingCommaTests.swift:79-81 exercises both declaration
    // and type-position custom attributes through the same expression list.
    let trailing = rezel_lang_swift::parser()
        .with_strict(true)
        .parse("@Foo(a, b, c,) struct S {}\nfunc f(_: @foo(1, 2,) Int) {}")
        .unwrap();
    assert_eq!(
        named_node_count(&trailing.top_node(), "LabeledExpression"),
        5,
        "{trailing}"
    );
}

#[test]
fn trailing_condition_commas_follow_the_official_body_boundary() {
    // SwiftSyntax 60e8eb850721, TrailingCommaTests.swift:119-301. Its
    // `atStartOfConditionalStatementBody` lookahead distinguishes the final
    // body from a closure-valued condition after a comma.
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        ("TrailingCommaTests.swift:119", "if true, { }", 1),
        ("TrailingCommaTests.swift:121", "if true, { }; { }()", 1),
        (
            "TrailingCommaTests.swift:123",
            "if true, { print(\"if-body\") } else { print(\"else-body\") }",
            1,
        ),
        (
            "TrailingCommaTests.swift:185",
            "if true, { print(0) }\n{ }()",
            1,
        ),
        (
            "TrailingCommaTests.swift:130",
            "if true, { print(\"if-body\") } else if true, { print(\"else-if-body\") } { print(\"else-body\") }",
            3,
        ),
        (
            "TrailingCommaTests.swift:137",
            "if true, { if true { { } } }",
            2,
        ),
        (
            "TrailingCommaTests.swift:139",
            "{ if true, { print(0) } }",
            1,
        ),
        (
            "TrailingCommaTests.swift:141",
            "( if true, { print(0) } )",
            1,
        ),
        (
            "TrailingCommaTests.swift:157",
            "if true, { true }, { print(0) }",
            2,
        ),
        (
            "TrailingCommaTests.swift:213",
            "if true, { true }\n,{ print(0) }",
            2,
        ),
        (
            "TrailingCommaTests.swift:283",
            "guard true, else { break }",
            1,
        ),
        (
            "TrailingCommaTests.swift:301",
            "while true, { print(0) }",
            1,
        ),
    ];

    for (origin, source, conditions) in cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}")
        });
        assert_eq!(
            named_node_count(&tree.top_node(), "ConditionElement"),
            conditions,
            "{origin}: {tree}"
        );
    }
}

#[test]
fn statement_condition_trailing_closures_follow_the_official_delimiter_boundary() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    let cases = [
        ("StatementTests.swift:1063", "if test { x in\n  x\n} {}"),
        (
            "Sources/SwiftParser/Expressions.swift:136 and Patterns.swift:75",
            "switch value { case _ where parser.withLookahead { $0.matches(value) }: break }",
        ),
    ];

    for (origin, source) in cases {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}")
        });
        let call = first_descendant_with_child(
            &tree.top_node(),
            "FunctionCallExpression",
            "TrailingClosureClause",
        )
        .unwrap_or_else(|| panic!("{origin}: {tree}"));
        assert_eq!(
            named_node_count(&call, "TrailingClosureClause"),
            1,
            "{origin}: {tree}"
        );
    }

    let body = parser.parse("if test { body }").unwrap();
    assert_eq!(
        named_node_count(&body.top_node(), "FunctionCallExpression"),
        0,
        "{body}"
    );
}

#[test]
fn wildcard_call_labels_accept_attributed_type_expressions() {
    // SwiftSyntax 60e8eb850721, TrailingCommaTests.swift:81. The comma is
    // already handled by the shared expression list; `_:` is the distinct
    // argument-label boundary in this exact source.
    let source = "f(_: @foo(1, 2,) Int)";
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(source)
        .unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}")
        });
    let label = first_descendant_named(&tree.top_node(), "ArgumentLabel").unwrap();
    assert_eq!(direct_child_names(&label), ["_"], "{tree}");
    assert_eq!(
        named_node_count(&tree.top_node(), "AttributedType"),
        1,
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "LabeledExpression"),
        3,
        "{tree}"
    );
}

#[test]
fn effects_attributes_preserve_opaque_token_payloads() {
    // SwiftSyntax 60e8eb850721, AttributeTests.swift:653-663. Its parser
    // deliberately leaves these payloads as tokens for SIL.
    let source = r"
@_effects(notEscaping self.value**)
func first() {}

@_effects(escaping self.value**.class*.value** => return.value**)
func second() {}
";
    let tree = rezel_lang_swift::parser()
        .with_strict(true)
        .parse(source)
        .unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{error}\n{source}\n{recovered}");
        });
    assert_eq!(
        named_node_count(&tree.top_node(), "EffectsAttributeArgumentList"),
        2,
        "{tree}"
    );
    assert_eq!(
        named_node_count(&tree.top_node(), "EffectsAttributeOperator"),
        5,
        "{tree}"
    );
    assert_eq!(named_node_count(&tree.top_node(), "*"), 1, "{tree}");
    for expression_node in [
        "AttributeArgumentClause",
        "LabeledExpression",
        "MemberAccessExpression",
        "SequenceExpression",
    ] {
        assert_eq!(
            named_node_count(&tree.top_node(), expression_node),
            0,
            "{tree}"
        );
    }

    // SwiftSyntax and the pinned Swift 6.3.3 frontend stop at the first right
    // parenthesis rather than balancing nested parentheses in this raw list.
    assert!(
        rezel_lang_swift::parser()
            .with_strict(true)
            .parse("@_effects(foo(bar))\nfunc nested() {}")
            .is_err()
    );
}

#[test]
fn specialize_attributes_preserve_labeled_and_where_arguments() {
    let parser = rezel_lang_swift::parser().with_strict(true);

    // SwiftSyntax 60e8eb850721, AttributeTests.swift:179 and
    // AvailabilityTests.swift:79.
    let availability_source = r"
@_specialize(exported: true, kind: full, availability: iOS, introduced: 15.4; where T == Swift.Int)
public func first<T>(_ value: T) {}

@_specialize(exported: true, availability: SwiftStdlib 5.1, *; where T == Int)
public func second<T>(_ value: T) {}
";
    let availability = parser.parse(availability_source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser()
            .parse(availability_source)
            .unwrap();
        panic!("{error}\n{availability_source}\n{recovered}");
    });
    assert_eq!(
        named_node_count(&availability.top_node(), "SpecializeAttributeArgumentList"),
        2,
        "{availability}"
    );
    assert_eq!(
        named_node_count(&availability.top_node(), "SpecializeAvailabilityArgument"),
        2,
        "{availability}"
    );
    assert_eq!(
        named_node_count(&availability.top_node(), "AvailabilityLabeledArgument"),
        1,
        "{availability}"
    );

    // SwiftSyntax DeclarationTests.swift:725, 796-797.
    let where_source = r"
@_specialize(where T == Int, U == Float)
func generic<T, U>() {}

@specialized(where Array<T> == Int)
func specialized<T>() {}
";
    let where_tree = parser.parse(where_source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(where_source).unwrap();
        panic!("{error}\n{where_source}\n{recovered}");
    });
    assert_eq!(
        named_node_count(&where_tree.top_node(), "GenericWhereClause"),
        2,
        "{where_tree}"
    );
    assert_eq!(
        named_node_count(&where_tree.top_node(), "SpecializedAttributeArgument"),
        1,
        "{where_tree}"
    );

    // SwiftSyntax AttributeTests.swift:108 and DeclarationTests.swift:782.
    // These cover the three layout-constraint arities without treating their
    // integer arguments as calls or general expressions.
    let layout_source = r"
@_specialize(where T: _Trivial, U: _Trivial(32), V: _TrivialAtMost(64, 8), W: _TrivialStride(16), X: _BridgeObject)
func layouts<T, U, V, W, X>() {}
";
    let layouts = parser.parse(layout_source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(layout_source).unwrap();
        panic!("{error}\n{layout_source}\n{recovered}");
    });
    assert_eq!(
        named_node_count(&layouts.top_node(), "LayoutRequirement"),
        5,
        "{layouts}"
    );
    assert_eq!(
        named_node_count(&layouts.top_node(), "FunctionCallExpression"),
        0,
        "{layouts}"
    );

    // Swift stdlib Prespecialize.swift:148-151 uses the compound declaration
    // name form. The parser also accepts SwiftSyntax's zero-argument form;
    // spi remains a labeled identifier rather than an expression.
    let target_source = r"
@_specialize(target: _appendElementAssumeUniqueAndCapacity(_:newElement:), spi: Private, where T == Swift::Int)
func targeted<T>() {}

@_specialize(target: _makeUniqueAndReserveCapacityIfNotUnique(), where T == Swift::Int)
func zeroArgumentTarget<T>() {}
";
    let target = parser.parse(target_source).unwrap_or_else(|error| {
        let recovered = rezel_lang_swift::parser().parse(target_source).unwrap();
        panic!("{error}\n{target_source}\n{recovered}");
    });
    assert_eq!(
        named_node_count(&target.top_node(), "SpecializeTargetFunctionArgument"),
        2,
        "{target}"
    );
    assert_eq!(
        named_node_count(&target.top_node(), "LabeledSpecializeArgument"),
        1,
        "{target}"
    );
    assert_eq!(
        named_node_count(&target.top_node(), "AttributeArgumentClause"),
        0,
        "{target}"
    );

    // Swift 6.3.3 diagnoses non-identifiers as SPI values. SwiftSyntax's
    // consumeAnyToken() here is recovery behavior, not the accepted grammar.
    assert!(
        parser
            .parse("@_specialize(spi: \"Private\", where T == Int)\nfunc invalid<T>() {}")
            .is_err()
    );
}

#[test]
fn module_selector_module_names_require_identifiers() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    // SwiftSyntax ModuleSelectorTests.swift:68 and 1136 reject these forms.
    for source in ["import ctypes::bits", "var value: self::Int"] {
        assert!(
            parser.parse(source).is_err(),
            "unexpectedly accepted {source}"
        );
    }

    let escaped = parser.parse("var value: `self`::Int").unwrap();
    let selector = first_descendant_named(&escaped.top_node(), "ModuleSelector").unwrap();
    assert_module_selector(&selector);
}

#[test]
fn macro_expansion_declarations_follow_code_item_context() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source, expected_children) in [
        (
            "DeclarationTests.swift:2031",
            "struct X { #memberwiseInit(access: .public) }",
            &["#", "Identifier", "ArgumentClause"][..],
        ),
        (
            "DeclarationTests.swift:2051",
            "struct X { #case }",
            &["#", "Identifier"],
        ),
        (
            "ModuleSelectorTests.swift:1005",
            "struct CreatesDeclExpectation { #main::myMacro() }",
            &["#", "ModuleSelector", "Identifier", "ArgumentClause"],
        ),
        (
            "TrailingCommaTests.swift:92",
            "struct S { #foo(1, 2,) }",
            &["#", "Identifier", "ArgumentClause"],
        ),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            let recovered = rezel_lang_swift::parser().parse(source).unwrap();
            panic!("{origin}: {error}\n{source}\n{recovered}");
        });
        let declaration =
            first_descendant_named(&tree.top_node(), "MacroExpansionDeclaration").unwrap();
        assert!(declaration.node_type().is_name("Declaration"), "{tree}");
        assert_eq!(
            direct_child_names(&declaration),
            expected_children,
            "{tree}"
        );
        if origin.starts_with("ModuleSelectorTests") {
            assert_module_selector(&declaration.child_by_name("ModuleSelector").unwrap());
        }
    }

    for (origin, source, prefix) in [
        (
            "DeclarationTests.swift:2066 attribute",
            "@attribute #topLevelWithAttr",
            "Attribute",
        ),
        (
            "DeclarationTests.swift:2066 modifier",
            "public #topLevelWithModifier",
            "DeclarationModifier",
        ),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            panic!("{origin}: {error}\n{source}");
        });
        let item = first_descendant_named(&tree.top_node(), "CodeBlockItem").unwrap();
        assert_eq!(
            direct_child_names(&item),
            [prefix, "MacroExpansionDeclaration"],
            "{tree}"
        );
    }

    // SwiftSyntax's code-item lookahead keeps an unadorned expansion as an
    // expression unless the surrounding member list requires a declaration.
    for source in ["#case", "func f() { #case }"] {
        let tree = parser.parse(source).unwrap();
        assert_eq!(
            named_node_count(&tree.top_node(), "MacroExpansionExpression"),
            1,
            "{tree}"
        );
        assert_eq!(
            named_node_count(&tree.top_node(), "MacroExpansionDeclaration"),
            0,
            "{tree}"
        );
    }
}

#[test]
fn pound_source_location_directives_remain_distinct_from_macros() {
    let parser = rezel_lang_swift::parser().with_strict(true);
    for (origin, source, expected_children) in [
        (
            "DirectiveTests.swift:91",
            "#sourceLocation()",
            &["#sourceLocation", "(", ")"][..],
        ),
        (
            "DirectiveTests.swift:97",
            "#sourceLocation(file: \"foo\", line: 42)",
            &["#sourceLocation", "(", "PoundSourceLocationArguments", ")"],
        ),
        (
            "DirectiveTests.swift:101",
            "class C { #sourceLocation(file: \"f.swift\", line: 1) }",
            &["#sourceLocation", "(", "PoundSourceLocationArguments", ")"],
        ),
    ] {
        let tree = parser.parse(source).unwrap_or_else(|error| {
            panic!("{origin}: {error}\n{source}");
        });
        let directive = first_descendant_named(&tree.top_node(), "PoundSourceLocation").unwrap();
        assert!(directive.node_type().is_name("Declaration"), "{tree}");
        assert_eq!(direct_child_names(&directive), expected_children, "{tree}");
        assert_eq!(
            named_node_count(&tree.top_node(), "MacroExpansionDeclaration"),
            0,
            "{tree}"
        );
        if let Some(arguments) = directive.child_by_name("PoundSourceLocationArguments") {
            assert_eq!(
                direct_child_names(&arguments),
                [
                    "file",
                    ":",
                    "StringLiteral",
                    ",",
                    "line",
                    ":",
                    "IntegerLiteral"
                ],
                "{tree}"
            );
        }
    }

    let prefixed = parser.parse("struct S { #sourceLocationExtra() }").unwrap();
    assert_eq!(
        named_node_count(&prefixed.top_node(), "PoundSourceLocation"),
        0,
        "{prefixed}"
    );
    assert_eq!(
        named_node_count(&prefixed.top_node(), "MacroExpansionDeclaration"),
        1,
        "{prefixed}"
    );
}
