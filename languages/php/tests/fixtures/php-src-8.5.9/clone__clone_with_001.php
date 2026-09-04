<?php

class Dummy { }

$x = new stdClass();

$foo = 'FOO';
$bar = new Dummy();
$array = [
	'baz' => 'BAZ',
	'array' => [1, 2, 3],
];

var_dump(clone $x);
var_dump(clone($x));
var_dump(clone($x, [ 'foo' => $foo, 'bar' => $bar ]));
var_dump(clone($x, $array));
var_dump(clone($x, [ 'obj' => $x ]));

var_dump(clone($x, [
	'abc',
	'def',
	new Dummy(),
	'named' => 'value',
]));

?>

