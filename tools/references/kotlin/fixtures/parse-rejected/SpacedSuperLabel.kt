interface K {
    fun value(): String
}

class A : K {
    override fun value() = "A"

    inner class B : K {
        override fun value() = "B"

        fun broken() = super @A.value()
    }
}
