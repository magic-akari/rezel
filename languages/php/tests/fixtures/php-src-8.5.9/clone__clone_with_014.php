<?php

readonly class Test {
    public public(set) int $a;
    public public(set) int $b;

    public function __construct() {
        $this->a = 1;
        $this->b = 2;
    }
}

$test = clone(new Test(), ['a' => 3]);
var_dump($test);

try {
    $test->b = 4;
} catch (Error $e) {
    echo $e::class, ": ", $e->getMessage(), PHP_EOL;
}

var_dump($test);

?>

