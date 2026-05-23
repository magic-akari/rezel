package broken

func run(channel chan int) {
	select {
	case value := <-channel
	}
}
