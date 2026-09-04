<?php

try {
    assert(false && new class {
        public function __construct( #[Foo] public private(set) bool $boolVal = false { final set => $this->boolVal = 1;} ) {}
    });
} catch (Error $e) {
    echo $e->getMessage(), "\n";
}

?>

