import com.sun.source.tree.BlockTree;
import com.sun.source.tree.BreakTree;
import com.sun.source.tree.CaseTree;
import com.sun.source.tree.ClassTree;
import com.sun.source.tree.CompilationUnitTree;
import com.sun.source.tree.ContinueTree;
import com.sun.source.tree.IdentifierTree;
import com.sun.source.tree.ImportTree;
import com.sun.source.tree.LabeledStatementTree;
import com.sun.source.tree.LambdaExpressionTree;
import com.sun.source.tree.MemberReferenceTree;
import com.sun.source.tree.MemberSelectTree;
import com.sun.source.tree.MethodTree;
import com.sun.source.tree.ModifiersTree;
import com.sun.source.tree.ModuleTree;
import com.sun.source.tree.PrimitiveTypeTree;
import com.sun.source.tree.RequiresTree;
import com.sun.source.tree.Tree;
import com.sun.source.tree.TypeParameterTree;
import com.sun.source.tree.VariableTree;
import com.sun.source.util.JavacTask;
import com.sun.source.util.SourcePositions;
import com.sun.source.util.TreeScanner;
import com.sun.source.util.Trees;
import java.io.BufferedWriter;
import java.io.IOException;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayDeque;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.Comparator;
import java.util.IdentityHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import javax.lang.model.element.Modifier;
import javax.tools.Diagnostic;
import javax.tools.DiagnosticCollector;
import javax.tools.JavaCompiler;
import javax.tools.JavaFileObject;
import javax.tools.StandardJavaFileManager;
import javax.tools.ToolProvider;

public final class JavacReference {
    private static final int JAVA_RELEASE = 26;
    private static final String SCHEMA = "rezel.javac-java-reference-snapshot.v1";
    private static final String STDLIB_FINGERPRINT_SCHEMA =
            "rezel.javac-stdlib-ast-fingerprints.v1";
    private static final long FNV_PRIME = 0x0000_0100_0000_01b3L;
    private static final Path DEFAULT_ROOT = Path.of("tools/references/javac");

    private JavacReference() {}

    public static void main(String[] arguments) throws Exception {
        requireJavaRelease();
        if (arguments.length == 3 && arguments[0].equals("--stdlib-fingerprints")) {
            writeStdlibFingerprints(
                    Path.of(arguments[1]).toAbsolutePath().normalize(),
                    Path.of(arguments[2]).toAbsolutePath().normalize());
            return;
        }

        Config config = Config.parse(arguments);
        Snapshot snapshot = buildSnapshot(config.root());
        String rendered = render(snapshot);
        Path output = config.root().resolve("snapshots/java.json");
        if (config.update()) {
            Files.createDirectories(output.getParent());
            Files.writeString(output, rendered, StandardCharsets.UTF_8);
            return;
        }

        String checked = Files.readString(output, StandardCharsets.UTF_8);
        if (!checked.equals(rendered)) {
            throw new IllegalStateException(
                    "javac reference snapshot is stale; run JavacReference.java --update");
        }
    }

    private static void requireJavaRelease() {
        int actual = Runtime.version().feature();
        if (actual != JAVA_RELEASE) {
            throw new IllegalStateException(
                    "Java " + JAVA_RELEASE + " is required, found Java " + actual);
        }
    }

    private static void writeStdlibFingerprints(Path root, Path output) throws IOException {
        if (!Files.isDirectory(root)) {
            throw new IllegalArgumentException(
                    "Java source root does not exist: " + root);
        }
        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        if (compiler == null) {
            throw new IllegalStateException("a full JDK with javac is required");
        }
        List<Path> sources = javaSources(root);
        Files.createDirectories(output.getParent());
        try (BufferedWriter writer = Files.newBufferedWriter(output, StandardCharsets.UTF_8);
                StandardJavaFileManager files =
                        compiler.getStandardFileManager(
                                null, Locale.ROOT, StandardCharsets.UTF_8)) {
            writer.write("{\n\t\"schema\": ");
            writeString(writer, STDLIB_FINGERPRINT_SCHEMA);
            writer.write(",\n\t\"javaRuntimeVersion\": ");
            writeString(writer, Runtime.version().toString());
            writer.write(",\n\t\"coordinates\": \"raw-utf8-bytes\",");
            writer.write("\n\t\"sources\": [\n");
            for (int index = 0; index < sources.size(); index++) {
                Path path = sources.get(index);
                Fixture fixture = fixture(root, path);
                ParsedSource parsed = parse(compiler, files, fixture);
                String relative = fixturePath(root, path);
                writer.write("\t\t");
                appendStdlibFingerprint(writer, relative, parsed);
                if (index + 1 != sources.size()) {
                    writer.write(',');
                }
                writer.write('\n');
                if ((index + 1) % 1_000 == 0) {
                    System.err.println(
                            "javac AST fingerprints: "
                                    + (index + 1)
                                    + "/"
                                    + sources.size());
                }
            }
            writer.write("\t]\n}\n");
        }
    }

    private static List<Path> javaSources(Path root) throws IOException {
        try (var walk = Files.walk(root)) {
            return walk.filter(Files::isRegularFile)
                    .filter(path -> path.getFileName().toString().endsWith(".java"))
                    .sorted(Comparator.comparing(path -> fixturePath(root, path)))
                    .toList();
        }
    }

    private static void appendStdlibFingerprint(
            BufferedWriter output, String path, ParsedSource parsed) throws IOException {
        boolean accepted = parsed.errorCount() == 0 && parsed.units().size() == 1;
        StringBuilder record = new StringBuilder();
        record.append("{\"path\":");
        appendString(record, path);
        record.append(",\"accepted\":").append(accepted);
        record.append(",\"errorCount\":").append(parsed.errorCount());
        if (accepted) {
            AstFingerprint fingerprint = AstFingerprint.from(parsed.units().getFirst());
            record.append(",\"nodes\":").append(fingerprint.nodes());
            record.append(",\"fingerprint\":");
            appendString(record, fingerprint.hexadecimal());
        } else {
            record.append(",\"nodes\":0,\"fingerprint\":null");
        }
        record.append('}');
        output.write(record.toString());
    }

    private static void writeString(BufferedWriter output, String value) throws IOException {
        StringBuilder encoded = new StringBuilder();
        appendString(encoded, value);
        output.write(encoded.toString());
    }

    private static Snapshot buildSnapshot(Path root) throws IOException {
        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        if (compiler == null) {
            throw new IllegalStateException("a full JDK with javac is required");
        }

        Path fixtures = root.resolve("fixtures");
        List<AcceptedCase> accepted = new ArrayList<>();
        for (Fixture fixture : fixtures(fixtures.resolve("parse-accepted"))) {
            ParsedSource parsed = parse(compiler, fixture);
            if (parsed.errorCount() != 0) {
                throw new IllegalStateException(
                        fixture.id()
                                + " must be accepted by JavacTask.parse, but produced "
                                + parsed.errorCount()
                                + " ERROR diagnostics");
            }
            if (parsed.units().size() != 1) {
                throw new IllegalStateException(
                        fixture.id()
                                + " must produce one compilation unit, found "
                                + parsed.units().size());
            }
            accepted.add(new AcceptedCase(
                    fixture.id(),
                    fixture.sourceName(),
                    fixture.source(),
                    parsed.units().getFirst()));
        }

        List<RejectedCase> rejected = new ArrayList<>();
        for (Fixture fixture : fixtures(fixtures.resolve("parse-rejected"))) {
            ParsedSource parsed = parse(compiler, fixture);
            if (parsed.errorCount() == 0) {
                throw new IllegalStateException(
                        fixture.id()
                                + " must produce at least one ERROR diagnostic from "
                                + "JavacTask.parse");
            }
            rejected.add(
                    new RejectedCase(fixture.id(), fixture.sourceName(), fixture.source()));
        }

        if (accepted.isEmpty() || rejected.isEmpty()) {
            throw new IllegalStateException(
                    "both parse-accepted and parse-rejected fixtures are required");
        }
        requirePublicKindCoverage(accepted);
        return new Snapshot(List.copyOf(accepted), List.copyOf(rejected));
    }

    private static void requirePublicKindCoverage(List<AcceptedCase> accepted) {
        Set<String> expected = new TreeSet<>();
        for (Tree.Kind kind : Tree.Kind.values()) {
            if (kind != Tree.Kind.ERRONEOUS && kind != Tree.Kind.OTHER) {
                expected.add(kind.name());
            }
        }
        Set<String> covered = new TreeSet<>();
        for (AcceptedCase fixture : accepted) {
            collectKinds(fixture.tree(), covered);
        }
        if (!covered.equals(expected)) {
            Set<String> missing = new TreeSet<>(expected);
            missing.removeAll(covered);
            Set<String> unexpected = new TreeSet<>(covered);
            unexpected.removeAll(expected);
            throw new IllegalStateException(
                    "accepted fixtures do not cover the public Java "
                            + JAVA_RELEASE
                            + " Tree.Kind inventory; missing="
                            + missing
                            + ", unexpected="
                            + unexpected);
        }
    }

    private static void collectKinds(AstNode node, Set<String> output) {
        output.add(node.kind());
        for (AstEdge edge : node.children()) {
            collectKinds(edge.node(), output);
        }
    }

    private static List<Fixture> fixtures(Path directory) throws IOException {
        if (!Files.isDirectory(directory)) {
            throw new IllegalStateException("fixture directory does not exist: " + directory);
        }
        List<Path> paths;
        try (var walk = Files.walk(directory)) {
            paths =
                    walk.filter(Files::isRegularFile)
                            .filter(path -> path.getFileName().toString().endsWith(".java"))
                            .sorted(Comparator.comparing(path -> fixturePath(directory, path)))
                            .toList();
        }
        List<Fixture> fixtures = new ArrayList<>(paths.size());
        for (Path path : paths) {
            fixtures.add(fixture(directory, path));
        }
        return List.copyOf(fixtures);
    }

    private static Fixture fixture(Path root, Path path) throws IOException {
        byte[] bytes = Files.readAllBytes(path);
        String source = new String(bytes, StandardCharsets.UTF_8);
        String relative = fixturePath(root, path);
        if (!Arrays.equals(bytes, source.getBytes(StandardCharsets.UTF_8))) {
            throw new IllegalArgumentException(
                    "Java source is not canonical UTF-8: " + relative);
        }
        String id = relative.substring(0, relative.length() - ".java".length());
        return new Fixture(id, path.getFileName().toString(), path, source);
    }

    private static String fixturePath(Path directory, Path path) {
        return directory.relativize(path).toString().replace(path.getFileSystem().getSeparator(), "/");
    }

    private static ParsedSource parse(JavaCompiler compiler, Fixture fixture) throws IOException {
        try (StandardJavaFileManager files =
                compiler.getStandardFileManager(null, Locale.ROOT, StandardCharsets.UTF_8)) {
            return parse(compiler, files, fixture);
        }
    }

    private static ParsedSource parse(
            JavaCompiler compiler, StandardJavaFileManager files, Fixture fixture)
            throws IOException {
        DiagnosticCollector<JavaFileObject> diagnostics = new DiagnosticCollector<>();
        List<AstNode> units = new ArrayList<>();
        Iterable<? extends JavaFileObject> inputs =
                files.getJavaFileObjectsFromPaths(List.of(fixture.path()));
        JavacTask task =
                (JavacTask)
                        compiler.getTask(
                                null,
                                files,
                                diagnostics,
                                List.of(
                                        "-proc:none",
                                        "-encoding",
                                        "UTF-8",
                                        "--release",
                                        Integer.toString(JAVA_RELEASE)),
                                null,
                                inputs);
        Iterable<? extends CompilationUnitTree> parsed = task.parse();
        Trees trees = Trees.instance(task);
        SourcePositions positions = trees.getSourcePositions();
        Utf8Offsets offsets = new Utf8Offsets(fixture.source());
        for (CompilationUnitTree unit : parsed) {
            AstScanner scanner = new AstScanner(unit, positions, offsets);
            scanner.scan(unit, null);
            units.add(scanner.root());
        }
        long errorCount =
                diagnostics.getDiagnostics().stream()
                        .filter(diagnostic -> diagnostic.getKind() == Diagnostic.Kind.ERROR)
                        .count();
        return new ParsedSource(errorCount, List.copyOf(units));
    }

    private static final class AstScanner extends TreeScanner<Void, Void> {
        private final CompilationUnitTree unit;
        private final SourcePositions positions;
        private final Utf8Offsets offsets;
        private final ArrayDeque<Frame> ancestors = new ArrayDeque<>();
        private AstNode root;

        private AstScanner(
                CompilationUnitTree unit, SourcePositions positions, Utf8Offsets offsets) {
            this.unit = unit;
            this.positions = positions;
            this.offsets = offsets;
        }

        @Override
        public Void scan(Tree tree, Void unused) {
            if (tree == null) {
                return null;
            }

            long startUtf16 = positions.getStartPosition(unit, tree);
            long endUtf16 = positions.getEndPosition(unit, tree);
            AstNode node =
                    new AstNode(
                            tree.getKind().name(),
                            offsets.toUtf8(startUtf16),
                            offsets.toUtf8(endUtf16),
                            nameOf(tree),
                            modifiersOf(tree),
                            propertiesOf(tree),
                            new ArrayList<>());
            if (ancestors.isEmpty()) {
                if (root != null) {
                    throw new IllegalStateException("scanner produced more than one root");
                }
                root = node;
            } else {
                Frame parent = ancestors.peek();
                parent.node().children().add(new AstEdge(parent.childField(tree), node));
            }

            ancestors.push(new Frame(node, publicChildFields(tree)));
            try {
                return super.scan(tree, unused);
            } finally {
                ancestors.pop();
            }
        }

        private AstNode root() {
            if (root == null) {
                throw new IllegalStateException("scanner did not produce a root");
            }
            return root;
        }
    }

    private record Frame(AstNode node, IdentityHashMap<Tree, Set<String>> childFields) {
        private String childField(Tree child) {
            Set<String> fields = childFields.get(child);
            if (fields == null || fields.isEmpty()) {
                throw new IllegalStateException(
                        "TreeScanner visited "
                                + child.getKind()
                                + " outside the public child getters of its parent");
            }
            return String.join("|", fields);
        }
    }

    /*
     * TreeScanner defines child order. The public com.sun.source.tree
     * interfaces define child roles through their getters.
     */
    private static IdentityHashMap<Tree, Set<String>> publicChildFields(Tree tree) {
        IdentityHashMap<Tree, Set<String>> fields = new IdentityHashMap<>();
        Class<? extends Tree> treeInterface = tree.getKind().asInterface();
        if (treeInterface == null) {
            throw new IllegalStateException(
                    "no public Tree interface for kind " + tree.getKind());
        }
        List<Method> methods = new ArrayList<>(List.of(treeInterface.getMethods()));
        methods.sort(
                Comparator.comparing(Method::getName)
                        .thenComparing(method -> method.getDeclaringClass().getName()));
        for (Method method : methods) {
            if (!isPublicTreeGetter(method)) {
                continue;
            }
            Object value;
            try {
                value = method.invoke(tree);
            } catch (IllegalAccessException | InvocationTargetException exception) {
                throw new IllegalStateException(
                        "cannot read public Tree getter " + method, exception);
            }
            addChildValues(fields, value, getterField(method));
        }
        return fields;
    }

    private static boolean isPublicTreeGetter(Method method) {
        if (method.getParameterCount() != 0 || !method.getName().startsWith("get")) {
            return false;
        }
        if (!method.getDeclaringClass().getPackageName().equals("com.sun.source.tree")) {
            return false;
        }
        return !method.isAnnotationPresent(Deprecated.class);
    }

    private static String getterField(Method method) {
        String suffix = method.getName().substring("get".length());
        if (suffix.isEmpty()) {
            throw new IllegalStateException("empty Tree getter name: " + method);
        }
        int firstCodePoint = suffix.codePointAt(0);
        int firstLength = Character.charCount(firstCodePoint);
        return new StringBuilder()
                .appendCodePoint(Character.toLowerCase(firstCodePoint))
                .append(suffix.substring(firstLength))
                .toString();
    }

    private static void addChildValues(
            Map<Tree, Set<String>> fields, Object value, String field) {
        if (value instanceof Tree child) {
            fields.computeIfAbsent(child, ignored -> new TreeSet<>()).add(field);
            return;
        }
        if (value instanceof Iterable<?> children) {
            for (Object child : children) {
                addChildValues(fields, child, field);
            }
        }
    }

    private static String nameOf(Tree tree) {
        if (tree instanceof ClassTree declaration) {
            return declaration.getSimpleName().toString();
        }
        if (tree instanceof MethodTree declaration) {
            return declaration.getName().toString();
        }
        if (tree instanceof VariableTree declaration) {
            return declaration.getName().toString();
        }
        if (tree instanceof TypeParameterTree declaration) {
            return declaration.getName().toString();
        }
        if (tree instanceof IdentifierTree identifier) {
            return identifier.getName().toString();
        }
        if (tree instanceof MemberSelectTree selection) {
            return selection.getIdentifier().toString();
        }
        if (tree instanceof MemberReferenceTree reference) {
            return reference.getName().toString();
        }
        if (tree instanceof LabeledStatementTree statement) {
            return statement.getLabel().toString();
        }
        if (tree instanceof BreakTree statement && statement.getLabel() != null) {
            return statement.getLabel().toString();
        }
        if (tree instanceof ContinueTree statement && statement.getLabel() != null) {
            return statement.getLabel().toString();
        }
        return null;
    }

    private static List<String> modifiersOf(Tree tree) {
        ModifiersTree modifiers = null;
        if (tree instanceof ClassTree declaration) {
            modifiers = declaration.getModifiers();
        } else if (tree instanceof MethodTree declaration) {
            modifiers = declaration.getModifiers();
        } else if (tree instanceof VariableTree declaration) {
            modifiers = declaration.getModifiers();
        } else if (tree instanceof ModifiersTree direct) {
            modifiers = direct;
        }
        if (modifiers == null) {
            return List.of();
        }

        Set<String> normalized = new TreeSet<>();
        for (Modifier modifier : modifiers.getFlags()) {
            normalized.add(modifier.name());
        }
        return List.copyOf(normalized);
    }

    private static List<String> propertiesOf(Tree tree) {
        Set<String> properties = new TreeSet<>();
        if (tree instanceof ImportTree declaration) {
            properties.add("module=" + declaration.isModule());
            properties.add("static=" + declaration.isStatic());
        }
        if (tree instanceof BlockTree block) {
            properties.add("static=" + block.isStatic());
        }
        if (tree instanceof ModuleTree declaration) {
            properties.add("moduleKind=" + declaration.getModuleType().name());
        }
        if (tree instanceof RequiresTree directive) {
            properties.add("static=" + directive.isStatic());
            properties.add("transitive=" + directive.isTransitive());
        }
        if (tree instanceof PrimitiveTypeTree type) {
            properties.add("primitiveKind=" + type.getPrimitiveTypeKind().name());
        }
        if (tree instanceof MemberReferenceTree reference) {
            properties.add("referenceMode=" + reference.getMode().name());
        }
        if (tree instanceof LambdaExpressionTree expression) {
            properties.add("bodyKind=" + expression.getBodyKind().name());
        }
        if (tree instanceof CaseTree caseTree) {
            properties.add("caseKind=" + caseTree.getCaseKind().name());
        }
        return List.copyOf(properties);
    }

    private static final class AstFingerprint {
        private long state;
        private int nodes;

        private static AstFingerprint from(AstNode root) {
            AstFingerprint fingerprint = new AstFingerprint();
            fingerprint.addNode(root);
            return fingerprint;
        }

        private void addNode(AstNode node) {
            nodes = Math.incrementExact(nodes);
            addString(node.kind());
            addNullableLong(node.start());
            addNullableLong(node.end());
            addNullableString(node.name());
            addStrings(node.modifiers());
            addStrings(node.properties());
            addLong(node.children().size());
            for (AstEdge edge : node.children()) {
                addString(edge.field());
                addNode(edge.node());
            }
        }

        private void addNullableLong(Long value) {
            if (value == null) {
                addLong(0);
            } else {
                addLong(1);
                addLong(value);
            }
        }

        private void addNullableString(String value) {
            if (value == null) {
                addLong(0);
            } else {
                addLong(1);
                addString(value);
            }
        }

        private void addStrings(List<String> values) {
            addLong(values.size());
            for (String value : values) {
                addString(value);
            }
        }

        private void addString(String value) {
            byte[] bytes = value.getBytes(StandardCharsets.UTF_8);
            addLong(bytes.length);
            for (byte valueByte : bytes) {
                addLong(Byte.toUnsignedInt(valueByte));
            }
        }

        private void addLong(long value) {
            state = (state ^ value) * FNV_PRIME;
        }

        private int nodes() {
            return nodes;
        }

        private String hexadecimal() {
            return String.format(Locale.ROOT, "%016x", state);
        }
    }

    private static final class Utf8Offsets {
        private static final long NO_POSITION = Diagnostic.NOPOS;
        private final long[] byUtf16Boundary;

        private Utf8Offsets(String source) {
            byUtf16Boundary = new long[source.length() + 1];
            Arrays.fill(byUtf16Boundary, NO_POSITION);
            long bytes = 0;
            int utf16 = 0;
            byUtf16Boundary[0] = 0;
            while (utf16 < source.length()) {
                int codePoint = source.codePointAt(utf16);
                int codeUnits = Character.charCount(codePoint);
                bytes += utf8Length(codePoint);
                utf16 += codeUnits;
                byUtf16Boundary[utf16] = bytes;
            }
        }

        private Long toUtf8(long utf16) {
            if (utf16 == NO_POSITION || utf16 < 0 || utf16 >= byUtf16Boundary.length) {
                return null;
            }
            long value = byUtf16Boundary[(int) utf16];
            return value == NO_POSITION ? null : value;
        }

        private static int utf8Length(int codePoint) {
            if (codePoint <= 0x7f) {
                return 1;
            }
            if (codePoint <= 0x7ff) {
                return 2;
            }
            if (codePoint <= 0xffff) {
                return 3;
            }
            return 4;
        }
    }

    private static String render(Snapshot snapshot) {
        StringBuilder output = new StringBuilder();
        output.append("{\n");
        fieldName(output, 1, "schema");
        appendString(output, SCHEMA);
        output.append(",\n");
        fieldName(output, 1, "javaRelease");
        output.append(JAVA_RELEASE).append(",\n");
        fieldName(output, 1, "javaRuntimeVersion");
        appendString(output, Runtime.version().toString());
        output.append(",\n");
        fieldName(output, 1, "coordinates");
        appendString(output, "raw-utf8-bytes");
        output.append(",\n");
        fieldName(output, 1, "accepted");
        appendAcceptedCases(output, snapshot.accepted(), 1);
        output.append(",\n");
        fieldName(output, 1, "rejected");
        appendRejectedCases(output, snapshot.rejected(), 1);
        output.append('\n').append("}\n");
        return output.toString();
    }

    private static void appendAcceptedCases(
            StringBuilder output, List<AcceptedCase> cases, int depth) {
        output.append("[\n");
        for (int index = 0; index < cases.size(); index++) {
            AcceptedCase fixture = cases.get(index);
            indent(output, depth + 1);
            output.append("{\n");
            fieldName(output, depth + 2, "id");
            appendString(output, fixture.id());
            output.append(",\n");
            fieldName(output, depth + 2, "sourceName");
            appendString(output, fixture.sourceName());
            output.append(",\n");
            fieldName(output, depth + 2, "source");
            appendString(output, fixture.source());
            output.append(",\n");
            fieldName(output, depth + 2, "tree");
            appendNode(output, fixture.tree(), depth + 2);
            output.append('\n');
            indent(output, depth + 1);
            output.append('}');
            if (index + 1 != cases.size()) {
                output.append(',');
            }
            output.append('\n');
        }
        indent(output, depth);
        output.append(']');
    }

    private static void appendRejectedCases(
            StringBuilder output, List<RejectedCase> cases, int depth) {
        output.append("[\n");
        for (int index = 0; index < cases.size(); index++) {
            RejectedCase fixture = cases.get(index);
            indent(output, depth + 1);
            output.append("{\n");
            fieldName(output, depth + 2, "id");
            appendString(output, fixture.id());
            output.append(",\n");
            fieldName(output, depth + 2, "sourceName");
            appendString(output, fixture.sourceName());
            output.append(",\n");
            fieldName(output, depth + 2, "source");
            appendString(output, fixture.source());
            output.append('\n');
            indent(output, depth + 1);
            output.append('}');
            if (index + 1 != cases.size()) {
                output.append(',');
            }
            output.append('\n');
        }
        indent(output, depth);
        output.append(']');
    }

    private static void appendNode(StringBuilder output, AstNode node, int depth) {
        output.append("{\n");
        fieldName(output, depth + 1, "kind");
        appendString(output, node.kind());
        output.append(",\n");
        fieldName(output, depth + 1, "start");
        appendNullableNumber(output, node.start());
        output.append(",\n");
        fieldName(output, depth + 1, "end");
        appendNullableNumber(output, node.end());
        output.append(",\n");
        fieldName(output, depth + 1, "name");
        appendNullableString(output, node.name());
        output.append(",\n");
        fieldName(output, depth + 1, "modifiers");
        appendStrings(output, node.modifiers());
        output.append(",\n");
        fieldName(output, depth + 1, "properties");
        appendStrings(output, node.properties());
        output.append(",\n");
        fieldName(output, depth + 1, "children");
        output.append("[\n");
        for (int index = 0; index < node.children().size(); index++) {
            AstEdge edge = node.children().get(index);
            indent(output, depth + 2);
            output.append("{\n");
            fieldName(output, depth + 3, "field");
            appendString(output, edge.field());
            output.append(",\n");
            fieldName(output, depth + 3, "node");
            appendNode(output, edge.node(), depth + 3);
            output.append('\n');
            indent(output, depth + 2);
            output.append('}');
            if (index + 1 != node.children().size()) {
                output.append(',');
            }
            output.append('\n');
        }
        indent(output, depth + 1);
        output.append("]\n");
        indent(output, depth);
        output.append('}');
    }

    private static void fieldName(StringBuilder output, int depth, String name) {
        indent(output, depth);
        appendString(output, name);
        output.append(": ");
    }

    private static void indent(StringBuilder output, int depth) {
        output.append("\t".repeat(depth));
    }

    private static void appendNullableNumber(StringBuilder output, Number value) {
        if (value == null) {
            output.append("null");
        } else {
            output.append(value);
        }
    }

    private static void appendNullableString(StringBuilder output, String value) {
        if (value == null) {
            output.append("null");
        } else {
            appendString(output, value);
        }
    }

    private static void appendStrings(StringBuilder output, List<String> values) {
        output.append('[');
        for (int index = 0; index < values.size(); index++) {
            if (index != 0) {
                output.append(", ");
            }
            appendString(output, values.get(index));
        }
        output.append(']');
    }

    private static void appendString(StringBuilder output, String value) {
        output.append('"');
        value.codePoints()
                .forEach(
                        codePoint -> {
                            switch (codePoint) {
                                case '"' -> output.append("\\\"");
                                case '\\' -> output.append("\\\\");
                                case '\b' -> output.append("\\b");
                                case '\f' -> output.append("\\f");
                                case '\n' -> output.append("\\n");
                                case '\r' -> output.append("\\r");
                                case '\t' -> output.append("\\t");
                                default -> {
                                    if (codePoint < 0x20) {
                                        output.append(
                                                String.format(
                                                        Locale.ROOT, "\\u%04x", codePoint));
                                    } else {
                                        output.appendCodePoint(codePoint);
                                    }
                                }
                            }
                        });
        output.append('"');
    }

    private record Config(boolean update, Path root) {
        private static Config parse(String[] arguments) {
            if (arguments.length < 1 || arguments.length > 2) {
                throw usage();
            }
            boolean update =
                    switch (arguments[0]) {
                        case "--check" -> false;
                        case "--update" -> true;
                        default -> throw usage();
                    };
            Path root = arguments.length == 2 ? Path.of(arguments[1]) : DEFAULT_ROOT;
            return new Config(update, root.toAbsolutePath().normalize());
        }

        private static IllegalArgumentException usage() {
            return new IllegalArgumentException(
                    "usage: JavacReference.java (--check|--update) [REFERENCE_ROOT]");
        }
    }

    private record Fixture(String id, String sourceName, Path path, String source) {}

    private record ParsedSource(long errorCount, List<AstNode> units) {}

    private record AstNode(
            String kind,
            Long start,
            Long end,
            String name,
            List<String> modifiers,
            List<String> properties,
            List<AstEdge> children) {}

    private record AstEdge(String field, AstNode node) {}

    private record AcceptedCase(String id, String sourceName, String source, AstNode tree) {}

    private record RejectedCase(String id, String sourceName, String source) {}

    private record Snapshot(List<AcceptedCase> accepted, List<RejectedCase> rejected) {}
}
