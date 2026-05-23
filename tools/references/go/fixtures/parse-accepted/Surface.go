//go:build linux && go1.24

// Package surface exercises the public go/ast syntax model.
package surface

import (
	. "fmt"
	alias "io"
)

const (
	zero           = iota
	one        int = 1
	floatValue     = 1.25
	imaginary      = 2i
	runeValue      = '界'
	rawString      = `raw
string`
)

var (
	values = map[string]int{"one": 1}
	eof    = alias.EOF
)

type Alias[T any] = map[string]T

type Pair[A, B any] struct {
	Left  A
	Right B
	Embedded
}

type Constraint interface {
	~int | ~string
	Compare(Pair[int, string]) bool
}

type Shapes struct {
	Array   [2][3]int
	Slice   []byte
	Map     map[string][]int
	Send    chan<- int
	Receive <-chan int
	Both    chan int
	Pointer *Pair[int, string]
}

type Tagged struct {
	Value string `json:"value"` // field comment
}

func AssemblyStub(value int) int

func Transform[T any, R comparable](value T, function func(T) R) R {
	return function(value)
}

func variadic(values ...int) {}

func (pair Pair[A, B]) First() A {
	return pair.Left
}

func surface(channel chan int, input any, items []int) (result int) {
	var local = map[string]int{"one": 1}
	const localConstant = 1
	_ = local
	_ = localConstant

	function := func(value int) int { return value }
	result = function(1)
	pointer := new(Pair[int, string])
	_ = (*pointer).Left
	_ = []Pair[int, string]{{Left: 1, Right: "one"}, {2, "two"}}
	_ = items[0]
	_ = items[1:2]
	_ = items[1:2:cap(items)]
	_ = input.(Pair[int, string])
	_ = Transform[int, string](1, func(int) string { return "" })
	_ = Transform[int, string]
	_ = -items[0] + 3*4<<1&^2
	_ = &items[0]
	_ = <-channel
	variadic(items...)
	Println(eof)

	result = (result)
	channel <- result

outer:
	for index := 0; index < len(items); index++ {
		if value := items[index]; value < 0 {
			continue outer
		} else {
			result += value
		}
	}
	for _, value := range items {
		result += value
	}
	var index int
	for index = range items {
		result += index
	}
	for range 3 {
		break
	}

	switch current := result; current {
	case 0:
		fallthrough
	default:
		result++
	}
	switch current := input; typed := current.(type) {
	case int, string:
		_ = typed
	default:
	}
	select {
	case channel <- result:
	case received := <-channel:
		result = received
	default:
	}

	operators(result, true, channel)
	go variadic(result)
	defer variadic(result)
	variadic(result)
empty:
	;
	goto done

done:
	return result

implicit:
}

func operators(value int, flag bool, channel chan int) {
	_ = +value
	_ = -value
	_ = !flag
	_ = ^value
	_ = &value
	_ = <-channel

	_ = value + value - value | value ^ value
	_ = value * value / value % value << value >> value & value &^ value
	_ = value == value || value != value && value < value
	_ = value <= value || value > value || value >= value

	value += 1
	value -= 1
	value *= 1
	value /= 1
	value %= 1
	value &= 1
	value |= 1
	value ^= 1
	value <<= 1
	value >>= 1
	value &^= 1
	value--
}
