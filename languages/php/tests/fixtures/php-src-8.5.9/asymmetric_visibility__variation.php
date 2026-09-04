<?php

class Test {
    public private(set) int $prop;
    public private(set) array $array;

    public function init() {
        $this->prop = 1;
        $this->array = [];
    }

    public function r() {
        echo $this->prop;
    }

    public function w() {
        $this->prop = 1;
        echo 'done';
    }

    public function rw() {
        $this->prop += 1;
        echo 'done';
    }

    public function im() {
        $this->array[] = 1;
        echo 'done';
    }

    public function is() {
        echo (int) isset($this->prop);
    }

    public function us() {
        unset($this->prop);
        echo 'done';
    }

    public function us_dim() {
        unset($this->array[0]);
        echo 'done';
    }
}

function r($test) {
    echo $test->prop;
}

function w($test) {
    $test->prop = 0;
    echo 'done';
}

function rw($test) {
    $test->prop += 1;
    echo 'done';
}

function im($test) {
    $test->array[] = 1;
    echo 'done';
}

function is($test) {
    echo (int) isset($test->prop);
}

function us($test) {
    unset($test->prop);
    echo 'done';
}

function us_dim($test) {
    unset($test->array[0]);
    echo 'done';
}

foreach ([true, false] as $init) {
    foreach ([true, false] as $scope) {
        foreach (['r', 'w', 'rw', 'im', 'is', 'us', 'us_dim'] as $op) {
            $test = new Test();
            if ($init) {
                $test->init();
            }

            echo 'Init: ' . ((int) $init) . ', scope: ' . ((int) $scope) . ', op: ' . $op . ": ";
            try {
                if ($scope) {
                    $test->{$op}();
                } else {
                    $op($test);
                }
            } catch (Error $e) {
                echo $e->getMessage();
            }
            echo "\n";
        }
    }
}

?>

