<?php

declare(strict_types=1);

const EXPECTED_PHP_VERSION = '8.5.9';
const EXPECTED_SECTIONS = 5301;
const EXPECTED_BYTES = 1839044;
const EXPECTED_NON_UTF8 = 5;
const EXPECTED_ACCEPTED = 5162;
const EXPECTED_REJECTED = 139;

if ($argc !== 3) {
    fwrite(STDERR, "usage: php oracle.php PHP_SRC OUTPUT\n");
    exit(2);
}
if (PHP_VERSION !== EXPECTED_PHP_VERSION) {
    throw new RuntimeException('expected PHP ' . EXPECTED_PHP_VERSION . ', found ' . PHP_VERSION);
}

$root = realpath($argv[1]);
if ($root === false) {
    throw new RuntimeException("php-src root does not exist: {$argv[1]}");
}
$paths = [];
$iterator = new RecursiveIteratorIterator(
    new RecursiveDirectoryIterator($root . '/Zend/tests'),
);
foreach ($iterator as $file) {
    if ($file->isFile() && $file->getExtension() === 'phpt') {
        $paths[] = $file->getPathname();
    }
}
sort($paths, SORT_STRING);

$records = [];
$accepted = 0;
$rejected = 0;
$nonUtf8 = 0;
$sections = 0;
$bytes = 0;
foreach ($paths as $path) {
    $test = file_get_contents($path);
    if ($test === false) {
        throw new RuntimeException("failed to read {$path}");
    }
    if (preg_match('//u', $test) !== 1) {
        ++$nonUtf8;
        continue;
    }
    $source = fileSection($test);
    if ($source === null) {
        continue;
    }

    ++$sections;
    $bytes += strlen($source);
    $relative = substr($path, strlen($root) + 1);
    $status = 'accepted';
    $error = null;
    set_error_handler(static fn (): bool => true);
    try {
        @token_get_all($source, TOKEN_PARSE);
        ++$accepted;
    } catch (Throwable $exception) {
        $status = 'rejected';
        $error = get_class($exception) . ': ' . $exception->getMessage();
        ++$rejected;
    } finally {
        restore_error_handler();
    }
    $records[] = [
        'path' => $relative,
        'status' => $status,
        'bytes' => strlen($source),
        'sha256' => hash('sha256', $source),
        'error' => $error,
    ];
}

$actual = [$sections, $bytes, $nonUtf8, $accepted, $rejected];
$expected = [
    EXPECTED_SECTIONS,
    EXPECTED_BYTES,
    EXPECTED_NON_UTF8,
    EXPECTED_ACCEPTED,
    EXPECTED_REJECTED,
];
if ($actual !== $expected) {
    throw new RuntimeException(
        'pinned oracle inventory changed: expected ' . json_encode($expected) .
        ', found ' . json_encode($actual),
    );
}

$report = [
    'php_version' => PHP_VERSION,
    'sections' => $sections,
    'bytes' => $bytes,
    'non_utf8' => $nonUtf8,
    'accepted' => $accepted,
    'rejected' => $rejected,
    'records' => $records,
];
$json = json_encode($report, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_THROW_ON_ERROR);
if (file_put_contents($argv[2], $json . "\n") === false) {
    throw new RuntimeException("failed to write {$argv[2]}");
}
fwrite(
    STDERR,
    "PHP " . PHP_VERSION . ": sections={$sections} bytes={$bytes} non_utf8={$nonUtf8} " .
    "accepted={$accepted} rejected={$rejected}\n",
);

function fileSection(string $test): ?string
{
    foreach (["\n--FILE--\n", "\n--FILEEOF--\n"] as $marker) {
        $markerStart = strpos($test, $marker);
        if ($markerStart === false) {
            continue;
        }
        $start = $markerStart + strlen($marker);
        $rest = substr($test, $start);
        $sectionEnd = strpos($rest, "\n--");
        return $sectionEnd === false ? $rest : substr($rest, 0, $sectionEnd);
    }
    return null;
}
