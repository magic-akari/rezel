<?php

class Test {
    public string $prop {
        set(string|array $prop) {
            $this->prop = is_array($prop) ? join(', ', $prop) : $prop;
        }
    }
}

$test = new Test();
var_dump($test->prop = 'prop');
var_dump($test->prop = ['prop1', 'prop2']);
var_dump($test->prop);

?>

