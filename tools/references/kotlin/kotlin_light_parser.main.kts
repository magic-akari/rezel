@file:OptIn(
    org.jetbrains.kotlin.K1Deprecation::class,
    org.jetbrains.kotlin.config.CompilerConfiguration.Internals::class,
)

import com.intellij.openapi.util.Disposer
import java.nio.charset.StandardCharsets
import java.nio.file.Files
import java.nio.file.Path
import org.jetbrains.kotlin.KtInMemoryTextSourceFile
import org.jetbrains.kotlin.cli.jvm.compiler.EnvironmentConfigFiles
import org.jetbrains.kotlin.cli.jvm.compiler.KotlinCoreEnvironment
import org.jetbrains.kotlin.config.CompilerConfiguration
import org.jetbrains.kotlin.parsing.KotlinLightParser

val expectedKotlinVersion = "2.4.10"
val defaultRoot = Path.of("tools/references/kotlin")

data class Fixture(
    val path: Path,
    val source: String,
)

fun fixtures(directory: Path): List<Fixture> {
    require(Files.isDirectory(directory)) { "fixture directory does not exist: $directory" }
    val paths = mutableListOf<Path>()
    Files.walk(directory).use { walk ->
        walk
            .filter(Files::isRegularFile)
            .filter { path -> path.fileName.toString().endsWith(".kt") }
            .forEach(paths::add)
    }
    paths.sortBy(Path::toString)
    return paths.map { path ->
        val bytes = Files.readAllBytes(path)
        val source = String(bytes, StandardCharsets.UTF_8)
        require(bytes.contentEquals(source.toByteArray(StandardCharsets.UTF_8))) {
            "Kotlin fixture is not canonical UTF-8: $path"
        }
        Fixture(path, source)
    }
}

fun parseErrors(root: Path, fixture: Fixture): List<String> {
    val errors = mutableListOf<String>()
    val relativePath = root.relativize(fixture.path).toString().replace('\\', '/')
    val sourceFile =
        KtInMemoryTextSourceFile(
            fixture.path.fileName.toString(),
            relativePath,
            fixture.source,
        )
    KotlinLightParser.buildLightTree(fixture.source, sourceFile) { start, end, message ->
        errors += "$start..$end: ${message.orEmpty()}"
    }
    return errors
}

val usage = "usage: kotlin_light_parser.main.kts --check [REFERENCE_ROOT]"
require(args.size in 1..2 && args[0] == "--check") { usage }
require(KotlinVersion.CURRENT.toString() == expectedKotlinVersion) {
    "Kotlin $expectedKotlinVersion is required, found ${KotlinVersion.CURRENT}"
}

val root = args.getOrNull(1)?.let(Path::of) ?: defaultRoot
val normalizedRoot = root.toAbsolutePath().normalize()
val accepted = fixtures(normalizedRoot.resolve("fixtures/parse-accepted"))
val rejected = fixtures(normalizedRoot.resolve("fixtures/parse-rejected"))
require(accepted.isNotEmpty() && rejected.isNotEmpty()) {
    "accepted and rejected Kotlin fixtures are both required"
}

val disposable = Disposer.newDisposable("rezel-kotlin-light-parser-reference")
try {
    KotlinCoreEnvironment.createForProduction(
        disposable,
        CompilerConfiguration(),
        EnvironmentConfigFiles.JVM_CONFIG_FILES,
    )
    for (fixture in accepted) {
        val errors = parseErrors(normalizedRoot, fixture)
        require(errors.isEmpty()) {
            "Kotlin light parser rejected ${fixture.path}:\n${errors.joinToString("\n")}"
        }
    }
    for (fixture in rejected) {
        val errors = parseErrors(normalizedRoot, fixture)
        require(errors.isNotEmpty()) {
            "Kotlin light parser unexpectedly accepted ${fixture.path}"
        }
    }
} finally {
    Disposer.dispose(disposable)
}

println("verified ${accepted.size} accepted and ${rejected.size} rejected Kotlin 2.4.10 fixtures")
