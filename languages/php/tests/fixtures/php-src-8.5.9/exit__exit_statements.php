<?php

const FILE_PATH = __DIR__ . '/exit_statements.php';
const FILE_CONTENT = <<<'TEMPLATE'
<?php
echo "Before FUNCTION";
try {
    FUNCTION;
} catch (\Throwable $e) {
    echo $e::class, ': ', $e->getMessage(), PHP_EOL;
}

TEMPLATE;


$php = getenv('TEST_PHP_EXECUTABLE_ESCAPED');
$command = $php . ' ' . escapeshellarg(FILE_PATH);

foreach (['exit', 'die'] as $value) {
    echo 'Using ', $value, ' as value:', PHP_EOL;
    $output = [];
    $content = str_replace('FUNCTION', $value, FILE_CONTENT);
    file_put_contents(FILE_PATH, $content);
    exec($command, $output, $exit_status);
    echo 'Exit status is: ', $exit_status, PHP_EOL,
         'Output is:', PHP_EOL, join($output), PHP_EOL;
}

?>

