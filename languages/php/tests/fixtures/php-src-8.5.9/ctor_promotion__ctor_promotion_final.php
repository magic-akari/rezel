<?php

class Demo {
    public function __construct(
        final string $foo,
        final public string $bar,
    ) {}
}

$d = new Demo("first", "second");
var_dump($d);

?>

